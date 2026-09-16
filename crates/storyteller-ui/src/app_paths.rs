use std::{
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
