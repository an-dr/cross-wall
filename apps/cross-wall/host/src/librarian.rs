//! Owns the wallpaper folder on one thread, so changes never overlap and never
//! touch the frame thread. Rescans after every change and every 5 seconds, which
//! is how changes made in a file manager appear.
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cross_wall_library::{Snapshot, WallpaperFolder};
use cross_wall_sources::{Client, Photo, Progress};

const RESCAN_EVERY: Duration = Duration::from_secs(5);

pub enum Command {
    SetRoot(Option<PathBuf>),
    /// Keeps a source photo in the folder root, downloading it unless already kept.
    Add { client: Client, photo: Photo, liked: bool, progress: Arc<Progress> },
    SetLiked { path: String, liked: bool, key: String },
    CreateFolder { parent: String, name: String },
    Rename { path: String, name: String },
    Move { path: String, folder: String },
    Delete { path: String },
}

pub struct Update {
    pub snapshot: Snapshot,
    /// The key of a finished action, so its busy state can end.
    pub finished: Option<String>,
    pub problem: Option<String>,
}

pub struct Librarian {
    pub commands: Sender<Command>,
    pub updates: Receiver<Update>,
}

impl Librarian {
    pub fn start(root: Option<PathBuf>) -> Self {
        let (commands, command_rx) = channel();
        let (update_tx, updates) = channel();
        std::thread::spawn(move || run(root, command_rx, update_tx));
        Self { commands, updates }
    }
}

fn open(root: Option<PathBuf>) -> (Option<WallpaperFolder>, Option<PathBuf>, Option<String>) {
    match root.clone().map(WallpaperFolder::open) {
        Some(Ok(folder)) => (Some(folder), root, None),
        Some(Err(problem)) => (None, root, Some(problem)),
        None => (None, None, None),
    }
}

