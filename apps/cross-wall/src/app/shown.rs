//! What a gallery tile and the preview show: a photo offered by a source, or a
//! file in the wallpaper folder. Each kind keeps its own data, so code that
//! needs one kind matches on it instead of guessing from an id.
use cross_wall_library::Entry;
use cross_wall_sources::Photo;

#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    Remote(Photo),
    Local(Entry),
}

/// The credit line under a photo. Empty strings are not shown.
pub struct Credit<'a> {
    pub name: &'a str,
    pub link: &'a str,
    pub license: &'a str,
    /// The photo's page at its source.
    pub page: &'a str,
}

impl Shown {
    /// Unique among everything shown, for image slots, widget ids, and busy state.
    pub fn key(&self) -> String {
        match self {
            Shown::Remote(photo) => format!("remote:{}", photo.id),
            Shown::Local(entry) => format!("local:{}", entry.path),
        }
    }

    /// Width and height in pixels; zero when unknown.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Shown::Remote(photo) => (photo.width, photo.height),
            Shown::Local(entry) => (entry.width, entry.height),
        }
    }

    /// The path in the wallpaper folder, for a local file.
    pub fn local_path(&self) -> Option<&str> {
        match self {
            Shown::Local(entry) => Some(&entry.path),
            Shown::Remote(_) => None,
        }
    }

    pub fn credit(&self) -> Credit<'_> {
        match self {
            Shown::Remote(p) => Credit {
                name: &p.photographer,
                link: &p.photographer_url,
                license: &p.license,
                page: &p.url,
            },
            Shown::Local(entry) => match &entry.info.credit {
                Some(c) => Credit {
                    name: &c.photographer,
                    link: &c.photographer_url,
                    license: &c.license,
                    page: &c.url,
                },
                // A file the app did not download is known by its name.
                None => Credit {
                    name: entry.name(),
                    link: "",
                    license: "",
                    page: "",
                },
            },
        }
    }
}
