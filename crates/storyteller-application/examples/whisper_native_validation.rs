//! CPU-hosted native ABI/JSON and GPU-fallback rejection checks; not a GPU benchmark.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use storyteller_application::{
    detect_runtime, transcribe_audiobook_in_chunks, ChunkedTranscriptionConfig, TranscriptionEngine,
};
use storyteller_core::{
    contextual_transcription_windows, merge_contextual_word_transcripts, parse_whisper_transcript,
    parse_whisper_words, plan_transcription_chunks, write_transcript, CancellationToken, TimedWord,
    TranscriptionBackend,
};

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
    let second = output.join("second input with spaces.wav");
    fs::copy(&source, &second).map_err(|error| error.to_string())?;
    let prefixes = [output.join("native cpu 1"), output.join("native cpu 2")];
    let mut command = Command::new(&args[3]);
    command
        .arg("--model")
        .arg(&args[4])
        .arg("--file")
        .arg(&source)
        .arg("--output-file")
        .arg(&prefixes[0])
        .arg("--file")
        .arg(&second)
        .arg("--output-file")
        .arg(&prefixes[1])
        .args([
            "--language",
            "en",
            "--threads",
            "2",
            "--suppress-nst",
            "--no-gpu",
            "--output-json-full",
        ]);
    let native = command.output().map_err(|error| error.to_string())?;
    if !native.status.success() {
        return Err(format!(
            "Native CPU validation failed: {}",
            String::from_utf8_lossy(&native.stderr)
        ));
    }
    fs::write(output.join("native-cpu.stderr.txt"), &native.stderr)
        .map_err(|error| error.to_string())?;
    fs::write(output.join("native-cpu.stdout.txt"), &native.stdout)
        .map_err(|error| error.to_string())?;
    let log = String::from_utf8_lossy(&native.stderr);
    let model_loads = log
        .lines()
        .filter(|line| line.contains(": loading model from '"))
        .count();
    if model_loads != 1 {
        return Err(format!(
            "Expected one native model load for both inputs; found {model_loads}."
        ));
    }
    let mut baseline_words = Vec::new();
    for prefix in prefixes {
        let json =
            fs::read_to_string(prefix.with_extension("json")).map_err(|error| error.to_string())?;
        let transcript = parse_whisper_transcript(&json, 30_000)?;
        let words = parse_whisper_words(&json, 30_000)?;
        if baseline_words.is_empty() {
            baseline_words = words.clone();
        }
        if transcript.segments.is_empty() {
            return Err("Native test fixture returned no timed English speech.".into());
        }
        println!(
            "Native Whisper English JSON validated: {} segments, {} native timed words; {}",
            transcript.segments.len(),
            words.len(),
            transcript
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    println!("Native Whisper model reused: two inputs, one load, separate English JSON outputs.");
    validate_boundary(
        &source,
        &output,
        &args[2],
        &args[3],
        &args[4],
        &baseline_words,
    )?;
    let rejected_source = output.join("gpu rejection multi-window.wav");
    let conversion = Command::new(&args[2])
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-y",
            "-stream_loop",
            "-1",
            "-i",
        ])
        .arg(&source)
        .args(["-t", "335", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(&rejected_source)
        .output()
        .map_err(|error| error.to_string())?;
    if !conversion.status.success() {
        return Err(format!(
            "Could not generate multi-window rejection fixture: {}",
            String::from_utf8_lossy(&conversion.stderr)
        ));
    }
    let config = ChunkedTranscriptionConfig {
        ffmpeg: PathBuf::from(&args[2]),
        engine: TranscriptionEngine::WhisperCuda {
            executable: PathBuf::from(&args[3]),
            model: PathBuf::from(&args[4]),
        },
        workers: 1,
    };
    let error = transcribe_audiobook_in_chunks(
        &rejected_source,
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
    let plan: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output.join("transcription-plan.json"))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if plan["chunks"]
        .as_array()
        .is_none_or(|chunks| chunks.len() < 2)
    {
        return Err("GPU rejection check did not exercise multiple chunk inputs.".into());
    }
    println!("GPU fallback rejected and temporary files cleaned: {error}");
    Ok(())
}

fn identity(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn checked_output(command: &mut Command, label: &str) -> Result<std::process::Output, String> {
    let result = command
        .output()
        .map_err(|error| format!("{label}: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "{label}: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(result)
}

fn validate_boundary(
    source: &Path,
    output: &Path,
    ffmpeg: &std::ffi::OsStr,
    cli: &std::ffi::OsStr,
    model: &std::ffi::OsStr,
    baseline: &[TimedWord],
) -> Result<(), String> {
    let target = baseline
        .iter()
        .find(|word| {
            let key = identity(&word.text);
            key.len() >= 8
                && word.end_ms > word.start_ms
                && word.end_ms < 3000
                && baseline
                    .iter()
                    .filter(|other| identity(&other.text) == key)
                    .count()
                    == 1
        })
        .ok_or("No unique early native timed word for the Whisper boundary fixture.")?;
    let key = identity(&target.text);
    let cut = 24_500;
    let padding = cut - (target.start_ms + (target.end_ms - target.start_ms) / 2);
    if baseline
        .last()
        .is_none_or(|last| last.end_ms + padding >= 31_000)
    {
        return Err("Whisper boundary fixture would truncate the original speech.".into());
    }
    let padded = output.join("boundary.wav");
    checked_output(
        Command::new(ffmpeg)
            .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
            .arg(source)
            .arg("-af")
            .arg(format!("adelay={padding}:all=1,apad"))
            .args(["-t", "31", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
            .arg(&padded),
        "Generate Whisper boundary fixture",
    )?;
    let chunks = plan_transcription_chunks(31_000, &[cut], 25_000)?;
    let windows =
        contextual_transcription_windows(31_000, &chunks, TranscriptionBackend::WhisperCuda)?;
    if windows.len() != 2 || chunks[0].end_ms != cut {
        return Err("Whisper boundary fixture did not preserve its forced cut.".into());
    }
    let mut command = Command::new(cli);
    command.arg("--model").arg(model).args([
        "--language",
        "en",
        "--threads",
        "2",
        "--suppress-nst",
        "--no-gpu",
        "--output-json-full",
    ]);
    let mut prefixes = Vec::new();
    for window in &windows {
        let wav = output.join(format!("boundary-input-{}.wav", window.owned.index));
        checked_output(
            Command::new(ffmpeg)
                .args([
                    "-hide_banner",
                    "-nostdin",
                    "-loglevel",
                    "error",
                    "-y",
                    "-ss",
                ])
                .arg(format!("{:.3}", window.start_ms as f64 / 1000.0))
                .arg("-i")
                .arg(&padded)
                .arg("-t")
                .arg(format!("{:.3}", window.duration_ms() as f64 / 1000.0))
                .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
                .arg(&wav),
            "Extract Whisper boundary context",
        )?;
        let prefix = output.join(format!("boundary-native-{}", window.owned.index));
        command
            .arg("--file")
            .arg(&wav)
            .arg("--output-file")
            .arg(&prefix);
        prefixes.push(prefix);
    }
    let long = output.join("native-long-input.wav");
    checked_output(
        Command::new(ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-y",
                "-stream_loop",
                "-1",
                "-i",
            ])
            .arg(source)
            .args(["-t", "35", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
            .arg(&long),
        "Generate Whisper input longer than thirty seconds",
    )?;
    let long_prefix = output.join("native-long-input");
    command
        .arg("--file")
        .arg(&long)
        .arg("--output-file")
        .arg(&long_prefix);
    let native = checked_output(&mut command, "Native Whisper boundary inference")?;
    fs::write(output.join("boundary-native.stderr.txt"), &native.stderr)
        .map_err(|e| e.to_string())?;
    fs::write(output.join("boundary-native.stdout.txt"), &native.stdout)
        .map_err(|e| e.to_string())?;
    if String::from_utf8_lossy(&native.stderr)
        .lines()
        .filter(|line| line.contains(": loading model from '"))
        .count()
        != 1
    {
        return Err("Whisper boundary inputs did not reuse one native model context.".into());
    }
    let mut parts = Vec::new();
    for (&window, prefix) in windows.iter().zip(&prefixes) {
        let json = fs::read_to_string(prefix.with_extension("json")).map_err(|e| e.to_string())?;
        let words = parse_whisper_words(&json, window.duration_ms())?;
        if !words.iter().any(|word| identity(&word.text) == key) {
            return Err(format!(
                "Whisper contextual input {} lost '{key}'.",
                window.owned.index
            ));
        }
        parts.push((window, words));
    }
    let long_json =
        fs::read_to_string(long_prefix.with_extension("json")).map_err(|e| e.to_string())?;
    let long_words = parse_whisper_words(&long_json, 35_000)?;
    if !long_words.iter().any(|word| word.start_ms > 30_000) {
        return Err(
            "Native Whisper did not retain timed words after thirty seconds in one input file."
                .into(),
        );
    }
    println!(
        "Native Whisper accepted one 35-second file and retained timed words after thirty seconds."
    );
    let transcript =
        merge_contextual_word_transcripts(31_000, &parts, TranscriptionBackend::WhisperCuda)?;
    let occurrences = transcript
        .segments
        .iter()
        .flat_map(|s| s.text.split_whitespace())
        .filter(|word| identity(word) == key)
        .count();
    if occurrences != 1 {
        return Err(format!(
            "Whisper boundary word '{key}' appears {occurrences} times."
        ));
    }
    write_transcript(&output.join("boundary-transcript.json"), &transcript)?;
    let evidence = serde_json::json!({
        "engine":"Whisper Turbo native CPU validation", "target":key,
        "baseline_start_ms":target.start_ms, "baseline_end_ms":target.end_ms,
        "source_word_start_ms":target.start_ms + padding, "source_word_end_ms":target.end_ms + padding,
        "cut_ms":cut, "target_occurrences":occurrences, "inference_windows":windows,
        "long_input_duration_ms":35_000,
        "long_input_last_word":long_words.last(),
        "transcript":transcript,
    });
    fs::write(
        output.join("boundary-evidence.json"),
        serde_json::to_vec_pretty(&evidence).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("Native Whisper word boundary retained once: {key} across {cut} ms; bounded inputs, one model load.");
    Ok(())
}
