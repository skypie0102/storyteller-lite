from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected exactly one match in {path}, found {count}: {old[:80]!r}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


runtime = "crates/storyteller-ui/src/runtime_setup.rs"

replace_once(
    runtime,
    '''    let app_root = current_executable_dir().ok_or_else(|| {
        "Could not determine the StoryTeller Lite application folder.".to_string()
    })?;
    let tools_dir = app_root.join("tools");
    let models_dir = app_root.join("models");
''',
    '''    let app_root = managed_app_root().ok_or_else(|| {
        "Windows LOCALAPPDATA is unavailable, so the persistent StoryTeller Lite runtime folder could not be determined."
            .to_string()
    })?;
    let tools_dir = app_root.join("tools");
    let models_dir = app_root.join("models");
''',
)

replace_once(
    runtime,
    '''    if let Some(executable_dir) = current_executable_dir() {
        candidates.push(executable_dir.join("tools").join(&file_name));
        candidates.push(executable_dir.join(&file_name));
    }
    if let Some(path) = env::var_os("PATH") {
''',
    '''    if let Some(executable_dir) = current_executable_dir() {
        candidates.push(executable_dir.join("tools").join(&file_name));
        candidates.push(executable_dir.join(&file_name));
    }
    if let Some(app_root) = managed_app_root() {
        candidates.push(app_root.join("tools").join(&file_name));
    }
    if let Some(path) = env::var_os("PATH") {
''',
)

replace_once(
    runtime,
    '''    for path in persistent_whisper_executables() {
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
''',
    '''    if let Some(app_root) = managed_app_root() {
        for name in &names {
            candidates.push(WhisperCandidate {
                path: app_root.join("tools").join(name),
                source_rank: 1,
            });
        }
    }

    for path in persistent_whisper_executables() {
        candidates.push(WhisperCandidate {
            path,
            source_rank: 2,
        });
    }

    if let Some(path) = env::var_os("PATH") {
        for entry in env::split_paths(&path) {
            for name in &names {
                candidates.push(WhisperCandidate {
                    path: entry.join(name),
                    source_rank: 3,
                });
            }
        }
    }
''',
)

replace_once(
    runtime,
    '''    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local_app_data)
                .join("Storyteller OneClick Lite")
                .join("models")
                .join(&file_name),
        );
    }
''',
    '''    if let Some(app_root) = managed_app_root() {
        candidates.push(app_root.join("models").join(&file_name));
    }
''',
)

replace_once(
    runtime,
    '''fn current_executable_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn executable_file_name(base_name: &str) -> String {
''',
    '''fn managed_app_root() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| managed_app_root_from(&root))
}

fn managed_app_root_from(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Storyteller OneClick Lite")
}

fn current_executable_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn executable_file_name(base_name: &str) -> String {
''',
)

replace_once(
    runtime,
    '''    #[test]
    fn cuda_path_hint_is_recognized_without_dll_scan() {
        assert!(is_cuda_whisper_build(Path::new(
            "C:\\\\cache\\\\whisper-cpp-windows-x64-cuda-13.1.0\\\\whisper-cli.exe"
        )));
    }
}
''',
    '''    #[test]
    fn cuda_path_hint_is_recognized_without_dll_scan() {
        assert!(is_cuda_whisper_build(Path::new(
            "C:\\\\cache\\\\whisper-cpp-windows-x64-cuda-13.1.0\\\\whisper-cli.exe"
        )));
    }

    #[test]
    fn managed_runtime_root_is_per_user() {
        let root = managed_app_root_from(Path::new("local-app-data"));
        assert_eq!(
            root,
            PathBuf::from("local-app-data").join("Storyteller OneClick Lite")
        );
        assert_eq!(
            root.join("tools").join(executable_file_name("ffmpeg")),
            PathBuf::from("local-app-data")
                .join("Storyteller OneClick Lite")
                .join("tools")
                .join(executable_file_name("ffmpeg"))
        );
    }
}
''',
)

