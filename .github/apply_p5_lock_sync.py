from pathlib import Path

path = Path("Cargo.lock")
data = path.read_bytes()

old_lines = [
    '[[package]]',
    'name = "storyteller-ui"',
    'version = "0.1.0"',
    'dependencies = [',
    ' "rfd",',
    ' "slint",',
    ' "slint-build",',
    ' "storyteller-core",',
    ']',
    '',
]
new_lines = [
    '[[package]]',
    'name = "storyteller-ui"',
    'version = "0.1.0"',
    'dependencies = [',
    ' "rfd",',
    ' "serde",',
    ' "serde_json",',
    ' "slint",',
    ' "slint-build",',
    ' "storyteller-core",',
    ']',
    '',
]

for newline in (b"\n", b"\r\n"):
    old = newline.join(line.encode("utf-8") for line in old_lines)
    count = data.count(old)
    if count == 1:
        new = newline.join(line.encode("utf-8") for line in new_lines)
        path.write_bytes(data.replace(old, new))
        break
    if count > 1:
        raise SystemExit(f"Cargo.lock storyteller-ui entry matched {count} times")
else:
    raise SystemExit("Cargo.lock storyteller-ui entry did not match the expected stale shape")
