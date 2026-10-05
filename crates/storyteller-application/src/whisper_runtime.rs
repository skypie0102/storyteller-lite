//! Optional pinned Windows CUDA runtime. The base installation remains Whistle-only.
use crate::{persistent_app_root, runtime_setup::*, RuntimeStatus};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const WHISPER_RELEASE: &str = "b5130";
pub const WHISPER_ARCHIVE_SHA256: &str =
    "0b29b2175bb17ec26da29677cbc7c467c57d103245144d62a49a703f6bc3fdae";
pub const WHISPER_MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
pub const WHISPER_MODEL_SHA256: &str =
    "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2";
pub const MIN_WHISPER_FREE_VRAM_MIB: u64 = 4096;

// Only these verified files are published from the official CUDA 11.8 archive.
const BUNDLE_FILES: &[(&str, &str)] = &[
    (
        "cudart64_110.dll",
        "ba5c2fb526c4ee4bb218ceb3fa5e8bfde89ce474f38711fdcce802549bf9fc6f",
    ),
    (
        "cuinj64_118.dll",
        "b97f3becdfa7d5652b456377ced74766f5e21cb52d2d6edac5bc6114d786f040",
    ),
    (
        "ggml-base.dll",
        "4aaeb79e4333d1ed0ebdec6b0f66ead6399474025200d6e1cee908727ede84ec",
    ),
    (
        "ggml-cpu-alderlake.dll",
        "6ba38a8afeffcf70ea3f0e7a102a7f85b0c114d09db16bf1d5ceacc4f6e90fd9",
    ),
    (
        "ggml-cpu-cannonlake.dll",
        "5ba615af56afc45ff3b745d54767bbf3212051ad7deefb7e75b5a03a8df87de4",
    ),
    (
        "ggml-cpu-cascadelake.dll",
        "38056401bdd11ac9fb1f85438c37b3bbbb36ba485968e9a229a232e2e516290e",
    ),
    (
        "ggml-cpu-haswell.dll",
        "81e5e339dcdb07d736a7a3bfecbcb229e6c13a48c5982f3eac48c0cccc116a9a",
    ),
    (
        "ggml-cpu-icelake.dll",
        "56152bd8b3050523f966e42c49a181e17567f6d1abecafb8bfacd578e217ad58",
    ),
    (
        "ggml-cpu-sandybridge.dll",
        "c41a325cbdb0b94834d92592d45f680307cce1cf317142b7618338b2e45d337d",
    ),
    (
        "ggml-cpu-skylakex.dll",
        "f17a78fb30bd1da16998a176ec74f9ff47c14a318f7432ab2e94c3f62fe12d36",
    ),
    (
        "ggml-cpu-sse42.dll",
        "4ae79d2dd881be237f2e3553fb7915c262f0d4ca6b786dff84e200dce20dab17",
    ),
    (
        "ggml-cpu-x64.dll",
        "80c819dfdda94054dfa30c9befa6b8d42be3b24972a8bc79e160c5dca4320490",
    ),
    (
        "ggml-cuda.dll",
        "d16717ed5923feb18e99c99d7aafef28dd84c83a74748e7af73c454d13ba9404",
    ),
    (
        "ggml.dll",
        "c4045c2d73aa6b099c58a67ecff12d58d567b91d6db42baff85fd0dcf22f8fb2",
    ),
    (
        "nvrtc-builtins64_118.dll",
        "8642cd940445f0eb79ae73c04b3dfb8c8e61fc3aca191dc2239b28a9cb6964d8",
    ),
    (
        "nvrtc64_112_0.dll",
        "7a1d6c894d9db5f043e7cca5ea328bc4f29c45aa7af3f0dd1bde1f31f02b41bd",
    ),
    (
        "whisper-cli.exe",
        "71ba6d5fc3eea6113d993ea5db46f8633472583ac965b65342a990eb793d6797",
    ),
    (
        "whisper.dll",
        "6971177f718779941900a08e4d3e81d4edb1fb52be2c0b6296c0479071c221ff",
    ),
];

