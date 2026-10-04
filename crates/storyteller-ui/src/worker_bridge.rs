//! Slint adapter: requests application commands and renders only changed snapshots.
use crate::{refresh_main_view, AppWindow, QueueRow, SharedApplication, StageDetailRow, StageRow};
use slint::VecModel;
use storyteller_application::ApplicationCommand;

#[derive(Default)]
pub(crate) struct ViewBridge {
    queue_revision: Option<u64>,
    runtime_revision: Option<u64>,
    settings_was_open: bool,
    review_was_visible: bool,
}

impl ViewBridge {
    pub(crate) fn poll(
        &mut self,
        ui_weak: &slint::Weak<AppWindow>,
        app: &SharedApplication,
        queue_rows: &VecModel<QueueRow>,
        stage_rows: &VecModel<StageRow>,
        details: &VecModel<StageDetailRow>,
    ) -> bool {
        let Some(ui) = ui_weak.upgrade() else {
            return false;
        };
        let opened = ui.get_settings_open() && !self.settings_was_open;
        let review_visible =
            ui.get_needs_review() && !ui.get_settings_open() && ui.get_workspace_page() == 1;
        if self.review_was_visible && !review_visible {
            let _ = app.borrow_mut().dispatch(ApplicationCommand::Review(
                storyteller_application::ReviewAction::Stop,
            ));
        }
        self.review_was_visible = review_visible;
        self.settings_was_open = ui.get_settings_open();
        let install = ui.get_runtime_install_requested();
        let scan = ui.get_runtime_refresh_requested() || opened;
        ui.set_runtime_install_requested(false);
        ui.set_runtime_refresh_requested(false);
        if (install || scan) && !app.borrow().snapshot().runtime.busy {
            let command = if install {
                ApplicationCommand::InstallMissingRuntime
            } else {
                ApplicationCommand::ScanRuntime
            };
            if let Err(error) = app.borrow_mut().dispatch(command) {
                ui.set_runtime_install_status_text(error.into());
            }
        }
        app.borrow_mut().poll();
        self.render(&ui, app, queue_rows, stage_rows, details)
    }

    pub(crate) fn render(
        &mut self,
        ui: &AppWindow,
        app: &SharedApplication,
        queue_rows: &VecModel<QueueRow>,
        stage_rows: &VecModel<StageRow>,
        details: &VecModel<StageDetailRow>,
    ) -> bool {
        let app = app.borrow();
        let snapshot = app.snapshot();
        let changed = self.queue_revision != Some(snapshot.queue_revision);
        if changed {
            refresh_main_view(ui, snapshot.queue, queue_rows, stage_rows, details);
            if let Some(notice) = snapshot.notice {
                ui.set_status_text(notice.into());
            } else if snapshot.queue.active_job().is_none() {
                ui.set_status_text(
                    if snapshot.queue.state() == storyteller_core::QueueState::Paused {
                        "Queue paused"
                    } else {
                        "Ready"
                    }
                    .into(),
                );
            }
            self.queue_revision = Some(snapshot.queue_revision);
        }
        if self.runtime_revision != Some(snapshot.runtime_revision) {
            ui.set_runtime_busy(snapshot.runtime.busy);
            ui.set_runtime_install_status_text(snapshot.runtime.message.clone().into());
            if let Some(status) = &snapshot.runtime.status {
                ui.set_worker_recommendation_text(status.workers.description().into());
                ui.set_runtime_summary_text(status.summary().into());
                ui.set_runtime_ffmpeg_text(status.ffmpeg_text().into());
                ui.set_runtime_transcription_text(status.transcription_text().into());
                ui.set_runtime_model_text(status.model_text().into());
                ui.set_runtime_ready(status.ready());
            }
            self.runtime_revision = Some(snapshot.runtime_revision);
        }
        changed
    }
}
