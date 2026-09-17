from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}: {old!r}")
    file_path.write_text(text.replace(old, new), encoding="utf-8")


handoff = "docs/HANDOFF.md"
old_handoff = "- The first P5 interoperability slice is integrated as `ef7900ceb5efd3541d8b33f056d3f3ff8d9920e8`. It adds standards-clean representative EPUB fixtures, a manual EPUBCheck 5.4.0 gate, required Media Overlay `epub:textref` output, and matching internal-validator enforcement. Final interoperability run `35065877026` passed rustfmt, strict core Clippy, core tests, fixture export, pinned EPUBCheck checksum verification, and EPUBCheck on text-overlay, supplemental Introduction/Credits, and Graphic Readout outputs.\n"
new_handoff = old_handoff + (
    "- A first real reading-system interoperability gate is integrated as `d508c9d0a818835e75ace7ad6384c2eeb2efe9de` plus `46ad99931b0c5eccc8f70fe8e4b79b11e2ea9409`. The manual-only Thorium workflow pins Thorium Reader 3.5.1 for Linux amd64 to SHA-256 `72ab951d4963500b68c91a6496c662f30698dc27422a1b1af720b1cfbdfd327e`, exports the same text/supplemental/Graphic Readout fixtures, and opens each in an isolated Xvfb-backed Thorium profile. Validation run `35220207088` passed all three cases, requiring the real reader process to remain alive and to persist an imported publication. This is deliberately import/open evidence only; it does not claim automated audible Media Overlay playback or synchronized-highlight verification.\n"
)
replace_exact(handoff, old_handoff, new_handoff)

roadmap = "docs/ROADMAP.md"
old_boundary = "The first external run exposed a real conformance defect: Storyteller-generated SMIL `<seq>` elements lacked required `epub:textref`. All text, supplemental, and mixed Graphic Readout generation paths now emit the required reference, and the internal validator independently requires/resolves it so the same class of defect is caught before publication. The final run passed EPUBCheck 5.4.0 on all three fixtures.\n\n### Validated checkpoint resume and relaunch recovery — implemented\n"
new_boundary = "The first external run exposed a real conformance defect: Storyteller-generated SMIL `<seq>` elements lacked required `epub:textref`. All text, supplemental, and mixed Graphic Readout generation paths now emit the required reference, and the internal validator independently requires/resolves it so the same class of defect is caught before publication. The final run passed EPUBCheck 5.4.0 on all three fixtures.\n\n### Thorium reading-system import/open baseline — implemented\n\nA manual-only `.github/workflows/thorium-validation.yml` gate now exercises the same three exported fixtures in a real EPUB reading system. It downloads the official Thorium Reader 3.5.1 Linux amd64 Debian package and verifies SHA-256 `72ab951d4963500b68c91a6496c662f30698dc27422a1b1af720b1cfbdfd327e`, installs Thorium plus Xvfb, and runs `.github/scripts/thorium-reader-smoke.sh` with a fresh isolated profile per fixture.\n\nThe smoke requires Thorium to remain alive through the observation window and to persist an imported publication for each text-overlay, supplemental-edge, and Graphic Readout fixture. Validation run `35220207088` passed all three cases. This is intentionally narrower than playback validation: it proves real-reader import/open compatibility, but it does **not** claim automated audible Media Overlay playback, timing, or synchronized-highlight verification. Those remain manual/reader-specific evidence unless a reliable automation surface is introduced later.\n\n### Validated checkpoint resume and relaunch recovery — implemented\n"
replace_exact(roadmap, old_boundary, new_boundary)

old_next = "- exercise the exported text, supplemental, and Graphic Readout books in representative reading systems where automation or reproducible fixture evidence is practical, recording reader-specific limitations separately from EPUBCheck conformance;\n"
new_next = "- keep the pinned Thorium 3.5.1 import/open smoke as reproducible real-reader regression coverage; add manual playback/highlighting evidence or a second independent reading system only when it can be recorded honestly and repeatably, and keep reader-specific limitations separate from EPUBCheck conformance;\n"
replace_exact(roadmap, old_next, new_next)

for path in [handoff, roadmap]:
    text = Path(path).read_text(encoding="utf-8")
    required = [
        "35220207088",
        "72ab951d4963500b68c91a6496c662f30698dc27422a1b1af720b1cfbdfd327e",
        "import/open",
    ]
    for marker in required:
        if marker not in text:
            raise SystemExit(f"{path}: missing Thorium checkpoint marker {marker!r}")

print("P5 Thorium interoperability documentation checkpoint applied")
