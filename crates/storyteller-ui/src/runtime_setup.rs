use std::{
    collections::HashSet,
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
const LEGACY_SCAN_ENTRY_LIMIT: usize = 8_000;
const LEGACY_SCAN_DEPTH: usize = 7;

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
            if self
                .whisper_cli
                .as_deref()
                .is_some_and(is_cuda_whisper_build)
            {
                "All runtime dependencies are ready. CUDA whisper.cpp selected.".into()
            } else {
                "All runtime dependencies are ready.".into()
            }
        } else {
            format!("Missing: {}", missing.join(", "))
        }
    }

    pub(crate) fn ffmpeg_text(&self) -> String {
        dependency_text(self.ffmpeg.as_deref(), "ffmpeg was not found")
    }

    pub(crate) fn whisper_text(&self) -> String {
        match self.whisper_cli.as_deref() {
            Some(path) if is_cuda_whisper_build(path) => {
                format!("Ready — CUDA build — {}", path.display())
            }
            Some(path) => format!("Ready — CPU/unknown build — {}", path.display()),
            None => "Missing — whisper-cli/main was not found".into(),
        }
    }

    pub(crate) fn model_text(&self) -> String {
        dependency_text(
            self.whisper_model.as_deref(),
            &format!("{MODEL_FILE} was not found"),
        )
    }
}

#[derive(Debug)]
struct WhisperCandidate {
    path: PathBuf,
    source_rank: u8,
}

pub(crate) fn detect_runtime() -> RuntimeStatus {
    RuntimeStatus {
        ffmpeg: find_executable("STORYTELLER_FFMPEG", "ffmpeg", &["-version"]),
        whisper_cli: find_whisper_executable(),
        whisper_model: find_whisper_model(MODEL_NAME),
    }
}

pub(crate) fn configure_runtime_environment() -> RuntimeStatus {
    let status = detect_runtime();
    if let Some(path) = status.ffmpeg.as_deref() {
        env::set_var("STORYTELLER_FFMPEG", path);
    }
    if let Some(path) = status.whisper_cli.as_deref() {
        env::set_var("STORYTELLER_WHISPER", path);
    }
    if let Some(path) = status.whisper_model.as_deref() {
        env::set_var("STORYTELLER_WHISPER_MODEL", path);
    }
    status
}

pub(crate) fn install_missing_dependencies(
    status: &RuntimeStatus,
    mut progress: impl FnMut(String),
) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(
            "Automatic dependency installation is currently supported on Windows only.".into(),
        );
    }
    if status.ready() {
        progress("All runtime dependencies are already available.".into());
        return Ok(());
    }

    let app_root = current_executable_dir().ok_or_else(|| {
        "Could not determine the StoryTeller Lite application folder.".to_string()
    })?;
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
    let temp_dir = env::temp_dir().join(format!(
        "storyteller-lite-runtime-{}-{stamp}",
        std::process::id()
    ));
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
            let mut installed_from_cache = false;
            if let Some(archive) = find_legacy_cuda_archive() {
                progress(format!(
                    "Found cached CUDA whisper.cpp archive at {}; importing it…",
                    archive.display()
                ));
                if install_whisper_from_archive(&archive, &tools_dir, &temp_dir).is_ok() {
                    let installed = tools_dir.join(executable_file_name("whisper-cli"));
                    if probe_executable(&installed, &["--help"]) {
                        installed_from_cache = true;
                    }
                }
                if !installed_from_cache {
                    progress(
                        "The cached CUDA archive could not be reused; downloading a verified build instead…"
                            .into(),
                    );
                }
            }

            if !installed_from_cache {
                let prefer_cuda = nvidia_gpu_available();
                progress(if prefer_cuda {
                    "NVIDIA GPU detected; downloading and verifying a CUDA-enabled whisper.cpp build…"
                        .into()
                } else {
                    "Downloading and verifying whisper.cpp…".into()
                });
                install_whisper(&tools_dir, &temp_dir, prefer_cuda)?;
                let installed = tools_dir.join(executable_file_name("whisper-cli"));
                if !probe_executable(&installed, &["--help"]) {
                    return Err("Downloaded whisper-cli could not be started.".into());
                }
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

fn find_executable(
    environment_variable: &str,
    base_name: &str,
    probe_args: &[&str],
) -> Option<PathBuf> {
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

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file() && probe_executable(candidate, probe_args))
}

