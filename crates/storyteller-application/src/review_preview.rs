//! A single application-owned audio process. Spawn, kill, wait and exit detection stay off the UI.
use std::{
    env,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};
use storyteller_core::{prepared_job_sources, AudioReviewItem, Job};

enum PreviewRequest {
    Play {
        generation: u64,
        job: Box<Job>,
        item: Box<AudioReviewItem>,
        seek_ms: u64,
    },
    Stop,
    Shutdown,
}
struct PreviewEvent {
    generation: u64,
    playing: bool,
    message: String,
}
struct PreviewTask {
    sender: Sender<PreviewRequest>,
    receiver: Receiver<PreviewEvent>,
    handle: JoinHandle<()>,
}
#[derive(Default)]
pub(crate) struct AudioPreview {
    generation: u64,
    task: Option<PreviewTask>,
}

impl AudioPreview {
    pub(crate) fn play(
        &mut self,
        job: Job,
        item: AudioReviewItem,
        seek_ms: u64,
    ) -> Result<(), String> {
        self.generation = self.generation.wrapping_add(1);
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
            self.task = Some(spawn_player(start_preview)?);
        }
        self.task
            .as_ref()
            .unwrap()
            .sender
            .send(PreviewRequest::Play {
                generation: self.generation,
                job: Box::new(job),
                item: Box::new(item),
                seek_ms,
            })
            .map_err(|_| "Audio preview worker stopped.".to_string())
    }
    pub(crate) fn stop(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(task) = &self.task {
            let _ = task.sender.send(PreviewRequest::Stop);
        }
    }
    pub(crate) fn poll(&mut self) -> Option<(bool, String)> {
        let mut latest = None;
        if let Some(task) = &self.task {
            for event in task.receiver.try_iter() {
                if event.generation == self.generation {
                    latest = Some((event.playing, event.message));
                }
            }
            if task.handle.is_finished() {
                latest = Some((false, "Audio preview worker stopped.".into()));
            }
        }
        latest
    }
    pub(crate) fn shutdown(&mut self) {
        if let Some(task) = self.task.take() {
            let _ = task.sender.send(PreviewRequest::Shutdown);
            let _ = task.handle.join();
        }
    }
}

