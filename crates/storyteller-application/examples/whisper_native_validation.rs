//! CPU-hosted native ABI/JSON and GPU-fallback rejection checks; not a GPU benchmark.
use std::{env, fs, path::PathBuf, process::Command};
use storyteller_application::{
    detect_runtime, transcribe_audiobook_in_chunks, ChunkedTranscriptionConfig, TranscriptionEngine,
};
use storyteller_core::{parse_whisper_transcript, CancellationToken};

fn main() -> Result<(), String> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err(
            "Usage: whisper_native_validation AUDIO OUTPUT_DIRECTORY FFMPEG WHISPER_CLI TEST_MODEL"
                .into(),
        );
    }
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    env::set_var("STORYTELLER_WHISPER", &args[3]);
    env::set_var("STORYTELLER_WHISPER_MODEL", &args[4]);
    let runtime = detect_runtime();
    if runtime.whisper.cli != Some(PathBuf::from(&args[3]))
        || runtime.whisper.model != Some(PathBuf::from(&args[4]))
    {
        return Err(
            "Native validation requires the exact supported CUDA bundle and Turbo Q5 model.".into(),
        );
    }
    fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    let prefix = output.join("native-cpu");
    let native = Command::new(&args[3])
        .arg("--model")
        .arg(&args[4])
        .arg("--file")
        .arg(&source)
        .args([
            "--language",
            "en",
            "--threads",
            "2",
            "--suppress-nst",
            "--no-gpu",
            "--output-json",
            "--output-file",
        ])
        .arg(&prefix)
        .output()
        .map_err(|error| error.to_string())?;
    if !native.status.success() {
        return Err(format!(
            "Native CPU validation failed: {}",
            String::from_utf8_lossy(&native.stderr)
        ));
    }
    let json =
        fs::read_to_string(prefix.with_extension("json")).map_err(|error| error.to_string())?;
    let transcript = parse_whisper_transcript(&json, 30_000)?;
    if transcript.segments.is_empty() {
        return Err("Native test fixture returned no timed English speech.".into());
    }
    println!(
        "Native Whisper English JSON validated: {} segments; {}",
        transcript.segments.len(),
        transcript
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let config = ChunkedTranscriptionConfig {
        ffmpeg: PathBuf::from(&args[2]),
        engine: TranscriptionEngine::WhisperCuda {
            executable: PathBuf::from(&args[3]),
            model: PathBuf::from(&args[4]),
        },
        workers: 1,
    };
    let error = transcribe_audiobook_in_chunks(
        &source,
        &output,
        &output.join("transcript.json"),
        &config,
        &CancellationToken::default(),
        &mut |_| Ok(()),
    )
    .expect_err("CPU-hosted validation must reject GPU inference without a GPU");
    if !error.contains("GPU initialization failed") && !error.contains("confirm CUDA offload") {
        return Err(format!("Unexpected fallback error: {error}"));
    }
    if output.join("transcript.json").exists() || output.join("transcription-chunks.tmp").exists() {
        return Err("Rejected GPU inference left transcript or temporary PCM output.".into());
    }
    println!("GPU fallback rejected and temporary files cleaned: {error}");
    Ok(())
}