fn find_whisper_executable() -> Option<PathBuf> {
    if let Some(value) = env::var_os("STORYTELLER_WHISPER").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(value);
        if probe_executable(&path, &["--help"]) {
            return Some(path);
        }
    }

    let names = whisper_executable_names();
    let mut candidates = Vec::<WhisperCandidate>::new();

    if let Some(executable_dir) = current_executable_dir() {
        for name in &names {
            candidates.push(WhisperCandidate {
                path: executable_dir.join("tools").join(name),
                source_rank: 0,
            });
            candidates.push(WhisperCandidate {
                path: executable_dir.join(name),
                source_rank: 0,
            });
        }
    }

    for path in persistent_whisper_executables() {
        candidates.push(WhisperCandidate {
            path,
            source_rank: 1,
        });
    }

    if let Some(path) = env::var_os("PATH") {
        for entry in env::split_paths(&path) {
            for name in &names {
                candidates.push(WhisperCandidate {
                    path: entry.join(name),
                    source_rank: 2,
                });
            }
        }
    }

    let mut seen = HashSet::new();
    candidates.retain(|candidate| {
        let key = candidate.path.to_string_lossy().to_lowercase();
        seen.insert(key)
    });
    candidates.retain(|candidate| {
        candidate.path.is_file() && probe_executable(&candidate.path, &["--help"])
    });
    candidates.sort_by_key(|candidate| {
        (
            if is_cuda_whisper_build(&candidate.path) {
                0u8
            } else {
                1u8
            },
            candidate.source_rank,
            candidate.path.to_string_lossy().len(),
        )
    });
    candidates
        .into_iter()
        .next()
        .map(|candidate| candidate.path)
}

fn whisper_executable_names() -> Vec<String> {
    if cfg!(windows) {
        vec!["whisper-cli.exe".into(), "main.exe".into()]
    } else {
        vec!["whisper-cli".into(), "main".into()]
    }
}

fn persistent_whisper_executables() -> Vec<PathBuf> {
    let mut executables = Vec::new();
    let mut archives = Vec::new();
    let mut budget = LEGACY_SCAN_ENTRY_LIMIT;
    for root in legacy_search_roots() {
        scan_legacy_tree(&root, 0, &mut budget, &mut executables, &mut archives);
        if budget == 0 {
            break;
        }
    }
    executables
}

fn find_legacy_cuda_archive() -> Option<PathBuf> {
    let mut executables = Vec::new();
    let mut archives = Vec::new();
    let mut budget = LEGACY_SCAN_ENTRY_LIMIT;
    for root in legacy_search_roots() {
        scan_legacy_tree(&root, 0, &mut budget, &mut executables, &mut archives);
        if budget == 0 {
            break;
        }
    }
    archives.into_iter().next()
}

fn legacy_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["LOCALAPPDATA", "APPDATA", "PROGRAMDATA"] {
        if let Some(value) = env::var_os(variable) {
            add_hinted_roots(PathBuf::from(value), &mut roots);
        }
    }

    if let Some(value) = env::var_os("USERPROFILE") {
        let profile = PathBuf::from(value);
        add_known_storyteller_roots(&profile, &mut roots);
        add_hinted_roots(profile.join(".cache"), &mut roots);
        roots.push(profile.join(".storyteller-oneclick"));
        roots.push(profile.join(".storyteller"));
    }

    let mut seen = HashSet::new();
    roots.retain(|root| root.is_dir() && seen.insert(root.to_string_lossy().to_lowercase()));
    roots
}

fn add_hinted_roots(base: PathBuf, roots: &mut Vec<PathBuf>) {
    add_known_storyteller_roots(&base, roots);
    let Ok(entries) = fs::read_dir(&base) else {
        return;
    };
    for entry in entries.flatten().take(512) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if directory_name_is_runtime_hint(&name) {
            roots.push(path);
        }
    }
}

fn add_known_storyteller_roots(base: &Path, roots: &mut Vec<PathBuf>) {
    for name in [
        "Storyteller OneClick",
        "Storyteller OneClick Lite",
        "StoryTeller OneClick",
        "storyteller-oneclick",
        "storyteller-oneclick-lite",
        "StoryTeller",
        "Storyteller",
        "Shadow Monarch Books",
        "shadowmonarchbooks",
        "whisper.cpp",
        "whisper-cpp",
    ] {
        roots.push(base.join(name));
    }
}

fn directory_name_is_runtime_hint(name: &str) -> bool {
    name.contains("storyteller")
        || name.contains("story-teller")
        || name.contains("whisper")
        || name.contains("shadowmonarch")
        || name.contains("shadow monarch")
        || name.contains("shdwmnrch")
}

