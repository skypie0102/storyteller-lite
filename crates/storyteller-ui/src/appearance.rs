use crate::AppWindow;
use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use std::{fs, io, path::Path, path::PathBuf};

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Default, Deserialize, Serialize)]
struct Appearance {
    #[serde(default)]
    theme: Theme,
}

pub(crate) fn install(ui: &AppWindow, path: PathBuf) {
    let appearance = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Appearance>(&bytes).ok())
        .unwrap_or_default();
    ui.set_dark_theme(matches!(appearance.theme, Theme::Dark));
    let weak = ui.as_weak();
    ui.on_theme_changed(move |dark| {
        if let Err(error) = save(&path, dark) {
            if let Some(ui) = weak.upgrade() {
                ui.set_status_text(
                    format!("Theme changed, but the preference could not be saved: {error}").into(),
                );
            }
        }
    });
}

fn save(path: &Path, dark: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let appearance = Appearance {
        theme: if dark { Theme::Dark } else { Theme::Light },
    };
    let bytes = serde_json::to_vec_pretty(&appearance).map_err(io::Error::other)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
