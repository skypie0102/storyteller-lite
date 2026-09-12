use crate::{
    copy_file_cancellable, run_cancellable_command, AudioCodec, AudioEncoding, CancellationToken,
    CommandOutput, CommandRunError, CommandStream,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodedAudioDescriptor {
    pub file_name: String,
    pub media_type: String,
    pub codec: String,
    pub bitrate_kbps: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAudio {
    pub descriptor: EncodedAudioDescriptor,
    pub relative_artifacts: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioEncodeProgress {
    pub processed_audio_seconds: f64,
}

pub fn encode_audiobook(
    source: &Path,
    output_dir: &Path,
    encoding: AudioEncoding,
    ffmpeg: &Path,
    cancellation: &CancellationToken,
    observer: &mut dyn FnMut(AudioEncodeProgress) -> Result<(), String>,
) -> Result<EncodedAudio, String> {
    validate_nonempty_file(source, "Prepared audiobook")?;
    reset_directory(output_dir)?;

    let (file_name, media_type, codec) = output_identity(source, encoding)?;
    let destination = output_dir.join(&file_name);

    match encoding.codec {
        AudioCodec::Copy => {
            copy_file_cancellable(source, &destination, cancellation)?;
        }
        AudioCodec::Opus | AudioCodec::Aac => {
            let bitrate = encoding
                .bitrate
                .ok_or("Encoded audio requires a bitrate.")?
                .kbps();
            let mut command = Command::new(ffmpeg);
            command
                .arg("-hide_banner")
                .arg("-nostdin")
                .arg("-y")
                .arg("-i")
                .arg(source)
                .arg("-map")
                .arg("0:a:0")
                .arg("-vn")
                .arg("-map_metadata")
                .arg("-1")
                .arg("-progress")
                .arg("pipe:1")
                .arg("-nostats");
            match encoding.codec {
                AudioCodec::Opus => {
                    command
                        .arg("-c:a")
                        .arg("libopus")
                        .arg("-b:a")
                        .arg(format!("{bitrate}k"));
                }
                AudioCodec::Aac => {
                    command
                        .arg("-c:a")
                        .arg("aac")
                        .arg("-b:a")
                        .arg(format!("{bitrate}k"))
                        .arg("-movflags")
                        .arg("+faststart");
                }
                AudioCodec::Copy => unreachable!(),
            }
            command.arg(&destination);

            let mut observer_error = None;
            let result = run_cancellable_command(&mut command, cancellation, |stream, line| {
                if stream != CommandStream::Stdout || observer_error.is_some() {
                    return;
                }
                if let Some(seconds) = parse_ffmpeg_out_time_us(line) {
                    if let Err(error) = observer(AudioEncodeProgress {
                        processed_audio_seconds: seconds,
                    }) {
                        observer_error = Some(error);
                    }
                }
            });
            if let Some(error) = observer_error {
                return Err(error);
            }
            match result {
                Ok(output) if output.success => {}
                Ok(output) => return Err(command_failure("ffmpeg audio encode", &output)),
                Err(CommandRunError::Cancelled) => {
                    return Err("Audio encoding was cancelled.".into())
                }
                Err(error) => return Err(format!("Could not run ffmpeg audio encode: {error}")),
            }
        }
    }

    validate_nonempty_file(&destination, "Encoded audiobook")?;
    let descriptor = EncodedAudioDescriptor {
        file_name: file_name.clone(),
        media_type: media_type.into(),
        codec: codec.into(),
        bitrate_kbps: encoding.bitrate.map(|value| value.kbps()),
    };
    let descriptor_path = output_dir.join("encoded-audio.json");
    let json = serde_json::to_vec_pretty(&descriptor)
        .map_err(|error| format!("Could not serialize encoded audio descriptor: {error}"))?;
    fs::write(&descriptor_path, json).map_err(|error| {
        format!(
            "Could not write encoded audio descriptor {}: {error}",
            descriptor_path.display()
        )
    })?;

    Ok(EncodedAudio {
        descriptor,
        relative_artifacts: vec![PathBuf::from(file_name), PathBuf::from("encoded-audio.json")],
    })
}

pub fn read_encoded_audio_descriptor(path: &Path) -> Result<EncodedAudioDescriptor, String> {
    let data = fs::read(path).map_err(|error| {
        format!(
            "Could not read encoded audio descriptor {}: {error}",
            path.display()
        )
    })?;
    let descriptor: EncodedAudioDescriptor = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse encoded audio descriptor {}: {error}",
            path.display()
        )
    })?;
    if descriptor.file_name.trim().is_empty()
        || descriptor.media_type.trim().is_empty()
        || descriptor.codec.trim().is_empty()
    {
        return Err("Encoded audio descriptor contains blank required fields.".into());
    }
    if !is_epub_core_audio_media_type(&descriptor.media_type) {
        return Err(format!(
            "Encoded audio media type is not supported for EPUB Media Overlays: {}",
            descriptor.media_type
        ));
    }
    Ok(descriptor)
}