fn run(root: Option<PathBuf>, commands: Receiver<Command>, updates: Sender<Update>) {
    let (mut folder, mut wanted, mut problem) = open(root);
    let mut last_sent: Option<Snapshot> = None;
    let mut last_problem = None;
    let mut finished = None;
    loop {
        // A folder that was unavailable may have come back, such as a remounted drive.
        if folder.is_none() && wanted.is_some() {
            let reopened = open(wanted.clone());
            folder = reopened.0;
        }
        let snapshot = match folder.as_mut().map(WallpaperFolder::snapshot) {
            Some(Ok(snapshot)) => snapshot,
            Some(Err(error)) => {
                problem = problem.or(Some(error));
                folder = None;
                Snapshot::default()
            }
            None => Snapshot::default(),
        };
        // A problem is told once, not every 5 seconds.
        let new_problem = problem.take().filter(|p| last_problem.as_ref() != Some(p));
        if new_problem.is_some() {
            last_problem = new_problem.clone();
        } else if folder.is_some() {
            last_problem = None;
        }
        if last_sent.as_ref() != Some(&snapshot) || finished.is_some() || new_problem.is_some() {
            last_sent = Some(snapshot.clone());
            if updates.send(Update { snapshot, finished: finished.take(), problem: new_problem }).is_err() {
                return;
            }
        }
        match commands.recv_timeout(RESCAN_EVERY) {
            Ok(Command::SetRoot(root)) => {
                last_problem = None;
                (folder, wanted, problem) = open(root);
            }
            Ok(command) => {
                let (key, result) = perform(folder.as_mut(), command);
                finished = key;
                problem = result.err();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn perform(folder: Option<&mut WallpaperFolder>, command: Command) -> (Option<String>, Result<(), String>) {
    let key = match &command {
        Command::Add { photo, .. } => Some(photo.id.clone()),
        Command::SetLiked { key, .. } => Some(key.clone()),
        _ => None,
    };
    let Some(folder) = folder else {
        return (key, Err("Choose an available wallpaper folder in Settings first.".into()));
    };
    let result = match command {
        Command::SetRoot(_) => Ok(()),
        Command::Add { client, photo, liked, progress } => add(folder, &client, &photo, liked, &progress),
        Command::SetLiked { path, liked, .. } => folder.set_liked(&path, liked),
        Command::CreateFolder { parent, name } => folder.create_folder(&parent, &name).map(drop),
        Command::Rename { path, name } => folder.rename(&path, &name).map(drop),
        Command::Move { path, folder: destination } => folder.move_to(&path, &destination).map(drop),
        Command::Delete { path } => folder.delete(&path),
    };
    (key, result)
}

fn add(folder: &mut WallpaperFolder, client: &Client, photo: &Photo, liked: bool, progress: &Progress) -> Result<(), String> {
    if folder.has_photo(&photo.id) {
        return if liked { folder.like_kept_photo(&photo.id) } else { Ok(()) };
    }
    let image = client.download_tracked(&photo.src.original, progress).map_err(|e| e.to_string())?;
    folder.add_photo(photo, &image, liked, now()).map(drop)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cross_wall_sources::{Sources, Transport};

    struct Answer(u16);
    impl Transport for Answer {
        fn get(&self, _: &str, progress: &Progress) -> Result<(u16, Vec<u8>), String> {
            progress.set_total(5);
            progress.advance(5);
            Ok((self.0, b"image".to_vec()))
        }
    }
    fn photo() -> Photo {
        let s = String::new;
        Photo {
            id: "wallhaven-5".into(),
            width: 1,
            height: 1,
            url: s(),
            photographer: s(),
            photographer_url: s(),
            license: s(),
            src: Sources { original: s(), large2x: s(), medium: s() },
        }
    }
    fn next(l: &Librarian) -> Update {
        l.updates.recv_timeout(Duration::from_secs(5)).unwrap()
    }
    fn add(l: &Librarian, status: u16, progress: Arc<Progress>) {
        let client = Client::with_transport(Arc::new(Answer(status)));
        l.commands.send(Command::Add { client, photo: photo(), liked: true, progress }).unwrap();
    }

    #[test]
    fn a_download_lands_in_the_root_reports_progress_and_can_be_organised() {
        let dir = tempfile::tempdir().unwrap();
        let l = Librarian::start(Some(dir.path().into()));
        assert!(next(&l).snapshot.files.is_empty());
        let progress = Arc::new(Progress::default());
        add(&l, 200, progress.clone());
        let added = next(&l);
        assert_eq!(added.finished.as_deref(), Some("wallhaven-5"));
        assert_eq!(added.snapshot.files[0].path, "wallhaven-5.jpg");
        assert!(added.snapshot.files[0].info.liked);
        assert_eq!(progress.read(), (5, Some(5)));
        l.commands.send(Command::CreateFolder { parent: String::new(), name: "Sea".into() }).unwrap();
        assert_eq!(next(&l).snapshot.folders, ["Sea"]);
        l.commands.send(Command::Move { path: "wallhaven-5.jpg".into(), folder: "Sea".into() }).unwrap();
        assert_eq!(next(&l).snapshot.files[0].path, "Sea/wallhaven-5.jpg");
    }

    #[test]
    fn a_failed_download_saves_nothing_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let l = Librarian::start(Some(dir.path().into()));
        next(&l);
        add(&l, 500, Arc::new(Progress::default()));
        let update = next(&l);
        assert!(update.snapshot.files.is_empty());
        assert!(update.problem.unwrap().contains("500"));
    }

    #[test]
    fn without_a_folder_an_action_explains_what_to_do() {
        let l = Librarian::start(None);
        next(&l);
        add(&l, 200, Arc::new(Progress::default()));
        let update = next(&l);
        assert!(update.problem.unwrap().contains("Settings"));
        assert_eq!(update.finished.as_deref(), Some("wallhaven-5"));
    }

    #[test]
    fn a_missing_folder_is_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let l = Librarian::start(Some(dir.path().join("gone")));
        assert!(next(&l).problem.unwrap().contains("unavailable"));
        assert!(l.updates.recv_timeout(Duration::from_millis(200)).is_err());
    }
}
