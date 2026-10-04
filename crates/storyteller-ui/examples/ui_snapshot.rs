//! Actual compiled Slint scenes, headless screenshots and pointer/keyboard smoke checks.
//! cargo run --locked -p storyteller-ui --example ui_snapshot -- tools/ui/fixtures.json output-dir
use serde_json::Value;
use slint::{
    platform::{software_renderer::MinimalSoftwareWindow, Platform, WindowAdapter, WindowEvent},
    ComponentHandle, PhysicalSize, VecModel,
};
use std::{cell::Cell, error::Error, fs, io::Write, path::Path, rc::Rc};
slint::include_modules!();

struct SnapshotPlatform;
impl Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(MinimalSoftwareWindow::new(Default::default()))
    }
}
fn text(value: &Value) -> slint::SharedString {
    value.as_str().expect("fixture string").into()
}
fn boolean(value: &Value) -> bool {
    value.as_bool().expect("fixture boolean")
}
fn apply(ui: &AppWindow, properties: &Value) {
    for (name, value) in properties.as_object().expect("fixture properties") {
        macro_rules! strings {
            ($($key:literal => $setter:ident),* $(,)?) => {
                match name.as_str() { $($key => { ui.$setter(text(value)); continue; }),* _ => {} }
            };
        }
        macro_rules! booleans {
            ($($key:literal => $setter:ident),* $(,)?) => {
                match name.as_str() { $($key => { ui.$setter(boolean(value)); continue; }),* _ => {} }
            };
        }
        strings! {
            "status-text" => set_status_text, "epub-source-name" => set_epub_source_name,
            "audio-source-name" => set_audio_source_name, "output-name" => set_output_name,
            "output-directory" => set_output_directory, "active-job-id" => set_active_job_id,
            "active-title" => set_active_title, "active-progress-text" => set_active_progress_text,
            "active-activity-text" => set_active_activity_text, "active-context-text" => set_active_context_text,
            "active-timing-text" => set_active_timing_text, "active-metrics-text" => set_active_metrics_text,
            "review-item-position-text" => set_review_item_position_text, "review-item-time-text" => set_review_item_time_text,
            "review-item-transcript-text" => set_review_item_transcript_text, "review-item-decision-text" => set_review_item_decision_text,
            "review-seek-text" => set_review_seek_text, "review-allocator-status-text" => set_review_allocator_status_text,
            "review-preserve-edge-text" => set_review_preserve_edge_text, "runtime-ffmpeg-text" => set_runtime_ffmpeg_text,
            "runtime-transcription-text" => set_runtime_transcription_text, "runtime-model-text" => set_runtime_model_text,
            "runtime-summary-text" => set_runtime_summary_text,
        }
        booleans! {
            "runtime-ready" => set_runtime_ready, "runtime-busy" => set_runtime_busy,
            "settings-open" => set_settings_open, "has-active-job" => set_has_active_job,
            "queue-paused" => set_queue_paused, "needs-review" => set_needs_review,
            "review-item-ready" => set_review_item_ready, "review-busy" => set_review_busy,
            "review-can-previous" => set_review_can_previous, "review-can-next" => set_review_can_next,
            "review-complete" => set_review_complete, "review-can-preserve-edge" => set_review_can_preserve_edge,
            "cancel-confirmation" => set_cancel_confirmation, "exclude-confirmation" => set_exclude_confirmation,
            "status-details-open" => set_status_details_open,
        }
        match name.as_str() {
            "workspace-page" => ui.set_workspace_page(value.as_i64().unwrap() as i32),
            "active-overall-progress" => {
                ui.set_active_overall_progress(value.as_f64().unwrap() as f32)
            }
            "queue-rows" => ui.set_queue_rows(
                Rc::new(VecModel::from(
                    value
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|row| QueueRow {
                            id: text(&row["id"]),
                            position: text(&row["position"]),
                            title: text(&row["title"]),
                            status: text(&row["status"]),
                            detail: text(&row["detail"]),
                            waiting: boolean(&row["waiting"]),
                            retryable: boolean(&row["retryable"]),
                            can_move_up: boolean(&row["can-move-up"]),
                            can_move_down: boolean(&row["can-move-down"]),
                        })
                        .collect::<Vec<_>>(),
                ))
                .into(),
            ),
            "active-stage-details" => ui.set_active_stage_details(
                Rc::new(VecModel::from(
                    value
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|row| StageDetailRow {
                            label: text(&row["label"]),
                            state: text(&row["state"]),
                            elapsed: text(&row["elapsed"]),
                            activity: text(&row["activity"]),
                        })
                        .collect::<Vec<_>>(),
                ))
                .into(),
            ),
            "review-candidates" => ui.set_review_candidates(
                Rc::new(VecModel::from(
                    value
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|row| ReviewCandidateRow {
                            href: text(&row["href"]),
                            line_index: row["line-index"].as_i64().unwrap() as i32,
                            text: text(&row["text"]),
                            score: text(&row["score"]),
                        })
                        .collect::<Vec<_>>(),
                ))
                .into(),
            ),
            _ => panic!("Unknown fixture property: {name}"),
        }
    }
}
fn snapshot(ui: &AppWindow, path: &Path, width: u32, height: u32) -> Result<(), Box<dyn Error>> {
    let pixels = ui.window().take_snapshot()?;
    assert_eq!((pixels.width(), pixels.height()), (width, height));
    let mut file = fs::File::create(path)?;
    writeln!(file, "P6\n{width} {height}\n255")?;
    let rgb = pixels
        .as_bytes()
        .chunks_exact(4)
        .flat_map(|rgba| rgba[..3].iter().copied())
        .collect::<Vec<_>>();
    file.write_all(&rgb)?;
    Ok(())
}
fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
}
fn interaction_smoke(width: u32, height: u32) -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;
    ui.show()?;
    ui.window().set_size(PhysicalSize::new(width, height));
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    let picked = Rc::new(Cell::new(0));
    let calls = Rc::clone(&picked);
    ui.on_browse_epub(move || calls.set(calls.get() + 1));
    let submitted = Rc::new(Cell::new(0));
    let calls = Rc::clone(&submitted);
    ui.on_queue_book(move || calls.set(calls.get() + 1));
    let _ = ui.window().take_snapshot()?;
    // Real pointer entry and keyboard activation of the source picker.
    click(&ui, 120.0, 264.0);
    assert_eq!(picked.get(), 1, "EPUB picker is reachable");
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
    assert_eq!(picked.get(), 2, "EPUB picker accepts keyboard activation");
    click(&ui, width as f32 - 95.0, height as f32 - 100.0);
    assert_eq!(submitted.get(), 0, "Incomplete sources cannot start");
    ui.set_runtime_ready(true);
    ui.set_epub_source_name("book.epub".into());
    ui.set_audio_source_name("book.m4b".into());
    let _ = ui.window().take_snapshot()?;
    click(&ui, width as f32 - 95.0, height as f32 - 100.0);
    assert_eq!(submitted.get(), 1, "Ready sources can start");
    ui.set_runtime_busy(true);
    click(&ui, width as f32 - 95.0, height as f32 - 100.0);
    assert_eq!(submitted.get(), 1, "Setup in progress blocks start");
    click(&ui, width as f32 - 60.0, 42.0);
    assert!(ui.get_settings_open(), "Settings remains reachable");
    click(&ui, width as f32 - 240.0, 42.0);
    assert!(
        !ui.get_settings_open(),
        "New book navigation returns from Settings"
    );
    // Review completion is gated, and bulk exclusion requires its second click.
    ui.set_workspace_page(1);
    ui.set_has_active_job(true);
    ui.set_active_title("Review fixture".into());
    ui.set_needs_review(true);
    ui.set_review_item_ready(true);
    ui.set_review_item_position_text("Segment 1 of 2".into());
    let continued = Rc::new(Cell::new(0));
    let calls = Rc::clone(&continued);
    ui.on_continue_after_review(move || calls.set(calls.get() + 1));
    let finished = Rc::new(Cell::new(0));
    let calls = Rc::clone(&finished);
    ui.on_finish_review(move || calls.set(calls.get() + 1));
    let _ = ui.window().take_snapshot()?;
    click(&ui, width as f32 - 70.0, height as f32 - 100.0);
    assert_eq!(finished.get(), 0, "Unresolved review cannot finish");
    click(&ui, width as f32 - 225.0, height as f32 - 100.0);
    assert!(
        ui.get_exclude_confirmation(),
        "Bulk exclusion opens the consequence check"
    );
    assert_eq!(continued.get(), 0, "The first click cannot exclude audio");
    let _ = ui.window().take_snapshot()?;
    click(&ui, width as f32 - 140.0, height as f32 - 112.0);
    assert_eq!(
        continued.get(),
        1,
        "Only the second click confirms exclusion"
    );
    assert!(!ui.get_exclude_confirmation());
    ui.hide()?;
    println!("UI pointer/keyboard smoke passed at {width}x{height}");
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let fixtures = args.get(1).ok_or("Expected fixture JSON path")?;
    let output = Path::new(args.get(2).ok_or("Expected output folder")?);
    fs::create_dir_all(output)?;
    slint::platform::set_platform(Box::new(SnapshotPlatform))?;
    let scenes: Vec<Value> = serde_json::from_slice(&fs::read(fixtures)?)?;
    for (width, height, scale) in [(820, 620, 1.0), (1040, 760, 1.0), (820, 620, 2.0)] {
        for scene in &scenes {
            let ui = AppWindow::new()?;
            apply(&ui, &scene["properties"]);
            ui.show()?;
            ui.window().dispatch_event(WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
            });
            let physical_width = (width as f32 * scale) as u32;
            let physical_height = (height as f32 * scale) as u32;
            ui.window()
                .set_size(PhysicalSize::new(physical_width, physical_height));
            let name = scene["name"].as_str().unwrap();
            snapshot(
                &ui,
                &output.join(format!("{name}-{width}x{height}-{scale}x.ppm")),
                physical_width,
                physical_height,
            )?;
            ui.hide()?;
        }
        if scale == 1.0 {
            interaction_smoke(width, height)?;
        }
    }
    println!("39 compiled native Slint scene snapshots passed, including 200% scale.");
    Ok(())
}