#[derive(Debug, Clone)]
pub struct NvidiaGpu {
    pub name: String,
    pub free_vram_mib: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WhisperRuntimeStatus {
    pub cli: Option<PathBuf>,
    pub model: Option<PathBuf>,
    pub gpu: Option<NvidiaGpu>,
    pub gpu_text: String,
}

impl WhisperRuntimeStatus {
    pub fn hardware_ready(&self) -> bool {
        cfg!(all(windows, target_arch = "x86_64"))
            && self
                .gpu
                .as_ref()
                .is_some_and(|gpu| gpu.free_vram_mib >= MIN_WHISPER_FREE_VRAM_MIB)
    }
    pub fn ready(&self) -> bool {
        self.hardware_ready() && self.cli.is_some() && self.model.is_some()
    }
    pub fn summary(&self) -> String {
        if !self.hardware_ready() {
            return self.gpu_text.clone();
        }
        if self.cli.is_none() || self.model.is_none() {
            "Whisper GPU is optional — download its engine and Turbo model to use it.".into()
        } else {
            "Ready — Whisper Turbo on NVIDIA GPU · 1 worker".into()
        }
    }
    pub fn engine_text(&self) -> String {
        dependency_text(self.cli.as_deref())
    }
    pub fn model_text(&self) -> String {
        dependency_text(self.model.as_deref())
    }
}

pub(crate) fn detect_whisper_runtime() -> WhisperRuntimeStatus {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return WhisperRuntimeStatus {
            gpu_text: "Whisper GPU currently supports NVIDIA on Windows x64.".into(),
            ..Default::default()
        };
    }
    let gpu = detect_nvidia_gpu();
    let gpu_text = match &gpu {
        Some(gpu) if gpu.free_vram_mib >= MIN_WHISPER_FREE_VRAM_MIB => format!("{} · {:.1} GB free VRAM · 1 GPU worker", gpu.name, gpu.free_vram_mib as f64 / 1024.0),
        Some(gpu) => format!("{} · {:.1} GB free VRAM. Whisper needs at least 4 GB free; close other GPU apps or choose Whistle.", gpu.name, gpu.free_vram_mib as f64 / 1024.0),
        None => "No compatible NVIDIA GPU detected. Choose Whistle or install your NVIDIA driver, then check setup.".into(),
    };
    let mut cli_candidates = Vec::new();
    let mut model_candidates = Vec::new();
    if let Some(path) = env::var_os("STORYTELLER_WHISPER").filter(|s| !s.is_empty()) {
        cli_candidates.push(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("STORYTELLER_WHISPER_MODEL").filter(|s| !s.is_empty()) {
        model_candidates.push(PathBuf::from(path));
    }
    let roots = persistent_app_root().into_iter().chain(
        env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf)),
    );
    for root in roots {
        cli_candidates.push(root.join("tools/whisper-cuda/whisper-cli.exe"));
        model_candidates.push(root.join("models/ggml-large-v3-turbo-q5_0.bin"));
    }
    WhisperRuntimeStatus {
        cli: cli_candidates.into_iter().find(|p| verified_bundle(p)),
        model: model_candidates
            .into_iter()
            .find(|p| verified_file(p, WHISPER_MODEL_SHA256)),
        gpu,
        gpu_text,
    }
}

fn detect_nvidia_gpu() -> Option<NvidiaGpu> {
    // Inspect CUDA's first device in PCI order, matching the invocation below.
    if env::var_os("CUDA_VISIBLE_DEVICES").is_some() {
        return None;
    }
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=pci.bus_id,name,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_nvidia_inventory(&String::from_utf8_lossy(&output.stdout))
}

fn parse_nvidia_inventory(csv: &str) -> Option<NvidiaGpu> {
    let mut devices = csv
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    devices.sort_unstable();
    let fields = devices
        .first()?
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>();
    if fields.len() != 3 || fields[1].is_empty() {
        return None;
    }
    Some(NvidiaGpu {
        name: fields[1].into(),
        free_vram_mib: fields[2].parse().ok()?,
    })
}

fn verified_bundle(cli: &Path) -> bool {
    if cli.file_name().and_then(|s| s.to_str()) != Some("whisper-cli.exe") {
        return false;
    }
    let Some(dir) = cli.parent() else {
        return false;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    // ggml dynamically discovers backends. Extra binaries cannot join a pinned bundle.
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches!(
            entry
                .path()
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("dll" | "exe")
        ) && !BUNDLE_FILES.iter().any(|(file, _)| *file == name)
        {
            return false;
        }
    }
    BUNDLE_FILES
        .iter()
        .all(|(file, hash)| verified_file(&dir.join(file), hash))
}

