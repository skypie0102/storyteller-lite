use crate::persistent_app_root;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use storyteller_core::{fingerprint_source_file, CancellationToken, WHISTLE_LANGUAGE};

pub const WHISTLE_MODEL_REVISION: &str = "d3ea19e0fe4f99fa7dfb9afa63070b1c6eacaff1";
pub const WHISTLE_ENGINE_REVISION: &str = "f84005f8992caf37f17b0d64a4b5b31a84ce0d2a";
pub const WHISTLE_MODEL_SHA256: &str =
    "b6e02f048568ac5d01a2042556c658061e699acbc0aa2a1439f52f3d461dffeb";
pub const WHISTLE_WINDOWS_SHA256: &str =
    "c70ca998f6c542c862c06c22352046e303ef5c769384069ee25cef9f4667e4cf";
pub const WHISTLE_LINUX_SHA256: &str =
    "b197ceaef3b300a0b14c3a4fde92305527e43f9256c53d2a53d2a2fe8fe69678";
const FFMPEG_URL: &str = "https://github.com/GyanD/codexffmpeg/releases/download/9.0.1/ffmpeg-9.0.1-essentials_build.zip";
const FFMPEG_SHA256: &str = "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9";

#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    pub workers: crate::WorkerRecommendation,
    pub ffmpeg: Option<PathBuf>,
    pub whistle_cli: Option<PathBuf>,
    pub whistle_model: Option<PathBuf>,
}

impl RuntimeStatus {
    pub fn ready(&self) -> bool {
        self.ffmpeg.is_some() && self.whistle_cli.is_some() && self.whistle_model.is_some()
    }

    pub fn summary(&self) -> String {
        let mut missing = Vec::new();
        if self.ffmpeg.is_none() {
            missing.push("FFmpeg");
        }
        if self.whistle_cli.is_none() {
            missing.push("Whistle engine");
        }
        if self.whistle_model.is_none() {
            missing.push("Whistle model");
        }
        if missing.is_empty() {
            "Ready — Whistle on CPU".into()
        } else {
            format!("Missing: {}", missing.join(", "))
        }
    }

    pub fn ffmpeg_text(&self) -> String {
        dependency_text(self.ffmpeg.as_deref())
    }
    pub fn transcription_text(&self) -> String {
        dependency_text(self.whistle_cli.as_deref())
    }
    pub fn model_text(&self) -> String {
        dependency_text(self.whistle_model.as_deref())
    }
}

pub fn detect_runtime() -> RuntimeStatus {
    let ffmpeg = executable_candidates("STORYTELLER_FFMPEG", "ffmpeg")
        .into_iter()
        .find(|path| probe_ffmpeg(path));
    let expected_engine = if cfg!(windows) {
        WHISTLE_WINDOWS_SHA256
    } else {
        WHISTLE_LINUX_SHA256
    };
    let whistle_cli = executable_candidates("STORYTELLER_WHISTLE", "needle")
        .into_iter()
        .find(|path| verified_file(path, expected_engine));
    let mut models = Vec::new();
    if let Some(path) = nonempty_env_path("STORYTELLER_WHISTLE_MODEL") {
        models.push(path);
    }
    if let Some(root) = persistent_app_root() {
        models.push(root.join("models/whistle.cact"));
    }
    if let Some(root) = executable_dir() {
        models.push(root.join("models/whistle.cact"));
    }
    RuntimeStatus {
        workers: crate::worker_recommendation::detect_worker_recommendation(),
        ffmpeg,
        whistle_cli,
        whistle_model: models
            .into_iter()
            .find(|path| verified_file(path, WHISTLE_MODEL_SHA256)),
    }
}

pub fn configure_runtime_environment() -> RuntimeStatus {
    let runtime = detect_runtime();
    for (name, path) in [
        ("STORYTELLER_FFMPEG", runtime.ffmpeg.as_ref()),
        ("STORYTELLER_WHISTLE", runtime.whistle_cli.as_ref()),
        ("STORYTELLER_WHISTLE_MODEL", runtime.whistle_model.as_ref()),
    ] {
        if let Some(path) = path {
            env::set_var(name, path);
        }
    }
    runtime
}

/// A native, isolated invocation. No Python interpreter or Whisper runtime is used.
pub(crate) fn whistle_command(executable: &Path, model: &Path, audio: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--model")
        .arg(model)
        .arg("--audio")
        .arg(audio)
        .arg("--audio-word-timestamps")
        .arg("--audio-language")
        .arg(WHISTLE_LANGUAGE);
    command
        .env("NEEDLE_TELEMETRY", "0")
        .env("DO_NOT_TRACK", "1");
    command
}

