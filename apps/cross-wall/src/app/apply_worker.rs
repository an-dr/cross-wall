//! Sets the desktop wallpaper on a thread of its own and reports back.
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use cross_wall_library::extension_for;
use cross_wall_sources::{Client, Progress};

use super::widgets::progress_text;
use crate::clock::now;

/// Where the image to apply comes from.
pub enum Image {
    File(PathBuf),
    Download(String),
}

pub struct Applier {
    busy: bool,
    progress: Arc<Progress>,
    /// The app's own copy of the wallpaper, which the desktop reads.
    folder: PathBuf,
    done_tx: Sender<Result<(), String>>,
    done_rx: Receiver<Result<(), String>>,
}

impl Applier {
    pub fn new(folder: PathBuf) -> Self {
        let (done_tx, done_rx) = channel();
        Self {
            busy: false,
            progress: Arc::default(),
            folder,
            done_tx,
            done_rx,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn progress_text(&self) -> String {
        progress_text(self.progress.read(), "Applying wallpaper")
    }

    /// Copies the image to its own folder first: the desktop keeps reading the
    /// file, so moving or deleting the original must not remove the wallpaper.
    pub fn start(&mut self, client: &Client, image: Image) {
        self.busy = true;
        self.progress = Arc::default();
        let (tx, client, folder, progress) = (
            self.done_tx.clone(),
            client.clone(),
            self.folder.clone(),
            self.progress.clone(),
        );
        std::thread::spawn(move || {
            let result = (|| {
                let bytes = match image {
                    Image::File(file) => {
                        std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?
                    }
                    Image::Download(url) => client
                        .download_tracked(&url, &progress)
                        .map_err(|e| e.to_string())?,
                };
                // A new name each time: desktops can ignore a change to the same path.
                let target = folder.join(format!("wall-{}.{}", now(), extension_for(&bytes)));
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
                cross_wall_apply::apply(&target)?;
                for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                    if entry.path() != target {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
                Ok(())
            })();
            let _ = tx.send(result);
        });
    }

    /// The message for a finished attempt, if one finished since the last call.
    pub fn poll(&mut self) -> Option<String> {
        let result = self.done_rx.try_recv().ok()?;
        self.busy = false;
        Some(match result {
            Ok(()) => "Wallpaper applied.".into(),
            Err(error) => error,
        })
    }
}
