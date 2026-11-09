//! Gallery tiles: the photo, its like and download buttons, applying it, and
//! the thumbnail behind it, loaded when the tile first comes on screen.
use std::sync::Arc;

use bones_kernel::messages::ui::kinds::{Hyperlink, IconButton, Label, Progress, Spinner};
use bones_kernel::messages::ui::{icons, Event, Layout, Node, WidgetProps};

use super::kit::{stable_id, Kit};
use super::shared::{chosen_folder, Shared};
use super::shown::Shown;
use super::CrossWall;
use crate::images;
use crate::librarian::Command;
use crate::logs;

const ROW_HEIGHT: u16 = 210;
const TILE_GAP: u16 = 8;

/// "Saving 12.3 / 45.0 MB", or just the label until any bytes have arrived.
pub fn progress_text((done, total): (u64, Option<u64>), label: &str) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let done_mb = done as f64 / MB;
    match total {
        Some(total) if done < total => {
            format!("{label} {done_mb:.1} / {:.1} MB", total as f64 / MB)
        }
        _ if done > 0 => format!("{label} {done_mb:.1} MB"),
        _ => format!("{label}..."),
    }
}

impl CrossWall {
    /// The items as justified rows of tiles.
    pub fn gallery(&mut self, kit: &mut Kit, items: &[Shown]) -> Node {
        let tiles = items.iter().map(|item| self.tile(kit, item)).collect();
        Node::container(
            Layout::Gallery {
                row_height: ROW_HEIGHT,
                gap: TILE_GAP,
            },
            tiles,
        )
    }

    fn tile(&mut self, kit: &mut Kit, item: &Shown) -> Node {
        let key = item.key();
        let thumb = format!("thumb/{key}");
        // A tile seen before whose image was forgotten asks again.
        if self.shared.seen.contains(&key) && !self.shared.images.is_known(&thumb) {
            self.shared.request_thumbnail(item);
        }
        let image = self.shared.images.handle(&thumb).unwrap_or(0);
        let (width, height) = item.size();
        let (width, height) = fit_u16(width, height);
        let liked = self.shared.entry_for(item).is_some_and(|e| e.info.liked);
        let badge = if liked {
            icons::HEART.chars().next().map_or(0, u32::from)
        } else {
            0
        };
        let actions = self.tile_actions(kit, item);
        let shown = item.clone();
        let id = kit.keyed(("tile", &key), move |app, event| match event {
            Event::Appeared { .. } => {
                app.shared.seen.insert(shown.key());
                app.shared.request_thumbnail(&shown);
            }
            Event::Clicked { .. } => app.preview.open(shown),
            _ => {}
        });
        let tile = Node::Container {
            id,
            layout: Layout::Tile {
                image,
                width,
                height,
                badge,
            },
            children: actions,
        };
        let Some(path) = item.local_path() else {
            return tile;
        };
        let drag = stable_id(("drag", path));
        self.dragged.insert(drag, path.to_string());
        let dragged = Node::Container {
            id: drag,
            layout: Layout::Draggable,
            children: vec![tile],
        };
        let mut menu = vec![dragged];
        menu.extend(self.item_menu(kit, path, false));
        Node::Container {
            id: 0,
            layout: Layout::ContextMenu,
            children: menu,
        }
    }

    /// The buttons a tile shows while the pointer is over it, rightmost first.
    fn tile_actions(&mut self, kit: &mut Kit, item: &Shown) -> Vec<Node> {
        let mut nodes = Vec::new();
        if self.shared.apply.is_busy() {
            nodes.push(Spinner.node(0));
        } else {
            let shown = item.clone();
            let id = kit.on(move |app, _| app.shared.start_apply(&shown));
            nodes.push(IconButton::new("wallpaper", "Apply").node(id));
        }
        nodes.extend(self.keep_buttons(kit, item, false));
        let credit = item.credit();
        if !credit.link.is_empty() {
            nodes.push(
                Hyperlink {
                    label: credit.name.to_string(),
                    url: credit.link.to_string(),
                }
                .node(0),
            );
        }
        nodes
    }

