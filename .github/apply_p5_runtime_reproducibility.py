from pathlib import Path
import re


def sub_once(path, pattern, replacement, flags=re.S):
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    text, count = re.subn(pattern, replacement, text, count=1, flags=flags)
    if count != 1:
        raise SystemExit(f"expected one match in {path}, found {count}: {pattern}")
    p.write_text(text, encoding="utf-8")


runtime = "crates/storyteller-ui/src/runtime_setup.rs"
sub_once(
    runtime,
    r"        if status\.whisper_cli\.is_none\(\) \{.*?\n        \}\n\n        if status\.whisper_model\.is_none\(\) \{",
    '''        if status.whisper_cli.is_none() {
            let prefer_cuda = nvidia_gpu_available();
            progress(if prefer_cuda {
                format!(
                    "NVIDIA GPU detected; downloading and verifying pinned whisper.cpp CUDA build {WHISPER_BUILD_TAG}…"
                )
            } else {
                format!(
                    "Downloading and verifying pinned whisper.cpp CPU build {WHISPER_BUILD_TAG}…"
                )
            });
            install_whisper(&tools_dir, &temp_dir, prefer_cuda)?;
            let installed = tools_dir.join(executable_file_name("whisper-cli"));
            if !probe_executable(&installed, &["--help"]) {
                return Err("Downloaded whisper-cli could not be started.".into());
            }
        }

        if status.whisper_model.is_none() {''',
)
sub_once(
    runtime,
    r"fn persistent_whisper_executables\(\) -> Vec<PathBuf> \{.*?\n\}\n\nfn legacy_search_roots",
    '''fn persistent_whisper_executables() -> Vec<PathBuf> {
    let mut executables = Vec::new();
    let mut budget = LEGACY_SCAN_ENTRY_LIMIT;
    for root in legacy_search_roots() {
        scan_legacy_tree(&root, 0, &mut budget, &mut executables);
        if budget == 0 {
            break;
        }
    }
    executables
}

fn legacy_search_roots''',
)
sub_once(
    runtime,
    r"fn scan_legacy_tree\(.*?\n\}\n\nfn is_cuda_whisper_build",
    '''fn scan_legacy_tree(
    root: &Path,
    depth: usize,
    budget: &mut usize,
    executables: &mut Vec<PathBuf>,
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
        scan_legacy_tree(&path, depth + 1, budget, executables);
    }
}

fn is_cuda_whisper_build''',
)
sub_once(runtime, r"fn install_whisper_from_archive\(.*?\n\}\n\nfn install_model", "fn install_model")
sub_once(runtime, r"    #\[test\]\n    fn legacy_cuda_archive_name_matches_former_package\(\) \{.*?\n    \}\n\n", "")

docs = "docs/RUNTIME.md"
sub_once(
    docs,
    r"The runtime also recognizes the historical package naming pattern.*?sibling runtime DLLs/resources are copied with the executable\.\n",
    "Previously extracted StoryTeller/whisper runtimes remain valid discovery inputs, including legacy `main.exe` layouts. Automatic **Download missing** does not scan for or silently import archived builds: when no runnable CLI is available it downloads only the pinned, hash-verified StoryTeller-owned whisper.cpp build described below. Existing archives remain supported through the explicit **Import whisper archive…** action.\n",
)
sub_once(
    docs,
    r"- whisper\.cpp: official `ggml-org/whisper\.cpp` GitHub Windows x64 release assets;.*?compatible CUDA asset is unavailable\.\n",
    "- whisper.cpp: pinned `ggml-org/whisper.cpp` binary build `b5130`, built from commit `927cfce34f31707e17f2bff35c349632fb9e2c3a` (the same target commit as stable `v1.9.4`). CPU uses `whisper-bin-x64.zip` with SHA-256 `f9ec6c52a2e949b62ab51fa21d0d497958f9e41c3010c157c4e42932d5316f3c`; NVIDIA systems use `whisper-cublas-12.4.0-bin-x64.zip` with SHA-256 `af520ddd034d985b55dfeea3e465ed93653ba2aee1a55e865033edc548c272a7`. Lite does not query recent releases during automatic install.\n- `nvidia-smi` selects the pinned CUDA 12.4 archive; otherwise Lite selects the pinned CPU archive. A user who needs a different compatible whisper.cpp build can provide it through explicit runtime discovery/override or **Import whisper archive…**.\n",
)
print("runtime reproducibility patch applied")
