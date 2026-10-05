use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use storyteller_core::{
    contextual_transcription_windows, merge_contextual_word_transcripts, parse_whisper_words,
    parse_whistle_words, plan_transcription_chunks_for_backend, run_cancellable_command,
    validate_chunk_plan_for_backend, write_transcript, CancellationToken, CommandOutput,
    CommandRunError, CommandStream, TimedWord, TranscriptionBackend, TranscriptionChunk,
    TranscriptionChunkPolicy, TranscriptionWindow, WhisperNativeProgress, WhisperProgressTracker,
};

const SILENCE_SEARCH_RADIUS_MS: u64 = 2_500;
const SILENCE_MIN_DURATION_SECONDS: f64 = 0.35;
const SILENCE_NOISE_DB: i32 = -40;
// Eight <=307.5-second mono PCM files use at most 78.8 MB of temporary audio.
// Inference remains sequential in one CLI/GPU context, not eight GPU workers.
const WHISPER_CHUNKS_PER_INVOCATION: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkedTranscriptionSummary {
    pub duration_ms: u64,
    pub chunks: usize,
    pub effective_workers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkedTranscriptionProgress {
    pub completed_chunks: usize,
    pub total_chunks: usize,
    pub percent: u8,
    pub backend: Option<String>,
    /// Live per-input work, even when the overall integer percent is unchanged.
    pub input_progress: Option<WhisperNativeProgress>,
}

#[derive(Debug, Clone)]
pub struct ChunkedTranscriptionConfig {
    pub ffmpeg: PathBuf,
    pub engine: TranscriptionEngine,
    pub workers: usize,
}

#[derive(Debug, Clone)]
pub enum TranscriptionEngine {
    Whistle { executable: PathBuf, model: PathBuf },
    WhisperCuda { executable: PathBuf, model: PathBuf },
}

impl TranscriptionEngine {
    fn backend(&self) -> TranscriptionBackend {
        match self {
            Self::Whistle { .. } => TranscriptionBackend::Whistle,
            Self::WhisperCuda { .. } => TranscriptionBackend::WhisperCuda,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Whistle { .. } => "Whistle / native CPU",
            Self::WhisperCuda { .. } => "Whisper Turbo / NVIDIA GPU",
        }
    }
}

#[derive(Debug)]
enum WorkerEvent {
    NativeProgress {
        input_index: usize,
        percent: u8,
    },
    Completed {
        window: TranscriptionWindow,
        words: Vec<TimedWord>,
    },
    Failed(String),
}

pub fn transcribe_audiobook_in_chunks(
    source: &Path,
    stage_dir: &Path,
    transcript_path: &Path,
    config: &ChunkedTranscriptionConfig,
    cancellation: &CancellationToken,
    observer: &mut dyn FnMut(ChunkedTranscriptionProgress) -> Result<(), String>,
) -> Result<ChunkedTranscriptionSummary, String> {
    if matches!(config.engine, TranscriptionEngine::WhisperCuda { .. }) && config.workers != 1 {
        return Err("Whisper GPU uses exactly one transcription worker.".into());
    }
    if !(1..=storyteller_core::MAX_TRANSCRIPTION_WORKERS).contains(&config.workers) {
        return Err(format!(
            "Transcription worker count must be between 1 and {}.",
            storyteller_core::MAX_TRANSCRIPTION_WORKERS
        ));
    }
    if cancellation.is_requested() {
        return Err("Transcription was cancelled.".into());
    }

    let metadata = probe_audio_metadata(source, &config.ffmpeg, cancellation)?;
    let backend = config.engine.backend();
    let mut chunks = plan_transcription_chunks_for_backend(
        metadata.duration_ms,
        &metadata.chapter_boundaries_ms,
        backend,
    )?;
    refine_synthetic_boundaries(
        source,
        &config.ffmpeg,
        metadata.duration_ms,
        &metadata.chapter_boundaries_ms,
        &mut chunks,
        backend,
        cancellation,
    )?;
    validate_chunk_plan_for_backend(metadata.duration_ms, &chunks, backend)?;
    let windows = contextual_transcription_windows(metadata.duration_ms, &chunks, backend)?;
    write_chunk_plan(
        &stage_dir.join("transcription-plan.json"),
        metadata.duration_ms,
        &chunks,
        &windows,
        backend,
    )?;

    let effective_workers = config.workers.min(chunks.len()).max(1);
    let temporary_dir = stage_dir.join("transcription-chunks.tmp");
    if temporary_dir.exists() {
        fs::remove_dir_all(&temporary_dir).map_err(|error| {
            format!(
                "Could not reset temporary transcription chunks {}: {error}",
                temporary_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&temporary_dir).map_err(|error| {
        format!(
            "Could not create temporary transcription chunk folder {}: {error}",
            temporary_dir.display()
        )
    })?;

    let result = run_chunk_workers(
        source,
        &temporary_dir,
        &windows,
        config,
        effective_workers,
        cancellation,
        observer,
    )
    .and_then(|parts| {
        let merged = merge_contextual_word_transcripts(metadata.duration_ms, &parts, backend)?;
        write_transcript(transcript_path, &merged)
    });

    let cleanup = fs::remove_dir_all(&temporary_dir);
    if let Err(error) = result {
        let _ = cleanup;
        return Err(error);
    }
    cleanup.map_err(|error| {
        format!(
            "Could not remove temporary transcription chunks {}: {error}",
            temporary_dir.display()
        )
    })?;

    Ok(ChunkedTranscriptionSummary {
        duration_ms: metadata.duration_ms,
        chunks: chunks.len(),
        effective_workers,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_chunk_workers(
    source: &Path,
    temporary_dir: &Path,
    windows: &[TranscriptionWindow],
    config: &ChunkedTranscriptionConfig,
    effective_workers: usize,
    cancellation: &CancellationToken,
    observer: &mut dyn FnMut(ChunkedTranscriptionProgress) -> Result<(), String>,
) -> Result<Vec<(TranscriptionWindow, Vec<TimedWord>)>, String> {
    let queue = Arc::new(Mutex::new(VecDeque::from(windows.to_vec())));
    let worker_cancellation = CancellationToken::default();
    let (sender, receiver) = mpsc::channel::<WorkerEvent>();
    let mut handles = Vec::with_capacity(effective_workers);

    for _ in 0..effective_workers {
        let queue = Arc::clone(&queue);
        let sender = sender.clone();
        let source = source.to_path_buf();
        let temporary_dir = temporary_dir.to_path_buf();
        let config = config.clone();
        let worker_cancellation = worker_cancellation.clone();
        handles.push(thread::spawn(move || loop {
            if worker_cancellation.is_requested() {
                return;
            }
            let windows = match queue.lock() {
                Ok(mut queue) => {
                    let limit = match config.engine {
                        TranscriptionEngine::Whistle { .. } => 1,
                        TranscriptionEngine::WhisperCuda { .. } => WHISPER_CHUNKS_PER_INVOCATION,
                    };
                    let count = queue.len().min(limit);
                    queue.drain(..count).collect::<Vec<_>>()
                }
                Err(_) => {
                    worker_cancellation.request();
                    let _ = sender.send(WorkerEvent::Failed(
                        "Transcription worker queue lock was poisoned.".into(),
                    ));
                    return;
                }
            };
            if windows.is_empty() {
                return;
            }
            match transcribe_chunk_batch(
                &source,
                &temporary_dir,
                &windows,
                &config,
                &worker_cancellation,
                &mut |input_index, percent| {
                    if sender
                        .send(WorkerEvent::NativeProgress {
                            input_index,
                            percent,
                        })
                        .is_err()
                    {
                        worker_cancellation.request();
                    }
                },
            ) {
                Ok(parts) => {
                    for (window, words) in parts {
                        if sender
                            .send(WorkerEvent::Completed { window, words })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                Err(error) => {
                    worker_cancellation.request();
                    let _ = sender.send(WorkerEvent::Failed(error));
                    return;
                }
            }
        }));
    }
    drop(sender);

    let mut result = collect_worker_results(
        windows,
        &receiver,
        config.engine.label(),
        cancellation,
        &worker_cancellation,
        observer,
    );
    // Always stop and join owned processes before the caller removes PCM.
    for handle in handles {
        if handle.join().is_err() && result.is_ok() {
            result = Err("A transcription worker thread panicked.".into());
        }
    }
    if cancellation.is_requested() {
        return Err("Transcription was cancelled.".into());
    }
    result
}

fn collect_worker_results(
    windows: &[TranscriptionWindow],
    receiver: &mpsc::Receiver<WorkerEvent>,
    backend: &'static str,
    cancellation: &CancellationToken,
    worker_cancellation: &CancellationToken,
    observer: &mut dyn FnMut(ChunkedTranscriptionProgress) -> Result<(), String>,
) -> Result<Vec<(TranscriptionWindow, Vec<TimedWord>)>, String> {
    let mut meter = OwnedAudioProgress::new(windows, backend);
    let mut completed = 0usize;
    let mut parts = vec![None::<(TranscriptionWindow, Vec<TimedWord>)>; windows.len()];
    let mut first_error = None::<String>;
    let mut last_progress = None;

    while completed < windows.len() && first_error.is_none() {
        if cancellation.is_requested() {
            worker_cancellation.request();
            first_error = Some("Transcription was cancelled.".into());
            break;
        }
        let event = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if cancellation.is_requested() {
                    worker_cancellation.request();
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                worker_cancellation.request();
                first_error =
                    Some("Transcription workers stopped before all chunks completed.".into());
                break;
            }
        };
        let progress = match event {
            WorkerEvent::NativeProgress {
                input_index,
                percent,
            } => meter.update(input_index, percent, false),
            WorkerEvent::Completed { window, words } => {
                let index = window.owned.index;
                if windows.get(index) != Some(&window) || parts[index].is_some() {
                    Err("Transcription worker returned an unknown or repeated input.".into())
                } else {
                    parts[index] = Some((window, words));
                    completed += 1;
                    meter.update(index, 100, true)
                }
            }
            WorkerEvent::Failed(error) => Err(error),
        };
        match progress {
            Ok(progress) if last_progress.as_ref() != Some(&progress) => {
                last_progress = Some(progress.clone());
                if let Err(error) = observer(progress) {
                    worker_cancellation.request();
                    first_error = Some(error);
                }
            }
            Ok(_) => {}
            Err(error) => {
                worker_cancellation.request();
                first_error = Some(error);
            }
        }
    }

    if cancellation.is_requested() {
        return Err("Transcription was cancelled.".into());
    }
    if let Some(error) = first_error {
        return Err(error);
    }

    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            part.ok_or_else(|| {
                format!("Transcription chunk {} did not return a result.", index + 1)
            })
        })
        .collect()
}

struct OwnedAudioProgress<'a> {
    windows: &'a [TranscriptionWindow],
    decoded_ms: Vec<u64>,
    verified: Vec<bool>,
    decoded_audio_ms: u64,
    total_audio_ms: u64,
    completed_chunks: usize,
    latest_native_progress: Option<WhisperNativeProgress>,
    backend: &'static str,
}

impl<'a> OwnedAudioProgress<'a> {
    fn new(windows: &'a [TranscriptionWindow], backend: &'static str) -> Self {
        Self {
            windows,
            decoded_ms: vec![0; windows.len()],
            verified: vec![false; windows.len()],
            decoded_audio_ms: 0,
            total_audio_ms: windows
                .iter()
                .map(|window| window.owned.duration_ms())
                .sum(),
            completed_chunks: 0,
            latest_native_progress: None,
            backend,
        }
    }

    fn update(
        &mut self,
        index: usize,
        input_percent: u8,
        verified: bool,
    ) -> Result<ChunkedTranscriptionProgress, String> {
        let window = self
            .windows
            .get(index)
            .ok_or("Unknown transcription progress input.")?;
        if input_percent > 100 || self.total_audio_ms == 0 {
            return Err("Invalid transcription progress measurement.".into());
        }
        let decoded_input_ms = (window.duration_ms() as u128 * input_percent as u128 / 100) as u64;
        let owned_ms = decoded_input_ms
            .saturating_sub(window.owned.start_ms - window.start_ms)
            .min(window.owned.duration_ms());
        let previous = self.decoded_ms[index];
        self.decoded_ms[index] = previous.max(owned_ms);
        self.decoded_audio_ms += self.decoded_ms[index] - previous;
        if verified && !self.verified[index] {
            self.verified[index] = true;
            self.completed_chunks += 1;
        }
        if !verified
            && !self.verified[index]
            && self.latest_native_progress.is_none_or(|previous| {
                index > previous.input_index
                    || (index == previous.input_index && input_percent >= previous.percent)
            })
        {
            self.latest_native_progress = Some(WhisperNativeProgress {
                input_index: index,
                percent: input_percent,
            });
        }
        let percent = (self.decoded_audio_ms as u128 * 100 / self.total_audio_ms as u128) as u8;
        Ok(ChunkedTranscriptionProgress {
            completed_chunks: self.completed_chunks,
            total_chunks: self.windows.len(),
            // Native decoder progress is provisional. Analyze reaches 100
            // only after merge, transcript write and temporary cleanup succeed.
            percent: percent.min(99),
            backend: Some(self.backend.into()),
            input_progress: if verified {
                None
            } else {
                self.latest_native_progress
            },
        })
    }
}

fn convert_chunk_audio(
    source: &Path,
    wav_path: &Path,
    chunk: TranscriptionChunk,
    ffmpeg_path: &Path,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let start_seconds = chunk.start_ms as f64 / 1000.0;
    let duration_seconds = chunk.duration_ms() as f64 / 1000.0;
    let mut ffmpeg = Command::new(ffmpeg_path);
    ffmpeg
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{start_seconds:.3}"))
        .args(["-threads", "1"])
        .arg("-i")
        .arg(source)
        .arg("-t")
        .arg(format!("{duration_seconds:.3}"))
        .arg("-map")
        .arg("0:a:0")
        .arg("-vn")
        .arg("-ar")
        .arg("16000")
        .arg("-ac")
        .arg("1")
        .arg("-c:a")
        .arg("pcm_s16le")
        .args(["-threads", "1"])
        .arg(wav_path);
    match run_cancellable_command(&mut ffmpeg, cancellation, |_, _| {}) {
        Ok(output) if output.success => {}
        Ok(output) => {
            return Err(command_failure(
                "ffmpeg transcription chunk conversion",
                &output,
            ))
        }
        Err(CommandRunError::Cancelled) => return Err("Transcription was cancelled.".into()),
        Err(error) => return Err(format!("Could not convert transcription chunk: {error}")),
    }
    validate_nonempty_file(wav_path, "Converted transcription chunk")
}

fn transcribe_chunk_batch(
    source: &Path,
    temporary_dir: &Path,
    windows: &[TranscriptionWindow],
    config: &ChunkedTranscriptionConfig,
    cancellation: &CancellationToken,
    on_progress: &mut dyn FnMut(usize, u8),
) -> Result<Vec<(TranscriptionWindow, Vec<TimedWord>)>, String> {
    let whisper = matches!(config.engine, TranscriptionEngine::WhisperCuda { .. });
    let limit = if whisper {
        WHISPER_CHUNKS_PER_INVOCATION
    } else {
        1
    };
    if windows.is_empty() || windows.len() > limit {
        return Err("Invalid transcription batch size.".into());
    }
    let mut inputs = Vec::with_capacity(windows.len());
    for &window in windows {
        if cancellation.is_requested() {
            return Err("Transcription was cancelled.".into());
        }
        let wav = temporary_dir.join(format!("chunk-{:05}.wav", window.owned.index));
        convert_chunk_audio(
            source,
            &wav,
            window.input_chunk(),
            &config.ffmpeg,
            cancellation,
        )?;
        let prefix = temporary_dir.join(format!("whisper-{:05}", window.owned.index));
        inputs.push((wav, prefix));
    }
    if cancellation.is_requested() {
        return Err("Transcription was cancelled.".into());
    }
    let mut command = match &config.engine {
        TranscriptionEngine::Whistle { executable, model } => {
            crate::runtime_setup::whistle_command(executable, model, &inputs[0].0)
        }
        TranscriptionEngine::WhisperCuda { executable, model } => {
            crate::whisper_runtime::whisper_command(executable, model, &inputs)
        }
    };
    let mut progress = WhisperProgressTracker::new(&inputs);
    let output = run_transcription_command(
        &mut command,
        config.engine.label(),
        whisper,
        cancellation,
        &mut |stream, line| {
            if whisper {
                if let Some(event) = progress.observe(stream, line) {
                    on_progress(windows[event.input_index].owned.index, event.percent);
                }
            }
        },
    )?;
    let result = if whisper {
        read_whisper_batch(windows, &inputs, &output)
    } else {
        parse_whistle_words(&output.stdout, windows[0].duration_ms())
            .map(|words| vec![(windows[0], words)])
    };
    for (wav, prefix) in inputs {
        let _ = fs::remove_file(wav);
        if whisper {
            let _ = fs::remove_file(prefix.with_extension("json"));
        }
    }
    result
}

fn run_transcription_command(
    command: &mut Command,
    label: &str,
    whisper: bool,
    cancellation: &CancellationToken,
    on_line: &mut dyn FnMut(CommandStream, &str),
) -> Result<CommandOutput, String> {
    let mut gpu_failed = false;
    let result = run_cancellable_command(command, cancellation, |stream, line| {
        if whisper
            && stream == CommandStream::Stderr
            && crate::whisper_runtime::cuda_initialization_failed(line)
        {
            gpu_failed = true;
            cancellation.request();
        }
        if !gpu_failed {
            on_line(stream, line);
        }
    });
    if gpu_failed {
        return Err("Whisper GPU initialization failed; CPU fallback was stopped. Check your NVIDIA driver and free VRAM, or select Whistle for a new book.".into());
    }
    match result {
        Ok(output) if output.success => Ok(output),
        Ok(output) => Err(command_failure(label, &output)),
        Err(CommandRunError::Cancelled) => Err("Transcription was cancelled.".into()),
        Err(error) => Err(format!("Could not transcribe audio: {error}")),
    }
}

fn read_whisper_batch(
    windows: &[TranscriptionWindow],
    inputs: &[(PathBuf, PathBuf)],
    output: &CommandOutput,
) -> Result<Vec<(TranscriptionWindow, Vec<TimedWord>)>, String> {
    if windows.is_empty() || windows.len() != inputs.len() {
        return Err("Whisper batch inputs and outputs do not match.".into());
    }
    crate::whisper_runtime::require_cuda_offload(&output.stderr)?;
    // The CLI can exit successfully after skipping an unreadable input or
    // failing to open an output. Require every paired JSON before completing
    // any chunk from this batch, rather than trusting the process exit code.
    windows
        .iter()
        .zip(inputs)
        .map(|(&window, (_, prefix))| {
            let json = fs::read_to_string(prefix.with_extension("json")).map_err(|error| {
                format!(
                    "Could not read Whisper JSON for chunk {}: {error}",
                    window.owned.index + 1
                )
            })?;
            parse_whisper_words(&json, window.duration_ms()).map(|words| (window, words))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioMetadata {
    duration_ms: u64,
    chapter_boundaries_ms: Vec<u64>,
}

fn probe_audio_metadata(
    source: &Path,
    ffmpeg: &Path,
    cancellation: &CancellationToken,
) -> Result<AudioMetadata, String> {
    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-i")
        .arg(source)
        .arg("-map_metadata")
        .arg("0")
        .arg("-f")
        .arg("ffmetadata")
        .arg("-");
    let output = match run_cancellable_command(&mut command, cancellation, |_, _| {}) {
        Ok(output) if output.success => output,
        Ok(output) => {
            let duration = parse_ffmpeg_duration_ms(&output.stderr);
            if duration.is_none() {
                return Err(command_failure("ffmpeg audiobook metadata probe", &output));
            }
            output
        }
        Err(CommandRunError::Cancelled) => {
            return Err("Audiobook metadata probe was cancelled.".into())
        }
        Err(error) => return Err(format!("Could not probe audiobook metadata: {error}")),
    };
    let duration_ms = parse_ffmpeg_duration_ms(&output.stderr)
        .ok_or("ffmpeg did not report a usable audiobook duration.")?;
    let mut chapter_boundaries_ms = parse_ffmetadata_chapter_ends(&output.stdout);
    chapter_boundaries_ms.retain(|boundary| *boundary > 0 && *boundary < duration_ms);
    chapter_boundaries_ms.sort_unstable();
    chapter_boundaries_ms.dedup();
    Ok(AudioMetadata {
        duration_ms,
        chapter_boundaries_ms,
    })
}

fn refine_synthetic_boundaries(
    source: &Path,
    ffmpeg: &Path,
    duration_ms: u64,
    chapter_boundaries_ms: &[u64],
    chunks: &mut [TranscriptionChunk],
    backend: TranscriptionBackend,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let max_owned_ms = TranscriptionChunkPolicy::for_backend(backend).max_owned_ms;
    let chapters = chapter_boundaries_ms
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    if chunks.len() <= 1 {
        return Ok(());
    }
    let mut boundaries = chunks.iter().map(|chunk| chunk.end_ms).collect::<Vec<_>>();
    for index in 0..boundaries.len() - 1 {
        let target = boundaries[index];
        if chapters.contains(&target) {
            continue;
        }
        if let Some(refined) =
            find_nearby_silence_cut(source, ffmpeg, target, duration_ms, cancellation)?
        {
            let previous = if index == 0 { 0 } else { boundaries[index - 1] };
            let next = boundaries[index + 1];
            if refined > previous
                && refined < next
                && refined - previous <= max_owned_ms
                && next - refined <= max_owned_ms
            {
                boundaries[index] = refined;
            }
        }
    }

    let mut start_ms = 0u64;
    for (index, chunk) in chunks.iter_mut().enumerate() {
        chunk.start_ms = start_ms;
        chunk.end_ms = boundaries[index];
        start_ms = chunk.end_ms;
    }
    validate_chunk_plan_for_backend(duration_ms, chunks, backend)
}

fn find_nearby_silence_cut(
    source: &Path,
    ffmpeg: &Path,
    target_ms: u64,
    duration_ms: u64,
    cancellation: &CancellationToken,
) -> Result<Option<u64>, String> {
    let search_start_ms = target_ms.saturating_sub(SILENCE_SEARCH_RADIUS_MS);
    let search_end_ms = target_ms
        .saturating_add(SILENCE_SEARCH_RADIUS_MS)
        .min(duration_ms);
    if search_end_ms <= search_start_ms {
        return Ok(None);
    }
    let search_duration_ms = search_end_ms - search_start_ms;
    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-ss")
        .arg(format!("{:.3}", search_start_ms as f64 / 1000.0))
        .arg("-i")
        .arg(source)
        .arg("-t")
        .arg(format!("{:.3}", search_duration_ms as f64 / 1000.0))
        .arg("-map")
        .arg("0:a:0")
        .arg("-vn")
        .arg("-af")
        .arg(format!(
            "asetpts=PTS-STARTPTS,silencedetect=noise={}dB:d={SILENCE_MIN_DURATION_SECONDS}",
            SILENCE_NOISE_DB
        ))
        .arg("-f")
        .arg("null")
        .arg("-");
    let output = match run_cancellable_command(&mut command, cancellation, |_, _| {}) {
        Ok(output) if output.success => output,
        Ok(output) => return Err(command_failure("ffmpeg silence boundary search", &output)),
        Err(CommandRunError::Cancelled) => {
            return Err("Silence boundary search was cancelled.".into())
        }
        Err(error) => {
            return Err(format!(
                "Could not search for a safe audio boundary: {error}"
            ))
        }
    };
    let intervals = parse_silence_intervals(&output.stderr);
    let target_local_ms = target_ms.saturating_sub(search_start_ms);
    let best = intervals
        .into_iter()
        .filter(|(start, end)| end > start)
        .map(|(start, end)| {
            let midpoint = start + (end - start) / 2;
            let distance = midpoint.abs_diff(target_local_ms);
            (distance, midpoint)
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, midpoint)| search_start_ms.saturating_add(midpoint));
    Ok(best)
}

fn parse_ffmpeg_duration_ms(text: &str) -> Option<u64> {
    let line = text.lines().find(|line| line.contains("Duration:"))?;
    let (_, after) = line.split_once("Duration:")?;
    let clock = after.split(',').next()?.trim();
    parse_clock_ms(clock)
}

fn parse_clock_ms(clock: &str) -> Option<u64> {
    let mut fields = clock.split(':');
    let hours = fields.next()?.trim().parse::<u64>().ok()?;
    let minutes = fields.next()?.trim().parse::<u64>().ok()?;
    let seconds = fields.next()?.trim().parse::<f64>().ok()?;
    if fields.next().is_some() || !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let total = hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds;
    if total < 0.0 || total > u64::MAX as f64 / 1000.0 {
        return None;
    }
    Some((total * 1000.0).round() as u64)
}

fn parse_ffmetadata_chapter_ends(text: &str) -> Vec<u64> {
    let mut ends = Vec::new();
    let mut in_chapter = false;
    let mut numerator = 1u64;
    let mut denominator = 1000u64;
    let mut end = None::<u64>;

    let flush = |ends: &mut Vec<u64>,
                 in_chapter: bool,
                 numerator: u64,
                 denominator: u64,
                 end: Option<u64>| {
        if !in_chapter || denominator == 0 {
            return;
        }
        let Some(end) = end else {
            return;
        };
        let millis = (end as u128)
            .saturating_mul(numerator as u128)
            .saturating_mul(1000)
            / denominator as u128;
        if millis <= u64::MAX as u128 {
            ends.push(millis as u64);
        }
    };

    for line in text.lines().chain(std::iter::once("[END]")) {
        let line = line.trim();
        if line.starts_with('[') {
            flush(&mut ends, in_chapter, numerator, denominator, end);
            in_chapter = line == "[CHAPTER]";
            numerator = 1;
            denominator = 1000;
            end = None;
            continue;
        }
        if !in_chapter {
            continue;
        }
        if let Some(value) = line.strip_prefix("TIMEBASE=") {
            if let Some((left, right)) = value.split_once('/') {
                if let (Ok(left), Ok(right)) = (left.parse::<u64>(), right.parse::<u64>()) {
                    numerator = left;
                    denominator = right;
                }
            }
        } else if let Some(value) = line.strip_prefix("END=") {
            end = value.parse::<u64>().ok();
        }
    }
    ends
}

fn parse_silence_intervals(text: &str) -> Vec<(u64, u64)> {
    let mut intervals = Vec::new();
    let mut current_start = None::<f64>;
    for line in text.lines() {
        if let Some((_, value)) = line.split_once("silence_start:") {
            let value = value.split_whitespace().next().unwrap_or_default();
            current_start = value.parse::<f64>().ok();
            continue;
        }
        if let Some((_, value)) = line.split_once("silence_end:") {
            let value = value.split_whitespace().next().unwrap_or_default();
            let end = value.parse::<f64>().ok();
            if let (Some(start), Some(end)) = (current_start.take(), end) {
                if start.is_finite() && end.is_finite() && end > start {
                    intervals.push((
                        (start * 1000.0).round() as u64,
                        (end * 1000.0).round() as u64,
                    ));
                }
            }
        }
    }
    intervals
}

#[derive(serde::Serialize)]
struct ChunkPlanFile<'a> {
    duration_ms: u64,
    chunks: &'a [TranscriptionChunk],
    inference_windows: &'a [TranscriptionWindow],
    backend: TranscriptionBackend,
    policy: TranscriptionChunkPolicy,
}

fn write_chunk_plan(
    path: &Path,
    duration_ms: u64,
    chunks: &[TranscriptionChunk],
    inference_windows: &[TranscriptionWindow],
    backend: TranscriptionBackend,
) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(&ChunkPlanFile {
        duration_ms,
        chunks,
        inference_windows,
        backend,
        policy: TranscriptionChunkPolicy::for_backend(backend),
    })
    .map_err(|error| format!("Could not serialize transcription chunk plan: {error}"))?;
    fs::write(path, json).map_err(|error| {
        format!(
            "Could not write transcription chunk plan {}: {error}",
            path.display()
        )
    })
}

fn validate_nonempty_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("{label} is unavailable at {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "{label} is not a non-empty regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn command_failure(label: &str, output: &CommandOutput) -> String {
    let diagnostics = diagnostic_tail(if output.stderr.trim().is_empty() {
        &output.stdout
    } else {
        &output.stderr
    });
    let exit = output
        .exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".into());
    if diagnostics.is_empty() {
        format!("{label} failed with exit code {exit}.")
    } else {
        format!("{label} failed with exit code {exit}: {diagnostics}")
    }
}

fn diagnostic_tail(text: &str) -> String {
    let mut lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(8)
        .collect::<Vec<_>>();
    lines.reverse();
    lines.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };

    struct BatchFixture {
        root: PathBuf,
        windows: Vec<TranscriptionWindow>,
        inputs: Vec<(PathBuf, PathBuf)>,
        output: CommandOutput,
    }

    impl BatchFixture {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let root = env::temp_dir().join(format!(
                "storyteller-whisper-batch-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            let chunks = (0..3)
                .map(|index| TranscriptionChunk {
                    index,
                    start_ms: index as u64 * 20_000,
                    end_ms: (index as u64 + 1) * 20_000,
                })
                .collect::<Vec<_>>();
            let windows = contextual_transcription_windows(
                60_000,
                &chunks,
                TranscriptionBackend::WhisperCuda,
            )
            .unwrap();
            let inputs = chunks
                .iter()
                .map(|chunk| {
                    (
                        root.join(format!("chunk {}.wav", chunk.index)),
                        root.join(format!("result {}", chunk.index)),
                    )
                })
                .collect::<Vec<_>>();
            for (index, (_, prefix)) in inputs.iter().enumerate() {
                let segments = match index {
                    0 => {
                        r#"[{"offsets":{"from":100,"to":900},"text":"Lighthouse.","tokens":[{"text":"Lighthouse.","offsets":{"from":100,"to":900},"p":0.9}]}]"#
                    }
                    1 => "[]",
                    _ => {
                        r#"[{"offsets":{"from":2600,"to":3400},"text":"Harbor.","tokens":[{"text":"Harbor.","offsets":{"from":2600,"to":3400},"p":0.9}]}]"#
                    }
                };
                fs::write(prefix.with_extension("json"), format!(
                    r#"{{"params":{{"language":"en","translate":false}},"result":{{"language":"en"}},"transcription":{segments}}}"#
                )).unwrap();
            }
            Self {
                root, windows, inputs,
                output: CommandOutput {
                    success: true,
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: "whisper_backend_init_gpu: using CUDA0 backend\nwhisper_model_load: CUDA0 total size = 580.00 MB\n".into(),
                },
            }
        }
    }

    impl Drop for BatchFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn batch_retains_each_files_local_timing_and_silent_middle_chunk() {
        let fixture = BatchFixture::new();
        let parts = read_whisper_batch(&fixture.windows, &fixture.inputs, &fixture.output).unwrap();
        assert_eq!(parts.len(), 3);
        assert!(parts[1].1.is_empty());
        let merged =
            merge_contextual_word_transcripts(60_000, &parts, TranscriptionBackend::WhisperCuda)
                .unwrap();
        assert_eq!(merged.segments.len(), 2);
        assert_eq!(merged.segments[0].text, "Lighthouse.");
        assert_eq!(merged.segments[1].text, "Harbor.");
        assert_eq!(merged.segments[1].start_ms, 40_100);
        assert_eq!(merged.segments[1].end_ms, 40_900);
    }

    #[test]
    fn successful_process_cannot_hide_missing_or_malformed_batch_outputs() {
        let fixture = BatchFixture::new();
        fs::remove_file(fixture.inputs[1].1.with_extension("json")).unwrap();
        let error =
            read_whisper_batch(&fixture.windows, &fixture.inputs, &fixture.output).unwrap_err();
        assert!(error.contains("chunk 2"));
        fs::write(fixture.inputs[1].1.with_extension("json"), "{}").unwrap();
        assert!(read_whisper_batch(&fixture.windows, &fixture.inputs, &fixture.output).is_err());
    }

    #[test]
    fn owned_audio_progress_excludes_context_and_never_accepts_decoder_work() {
        let fixture = BatchFixture::new();
        let mut meter = OwnedAudioProgress::new(&fixture.windows, "Whisper");
        assert_eq!(meter.update(0, 50, false).unwrap().percent, 18);
        let middle = meter.update(1, 50, false).unwrap();
        assert_eq!(middle.percent, 35);
        assert_eq!(middle.completed_chunks, 0);
        assert_eq!(meter.update(0, 10, false).unwrap(), middle);
        assert_eq!(meter.update(0, 100, true).unwrap().percent, 50);
        assert_eq!(meter.update(1, 100, false).unwrap().percent, 66);
        let decoded = meter.update(2, 100, false).unwrap();
        assert_eq!(decoded.percent, 99);
        assert_eq!(decoded.completed_chunks, 1);
        assert_eq!(meter.update(1, 100, true).unwrap().completed_chunks, 2);
        assert_eq!(meter.update(1, 100, true).unwrap().completed_chunks, 2);
        let checked = meter.update(2, 100, true).unwrap();
        assert_eq!(checked.percent, 99);
        assert_eq!(checked.completed_chunks, 3);
    }

    #[test]
    fn hours_long_book_reports_live_part_work_before_overall_percent_changes() {
        let chunks = plan_transcription_chunks_for_backend(
            7_200_000,
            &[],
            TranscriptionBackend::WhisperCuda,
        )
        .unwrap();
        let windows =
            contextual_transcription_windows(7_200_000, &chunks, TranscriptionBackend::WhisperCuda)
                .unwrap();
        let mut meter = OwnedAudioProgress::new(&windows, "Whisper");
        let first = meter.update(0, 5, false).unwrap();
        let next = meter.update(0, 10, false).unwrap();
        assert_eq!(first.percent, 0);
        assert_eq!(next.percent, 0);
        assert_eq!(next.completed_chunks, 0);
        assert_ne!(first, next);
        assert_eq!(
            next.input_progress,
            Some(WhisperNativeProgress {
                input_index: 0,
                percent: 10
            })
        );
        assert!(meter.update(0, 100, true).unwrap().input_progress.is_none());
    }

    #[test]
    fn provisional_progress_cannot_accept_a_batch_with_a_missing_json() {
        let fixture = BatchFixture::new();
        fs::remove_file(fixture.inputs[1].1.with_extension("json")).unwrap();
        let (sender, receiver) = mpsc::channel();
        for index in 0..3 {
            sender
                .send(WorkerEvent::NativeProgress {
                    input_index: index,
                    percent: 100,
                })
                .unwrap();
        }
        let error =
            read_whisper_batch(&fixture.windows, &fixture.inputs, &fixture.output).unwrap_err();
        sender.send(WorkerEvent::Failed(error)).unwrap();
        let mut observed = Vec::new();
        let cancellation = CancellationToken::default();
        let worker_cancellation = CancellationToken::default();
        let error = collect_worker_results(
            &fixture.windows,
            &receiver,
            "Whisper",
            &cancellation,
            &worker_cancellation,
            &mut |progress| {
                observed.push(progress);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("chunk 2"));
        assert!(worker_cancellation.is_requested());
        assert_eq!(observed.last().unwrap().percent, 99);
        assert!(observed
            .iter()
            .all(|p| p.completed_chunks == 0 && p.percent < 100));
    }

    #[test]
    fn narrator_stdout_cannot_supply_cuda_proof() {
        let mut fixture = BatchFixture::new();
        fixture.output.stdout = fixture.output.stderr.clone();
        fixture.output.stderr = "whisper_backend_init_gpu: using CPU backend".into();
        assert!(read_whisper_batch(&fixture.windows, &fixture.inputs, &fixture.output).is_err());
    }

    #[test]
    fn live_progress_and_observer_failure_cancel_a_running_owned_process() {
        for cancel_on_progress in [true, false] {
            let fixture = BatchFixture::new();
            let finish_path = fixture.root.join("child-finished.txt");
            let mut command = Command::new(env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "chunked_transcription::tests::progress_child_fixture",
                    "--nocapture",
                ])
                .env("STORYTELLER_TEST_TRANSCRIPTION_CHILD", "progress")
                .env("STORYTELLER_TEST_PROGRESS_AUDIO", &fixture.inputs[0].0)
                .env("STORYTELLER_TEST_PROGRESS_FINISH", &finish_path);
            let cancellation = CancellationToken::default();
            let worker_cancellation = CancellationToken::default();
            let worker_token = worker_cancellation.clone();
            let inputs = fixture.inputs.clone();
            let (sender, receiver) = mpsc::channel();
            let started = Instant::now();
            let worker = thread::spawn(move || {
                let mut tracker = WhisperProgressTracker::new(&inputs);
                let result = run_transcription_command(
                    &mut command,
                    "Whisper",
                    true,
                    &worker_token,
                    &mut |stream, line| {
                        if let Some(event) = tracker.observe(stream, line) {
                            sender
                                .send(WorkerEvent::NativeProgress {
                                    input_index: event.input_index,
                                    percent: event.percent,
                                })
                                .unwrap();
                        }
                    },
                );
                if let Err(error) = &result {
                    sender.send(WorkerEvent::Failed(error.clone())).unwrap();
                }
                result
            });
            let mut saw_live_progress = false;
            let error = collect_worker_results(
                &fixture.windows,
                &receiver,
                "Whisper",
                &cancellation,
                &worker_cancellation,
                &mut |progress| {
                    if progress.percent > 0 {
                        saw_live_progress = true;
                        assert_eq!(progress.percent, 18);
                        assert_eq!(progress.completed_chunks, 0);
                        assert!(!finish_path.exists());
                        if cancel_on_progress {
                            cancellation.request();
                        } else {
                            return Err("Progress observer stopped.".into());
                        }
                    }
                    Ok(())
                },
            )
            .unwrap_err();
            assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
            assert!(saw_live_progress);
            assert!(!finish_path.exists());
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(
                error,
                if cancel_on_progress {
                    "Transcription was cancelled."
                } else {
                    "Progress observer stopped."
                }
            );
        }
    }

    #[test]
    fn progress_child_fixture() {
        if env::var("STORYTELLER_TEST_TRANSCRIPTION_CHILD").as_deref() == Ok("progress") {
            // Narrator stdout must neither stop the GPU job nor advance work.
            println!("whisper_backend_init_gpu: no GPU found");
            println!("The narrator failed to initialize the radio.");
            println!("whisper_print_progress_callback: progress = 100%");
            eprintln!("whisper_backend_init_gpu: using CUDA0 backend");
            eprintln!("whisper_model_load: CUDA0 total size = 580.00 MB");
            let audio = PathBuf::from(env::var_os("STORYTELLER_TEST_PROGRESS_AUDIO").unwrap());
            eprintln!(
                "main: processing '{}' (360000 samples, 22.5 sec)",
                audio.display()
            );
            eprintln!("whisper_print_progress_callback: progress =  50%");
            thread::sleep(Duration::from_secs(30));
            fs::write(
                env::var_os("STORYTELLER_TEST_PROGRESS_FINISH").unwrap(),
                "finished",
            )
            .unwrap();
        }
    }

    #[test]
    fn gpu_fallback_stops_the_owned_process_before_it_can_complete_a_batch() {
        let mut command = Command::new(env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "chunked_transcription::tests::gpu_fallback_child_fixture",
                "--nocapture",
            ])
            .env("STORYTELLER_TEST_TRANSCRIPTION_CHILD", "no-gpu");
        let started = Instant::now();
        let error = run_transcription_command(
            &mut command,
            "Whisper",
            true,
            &CancellationToken::default(),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(error.contains("CPU fallback was stopped"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn gpu_fallback_child_fixture() {
        if env::var("STORYTELLER_TEST_TRANSCRIPTION_CHILD").as_deref() == Ok("no-gpu") {
            eprintln!("whisper_backend_init_gpu: no GPU found");
            thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn gpu_worker_limit_is_checked_before_audio_or_processes_are_opened() {
        let config = ChunkedTranscriptionConfig {
            ffmpeg: "missing-ffmpeg".into(),
            engine: TranscriptionEngine::WhisperCuda {
                executable: "missing-whisper".into(),
                model: "missing-model".into(),
            },
            workers: 2,
        };
        let error = transcribe_audiobook_in_chunks(
            Path::new("missing-audio"),
            Path::new("missing-stage"),
            Path::new("missing-transcript"),
            &config,
            &CancellationToken::default(),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("exactly one"));
    }

    #[test]
    fn parses_ffmpeg_duration_clock() {
        let stderr = "  Duration: 10:02:03.450, start: 0.000000, bitrate: 64 kb/s";
        assert_eq!(parse_ffmpeg_duration_ms(stderr), Some(36_123_450));
    }

    #[test]
    fn parses_ffmetadata_chapter_timebases() {
        let metadata = r#";FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=1800000
title=One
[CHAPTER]
TIMEBASE=1/10000000
START=18000000000
END=36000000000
title=Two
"#;
        assert_eq!(
            parse_ffmetadata_chapter_ends(metadata),
            vec![1_800_000, 3_600_000]
        );
    }

    #[test]
    fn parses_silence_detector_pairs_in_milliseconds() {
        let stderr = "[silencedetect @ x] silence_start: 12.500\n[silencedetect @ x] silence_end: 13.250 | silence_duration: 0.750\n";
        assert_eq!(parse_silence_intervals(stderr), vec![(12_500, 13_250)]);
    }
}