    /// Like and Download for an item, or its progress while one is under way.
    /// `primary` makes Download the main action, as in the preview.
    pub fn keep_buttons(&mut self, kit: &mut Kit, item: &Shown, primary: bool) -> Vec<Node> {
        let key = item.key();
        if let Some(progress) = self.shared.pending.get(&key) {
            return vec![
                Label {
                    text: progress_text(progress.read(), "Saving"),
                }
                .node(0),
                Spinner.node(0),
            ];
        }
        let kept = self
            .shared
            .entry_for(item)
            .map(|e| (e.path.clone(), e.info.liked));
        let liked = kept.as_ref().is_some_and(|k| k.1);
        let mut nodes = Vec::new();
        let shown = item.clone();
        let like = kit.on(move |app, _| app.shared.toggle_like(&shown));
        let (icon, label) = if liked {
            ("heart-off", "Liked")
        } else {
            ("heart", "Like")
        };
        if kept.is_none() && matches!(item, Shown::Remote(_)) {
            let shown = item.clone();
            let download = kit.on(move |app, _| {
                if let Shown::Remote(photo) = &shown {
                    app.shared.pending.insert(shown.key(), Arc::default());
                    app.shared.start_download(photo, false);
                }
            });
            nodes.push(
                IconButton::new("download", "Download")
                    .primary(primary)
                    .node(download),
            );
        }
        if kept.is_some() || matches!(item, Shown::Remote(_)) {
            nodes.push(IconButton::new(icon, label).node(like));
        }
        nodes
    }

    /// The thin bar under a screen's header: images loading, or a search.
    pub fn progress_strip(&mut self, asking: bool) -> Option<Node> {
        let progress = self.shared.images.progress();
        let (fraction, text) = match progress {
            Some((done, total)) => (
                (done as f32 + 0.5) / total as f32,
                format!("Loading wallpapers {done} / {total}"),
            ),
            None if asking => (-1.0, String::new()),
            None => return None,
        };
        Some(Progress { fraction, text }.node(0))
    }
}

impl Shared {
    /// Starts loading an item's thumbnail, once.
    pub fn request_thumbnail(&mut self, item: &Shown) {
        let key = format!("thumb/{}", item.key());
        if self.images.is_known(&key) {
            return;
        }
        match item {
            Shown::Local(entry) => {
                let file = chosen_folder(self.settings())
                    .unwrap_or_default()
                    .join(&entry.path);
                let cache = self.cache.clone();
                self.images.get(&key, || {
                    Box::new(move || images::local_thumbnail(&file, &cache))
                });
            }
            Shown::Remote(photo) => {
                let (client, wanted) = (self.client.clone(), photo.clone());
                let path = self.cache.join(format!("{}-thumb.jpg", photo.id));
                self.images.get(&key, || {
                    Box::new(move || {
                        images::cached(path, || {
                            client
                                .download_thumbnail(&wanted)
                                .map_err(|e| e.to_string())
                        })
                    })
                });
            }
        }
    }

    /// Likes or unlikes an item; a remote photo not yet kept is downloaded liked.
    pub fn toggle_like(&mut self, item: &Shown) {
        let key = item.key();
        if self.pending.contains_key(&key) {
            return;
        }
        let kept = self.entry_for(item).map(|e| (e.path.clone(), e.info.liked));
        self.pending.insert(key.clone(), Arc::default());
        match (kept, item) {
            (Some((path, liked)), _) => self.command(Command::SetLiked {
                path,
                liked: !liked,
                key,
            }),
            (None, Shown::Remote(photo)) => {
                let photo = photo.clone();
                self.start_download(&photo, true);
            }
            (None, Shown::Local(_)) => {
                self.pending.remove(&key);
            }
        }
    }

    pub fn start_download(&mut self, photo: &cross_wall_sources::Photo, liked: bool) {
        logs::info("download", &format!("{} liked={liked}", photo.id));
        let key = Shown::Remote(photo.clone()).key();
        let progress = self.pending.get(&key).cloned().unwrap_or_default();
        self.command(Command::Add {
            client: self.client.clone(),
            photo: photo.clone(),
            liked,
            progress,
            key,
        });
    }
}

/// A photo's pixel size scaled to fit `u16`, keeping its shape; only the
/// shape matters to the gallery.
fn fit_u16(width: u32, height: u32) -> (u16, u16) {
    let largest = width.max(height);
    if largest <= u32::from(u16::MAX) {
        return (width as u16, height as u16);
    }
    let scale = f64::from(u16::MAX) / f64::from(largest);
    (
        (f64::from(width) * scale) as u16,
        (f64::from(height) * scale) as u16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_text_shows_what_is_known() {
        assert_eq!(progress_text((0, None), "Saving"), "Saving...");
        assert_eq!(
            progress_text((1024 * 1024, None), "Saving"),
            "Saving 1.0 MB"
        );
        assert_eq!(
            progress_text((1024 * 1024, Some(2 * 1024 * 1024)), "Saving"),
            "Saving 1.0 / 2.0 MB"
        );
    }

    #[test]
    fn huge_sizes_keep_their_shape() {
        assert_eq!(fit_u16(6000, 4000), (6000, 4000));
        let (w, h) = fit_u16(131070, 65535);
        assert_eq!((w, h), (65535, 32767));
    }
}
