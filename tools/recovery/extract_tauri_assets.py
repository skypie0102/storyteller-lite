#!/usr/bin/env python3
"""Recover v0.39.0 Storyteller One-Click Tauri web assets without running it.

This is intentionally hash-locked to the exact desktop executable recovered from
Storyteller-OneClick-v0.39.0-Windows-x64-Setup.exe. The executable stores each
known web asset as the UTF-8 resource path immediately followed by a Brotli
stream.

Requires the Python ``brotli`` package. This is a forensic/recovery helper only;
it is not part of the Storyteller Lite runtime.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import sys

EXPECTED_EXE_SHA256 = "fbfb10664dd127227bf95e881c2f30125d841baaa83a64518c848adcce38dbb1"
ASSETS = {
    "/index.html": (453, "d5d58c4016fe542216cf4cfeb0621a5853bef897cabc22693585956a2a19e1fe"),
    "/assets/index-CudKkaga.js": (325836, "d25d9bc9eb4dd4ebef49bdc90ae342c44fc29c11a0f7fa670c3c9aa5a0633842"),
    "/assets/index-DJf4moyJ.css": (12779, "4463b66cd7c0d20e32ba8deb994c7c05731590f1e6edd84ad499b97eda9d189d"),
    "/assets/main-current-CPAuPPOe.css": (59741, "1f5e375c7a776859396d4761828834c0c4a5eeb791f97e08a6aa36470ec6762f"),
    "/assets/allocator-current-CszKz2tT.css": (56489, "d40448d5e6152854f6dc69aaeb606c3c04554a8dc26d3d3ccf9db4460d185b07"),
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def marker_offsets(blob: bytes, marker: bytes) -> list[int]:
    offsets: list[int] = []
    cursor = 0
    while True:
        offset = blob.find(marker, cursor)
        if offset < 0:
            return offsets
        offsets.append(offset)
        cursor = offset + 1


def try_recover_stream(blob: bytes, marker: bytes, offset: int) -> tuple[bytes, int] | None:
    try:
        import brotli
    except ImportError as exc:  # pragma: no cover - recovery environment dependent
        raise RuntimeError(
            "Python package 'brotli' is required for Tauri asset recovery. "
            "Install it in an isolated recovery environment."
        ) from exc

    cursor = offset + len(marker)
    decoder = brotli.Decompressor()
    output = bytearray()
    try:
        # Byte-wise feeding makes the exact end of each independent Brotli
        # stream deterministic even though unrelated binary data follows it.
        while cursor < len(blob) and not decoder.is_finished():
            output.extend(decoder.process(blob[cursor : cursor + 1]))
            cursor += 1
    except brotli.error:
        return None
    if not decoder.is_finished():
        return None
    return bytes(output), cursor


def recover_verified_stream(
    blob: bytes, marker: bytes, expected_size: int, expected_hash: str
) -> tuple[bytes, int, int]:
    offsets = marker_offsets(blob, marker)
    if not offsets:
        raise ValueError(f"Embedded asset marker not found: {marker.decode()}")

    matches: list[tuple[bytes, int, int]] = []
    for offset in offsets:
        recovered = try_recover_stream(blob, marker, offset)
        if recovered is None:
            continue
        output, end_offset = recovered
        if len(output) == expected_size and sha256(output) == expected_hash:
            matches.append((output, offset, end_offset))

    if len(matches) != 1:
        raise ValueError(
            f"Expected exactly one verified Brotli stream for {marker.decode()}, "
            f"found {len(matches)} among {len(offsets)} marker occurrence(s)."
        )
    return matches[0]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "desktop_exe",
        type=Path,
        help="Recovered v0.39.0 desktop executable (NSIS block 7)",
    )
    parser.add_argument("output_dir", type=Path, help="Directory for recovered web assets")
    args = parser.parse_args()

    blob = args.desktop_exe.read_bytes()
    actual = sha256(blob)
    if actual != EXPECTED_EXE_SHA256:
        parser.error(
            "desktop executable SHA-256 mismatch; refusing version-specific extraction\n"
            f"expected: {EXPECTED_EXE_SHA256}\nactual:   {actual}"
        )

    args.output_dir.mkdir(parents=True, exist_ok=True)
    for resource, (expected_size, expected_hash) in ASSETS.items():
        recovered, marker_offset, end_offset = recover_verified_stream(
            blob, resource.encode("utf-8"), expected_size, expected_hash
        )
        actual_hash = sha256(recovered)
        if len(recovered) != expected_size or actual_hash != expected_hash:
            raise SystemExit(
                f"Recovered asset verification failed for {resource}: "
                f"size={len(recovered)} sha256={actual_hash}"
            )
        destination = args.output_dir / resource.lstrip("/")
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(recovered)
        compressed_size = end_offset - (marker_offset + len(resource.encode("utf-8")))
        print(
            f"{resource}: {len(recovered)} bytes, compressed={compressed_size}, "
            f"marker=0x{marker_offset:x}, sha256={actual_hash}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
