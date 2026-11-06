//! Loads and decodes images on worker threads and hands egui textures to the frame thread.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

const MIN_WORKERS: usize = 4;
const MAX_WORKERS: usize = 8;
/// Originals can be 6000 pixels wide, too much memory to keep for a whole library.
const MAX_THUMBNAIL: u32 = 640;
const MAX_PREVIEW: u32 = 2560;
const CACHE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

pub type Bytes = Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>;
type Decoded = Result<egui::ColorImage, String>;

/// Newest first: what is on screen now was asked for last, and should not wait
/// behind photos already scrolled past.
#[derive(Default)]
struct JobStack {
    jobs: Mutex<Vec<(String, Bytes)>>,
    ready: Condvar,
}

impl JobStack {
    fn push(&self, job: (String, Bytes)) {
        self.jobs.lock().expect("no worker panics holding this lock").push(job);
        self.ready.notify_one();
    }

    fn pop(&self) -> (String, Bytes) {
        let mut jobs = self.jobs.lock().expect("no worker panics holding this lock");
        loop {
            if let Some(job) = jobs.pop() {
                return job;
            }
            jobs = self.ready.wait(jobs).expect("no worker panics holding this lock");
        }
    }
}

pub enum Slot {
    Loading,
    Ready(egui::TextureHandle),
    Failed(String),
}

pub struct Images {
    slots: HashMap<String, Slot>,
    jobs: Arc<JobStack>,
    done: Receiver<(String, Decoded)>,
    done_tx: Sender<(String, Decoded)>,
    /// Images asked for since the queue was last empty, for the progress bar.
    batch: usize,
}

impl Images {
    pub fn new() -> Self {
        let jobs = Arc::new(JobStack::default());
        let (done_tx, done) = channel();
        let workers = std::thread::available_parallelism().map_or(MIN_WORKERS, |n| n.get().clamp(MIN_WORKERS, MAX_WORKERS));
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
        Self { slots: HashMap::new(), jobs, done, done_tx, batch: 0 }
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

    /// Frees full-size images other than those named.
    pub fn retain_large(&mut self, keep: &std::collections::HashSet<String>) {
        self.slots.retain(|key, _| !key.starts_with("large/") || keep.contains(key));
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, decoded)) = self.done.try_recv() {
            // Forgotten while it loaded: keeping it would hold a texture nobody can free.
            if !self.slots.contains_key(&key) {
                continue;
            }
            let slot = match decoded {
                Ok(image) => Slot::Ready(ctx.load_texture(&key, image, egui::TextureOptions::LINEAR)),
                Err(error) => Slot::Failed(error),
            };
            self.slots.insert(key, slot);
        }
    }

    /// How many of the current batch have finished, and its size. `None` when idle.
    pub fn progress(&mut self) -> Option<(usize, usize)> {
        let waiting = self.slots.values().filter(|slot| matches!(slot, Slot::Loading)).count();
        if waiting == 0 {
            self.batch = 0;
            return None;
        }
        self.batch = self.batch.max(waiting);
        Some((self.batch - waiting, self.batch))
    }

    /// Frees textures whose keys start with `prefix`, such as a replaced result list.
    pub fn forget(&mut self, prefix: &str) {
        self.slots.retain(|key, _| !key.starts_with(prefix));
    }
}

fn load(key: String, bytes: Bytes) -> (String, Decoded) {
    let limit = if key.starts_with("large/") { MAX_PREVIEW } else { MAX_THUMBNAIL };
    let decoded = bytes().and_then(|b| decode(&b, limit));
    (key, decoded)
}

fn decode(bytes: &[u8], limit: u32) -> Decoded {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let image = if image.width().max(image.height()) > limit { image.thumbnail(limit, limit) } else { image };
    let image = image.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}

/// Reads `path`, or fetches and stores it. A failed cache write only costs a later refetch.
pub fn cached(path: PathBuf, fetch: impl FnOnce() -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
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
        small.write_to(&mut jpeg, image::ImageFormat::Jpeg).map_err(|e| e.to_string())?;
        Ok(jpeg.into_inner())
    })
}

/// Deletes cache files not written for a month, so browsing cannot fill the disk.
pub fn prune_cache(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|age| age > CACHE_LIFETIME);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The part of a texture filling a box of `target` shape without distorting it.
pub fn cover_uv(image: [usize; 2], target: egui::Vec2) -> egui::Rect {
    let image_ratio = image[0] as f32 / image[1].max(1) as f32;
    let target_ratio = target.x / target.y;
    let (w, h) = if image_ratio > target_ratio {
        (target_ratio / image_ratio, 1.0)
    } else {
        (1.0, image_ratio / target_ratio)
    };
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_fetches_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x/1.jpg");
        assert_eq!(cached(path.clone(), || Ok(vec![1])).unwrap(), vec![1]);
        assert_eq!(cached(path, || Err("must not fetch".into())).unwrap(), vec![1]);
    }

    #[test]
    fn cover_crops_the_long_side() {
        let wide = cover_uv([200, 100], egui::vec2(100.0, 100.0));
        assert!((wide.width() - 0.5).abs() < 1e-6 && wide.height() == 1.0);
        let tall = cover_uv([100, 200], egui::vec2(100.0, 100.0));
        assert!((tall.height() - 0.5).abs() < 1e-6 && tall.width() == 1.0);
    }

    #[test]
    fn big_images_are_reduced_to_the_limit() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(100, 50).write_to(&mut png, image::ImageFormat::Png).unwrap();
        assert_eq!(decode(png.get_ref(), 20).unwrap().size, [20, 10]);
    }

    #[test]
    fn an_image_forgotten_while_loading_is_dropped() {
        let ctx = egui::Context::default();
        let mut images = Images::new();
        images.get("large/a", || Box::new(|| Err("x".into())));
        images.retain_large(&std::collections::HashSet::new());
        std::thread::sleep(Duration::from_millis(100));
        images.poll(&ctx);
        assert!(images.slots.is_empty());
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
        assert_eq!((thumb.width(), thumb.height()), (MAX_THUMBNAIL, MAX_THUMBNAIL / 2));
        local_thumbnail(&file, &cache).unwrap();
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
        image::RgbImage::new(1000, 1000).save(&file).unwrap();
        let edited = image::load_from_memory(&local_thumbnail(&file, &cache).unwrap()).unwrap();
        assert_eq!(edited.height(), MAX_THUMBNAIL);
    }

    /// Timing on a 6000 x 4000 photo: `cargo test -p cross-wall-app measure -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn measure_thumbnail_speed() {
        let dir = tempfile::tempdir().unwrap();
        let (file, cache) = (dir.path().join("big.jpg"), dir.path().join("cache"));
        let photo = image::RgbImage::from_fn(6000, 4000, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8]));
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
