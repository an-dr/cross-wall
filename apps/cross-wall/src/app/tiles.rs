//! The justified gallery every screen shows, and the actions on each tile.
use std::sync::Arc;

use egui_animation::{animate_bool_eased, easing};

use super::shared::{chosen_folder, Shared};
use super::shown::Shown;
use super::widgets::progress_text;
use crate::gallery;
use crate::icons;
use crate::images::{self, Slot};
use crate::librarian::Command;
use crate::logs;

const ROW_HEIGHT: f32 = 210.0;
const TILE_GAP: f32 = 8.0;

impl Shared {
    pub fn gallery_rows(&mut self, ui: &mut egui::Ui, items: &[Shown]) {
        let aspects: Vec<f32> = items
            .iter()
            .map(|item| {
                let (width, height) = item.size();
                gallery::aspect(width, height)
            })
            .collect();
        let width = ui.available_width();
        for row in gallery::rows(&aspects, width, ROW_HEIGHT, TILE_GAP) {
            let (row_rect, _) =
                ui.allocate_exact_size(egui::vec2(width, row.height), egui::Sense::hover());
            // Off-screen rows request nothing, so scrolling is what loads images.
            if ui.is_rect_visible(row_rect) {
                let mut x = row_rect.left();
                for (index, tile_width) in row.tiles {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, row_rect.top()),
                        egui::vec2(tile_width, row.height),
                    );
                    self.tile(ui, &items[index], rect);
                    x += tile_width + TILE_GAP;
                }
            }
            ui.add_space(TILE_GAP - ui.spacing().item_spacing.y);
        }
    }

    /// A thin bar under the toolbar. It slides while `asking` and fills as
    /// thumbnails arrive. It always keeps its height, so the grid never jumps.
    pub fn progress_strip(&mut self, ui: &mut egui::Ui, asking: bool) {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), egui::Sense::hover());
        let progress = self.images.progress();
        let accent = ui.visuals().selection.bg_fill;
        let track = ui.visuals().faint_bg_color;
        let time = ui.input(|i| i.time) as f32;
        let segment = if let Some((done, total)) = progress {
            Some((0.0, (done as f32 + 0.5) / total as f32))
        } else if asking {
            let start = (time * 0.7).fract() * 1.25 - 0.25;
            Some((start.max(0.0), (start + 0.25).min(1.0)))
        } else {
            None
        };
        let Some((from, to)) = segment else { return };
        let radius = egui::CornerRadius::same(2);
        ui.painter().rect_filled(rect, radius, track);
        let bar = egui::Rect::from_x_y_ranges(
            rect.left() + rect.width() * from..=rect.left() + rect.width() * to,
            rect.y_range(),
        );
        ui.painter().rect_filled(bar, radius, accent);
        if let Some((done, total)) = progress {
            let text = format!("Loading wallpapers {done} / {total}");
            let font = egui::TextStyle::Small.resolve(ui.style());
            let colour = ui.visuals().weak_text_color();
            let at = rect.right_bottom() + egui::vec2(0.0, 3.0);
            ui.painter()
                .text(at, egui::Align2::RIGHT_TOP, text, font, colour);
        }
    }

    /// Like and Download for an item, or its progress while one is under way.
    pub fn keep_buttons(&mut self, ui: &mut egui::Ui, item: &Shown) {
        let key = item.key();
        if let Some(progress) = self.pending.get(&key) {
            ui.spinner();
            ui.label(progress_text(progress.read(), "Saving"));
            return;
        }
        let kept = self.entry_for(item).map(|e| (e.path.clone(), e.info.liked));
        let Shown::Remote(photo) = item else {
            // A local file that is gone has nothing left to like.
            let Some((path, liked)) = kept else { return };
            if like_button(ui, liked) {
                self.pending.insert(key.clone(), Arc::default());
                self.command(Command::SetLiked {
                    path,
                    liked: !liked,
                    key,
                });
            }
            return;
        };
        let liked = kept.as_ref().is_some_and(|k| k.1);
        if like_button(ui, liked) {
            self.pending.insert(key.clone(), Arc::default());
            match kept.clone() {
                Some((path, liked)) => self.command(Command::SetLiked {
                    path,
                    liked: !liked,
                    key: key.clone(),
                }),
                None => self.start_download(photo, true),
            }
        }
        if kept.is_none() && icons::primary(ui, icons::DOWNLOAD, "Download").clicked() {
            self.pending.insert(key, Arc::default());
            self.start_download(photo, false);
        }
    }

    fn start_download(&mut self, photo: &cross_wall_sources::Photo, liked: bool) {
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

    fn thumb_slot(&mut self, item: &Shown) -> &Slot {
        let key = format!("thumb/{}", item.key());
        match item {
            // Files in the folder need no network, so the library works offline.
            Shown::Local(entry) => {
                let file = chosen_folder(self.settings())
                    .unwrap_or_default()
                    .join(&entry.path);
                let cache = self.cache.clone();
                self.images.get(&key, || {
                    Box::new(move || images::local_thumbnail(&file, &cache))
                })
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
                })
            }
        }
    }

    fn tile(&mut self, ui: &mut egui::Ui, item: &Shown, rect: egui::Rect) {
        let id = ui.id().with(("tile", item.key()));
        let local_path = item.local_path().map(String::from);
        let sense = if local_path.is_some() {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click()
        };
        let response = ui
            .interact(rect, id, sense)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if let Some(path) = &local_path {
            response.dnd_set_drag_payload(path.clone());
            let mut chosen = None;
            response.context_menu(|ui| {
                chosen = self
                    .organiser
                    .item_menu(ui, path, false, &self.snapshot.folders)
            });
            if let Some(command) = chosen {
                self.command(command);
            }
        }
        let radius = ui.visuals().widgets.inactive.corner_radius;
        let time = ui.input(|i| i.time) as f32;
        let texture = match self.thumb_slot(item) {
            Slot::Ready(texture) => Some(texture.clone()),
            _ => None,
        };
        match texture {
            Some(texture) => {
                let uv = images::cover_uv(texture.size(), rect.size());
                let appear =
                    animate_bool_eased(ui.ctx(), id.with("appear"), true, easing::cubic_out, 0.25);
                ui.painter()
                    .rect_filled(rect, radius, ui.visuals().faint_bg_color);
                let tint = egui::Color32::WHITE.gamma_multiply(appear);
                egui::Image::new(&texture)
                    .uv(uv)
                    .tint(tint)
                    .corner_radius(radius)
                    .maintain_aspect_ratio(false)
                    .paint_at(ui, rect);
            }
            None => {
                // A soft pulse while the photo loads, rather than a row of spinners.
                let base = ui.visuals().faint_bg_color;
                let pulse = 0.65 + 0.35 * (time * 2.5).sin().abs();
                ui.painter()
                    .rect_filled(rect, radius, base.gamma_multiply(pulse));
            }
        }
        let hovered = ui.rect_contains_pointer(rect);
        let shown =
            animate_bool_eased(ui.ctx(), id.with("hover"), hovered, easing::cubic_out, 0.16);
        if self.entry_for(item).is_some_and(|e| e.info.liked) && shown < 0.5 {
            let badge = rect.right_top() + egui::vec2(-16.0, 16.0);
            ui.painter()
                .circle_filled(badge, 11.0, egui::Color32::from_black_alpha(140));
            ui.painter().text(
                badge,
                egui::Align2::CENTER_CENTER,
                "\u{2665}",
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
        }
        if shown > 0.0 {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - 40.0),
                rect.right_bottom(),
            );
            let bar_radius = egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: radius.sw,
                se: radius.se,
            };
            ui.painter().rect_filled(
                bar,
                bar_radius,
                egui::Color32::from_black_alpha((170.0 * shown) as u8),
            );
        }
        if shown > 0.5 {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 8.0, rect.bottom() - 40.0),
                rect.right_bottom() - egui::vec2(8.0, 0.0),
            );
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.scope_builder(
                egui::UiBuilder::new()
                    .id_salt(("tile-bar", item.key()))
                    .max_rect(bar)
                    .layout(layout),
                |ui| {
                    ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                    ui.visuals_mut().widgets.inactive.weak_bg_fill =
                        egui::Color32::from_white_alpha(40);
                    ui.visuals_mut().widgets.hovered.weak_bg_fill =
                        egui::Color32::from_white_alpha(70);
                    let credit = item.credit();
                    if rect.width() > 330.0 && !credit.link.is_empty() {
                        let name = egui::RichText::new(credit.name)
                            .small()
                            .color(egui::Color32::WHITE);
                        ui.add(
                            egui::Hyperlink::from_label_and_url(name, credit.link)
                                .open_in_new_tab(true),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.apply.is_busy() {
                            ui.spinner();
                        } else if icons::button(ui, icons::APPLY, "Apply").clicked() {
                            self.start_apply(item);
                        }
                        self.keep_buttons(ui, item);
                    });
                },
            );
        }
        if response.clicked() {
            self.open = Some(item.clone());
        }
    }
}

/// The heart button. Says whether it was clicked.
fn like_button(ui: &mut egui::Ui, liked: bool) -> bool {
    let (heart, label) = if liked {
        (icons::LIKED, "Liked")
    } else {
        (icons::LIKE, "Like")
    };
    icons::button(ui, heart, label).clicked()
}