runtime_doc = "docs/RUNTIME.md"
replace_once(
    runtime_doc,
    "The runtime checks an explicit `STORYTELLER_FFMPEG` override first, then the portable application/tool folders and `PATH`. Settings probes the resolved executable with `ffmpeg -version` before reporting it ready.",
    "The runtime checks an explicit `STORYTELLER_FFMPEG` override first, then the portable application/tool folders, the managed per-user `tools/` folder under `%LOCALAPPDATA%\\Storyteller OneClick Lite`, and `PATH`. Settings probes the resolved executable with `ffmpeg -version` before reporting it ready.",
)
replace_once(
    runtime_doc,
    "- the portable StoryTeller application and `tools/` folders;\n- `PATH`;\n- bounded persistent-runtime searches under `%LOCALAPPDATA%`, `%APPDATA%`, `%PROGRAMDATA%`, the user's profile, and `.cache`;",
    "- the portable StoryTeller application and adjacent `tools/` folders;\n- the managed per-user `%LOCALAPPDATA%\\Storyteller OneClick Lite\\tools\\` folder;\n- `PATH`;\n- bounded persistent-runtime searches under `%LOCALAPPDATA%`, `%APPDATA%`, `%PROGRAMDATA%`, the user's profile, and `.cache`;",
)
replace_once(
    runtime_doc,
    "If a compatible cached archive is found but no runnable CLI is available, **Download missing** tries to reuse/extract that archive into the current portable `tools/` folder before downloading a replacement.",
    "If a compatible cached archive is found but no runnable CLI is available, **Download missing** tries to reuse/extract that archive into the managed per-user `tools/` folder before downloading a replacement.",
)
replace_once(
    runtime_doc,
    "If anything is missing, **Download missing** offers an explicit, user-initiated portable install on Windows.",
    "If anything is missing, **Download missing** offers an explicit, user-initiated per-user install on Windows. Owned downloads do not require the application/EXE directory itself to be writable.",
)
replace_once(
    runtime_doc,
    '''Downloads are performed on a background thread so the Slint UI remains responsive. Missing dependencies are installed beside the portable application:

```text
tools/ffmpeg.exe
tools/whisper-cli.exe
models/ggml-large-v3-turbo.bin
```
''',
    '''Downloads are performed on a background thread so the Slint UI remains responsive. Missing dependencies are installed into the managed per-user application-data root:

```text
%LOCALAPPDATA%\\Storyteller OneClick Lite\\tools\\ffmpeg.exe
%LOCALAPPDATA%\\Storyteller OneClick Lite\\tools\\whisper-cli.exe
%LOCALAPPDATA%\\Storyteller OneClick Lite\\models\\ggml-large-v3-turbo.bin
```

Portable adjacent `tools/` / `models/` resources remain valid discovery inputs for deliberately self-contained bundles, but automatic downloads no longer mutate the application directory.
''',
)
replace_once(
    runtime_doc,
    "After installation, StoryTeller Lite probes ffmpeg and whisper.cpp again and re-runs dependency detection before displaying the final state. If one dependency succeeds and a later dependency fails, the successful portable file is retained and the next attempt downloads only what is still missing.",
    "After installation, StoryTeller Lite probes ffmpeg and whisper.cpp again and re-runs dependency detection before displaying the final state. If one dependency succeeds and a later dependency fails, the successful per-user managed file is retained and the next attempt downloads only what is still missing.",
)
replace_once(
    runtime_doc,
    "Automatic installation is currently Windows-focused and uses PowerShell. The portable application directory must be writable; a build placed under a protected directory such as `Program Files` may need to be moved to a user-writable folder before installing dependencies. The current downloader does not expose mid-download cancellation yet.",
    "Automatic installation is currently Windows-focused and uses PowerShell. `%LOCALAPPDATA%` must be available and writable for managed downloads, so an installed EXE may live under a protected directory such as `Program Files` without requiring elevation merely to install StoryTeller-owned runtime dependencies. The current downloader does not expose mid-download cancellation yet.",
)

workflow = ".github/workflows/windows-build.yml"
replace_once(
    workflow,
    '''          Automatic downloads remain user-initiated. This is a developer-test build,
          not a public release.
''',
    '''          Automatic downloads remain user-initiated. Verified dependencies are stored
          per user under:
            %LOCALAPPDATA%\\Storyteller OneClick Lite\\tools\\
            %LOCALAPPDATA%\\Storyteller OneClick Lite\\models\\
          so the package/executable directory does not need to be writable.

          This is a developer-test build, not a public release.
''',
)
