//! The wallpaper folder: a folder the user chose, whose contents are the truth.
//! Likes and photo credits live in a hidden `.cross-wall.json` at its root (ADR-003).
//! Paths are relative to the root and always use `/`. Names starting with a dot
//! are never listed, which keeps this app's own files out of the library.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

const METADATA: &str = ".cross-wall.json";
/// How long one scan may spend reading image sizes, so other work gets a turn.
const SIZE_BUDGET: Duration = Duration::from_millis(150);
const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "bmp"];
/// Characters Windows refuses, refused everywhere so a folder can move between systems.
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Who made a downloaded photo and where it came from, as saved in the folder.
/// Files from 0.2.0 store a source's whole photo record here; the extra fields are ignored.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Credit {
    /// The source's id for the photo, prefixed by the source.
    pub id: String,
    /// The photo's page at the source.
    pub url: String,
    pub photographer: String,
    pub photographer_url: String,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileInfo {
    pub liked: bool,
    /// Set for photos downloaded from a source. Named `photo` up to 0.2.0.
    #[serde(alias = "photo")]
    pub credit: Option<Credit>,
    /// Milliseconds since the Unix epoch; zero for files the app did not download.
    pub added_at: u64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
struct Metadata {
    files: BTreeMap<String, FileInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub info: FileInfo,
}

impl Entry {
    pub fn name(&self) -> &str {
        file_name(&self.path)
    }

    pub fn folder(&self) -> &str {
        parent(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    /// Every folder except the root, sorted.
    pub folders: Vec<String>,
    /// Every image, sorted by path.
    pub files: Vec<Entry>,
}

impl Snapshot {
    /// The file holding a photo downloaded from a source, wherever it was moved to.
    pub fn find_photo(&self, id: &str) -> Option<&Entry> {
        self.files
            .iter()
            .find(|e| e.info.credit.as_ref().is_some_and(|c| c.id == id))
    }

    pub fn find_path(&self, path: &str) -> Option<&Entry> {
        self.files.iter().find(|e| e.path == path)
    }
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The containing folder; the root is the empty string.
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_string()
    } else {
        format!("{folder}/{name}")
    }
}

/// Why a new file or folder name cannot be used.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("A name cannot be empty.".into());
    }
    if name.starts_with('.') {
        return Err("A name cannot start with a dot, because such items are hidden here.".into());
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("A name cannot end with a dot or a space.".into());
    }
    if name
        .chars()
        .any(|c| FORBIDDEN.contains(&c) || c.is_control())
    {
        return Err("A name cannot contain / \\ : * ? \" < > |".into());
    }
    Ok(())
}

#[derive(Debug)]
pub struct WallpaperFolder {
    root: PathBuf,
    /// Image sizes by path and modification time, so a rescan only reads what changed.
    sizes: HashMap<String, (SystemTime, u32, u32)>,
    scan_complete: bool,
    /// What happened to a metadata file that could not be read, until it is told.
    notice: RefCell<Option<String>>,
}

impl WallpaperFolder {
    /// Refuses a missing folder rather than creating it: it may be an unmounted drive.
    pub fn open(root: PathBuf) -> Result<Self, String> {
        let folder = Self {
            root,
            sizes: HashMap::new(),
            scan_complete: false,
            notice: RefCell::new(None),
        };
        folder.check_available()?;
        Ok(folder)
    }

    fn check_available(&self) -> Result<(), String> {
        if self.root.is_dir() {
            Ok(())
        } else {
            Err(format!(
                "The wallpaper folder is unavailable: {}",
                self.root.display()
            ))
        }
    }

    /// Whether the last scan read every image size. If not, scan again for the rest.
    pub fn is_scan_complete(&self) -> bool {
        self.scan_complete
    }

