# Native UI rebuild

StoryTeller Lite uses one native Rust + Slint window, with New book, Queue and Settings workspaces. English-only Whistle remains the transcription engine.

## Book creation

Choose an EPUB and its matching audiobook with keyboard-accessible native buttons. The output filename and folder appear before starting. Change folder selects a destination without changing either source. Existing outputs are preserved: a new book cannot start with an occupied destination.

The app scans local tools in the background on startup. Start book requires both sources and ready tools. Local setup takes you to Settings for user-initiated verified downloads. While another book is active, the same action adds a waiting book. Successful submission opens Queue and clears the source selection.

Processing preferences are in Settings rather than repeated beside every book. Defaults remain Opus/64K, Smart review, English and one Whistle CPU worker. Preferences apply to newly queued books.

## Processing and queue

The active card shows actual progress, current activity, processed audio and available timing. All seven stage labels have a stable vertical order. No estimated transcription progress is fabricated. The adjacent list shows waiting books followed by the three latest outcomes, with reorder, retry, remove and Open output folder actions when applicable.

Pause after book allows the current book to finish. Resume queue continues paused work. Cancel first explains the consequences; the final command identifies the displayed book, so an old confirmation cannot cancel a newly active book. Removing a queue row does not delete its source or output.

## Audio review

Review opens on Queue when a book needs decisions. Playback, previous/next and seek controls remain above the scrollable content; exclusion and completion actions remain below it. The transcript and nearby EPUB matches have separate scroll areas so a long passage does not bury the first available match.

Text similarity is a lexical-overlap score, not a probability that the match is correct. Review preserves existing text/image assignment, introduction/credits preservation and per-segment exclusion. Decisions save immediately and advance to the first undecided segment. Finish review remains disabled until the durable report is complete. Exclude remaining requires a second click explaining that excluded audio has no synchronized text.

An application-owned worker loads review reports and caches alignment/corpus evidence per book. A single cancellable request stream coalesces navigation; generation and job identity checks discard stale results. Seeking uses the cached selected segment. Candidate loading and file/ZIP reads no longer run on the UI thread. Evidence failures remain visible and do not silently complete a review. Reload review can recover a repaired report.

Audio preview has a separate application-owned worker. It owns one ffplay child, detects natural exit and handles process spawn, kill and wait off the UI thread. Changing segments, saving a decision, leaving review, cancellation and shutdown stop preview. Missing ffplay leaves manual decisions available and displays a useful error.

## Visual and interaction validation

The minimum content size is 820×620; the default is 1040×760. Settings, long queues, long review text and status details scroll locally. Navigation stays available during dependency setup.

The shared fixtures in [tools/ui/fixtures.json](../tools/ui/fixtures.json) cover empty setup, missing tools, selected long filenames, processing, cancellation, paused recovery, completed/failed books, error details, pending/completed/loading review, bulk exclusion and ready/missing Settings.

Run the actual compiled Slint scenes without a desktop session:

```text
cargo run --locked -p storyteller-ui --example ui_snapshot -- tools/ui/fixtures.json ui-snapshots
```

The harness uses Slint 1.17.1's software renderer to capture 39 PPM images: thirteen scenes at 820×620 and 1040×760, plus compact scenes at 200% scale. It also dispatches real pointer and keyboard events to check source selection, start guards, workspace navigation, review completion and the two-click bulk exclusion path. Fixtures have no processing/download/file-dialog callbacks.

Windows checkpoint [37199779738](https://github.com/skypie0102/storyteller-lite/actions/runs/37199779738) passed on source commit `47e74e944fbdcd691c2956050ed26ae4c961f51f`: formatting, strict locked workspace Clippy, all 164 workspace tests, native Slint build, all 39 scene renders, pointer/keyboard checks at both normal sizes, and packaged relaunch recovery. Tab followed by Space activates the source picker; native Fluent buttons deliberately do not take keyboard focus from a pointer click.

All 39 renders were inspected at compact, standard and 200% scale. The screenshots below are lossless conversions of actual Windows fixture renders, with sample book data. [Screenshot provenance](ui-snapshots/provenance.json) records the tested source, artifact digest, dimensions and per-scene hashes; [the 200% review render](ui-snapshots/audio-review-200-percent.png) is also retained. The temporary branch-only checkpoint workflow was removed after the pass. No runtime dependencies or lockfile changes were needed.

### New book — compact window

![Output preview and selected sources at 820×620](ui-snapshots/new-book.png)

### Processing — compact window

![Seven processing stages beside the queue at 820×620](ui-snapshots/processing.png)

### Audio review — compact window

![Transcript and nearby EPUB matches at 820×620](ui-snapshots/audio-review.png)

### Settings — standard window

![Processing preferences and local setup at 1040×760](ui-snapshots/settings.png)

These scene and interaction checks do not establish full-book recognition accuracy, audible preview quality, reader interoperability or full release packaging; those remain R5 acceptance work.
