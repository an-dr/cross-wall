//! Loads and decodes images on worker threads and hands egui textures to the frame thread.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const WORKERS: usize = 6;
const MAX_TEXTURE: u32 = 1600;
const CACHE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

type Bytes = Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>;
type Decoded = Result<egui::ColorImage, String>;

pub enum Slot {
    Loading,
    Ready(egui::TextureHandle),
    Failed(String),
}

pub struct Images {
    slots: HashMap<String, Slot>,
    jobs: Sender<(String, Bytes)>,
    done: Receiver<(String, Decoded)>,
}

impl Images {
    pub fn new() -> Self {
        let (jobs, job_queue) = channel::<(String, Bytes)>();
        let (done_tx, done) = channel();
        let job_queue = Arc::new(Mutex::new(job_queue));
        for _ in 0..WORKERS {
            let queue = job_queue.clone();
            let done_tx = done_tx.clone();
            std::thread::spawn(move || loop {
                let job = queue.lock().expect("no worker panics holding this lock").recv();
                let Ok((key, bytes)) = job else { return };
                if done_tx.send((key, bytes().and_then(|b| decode(&b)))).is_err() {
                    return;
                }
            });
        }
        Self { slots: HashMap::new(), jobs, done }
    }

    /// The image under `key`, starting `bytes` on a worker the first time it is asked for.
    pub fn get(&mut self, key: &str, bytes: impl FnOnce() -> Bytes) -> &Slot {
        if !self.slots.contains_key(key) {
            let _ = self.jobs.send((key.to_string(), bytes()));
            self.slots.insert(key.to_string(), Slot::Loading);
        }
        &self.slots[key]
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, decoded)) = self.done.try_recv() {
            let slot = match decoded {
                Ok(image) => Slot::Ready(ctx.load_texture(&key, image, egui::TextureOptions::LINEAR)),
                Err(error) => Slot::Failed(error),
            };
            self.slots.insert(key, slot);
        }
    }

    /// Frees textures whose keys start with `prefix`, such as a replaced result list.
    pub fn forget(&mut self, prefix: &str) {
        self.slots.retain(|key, _| !key.starts_with(prefix));
    }
}

fn decode(bytes: &[u8]) -> Decoded {
    // Originals can be 6000 pixels wide, far more than the screen shows and
    // too much memory to keep as textures for a whole library.
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let image = if image.width().max(image.height()) > MAX_TEXTURE { image.thumbnail(MAX_TEXTURE, MAX_TEXTURE) } else { image };
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
    fn decode_refuses_garbage() {
        assert!(decode(b"nope").is_err());
    }
}
