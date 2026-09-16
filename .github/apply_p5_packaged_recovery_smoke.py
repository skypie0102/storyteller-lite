from pathlib import Path

path = Path("crates/storyteller-ui/src/main.rs")
text = path.read_text(encoding="utf-8")

replacements = [
    (
        "mod app_paths;\nmod review_ui;\n",
        "mod app_paths;\nmod recovery_smoke;\nmod review_ui;\n",
    ),
    (
        "fn main() -> Result<(), slint::PlatformError> {\n    let ui = AppWindow::new()?;\n",
        "fn main() -> Result<(), slint::PlatformError> {\n    if std::env::args_os().any(|argument| argument == \"--recovery-smoke\") {\n        if let Err(error) = recovery_smoke::run() {\n            eprintln!(\"Packaged recovery smoke failed: {error}\");\n            std::process::exit(2);\n        }\n        return Ok(());\n    }\n\n    let ui = AppWindow::new()?;\n",
    ),
]

for old, new in replacements:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected exactly one main.rs match, found {count}: {old!r}")
    text = text.replace(old, new, 1)

path.write_text(text, encoding="utf-8")
