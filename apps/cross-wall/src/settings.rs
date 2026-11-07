//! Per-computer settings, kept outside the wallpaper folder. Every change is
//! saved at once, by a writer thread, so the frame thread never waits on disk.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

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
        Self {
            theme: ThemeMode::Dark,
            wallpaper_folder: String::new(),
            library_groups: false,
            ui_zoom: 1.0,
            min_resolution: None,
            logging: None,
        }
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

/// The settings in use, saved in the background after every change.
pub struct Store {
    settings: Settings,
    saves: Option<Sender<Settings>>,
    errors: Receiver<String>,
    writer: Option<JoinHandle<()>>,
}

impl Store {
    pub fn open(path: PathBuf) -> Self {
        let settings = Settings::load(&path);
        let (saves, wanted) = channel::<Settings>();
        let (error_tx, errors) = channel();
        let writer = std::thread::spawn(move || {
            while let Ok(mut latest) = wanted.recv() {
                // Only the newest of a burst, such as a dragged slider, is worth writing.
                while let Ok(newer) = wanted.try_recv() {
                    latest = newer;
                }
                if let Err(error) = latest.save(&path) {
                    let _ = error_tx.send(format!("Could not save settings: {error}"));
                }
            }
        });
        Self {
            settings,
            saves: Some(saves),
            errors,
            writer: Some(writer),
        }
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// Applies a change and saves it if anything changed. Says whether it did.
    pub fn change(&mut self, edit: impl FnOnce(&mut Settings)) -> bool {
        let before = self.settings.clone();
        edit(&mut self.settings);
        let changed = self.settings != before;
        if changed {
            if let Some(saves) = &self.saves {
                let _ = saves.send(self.settings.clone());
            }
        }
        changed
    }

    /// A save that failed since the last call.
    pub fn poll_error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}

impl Drop for Store {
    /// Waits for the last save, so a change made just before closing is kept.
    fn drop(&mut self) {
        drop(self.saves.take());
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

pub fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cross-wall")
        .join("settings.json")
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cross-wall")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_from_before_the_wallpaper_folder_load_and_start_dark() {
        assert_eq!(Settings::default().theme, ThemeMode::Dark);
        let old: Settings = serde_json::from_str(
            r#"{"pexels_key":"gone","sync_folder":"old","wallpaper_folder":"f"}"#,
        )
        .unwrap();
        assert_eq!(
            (old.theme, old.wallpaper_folder.as_str()),
            (ThemeMode::Dark, "f")
        );
    }

    #[test]
    fn round_trips_and_survives_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let settings = Settings {
            theme: ThemeMode::Light,
            wallpaper_folder: "f".into(),
            library_groups: true,
            ui_zoom: 1.5,
            min_resolution: Some([3840, 2160]),
            logging: Some(true),
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::write(&path, "{broken").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn the_store_saves_every_change_and_only_real_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut store = Store::open(path.clone());
        assert!(!store.change(|s| s.theme = ThemeMode::Dark));
        assert!(store.change(|s| s.ui_zoom = 1.25));
        assert!(store.change(|s| s.ui_zoom = 1.5));
        drop(store);
        assert_eq!(Settings::load(&path).ui_zoom, 1.5);
    }
}