    /// Something the user should be told that is not a failure of the last action,
    /// such as likes that could not be read. Each notice is given once.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.get_mut().take()
    }

    pub fn snapshot(&mut self) -> Result<Snapshot, String> {
        self.snapshot_within(SIZE_BUDGET)
    }

    /// Images whose size was not reached in time list with size zero for now.
    pub fn snapshot_within(&mut self, budget: Duration) -> Result<Snapshot, String> {
        self.check_available()?;
        let deadline = Instant::now() + budget;
        let mut complete = true;
        let metadata = self.read_metadata();
        let mut snapshot = Snapshot::default();
        let mut pending = vec![String::new()];
        let mut seen = HashMap::new();
        while let Some(folder) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(self.root.join(&folder)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(name) = entry.file_name().to_str().map(String::from) else {
                    continue;
                };
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                // Links are skipped: they could lead outside the folder, or in a circle.
                if name.starts_with('.') || kind.is_symlink() {
                    continue;
                }
                let path = join(&folder, &name);
                if kind.is_dir() {
                    pending.push(path.clone());
                    snapshot.folders.push(path);
                } else if is_image(&name) {
                    let modified = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    let known = self
                        .sizes
                        .get(&path)
                        .filter(|s| s.0 == modified)
                        .map(|s| (s.1, s.2));
                    let (width, height) = match known {
                        Some(size) => {
                            seen.insert(path.clone(), (modified, size.0, size.1));
                            size
                        }
                        None if Instant::now() < deadline => {
                            let size = image::image_dimensions(entry.path()).unwrap_or((0, 0));
                            seen.insert(path.clone(), (modified, size.0, size.1));
                            size
                        }
                        None => {
                            complete = false;
                            (0, 0)
                        }
                    };
                    let info = metadata.files.get(&path).cloned().unwrap_or_default();
                    snapshot.files.push(Entry {
                        path,
                        width,
                        height,
                        info,
                    });
                }
            }
        }
        self.sizes = seen;
        self.scan_complete = complete;
        snapshot.folders.sort();
        snapshot.files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(snapshot)
    }

    /// Saves a downloaded photo in the root. A photo already kept, wherever it was
    /// moved to, is not saved twice. Returns the path. The image is complete before
    /// it takes its name, so an interrupted write never appears in the library.
    pub fn add_photo(
        &mut self,
        credit: &Credit,
        image: &[u8],
        liked: bool,
        now: u64,
    ) -> Result<String, String> {
        self.check_available()?;
        if let Some(path) = self.kept_path(&credit.id) {
            if liked {
                self.set_liked(&path, true)?;
            }
            return Ok(path);
        }
        let extension = extension_for(image);
        // Sources already give safe ids; the folder does not rely on that.
        let base: String = credit
            .id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let mut name = format!("{base}.{extension}");
        let mut number = 2;
        while self.root.join(&name).exists() {
            name = format!("{base}-{number}.{extension}");
            number += 1;
        }
        write_atomic(&self.root.join(&name), image)?;
        let mut metadata = self.read_metadata();
        metadata.files.insert(
            name.clone(),
            FileInfo {
                liked,
                credit: Some(credit.clone()),
                added_at: now,
            },
        );
        self.write_metadata(metadata)?;
        Ok(name)
    }

    /// Where a source photo is kept, from the metadata alone, so no scan is needed.
    fn kept_path(&self, id: &str) -> Option<String> {
        let metadata = self.read_metadata();
        metadata
            .files
            .into_iter()
            .find(|(path, info)| {
                info.credit.as_ref().is_some_and(|c| c.id == id) && self.root.join(path).is_file()
            })
            .map(|(path, _)| path)
    }

    /// Whether this source photo is already kept, so a caller can skip downloading it.
    pub fn has_photo(&self, id: &str) -> bool {
        self.kept_path(id).is_some()
    }

    /// Records a like on a kept photo without any image bytes.
    pub fn like_kept_photo(&mut self, id: &str) -> Result<(), String> {
        let path = self
            .kept_path(id)
            .ok_or("That photo is no longer in the folder.")?;
        self.set_liked(&path, true)
    }

    pub fn set_liked(&mut self, path: &str, liked: bool) -> Result<(), String> {
        let full = self.resolve(path)?;
        if !full.is_file() {
            return Err(format!("{path} no longer exists."));
        }
        let mut metadata = self.read_metadata();
        metadata.files.entry(path.to_string()).or_default().liked = liked;
        self.write_metadata(metadata)
    }

    pub fn create_folder(&mut self, parent_folder: &str, name: &str) -> Result<String, String> {
        check_name(name)?;
        let path = join(parent_folder, name);
        let full = self.resolve(&path)?;
        if full.exists() {
            return Err(format!("{name} already exists here."));
        }
        std::fs::create_dir(&full).map_err(|e| format!("{}: {e}", full.display()))?;
        Ok(path)
    }

    /// Renames a file or folder in place.
    pub fn rename(&mut self, path: &str, new_name: &str) -> Result<String, String> {
        check_name(new_name)?;
        if !self.resolve(path)?.is_dir() && !is_image(new_name) {
            return Err("A wallpaper must keep an image extension such as .jpg.".into());
        }
        // On a case-insensitive disk a case-only rename names the same file, and is
        // still a real rename. On a case-sensitive one the other name is another file.
        let destination = join(parent(path), new_name);
        let same_item = destination.eq_ignore_ascii_case(path)
            && is_same_file(&self.resolve(path)?, &self.resolve(&destination)?);
        self.relocate(path, &destination, same_item)
    }

    /// Moves a file or folder into another folder; the root is the empty string.
    pub fn move_to(&mut self, path: &str, folder: &str) -> Result<String, String> {
        if folder == path || folder.starts_with(&format!("{path}/")) {
            return Err("A folder cannot be moved into itself.".into());
        }
        if !self.resolve(folder)?.is_dir() {
            return Err(format!("{folder} is not a folder."));
        }
        self.relocate(path, &join(folder, file_name(path)), false)
    }

    /// Sends a file or folder to the system's Recycle Bin or Trash, so it can be restored.
    pub fn delete(&mut self, path: &str) -> Result<(), String> {
        if path.is_empty() {
            return Err("The wallpaper folder itself cannot be deleted here.".into());
        }
        let full = self.resolve(path)?;
        trash::delete(&full).map_err(|e| format!("Could not move {path} to the trash: {e}"))?;
        let mut metadata = self.read_metadata();
        metadata.files.retain(|key, _| !is_at_or_under(key, path));
        self.write_metadata(metadata)
    }

    fn relocate(&mut self, from: &str, to: &str, same_item: bool) -> Result<String, String> {
        let (source, destination) = (self.resolve(from)?, self.resolve(to)?);
        if !source.exists() {
            return Err(format!("{from} no longer exists."));
        }
        if from == to {
            return Ok(to.to_string());
        }
        // A plain rename would overwrite a file silently on some systems.
        if destination.exists() && !same_item {
            return Err(format!("{} already exists there.", file_name(to)));
        }
        std::fs::rename(&source, &destination)
            .map_err(|e| format!("Could not move {from}: {e}"))?;
        let mut metadata = self.read_metadata();
        let moved: Vec<String> = metadata
            .files
            .keys()
            .filter(|key| is_at_or_under(key, from))
            .cloned()
            .collect();
        for key in moved {
            if let Some(info) = metadata.files.remove(&key) {
                metadata
                    .files
                    .insert(format!("{to}{}", &key[from.len()..]), info);
            }
        }
        self.write_metadata(metadata)?;
        Ok(to.to_string())
    }

    /// Absolute path of an item, refusing anything that could leave the folder.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let safe = path.is_empty()
            || path.split('/').all(|part| {
                !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', ':'])
            });
        if safe {
            Ok(self.root.join(path))
        } else {
            Err(format!("Refused path: {path}"))
        }
    }

    /// A damaged file is set aside under a name of its own, so it is never
    /// overwritten and may be recovered by hand, and the user is told.
    fn read_metadata(&self) -> Metadata {
        let path = self.root.join(METADATA);
        let Ok(bytes) = std::fs::read(&path) else {
            return Metadata::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis());
            let mut aside = format!(".cross-wall.damaged-{stamp}.json");
            let mut number = 2;
            while self.root.join(&aside).exists() {
                aside = format!(".cross-wall.damaged-{stamp}-{number}.json");
                number += 1;
            }
            *self.notice.borrow_mut() = Some(match std::fs::rename(&path, self.root.join(&aside)) {
                Ok(()) => format!("Likes and credits could not be read. The old file was kept as {aside} in the wallpaper folder."),
                Err(error) => format!("Likes and credits could not be read, and the file could not be set aside: {error}"),
            });
            Metadata::default()
        })
    }

    /// Also forgets files that are gone, because the folder is the truth.
    fn write_metadata(&self, mut metadata: Metadata) -> Result<(), String> {
        metadata.files.retain(|key, info| {
            (info.liked || info.credit.is_some()) && self.root.join(key).is_file()
        });
        let path = self.root.join(METADATA);
        let text = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
        write_atomic(&path, &text)?;
        hide(&path);
        Ok(())
    }
}