fn spawn_player(
    start: fn(&Job, &AudioReviewItem, u64) -> Result<Child, String>,
) -> Result<PreviewTask, String> {
    let (sender, requests) = mpsc::channel();
    let (events, receiver) = mpsc::channel();
    let handle = thread::Builder::new().name("review-preview".into()).spawn(move || {
        let mut child: Option<Child> = None;
        let mut generation = 0;
        loop {
            match requests.recv_timeout(Duration::from_millis(50)) {
                Ok(PreviewRequest::Play { generation: next, job, item, seek_ms }) => {
                    stop_child(&mut child);
                    generation = next;
                    let result = start(&job, &item, seek_ms);
                    let event = match result {
                        Ok(player) => { child = Some(player); PreviewEvent { generation, playing: true, message: "Playing selected audio…".into() } }
                        Err(error) => PreviewEvent { generation, playing: false, message: error },
                    };
                    let _ = events.send(event);
                }
                Ok(PreviewRequest::Stop) => stop_child(&mut child),
                Ok(PreviewRequest::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => { stop_child(&mut child); return; }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(player) = &mut child {
                let event = match player.try_wait() {
                    Ok(Some(status)) => Some(PreviewEvent { generation, playing: false, message: if status.success() { "Preview finished.".into() } else { format!("Audio preview failed ({status}). Check the audio device and ffplay installation.") } }),
                    Err(error) => Some(PreviewEvent { generation, playing: false, message: format!("Could not check audio preview: {error}") }),
                    Ok(None) => None,
                };
                if let Some(event) = event {
                    stop_child(&mut child);
                    let _ = events.send(event);
                }
            }
        }
    }).map_err(|error| format!("Could not start audio preview worker: {error}"))?;
    Ok(PreviewTask {
        sender,
        receiver,
        handle,
    })
}
fn stop_child(child: &mut Option<Child>) {
    if let Some(mut child) = child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
fn start_preview(job: &Job, item: &AudioReviewItem, seek_ms: u64) -> Result<Child, String> {
    let prepared = prepared_job_sources(job, &crate::job_workspace(job))?;
    let start = item.audio_start_ms.saturating_add(seek_ms);
    let duration = item.audio_end_ms.saturating_sub(start);
    if duration == 0 {
        return Err("The preview is at the end of this segment.".into());
    }
    let ffplay = resolve_ffplay().ok_or("Audio preview needs ffplay. Install FFmpeg with ffplay or set STORYTELLER_FFPLAY. You can still review text and make decisions.")?;
    Command::new(ffplay)
        .args(["-nodisp", "-autoexit", "-loglevel", "error", "-ss"])
        .arg(seconds(start))
        .arg("-t")
        .arg(seconds(duration))
        .arg(prepared.audiobook())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start audio preview: {error}"))
}
fn seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}
fn resolve_ffplay() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffplay.exe"
    } else {
        "ffplay"
    };
    let mut paths = Vec::new();
    if let Some(path) = env::var_os("STORYTELLER_FFPLAY") {
        paths.push(PathBuf::from(path));
    }
    if let Some(ffmpeg) = env::var_os("STORYTELLER_FFMPEG").map(PathBuf::from) {
        if let Some(parent) = ffmpeg.parent() {
            paths.push(parent.join(name));
        }
    }
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path).map(|directory| directory.join(name)));
    }
    paths.into_iter().find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use storyteller_core::{AudioReviewDecision, JobInputs, JobSettings};
    fn job() -> Job {
        Job::new(
            JobInputs {
                title: "preview".into(),
                epub_path: "book.epub".into(),
                audiobook_path: "book.wav".into(),
                output_path: "output.epub".into(),
            },
            JobSettings::default(),
        )
        .unwrap()
    }
    fn item() -> AudioReviewItem {
        AudioReviewItem {
            id: "segment".into(),
            alignment_index: 0,
            audio_start_ms: 0,
            audio_end_ms: 5_000,
            transcript_text: "preview".into(),
            suggestion: None,
            edge: None,
            silence: None,
            decision: AudioReviewDecision::Pending,
        }
    }
    fn child(long: bool) -> Result<Child, String> {
        Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "review_preview::tests::preview_child_fixture",
                "--nocapture",
            ])
            .env(
                "STORYTELLER_TEST_PREVIEW_CHILD",
                if long { "long" } else { "short" },
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())
    }
    fn long_child(_: &Job, _: &AudioReviewItem, _: u64) -> Result<Child, String> {
        child(true)
    }
    fn short_child(_: &Job, _: &AudioReviewItem, _: u64) -> Result<Child, String> {
        child(false)
    }
    fn failed_child(_: &Job, _: &AudioReviewItem, _: u64) -> Result<Child, String> {
        Err("fixture missing ffplay".into())
    }
    fn wait_for(preview: &mut AudioPreview, playing: bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some((state, message)) = preview.poll() {
                if state == playing {
                    return message;
                }
            }
            assert!(Instant::now() < deadline, "preview did not report state");
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn preview_child_fixture() {
        if env::var("STORYTELLER_TEST_PREVIEW_CHILD").as_deref() == Ok("long") {
            thread::sleep(Duration::from_secs(30));
        }
    }
    #[test]
    fn natural_audio_exit_updates_playing_state() {
        let mut preview = AudioPreview {
            task: Some(spawn_player(short_child).unwrap()),
            ..Default::default()
        };
        preview.play(job(), item(), 0).unwrap();
        assert_eq!(wait_for(&mut preview, false), "Preview finished.");
        preview.shutdown();
    }
    #[test]
    fn stop_invalidates_pending_events_and_shutdown_reaps_the_real_child() {
        let mut preview = AudioPreview {
            task: Some(spawn_player(long_child).unwrap()),
            ..Default::default()
        };
        preview.play(job(), item(), 0).unwrap();
        wait_for(&mut preview, true);
        preview.stop();
        assert!(preview.poll().is_none());
        let started = Instant::now();
        preview.shutdown();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(preview.task.is_none());
    }
    #[test]
    fn startup_failure_is_visible_without_blocking_review() {
        let mut preview = AudioPreview {
            task: Some(spawn_player(failed_child).unwrap()),
            ..Default::default()
        };
        preview.play(job(), item(), 0).unwrap();
        assert_eq!(wait_for(&mut preview, false), "fixture missing ffplay");
        preview.shutdown();
    }
}
