//! Loads and decodes images on worker threads and uploads them to bones' ui
//! module from the frame thread, where widgets show them by handle.
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use bones_kernel::ui::UiPort;

const MIN_WORKERS: usize = 4;
const MAX_WORKERS: usize = 8;
/// Originals can be 6000 pixels wide, too much memory to keep for a whole library.
const MAX_THUMBNAIL: u32 = 640;
const MAX_PREVIEW: u32 = 2560;
const CACHE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

pub type Bytes = Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>;
/// Width, height, and straight RGBA8 pixels.
type Decoded = Result<(u32, u32, Vec<u8>), String>;

/// Newest first: what is on screen now was asked for last, and should not wait
/// behind photos already scrolled past.
#[derive(Default)]
struct JobStack {
    jobs: Mutex<Vec<(String, Bytes)>>,
    ready: Condvar,
}

impl JobStack {
    fn push(&self, job: (String, Bytes)) {
        self.jobs
            .lock()
            .expect("no worker panics holding this lock")
            .push(job);
        self.ready.notify_one();
    }

    fn pop(&self) -> (String, Bytes) {
        let mut jobs = self
            .jobs
            .lock()
            .expect("no worker panics holding this lock");
        loop {
            if let Some(job) = jobs.pop() {
                return job;
            }
            jobs = self
                .ready
                .wait(jobs)
                .expect("no worker panics holding this lock");
        }
    }
}

pub enum Slot {
    Loading,
    /// Uploaded under `handle`.
    Ready {
        handle: u32,
    },
    Failed(String),
}

pub struct Images {
    slots: HashMap<String, Slot>,
    jobs: Arc<JobStack>,
    done: Receiver<(String, Decoded)>,
    done_tx: Sender<(String, Decoded)>,
    /// Images asked for since the queue was last empty, for the progress bar.
    batch: usize,
    next_handle: u32,
    /// Uploaded images forgotten since the last poll, to free in the ui module.
    to_free: Vec<u32>,
}

impl Images {
    pub fn new() -> Self {
        let jobs = Arc::new(JobStack::default());
        let (done_tx, done) = channel();
        let workers = std::thread::available_parallelism()
            .map_or(MIN_WORKERS, |n| n.get().clamp(MIN_WORKERS, MAX_WORKERS));
        for _ in 0..workers {
            let queue = jobs.clone();
            let done_tx = done_tx.clone();
            std::thread::spawn(move || loop {
                let (key, bytes) = queue.pop();
                if done_tx.send(load(key, bytes)).is_err() {
                    return;
                }
            });
        }
        Self {
            slots: HashMap::new(),
            jobs,
            done,
            done_tx,
            batch: 0,
            next_handle: 0,
            to_free: Vec::new(),
        }
    }

    /// The image under `key`, starting `bytes` on a worker the first time it is asked for.
    pub fn get(&mut self, key: &str, bytes: impl FnOnce() -> Bytes) -> &Slot {
        if !self.slots.contains_key(key) {
            // A preview the user is waiting for must not queue behind a page of thumbnails.
            if key.starts_with("large/") {
                let (done, name, work) = (self.done_tx.clone(), key.to_string(), bytes());
                std::thread::spawn(move || {
                    let _ = done.send(load(name, work));
                });
            } else {
                self.jobs.push((key.to_string(), bytes()));
            }
            self.slots.insert(key.to_string(), Slot::Loading);
            self.batch += 1;
        }
        &self.slots[key]
    }

    pub fn is_known(&self, key: &str) -> bool {
        self.slots.contains_key(key)
    }

    /// Starts loading in the background queue, for images not needed yet.
    pub fn preload(&mut self, key: &str, bytes: impl FnOnce() -> Bytes) {
        if !self.slots.contains_key(key) {
            self.jobs.push((key.to_string(), bytes()));
            self.slots.insert(key.to_string(), Slot::Loading);
        }
    }

    /// The handle of the image under `key`, once it is uploaded.
    pub fn handle(&self, key: &str) -> Option<u32> {
        match self.slots.get(key) {
            Some(Slot::Ready { handle, .. }) => Some(*handle),
            _ => None,
        }
    }

    /// Frees full-size images other than those named.
    pub fn retain_large(&mut self, keep: &std::collections::HashSet<String>) {
        self.retain(|key| !key.starts_with("large/") || keep.contains(key));
    }

    fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        let to_free = &mut self.to_free;
        self.slots.retain(|key, slot| {
            let kept = keep(key);
            if let (false, Slot::Ready { handle, .. }) = (kept, slot) {
                to_free.push(*handle);
            }
            kept
        });
    }

    /// Uploads what the workers finished and frees what was forgotten, as
    /// `owner`'s images.
    pub fn poll(&mut self, port: &mut dyn UiPort, owner: &str) {
        for handle in self.to_free.drain(..) {
            port.free_image(owner, handle);
        }
        while let Ok((key, decoded)) = self.done.try_recv() {
            // Forgotten while it loaded: keeping it would hold an image nobody can free.
            if !self.slots.contains_key(&key) {
                continue;
            }
            let slot = match decoded {
                Ok((width, height, rgba)) => {
                    self.next_handle += 1;
                    let handle = self.next_handle;
                    port.set_image(owner, handle, width, height, rgba);
                    Slot::Ready { handle }
                }
                Err(error) => Slot::Failed(error),
            };
            self.slots.insert(key, slot);
        }
    }

    /// How many of the current batch have finished, and its size. `None` when idle.
    pub fn progress(&mut self) -> Option<(usize, usize)> {
        let waiting = self
            .slots
            .values()
            .filter(|slot| matches!(slot, Slot::Loading))
            .count();
        if waiting == 0 {
            self.batch = 0;
            return None;
        }
        self.batch = self.batch.max(waiting);
        Some((self.batch - waiting, self.batch))
    }

    /// Frees images whose keys start with `prefix`, such as a replaced result list.
    pub fn forget(&mut self, prefix: &str) {
        self.retain(|key| !key.starts_with(prefix));
    }
}

fn load(key: String, bytes: Bytes) -> (String, Decoded) {
    let limit = if key.starts_with("large/") {
        MAX_PREVIEW
    } else {
        MAX_THUMBNAIL
    };
    let decoded = bytes().and_then(|b| decode(&b, limit));
    (key, decoded)
}

fn decode(bytes: &[u8], limit: u32) -> Decoded {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let image = if image.width().max(image.height()) > limit {
        image.thumbnail(limit, limit)
    } else {
        image
    };
    let image = image.to_rgba8();
    Ok((image.width(), image.height(), image.into_raw()))
}

/// Reads `path`, or fetches and stores it. A failed cache write only costs a later refetch.
pub fn cached(
    path: PathBuf,
    fetch: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = std::fs::read(&path) {
        return Ok(bytes);
    }
    let bytes = fetch()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temporary = path.with_extension("tmp");
    if std::fs::write(&temporary, &bytes).is_ok() {
        let _ = std::fs::rename(&temporary, &path);
    }
    Ok(bytes)
}

/// A small JPEG of a local image, made once and kept in `cache`. Keyed by path,
/// size, and modification time, so an edited image gets a new one.
pub fn local_thumbnail(file: &Path, cache: &Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata.modified().ok().hash(&mut hasher);
    let path = cache.join(format!("local-{:016x}.jpg", hasher.finish()));
    cached(path, || {
        let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
        let image = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
        let small = image.thumbnail(MAX_THUMBNAIL, MAX_THUMBNAIL).to_rgb8();
        let mut jpeg = std::io::Cursor::new(Vec::new());
        small
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .map_err(|e| e.to_string())?;
        Ok(jpeg.into_inner())
    })
}

/// Deletes cache files not written for a month, then the oldest beyond a total
/// size, so browsing cannot fill the disk. Previews can be whole originals.
pub fn prune_cache(dir: &Path) {
    prune_cache_to(dir, MAX_CACHE_BYTES);
}