fn output_identity(
    source: &Path,
    encoding: AudioEncoding,
) -> Result<(String, &'static str, &'static str), String> {
    match encoding.codec {
        AudioCodec::Copy => {
            let extension = source
                .extension()
                .and_then(|value| value.to_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or("Copy audio requires a source filename extension.")?
                .to_ascii_lowercase();
            let media_type = media_type_for_copy_extension(&extension).ok_or_else(|| {
                format!(
                    "Copy mode cannot safely embed .{extension} as EPUB 3.3 Media Overlay audio. Use Opus or AAC encoding instead."
                )
            })?;
            Ok((format!("audio.{extension}"), media_type, "copy"))
        }
        AudioCodec::Opus => Ok((
            "audio.opus".into(),
            "audio/ogg; codecs=opus",
            "opus",
        )),
        AudioCodec::Aac => Ok(("audio.m4a".into(), "audio/mp4", "aac")),
    }
}

fn media_type_for_copy_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "mp3" => Some("audio/mpeg"),
        "m4a" | "m4b" | "mp4" => Some("audio/mp4"),
        "opus" => Some("audio/ogg; codecs=opus"),
        _ => None,
    }
}

fn is_epub_core_audio_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "audio/mpeg" | "audio/mp4" | "audio/ogg; codecs=opus"
    )
}

fn parse_ffmpeg_out_time_us(line: &str) -> Option<f64> {
    let value = line.strip_prefix("out_time_us=")?.trim();
    if value == "N/A" {
        return None;
    }
    let microseconds = value.parse::<i64>().ok()?;
    if microseconds < 0 {
        return None;
    }
    Some(microseconds as f64 / 1_000_000.0)
}

fn reset_directory(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|error| {
            format!(
                "Could not reset Encode workspace {}: {error}",
                path.display()
            )
        })?;
    }
    fs::create_dir_all(path).map_err(|error| {
        format!(
            "Could not create Encode workspace {}: {error}",
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
    use crate::{AudioBitrate, AudioEncoding};

    #[test]
    fn ffmpeg_progress_uses_real_microsecond_output_timestamp() {
        assert_eq!(
            parse_ffmpeg_out_time_us("out_time_us=1250000"),
            Some(1.25)
        );
        assert_eq!(parse_ffmpeg_out_time_us("out_time_us=N/A"), None);
        assert_eq!(parse_ffmpeg_out_time_us("out_time_us=-1"), None);
    }

    #[test]
    fn output_identity_matches_epub_core_audio_types() {
        assert_eq!(
            output_identity(Path::new("book.m4b"), AudioEncoding::copy()).unwrap(),
            ("audio.m4b".into(), "audio/mp4", "copy")
        );
        assert_eq!(
            output_identity(
                Path::new("book.m4b"),
                AudioEncoding::new(AudioCodec::Opus, Some(AudioBitrate::Kbps64)).unwrap()
            )
            .unwrap(),
            (
                "audio.opus".into(),
                "audio/ogg; codecs=opus",
                "opus"
            )
        );
        assert!(output_identity(Path::new("book.flac"), AudioEncoding::copy()).is_err());
        assert!(output_identity(Path::new("book.ogg"), AudioEncoding::copy()).is_err());
    }
}
