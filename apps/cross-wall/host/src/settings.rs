//! Per-computer settings. Never placed in the sync folder: the API key stays local.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    /// The folder the user chose. Downloads go to its root.
    pub wallpaper_folder: String,
    /// Shows the library as sections per folder instead of one folder at a time.
    pub library_groups: bool,
    /// Multiplies the display scale, which the app follows by itself.
    pub ui_zoom: f32,
    /// `None` follows the main display. `[0, 0]` is any size.
    pub min_resolution: Option<[u32; 2]>,
    /// `None` takes the build's default: off in release, on in development.
    pub logging: Option<bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: ThemeMode::Dark, wallpaper_folder: String::new(), library_groups: false, ui_zoom: 1.0, min_resolution: None, logging: None }
    }
}

impl Settings {
    /// A missing or unreadable file gives the defaults: the app must still open to fix it.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("settings path has no folder")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("tmp");
        let text = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())
    }
}

pub fn settings_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("cross-wall").join("settings.json")
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("cross-wall")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_starts_dark_even_for_older_settings_files() {
        assert_eq!(Settings::default().theme, ThemeMode::Dark);
        let old: Settings = serde_json::from_str(r#"{"pexels_key":"gone","sync_folder":"old","wallpaper_folder":"f"}"#).unwrap();
        assert_eq!((old.theme, old.wallpaper_folder.as_str()), (ThemeMode::Dark, "f"));
    }

    #[test]
    fn round_trips_and_survives_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let settings = Settings { theme: ThemeMode::Light, wallpaper_folder: "f".into(), library_groups: true, ui_zoom: 1.5, min_resolution: Some([3840, 2160]), logging: Some(true) };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::write(&path, "{broken").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