fn scan_legacy_tree(
    root: &Path,
    depth: usize,
    budget: &mut usize,
    executables: &mut Vec<PathBuf>,
    archives: &mut Vec<PathBuf>,
) {
    if depth > LEGACY_SCAN_DEPTH || *budget == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name == "whisper-cli.exe" || name == "main.exe" {
                executables.push(path);
            } else if is_legacy_cuda_archive_name(&name) {
                archives.push(path);
            }
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if matches!(name.as_str(), ".git" | "node_modules" | "target") {
            continue;
        }
        scan_legacy_tree(&path, depth + 1, budget, executables, archives);
    }
}

fn is_legacy_cuda_archive_name(name: &str) -> bool {
    name.starts_with("whisper-cpp-windows-x64-cuda-")
        && (name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".zip"))
}

fn is_cuda_whisper_build(path: &Path) -> bool {
    let path_text = path.to_string_lossy().to_lowercase();
    if path_text.contains("cuda") || path_text.contains("cublas") {
        return true;
    }

    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        name.contains("ggml-cuda")
            || name.starts_with("cublas64")
            || name.starts_with("cublaslt64")
            || name.starts_with("cudart64")
    })
}

fn find_whisper_model(model_name: &str) -> Option<PathBuf> {
    if let Some(value) = env::var_os("STORYTELLER_WHISPER_MODEL").filter(|value| !value.is_empty())
    {
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
    if let Some(legacy) = find_named_file_in_legacy_roots(&file_name) {
        candidates.push(legacy);
    }
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join("models").join(&file_name));
    }

    candidates
        .into_iter()
        .find(|candidate| validate_nonempty_file(candidate, "Whisper model").is_ok())
}

fn find_named_file_in_legacy_roots(file_name: &str) -> Option<PathBuf> {
    let expected = file_name.to_lowercase();
    let mut budget = LEGACY_SCAN_ENTRY_LIMIT;
    for root in legacy_search_roots() {
        if let Some(path) = find_named_file_in_tree(&root, &expected, 0, &mut budget) {
            return Some(path);
        }
        if budget == 0 {
            break;
        }
    }
    None
}

