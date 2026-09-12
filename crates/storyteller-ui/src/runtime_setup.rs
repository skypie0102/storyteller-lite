use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const MODEL_NAME: &str = "large-v3-turbo";
const MODEL_FILE: &str = "ggml-large-v3-turbo.bin";
const MODEL_SHA256: &str = "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69";
const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin?download=true";
const FFMPEG_ZIP_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
const FFMPEG_SHA_URL: &str =
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256";
const WHISPER_RELEASES_API: &str =
    "https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=10";

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeStatus {
    pub ffmpeg: Option<PathBuf>,
    pub whisper_cli: Option<PathBuf>,
    pub whisper_model: Option<PathBuf>,
}

impl RuntimeStatus {
    pub(crate) fn ready(&self) -> bool {
        self.ffmpeg.is_some() && self.whisper_cli.is_some() && self.whisper_model.is_some()
    }

    pub(crate) fn summary(&self) -> String {
        let mut missing = Vec::new();
        if self.ffmpeg.is_none() {
            missing.push("ffmpeg");
        }
        if self.whisper_cli.is_none() {
            missing.push("whisper.cpp");
        }
        if self.whisper_model.is_none() {
            missing.push("Whisper model");
        }
        if missing.is_empty() {
            "All runtime dependencies are ready.".into()
        } else {
            format!("Missing: {}", missing.join(", "))
        }
    }

    pub(crate) fn ffmpeg_text(&self) -> String {
        dependency_text(self.ffmpeg.as_deref(), "ffmpeg was not found")
    }

    pub(crate) fn whisper_text(&self) -> String {
        dependency_text(self.whisper_cli.as_deref(), "whisper-cli was not found")
    }

    pub(crate) fn model_text(&self) -> String {
        dependency_text(
            self.whisper_model.as_deref(),
            &format!("{MODEL_FILE} was not found"),
        )
    }
}

pub(crate) fn detect_runtime() -> RuntimeStatus {
    RuntimeStatus {
        ffmpeg: find_executable("STORYTELLER_FFMPEG", "ffmpeg", &["-version"]),
        whisper_cli: find_executable("STORYTELLER_WHISPER", "whisper-cli", &["--help"]),
        whisper_model: find_whisper_model(MODEL_NAME),
    }
}

pub(crate) fn install_missing_dependencies(
    status: &RuntimeStatus,
    mut progress: impl FnMut(String),
) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Automatic dependency installation is currently supported on Windows only.".into());
    }
    if status.ready() {
        progress("All runtime dependencies are already available.".into());
        return Ok(());
    }

    let app_root = current_executable_dir()
        .ok_or_else(|| "Could not determine the StoryTeller Lite application folder.".to_string())?;
    let tools_dir = app_root.join("tools");
    let models_dir = app_root.join("models");
    fs::create_dir_all(&tools_dir)
        .map_err(|error| format!("Could not create {}: {error}", tools_dir.display()))?;
    fs::create_dir_all(&models_dir)
        .map_err(|error| format!("Could not create {}: {error}", models_dir.display()))?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let temp_dir = env::temp_dir().join(format!("storyteller-lite-runtime-{}-{stamp}", std::process::id()));
    if temp_dir.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    fs::create_dir_all(&temp_dir)
        .map_err(|error| format!("Could not create temporary download folder: {error}"))?;

    let result = (|| {
        if status.ffmpeg.is_none() {
            progress("Downloading and verifying ffmpeg…".into());
            install_ffmpeg(&tools_dir, &temp_dir)?;
            let installed = tools_dir.join(executable_file_name("ffmpeg"));
            if !probe_executable(&installed, &["-version"]) {
                return Err("Downloaded ffmpeg could not be started.".into());
            }
        }

        if status.whisper_cli.is_none() {
            progress("Downloading and verifying whisper.cpp…".into());
            install_whisper(&tools_dir, &temp_dir)?;
            let installed = tools_dir.join(executable_file_name("whisper-cli"));
            if !probe_executable(&installed, &["--help"]) {
                return Err("Downloaded whisper-cli could not be started.".into());
            }
        }

        if status.whisper_model.is_none() {
            progress("Downloading Whisper large-v3-turbo model (~1.62 GB)…".into());
            install_model(&models_dir, &temp_dir)?;
            let installed = models_dir.join(MODEL_FILE);
            validate_nonempty_file(&installed, "Downloaded Whisper model")?;
        }

        Ok(())
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    result
}

fn dependency_text(path: Option<&Path>, missing: &str) -> String {
    match path {
        Some(path) => format!("Ready — {}", path.display()),
        None => format!("Missing — {missing}"),
    }
}

fn find_executable(environment_variable: &str, base_name: &str, probe_args: &[&str]) -> Option<PathBuf> {
    if let Some(value) = env::var_os(environment_variable).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(value);
        if probe_executable(&path, probe_args) {
            return Some(path);
        }
    }

    let file_name = executable_file_name(base_name);
    let mut candidates = Vec::new();
    if let Some(executable_dir) = current_executable_dir() {
        candidates.push(executable_dir.join("tools").join(&file_name));
        candidates.push(executable_dir.join(&file_name));
    }

    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|entry| entry.join(&file_name)));
    }

    if cfg!(windows) {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local_app_data)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join(&file_name),
            );
        }
        if let Some(user_profile) = env::var_os("USERPROFILE") {
            let user_profile = PathBuf::from(user_profile);
            candidates.push(user_profile.join("scoop").join("shims").join(&file_name));
            if base_name == "ffmpeg" {
                candidates.push(
                    user_profile
                        .join("scoop")
                        .join("apps")
                        .join("ffmpeg")
                        .join("current")
                        .join("bin")
                        .join(&file_name),
                );
                candidates.push(
                    user_profile
                        .join("scoop")
                        .join("apps")
                        .join("ffmpeg-essentials")
                        .join("current")
                        .join("bin")
                        .join(&file_name),
                );
            }
        }
        if let Some(program_data) = env::var_os("PROGRAMDATA") {
            candidates.push(
                PathBuf::from(program_data)
                    .join("chocolatey")
                    .join("bin")
                    .join(&file_name),
            );
        }
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file() && probe_executable(candidate, probe_args))
}

