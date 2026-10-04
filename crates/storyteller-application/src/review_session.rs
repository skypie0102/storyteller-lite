//! Cached presentation and cancellable evidence loading; no UI or process waits on the host timer.
use crate::{job_workspace, load_audio_review_report};
use std::{
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
};
use storyteller_core::{
    prepared_job_sources, read_epub_corpus, review_image_candidates, review_text_candidates,
    AlignmentDocument, AudioReviewItem, AudioReviewReport, CancellationToken, EpubCorpus, Job,
    JobId, PipelineStage, DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
};

#[derive(Clone, Copy)]
pub enum ReviewAction {
    Previous,
    Next,
    SeekRelative(i32),
    Play,
    Stop,
    Reload,
}

#[derive(Clone, Debug)]
pub struct ReviewCandidate {
    pub href: String,
    pub line_index: i32,
    pub image_href: Option<String>,
    pub text: String,
    pub score: String,
}

#[derive(Default)]
pub struct ReviewView {
    pub revision: u64,
    pub job_id: Option<JobId>,
    pub report: Option<Arc<AudioReviewReport>>,
    pub selected_index: usize,
    pub seek_ms: u64,
    pub candidates: Vec<ReviewCandidate>,
    pub busy: bool,
    pub message: String,
    pub preview_message: String,
    pub playing: bool,
}

impl ReviewView {
    pub fn selected_item(&self) -> Option<&AudioReviewItem> {
        self.report.as_ref()?.unmatched.get(self.selected_index)
    }
}

struct LoadRequest {
    generation: u64,
    job: Job,
    index: usize,
    advance: bool,
    cancel: CancellationToken,
}
struct LoadedReview {
    report: Arc<AudioReviewReport>,
    index: usize,
    candidates: Vec<ReviewCandidate>,
    message: String,
}
struct LoadEvent {
    generation: u64,
    job_id: JobId,
    result: Result<LoadedReview, String>,
}
struct ReviewTask {
    sender: Sender<Option<LoadRequest>>,
    receiver: Receiver<LoadEvent>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct ReviewSession {
    pub(crate) view: ReviewView,
    job: Option<Job>,
    generation: u64,
    cancel: CancellationToken,
    task: Option<ReviewTask>,
    preview: crate::review_preview::AudioPreview,
}

impl ReviewSession {
    pub(crate) fn synchronize(&mut self, job: Option<&Job>) {
        if self.view.job_id == job.map(|job| job.id) {
            return;
        }
        self.cancel.request();
        self.generation = self.generation.wrapping_add(1);
        self.preview.stop();
        self.job = job.cloned();
        let revision = self.view.revision.wrapping_add(1);
        self.view = ReviewView {
            revision,
            job_id: job.map(|job| job.id),
            ..Default::default()
        };
        if job.is_some() {
            self.request_load(false);
        }
    }

    pub(crate) fn reload_after_decision(&mut self) {
        self.stop_preview();
        self.request_load(true);
    }

    pub(crate) fn action(&mut self, action: ReviewAction) -> Result<(), String> {
        match action {
            ReviewAction::Reload => {
                self.stop_preview();
                self.request_load(false);
            }
            ReviewAction::Stop => self.stop_preview(),
            ReviewAction::Previous | ReviewAction::Next => {
                let count = self
                    .view
                    .report
                    .as_ref()
                    .map_or(0, |report| report.unmatched.len());
                if count == 0 {
                    return Ok(());
                }
                let index = match action {
                    ReviewAction::Previous => self.view.selected_index.saturating_sub(1),
                    _ => self.view.selected_index.saturating_add(1).min(count - 1),
                };
                if index != self.view.selected_index {
                    self.stop_preview();
                    self.view.selected_index = index;
                    self.view.seek_ms = 0;
                    self.request_load(false);
                }
            }
            ReviewAction::SeekRelative(direction) => {
                let item = self
                    .view
                    .selected_item()
                    .ok_or("No audio segment is selected.")?;
                let limit = item
                    .audio_end_ms
                    .saturating_sub(item.audio_start_ms)
                    .saturating_sub(1);
                let delta = i64::from(direction).saturating_mul(5_000);
                let next = self.view.seek_ms.saturating_add_signed(delta).min(limit);
                self.stop_preview();
                self.view.seek_ms = next;
                self.touch();
            }
            ReviewAction::Play => {
                if self.view.busy {
                    return Err("Wait for the selected segment to load.".into());
                }
                let item = self
                    .view
                    .selected_item()
                    .ok_or("No audio segment is selected.")?;
                let job = self.job.as_ref().ok_or("No book is waiting for review.")?;
                self.preview
                    .play(job.clone(), item.clone(), self.view.seek_ms)?;
                self.view.playing = true;
                self.view.preview_message = "Starting audio preview…".into();
                self.touch();
            }
        }
        Ok(())
    }

