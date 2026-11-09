//! A full-window view of one item that steps through the shown items with the
//! arrow keys, loading the neighbours ahead so stepping is immediate.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bones_kernel::messages::ui::kinds::{Hyperlink, IconButton, Label, Strong};
use bones_kernel::messages::ui::{icons, Event, Layout, Node, WidgetProps};
use cross_wall_sources::Progress;

use super::kit::Kit;
use super::shared::Shared;
use super::shown::Shown;
use super::tiles::progress_text;
use super::CrossWall;
use crate::gallery;
use crate::images::{self, Slot};

/// Items loaded ahead on each side of the open one.
const PRELOAD_RADIUS: usize = 3;

#[derive(Default)]
pub struct Preview {
    open: Option<Shown>,
    /// Download progress of each full-size image, by `Shown::key`.
    progress: HashMap<String, Arc<Progress>>,
}

impl Preview {
    pub fn open(&mut self, item: Shown) {
        self.open = Some(item);
    }

    fn close(&mut self, shared: &mut Shared) {
        self.open = None;
        shared.images.retain_large(&HashSet::new());
        self.progress.clear();
    }

    /// Moves to the item `step` places away in what the screen shows.
    fn step(&mut self, shared: &Shared, step: isize) {
        let Some(open) = &self.open else { return };
        let key = open.key();
        let next = shared
            .shown
            .iter()
            .position(|s| s.key() == key)
            .and_then(|p| p.checked_add_signed(step))
            .and_then(|p| shared.shown.get(p).cloned());
        if next.is_some() {
            self.open = next;
        }
    }

    /// Arrow keys step and Escape closes, while the preview is open.
    pub fn on_key(&mut self, shared: &mut Shared, key: &str) {
        if self.open.is_none() {
            return;
        }
        match key {
            "ArrowRight" => self.step(shared, 1),
            "ArrowLeft" => self.step(shared, -1),
            "Escape" => self.close(shared),
            _ => {}
        }
    }

    fn progress_for(&mut self, key: &str) -> Arc<Progress> {
        self.progress.entry(key.to_string()).or_default().clone()
    }

    /// How to fetch a full-size image: the kept file, or the source through the cache.
    fn large_source(&mut self, shared: &Shared, item: &Shown) -> images::Bytes {
        let progress = self.progress_for(&item.key());
        let local = shared.local_file(item);
        match item {
            Shown::Local(entry) => {
                let name = entry.name().to_string();
                Box::new(move || match local {
                    Some(file) => std::fs::read(file).map_err(|e| e.to_string()),
                    None => Err(format!("{name} is no longer in the wallpaper folder.")),
                })
            }
            Shown::Remote(photo) => {
                let client = shared.client.clone();
                let url = photo.src.preview_url().to_string();
                let path = shared.cache.join(format!("{}-large.jpg", photo.id));
                Box::new(move || match local {
                    Some(file) => std::fs::read(file).map_err(|e| e.to_string()),
                    None => images::cached(path, || {
                        client
                            .download_tracked(&url, &progress)
                            .map_err(|e| e.to_string())
                    }),
                })
            }
        }
    }

    /// Loads the items around the open one, so stepping to them is immediate,
    /// and frees full-size images that have left that range.
    fn preload_around(&mut self, shared: &mut Shared, key: &str, position: Option<usize>) {
        let around: Vec<Shown> = position
            .map(|p| gallery::neighbours(p, shared.shown.len(), PRELOAD_RADIUS))
            .unwrap_or_default()
            .into_iter()
            .map(|index| shared.shown[index].clone())
            .collect();
        let mut keep: HashSet<String> = around
            .iter()
            .map(|s| format!("large/{}", s.key()))
            .collect();
        keep.insert(format!("large/{key}"));
        shared.images.retain_large(&keep);
        self.progress
            .retain(|key, _| keep.contains(&format!("large/{key}")));
        // The queue runs newest first, so the farthest goes in first.
        for neighbour in around.iter().rev() {
            let slot = format!("large/{}", neighbour.key());
            if !shared.images.is_known(&slot) {
                let source = self.large_source(shared, neighbour);
                shared.images.preload(&slot, || source);
            }
        }
    }
}

impl CrossWall {
    /// The open item over the whole window, if one is open.
    pub fn preview_view(&mut self, kit: &mut Kit) -> Option<Node> {
        let item = self.preview.open.clone()?;
        let key = item.key();
        let position = self.shared.shown.iter().position(|s| s.key() == key);
        self.preview
            .preload_around(&mut self.shared, &key, position);
        let source = self.preview.large_source(&self.shared, &item);
        let progress = self.preview.progress_for(&key);
        let slot_key = format!("large/{key}");
        let (image, problem) = match self.shared.images.get(&slot_key, || source) {
            Slot::Ready { handle, .. } => (*handle, None),
            Slot::Failed(error) => (0, Some(error.clone())),
            Slot::Loading => (0, None),
        };

        let mut bar = Vec::new();
        let credit = item.credit();
        if credit.link.is_empty() {
            bar.push(
                Strong {
                    text: credit.name.to_string(),
                }
                .node(0),
            );
        } else {
            bar.push(
                Hyperlink {
                    label: credit.name.to_string(),
                    url: credit.link.to_string(),
                }
                .node(0),
            );
        }
        let (width, height) = item.size();
        if width > 0 {
            bar.push(
                Label {
                    text: format!("{width} \u{d7} {height}"),
                }
                .node(0),
            );
        }
        if !credit.license.is_empty() {
            bar.push(
                Label {
                    text: credit.license.to_string(),
                }
                .node(0),
            );
        }
        if !credit.page.is_empty() {
            bar.push(
                Hyperlink {
                    label: format!("{}  View at the source", icons::EXTERNAL_LINK),
                    url: credit.page.to_string(),
                }
                .node(0),
            );
        }
        if let Some(problem) = problem {
            bar.push(Label { text: problem }.node(0));
        } else if image == 0 {
            let (done, total) = progress.read();
            let text = if total.is_some_and(|t| done < t) {
                progress_text((done, total), "Downloading")
            } else if done > 0 || total.is_some() {
                "Preparing image".to_string()
            } else {
                "Opening image".to_string()
            };
            bar.push(Label { text }.node(0));
        }
        let mut trailing = Vec::new();
        if self.shared.apply.is_busy() {
            trailing.push(
                Label {
                    text: self.shared.apply.progress_text(),
                }
                .node(0),
            );
        } else {
            let applied = item.clone();
            let id = kit.on(move |app, _| app.shared.start_apply(&applied));
            trailing.push(IconButton::new("wallpaper", "Apply").primary(true).node(id));
        }
        trailing.extend(self.keep_buttons(kit, &item, false));
        bar.push(Node::container(Layout::RightToLeft, trailing));

        let count = self.shared.shown.len();
        let stage = kit.keyed("preview", |app, event| match event {
            Event::Clicked { .. } => app.preview.close(&mut app.shared),
            Event::Selected { index, .. } => {
                let step = if *index == 0 { -1 } else { 1 };
                app.preview.step(&app.shared, step);
            }
            _ => {}
        });
        Some(Node::Container {
            id: stage,
            layout: Layout::Stage {
                image,
                previous: position.is_some_and(|p| p > 0),
                next: position.is_some_and(|p| p + 1 < count),
            },
            children: bar,
        })
    }
}
