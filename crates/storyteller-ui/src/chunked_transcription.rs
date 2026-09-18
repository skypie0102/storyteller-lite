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
    merge_chunk_transcripts, plan_transcription_chunks, read_whisper_transcript_chunk,
    run_cancellable_command, validate_chunk_plan, write_whisper_transcript, CancellationToken,
    CommandOutput, CommandRunError, CommandStream, TranscriptionChunk, WhisperTranscript,
    DEFAULT_MAX_TRANSCRIPTION_CHUNK_MS,
};

const SILENCE_SEARCH_RADIUS_MS: u64 = 60_000;
const SILENCE_MIN_DURATION_SECONDS: f64 = 0.35;
const SILENCE_NOISE_DB: i32 = -40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChunkedTranscriptionSummary {
    pub duration_ms: u64,
    pub chunks: usize,
    pub per_worker_threads: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChunkedTranscriptionProgress {
    pub completed_chunks: usize,
    pub total_chunks: usize,
    pub percent: u8,
    pub backend: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChunkedTranscriptionConfig {
    pub ffmpeg: PathBuf,
    pub whisper_cli: PathBuf,
    pub whisper_model: PathBuf,
    pub language: String,
    pub workers: usize,
    pub total_cpu_threads: usize,
}

#[derive(Debug)]
enum WorkerEvent {
    Progress {
        chunk_index: usize,
        percent: u8,
        backend: Option<String>,
    },
    Completed {
        chunk: TranscriptionChunk,
        transcript: WhisperTranscript,
    },
    Failed(String),
}

pub(crate) fn transcribe_audiobook_in_chunks(
    source: &Path,
    stage_dir: &Path,
    transcript_path: &Path,
    config: &ChunkedTranscriptionConfig,
    cancellation: &CancellationToken,
    observer: &mut dyn FnMut(ChunkedTranscriptionProgress) -> Result<(), String>,
) -> Result<ChunkedTranscriptionSummary, String> {
    if config.workers == 0 {
        return Err("Whisper worker count must be positive.".into());
    }
    if config.total_cpu_threads == 0 {
        return Err("Available CPU thread count must be positive.".into());
    }

    let metadata = probe_audio_metadata(source, &config.ffmpeg, cancellation)?;
    let mut chunks = plan_transcription_chunks(
        metadata.duration_ms,
        &metadata.chapter_boundaries_ms,
        DEFAULT_MAX_TRANSCRIPTION_CHUNK_MS,
    )?;
    refine_synthetic_boundaries(
        source,
        &config.ffmpeg,
        metadata.duration_ms,
        &metadata.chapter_boundaries_ms,
        &mut chunks,
        cancellation,
    )?;
    validate_chunk_plan(metadata.duration_ms, &chunks)?;
    write_chunk_plan(
        &stage_dir.join("transcription-plan.json"),
        metadata.duration_ms,
        &chunks,
    )?;

    let effective_workers = config.workers.min(chunks.len()).max(1);
    let per_worker_threads = (config.total_cpu_threads / effective_workers).max(1);
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
        &chunks,
        config,
        effective_workers,
        per_worker_threads,
        cancellation,
        observer,
    )
    .and_then(|parts| {
        let merged = merge_chunk_transcripts(metadata.duration_ms, &parts)?;
        write_whisper_transcript(transcript_path, &merged)
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
        per_worker_threads,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_chunk_workers(
    source: &Path,
    temporary_dir: &Path,
    chunks: &[TranscriptionChunk],
    config: &ChunkedTranscriptionConfig,
    effective_workers: usize,
    per_worker_threads: usize,
    cancellation: &CancellationToken,
    observer: &mut dyn FnMut(ChunkedTranscriptionProgress) -> Result<(), String>,
) -> Result<Vec<(TranscriptionChunk, WhisperTranscript)>, String> {
    let queue = Arc::new(Mutex::new(VecDeque::from(chunks.to_vec())));
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
            let chunk = match queue.lock() {
                Ok(mut queue) => queue.pop_front(),
                Err(_) => {
                    worker_cancellation.request();
                    let _ = sender.send(WorkerEvent::Failed(
                        "Transcription worker queue lock was poisoned.".into(),
                    ));
                    return;
                }
            };
            let Some(chunk) = chunk else {
                return;
            };
            match transcribe_one_chunk(
                &source,
                &temporary_dir,
                chunk,
                &config,
                per_worker_threads,
                &worker_cancellation,
                &sender,
            ) {
                Ok(transcript) => {
                    if sender
                        .send(WorkerEvent::Completed { chunk, transcript })
                        .is_err()
                    {
                        return;
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

    let mut progress = vec![0u8; chunks.len()];
    let mut completed = 0usize;
    let mut parts = vec![None::<(TranscriptionChunk, WhisperTranscript)>; chunks.len()];
    let mut first_error = None::<String>;

    while completed < chunks.len() && first_error.is_none() {
        let event = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if cancellation.is_requested() {
                    worker_cancellation.request();
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                first_error =
                    Some("Transcription workers stopped before all chunks completed.".into());
                break;
            }
        };
        match event {
            WorkerEvent::Progress {
                chunk_index,
                percent,
                backend,
            } => {
                if let Some(slot) = progress.get_mut(chunk_index) {
                    *slot = percent.min(100);
                }
                let overall = progress.iter().map(|value| *value as usize).sum::<usize>()
                    / progress.len().max(1);
                if let Err(error) = observer(ChunkedTranscriptionProgress {
                    completed_chunks: completed,
                    total_chunks: chunks.len(),
                    percent: overall.min(100) as u8,
                    backend,
                }) {
                    worker_cancellation.request();
                    first_error = Some(error);
                }
            }
            WorkerEvent::Completed { chunk, transcript } => {
                progress[chunk.index] = 100;
                parts[chunk.index] = Some((chunk, transcript));
                completed += 1;
                let overall = progress.iter().map(|value| *value as usize).sum::<usize>()
                    / progress.len().max(1);
                if let Err(error) = observer(ChunkedTranscriptionProgress {
                    completed_chunks: completed,
                    total_chunks: chunks.len(),
                    percent: overall.min(100) as u8,
                    backend: None,
                }) {
                    worker_cancellation.request();
                    first_error = Some(error);
                }
            }
            WorkerEvent::Failed(error) => {
                worker_cancellation.request();
                first_error = Some(error);
            }
        }
    }

    for handle in handles {
        if handle.join().is_err() && first_error.is_none() {
            first_error = Some("A transcription worker thread panicked.".into());
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

fn transcribe_one_chunk(
    source: &Path,
    temporary_dir: &Path,
    chunk: TranscriptionChunk,
    config: &ChunkedTranscriptionConfig,
    per_worker_threads: usize,
    cancellation: &CancellationToken,
    sender: &mpsc::Sender<WorkerEvent>,
) -> Result<WhisperTranscript, String> {
    let wav_path = temporary_dir.join(format!("chunk-{:05}.wav", chunk.index));
    let output_prefix = temporary_dir.join(format!("chunk-{:05}", chunk.index));
    let raw_transcript = output_prefix.with_extension("json");

    let start_seconds = chunk.start_ms as f64 / 1000.0;
    let duration_seconds = chunk.duration_ms() as f64 / 1000.0;
    let mut ffmpeg = Command::new(&config.ffmpeg);
    ffmpeg
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{start_seconds:.3}"))
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
        .arg(&wav_path);
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
    validate_nonempty_file(&wav_path, "Converted transcription chunk")?;

    let mut whisper = Command::new(&config.whisper_cli);
    whisper
        .arg("-m")
        .arg(&config.whisper_model)
        .arg("-f")
        .arg(&wav_path)
        .arg("-l")
        .arg(&config.language)
        .arg("-ojf")
        .arg("-of")
        .arg(&output_prefix)
        .arg("-pp")
        .arg("-t")
        .arg(per_worker_threads.to_string());

    match run_cancellable_command(&mut whisper, cancellation, |stream, line| {
        if stream != CommandStream::Stderr {
            return;
        }
        let percent = parse_whisper_progress(line);
        let backend = parse_whisper_backend(line);
        if percent.is_some() || backend.is_some() {
            let _ = sender.send(WorkerEvent::Progress {
                chunk_index: chunk.index,
                percent: percent.unwrap_or(0),
                backend,
            });
        }
    }) {
        Ok(output) if output.success => {}
        Ok(output) => return Err(command_failure("whisper.cpp transcription chunk", &output)),
        Err(CommandRunError::Cancelled) => return Err("Transcription was cancelled.".into()),
        Err(error) => return Err(format!("Could not transcribe audio chunk: {error}")),
    }
    validate_nonempty_file(&raw_transcript, "Whisper chunk transcript")?;
    let transcript = read_whisper_transcript_chunk(&raw_transcript)?;
    let _ = fs::remove_file(&wav_path);
    let _ = fs::remove_file(&raw_transcript);
    Ok(transcript)
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
    cancellation: &CancellationToken,
) -> Result<(), String> {
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
            if refined > previous && refined < next {
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
    validate_chunk_plan(duration_ms, chunks)
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
}

fn write_chunk_plan(
    path: &Path,
    duration_ms: u64,
    chunks: &[TranscriptionChunk],
) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(&ChunkPlanFile {
        duration_ms,
        chunks,
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

fn parse_whisper_progress(line: &str) -> Option<u8> {
    let (_, remainder) = line.split_once("progress =")?;
    let (percent, _) = remainder.split_once('%')?;
    percent
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|value| *value <= 100)
}

fn parse_whisper_backend(line: &str) -> Option<String> {
    let marker = "backend_init_gpu: using ";
    let (_, backend) = line.split_once(marker)?;
    let backend = backend.trim();
    if backend.is_empty() {
        None
    } else {
        Some(format!("whisper.cpp / {backend}"))
    }
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