pub fn install_missing_dependencies(
    runtime: &RuntimeStatus,
    mut progress: impl FnMut(String),
) -> Result<(), String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("Automatic dependency installation is supported on Windows x64.".into());
    }
    let root = persistent_app_root().ok_or("Windows LOCALAPPDATA is unavailable.")?;
    fs::create_dir_all(root.join("tools/whistle"))
        .map_err(|error| format!("Could not create runtime folder: {error}"))?;
    fs::create_dir_all(root.join("models"))
        .map_err(|error| format!("Could not create model folder: {error}"))?;
    if runtime.ffmpeg.is_none() {
        progress("Downloading and verifying FFmpeg 9.0.1…".into());
        let archive = root.join("tools/ffmpeg.download.zip");
        download_verified(FFMPEG_URL, &archive, FFMPEG_SHA256)?;
        let expanded = root.join("tools/ffmpeg.extract.tmp");
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$expanded = {expanded}
try {{
    if (Test-Path -LiteralPath $expanded) {{ Remove-Item -LiteralPath $expanded -Recurse -Force }}
    Expand-Archive -LiteralPath {archive} -DestinationPath $expanded
    $files = @(Get-ChildItem -LiteralPath $expanded -Recurse -Filter 'ffmpeg.exe' -File)
    if ($files.Count -ne 1) {{ throw 'FFmpeg archive has an ambiguous executable' }}
    Copy-Item -LiteralPath $files[0].FullName -Destination {destination} -Force
}} finally {{
    if (Test-Path -LiteralPath $expanded) {{ Remove-Item -LiteralPath $expanded -Recurse -Force }}
    Remove-Item -LiteralPath {archive} -Force -ErrorAction SilentlyContinue
}}
"#,
            expanded = ps_path(&expanded),
            archive = ps_path(&archive),
            destination = ps_path(&root.join("tools/ffmpeg.exe"))
        );
        run_powershell(&script)?;
        if !probe_ffmpeg(&root.join("tools/ffmpeg.exe")) {
            return Err("Downloaded FFmpeg could not be started.".into());
        }
    }
    if runtime.whistle_cli.is_none() {
        progress("Downloading and verifying the native Whistle engine…".into());
        let url = format!("https://huggingface.co/Cactus-Compute/needle3/resolve/{WHISTLE_ENGINE_REVISION}/windows-x86_64/needle.exe");
        download_verified(
            &url,
            &root.join("tools/whistle/needle.exe"),
            WHISTLE_WINDOWS_SHA256,
        )?;
    }
    if runtime.whistle_model.is_none() {
        progress("Downloading and verifying Whistle (16.9 MB)…".into());
        let url = format!("https://huggingface.co/Cactus-Compute/whistle/resolve/{WHISTLE_MODEL_REVISION}/whistle.cact");
        download_verified(
            &url,
            &root.join("models/whistle.cact"),
            WHISTLE_MODEL_SHA256,
        )?;
    }
    Ok(())
}

fn download_verified(url: &str, destination: &Path, sha256: &str) -> Result<(), String> {
    // Download beside the destination so final publication stays on one volume.
    let temporary = destination.with_extension("download.tmp");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$temporary = {temporary}
try {{
    Invoke-WebRequest -Uri {url} -OutFile $temporary
    if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash.ToLowerInvariant() -ne {sha256}) {{ throw 'Dependency checksum mismatch' }}
    Move-Item -LiteralPath $temporary -Destination {destination} -Force
}} finally {{
    if (Test-Path -LiteralPath $temporary) {{ Remove-Item -LiteralPath $temporary -Force }}
}}
"#,
        temporary = ps_path(&temporary),
        url = ps_string(url),
        sha256 = ps_string(sha256),
        destination = ps_path(destination)
    );
    run_powershell(&script)
}

fn run_powershell(script: &str) -> Result<(), String> {
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|error| format!("Could not start dependency installer: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("Dependency installation failed: {}", stderr.trim()))
}

fn verified_file(path: &Path, sha256: &str) -> bool {
    fingerprint_source_file(path, &CancellationToken::default(), "Runtime asset")
        .is_ok_and(|fingerprint| fingerprint == format!("sha256:{sha256}"))
}

fn dependency_text(path: Option<&Path>) -> String {
    path.map(|path| format!("Ready — {}", path.display()))
        .unwrap_or_else(|| "Missing".into())
}

fn executable_candidates(variable: &str, name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = nonempty_env_path(variable) {
        candidates.push(path);
    }
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    if let Some(root) = persistent_app_root() {
        candidates.push(root.join("tools/whistle").join(&name));
        candidates.push(root.join("tools").join(&name));
    }
    if let Some(root) = executable_dir() {
        candidates.push(root.join("tools/whistle").join(&name));
        candidates.push(root.join("tools").join(&name));
        candidates.push(root.join(&name));
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|path| path.join(&name)));
    }
    candidates
}

fn nonempty_env_path(variable: &str) -> Option<PathBuf> {
    env::var_os(variable)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}
fn executable_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}
fn probe_ffmpeg(path: &Path) -> bool {
    Command::new(path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
fn ps_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}
fn ps_path(path: &Path) -> String {
    ps_string(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_command_forces_english_requests_times_and_keeps_paths_literal() {
        let command = whistle_command(
            Path::new("needle.exe"),
            Path::new("C:/O'Brien/whistle.cact"),
            Path::new("audio with spaces.wav"),
        );
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "--model",
                "C:/O'Brien/whistle.cact",
                "--audio",
                "audio with spaces.wav",
                "--audio-word-timestamps",
                "--audio-language",
                "en"
            ]
        );
    }

    #[test]
    fn owned_runtime_assets_are_pinned_and_unverified_bytes_fail() {
        for digest in [
            WHISTLE_MODEL_SHA256,
            WHISTLE_WINDOWS_SHA256,
            WHISTLE_LINUX_SHA256,
        ] {
            assert_eq!(digest.len(), 64);
        }
        assert!(!verified_file(
            Path::new("missing.cact"),
            WHISTLE_MODEL_SHA256
        ));
        assert_eq!(ps_string("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn summary_reports_only_missing_assets() {
        let runtime = RuntimeStatus {
            ffmpeg: Some("ffmpeg.exe".into()),
            ..Default::default()
        };
        assert_eq!(runtime.summary(), "Missing: Whistle engine, Whistle model");
    }
}
