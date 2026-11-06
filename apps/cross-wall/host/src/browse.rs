//! Curated and search result paging. A generation number makes answers to a
//! replaced query harmless, because network replies arrive in any order.
use std::sync::mpsc::{channel, Receiver, Sender};

use cross_wall_pexels::{Client, Error, Orientation, Page, Photo};

#[derive(Clone, PartialEq, Default)]
struct Query {
    text: String,
    orientation: Orientation,
}

pub struct Browse {
    pub text: String,
    pub orientation: Orientation,
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
            text: String::new(),
            orientation: Orientation::Any,
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

    /// Replaces the results with the current text and orientation. Empty text means curated.
    pub fn search(&mut self, client: &Client) {
        self.started = true;
        self.generation += 1;
        self.query = Query { text: self.text.trim().to_string(), orientation: self.orientation };
        self.photos.clear();
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
            let answer = if query.text.is_empty() {
                client.curated(page)
            } else {
                client.search(&query.text, query.orientation, page)
            };
            let _ = tx.send((generation, answer));
        });
    }

    pub fn poll(&mut self) {
        while let Ok((generation, answer)) = self.rx.try_recv() {
            self.receive(generation, answer);
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
                self.has_more = page.next_page.is_some();
                self.photos.extend(page.photos);
                if self.photos.is_empty() {
                    self.message = "No photos found.".into();
                }
            }
            Err(error) => self.message = error.to_string(),
        }
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
        let json = format!(
            r#"{{"photos":[{}],"next_page":{}}}"#,
            ids.iter()
                .map(|id| format!(
                    r#"{{"id":{id},"width":1,"height":1,"url":"u","photographer":"p","photographer_url":"u",
                    "src":{{"original":"o","large2x":"l","medium":"m"}}}}"#
                ))
                .collect::<Vec<_>>()
                .join(","),
            if more { "\"n\"" } else { "null" },
        );
        Ok(serde_json::from_str(&json).unwrap())
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
        assert_eq!(b.message, "No photos found.");
        b.receive(0, Err(Error::RateLimited));
        assert!(b.message.contains("rate limit"));
    }
}
