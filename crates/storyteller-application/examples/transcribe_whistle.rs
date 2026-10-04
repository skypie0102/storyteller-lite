//! Runs the same chunked Whistle adapter used by the desktop pipeline.
use std::{env, fs, path::PathBuf};
use storyteller_application::{transcribe_audiobook_in_chunks, ChunkedTranscriptionConfig};
use storyteller_core::CancellationToken;

fn main() -> Result<(), String> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if !(5..=7).contains(&args.len()) {
        return Err("Usage: transcribe_whistle AUDIO OUTPUT_DIRECTORY FFMPEG NEEDLE WHISTLE_MODEL [WORKERS] [LANGUAGE]".into());
    }
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    let config = ChunkedTranscriptionConfig {
        ffmpeg: PathBuf::from(&args[2]),
        whistle_cli: PathBuf::from(&args[3]),
        whistle_model: PathBuf::from(&args[4]),
        workers: args
            .get(5)
            .map(|value| value.to_string_lossy().parse::<usize>())
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or(1),
        language: args
            .get(6)
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "auto".into()),
    };
    let summary = transcribe_audiobook_in_chunks(
        &source,
        &output,
        &output.join("transcript.json"),
        &config,
        &CancellationToken::default(),
        &mut |progress| {
            eprintln!(
                "{}% — {}/{} chunks",
                progress.percent, progress.completed_chunks, progress.total_chunks
            );
            Ok(())
        },
    )?;
    println!(
        "Transcribed {} ms in {} chunks with {} workers.",
        summary.duration_ms, summary.chunks, summary.effective_workers
    );
    Ok(())
}