/// Whether two paths name one existing file or folder on disk.
#[cfg(unix)]
fn is_same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Whether two paths name one existing file or folder on disk.
#[cfg(not(unix))]
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Whether `key` is the item at `path` or inside it, for relative folder paths.
pub fn is_at_or_under(key: &str, path: &str) -> bool {
    key == path || key.starts_with(&format!("{path}/"))
}

fn is_image(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        IMAGE_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

/// Sources do not always serve what their link's name says.
pub fn extension_for(image: &[u8]) -> &'static str {
    match image {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        [b'B', b'M', ..] => "bmp",
        _ => "jpg",
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let describe = |e: std::io::Error| format!("{}: {e}", path.display());
    let folder = path.parent().ok_or("path has no folder")?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("path has no name")?;
    let temporary = folder.join(format!(".{}.tmp", name.trim_start_matches('.')));
    std::fs::write(&temporary, bytes).map_err(describe)?;
    std::fs::rename(&temporary, path).map_err(describe)
}

#[cfg(windows)]
fn hide(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    // SAFETY: `wide` is a NUL-terminated path that outlives the call. A failure only
    // leaves the file visible, which harms nothing.
    unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
}

/// The leading dot already hides it elsewhere.
#[cfg(not(windows))]
fn hide(_path: &Path) {}

#[cfg(test)]
mod tests;
