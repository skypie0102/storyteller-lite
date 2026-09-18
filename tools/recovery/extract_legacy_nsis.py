#!/usr/bin/env python3
"""Static extractor for the preserved Storyteller One-Click v0.39.0 installer.

This does NOT execute the Windows installer. It validates the exact known
installer SHA-256, locates the NSIS first header, decompresses the solid LZMA
payload with Python's standard library, and enumerates the size-prefixed data
blocks used by this specific build.

It is intentionally a recovery tool, not a general NSIS implementation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import lzma
from pathlib import Path

EXPECTED_SHA256 = "417bce5a6e95bfac497bde4b1bbe48c3fb3ec7834909d0195f93741e9acf8eb9"
NSIS_SIGNATURE = b"\xef\xbe\xad\xdeNullsoftInst"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def decode_lzma_properties(prop: int) -> tuple[int, int, int]:
    lc = prop % 9
    remainder = prop // 9
    lp = remainder % 5
    pb = remainder // 5
    if pb > 4:
        raise ValueError(f"Unsupported LZMA property byte: 0x{prop:02x}")
    return lc, lp, pb


def decompress_installer(path: Path) -> tuple[bytes, int, int, int]:
    blob = path.read_bytes()
    signature_at = blob.find(NSIS_SIGNATURE)
    if signature_at < 4:
        raise ValueError("NSIS first-header signature was not found.")
    first_header_at = signature_at - 4
    first_header = blob[first_header_at:first_header_at + 28]
    if len(first_header) != 28:
        raise ValueError("Truncated NSIS first header.")

    header_length = int.from_bytes(first_header[20:24], "little")
    archive_size = int.from_bytes(first_header[24:28], "little")
    stream_at = first_header_at + 28
    if archive_size <= 33 or first_header_at + archive_size > len(blob):
        raise ValueError("Invalid NSIS archive size.")

    prop = blob[stream_at]
    dictionary_size = int.from_bytes(blob[stream_at + 1:stream_at + 5], "little")
    lc, lp, pb = decode_lzma_properties(prop)
    compressed = blob[stream_at + 5:first_header_at + archive_size]
    decoder = lzma.LZMADecompressor(
        format=lzma.FORMAT_RAW,
        filters=[{
            "id": lzma.FILTER_LZMA1,
            "dict_size": dictionary_size,
            "lc": lc,
            "lp": lp,
            "pb": pb,
        }],
    )
    decoded = decoder.decompress(compressed)
    if not decoder.eof:
        raise ValueError("The solid LZMA stream did not reach its end marker.")
    if len(decoded) < header_length + 8:
        raise ValueError("Decompressed NSIS stream is shorter than its header.")
    if int.from_bytes(decoded[:4], "little") != header_length:
        raise ValueError("Compiled-header length does not match the NSIS first header.")
    return decoded, header_length, first_header_at, archive_size


def guess_extension(data: bytes) -> str:
    if data.startswith(b"MZ"):
        return "exe"
    if data.startswith(b"BM"):
        return "bmp"
    if data.startswith(b"PK\x03\x04"):
        return "zip"
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return "png"
    if data.startswith(b"#!"):
        return "txt"
    stripped = data.lstrip()
    if stripped.startswith((b"#", b"The ", b"{", b"[")):
        return "txt"
    return "bin"


def enumerate_blocks(decoded: bytes, header_length: int) -> list[dict[str, object]]:
    # This exact v0.39.0 build has one 32-bit value immediately after the
    # compiled header, followed by size-prefixed data blocks. The recovery
    # notes document the observed value (31) and the verified block hashes.
    marker_at = header_length
    marker = int.from_bytes(decoded[marker_at:marker_at + 4], "little")
    if marker != 31:
        raise ValueError(f"Unexpected post-header marker: {marker}; expected 31.")

    position = header_length + 4
    blocks: list[dict[str, object]] = []
    while position < len(decoded):
        if position + 4 > len(decoded):
            raise ValueError("Truncated block length at end of stream.")
        encoded_size = int.from_bytes(decoded[position:position + 4], "little")
        size = encoded_size & 0x7FFF_FFFF
        data_at = position + 4
        end = data_at + size
        if end > len(decoded):
            raise ValueError(f"Block {len(blocks)} extends beyond the decompressed stream.")
        data = decoded[data_at:end]
        blocks.append({
            "index": len(blocks),
            "prefix_offset": position,
            "data_offset": data_at,
            "size": size,
            "high_bit": bool(encoded_size & 0x8000_0000),
            "extension": guess_extension(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })
        position = end
    return blocks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("installer", type=Path)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--force", action="store_true", help="allow an installer with a different SHA-256")
    args = parser.parse_args()

    actual_sha = sha256(args.installer)
    if actual_sha != EXPECTED_SHA256 and not args.force:
        raise SystemExit(
            "Refusing to parse an unknown installer. "
            f"Expected {EXPECTED_SHA256}, found {actual_sha}. Use --force only for deliberate research."
        )

    decoded, header_length, first_header_at, archive_size = decompress_installer(args.installer)
    blocks = enumerate_blocks(decoded, header_length)
    summary = {
        "installer_sha256": actual_sha,
        "first_header_offset": first_header_at,
        "header_length": header_length,
        "archive_size": archive_size,
        "decompressed_size": len(decoded),
        "block_count": len(blocks),
        "blocks": blocks,
    }

    if args.output_dir:
        args.output_dir.mkdir(parents=True, exist_ok=True)
        for block in blocks:
            start = int(block["data_offset"])
            size = int(block["size"])
            suffix = str(block["extension"])
            (args.output_dir / f"{int(block['index']):04d}.{suffix}").write_bytes(decoded[start:start + size])
        (args.output_dir / "index.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    else:
        print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
