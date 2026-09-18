use crate::app_paths::persistent_app_root;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const IMPORT_SCAN_DEPTH: usize = 8;
const IMPORT_SCAN_ENTRY_LIMIT: usize = 8_000;

pub(crate) fn import_whisper_archive(archive: &Path) -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err("Whisper archive import is currently supported on Windows only.".into());
    }

    let metadata = fs::metadata(archive).map_err(|error| {
        format!(
            "Could not read the selected whisper.cpp archive {}: {error}",
            archive.display()
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "The selected whisper.cpp archive is not a non-empty file: {}",
            archive.display()
        ));
    }
    if !is_supported_archive(archive) {
        return Err(
            "Choose a whisper.cpp .zip, .tgz, or .tar.gz archive. The archive is not uploaded anywhere."
                .into(),
        );
    }

    let app_root = persistent_app_root().ok_or_else(|| {
        "Windows LOCALAPPDATA is unavailable, so the persistent runtime folder could not be determined."
            .to_string()
    })?;
    let imports_root = app_root.join("runtime").join("whisper");
    fs::create_dir_all(&imports_root).map_err(|error| {
        format!(
            "Could not create persistent whisper.cpp runtime folder {}: {error}",
            imports_root.display()
        )
    })?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let label = archive_label(archive);
    let install_dir = imports_root.join(format!("{label}-{stamp}"));
    fs::create_dir_all(&install_dir).map_err(|error| {
        format!(
            "Could not create whisper.cpp import folder {}: {error}",
            install_dir.display()
        )
    })?;

    let result = (|| {
        extract_archive(archive, &install_dir)?;
        let cli = find_whisper_cli(&install_dir).ok_or_else(|| {
            "The selected archive did not contain whisper-cli.exe or legacy main.exe.".to_string()
        })?;
        if !probe_whisper(&cli) {
            return Err(format!(
                "The imported whisper.cpp executable could not start successfully: {}. Its CUDA/runtime DLLs may be missing or incompatible with this PC.",
                cli.display()
            ));
        }
        Ok(cli)
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&install_dir);
    }
    result
}

fn is_supported_archive(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name.ends_with(".zip") || name.ends_with(".tgz") || name.ends_with(".tar.gz")
}

fn archive_label(path: &Path) -> String {
    let mut name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "whisper-import".into());
    for suffix in [".tar.gz", ".tgz", ".zip"] {
        if name.to_lowercase().ends_with(suffix) {
            name.truncate(name.len().saturating_sub(suffix.len()));
            break;
        }
    }
    let sanitized = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('-').is_empty() {
        "whisper-import".into()
    } else {
        sanitized
    }
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<(), String> {
    let archive_name = archive
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let script = if archive_name.ends_with(".zip") {
        format!(
            "$ErrorActionPreference='Stop'; Expand-Archive -LiteralPath {} -DestinationPath {} -Force;",
            ps_path(archive),
            ps_path(destination)
        )
    } else {
        format!(
            "$ErrorActionPreference='Stop'; tar.exe -xf {} -C {}; if ($LASTEXITCODE -ne 0) {{ throw 'tar.exe could not extract the selected whisper.cpp archive.' }};",
            ps_path(archive),
            ps_path(destination)
        )
    };
    run_powershell(&script)
}

fn find_whisper_cli(root: &Path) -> Option<PathBuf> {
    let mut budget = IMPORT_SCAN_ENTRY_LIMIT;
    let mut legacy_main = None;
    find_whisper_cli_in_tree(root, 0, &mut budget, &mut legacy_main).or(legacy_main)
}

fn find_whisper_cli_in_tree(
    root: &Path,
    depth: usize,
    budget: &mut usize,
    legacy_main: &mut Option<PathBuf>,
) -> Option<PathBuf> {
    if depth > IMPORT_SCAN_DEPTH || *budget == 0 {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let file_type = match entry.file_type() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let path = entry.path();
        if file_type.is_file() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name == "whisper-cli.exe" {
                return Some(path);
            }
            if name == "main.exe" && legacy_main.is_none() {
                *legacy_main = Some(path);
            }
            continue;
        }
        if file_type.is_dir() {
            if let Some(found) = find_whisper_cli_in_tree(&path, depth + 1, budget, legacy_main) {
                return Some(found);
            }
        }
    }
    None
}

fn probe_whisper(path: &Path) -> bool {
    Command::new(path)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
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
        .ok_or_else(|| "PowerShell was not found; archive import is unavailable.".to_string())?;

    let output = Command::new(shell)
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script)
        .output()
        .map_err(|error| format!("Could not start the whisper.cpp archive importer: {error}"))?;
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
    if detail.is_empty() {
        Err("The selected whisper.cpp archive could not be extracted.".into())
    } else {
        Err(format!(
            "The selected whisper.cpp archive could not be extracted: {detail}"
        ))
    }
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
    fn accepts_former_cuda_archive_shape() {
        assert!(is_supported_archive(Path::new(
            "whisper-cpp-windows-x64-cuda-13.1.0.tar.gz"
        )));
    }

    #[test]
    fn archive_label_preserves_cuda_identity() {
        assert_eq!(
            archive_label(Path::new("whisper-cpp-windows-x64-cuda-13.1.0.tar.gz")),
            "whisper-cpp-windows-x64-cuda-13.1.0"
        );
    }
}
