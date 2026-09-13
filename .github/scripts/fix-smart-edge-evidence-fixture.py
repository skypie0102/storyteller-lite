from pathlib import Path

path = Path("crates/storyteller-core/src/review_assignment.rs")
text = path.read_text(encoding="utf-8")
old = '''                transcript_text: "bravo middle".into(),
                suggestion: None,
                decision,
'''
new = '''                transcript_text: "bravo middle".into(),
                suggestion: None,
                edge: None,
                silence: None,
                decision,
'''
count = text.count(old)
if count != 1:
    raise RuntimeError(f"review assignment fixture: expected one match, found {count}")
path.write_text(text.replace(old, new, 1), encoding="utf-8", newline="\n")
