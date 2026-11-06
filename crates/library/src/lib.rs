//! Liked photos in a local folder, merged with a shared folder (docs/adr/ADR-002).
//!
//! Both folders have the same layout: `likes/<id>.json`, `images/<id>.jpg`, and
//! `removed/<id>.json`. For one photo the newest of its like and its removal
//! wins, with a removal winning a tie. Names outside the ID alphabet, such as a
//! sync tool's conflict copies and temporary files, are ignored.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cross_wall_sources::Photo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Like {
    pub photo: Photo,
    /// Milliseconds since the Unix epoch.
    pub liked_at: u64,
    /// The image's size, which tells a fully synced image from a partial one.
    pub image_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Removal {
    pub removed_at: u64,
}

pub struct Library {
    local: PathBuf,
    shared: Option<PathBuf>,
}

impl Library {
    /// `shared` is the folder the user chose; entries go in a `cross-wall` folder inside it.
    pub fn new(local: PathBuf, shared: Option<PathBuf>) -> Self {
        Self { local, shared }
    }

    /// Records a like once its image is complete. The entry is written after
    /// the image, so an interrupted write never appears as liked.
    pub fn like(&self, photo: &Photo, image: &[u8], now: u64) -> Result<(), String> {
        write_atomic(&image_path(&self.local, &photo.id), image)?;
        let like = Like { photo: photo.clone(), liked_at: now, image_bytes: image.len() as u64 };
        write_json(&self.local.join("likes").join(entry_name(&photo.id)), &like)?;
        // An older removal would otherwise still be read next to a newer like: harmless, but untidy.
        let _ = std::fs::remove_file(self.local.join("removed").join(entry_name(&photo.id)));
        Ok(())
    }

    pub fn unlike(&self, id: &str, now: u64) -> Result<(), String> {
        write_json(&self.local.join("removed").join(entry_name(id)), &Removal { removed_at: now })?;
        delete_like(&self.local, id)
    }

    /// Everything liked here and complete, newest first.
    pub fn list(&self) -> Vec<Like> {
        let mut likes: Vec<Like> = read_entries::<Like>(&self.local.join("likes"))
            .into_values()
            .filter(|like| image_is_complete(&self.local, like))
            .collect();
        likes.sort_by(|a, b| b.liked_at.cmp(&a.liked_at).then(a.photo.id.cmp(&b.photo.id)));
        likes
    }

    pub fn image(&self, id: &str) -> PathBuf {
        image_path(&self.local, id)
    }

    pub fn is_liked(&self, id: &str) -> bool {
        self.list().iter().any(|like| like.photo.id == id)
    }