fn find_whisper_model(model_name: &str) -> Option<PathBuf> {
    if let Some(value) = env::var_os("STORYTELLER_WHISPER_MODEL").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(value);
        if validate_nonempty_file(&path, "Configured Whisper model").is_ok() {
            return Some(path);
        }
    }

    let file_name = format!("ggml-{model_name}.bin");
    let mut candidates = Vec::new();
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local_app_data)
                .join("Storyteller OneClick Lite")
                .join("models")
                .join(&file_name),
        );
    }
    if let Some(executable_dir) = current_executable_dir() {
        candidates.push(executable_dir.join("models").join(&file_name));
        candidates.push(executable_dir.join("tools").join("models").join(&file_name));
    }
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join("models").join(&file_name));
    }

    if let Some(found) = candidates
        .into_iter()
        .find(|candidate| validate_nonempty_file(candidate, "Whisper model").is_ok())
    {
        return Some(found);
    }

    find_huggingface_cached_model(&file_name)
}

fn find_huggingface_cached_model(file_name: &str) -> Option<PathBuf> {
    let user_profile = env::var_os("USERPROFILE")?;
    let hub = PathBuf::from(user_profile).join(".cache").join("huggingface").join("hub");
    for repository in ["models--ggerganov--whisper.cpp", "models--ggml-org--whisper.cpp"] {
        let snapshots = hub.join(repository).join("snapshots");
        let Ok(entries) = fs::read_dir(snapshots) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path().join(file_name);
            if validate_nonempty_file(&candidate, "Cached Whisper model").is_ok() {
                return Some(candidate);
            }
        }
    }
    None
}

fn install_ffmpeg(tools_dir: &Path, temp_dir: &Path) -> Result<(), String> {
    let zip_path = temp_dir.join("ffmpeg.zip");
    let sha_path = temp_dir.join("ffmpeg.sha256");
    let extract_dir = temp_dir.join("ffmpeg");
    let target = tools_dir.join(executable_file_name("ffmpeg"));
    let script = format!(
        r#"$ErrorActionPreference='Stop';
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;
Invoke-WebRequest -UseBasicParsing -Uri {url} -OutFile {zip};
Invoke-WebRequest -UseBasicParsing -Uri {sha_url} -OutFile {sha};
$expected=([regex]::Match((Get-Content -Raw {sha}),'[A-Fa-f0-9]{{64}}')).Value.ToUpperInvariant();
if ($expected.Length -ne 64) {{ throw 'Could not read the published ffmpeg SHA-256.' }}
$actual=(Get-FileHash {zip} -Algorithm SHA256).Hash.ToUpperInvariant();
if ($actual -ne $expected) {{ throw 'ffmpeg SHA-256 verification failed.' }}
Expand-Archive -LiteralPath {zip} -DestinationPath {extract} -Force;
$exe=Get-ChildItem -LiteralPath {extract} -Filter ffmpeg.exe -Recurse | Select-Object -First 1;
if (-not $exe) {{ throw 'ffmpeg.exe was not present in the downloaded archive.' }}
Copy-Item -LiteralPath $exe.FullName -Destination {target} -Force;"#,
        url = ps_string(FFMPEG_ZIP_URL),
        sha_url = ps_string(FFMPEG_SHA_URL),
        zip = ps_path(&zip_path),
        sha = ps_path(&sha_path),
        extract = ps_path(&extract_dir),
        target = ps_path(&target),
    );
    run_powershell(&script)
}

