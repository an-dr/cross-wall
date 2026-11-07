//! The Browse screen: search a source, filter the results, and scroll for more.
use cross_wall_sources::{MinSize, Orientation, Source};

use super::shared::Shared;
use super::shown::Shown;
use super::widgets::{centered_button, empty_state};
use crate::browse::Browse;
use crate::icons;
use crate::settings::Settings;

const AUTO_LOAD_MARGIN: f32 = 500.0;
const RESOLUTIONS: [(u32, u32); 5] = [
    (1280, 720),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
    (5120, 2880),
];

pub struct BrowseView {
    browse: Browse,
}

impl BrowseView {
    pub fn new() -> Self {
        Self {
            browse: Browse::new(),
        }
    }

    pub fn poll(&mut self, shared: &Shared) {
        self.browse.poll(&shared.client);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared, display_size: Option<(u32, u32)>) {
        self.browse.min = effective_min(shared.settings(), display_size);
        self.browse.start_once(&shared.client);
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            let searchable = self.browse.source.has_search();
            let hint = if searchable {
                "Search wallpapers"
            } else {
                "This source has no search"
            };
            let edit = egui::TextEdit::singleline(&mut self.browse.text)
                .hint_text(hint)
                .margin(egui::Margin::symmetric(10, 5))
                .desired_width((ui.available_width() - 450.0).max(130.0));
            let edit = ui.add_enabled(searchable, edit);
            refresh |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            egui::ComboBox::from_id_salt("source")
                .selected_text(self.browse.source.name())
                .show_ui(ui, |ui| {
                    for source in Source::ALL {
                        refresh |= ui
                            .selectable_value(&mut self.browse.source, source, source.name())
                            .changed();
                    }
                });
            egui::ComboBox::from_id_salt("orientation")
                .selected_text(orientation_name(self.browse.orientation))
                .show_ui(ui, |ui| {
                    for o in [
                        Orientation::Any,
                        Orientation::Landscape,
                        Orientation::Portrait,
                        Orientation::Square,
                    ] {
                        refresh |= ui
                            .selectable_value(&mut self.browse.orientation, o, orientation_name(o))
                            .changed();
                    }
                });
            if resolution_menu(ui, shared, display_size) {
                self.browse.min = effective_min(shared.settings(), display_size);
                refresh = true;
            }
        });
        ui.add_space(6.0);
        if refresh {
            shared.images.forget("thumb/remote:");
            self.browse.search(&shared.client);
        }
        shared.progress_strip(ui, self.browse.loading);
        if self.browse.photos.is_empty() && self.browse.loading {
            empty_state(
                ui,
                icons::BROWSE,
                "Finding wallpapers",
                "Asking the source for photos...",
            );
            return;
        }
        if self.browse.photos.is_empty() && !self.browse.message.is_empty() {
            let message = self.browse.message.clone();
            empty_state(ui, icons::BROWSE, "No wallpapers to show", &message);
            if self.browse.has_more && centered_button(ui, icons::RETRY, "Try again") {
                self.browse.load_more(&shared.client);
            }
            return;
        }
        let items: Vec<Shown> = self
            .browse
            .photos
            .iter()
            .cloned()
            .map(Shown::Remote)
            .collect();
        let scroll = egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                shared.shown = items.clone();
                shared.gallery_rows(ui, &items);
                if !self.browse.message.is_empty() {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| ui.label(self.browse.message.clone()));
                }
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    if self.browse.loading {
                        ui.spinner();
                    } else if self.browse.is_stalled()
                        && icons::button(ui, icons::RETRY, "Try again").clicked()
                    {
                        self.browse.load_more(&shared.client);
                    }
                    ui.add_space(4.0);
                    let source = self.browse.source;
                    ui.hyperlink_to(
                        egui::RichText::new(format!("Photos provided by {}", source.name()))
                            .small(),
                        source.home(),
                    );
                    ui.add_space(8.0);
                });
            });
        // Starts before the very end, so scrolling rarely reaches a gap.
        let bottom = scroll.state.offset.y + scroll.inner_rect.height();
        if bottom + AUTO_LOAD_MARGIN >= scroll.content_size.y && self.browse.can_auto_load() {
            self.browse.load_more(&shared.client);
        }
    }
}

/// The minimum size asked for: the chosen one, or the main display's.
fn effective_min(settings: &Settings, display_size: Option<(u32, u32)>) -> MinSize {
    let (width, height) = match settings.min_resolution {
        Some([width, height]) => (width, height),
        None => display_size.unwrap_or((0, 0)),
    };
    MinSize { width, height }
}

fn resolution_label(min_resolution: Option<[u32; 2]>, display_size: Option<(u32, u32)>) -> String {
    match (min_resolution, display_size) {
        (None, Some((w, h))) => format!("\u{2265} {w} \u{d7} {h}"),
        (None, None) | (Some([0, 0]), _) => "Any size".to_string(),
        (Some([w, h]), _) => format!("\u{2265} {w} \u{d7} {h}"),
    }
}

/// The minimum-resolution menu. Saves the choice and says whether it changed.
fn resolution_menu(
    ui: &mut egui::Ui,
    shared: &mut Shared,
    display_size: Option<(u32, u32)>,
) -> bool {
    let mut chosen = shared.settings().min_resolution;
    let display = display_size.map_or("Main display".to_string(), |(w, h)| {
        format!("Main display ({w} \u{d7} {h})")
    });
    let label = resolution_label(chosen, display_size);
    egui::ComboBox::from_id_salt("min-resolution")
        .selected_text(label)
        .width(150.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, display);
            ui.selectable_value(&mut chosen, Some([0, 0]), "Any size");
            for (w, h) in RESOLUTIONS {
                ui.selectable_value(&mut chosen, Some([w, h]), format!("{w} \u{d7} {h}"));
            }
        });
    shared.store.change(|s| s.min_resolution = chosen)
}

fn orientation_name(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Any => "Any shape",
        Orientation::Landscape => "Landscape",
        Orientation::Portrait => "Portrait",
        Orientation::Square => "Square",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minimum_follows_the_display_unless_chosen() {
        let mut settings = Settings::default();
        assert_eq!(
            effective_min(&settings, Some((2560, 1440))),
            MinSize {
                width: 2560,
                height: 1440
            }
        );
        assert_eq!(effective_min(&settings, None), MinSize::ANY);
        settings.min_resolution = Some([1920, 1080]);
        assert_eq!(
            effective_min(&settings, Some((2560, 1440))),
            MinSize {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(resolution_label(Some([0, 0]), Some((1, 1))), "Any size");
    }
}