fn prune_cache_to(dir: &Path, max_bytes: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut kept = Vec::new();
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let old = SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age > CACHE_LIFETIME);
        if old {
            let _ = std::fs::remove_file(entry.path());
        } else if metadata.is_file() {
            kept.push((modified, metadata.len(), entry.path()));
        }
    }
    kept.sort_by_key(|k| std::cmp::Reverse(k.0));
    let mut total = 0;
    for (_, size, path) in kept {
        total += size;
        if total > max_bytes {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_keeps_the_newest_files_within_its_size() {
        let dir = tempfile::tempdir().unwrap();
        for (name, age) in [("old", 30), ("mid", 20), ("new", 10)] {
            let path = dir.path().join(name);
            std::fs::write(&path, [0; 10]).unwrap();
            let at = SystemTime::now() - Duration::from_secs(age);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(at)
                .unwrap();
        }
        prune_cache_to(dir.path(), 25);
        assert!(dir.path().join("new").exists() && dir.path().join("mid").exists());
        assert!(!dir.path().join("old").exists());
    }

    #[test]
    fn cached_fetches_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x/1.jpg");
        assert_eq!(cached(path.clone(), || Ok(vec![1])).unwrap(), vec![1]);
        assert_eq!(
            cached(path, || Err("must not fetch".into())).unwrap(),
            vec![1]
        );
    }

    #[test]
    fn big_images_are_reduced_to_the_limit() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(100, 50)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let (width, height, rgba) = decode(png.get_ref(), 20).unwrap();
        assert_eq!((width, height, rgba.len()), (20, 10, 20 * 10 * 4));
    }

    /// Records uploads instead of showing them.
    #[derive(Default)]
    struct Uploads {
        set: Vec<u32>,
        freed: Vec<u32>,
    }

    impl UiPort for Uploads {
        fn show(&mut self, _: &str, _: bones_kernel::messages::ui::Spec) {}
        fn take_events(&mut self, _: &str) -> Vec<bones_kernel::messages::ui::Event> {
            Vec::new()
        }
        fn set_zoom(&mut self, _: f32) {}
        fn set_image(&mut self, _: &str, handle: u32, _: u32, _: u32, _: Vec<u8>) {
            self.set.push(handle);
        }
        fn free_image(&mut self, _: &str, handle: u32) {
            self.freed.push(handle);
        }
        fn set_theme(&mut self, _: bones_kernel::ui::Theme) {}
    }

    #[test]
    fn an_image_forgotten_while_loading_is_dropped() {
        let mut uploads = Uploads::default();
        let mut images = Images::new();
        images.get("large/a", || Box::new(|| Err("x".into())));
        images.retain_large(&std::collections::HashSet::new());
        std::thread::sleep(Duration::from_millis(100));
        images.poll(&mut uploads, "app");
        assert!(images.slots.is_empty());
        assert!(uploads.set.is_empty());
    }

    #[test]
    fn a_loaded_image_is_uploaded_and_freed_when_forgotten() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(4, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        let mut uploads = Uploads::default();
        let mut images = Images::new();
        images.get("thumb/a", move || Box::new(move || Ok(bytes)));
        for _ in 0..50 {
            images.poll(&mut uploads, "app");
            if images.handle("thumb/a").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let handle = images.handle("thumb/a").expect("the image never loaded");
        assert_eq!(uploads.set, [handle]);
        images.forget("thumb/");
        images.poll(&mut uploads, "app");
        assert_eq!(uploads.freed, [handle]);
    }

    #[test]
    fn newest_jobs_run_first() {
        let stack = JobStack::default();
        for name in ["old", "new"] {
            stack.push((name.to_string(), Box::new(|| Ok(Vec::new()))));
        }
        assert_eq!(stack.pop().0, "new");
        assert_eq!(stack.pop().0, "old");
    }

    #[test]
    fn local_thumbnails_are_small_cached_and_follow_edits() {
        let dir = tempfile::tempdir().unwrap();
        let (file, cache) = (dir.path().join("big.png"), dir.path().join("cache"));
        image::RgbImage::new(2000, 1000).save(&file).unwrap();
        let thumb = image::load_from_memory(&local_thumbnail(&file, &cache).unwrap()).unwrap();
        assert_eq!(
            (thumb.width(), thumb.height()),
            (MAX_THUMBNAIL, MAX_THUMBNAIL / 2)
        );
        local_thumbnail(&file, &cache).unwrap();
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
        image::RgbImage::new(1000, 1000).save(&file).unwrap();
        let edited = image::load_from_memory(&local_thumbnail(&file, &cache).unwrap()).unwrap();
        assert_eq!(edited.height(), MAX_THUMBNAIL);
    }

    /// Timing on a 6000 x 4000 photo: `cargo test -p cross-wall measure -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn measure_thumbnail_speed() {
        let dir = tempfile::tempdir().unwrap();
        let (file, cache) = (dir.path().join("big.jpg"), dir.path().join("cache"));
        let photo = image::RgbImage::from_fn(6000, 4000, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8])
        });
        photo.save(&file).unwrap();
        for label in ["first", "cached"] {
            let start = std::time::Instant::now();
            let bytes = local_thumbnail(&file, &cache).unwrap();
            decode(&bytes, MAX_THUMBNAIL).unwrap();
            println!("{label}: {:?}", start.elapsed());
        }
    }

    #[test]
    fn decode_refuses_garbage() {
        assert!(decode(b"nope", 100).is_err());
    }
}
