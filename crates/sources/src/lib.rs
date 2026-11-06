//! Photo sources behind one search call. No UI and no engine dependency.
mod providers;

use std::fmt;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Generous for an original photo, while refusing a response that is plainly not one.
const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Wallhaven,
    Openverse,
    Commons,
    Picsum,
    Nasa,
    Pexels,
}

impl Source {
    pub const ALL: [Source; 6] =
        [Source::Wallhaven, Source::Openverse, Source::Commons, Source::Picsum, Source::Nasa, Source::Pexels];

    pub fn name(self) -> &'static str {
        match self {
            Source::Wallhaven => "Wallhaven",
            Source::Openverse => "Openverse",
            Source::Commons => "Wikimedia Commons",
            Source::Picsum => "Lorem Picsum",
            Source::Nasa => "NASA Images",
            Source::Pexels => "Pexels",
        }
    }

    pub fn home(self) -> &'static str {
        match self {
            Source::Wallhaven => "https://wallhaven.cc",
            Source::Openverse => "https://openverse.org",
            Source::Commons => "https://commons.wikimedia.org",
            Source::Picsum => "https://picsum.photos",
            Source::Nasa => "https://images.nasa.gov",
            Source::Pexels => "https://www.pexels.com",
        }
    }

    pub fn needs_key(self) -> bool {
        self == Source::Pexels
    }

    pub fn has_search(self) -> bool {
        self != Source::Picsum
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sources {
    pub original: String,
    pub large2x: String,
    pub medium: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    /// Safe as a file name. Likes stored before there were several sources hold a number.
    #[serde(deserialize_with = "id_from_text_or_number")]
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub url: String,
    pub photographer: String,
    pub photographer_url: String,
    pub src: Sources,
    #[serde(default)]
    pub license: String,
}

fn id_from_text_or_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        other => Err(serde::de::Error::custom(format!("bad photo id: {other}"))),
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub photos: Vec<Photo>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Any,
    Landscape,
    Portrait,
    Square,
}

impl Orientation {
    /// For sources that cannot filter by shape. An unknown size is kept.
    fn accepts(self, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 {
            return true;
        }
        match self {
            Orientation::Any => true,
            Orientation::Landscape => width > height,
            Orientation::Portrait => height > width,
            Orientation::Square => width.abs_diff(height) * 10 <= width.max(height),
        }
    }
}

/// The smallest photo to show. Zero means any size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MinSize {
    pub width: u32,
    pub height: u32,
}

impl MinSize {
    pub const ANY: MinSize = MinSize { width: 0, height: 0 };

    /// A portrait search for a landscape screen's size wants the same size turned.
    pub fn turned_for(self, orientation: Orientation) -> MinSize {
        if orientation == Orientation::Portrait {
            MinSize { width: self.width.min(self.height), height: self.width.max(self.height) }
        } else {
            self
        }
    }

    /// An unknown size is kept: some sources do not report one.
    fn accepts(self, width: u32, height: u32) -> bool {
        width == 0 || height == 0 || (width >= self.width && height >= self.height)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    BadKey,
    RateLimited,
    Network(String),
    Status(u16),
    Malformed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::BadKey => write!(f, "The photo source rejected the request. For Pexels, check your API key in Settings."),
            Error::RateLimited => write!(f, "The photo source's rate limit was reached. Try again later or pick another source."),
            Error::Network(e) => write!(f, "Could not reach the photo source: {e}"),
            Error::Status(code) => write!(f, "The photo source answered with HTTP {code}."),
            Error::Malformed(e) => write!(f, "The photo source sent an unexpected answer: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// One HTTP GET. A seam so tests run with no network.
pub trait Transport: Send + Sync {
    /// Answers the status and body; an `Err` means no answer at all.
    fn get(&self, url: &str, authorization: Option<&str>) -> Result<(u16, Vec<u8>), String>;
}

pub struct Https;

impl Transport for Https {
    fn get(&self, url: &str, authorization: Option<&str>) -> Result<(u16, Vec<u8>), String> {
        if !url.starts_with("https://") {
            return Err("only https URLs may be fetched".into());
        }
        let connector = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
        let agent = ureq::builder()
            .tls_connector(Arc::new(connector))
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            // Wikimedia refuses requests with no descriptive client name.
            .user_agent(concat!("cross-wall/", env!("CARGO_PKG_VERSION"), " (https://github.com/an-dr/cross-wall)"))
            .build();
        let mut request = agent.get(url);
        if let Some(key) = authorization {
            request = request.set("Authorization", key);
        }
        let response = match request.call() {
            Ok(response) => response,
            Err(ureq::Error::Status(code, _)) => return Ok((code, Vec::new())),
            Err(error) => return Err(error.to_string()),
        };
        let status = response.status();
        let mut body = Vec::new();
        // One byte past the cap, so an oversized body fails instead of truncating.
        response.into_reader().take(MAX_BYTES + 1).read_to_end(&mut body).map_err(|e| e.to_string())?;
        if body.len() as u64 > MAX_BYTES {
            return Err("response too large".into());
        }
        Ok((status, body))
    }
}

#[derive(Clone)]
pub struct Client {
    key: String,
    transport: Arc<dyn Transport>,
}

impl Client {
    pub fn new(key: impl Into<String>) -> Self {
        Self::with_transport(key, Arc::new(Https))
    }

    pub fn with_transport(key: impl Into<String>, transport: Arc<dyn Transport>) -> Self {
        Self { key: key.into(), transport }
    }

    /// One page of results. An empty query means the source's own selection.
    pub fn search(&self, source: Source, query: &str, orientation: Orientation, min: MinSize, page: u32) -> Result<Page, Error> {
        let min = min.turned_for(orientation);
        let url = providers::url(source, query, orientation, min, page);
        let key = source.needs_key().then_some(self.key.as_str());
        let body = check(self.transport.get(&url, key))?;
        let mut result = providers::parse(source, &body)?;
        // Several sources cannot filter by size or shape, so every answer is checked here.
        result.photos.retain(|p| orientation.accepts(p.width, p.height) && min.accepts(p.width, p.height));
        Ok(result)
    }

    /// A small version of the photo. Thumbnail services can fail for images that
    /// still exist, as Openverse's proxy does, so this falls back to the original.
    pub fn download_thumbnail(&self, photo: &Photo) -> Result<Vec<u8>, Error> {
        self.download(&photo.src.medium).or_else(|_| self.download(&photo.src.original))
    }

    /// Downloads an image from a URL a source gave. No key is sent: images are on public CDNs.
    pub fn download(&self, url: &str) -> Result<Vec<u8>, Error> {
        check(self.transport.get(url, None))
    }
}

fn check(answer: Result<(u16, Vec<u8>), String>) -> Result<Vec<u8>, Error> {
    match answer.map_err(Error::Network)? {
        (200, body) => Ok(body),
        (401 | 403, _) => Err(Error::BadKey),
        (429, _) => Err(Error::RateLimited),
        (code, _) => Err(Error::Status(code)),
    }
}

#[cfg(test)]
mod tests;
