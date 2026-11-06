//! Client for the Pexels photo API. No UI and no engine dependency.
use std::fmt;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const API: &str = "https://api.pexels.com/v1";
const PER_PAGE: u32 = 30;
/// Generous for an original photo, while refusing a response that is plainly not one.
const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sources {
    pub original: String,
    pub large2x: String,
    pub medium: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub url: String,
    pub photographer: String,
    pub photographer_url: String,
    pub src: Sources,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Page {
    pub photos: Vec<Photo>,
    #[serde(default)]
    pub next_page: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Any,
    Landscape,
    Portrait,
    Square,
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
            Error::BadKey => write!(f, "Pexels rejected the API key. Check it in Settings."),
            Error::RateLimited => write!(f, "Pexels rate limit reached. Try again later."),
            Error::Network(e) => write!(f, "Could not reach Pexels: {e}"),
            Error::Status(code) => write!(f, "Pexels answered with HTTP {code}."),
            Error::Malformed(e) => write!(f, "Pexels sent an unexpected answer: {e}"),
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
        response
            .into_reader()
            .take(MAX_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|e| e.to_string())?;
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

    pub fn curated(&self, page: u32) -> Result<Page, Error> {
        self.page(&format!("{API}/curated?per_page={PER_PAGE}&page={page}"))
    }

    pub fn search(&self, query: &str, orientation: Orientation, page: u32) -> Result<Page, Error> {
        let mut url = format!("{API}/search?query={}&per_page={PER_PAGE}&page={page}", encode(query));
        match orientation {
            Orientation::Any => {}
            Orientation::Landscape => url.push_str("&orientation=landscape"),
            Orientation::Portrait => url.push_str("&orientation=portrait"),
            Orientation::Square => url.push_str("&orientation=square"),
        }
        self.page(&url)
    }

    /// Downloads an image from a URL Pexels gave. The key is not sent: images are on a public CDN.
    pub fn download(&self, url: &str) -> Result<Vec<u8>, Error> {
        check(self.transport.get(url, None))
    }

    fn page(&self, url: &str) -> Result<Page, Error> {
        let body = check(self.transport.get(url, Some(&self.key)))?;
        serde_json::from_slice(&body).map_err(|e| Error::Malformed(e.to_string()))
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

fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Canned {
        answer: Result<(u16, Vec<u8>), String>,
        asked: Mutex<Vec<(String, Option<String>)>>,
    }
    impl Transport for Canned {
        fn get(&self, url: &str, auth: Option<&str>) -> Result<(u16, Vec<u8>), String> {
            self.asked.lock().unwrap().push((url.into(), auth.map(Into::into)));
            self.answer.clone()
        }
    }
    fn client(answer: Result<(u16, Vec<u8>), String>) -> (Client, Arc<Canned>) {
        let canned = Arc::new(Canned { answer, asked: Mutex::new(Vec::new()) });
        (Client::with_transport("KEY", canned.clone()), canned)
    }

    const PAGE: &str = r##"{"page":1,"photos":[{"id":7,"width":6000,"height":4000,"url":"https://p/7",
      "photographer":"A B","photographer_url":"https://p/a","avg_color":"#000",
      "src":{"original":"https://i/o","large2x":"https://i/l","medium":"https://i/m","tiny":"https://i/t"}}],
      "next_page":"https://n"}"##;

    #[test]
    fn parses_a_page_and_sends_the_key() {
        let (c, canned) = client(Ok((200, PAGE.into())));
        let page = c.curated(2).unwrap();
        assert_eq!(page.photos[0].id, 7);
        assert_eq!(page.photos[0].src.medium, "https://i/m");
        assert!(page.next_page.is_some());
        let asked = canned.asked.lock().unwrap();
        assert!(asked[0].0.ends_with("/curated?per_page=30&page=2"));
        assert_eq!(asked[0].1.as_deref(), Some("KEY"));
    }

    #[test]
    fn search_encodes_the_query_and_orientation() {
        let (c, canned) = client(Ok((200, PAGE.into())));
        c.search("sea & sky/é", Orientation::Landscape, 1).unwrap();
        let url = canned.asked.lock().unwrap()[0].0.clone();
        assert!(url.contains("query=sea%20%26%20sky%2F%C3%A9&"), "{url}");
        assert!(url.ends_with("&orientation=landscape"));
    }

    #[test]
    fn download_does_not_send_the_key() {
        let (c, canned) = client(Ok((200, vec![1, 2])));
        assert_eq!(c.download("https://i/o").unwrap(), vec![1, 2]);
        assert_eq!(canned.asked.lock().unwrap()[0].1, None);
    }

    #[test]
    fn maps_failures() {
        for (status, expected) in [(401, Error::BadKey), (429, Error::RateLimited), (500, Error::Status(500))] {
            assert_eq!(client(Ok((status, vec![]))).0.curated(1).unwrap_err(), expected);
        }
        assert!(matches!(client(Err("down".into())).0.curated(1), Err(Error::Network(_))));
        assert!(matches!(client(Ok((200, b"nope".to_vec()))).0.curated(1), Err(Error::Malformed(_))));
    }
}
