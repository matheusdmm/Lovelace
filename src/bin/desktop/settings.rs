use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State, Theme, WebviewWindow, WebviewWindowBuilder};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Appearance {
    #[default]
    System,
    Light,
    Dark,
    Loveless,
}

impl Appearance {
    fn native(self) -> Option<Theme> {
        match self {
            Self::System => None,
            Self::Light => Some(Theme::Light),
            Self::Dark | Self::Loveless => Some(Theme::Dark),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Backdrop {
    #[default]
    Solid,
    Mica,
    Acrylic,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SpokenLanguage {
    #[default]
    Pt,
    En,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub(super) struct Preferences {
    appearance: Appearance,
    backdrop: Backdrop,
    spoken_language: SpokenLanguage,
}

fn read_preferences(path: &Path) -> Result<Preferences> {
    match fs::File::open(path) {
        Ok(file) => serde_json::from_reader(std::io::BufReader::new(file))
            .context("Could not read settings"),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Preferences::default()),
        Err(error) => Err(error).context("Could not open settings"),
    }
}

fn write_preferences(path: &Path, preferences: Preferences) -> Result<()> {
    let directory = path.parent().context("Settings folder is unavailable")?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), &preferences)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

pub(super) struct SettingsStore {
    path: PathBuf,
    preferences: Mutex<Preferences>,
    backdrops: Vec<Backdrop>,
}

#[derive(Serialize)]
pub(super) struct SettingsSnapshot {
    preferences: Preferences,
    backdrops: Vec<Backdrop>,
}

#[cfg(windows)]
fn supported_backdrops() -> Vec<Backdrop> {
    let mut effects = vec![Backdrop::Solid];
    let build = windows_version::OsVersion::current().build;
    if build >= 22000 {
        effects.push(Backdrop::Mica);
    }
    if build >= 17763 {
        effects.push(Backdrop::Acrylic);
    }
    effects
}

#[cfg(not(windows))]
fn supported_backdrops() -> Vec<Backdrop> {
    vec![Backdrop::Solid]
}

#[cfg(windows)]
fn set_backdrop(window: &WebviewWindow, previous: Backdrop, next: Preferences) -> Result<()> {
    use window_vibrancy::{apply_acrylic, apply_mica, clear_acrylic, clear_mica};

    if previous != next.backdrop {
        match previous {
            Backdrop::Solid => (),
            Backdrop::Mica => clear_mica(window)?,
            Backdrop::Acrylic => clear_acrylic(window)?,
        }
    }
    match next.backdrop {
        Backdrop::Solid => (),
        Backdrop::Mica => apply_mica(
            window,
            next.appearance.native().map(|theme| theme == Theme::Dark),
        )?,
        Backdrop::Acrylic => apply_acrylic(window, None)?,
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_backdrop(_window: &WebviewWindow, _previous: Backdrop, next: Preferences) -> Result<()> {
    ensure!(
        next.backdrop == Backdrop::Solid,
        "Window effects are not available on this system"
    );
    Ok(())
}

pub(super) fn setup(app: &mut tauri::App) -> Result<()> {
    let path = app.path().app_config_dir()?.join("settings.json");
    let backdrops = supported_backdrops();
    let mut preferences = read_preferences(&path).unwrap_or_else(|error| {
        eprintln!("{error:#}");
        Preferences::default()
    });
    if !backdrops.contains(&preferences.backdrop) {
        preferences.backdrop = Backdrop::Solid;
    }

    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .context("Main window is not configured")?
        .clone();
    config.transparent = backdrops.len() > 1;
    config.theme = preferences.appearance.native();
    config.visible = false;
    let window = WebviewWindowBuilder::from_config(app, &config)?.build()?;
    if let Err(error) = set_backdrop(&window, Backdrop::Solid, preferences) {
        eprintln!("{error:#}");
        preferences.backdrop = Backdrop::Solid;
    }
    app.manage(SettingsStore {
        path,
        preferences: Mutex::new(preferences),
        backdrops,
    });
    window.show()?;
    Ok(())
}

#[tauri::command]
pub(super) fn get_settings(state: State<'_, SettingsStore>) -> SettingsSnapshot {
    SettingsSnapshot {
        preferences: *state.preferences.lock().unwrap(),
        backdrops: state.backdrops.clone(),
    }
}

fn apply_preferences(
    window: &WebviewWindow,
    previous: Preferences,
    next: Preferences,
) -> Result<()> {
    if previous.appearance != next.appearance {
        window.set_theme(next.appearance.native())?;
    }
    if previous.backdrop != next.backdrop || previous.appearance != next.appearance {
        set_backdrop(window, previous.backdrop, next)?;
    }
    Ok(())
}

#[tauri::command]
pub(super) fn save_settings(
    window: WebviewWindow,
    state: State<'_, SettingsStore>,
    preferences: Preferences,
) -> Result<(), String> {
    let save = || -> Result<()> {
        ensure!(
            state.backdrops.contains(&preferences.backdrop),
            "This window effect is not available on this system"
        );
        let mut current = state.preferences.lock().unwrap();
        apply_preferences(&window, *current, preferences)
            .and_then(|()| {
                write_preferences(&state.path, preferences).context("Could not save settings")
            })
            .inspect_err(|_| {
                let _ = apply_preferences(&window, preferences, *current);
            })?;
        *current = preferences;
        Ok(())
    };
    save().map_err(super::error_message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loveless_uses_dark_window_chrome_and_survives_reopening() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("settings.json");
        let settings = Preferences {
            appearance: Appearance::Loveless,
            ..Preferences::default()
        };
        assert_eq!(settings.appearance.native(), Some(Theme::Dark));
        write_preferences(&path, settings)?;
        let saved: serde_json::Value = serde_json::from_reader(fs::File::open(&path)?)?;
        assert_eq!(saved["appearance"], "loveless");
        assert_eq!(read_preferences(&path)?, settings);
        Ok(())
    }

    #[test]
    fn first_launch_defaults_and_saved_preferences_survive_reopening() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("config").join("settings.json");
        assert_eq!(read_preferences(&path)?, Preferences::default());
        let settings = Preferences {
            appearance: Appearance::Dark,
            backdrop: Backdrop::Mica,
            spoken_language: SpokenLanguage::En,
        };
        write_preferences(&path, settings)?;
        assert_eq!(read_preferences(&path)?, settings);
        write_preferences(&path, Preferences::default())?;
        assert_eq!(read_preferences(&path)?, Preferences::default());
        Ok(())
    }
}
