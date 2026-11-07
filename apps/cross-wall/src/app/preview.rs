//! A full-window view of one item that steps through the shown items with the
//! arrow keys, loading the neighbours ahead so stepping is immediate.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cross_wall_sources::Progress;
use egui_animation::{animate_bool_eased, easing};

use super::motion::eased_since;
use super::shared::Shared;
use super::shown::Shown;
use super::widgets::progress_text;
use crate::gallery;
use crate::icons;
use crate::images::{self, Slot};

/// Items loaded ahead on each side of the open one.
const PRELOAD_RADIUS: usize = 3;

#[derive(Default)]
pub struct Preview {
    open: Option<Shown>,
    /// Download progress of each full-size image, by `Shown::key`.
    progress: HashMap<String, Arc<Progress>>,
    /// The direction and start time of the last step between items.
    step: (f32, f64),
    /// What the preview last drew, so it can fade out after closing.
    last: Option<(egui::TextureHandle, egui::Rect)>,
}

impl Preview {
    pub fn ui(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        if let Some(item) = shared.open.take() {
            self.open = Some(item);
        }
        let opened = animate_bool_eased(
            ctx,
            "preview-open",
            self.open.is_some(),
            easing::cubic_out,
            0.18,
        );
        let Some(item) = self.open.clone() else {
            self.fade_out(ctx, opened);
            return;
        };
        let key = item.key();
        let source = self.large_source(shared, &item);
        let progress = self.progress_for(&key);
        let screen = ctx.screen_rect();
        let position = shared.shown.iter().position(|s| s.key() == key);
        self.preload_around(shared, &key, position);
        let time = ctx.input(|i| i.time);
        let stepped = eased_since(self.step.1, time);
        let step_offset = self.step.0 * (1.0 - stepped) * 48.0;
        let mut step = ctx.input(|i| {
            i32::from(i.key_pressed(egui::Key::ArrowRight))
                - i32::from(i.key_pressed(egui::Key::ArrowLeft))
        });
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Area::new(egui::Id::new("preview"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // Takes the pointer, so nothing behind the preview reacts.
                ui.allocate_rect(screen, egui::Sense::click_and_drag());
                ui.painter().rect_filled(
                    screen,
                    0.0,
                    egui::Color32::from_black_alpha((242.0 * opened) as u8),
                );
                let stage = egui::Rect::from_min_max(
                    screen.min + egui::vec2(64.0, 20.0),
                    screen.max - egui::vec2(64.0, 76.0),
                );
                let large = match shared.images.get(&format!("large/{key}"), || source) {
                    Slot::Ready(texture) => Some(texture.clone()),
                    Slot::Failed(error) => {
                        let error = error.clone();
                        ui.put(
                            stage,
                            egui::Label::new(
                                egui::RichText::new(error).color(egui::Color32::WHITE),
                            ),
                        );
                        None
                    }
                    Slot::Loading => {
                        open_progress(ui, stage, progress.read());
                        None
                    }
                };
                if let Some(texture) = large {
                    let size = texture.size_vec2();
                    let fit = (stage.width() / size.x).min(stage.height() / size.y);
                    let grow = 0.97 + 0.03 * opened;
                    let centre = stage.center() + egui::vec2(step_offset, 0.0);
                    let rect = egui::Rect::from_center_size(centre, size * fit * grow);
                    self.last = Some((texture.clone(), rect));
                    let radius = ui.visuals().widgets.inactive.corner_radius;
                    let tint = egui::Color32::WHITE.gamma_multiply(opened * stepped);
                    egui::Image::new(&texture)
                        .tint(tint)
                        .corner_radius(radius)
                        .maintain_aspect_ratio(false)
                        .paint_at(ui, rect);
                }
                let visuals = ui.visuals_mut();
                visuals.override_text_color = Some(egui::Color32::WHITE);
                visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_white_alpha(30);
                visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_white_alpha(60);
                visuals.widgets.active.weak_bg_fill = egui::Color32::from_white_alpha(80);
                let round = |ui: &mut egui::Ui, centre: egui::Pos2, text: &str| {
                    let rect = egui::Rect::from_center_size(centre, egui::vec2(38.0, 38.0));
                    ui.put(
                        rect,
                        egui::Button::new(egui::RichText::new(text).size(18.0)).corner_radius(19),
                    )
                    .clicked()
                };
                close |= round(ui, screen.right_top() + egui::vec2(-32.0, 32.0), "\u{d7}");
                if position.is_some_and(|p| p > 0)
                    && round(
                        ui,
                        egui::pos2(screen.left() + 32.0, stage.center().y),
                        "\u{2039}",
                    )
                {
                    step = -1;
                }
                if position.is_some_and(|p| p + 1 < shared.shown.len())
                    && round(
                        ui,
                        egui::pos2(screen.right() - 32.0, stage.center().y),
                        "\u{203a}",
                    )
                {
                    step = 1;
                }
                let bar = egui::Rect::from_min_max(
                    egui::pos2(screen.left() + 24.0, screen.bottom() - 60.0),
                    screen.right_bottom() - egui::vec2(24.0, 12.0),
                );
                let layout = egui::Layout::left_to_right(egui::Align::Center);
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(layout), |ui| {
                    let credit = item.credit();
                    if credit.link.is_empty() {
                        ui.strong(credit.name);
                    } else {
                        ui.hyperlink_to(egui::RichText::new(credit.name).strong(), credit.link);
                    }
                    let (width, height) = item.size();
                    if width > 0 {
                        ui.label(format!("{width} \u{d7} {height}"));
                    }
                    if !credit.license.is_empty() {
                        ui.label(credit.license);
                    }
                    if !credit.page.is_empty() {
                        ui.hyperlink_to(
                            icons::with(icons::OPEN_LINK, "View at the source"),
                            credit.page,
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if shared.apply.is_busy() {
                            ui.spinner();
                        } else if icons::primary(ui, icons::APPLY, "Apply").clicked() {
                            shared.start_apply(&item);
                        }
                        shared.keep_buttons(ui, &item);
                    });
                });
            });
        let next = position
            .and_then(|p| p.checked_add_signed(step as isize))
            .filter(|_| step != 0)
            .and_then(|p| shared.shown.get(p).cloned());
        if next.is_some() {
            self.step = (step.signum() as f32, time);
            self.open = next;
        } else if close {
            self.open = None;
            shared.images.retain_large(&HashSet::new());
            self.progress.clear();
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

    /// Lets the last image and the backdrop fade away after closing.
    fn fade_out(&mut self, ctx: &egui::Context, opened: f32) {
        if opened <= 0.0 {
            self.last = None;
            return;
        }
        let Some((texture, rect)) = self.last.clone() else {
            return;
        };
        let area = egui::Area::new(egui::Id::new("preview-closing"))
            .order(egui::Order::Foreground)
            .interactable(false);
        area.fixed_pos(ctx.screen_rect().min).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            ui.painter().rect_filled(
                screen,
                0.0,
                egui::Color32::from_black_alpha((242.0 * opened) as u8),
            );
            let tint = egui::Color32::WHITE.gamma_multiply(opened);
            egui::Image::new(&texture)
                .tint(tint)
                .maintain_aspect_ratio(false)
                .paint_at(ui, rect);
        });
    }
}

