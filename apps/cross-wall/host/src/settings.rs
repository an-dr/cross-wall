//! Per-computer settings. Never placed in the sync folder: the API key stays local.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub pexels_key: String,
    pub sync_folder: String,
    /// Multiplies the display scale, which the app follows by itself.
    pub ui_zoom: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { pexels_key: String::new(), sync_folder: String::new(), ui_zoom: 1.0 }
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
    fn round_trips_and_survives_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let settings = Settings { pexels_key: "k".into(), sync_folder: "f".into(), ui_zoom: 1.5 };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::write(&path, "{broken").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
