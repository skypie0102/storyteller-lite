//! Actual compiled Slint scenes, headless screenshots and pointer/keyboard smoke checks.
//! cargo run --locked -p storyteller-ui --example ui_snapshot -- tools/ui/fixtures.json output-dir
use serde_json::Value;
use slint::{
    platform::{software_renderer::MinimalSoftwareWindow, Platform, WindowAdapter, WindowEvent},
    ComponentHandle, PhysicalSize, VecModel,
};
use std::{cell::Cell, error::Error, fs, io::Write, path::Path, rc::Rc};
slint::include_modules!();
#[path = "../src/appearance.rs"]
mod appearance;

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
            "worker-recommendation-text" => set_worker_recommendation_text,
            "gpu-status-text" => set_gpu_status_text,
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
            "runtime-installable" => set_runtime_installable,
            "queue-runtime-ready" => set_queue_runtime_ready,
            "dark-theme" => set_dark_theme,
        }
        match name.as_str() {
            "workspace-page" => ui.set_workspace_page(value.as_i64().unwrap() as i32),
            "transcription-backend-selection" => {
                ui.set_transcription_backend_selection(value.as_i64().unwrap() as i32)
            }
            "transcription-worker-selection" => {
                ui.set_transcription_worker_selection(value.as_i64().unwrap() as i32)
            }
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
        .as_chunks::<4>()
        .0
        .iter()
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
fn interaction_smoke(width: u32, height: u32, dark: bool) -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;
    ui.set_dark_theme(dark);
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
    // Navigate through New book, Queue and Settings to the EPUB picker.
    // Fluent buttons deliberately do not take keyboard focus on pointer clicks.
    for _ in 0..4 {
        let text = slint::platform::Key::Tab.into();
        ui.window().dispatch_event(WindowEvent::KeyPressed { text });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: slint::platform::Key::Tab.into(),
        });
    }
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
    assert_eq!(
        picked.get(),
        1,
        "EPUB picker accepts Tab and Space activation"
    );
    click(&ui, 120.0, 264.0);
    assert_eq!(picked.get(), 2, "EPUB picker is reachable by pointer");
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
    println!("UI pointer/keyboard smoke passed at {width}x{height}, dark={dark}");
    Ok(())
}
fn key(ui: &AppWindow, key: slint::platform::Key) {
    let text: slint::SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}
fn assert_canvas(ui: &AppWindow, expected: [u8; 3]) -> Result<(), Box<dyn Error>> {
    let pixels = ui.window().take_snapshot()?;
    assert_eq!(
        &pixels.as_bytes()[..3],
        expected.as_slice(),
        "Canvas follows the selected theme immediately"
    );
    Ok(())
}
fn appearance_smoke(width: u32, height: u32, output: &Path) -> Result<(), Box<dyn Error>> {
    let path = output.join(format!("appearance-{width}x{height}.json"));
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let ui = AppWindow::new()?;
    appearance::install(&ui, path.clone());
    assert!(ui.get_dark_theme(), "A new installation defaults to dark");
    ui.set_settings_open(true);
    ui.show()?;
    ui.window().set_size(PhysicalSize::new(width, height));
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    assert_canvas(&ui, [0x10, 0x17, 0x19])?;
    // Three navigation buttons precede the first Settings control, Theme.
    for _ in 0..4 {
        key(&ui, slint::platform::Key::Tab);
    }
    key(&ui, slint::platform::Key::DownArrow);
    assert!(
        !ui.get_dark_theme(),
        "Theme accepts keyboard selection of Light"
    );
    assert_canvas(&ui, [0xf4, 0xf6, 0xf5])?;
    let reopened = AppWindow::new()?;
    appearance::install(&reopened, path.clone());
    assert!(
        !reopened.get_dark_theme(),
        "Light is restored in a fresh window"
    );
    key(&ui, slint::platform::Key::UpArrow);
    assert!(ui.get_dark_theme(), "Theme can switch back to Dark");
    assert_canvas(&ui, [0x10, 0x17, 0x19])?;
    let reopened = AppWindow::new()?;
    appearance::install(&reopened, path.clone());
    assert!(
        reopened.get_dark_theme(),
        "Dark replaces a previously saved Light choice"
    );
    fs::write(&path, b"{\"theme\":\"unsupported\"}")?;
    let reopened = AppWindow::new()?;
    appearance::install(&reopened, path.clone());
    assert!(
        reopened.get_dark_theme(),
        "Unreadable preferences fall back to Dark"
    );
    fs::remove_file(path)?;
    ui.hide()?;
    println!("Native theme switching and saved appearance smoke passed at {width}x{height}");
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
        for dark in [true, false] {
            for scene in &scenes {
                let ui = AppWindow::new()?;
                apply(&ui, &scene["properties"]);
                ui.set_dark_theme(dark);
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
                    &output.join(format!(
                        "{name}-{width}x{height}-{scale}x-{}.ppm",
                        if dark { "dark" } else { "light" }
                    )),
                    physical_width,
                    physical_height,
                )?;
                ui.hide()?;
            }
            if scale == 1.0 {
                interaction_smoke(width, height, dark)?;
            }
        }
        if scale == 1.0 {
            appearance_smoke(width, height, output)?;
        }
    }
    println!(
        "{} compiled native Slint scene snapshots passed in dark and light, including 200% scale.",
        scenes.len() * 6
    );
    Ok(())
}