fn find_named_file_in_tree(
    root: &Path,
    expected_lower: &str,
    depth: usize,
    budget: &mut usize,
) -> Option<PathBuf> {
    if depth > LEGACY_SCAN_DEPTH || *budget == 0 {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(value) => value,
            Err(_) => continue,
        };
        if file_type.is_file()
            && entry.file_name().to_string_lossy().to_lowercase() == expected_lower
        {
            return Some(path);
        }
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if matches!(name.as_str(), ".git" | "node_modules" | "target") {
                continue;
            }
            if let Some(found) = find_named_file_in_tree(&path, expected_lower, depth + 1, budget) {
                return Some(found);
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

fn install_whisper(tools_dir: &Path, temp_dir: &Path, prefer_cuda: bool) -> Result<(), String> {
    let zip_path = temp_dir.join("whisper.zip");
    let extract_dir = temp_dir.join("whisper");
    let target = tools_dir.join(executable_file_name("whisper-cli"));
    let script = format!(
        r#"$ErrorActionPreference='Stop';
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;
$headers=@{{'User-Agent'='StoryTeller-Lite'}};
$releases=Invoke-RestMethod -Headers $headers -Uri {api};
$assets=$releases | ForEach-Object {{ $_.assets }};
$asset=$null;
if ({prefer_cuda}) {{
  $asset=$assets | Where-Object {{ $_.name -match '(?i)^whisper-(cublas-.*-bin-x64|bin-win-cuda-.*x64)\.zip$' }} | Sort-Object name -Descending | Select-Object -First 1;
}}
if (-not $asset) {{ $asset=$assets | Where-Object {{ $_.name -eq 'whisper-bin-x64.zip' }} | Select-Object -First 1; }}
if (-not $asset) {{ throw 'No official Windows x64 whisper.cpp binary asset was found.' }}
$digest=[string]$asset.digest;
if (-not $digest.StartsWith('sha256:')) {{ throw 'The whisper.cpp release asset did not publish a SHA-256 digest.' }}
Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $asset.browser_download_url -OutFile {zip};
$expected=$digest.Substring(7).ToUpperInvariant();
$actual=(Get-FileHash {zip} -Algorithm SHA256).Hash.ToUpperInvariant();
if ($actual -ne $expected) {{ throw 'whisper.cpp SHA-256 verification failed.' }}
Expand-Archive -LiteralPath {zip} -DestinationPath {extract} -Force;
$cli=Get-ChildItem -LiteralPath {extract} -Filter whisper-cli.exe -Recurse | Select-Object -First 1;
if (-not $cli) {{ $cli=Get-ChildItem -LiteralPath {extract} -Filter main.exe -Recurse | Select-Object -First 1; }}
if (-not $cli) {{ throw 'Neither whisper-cli.exe nor legacy main.exe was present in the downloaded archive.' }}
Get-ChildItem -LiteralPath $cli.Directory.FullName | ForEach-Object {{ Copy-Item -LiteralPath $_.FullName -Destination {tools} -Recurse -Force }};
if ($cli.Name -ieq 'main.exe') {{ Copy-Item -LiteralPath $cli.FullName -Destination {target} -Force; }}"#,
        api = ps_string(WHISPER_RELEASES_API),
        prefer_cuda = if prefer_cuda { "$true" } else { "$false" },
        zip = ps_path(&zip_path),
        extract = ps_path(&extract_dir),
        tools = ps_path(tools_dir),
        target = ps_path(&target),
    );
    run_powershell(&script)
}

fn install_whisper_from_archive(
    archive: &Path,
    tools_dir: &Path,
    temp_dir: &Path,
) -> Result<(), String> {
    let extract_dir = temp_dir.join("legacy-whisper");
    let target = tools_dir.join(executable_file_name("whisper-cli"));
    let archive_name = archive
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let extraction = if archive_name.ends_with(".zip") {
        format!(
            "Expand-Archive -LiteralPath {} -DestinationPath {} -Force;",
            ps_path(archive),
            ps_path(&extract_dir)
        )
    } else {
        format!(
            "New-Item -ItemType Directory -Force {} | Out-Null; tar.exe -xf {} -C {}; if ($LASTEXITCODE -ne 0) {{ throw 'Could not extract the cached CUDA whisper.cpp archive.' }};",
            ps_path(&extract_dir),
            ps_path(archive),
            ps_path(&extract_dir)
        )
    };
    let script = format!(
        r#"$ErrorActionPreference='Stop';
{extraction}
$cli=Get-ChildItem -LiteralPath {extract} -Filter whisper-cli.exe -Recurse | Select-Object -First 1;
if (-not $cli) {{ $cli=Get-ChildItem -LiteralPath {extract} -Filter main.exe -Recurse | Select-Object -First 1; }}
if (-not $cli) {{ throw 'The cached CUDA archive did not contain whisper-cli.exe or legacy main.exe.' }}
Get-ChildItem -LiteralPath $cli.Directory.FullName | ForEach-Object {{ Copy-Item -LiteralPath $_.FullName -Destination {tools} -Recurse -Force }};
if ($cli.Name -ieq 'main.exe') {{ Copy-Item -LiteralPath $cli.FullName -Destination {target} -Force; }}"#,
        extraction = extraction,
        extract = ps_path(&extract_dir),
        tools = ps_path(tools_dir),
        target = ps_path(&target),
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

fn nvidia_gpu_available() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let mut candidates = Vec::new();
    if let Some(windir) = env::var_os("WINDIR") {
        candidates.push(
            PathBuf::from(windir)
                .join("System32")
                .join("nvidia-smi.exe"),
        );
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("NVIDIA Corporation")
                .join("NVSMI")
                .join("nvidia-smi.exe"),
        );
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|entry| entry.join("nvidia-smi.exe")));
    }
    candidates.into_iter().any(|candidate| {
        candidate.is_file()
            && probe_executable(&candidate, &["--query-gpu=name", "--format=csv,noheader"])
    })
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
        .ok_or_else(|| {
            "PowerShell was not found; automatic dependency download is unavailable.".to_string()
        })?;

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
    let detail = detail
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");
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
        return Err(format!(
            "{label} is not a non-empty regular file: {}",
            path.display()
        ));
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
        assert_eq!(
            ps_string("C:\\O'Brien\\tool.exe"),
            "'C:\\O''Brien\\tool.exe'"
        );
    }

    #[test]
    fn legacy_cuda_archive_name_matches_former_package() {
        assert!(is_legacy_cuda_archive_name(
            "whisper-cpp-windows-x64-cuda-13.1.0.tar.gz"
        ));
        assert!(!is_legacy_cuda_archive_name("whisper-bin-x64.zip"));
    }

    #[test]
    fn cuda_path_hint_is_recognized_without_dll_scan() {
        assert!(is_cuda_whisper_build(Path::new(
            "C:\\cache\\whisper-cpp-windows-x64-cuda-13.1.0\\whisper-cli.exe"
        )));
    }
}