/// What the preview shows while a big image opens: a bar with sizes while it
/// downloads, then a message while it is decoded, which has no measurable progress.
fn open_progress(ui: &mut egui::Ui, stage: egui::Rect, (done, total): (u64, Option<u64>)) {
    let white = egui::Color32::WHITE;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let below = stage.center() + egui::vec2(0.0, 34.0);
    let fetching = total.is_some_and(|t| done < t);
    if fetching {
        let fraction = done as f32 / total.unwrap_or(1).max(1) as f32;
        let width = stage.width().min(420.0);
        let track = egui::Rect::from_center_size(stage.center(), egui::vec2(width, 6.0));
        ui.painter()
            .rect_filled(track, 3.0, egui::Color32::from_white_alpha(50));
        let mut bar = track;
        bar.set_width(width * fraction);
        ui.painter()
            .rect_filled(bar, 3.0, ui.visuals().selection.bg_fill);
        ui.painter().text(
            below,
            egui::Align2::CENTER_TOP,
            progress_text((done, total), "Downloading"),
            font,
            white,
        );
    } else {
        ui.put(
            egui::Rect::from_center_size(stage.center(), egui::vec2(32.0, 32.0)),
            egui::Spinner::new().color(white).size(32.0),
        );
        let text = if done > 0 || total.is_some() {
            "Preparing image"
        } else {
            "Opening image"
        };
        ui.painter()
            .text(below, egui::Align2::CENTER_TOP, text, font, white);
    }
}
