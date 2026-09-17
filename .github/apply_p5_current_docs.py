from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}: {old!r}")
    file_path.write_text(text.replace(old, new), encoding="utf-8")


handoff = "docs/HANDOFF.md"
replace_exact(
    handoff,
    "- Temporary validation PRs #1 through #25 are closed without merge. Validated product/docs changes are copied into `recovery/rust-slint`; temporary validation workflows, patch helpers, and merge commits are not treated as implementation truth.\n",
    "- All temporary validation PRs through this checkpoint are closed without merge. Validated product/docs changes are copied into `recovery/rust-slint`; temporary validation workflows, patch helpers, and merge commits are not treated as implementation truth.\n",
)

roadmap = "docs/ROADMAP.md"
replace_exact(
    roadmap,
    "Temporary validation PRs #1 through #25 are closed without merge; validated changes live on `recovery/rust-slint`, while validation-only workflows/helpers remain off the implementation branch.\n",
    "All temporary validation PRs through this checkpoint are closed without merge; validated changes live on `recovery/rust-slint`, while validation-only workflows/helpers remain off the implementation branch.\n",
)

for path in [handoff, roadmap]:
    text = Path(path).read_text(encoding="utf-8")
    if "temporary validation PRs through this checkpoint are closed without merge" not in text.lower():
        raise SystemExit(f"{path}: non-recursive validation checkpoint wording was not written")

print("P5 documentation checkpoint wording updated")
