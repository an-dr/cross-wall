//! Owns the library on one thread, so likes and syncs never overlap and never
//! touch the frame thread. Syncs after every change and every 30 seconds.
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cross_wall_library::{Library, Like};
use cross_wall_pexels::{Client, Photo};

const SYNC_EVERY: Duration = Duration::from_secs(30);

pub enum Command {
    Like(Client, Photo),
    Unlike(u64),
    SetShared(Option<PathBuf>),
}

pub struct Update {
    pub likes: Vec<Like>,
    /// The photo a finished command was about, so its Saving state can end.
    pub finished: Option<u64>,
    /// `None` clears a previous problem.
    pub problem: Option<String>,
}

pub struct Librarian {
    pub commands: Sender<Command>,
    pub updates: Receiver<Update>,
}

impl Librarian {
    pub fn start(local: PathBuf, shared: Option<PathBuf>) -> Self {
        let (commands, command_rx) = channel();
        let (update_tx, updates) = channel();
        std::thread::spawn(move || run(local, shared, command_rx, update_tx));
        Self { commands, updates }
    }
}

fn run(local: PathBuf, shared: Option<PathBuf>, commands: Receiver<Command>, updates: Sender<Update>) {
    let mut library = Library::new(local.clone(), shared);
    let mut next = Ok(None);
    loop {
        let mut finished = None;
        let mut problem = None;
        match next {
            Ok(Some(Command::Like(client, photo))) => {
                finished = Some(photo.id);
                problem = client
                    .download(&photo.src.original)
                    .map_err(|e| e.to_string())
                    .and_then(|image| library.like(&photo, &image, now()))
                    .err();
            }
            Ok(Some(Command::Unlike(id))) => {
                finished = Some(id);
                problem = library.unlike(id, now()).err();
            }
            Ok(Some(Command::SetShared(shared))) => library = Library::new(local.clone(), shared),
            Ok(None) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let synced = library.sync().err();
        let update = Update { likes: library.list(), finished, problem: problem.or(synced) };
        if updates.send(update).is_err() {
            return;
        }
        next = commands.recv_timeout(SYNC_EVERY).map(Some);
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cross_wall_pexels::{Sources, Transport};
    use std::sync::Arc;

    struct Answer(u16);
    impl Transport for Answer {
        fn get(&self, _: &str, _: Option<&str>) -> Result<(u16, Vec<u8>), String> {
            Ok((self.0, b"image".to_vec()))
        }
    }
    fn photo() -> Photo {
        let s = String::new;
        Photo {
            id: 5,
            width: 1,
            height: 1,
            url: s(),
            photographer: s(),
            photographer_url: s(),
            src: Sources { original: s(), large2x: s(), medium: s() },
        }
    }
    fn next(l: &Librarian) -> Update {
        l.updates.recv_timeout(Duration::from_secs(5)).unwrap()
    }

    #[test]
    fn like_then_unlike_reports_each_state() {
        let dir = tempfile::tempdir().unwrap();
        let l = Librarian::start(dir.path().into(), None);
        assert!(next(&l).likes.is_empty());
        let client = Client::with_transport("k", Arc::new(Answer(200)));
        l.commands.send(Command::Like(client, photo())).unwrap();
        let liked = next(&l);
        assert_eq!((liked.likes.len(), liked.finished), (1, Some(5)));
        l.commands.send(Command::Unlike(5)).unwrap();
        assert!(next(&l).likes.is_empty());
    }

    #[test]
    fn a_failed_download_saves_nothing_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let l = Librarian::start(dir.path().into(), None);
        next(&l);
        let client = Client::with_transport("k", Arc::new(Answer(500)));
        l.commands.send(Command::Like(client, photo())).unwrap();
        let update = next(&l);
        assert!(update.likes.is_empty());
        assert!(update.problem.unwrap().contains("500"));
    }
}
