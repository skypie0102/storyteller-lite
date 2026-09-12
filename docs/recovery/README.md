# Recovery source index

This directory preserves the product/history material used to rebuild Storyteller OneClick Lite after the Rust + Slint branch was lost/recovered.

## Authority order

When sources disagree, use this order:

1. **Current source code and tests on `recovery/rust-slint`** — technical truth about what exists now.
2. **`docs/ROADMAP.md` and `docs/HANDOFF.md`** — current product decisions and pending work.
3. **`docs/ui-guides/` mockups** — visual/product layout references.
4. **`LITE_PLANNING_HISTORY.txt`** — historical planning/implementation transcript. It can contain claims about branches/commits that are no longer present; treat those as recovery clues, not proof of current code.
5. **Storyteller OneClick v0.39.0 installer** — behavioral reference only. It predates the Rust + Slint refactor and is not the Lite codebase.

## Preserved materials

### Lite planning transcript

- Repository file: `docs/recovery/LITE_PLANNING_HISTORY.txt`
- Source SHA-256: `8c3784ff4ebd7ec0b54993cc72bab54e11a019d6f10666d66d34d8e127856a0c`
- Purpose: preserve the recovered sequence of Lite scope decisions and prior implementation claims so another agent can understand the intended direction.

Important discrepancy: the transcript repeatedly refers to `refactor/rust-slint` and historical commit SHAs. The live repository branch recovered in this chat is `recovery/rust-slint`, head `56a48b4142caa89f49cd9020bc537c698932d57c` at the time this packet was created. Always inspect the current repository before assuming a historical commit still exists.

### UI mockups

See `docs/ui-guides/README.md` and the two WebP files stored next to it. They are the current layout references for the main screen and the reduced manual-audio-allocation experience.

### Old installer

See `LEGACY_INSTALLER_REFERENCE.md`.

## Recovery principle

The pre-Rust application is a **behavioral regression reference, not a porting target**. Lite intentionally cuts features. The migration rule remains:

> Replace → regression-test → delete. Never delete → hope we remembered everything.

Do not reintroduce historical features simply because they are discoverable in the installer or planning transcript. Only restore features that are explicitly part of the current Lite roadmap.
