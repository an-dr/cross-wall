//! Curated and search result paging. A generation number makes answers to a
//! replaced query harmless, because network replies arrive in any order.
use std::sync::mpsc::{channel, Receiver, Sender};

use cross_wall_sources::{Client, Error, MinSize, Orientation, Page, Photo, Source};

/// Consecutive pages to read past when the app's own size check empties them,
/// so a source that cannot filter does not stop at a blank screen.
const MAX_EMPTY_PAGES: u32 = 5;

#[derive(Clone, PartialEq, Default)]
struct Query {
    min: MinSize,
    source: Source,
    text: String,
    orientation: Orientation,
}

pub struct Browse {
    pub source: Source,
    pub text: String,
    pub orientation: Orientation,
    pub min: MinSize,
    empty_pages: u32,
    read_on: bool,
    query: Query,
    pub photos: Vec<Photo>,
    page: u32,
    pub has_more: bool,
    pub loading: bool,
    pub message: String,
    generation: u64,
    started: bool,
    tx: Sender<(u64, Result<Page, Error>)>,
    rx: Receiver<(u64, Result<Page, Error>)>,
}

impl Browse {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            source: Source::default(),
            text: String::new(),
            orientation: Orientation::Any,
            min: MinSize::ANY,
            empty_pages: 0,
            read_on: false,
            query: Query::default(),
            photos: Vec::new(),
            page: 0,
            has_more: false,
            loading: false,
            message: String::new(),
            generation: 0,
            started: false,
            tx,
            rx,
        }
    }

    pub fn start_once(&mut self, client: &Client) {
        if !self.started {
            self.search(client);
        }
    }

    /// Replaces the results with the current text and orientation. Empty text means the source's own selection.
    pub fn search(&mut self, client: &Client) {
        self.started = true;
        self.generation += 1;
        self.query = Query { min: self.min, source: self.source, text: self.text.trim().to_string(), orientation: self.orientation };
        self.photos.clear();
        self.empty_pages = 0;
        self.page = 0;
        self.has_more = false;
        self.message.clear();
        self.load_more(client);
    }

    pub fn load_more(&mut self, client: &Client) {
        self.loading = true;
        let (tx, generation, query, page) = (self.tx.clone(), self.generation, self.query.clone(), self.page + 1);
        let client = client.clone();
        std::thread::spawn(move || {
            let answer = client.search(query.source, &query.text, query.orientation, query.min, page);
            let _ = tx.send((generation, answer));
        });
    }

    pub fn poll(&mut self, client: &Client) {
        while let Ok((generation, answer)) = self.rx.try_recv() {
            self.receive(generation, answer);
        }
        if std::mem::take(&mut self.read_on) {
            self.load_more(client);
        }
    }

    fn receive(&mut self, generation: u64, answer: Result<Page, Error>) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        match answer {
            Ok(page) => {
                self.page += 1;
                self.has_more = page.has_more;
                let added = page.photos.len();
                self.photos.extend(page.photos);
                self.empty_pages = if added == 0 { self.empty_pages + 1 } else { 0 };
                self.read_on = added == 0 && self.has_more && self.empty_pages < MAX_EMPTY_PAGES;
                if self.photos.is_empty() && !self.read_on {
                    self.message = if self.has_more {
                        "Nothing at this size so far. Load more, or lower the minimum resolution.".into()
                    } else {
                        "No photos found. Try another source or a lower minimum resolution.".into()
                    };
                }
            }
            Err(error) => self.message = error.to_string(),
        }
    }

    /// Empties the results for a source that cannot be searched yet.
    pub fn clear(&mut self) {
        self.reset();
        self.photos.clear();
        self.has_more = false;
    }

    /// Lets a changed API key be tried at once.
    pub fn reset(&mut self) {
        self.started = false;
        self.generation += 1;
        self.loading = false;
        self.message.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(ids: &[u64], more: bool) -> Result<Page, Error> {
        let photos = ids
            .iter()
            .map(|id| {
                serde_json::from_str(&format!(
                    r#"{{"id":{id},"width":1,"height":1,"url":"u","photographer":"p","photographer_url":"u",
                    "src":{{"original":"o","large2x":"l","medium":"m"}}}}"#
                ))
                .unwrap()
            })
            .collect();
        Ok(Page { photos, has_more: more })
    }

    #[test]
    fn pages_append_and_track_more() {
        let mut b = Browse::new();
        b.generation = 1;
        b.receive(1, page(&[1, 2], true));
        b.receive(1, page(&[3], false));
        assert_eq!(b.photos.len(), 3);
        assert_eq!(b.page, 2);
        assert!(!b.has_more);
    }

    #[test]
    fn empty_pages_read_on_up_to_a_limit() {
        let mut b = Browse::new();
        for _ in 0..MAX_EMPTY_PAGES - 1 {
            b.receive(0, page(&[], true));
            assert!(std::mem::take(&mut b.read_on));
        }
        b.receive(0, page(&[], true));
        assert!(!b.read_on);
        assert!(b.message.contains("minimum resolution"));
    }

    #[test]
    fn answers_to_a_replaced_query_are_ignored() {
        let mut b = Browse::new();
        b.generation = 2;
        b.loading = true;
        b.receive(1, page(&[1], true));
        assert!(b.photos.is_empty() && b.loading);
    }

    #[test]
    fn empty_results_and_errors_become_messages() {
        let mut b = Browse::new();
        b.receive(0, page(&[], false));
        assert!(b.message.starts_with("No photos found."));
        b.receive(0, Err(Error::RateLimited));
        assert!(b.message.contains("rate limit"));
    }
}
