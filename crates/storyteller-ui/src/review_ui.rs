//! Slint-only projection and command binding. Evidence and preview belong to the application.
use crate::{dispatch_command, format_millis, AppWindow, ReviewCandidateRow, SharedApplication};
use slint::{ComponentHandle, VecModel};
use std::{cell::RefCell, rc::Rc};
use storyteller_application::{ApplicationCommand, ReviewAction};
use storyteller_core::{
    AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination, AudioReviewEdge,
    AudioReviewItem, JobId,
};

#[derive(Default)]
pub(crate) struct ReviewUiController {
    revision: Option<u64>,
    candidates: Rc<VecModel<ReviewCandidateRow>>,
}

pub(crate) fn install_review_ui(
    ui: &AppWindow,
    app: SharedApplication,
) -> Rc<RefCell<ReviewUiController>> {
    let controller = Rc::new(RefCell::new(ReviewUiController::default()));
    ui.set_review_candidates(controller.borrow().candidates.clone().into());
    macro_rules! action {
        ($callback:ident, $action:expr) => {{
            let app = Rc::clone(&app);
            let weak = ui.as_weak();
            ui.$callback(move || {
                if let Some(ui) = weak.upgrade() {
                    dispatch_command(&ui, &app, ApplicationCommand::Review($action));
                }
            });
        }};
    }
    action!(on_review_previous, ReviewAction::Previous);
    action!(on_review_next, ReviewAction::Next);
    action!(on_review_play, ReviewAction::Play);
    action!(on_review_stop, ReviewAction::Stop);
    action!(on_review_refresh, ReviewAction::Reload);
    {
        let app = Rc::clone(&app);
        let weak = ui.as_weak();
        ui.on_review_seek_relative(move |direction| {
            if let Some(ui) = weak.upgrade() {
                dispatch_command(
                    &ui,
                    &app,
                    ApplicationCommand::Review(ReviewAction::SeekRelative(direction)),
                );
            }
        });
    }
    {
        let app = Rc::clone(&app);
        let weak = ui.as_weak();
        ui.on_review_assign(move |href, line_index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let command = (|| {
                let app = app.borrow();
                let review = app.snapshot().review;
                let (id, item) = selected_item(review)?;
                let candidate = review
                    .candidates
                    .iter()
                    .find(|candidate| {
                        candidate.href == href.as_str() && candidate.line_index == line_index
                    })
                    .ok_or("That match is no longer selected. Choose a current EPUB match.")?;
                if let Some(image_href) = &candidate.image_href {
                    Ok(ApplicationCommand::AssignGraphic {
                        id,
                        item_id: item.id,
                        document_href: candidate.href.clone(),
                        image_href: image_href.clone(),
                    })
                } else {
                    let line_index = usize::try_from(candidate.line_index)
                        .map_err(|_| "Invalid EPUB text match.")?;
                    Ok(ApplicationCommand::SaveReviewDecision {
                        id,
                        item_id: item.id,
                        decision: AudioReviewDecision::Assigned {
                            destination: AudioReviewDestination {
                                href: candidate.href.clone(),
                                line_index: Some(line_index),
                                image_href: None,
                                supplemental: None,
                            },
                            classification: None,
                            source: AudioReviewDecisionSource::Manual,
                        },
                    })
                }
            })();
            run_result(&ui, &app, command);
        });
    }
    {
        let app = Rc::clone(&app);
        let weak = ui.as_weak();
        ui.on_review_preserve_edge(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let command = selected_item(app.borrow().snapshot().review).map(|(id, item)| {
                ApplicationCommand::PreserveEdge {
                    id,
                    item_id: item.id,
                }
            });
            run_result(&ui, &app, command);
        });
    }
    {
        let app = Rc::clone(&app);
        let weak = ui.as_weak();
        ui.on_review_exclude(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let command = selected_item(app.borrow().snapshot().review).map(|(id, item)| {
                ApplicationCommand::SaveReviewDecision {
                    id,
                    item_id: item.id,
                    decision: AudioReviewDecision::Excluded {
                        reason: "Excluded during audio review.".into(),
                        source: AudioReviewDecisionSource::Manual,
                    },
                }
            });
            run_result(&ui, &app, command);
        });
    }
    {
        let app = Rc::clone(&app);
        let weak = ui.as_weak();
        ui.on_finish_review(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let command = app
                .borrow()
                .snapshot()
                .review
                .job_id
                .ok_or_else(|| "No book is waiting for review.".to_string())
                .map(ApplicationCommand::FinishReview);
            run_result(&ui, &app, command);
        });
    }
    controller
}

