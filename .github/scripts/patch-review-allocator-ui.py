from pathlib import Path

# Temporary deterministic source transform used only by feature validation.

def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


main_path = Path("crates/storyteller-ui/src/main.rs")
main = main_path.read_text(encoding="utf-8")
main = replace_once(
    main,
    "mod runtime_import;\nmod worker_bridge;",
    "mod review_ui;\nmod runtime_import;\nmod worker_bridge;",
    "main module declarations",
)
main = replace_once(
    main,
    "    let worker_bridge = Rc::new(RefCell::new(WorkerBridge::default()));\n    ui.set_queue_rows(queue_rows.clone().into());",
    "    let worker_bridge = Rc::new(RefCell::new(WorkerBridge::default()));\n    let review_ui_controller = review_ui::install_review_ui(&ui, Rc::clone(&queue));\n    ui.set_queue_rows(queue_rows.clone().into());",
    "review UI installation",
)
main = replace_once(
    main,
    "        let detail_stage_rows = Rc::clone(&detail_stage_rows);\n        let ui_weak = ui.as_weak();\n        poll_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {\n            worker_bridge.borrow_mut().poll(\n                &ui_weak,\n                &queue,\n                &queue_rows,\n                &stage_rows,\n                &detail_stage_rows,\n            );\n        });",
    "        let detail_stage_rows = Rc::clone(&detail_stage_rows);\n        let review_ui_controller = Rc::clone(&review_ui_controller);\n        let ui_weak = ui.as_weak();\n        poll_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {\n            worker_bridge.borrow_mut().poll(\n                &ui_weak,\n                &queue,\n                &queue_rows,\n                &stage_rows,\n                &detail_stage_rows,\n            );\n            review_ui::refresh_review_ui(&ui_weak, &queue, &review_ui_controller);\n        });",
    "poll timer review refresh",
)
main = replace_once(
    main,
    "    refresh_main_view(\n        &ui,\n        &queue.borrow(),\n        &queue_rows,\n        &stage_rows,\n        &detail_stage_rows,\n    );\n    let result = ui.run();",
    "    refresh_main_view(\n        &ui,\n        &queue.borrow(),\n        &queue_rows,\n        &stage_rows,\n        &detail_stage_rows,\n    );\n    review_ui::refresh_review_ui(&ui.as_weak(), &queue, &review_ui_controller);\n    let result = ui.run();",
    "initial review refresh",
)
main_path.write_text(main, encoding="utf-8", newline="\n")

bridge_path = Path("crates/storyteller-ui/src/worker_bridge.rs")
bridge = bridge_path.read_text(encoding="utf-8")
bridge = replace_once(
    bridge,
    "fn audio_review_draft_path(job: &Job) -> std::path::PathBuf {",
    "pub(crate) fn audio_review_draft_path(job: &Job) -> std::path::PathBuf {",
    "draft path visibility",
)
bridge = replace_once(
    bridge,
    "fn audio_review_path(job: &Job) -> std::path::PathBuf {",
    "pub(crate) fn audio_review_path(job: &Job) -> std::path::PathBuf {",
    "review path visibility",
)
bridge = replace_once(
    bridge,
    "pub(crate) fn load_audio_review_report(job: &Job) -> Result<AudioReviewReport, String> {\n    read_audio_review_report(&audio_review_path(job))\n}",
    "pub(crate) fn job_workspace(job: &Job) -> storyteller_core::JobWorkspace {\n    pipeline_backend::job_workspace(job)\n}\n\npub(crate) fn load_audio_review_report(job: &Job) -> Result<AudioReviewReport, String> {\n    read_audio_review_report(&audio_review_path(job))\n}",
    "workspace bridge",
)
bridge_path.write_text(bridge, encoding="utf-8", newline="\n")

review_path = Path("crates/storyteller-ui/src/review_ui.rs")
review = review_path.read_text(encoding="utf-8")
review = review.replace(
    "const UI_CANDIDATE_LIMIT: usize = 8;",
    "const UI_CANDIDATE_LIMIT: usize = 4;",
)
review_path.write_text(review, encoding="utf-8", newline="\n")