pub fn install_whisper_dependencies(
    runtime: &RuntimeStatus,
    mut progress: impl FnMut(String),
) -> Result<(), String> {
    if !runtime.whisper.hardware_ready() {
        return Err(runtime.whisper.gpu_text.clone());
    }
    install_ffmpeg_if_missing(runtime, &mut progress)?;
    let root = persistent_app_root().ok_or("Windows LOCALAPPDATA is unavailable.")?;
    if runtime.whisper.cli.is_none() {
        progress("Downloading and verifying Whisper CUDA (273 MB)…".into());
        let archive = root.join("tools/whisper-cuda.download.zip");
        let url = format!("https://github.com/ggml-org/whisper.cpp/releases/download/{WHISPER_RELEASE}/whisper-cublas-11.8.0-bin-x64.zip");
        download_verified(&url, &archive, WHISPER_ARCHIVE_SHA256)?;
        let expanded = root.join("tools/whisper-cuda.extract.tmp");
        let staged = root.join("tools/whisper-cuda.publish.tmp");
        let destination = root.join("tools/whisper-cuda");
        let backup = root.join("tools/whisper-cuda.previous.tmp");
        let copies = BUNDLE_FILES.iter().map(|(name, hash)| format!("$source = Join-Path $expanded 'Release/{name}'\nif ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant() -ne '{hash}') {{ throw 'Whisper bundle checksum mismatch' }}\nCopy-Item -LiteralPath $source -Destination (Join-Path $staged '{name}')\n")).collect::<String>();
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$expanded = {expanded}
$staged = {staged}
$destination = {destination}
$backup = {backup}
try {{
    foreach ($path in @($expanded, $staged)) {{ if (Test-Path -LiteralPath $path) {{ Remove-Item -LiteralPath $path -Recurse -Force }} }}
    if (Test-Path -LiteralPath $backup) {{
        if (!(Test-Path -LiteralPath $destination)) {{ Move-Item -LiteralPath $backup -Destination $destination }}
        else {{ Remove-Item -LiteralPath $backup -Recurse -Force }}
    }}
    Expand-Archive -LiteralPath {archive} -DestinationPath $expanded
    New-Item -ItemType Directory -Path $staged | Out-Null
    {copies}
    [System.IO.File]::WriteAllText((Join-Path $staged 'LICENSE-whisper.txt'), {license})
    if (Test-Path -LiteralPath $destination) {{ Move-Item -LiteralPath $destination -Destination $backup }}
    try {{ Move-Item -LiteralPath $staged -Destination $destination }}
    catch {{ if (Test-Path -LiteralPath $backup) {{ Move-Item -LiteralPath $backup -Destination $destination }}; throw }}
    if (Test-Path -LiteralPath $backup) {{ Remove-Item -LiteralPath $backup -Recurse -Force }}
}} finally {{
    foreach ($path in @($expanded, $staged, {archive})) {{ if (Test-Path -LiteralPath $path) {{ Remove-Item -LiteralPath $path -Recurse -Force }} }}
}}
"#,
            expanded = ps_path(&expanded),
            staged = ps_path(&staged),
            destination = ps_path(&destination),
            backup = ps_path(&backup),
            archive = ps_path(&archive),
            license = ps_string(include_str!("../assets/whisper-LICENSE.txt"))
        );
        run_powershell(&script)?;
        if !verified_bundle(&destination.join("whisper-cli.exe")) {
            return Err("Installed Whisper bundle failed verification.".into());
        }
    }
    if runtime.whisper.model.is_none() {
        progress("Downloading and verifying Whisper Turbo Q5 (574 MB)…".into());
        let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/{WHISPER_MODEL_REVISION}/ggml-large-v3-turbo-q5_0.bin");
        download_verified(
            &url,
            &root.join("models/ggml-large-v3-turbo-q5_0.bin"),
            WHISPER_MODEL_SHA256,
        )?;
    }
    Ok(())
}

pub(crate) fn whisper_command(
    executable: &Path,
    model: &Path,
    inputs: &[(PathBuf, PathBuf)],
) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--model")
        .arg(model)
        .args([
            "--language",
            "en",
            "--device",
            "0",
            "--threads",
            "2",
            "--output-json-full",
            "--print-progress",
            "--suppress-nst",
        ])
        .env("CUDA_DEVICE_ORDER", "PCI_BUS_ID")
        .env_remove("GGML_BACKEND_PATH");
    // The pinned CLI loads one context before looping over input files. It pairs
    // each input with the output prefix at the same position and resets text
    // context between files (whisper_full_default_params.no_context = true).
    for (audio, output_prefix) in inputs {
        command.arg("--file").arg(audio);
        command.arg("--output-file").arg(output_prefix);
    }
    command
}