    fn stop_preview(&mut self) {
        self.preview.stop();
        self.view.playing = false;
        self.view.preview_message.clear();
        self.touch();
    }

    fn request_load(&mut self, advance: bool) {
        let Some(job) = self.job.clone() else {
            return;
        };
        self.cancel.request();
        self.cancel = CancellationToken::default();
        self.generation = self.generation.wrapping_add(1);
        self.view.candidates.clear();
        self.view.message.clear();
        self.view.busy = true;
        self.touch();
        if self
            .task
            .as_ref()
            .is_some_and(|task| task.handle.is_finished())
        {
            if let Some(task) = self.task.take() {
                let _ = task.handle.join();
            }
        }
        if self.task.is_none() {
            let mut cache = None;
            match spawn_loader(move |request| load_review(request, &mut cache)) {
                Ok(task) => self.task = Some(task),
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
        let request = LoadRequest {
            generation: self.generation,
            job,
            index: self.view.selected_index,
            advance,
            cancel: self.cancel.clone(),
        };
        if self
            .task
            .as_ref()
            .unwrap()
            .sender
            .send(Some(request))
            .is_err()
        {
            self.fail("Review loading stopped. Choose another book or reopen the app.".into());
        }
    }

    pub(crate) fn poll(&mut self) {
        let events = self
            .task
            .as_ref()
            .map(|task| task.receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            if event.generation != self.generation || Some(event.job_id) != self.view.job_id {
                continue;
            }
            self.view.busy = false;
            match event.result {
                Ok(loaded) => {
                    self.view.report = Some(loaded.report);
                    self.view.selected_index = loaded.index;
                    self.view.candidates = loaded.candidates;
                    self.view.message = loaded.message;
                    let limit = self.view.selected_item().map_or(0, |item| {
                        item.audio_end_ms
                            .saturating_sub(item.audio_start_ms)
                            .saturating_sub(1)
                    });
                    self.view.seek_ms = self.view.seek_ms.min(limit);
                    self.touch();
                }
                Err(error) => self.fail(error),
            }
        }
        if self.view.busy
            && self
                .task
                .as_ref()
                .is_some_and(|task| task.handle.is_finished())
        {
            self.fail("Review loading stopped unexpectedly. Reopen the app to retry.".into());
        }
        if let Some((playing, message)) = self.preview.poll() {
            self.view.playing = playing;
            self.view.preview_message = message;
            self.touch();
        }
    }
    fn fail(&mut self, error: String) {
        self.view.busy = false;
        self.view.report = None;
        self.view.candidates.clear();
        self.view.message = format!("Could not load review: {error}");
        self.touch();
    }
    fn touch(&mut self) {
        self.view.revision = self.view.revision.wrapping_add(1);
    }
    pub(crate) fn shutdown(&mut self) {
        self.cancel.request();
        if let Some(task) = self.task.take() {
            let _ = task.sender.send(None);
            let _ = task.handle.join();
        }
        self.preview.shutdown();
    }
}
impl Drop for ReviewSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn spawn_loader(
    mut load: impl FnMut(&LoadRequest) -> Result<LoadedReview, String> + Send + 'static,
) -> Result<ReviewTask, String> {
    let (sender, requests) = mpsc::channel::<Option<LoadRequest>>();
    let (events, receiver) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("review-evidence".into())
        .spawn(move || {
            while let Ok(Some(mut request)) = requests.recv() {
                // Coalesce navigation: keep one owned worker and only the newest queued request.
                while let Ok(next) = requests.try_recv() {
                    let Some(next) = next else {
                        return;
                    };
                    request = next;
                }
                if request.cancel.is_requested() {
                    continue;
                }
                let result = load(&request);
                if !request.cancel.is_requested() {
                    let _ = events.send(LoadEvent {
                        generation: request.generation,
                        job_id: request.job.id,
                        result,
                    });
                }
            }
        })
        .map_err(|error| format!("Could not start review worker: {error}"))?;
    Ok(ReviewTask {
        sender,
        receiver,
        handle,
    })
}

struct BookEvidence {
    id: JobId,
    alignment: AlignmentDocument,
    corpus: EpubCorpus,
}
fn load_review(
    request: &LoadRequest,
    cache: &mut Option<BookEvidence>,
) -> Result<LoadedReview, String> {
    let report = Arc::new(load_audio_review_report(&request.job)?);
    let index = if request.advance {
        report
            .unmatched
            .iter()
            .position(|item| item.decision.is_pending())
            .unwrap_or(request.index)
    } else {
        request.index
    }
    .min(report.unmatched.len().saturating_sub(1));
    let mut loaded = LoadedReview {
        report,
        index,
        candidates: Vec::new(),
        message: String::new(),
    };
    let Some(item) = loaded.report.unmatched.get(index) else {
        return Ok(loaded);
    };
    match candidates(&request.job, item, &request.cancel, cache) {
        Ok((rows, message)) => {
            loaded.candidates = rows;
            loaded.message = message;
        }
        Err(error) => {
            loaded.message =
                format!("EPUB matches unavailable: {error} You can still exclude audio.")
        }
    }
    Ok(loaded)
}

fn candidates(
    job: &Job,
    item: &AudioReviewItem,
    cancel: &CancellationToken,
    cache: &mut Option<BookEvidence>,
) -> Result<(Vec<ReviewCandidate>, String), String> {
    let workspace = job_workspace(job);
    if cache.as_ref().is_none_or(|cached| cached.id != job.id) {
        let alignment = serde_json::from_slice(
            &std::fs::read(
                workspace
                    .stage_dir(PipelineStage::Align)
                    .join("alignment.json"),
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let corpus = read_epub_corpus(
            &workspace
                .stage_dir(PipelineStage::Analyze)
                .join("book-corpus.json"),
        )?;
        *cache = Some(BookEvidence {
            id: job.id,
            alignment,
            corpus,
        });
    }
    if cancel.is_requested() {
        return Err("Review selection changed.".into());
    }
    let evidence = cache.as_ref().unwrap();
    let mut rows = review_text_candidates(
        &evidence.alignment,
        &evidence.corpus,
        item.alignment_index,
        4,
    )?
    .into_iter()
    .map(|candidate| ReviewCandidate {
        href: candidate.href,
        line_index: i32::try_from(candidate.line_index).unwrap_or(i32::MAX),
        image_href: None,
        text: candidate.text,
        score: format!("{}%", u32::from(candidate.score_milli) / 10),
    })
    .collect::<Vec<_>>();
    let mut message = String::new();
    if item.edge.is_none() && !cancel.is_requested() {
        let images = prepared_job_sources(job, &workspace).and_then(|prepared| {
            review_image_candidates(
                prepared.epub(),
                &evidence.alignment,
                &evidence.corpus,
                item.alignment_index,
                DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
                2,
                cancel,
            )
        });
        match images {
            Ok(images) => {
                for (index, candidate) in images.into_iter().enumerate() {
                    let hint = candidate
                        .embedded_text
                        .iter()
                        .map(|text| text.trim())
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    let text = if hint.is_empty() {
                        candidate
                            .image_href
                            .rsplit('/')
                            .next()
                            .unwrap_or(&candidate.image_href)
                            .to_string()
                    } else {
                        hint
                    };
                    rows.push(ReviewCandidate {
                        href: candidate.document_href,
                        line_index: -(index as i32 + 1),
                        image_href: Some(candidate.image_href),
                        text: format!("Image narration · {text}"),
                        score: "image".into(),
                    });
                }
            }
            Err(error) => {
                message = format!("Image matches unavailable: {error} Text matches remain usable.")
            }
        }
    }
    Ok((rows, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Instant},
    };
    use storyteller_core::{
        AudioReviewDecision, AudioReviewDecisionSource, JobInputs, JobSettings,
    };

    fn job(title: &str) -> Job {
        Job::new(
            JobInputs {
                title: title.into(),
                epub_path: "book.epub".into(),
                audiobook_path: "book.wav".into(),
                output_path: "output.epub".into(),
            },
            JobSettings::default(),
        )
        .unwrap()
    }
    fn report(title: &str) -> AudioReviewReport {
        AudioReviewReport {
            total_segments: 2,
            matched_segments: 0,
            match_percent: 0.0,
            accepted_unmatched_exclusion: false,
            unmatched: (0..2)
                .map(|index| AudioReviewItem {
                    id: format!("segment-{index}"),
                    alignment_index: index,
                    audio_start_ms: 10_000 * index as u64,
                    audio_end_ms: 10_000 * index as u64 + 2_003,
                    transcript_text: title.into(),
                    suggestion: None,
                    edge: None,
                    silence: None,
                    decision: AudioReviewDecision::Pending,
                })
                .collect(),
        }
    }
    fn loaded(request: &LoadRequest) -> LoadedReview {
        LoadedReview {
            report: Arc::new(report(&request.job.inputs.title)),
            index: request.index,
            candidates: Vec::new(),
            message: String::new(),
        }
    }
    fn session_with_task(task: ReviewTask) -> ReviewSession {
        ReviewSession {
            view: ReviewView::default(),
            job: None,
            generation: 0,
            cancel: CancellationToken::default(),
            task: Some(task),
            preview: crate::review_preview::AudioPreview::default(),
        }
    }
    fn settle(session: &mut ReviewSession) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while session.view.busy {
            session.poll();
            assert!(Instant::now() < deadline, "review worker did not settle");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn switching_books_during_a_held_load_discards_old_evidence() {
        let first = job("first");
        let second = job("second");
        let (entered, entry) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let mut first_load = true;
        let task = spawn_loader(move |request| {
            if first_load {
                first_load = false;
                entered.send(()).unwrap();
                held.recv().unwrap();
            }
            Ok(loaded(request))
        })
        .unwrap();
        let mut session = session_with_task(task);
        session.synchronize(Some(&first));
        entry.recv_timeout(Duration::from_secs(3)).unwrap();
        session.synchronize(Some(&second));
        assert_eq!(session.view.job_id, Some(second.id));
        assert!(session.view.report.is_none());
        assert!(session.view.busy);
        release.send(()).unwrap();
        settle(&mut session);
        assert_eq!(
            session.view.selected_item().unwrap().transcript_text,
            "second"
        );
        session.synchronize(None);
        assert!(session.view.report.is_none());
        assert!(session.view.candidates.is_empty());
    }

    #[test]
    fn seek_is_bounded_and_idle_polling_does_not_reload_or_redraw() {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        let task = spawn_loader(move |request| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(loaded(request))
        })
        .unwrap();
        let mut session = session_with_task(task);
        session.synchronize(Some(&job("seek")));
        settle(&mut session);
        session.action(ReviewAction::Next).unwrap();
        settle(&mut session);
        assert_eq!(session.view.selected_index, 1);
        session
            .action(ReviewAction::SeekRelative(i32::MAX))
            .unwrap();
        assert_eq!(session.view.seek_ms, 2_002);
        session
            .action(ReviewAction::SeekRelative(i32::MIN))
            .unwrap();
        assert_eq!(session.view.seek_ms, 0);
        let revision = session.view.revision;
        for _ in 0..20 {
            session.poll();
        }
        assert_eq!(session.view.revision, revision);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    struct ReviewFixture {
        job: Job,
    }
    impl ReviewFixture {
        fn new() -> Self {
            let job = job("durable");
            let path = crate::audio_review_path(&job);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, serde_json::to_vec(&report("durable")).unwrap()).unwrap();
            Self { job }
        }
    }
    impl Drop for ReviewFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(job_workspace(&self.job).root());
        }
    }

    #[test]
    fn saved_decisions_reload_and_advance_without_requiring_epub_candidates() {
        let fixture = ReviewFixture::new();
        let mut session = ReviewSession::default();
        session.synchronize(Some(&fixture.job));
        settle(&mut session);
        assert!(session.view.message.contains("EPUB matches unavailable"));
        crate::review_service::save_decision(
            &fixture.job,
            "segment-0",
            AudioReviewDecision::Excluded {
                reason: "test".into(),
                source: AudioReviewDecisionSource::Manual,
            },
        )
        .unwrap();
        session.reload_after_decision();
        assert!(session.view.busy);
        assert!(session.view.candidates.is_empty());
        settle(&mut session);
        assert_eq!(session.view.selected_index, 1);
        assert_eq!(session.view.report.as_ref().unwrap().pending_count(), 1);
        assert!(!session.view.report.as_ref().unwrap().is_complete());
        session.shutdown();
    }

    #[test]
    fn broken_report_clears_old_completion_and_can_be_reloaded() {
        let fixture = ReviewFixture::new();
        let mut session = ReviewSession::default();
        session.synchronize(Some(&fixture.job));
        settle(&mut session);
        std::fs::write(crate::audio_review_path(&fixture.job), b"broken").unwrap();
        session.action(ReviewAction::Reload).unwrap();
        settle(&mut session);
        assert!(session.view.report.is_none());
        assert!(session.view.message.contains("Could not load review"));
        std::fs::write(
            crate::audio_review_path(&fixture.job),
            serde_json::to_vec(&report("restored")).unwrap(),
        )
        .unwrap();
        session.action(ReviewAction::Reload).unwrap();
        settle(&mut session);
        assert_eq!(
            session.view.selected_item().unwrap().transcript_text,
            "restored"
        );
        session.shutdown();
    }
}