    /// Brings the two folders to the same state. A missing shared folder is an
    /// error and is never created: it may be an unmounted drive, and writing
    /// there would hide the likes where the user cannot see them.
    pub fn sync(&self) -> Result<(), String> {
        let Some(chosen) = &self.shared else { return Ok(()) };
        if !chosen.is_dir() {
            return Err(format!("Sync folder is unavailable: {}", chosen.display()));
        }
        let shared = chosen.join("cross-wall");
        let (a, b) = (self.local.as_path(), shared.as_path());
        let (likes_a, likes_b) = (likes_in(a), likes_in(b));
        let (gone_a, gone_b) = (removals_in(a), removals_in(b));
        let mut ids: Vec<String> = likes_a.keys().chain(likes_b.keys()).chain(gone_a.keys()).chain(gone_b.keys()).cloned().collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let id = id.as_str();
            let (like_a, like_b) = (likes_a.get(id), likes_b.get(id));
            let removal = [gone_a.get(id), gone_b.get(id)].into_iter().flatten().map(|r| r.removed_at).max();
            let liked_at = [like_a, like_b].into_iter().flatten().map(|l| l.liked_at).max();
            match (removal, liked_at) {
                (Some(removed_at), liked_at) if liked_at.is_none_or(|at| removed_at >= at) => {
                    for root in [a, b] {
                        write_json(&root.join("removed").join(entry_name(id)), &Removal { removed_at })?;
                        delete_like(root, id)?;
                    }
                }
                _ => {
                    // The sides differ only if one is missing or older, so copy toward it.
                    copy_newer(a, like_a, b, like_b)?;
                    copy_newer(b, like_b, a, like_a)?;
                    for root in [a, b] {
                        let _ = std::fs::remove_file(root.join("removed").join(entry_name(id)));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Copies a like to `to` when it is newer there. An image still arriving is
/// left for a later sync, because its size does not match yet.
fn copy_newer(from: &Path, like: Option<&Like>, to: &Path, there: Option<&Like>) -> Result<(), String> {
    let Some(like) = like else { return Ok(()) };
    if there.is_some_and(|t| t.liked_at >= like.liked_at && image_is_complete(to, t)) || !image_is_complete(from, like) {
        return Ok(());
    }
    let id = like.photo.id.as_str();
    let image = std::fs::read(image_path(from, id)).map_err(|e| e.to_string())?;
    write_atomic(&image_path(to, id), &image)?;
    write_json(&to.join("likes").join(entry_name(id)), like)
}

fn likes_in(root: &Path) -> BTreeMap<String, Like> {
    read_entries(&root.join("likes"))
}

fn removals_in(root: &Path) -> BTreeMap<String, Removal> {
    read_entries(&root.join("removed"))
}

fn delete_like(root: &Path, id: &str) -> Result<(), String> {
    for path in [root.join("likes").join(entry_name(id)), image_path(root, id)] {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {e}", path.display())),
            _ => {}
        }
    }
    Ok(())
}

fn image_is_complete(root: &Path, like: &Like) -> bool {
    std::fs::metadata(image_path(root, &like.photo.id)).is_ok_and(|m| m.len() == like.image_bytes)
}

fn image_path(root: &Path, id: &str) -> PathBuf {
    root.join("images").join(format!("{id}.jpg"))
}

fn entry_name(id: &str) -> String {
    format!("{id}.json")
}

fn read_entries<T: serde::de::DeserializeOwned>(dir: &Path) -> BTreeMap<String, T> {
    let mut entries = BTreeMap::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|n| is_entry_id(n))
            .map(String::from);
        // An unreadable entry may still be arriving, so it is skipped until a later sync.
        let value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
        if let (Some(id), Some(value)) = (id, value) {
            entries.insert(id, value);
        }
    }
    entries
}

/// Excludes conflict copies and anything else this app did not name, and any
/// name that could reach outside the folder.
fn is_entry_id(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write_atomic(path, &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let describe = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(path.parent().ok_or("path has no folder")?).map_err(describe)?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes).map_err(describe)?;
    std::fs::rename(&temporary, path).map_err(describe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cross_wall_sources::Sources;

    fn photo(id: u64) -> Photo {
        let s = String::new;
        Photo {
            id: id.to_string(),
            license: s(),
            width: 1,
            height: 1,
            url: s(),
            photographer: s(),
            photographer_url: s(),
            src: Sources { original: s(), large2x: s(), medium: s() },
        }
    }

    /// Two computers sharing one folder.
    struct Pair {
        _dir: tempfile::TempDir,
        a: Library,
        b: Library,
        shared: PathBuf,
    }
    fn pair() -> Pair {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        let a = Library::new(dir.path().join("a"), Some(shared.clone()));
        let b = Library::new(dir.path().join("b"), Some(shared.clone()));
        Pair { _dir: dir, a, b, shared }
    }
    fn ids(l: &Library) -> Vec<u64> {
        let mut v: Vec<u64> = l.list().iter().map(|x| x.photo.id.parse().unwrap()).collect();
        v.sort();
        v
    }
    fn sync_all(p: &Pair) {
        for _ in 0..2 {
            p.a.sync().unwrap();
            p.b.sync().unwrap();
        }
    }

    #[test]
    fn a_like_reaches_the_other_computer_with_its_image() {
        let p = pair();
        p.a.like(&photo(1), b"img", 10).unwrap();
        sync_all(&p);
        assert_eq!(ids(&p.b), [1]);
        assert_eq!(std::fs::read(p.b.image("1")).unwrap(), b"img");
    }

    #[test]
    fn concurrent_likes_are_all_kept() {
        let p = pair();
        p.a.like(&photo(1), b"x", 10).unwrap();
        p.b.like(&photo(2), b"y", 10).unwrap();
        sync_all(&p);
        assert_eq!(ids(&p.a), [1, 2]);
        assert_eq!(ids(&p.b), [1, 2]);
    }

    #[test]
    fn an_unlike_spreads_and_does_not_come_back() {
        let p = pair();
        p.a.like(&photo(1), b"x", 10).unwrap();
        sync_all(&p);
        p.b.unlike("1", 20).unwrap();
        sync_all(&p);
        assert!(ids(&p.a).is_empty() && ids(&p.b).is_empty());
        assert!(!p.a.image("1").exists());
    }

    #[test]
    fn a_newer_like_beats_an_older_unlike_and_the_reverse() {
        let p = pair();
        p.a.like(&photo(1), b"x", 10).unwrap();
        sync_all(&p);
        p.b.unlike("1", 20).unwrap();
        p.a.like(&photo(1), b"x", 30).unwrap();
        sync_all(&p);
        assert_eq!(ids(&p.b), [1]);
        p.a.unlike("1", 40).unwrap();
        p.b.like(&photo(1), b"x", 35).unwrap();
        sync_all(&p);
        assert!(ids(&p.b).is_empty());
    }

    #[test]
    fn a_partly_synced_image_is_not_shown_or_copied() {
        let p = pair();
        p.a.like(&photo(1), b"full", 10).unwrap();
        p.a.sync().unwrap();
        std::fs::write(p.shared.join("cross-wall/images/1.jpg"), b"pa").unwrap();
        p.b.sync().unwrap();
        assert!(ids(&p.b).is_empty());
        std::fs::write(p.shared.join("cross-wall/images/1.jpg"), b"full").unwrap();
        p.b.sync().unwrap();
        assert_eq!(ids(&p.b), [1]);
    }

    #[test]
    fn a_like_without_its_image_is_not_listed() {
        let p = pair();
        p.a.like(&photo(1), b"x", 10).unwrap();
        std::fs::remove_file(p.a.image("1")).unwrap();
        assert!(ids(&p.a).is_empty());
    }

    #[test]
    fn conflict_copies_and_broken_entries_are_ignored() {
        let p = pair();
        p.a.like(&photo(1), b"x", 10).unwrap();
        p.a.sync().unwrap();
        let likes = p.shared.join("cross-wall/likes");
        std::fs::write(likes.join("1 (conflict).json"), "{}").unwrap();
        std::fs::write(likes.join("2.json"), "{broken").unwrap();
        sync_all(&p);
        assert_eq!(ids(&p.b), [1]);
    }

    #[test]
    fn a_missing_shared_folder_is_an_error_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("unmounted");
        let l = Library::new(dir.path().join("a"), Some(shared.clone()));
        l.like(&photo(1), b"x", 10).unwrap();
        assert!(l.sync().unwrap_err().contains("unavailable"));
        assert!(!shared.exists());
        assert_eq!(ids(&l), [1]);
    }

    #[test]
    fn without_a_shared_folder_likes_stay_local() {
        let dir = tempfile::tempdir().unwrap();
        let l = Library::new(dir.path().join("a"), None);
        l.like(&photo(1), b"x", 10).unwrap();
        l.sync().unwrap();
        assert_eq!(ids(&l), [1]);
    }
}