pub(crate) fn cuda_initialization_failed(line: &str) -> bool {
    line.strip_prefix("whisper_backend_init_gpu: ")
        .is_some_and(|diagnostic| {
            diagnostic == "no GPU found"
                || diagnostic
                    .strip_prefix("failed to initialize ")
                    .is_some_and(|backend| backend.ends_with(" backend"))
        })
}

// Callers supply stderr alone. Spoken words cannot prove or disprove offload.
pub(crate) fn require_cuda_offload(stderr: &str) -> Result<(), String> {
    let device = stderr.lines().find_map(|line| {
        let device = line
            .strip_prefix("whisper_backend_init_gpu: using ")?
            .strip_suffix(" backend")?;
        let index = device.strip_prefix("CUDA")?;
        (!index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())).then_some(device)
    });
    let weights_on_cuda = device.is_some_and(|device| {
        stderr.lines().any(|line| {
            let Some(diagnostic) = line.strip_prefix("whisper_model_load:") else {
                return false;
            };
            let Some(size) = diagnostic
                .trim_start()
                .strip_prefix(device)
                .and_then(|rest| rest.strip_prefix(" total size ="))
                .and_then(|rest| rest.trim().strip_suffix(" MB"))
                .and_then(|size| size.trim().parse::<f64>().ok())
            else {
                return false;
            };
            size.is_finite() && size > 0.0
        })
    });
    if !weights_on_cuda || stderr.lines().any(cuda_initialization_failed) {
        return Err("Whisper could not confirm CUDA offload. Check your NVIDIA driver and free VRAM, or select Whistle for a new book. The saved backend has not been changed.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_uses_first_pci_device_and_rejects_unknown_memory() {
        let gpu = parse_nvidia_inventory(
            "00000000:02:00.0, Second GPU, 9000\n00000000:01:00.0, First GPU, 5120\n",
        )
        .unwrap();
        assert_eq!(gpu.name, "First GPU");
        assert_eq!(gpu.free_vram_mib, 5120);
        assert!(parse_nvidia_inventory("0000:01:00.0, GPU, N/A").is_none());
        assert!(parse_nvidia_inventory("").is_none());
    }
    #[test]
    fn gpu_requested_or_compiled_is_not_offload_proof() {
        let gpu = "whisper_backend_init_gpu: using CUDA0 backend\nwhisper_model_load: CUDA0 total size = 580.00 MB\n";
        require_cuda_offload(gpu).unwrap();
        for log in [
            "use gpu = 1",
            "CUDA = 1",
            "whisper_backend_init_gpu: using CPU backend",
            "whisper_backend_init_gpu: using CUDA0 backend",
            "whisper_backend_init_gpu: no GPU found",
        ] {
            assert!(require_cuda_offload(log).is_err());
        }
        assert!(require_cuda_offload(&format!(
            "{gpu}whisper_backend_init_gpu: failed to initialize CUDA0 backend"
        ))
        .is_err());
        require_cuda_offload(&format!(
            "{gpu}The story said no GPU found and failed to initialize.\n"
        ))
        .unwrap();
        for weights in [
            "whisper_model_load: CUDA1 total size = 580.00 MB",
            "whisper_model_load: CUDA0 total size = 0.00 MB",
            "whisper_model_load: CUDA0 total size = NaN MB",
            "[00:01] whisper_model_load: CUDA0 total size = 580.00 MB",
        ] {
            assert!(require_cuda_offload(&format!(
                "whisper_backend_init_gpu: using CUDA0 backend\n{weights}"
            ))
            .is_err());
        }
    }
    #[test]
    fn command_forces_english_and_single_device_without_disabling_logs() {
        let command = whisper_command(
            Path::new("whisper-cli.exe"),
            Path::new("C:/O'Brien/model.bin"),
            &[
                ("audio with spaces.wav".into(), "chunk result".into()),
                ("second audio.wav".into(), "second result".into()),
            ],
        );
        let args = command
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|v| v == ["--language", "en"]));
        assert!(args.windows(2).any(|v| v == ["--device", "0"]));
        assert!(args.iter().any(|v| v == "--output-json-full"));
        assert!(args.iter().any(|v| v == "--print-progress"));
        assert!(!args
            .iter()
            .any(|v| v == "--no-gpu" || v == "--no-prints" || v == "--translate"));
        assert!(args.iter().any(|v| v == "C:/O'Brien/model.bin"));
        assert_eq!(BUNDLE_FILES.len(), 18);
        assert!(!verified_bundle(Path::new("missing/whisper-cli.exe")));
    }
}
