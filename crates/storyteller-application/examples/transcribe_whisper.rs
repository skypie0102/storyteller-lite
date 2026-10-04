//! Hardware acceptance helper using the same pinned GPU adapter as the desktop.
use std::{env, fs, path::PathBuf};
use storyteller_application::{
    detect_runtime, transcribe_audiobook_in_chunks, ChunkedTranscriptionConfig, TranscriptionEngine,
};
use storyteller_core::{CancellationToken, TranscriptionBackend};

fn main() -> Result<(), String> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err(
            "Usage: transcribe_whisper AUDIO OUTPUT_DIRECTORY FFMPEG WHISPER_CLI TURBO_Q5_MODEL"
                .into(),
        );
    }
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    env::set_var("STORYTELLER_FFMPEG", &args[2]);
    env::set_var("STORYTELLER_WHISPER", &args[3]);
    env::set_var("STORYTELLER_WHISPER_MODEL", &args[4]);
    let runtime = detect_runtime();
    if !runtime.ready_for(TranscriptionBackend::WhisperCuda) {
        return Err(format!(
            "GPU runtime is not ready: {}",
            runtime.summary_for(TranscriptionBackend::WhisperCuda)
        ));
    }
    fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    let config = ChunkedTranscriptionConfig {
        ffmpeg: runtime.ffmpeg.unwrap(),
        engine: TranscriptionEngine::WhisperCuda {
            executable: runtime.whisper.cli.unwrap(),
            model: runtime.whisper.model.unwrap(),
        },
        workers: 1,
    };
    let started = std::time::Instant::now();
    let summary = transcribe_audiobook_in_chunks(
        &source,
        &output,
        &output.join("transcript.json"),
        &config,
        &CancellationToken::default(),
        &mut |p| {
            eprintln!(
                "{}% — {}/{} chunks",
                p.percent, p.completed_chunks, p.total_chunks
            );
            Ok(())
        },
    )?;
    println!(
        "Transcribed {} ms in {} chunks with {} GPU worker in {:.2} seconds.",
        summary.duration_ms,
        summary.chunks,
        summary.effective_workers,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