fn install_whisper(tools_dir: &Path, temp_dir: &Path) -> Result<(), String> {
    let zip_path = temp_dir.join("whisper.zip");
    let extract_dir = temp_dir.join("whisper");
    let script = format!(
        r#"$ErrorActionPreference='Stop';
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;
$headers=@{{'User-Agent'='StoryTeller-Lite'}};
$releases=Invoke-RestMethod -Headers $headers -Uri {api};
$asset=$releases | ForEach-Object {{ $_.assets }} | Where-Object {{ $_.name -eq 'whisper-bin-x64.zip' }} | Select-Object -First 1;
if (-not $asset) {{ throw 'No official Windows x64 whisper.cpp binary asset was found.' }}
$digest=[string]$asset.digest;
if (-not $digest.StartsWith('sha256:')) {{ throw 'The whisper.cpp release asset did not publish a SHA-256 digest.' }}
Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $asset.browser_download_url -OutFile {zip};
$expected=$digest.Substring(7).ToUpperInvariant();
$actual=(Get-FileHash {zip} -Algorithm SHA256).Hash.ToUpperInvariant();
if ($actual -ne $expected) {{ throw 'whisper.cpp SHA-256 verification failed.' }}
Expand-Archive -LiteralPath {zip} -DestinationPath {extract} -Force;
$cli=Get-ChildItem -LiteralPath {extract} -Filter whisper-cli.exe -Recurse | Select-Object -First 1;
if (-not $cli) {{ throw 'whisper-cli.exe was not present in the downloaded archive.' }}
Get-ChildItem -LiteralPath $cli.Directory.FullName | ForEach-Object {{ Copy-Item -LiteralPath $_.FullName -Destination {tools} -Recurse -Force }};"#,
        api = ps_string(WHISPER_RELEASES_API),
        zip = ps_path(&zip_path),
        extract = ps_path(&extract_dir),
        tools = ps_path(tools_dir),
    );
    run_powershell(&script)
}

fn install_model(models_dir: &Path, temp_dir: &Path) -> Result<(), String> {
    let partial = temp_dir.join(format!("{MODEL_FILE}.part"));
    let target = models_dir.join(MODEL_FILE);
    let script = format!(
        r#"$ErrorActionPreference='Stop';
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;
Invoke-WebRequest -UseBasicParsing -Uri {url} -OutFile {partial};
$actual=(Get-FileHash {partial} -Algorithm SHA256).Hash.ToLowerInvariant();
if ($actual -ne {expected}) {{ throw 'Whisper model SHA-256 verification failed.' }}
Move-Item -LiteralPath {partial} -Destination {target} -Force;"#,
        url = ps_string(MODEL_URL),
        partial = ps_path(&partial),
        expected = ps_string(MODEL_SHA256),
        target = ps_path(&target),
    );
    run_powershell(&script)
}

fn run_powershell(script: &str) -> Result<(), String> {
    let shell = ["powershell.exe", "pwsh.exe"]
        .into_iter()
        .find(|candidate| {
            Command::new(candidate)
                .arg("-NoProfile")
                .arg("-NonInteractive")
                .arg("-Command")
                .arg("$PSVersionTable.PSVersion.ToString()")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        })
        .ok_or_else(|| "PowerShell was not found; automatic dependency download is unavailable.".to_string())?;

    let output = Command::new(shell)
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script)
        .output()
        .map_err(|error| format!("Could not start PowerShell dependency installer: {error}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let detail = detail.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
    if detail.is_empty() {
        Err("Dependency download failed without diagnostics.".into())
    } else {
        Err(format!("Dependency download failed: {detail}"))
    }
}

fn probe_executable(path: &Path, arguments: &[&str]) -> bool {
    Command::new(path)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn current_executable_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn executable_file_name(base_name: &str) -> String {
    if cfg!(windows) {
        format!("{base_name}.exe")
    } else {
        base_name.to_string()
    }
}

fn validate_nonempty_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("{label} is unavailable at {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!("{label} is not a non-empty regular file: {}", path.display()));
    }
    Ok(())
}

fn ps_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn ps_path(path: &Path) -> String {
    ps_string(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_summary_names_only_missing_dependencies() {
        let status = RuntimeStatus {
            ffmpeg: Some(PathBuf::from("ffmpeg.exe")),
            whisper_cli: None,
            whisper_model: None,
        };
        assert_eq!(status.summary(), "Missing: whisper.cpp, Whisper model");
        assert!(!status.ready());
    }

    #[test]
    fn powershell_strings_escape_single_quotes() {
        assert_eq!(ps_string("C:\\O'Brien\\tool.exe"), "'C:\\O''Brien\\tool.exe'");
    }
}