slint_path = Path("crates/storyteller-ui/ui/app-window.slint")
slint = slint_path.read_text(encoding="utf-8")
slint = replace_once(
    slint,
    "export struct StageDetailRow {\n    label: string,\n    state: string,\n    elapsed: string,\n    activity: string,\n}\n",
    "export struct StageDetailRow {\n    label: string,\n    state: string,\n    elapsed: string,\n    activity: string,\n}\n\nexport struct ReviewCandidateRow {\n    href: string,\n    line-index: int,\n    text: string,\n    score: string,\n}\n",
    "review candidate struct",
)
slint = replace_once(
    slint,
    '    in property <string> review-preview-text: "";\n',
    '    in property <string> review-preview-text: "";\n    in property <[ReviewCandidateRow]> review-candidates;\n    in property <string> review-item-position-text: "";\n    in property <string> review-item-time-text: "";\n    in property <string> review-item-transcript-text: "";\n    in property <string> review-item-decision-text: "";\n    in property <string> review-seek-text: "";\n    in property <string> review-allocator-status-text: "";\n    in-out property <string> review-preview-status-text: "";\n    in property <bool> review-can-previous: false;\n    in property <bool> review-can-next: false;\n    in property <bool> review-complete: false;\n',
    "review UI properties",
)
slint = replace_once(
    slint,
    "    callback continue-after-review();\n",
    "    callback continue-after-review();\n    callback review-previous();\n    callback review-next();\n    callback review-seek-relative(int);\n    callback review-play();\n    callback review-stop();\n    callback review-assign(string, int);\n    callback review-exclude();\n    callback finish-review();\n",
    "review callbacks",
)
slint = replace_once(
    slint,
    "min-height: root.needs-review ? 350px : 250px;",
    "min-height: root.needs-review ? 520px : 250px;",
    "review card height",
)
old_review = '''                    if root.needs-review : Rectangle {
                        min-height: 96px;
                        border-width: 1px;
                        border-color: Palette.border;
                        border-radius: 7px;
                        VerticalBox {
                            padding: 10px;
                            spacing: 5px;
                            Text { text: "UNMATCHED AUDIO REVIEW"; font-size: 12px; font-weight: 700; }
                            Text { text: root.review-summary-text; wrap: word-wrap; }
                            if root.review-preview-text != "" : Text {
                                text: root.review-preview-text;
                                font-size: 11px;
                                opacity: 0.72;
                                wrap: word-wrap;
                                max-height: 56px;
                            }
                        }
                    }
'''
new_review = '''                    if root.needs-review : Rectangle {
                        min-height: 315px;
                        border-width: 1px;
                        border-color: Palette.border;
                        border-radius: 7px;
                        VerticalBox {
                            padding: 10px;
                            spacing: 7px;
                            HorizontalBox {
                                spacing: 8px;
                                Text { text: "UNMATCHED AUDIO ALLOCATOR"; font-size: 12px; font-weight: 700; vertical-alignment: center; }
                                Rectangle { horizontal-stretch: 1; }
                                Text { text: root.review-item-position-text; opacity: 0.72; vertical-alignment: center; }
                                Button { text: "Previous"; enabled: root.review-can-previous; clicked => { root.review-previous(); } }
                                Button { text: "Next"; enabled: root.review-can-next; clicked => { root.review-next(); } }
                            }
                            Text { text: root.review-summary-text; wrap: word-wrap; opacity: 0.82; }
                            HorizontalBox {
                                spacing: 7px;
                                Text { text: root.review-item-time-text; font-weight: 600; vertical-alignment: center; }
                                Text { text: root.review-seek-text; opacity: 0.72; vertical-alignment: center; }
                                Rectangle { horizontal-stretch: 1; }
                                Button { text: "-5s"; clicked => { root.review-seek-relative(-1); } }
                                Button { text: "Play"; clicked => { root.review-play(); } }
                                Button { text: "+5s"; clicked => { root.review-seek-relative(1); } }
                                Button { text: "Stop"; clicked => { root.review-stop(); } }
                            }
                            if root.review-preview-status-text != "" : Text { text: root.review-preview-status-text; font-size: 11px; opacity: 0.72; overflow: elide; }
                            Text { text: root.review-item-transcript-text; wrap: word-wrap; max-height: 54px; }
                            Text { text: root.review-item-decision-text; font-size: 11px; opacity: 0.75; wrap: word-wrap; }
                            HorizontalBox {
                                spacing: 8px;
                                Text { text: "EPUB TEXT CANDIDATES"; font-size: 11px; font-weight: 700; vertical-alignment: center; }
                                Rectangle { horizontal-stretch: 1; }
                                Button { text: "Exclude segment"; clicked => { root.review-exclude(); } }
                            }
                            for candidate in root.review-candidates : Rectangle {
                                min-height: 42px;
                                max-height: 42px;
                                border-width: 1px;
                                border-color: Palette.border;
                                border-radius: 5px;
                                HorizontalBox {
                                    padding-left: 7px;
                                    padding-right: 7px;
                                    spacing: 7px;
                                    VerticalBox {
                                        horizontal-stretch: 1;
                                        spacing: 1px;
                                        Text { text: candidate.text; overflow: elide; font-size: 11px; }
                                        HorizontalBox {
                                            spacing: 6px;
                                            Text { text: candidate.href; overflow: elide; font-size: 9px; opacity: 0.58; horizontal-stretch: 1; }
                                            Text { text: candidate.score; font-size: 9px; opacity: 0.58; }
                                        }
                                    }
                                    Button { text: "Assign"; clicked => { root.review-assign(candidate.href, candidate.line-index); } }
                                }
                            }
                            Text { text: root.review-allocator-status-text; font-size: 11px; opacity: 0.78; wrap: word-wrap; }
                        }
                    }
'''
slint = replace_once(slint, old_review, new_review, "allocator panel")
old_buttons = '''                        if root.needs-review : Button {
                            text: "Continue without unmatched audio";
                            primary: true;
                            clicked => { root.continue-after-review(); }
                        }
'''
new_buttons = '''                        if root.needs-review : Button {
                            text: "Exclude all pending & continue";
                            clicked => { root.continue-after-review(); }
                        }
                        if root.needs-review : Button {
                            text: "Continue";
                            primary: true;
                            enabled: root.review-complete;
                            clicked => { root.finish-review(); }
                        }
'''
slint = replace_once(slint, old_buttons, new_buttons, "review action buttons")
slint_path.write_text(slint, encoding="utf-8", newline="\n")
