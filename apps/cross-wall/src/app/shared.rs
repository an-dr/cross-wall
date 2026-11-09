//! What every screen works with: the network client, images, the wallpaper
//! folder as last scanned, the settings, and the message shown to the user.
//! Each screen keeps its own state in its own module.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use cross_wall_library::{Entry, Snapshot};
use cross_wall_sources::{Client, Progress};

use super::apply_worker::{Applier, Image};
use super::organise::Organiser;
use super::shown::Shown;
use crate::images::Images;
use crate::librarian::{Command, Librarian};
use crate::settings::{Settings, Store};

pub fn chosen_folder(settings: &Settings) -> Option<PathBuf> {
    let folder = settings.wallpaper_folder.trim();
    (!folder.is_empty()).then(|| PathBuf::from(folder))
}

pub struct Shared {
    pub client: Client,
    pub cache: PathBuf,
    pub images: Images,
    pub store: Store,
    /// The folder as last scanned. The librarian thread owns all writes.
    pub snapshot: Snapshot,
    librarian: Librarian,
    /// Downloads and like changes under way, by `Shown::key`.
    pub pending: HashMap<String, Arc<Progress>>,
    /// The items the current screen shows, in order, which the preview steps through.
    pub shown: Vec<Shown>,
    pub apply: Applier,
    pub organiser: Organiser,
    pub picker: FolderPicker,
    /// Items whose tile has been on screen, so a forgotten thumbnail reloads.
    pub seen: HashSet<String>,
    /// The latest message for the user. Empty when there is none.
    pub status: String,
}

impl Shared {
    pub fn new(store: Store, data_dir: PathBuf) -> Self {
        let cache = data_dir.join("cache");
        let cache_to_prune = cache.clone();
        std::thread::spawn(move || crate::images::prune_cache(&cache_to_prune));
        Self {
            client: Client::new(),
            cache,
            images: Images::new(),
            snapshot: Snapshot::default(),
            librarian: Librarian::start(chosen_folder(store.get())),
            store,
            pending: HashMap::new(),
            shown: Vec::new(),
            apply: Applier::new(data_dir.join("wallpaper")),
            organiser: Organiser::default(),
            picker: FolderPicker::new(),
            seen: HashSet::new(),
            status: String::new(),
        }
    }

    pub fn settings(&self) -> &Settings {
        self.store.get()
    }

    pub fn command(&self, command: Command) {
        let _ = self.librarian.commands.send(command);
    }

    /// Switches to another wallpaper folder. Returns whether it changed.
    pub fn set_folder(&mut self, folder: String) -> bool {
        let changed = self.store.change(|s| s.wallpaper_folder = folder);
        if changed {
            self.command(Command::SetRoot(chosen_folder(self.store.get())));
        }
        changed
    }

    /// The kept file behind an item, whether it came from the library or Browse.
    pub fn entry_for(&self, item: &Shown) -> Option<&Entry> {
        match item {
            Shown::Remote(photo) => self.snapshot.find_photo(&photo.id),
            Shown::Local(entry) => self.snapshot.find_path(&entry.path),
        }
    }

    /// The item's file on disk, when the folder holds it.
    pub fn local_file(&self, item: &Shown) -> Option<PathBuf> {
        let entry = self.entry_for(item)?;
        Some(chosen_folder(self.settings())?.join(&entry.path))
    }

    pub fn start_apply(&mut self, item: &Shown) {
        let image = match (self.local_file(item), item) {
            (Some(file), _) => Image::File(file),
            (None, Shown::Remote(photo)) => Image::Download(photo.src.original.clone()),
            (None, Shown::Local(entry)) => {
                self.status = format!("{} is no longer in the wallpaper folder.", entry.name());
                return;
            }
        };
        self.status = "Applying wallpaper...".into();
        self.apply.start(&self.client, image);
    }

    /// Takes what the worker threads finished since the last frame.
    pub fn poll(&mut self) {
        if let Some(message) = self.apply.poll() {
            self.status = message;
        }
        while let Ok(update) = self.librarian.updates.try_recv() {
            self.snapshot = update.snapshot;
            if let Some(key) = update.finished {
                self.pending.remove(&key);
            }
            if let Some(problem) = update.problem {
                self.status = problem;
            }
        }
        if let Some(folder) = self.picker.poll() {
            self.set_folder(folder.display().to_string());
        }
        if let Some(error) = self.store.poll_error() {
            self.status = error;
        }
    }
}

/// The system's folder dialog blocks, so it runs on its own thread.
pub struct FolderPicker {
    tx: Sender<Option<PathBuf>>,
    rx: Receiver<Option<PathBuf>>,
}

impl FolderPicker {
    fn new() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx }
    }

    pub fn open(&self) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(
                rfd::FileDialog::new()
                    .set_title("Choose your wallpaper folder")
                    .pick_folder(),
            );
        });
    }

    fn poll(&self) -> Option<PathBuf> {
        self.rx.try_recv().ok().flatten()
    }
}