fn selected_item(
    review: &storyteller_application::ReviewView,
) -> Result<(JobId, AudioReviewItem), String> {
    if review.busy {
        return Err("Wait for the selected segment to load.".into());
    }
    let id = review
        .job_id
        .ok_or("No book is waiting for audio review.")?;
    let item = review
        .selected_item()
        .ok_or("No audio segment is selected.")?;
    Ok((id, item.clone()))
}
fn run_result(
    ui: &AppWindow,
    app: &SharedApplication,
    command: Result<ApplicationCommand, String>,
) {
    match command {
        Ok(command) => {
            dispatch_command(ui, app, command);
        }
        Err(error) => ui.set_status_text(error.into()),
    }
}

pub(crate) fn refresh_review_ui(
    weak: &slint::Weak<AppWindow>,
    app: &SharedApplication,
    controller: &Rc<RefCell<ReviewUiController>>,
) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    let app = app.borrow();
    let view = app.snapshot().review;
    let mut controller = controller.borrow_mut();
    if controller.revision == Some(view.revision) {
        return;
    }
    controller.revision = Some(view.revision);
    crate::update_model(
        &controller.candidates,
        view.candidates
            .iter()
            .map(|candidate| ReviewCandidateRow {
                href: candidate.href.clone().into(),
                line_index: candidate.line_index,
                text: candidate.text.clone().into(),
                score: candidate.score.clone().into(),
            })
            .collect(),
    );
    ui.set_review_busy(view.busy);
    ui.set_review_playing(view.playing);
    ui.set_review_preview_status_text(
        [view.message.as_str(), view.preview_message.as_str()]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
            .into(),
    );
    let report = view.report.as_deref();
    ui.set_review_complete(report.is_some_and(|report| report.is_complete()));
    ui.set_review_can_previous(view.selected_index > 0);
    ui.set_review_can_next(
        report.is_some_and(|report| view.selected_index + 1 < report.unmatched.len()),
    );
    ui.set_review_allocator_status_text(match report {
        Some(report) if report.is_complete() => {
            "All decisions saved. Finish review to continue.".into()
        }
        Some(report) => format!(
            "{} of {} segments still need a decision.",
            report.pending_count(),
            report.unmatched.len()
        )
        .into(),
        None => if view.busy {
            "Loading review…"
        } else {
            "Review unavailable. See the details above."
        }
        .into(),
    });
    let Some(item) = view.selected_item() else {
        ui.set_review_item_ready(false);
        ui.set_review_item_position_text(
            if view.busy {
                "Loading review…"
            } else {
                "Audio review"
            }
            .into(),
        );
        ui.set_review_item_time_text("".into());
        ui.set_review_item_transcript_text(
            if view.busy {
                ""
            } else {
                "No audio segment loaded."
            }
            .into(),
        );
        ui.set_review_item_decision_text("".into());
        ui.set_review_seek_text("".into());
        ui.set_review_can_preserve_edge(false);
        ui.set_review_preserve_edge_text("".into());
        return;
    };
    ui.set_review_item_ready(true);
    ui.set_review_item_position_text(
        format!(
            "Segment {} of {}",
            view.selected_index + 1,
            report.unwrap().unmatched.len()
        )
        .into(),
    );
    ui.set_review_item_time_text(
        format!(
            "{}–{}",
            format_millis(item.audio_start_ms),
            format_millis(item.audio_end_ms)
        )
        .into(),
    );
    ui.set_review_item_transcript_text(item.transcript_text.clone().into());
    ui.set_review_item_decision_text(decision_text(item).into());
    ui.set_review_seek_text(
        format!(
            "{} / {}",
            format_millis(view.seek_ms),
            format_millis(item.audio_end_ms.saturating_sub(item.audio_start_ms))
        )
        .into(),
    );
    ui.set_review_can_preserve_edge(item.edge.is_some());
    ui.set_review_preserve_edge_text(
        match item.edge {
            Some(AudioReviewEdge::Introduction) => "Preserve introduction",
            Some(AudioReviewEdge::Credits) => "Preserve credits",
            None => "",
        }
        .into(),
    );
}

fn decision_text(item: &AudioReviewItem) -> String {
    match &item.decision {
        AudioReviewDecision::Pending => match item.edge {
            Some(AudioReviewEdge::Introduction) => {
                "Needs a decision · This may be opening narration."
            }
            Some(AudioReviewEdge::Credits) => "Needs a decision · This may be closing narration.",
            None => "Needs a decision · Choose a match or exclude this segment.",
        }
        .into(),
        AudioReviewDecision::Assigned { destination, .. } => {
            if destination.supplemental.is_some() {
                "Saved · Preserved on a supplemental read-aloud page.".into()
            } else if destination.image_href.is_some() {
                format!("Saved · Image narration in {}", destination.href)
            } else {
                format!("Saved · Matched to {}", destination.href)
            }
        }
        AudioReviewDecision::Excluded { .. } => "Saved · Excluded from synchronization.".into(),
    }
}
