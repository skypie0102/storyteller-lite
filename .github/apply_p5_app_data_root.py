from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}: {old!r}")
    file_path.write_text(text.replace(old, new), encoding="utf-8")


app_paths = Path("crates/storyteller-ui/src/app_paths.rs")
if app_paths.exists():
    raise SystemExit(f"{app_paths} already exists")
app_paths.write_text(
    '''use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

const APP_DATA_DIR_NAME: &str = "Storyteller OneClick Lite";

pub(crate) fn persistent_app_root() -> Option<PathBuf> {
    root_from_local_app_data(env::var_os("LOCALAPPDATA"))
}

pub(crate) fn recovery_app_root() -> PathBuf {
    persistent_app_root().unwrap_or_else(|| app_root_from(&env::temp_dir()))
}

fn root_from_local_app_data(value: Option<OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| app_root_from(&root))
}

fn app_root_from(base: &Path) -> PathBuf {
    base.join(APP_DATA_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_root_name_is_stable() {
        assert_eq!(
            app_root_from(Path::new("local-app-data")),
            PathBuf::from("local-app-data").join(APP_DATA_DIR_NAME)
        );
    }

    #[test]
    fn empty_local_app_data_is_not_a_persistent_root() {
        assert_eq!(root_from_local_app_data(None), None);
        assert_eq!(root_from_local_app_data(Some(OsString::new())), None);
        assert_eq!(
            root_from_local_app_data(Some(OsString::from("local-app-data"))),
            Some(PathBuf::from("local-app-data").join(APP_DATA_DIR_NAME))
        );
    }
}
''',
    encoding="utf-8",
)

replace_exact(
    "crates/storyteller-ui/src/main.rs",
    "mod review_ui;\n",
    "mod app_paths;\nmod review_ui;\n",
)

replace_exact(
    "crates/storyteller-ui/src/runtime_import.rs",
    "use std::{\n    env, fs,\n",
    "use crate::app_paths::persistent_app_root;\nuse std::{\n    fs,\n",
)
replace_exact(
    "crates/storyteller-ui/src/runtime_import.rs",
    '''    let local_app_data = env::var_os("LOCALAPPDATA").ok_or_else(|| {
        "Windows LOCALAPPDATA is unavailable, so the persistent runtime folder could not be determined."
            .to_string()
    })?;
    let imports_root = PathBuf::from(local_app_data)
        .join("Storyteller OneClick Lite")
        .join("runtime")
        .join("whisper");
''',
    '''    let app_root = persistent_app_root().ok_or_else(|| {
        "Windows LOCALAPPDATA is unavailable, so the persistent runtime folder could not be determined."
            .to_string()
    })?;
    let imports_root = app_root.join("runtime").join("whisper");
''',
)

replace_exact(
    "crates/storyteller-ui/src/recovery_state.rs",
    "use std::{env, path::PathBuf};\n",
    "use crate::app_paths::recovery_app_root;\nuse std::path::PathBuf;\n",
)
replace_exact(
    "crates/storyteller-ui/src/recovery_state.rs",
    '''fn recovery_path() -> PathBuf {
    app_data_root().join("queue-recovery.json")
}

fn app_data_root() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("Storyteller OneClick Lite")
}
''',
    '''fn recovery_path() -> PathBuf {
    recovery_app_root().join("queue-recovery.json")
}
''',
)

replace_exact(
    "crates/storyteller-ui/src/runtime_setup.rs",
    "use std::{\n",
    "use crate::app_paths::persistent_app_root;\nuse std::{\n",
)
replace_exact(
    "crates/storyteller-ui/src/runtime_setup.rs",
    '''fn managed_app_root() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| managed_app_root_from(&root))
}

fn managed_app_root_from(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Storyteller OneClick Lite")
}

''',
    "",
)
replace_exact(
    "crates/storyteller-ui/src/runtime_setup.rs",
    "managed_app_root()",
    "persistent_app_root()",
    expected=4,
)
replace_exact(
    "crates/storyteller-ui/src/runtime_setup.rs",
    '''    #[test]
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
''',
    "",
)
