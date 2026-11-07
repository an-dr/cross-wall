//! The Library and Liked screens: the wallpaper folder by folder or in groups,
//! and the liked files. All changes go through the librarian thread and come
//! back as the next snapshot.
use cross_wall_library::{file_name, join, parent};

use super::shared::Shared;
use super::shown::Shown;
use super::widgets::empty_state;
use crate::icons;
use crate::librarian::Command;

#[derive(Default)]
pub struct LibraryView {
    /// The open folder in the folder view; the root is the empty string.
    current_folder: String,
}

impl LibraryView {
    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        if shared.settings().wallpaper_folder.trim().is_empty() {
            choose_folder_state(ui, shared);
            return;
        }
        if !shared.snapshot.folders.contains(&self.current_folder) {
            self.current_folder.clear();
        }
        let groups = shared.settings().library_groups;
        ui.horizontal(|ui| {
            self.breadcrumb(ui, shared);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut chosen = groups;
                ui.selectable_value(&mut chosen, true, icons::with(icons::GROUPS_VIEW, "Groups"));
                ui.selectable_value(
                    &mut chosen,
                    false,
                    icons::with(icons::FOLDERS_VIEW, "Folders"),
                );
                shared.store.change(|s| s.library_groups = chosen);
                if icons::button(ui, icons::NEW_FOLDER, "New folder").clicked() {
                    shared.organiser.new_folder(if chosen {
                        String::new()
                    } else {
                        self.current_folder.clone()
                    });
                }
            });
        });
        ui.add_space(6.0);
        shared.progress_strip(ui, false);
        ui.add_space(6.0);
        if shared.snapshot.files.is_empty() && shared.snapshot.folders.is_empty() {
            empty_state(
                ui,
                icons::BROWSE,
                "This folder is empty",
                "Download wallpapers from Browse, or add images with your file manager.",
            );
            return;
        }
        let groups = shared.settings().library_groups;
        let view = format!("{groups}/{}", self.current_folder);
        let appear = shared.motion.transition(ui.ctx(), "library-view", view);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.set_opacity(appear);
                ui.add_space((1.0 - appear) * 8.0);
                if groups {
                    self.groups_view(ui, shared);
                } else {
                    self.folder_view(ui, shared);
                }
                ui.add_space(8.0);
            });
    }

    /// The path to the open folder. Each part opens that folder and accepts a dropped item.
    fn breadcrumb(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        if shared.settings().library_groups {
            ui.heading("Library");
            return;
        }
        let current = self.current_folder.clone();
        let mut parts = vec![(String::new(), "Library".to_string())];
        let mut built = String::new();
        for part in current.split('/').filter(|p| !p.is_empty()) {
            built = join(&built, part);
            parts.push((built.clone(), part.to_string()));
        }
        for (index, (path, name)) in parts.iter().enumerate() {
            if index > 0 {
                ui.weak("\u{203a}");
            }
            let response = ui.selectable_label(*path == current, name);
            if response.clicked() {
                self.current_folder = path.clone();
            }
            accept_drop(shared, &response, path);
        }
    }

    fn folder_view(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let current = self.current_folder.clone();
        let folders: Vec<String> = shared
            .snapshot
            .folders
            .iter()
            .filter(|f| parent(f) == current && **f != current)
            .cloned()
            .collect();
        if !folders.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for folder in &folders {
                    self.folder_chip(ui, shared, folder);
                }
            });
            ui.add_space(10.0);
        }
        let items = files_in(shared, &current);
        shared.shown = items.clone();
        shared.gallery_rows(ui, &items);
    }

    /// Every folder that holds images as one titled section, the root first.
    fn groups_view(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        let mut all = Vec::new();
        for folder in std::iter::once(String::new()).chain(shared.snapshot.folders.clone()) {
            let items = files_in(shared, &folder);
            let response = ui.horizontal(|ui| {
                let title = if folder.is_empty() {
                    "Library".to_string()
                } else {
                    folder.replace('/', " \u{203a} ")
                };
                let heading = ui.add(
                    egui::Label::new(egui::RichText::new(title).strong().size(16.0))
                        .sense(egui::Sense::click()),
                );
                ui.weak(format!("{}", items.len()));
                heading
            });
            let heading = response.inner.on_hover_text("Open this folder");
            if heading.clicked() {
                self.current_folder = folder.clone();
                shared.store.change(|s| s.library_groups = false);
            }
            accept_drop(shared, &heading, &folder);
            if !folder.is_empty() {
                let mut chosen = None;
                heading.context_menu(|ui| {
                    chosen = shared
                        .organiser
                        .item_menu(ui, &folder, true, &shared.snapshot.folders)
                });
                if let Some(command) = chosen {
                    shared.command(command);
                }
            }
            ui.add_space(4.0);
            shared.gallery_rows(ui, &items);
            ui.add_space(12.0);
            all.extend(items);
        }
        shared.shown = all;
    }

    fn folder_chip(&mut self, ui: &mut egui::Ui, shared: &mut Shared, folder: &str) {
        let count = shared
            .snapshot
            .files
            .iter()
            .filter(|e| e.folder() == folder)
            .count();
        let text = icons::with(
            icons::LIBRARY,
            &format!("{}  \u{b7}  {count}", file_name(folder)),
        );
        let button = egui::Button::new(text)
            .min_size(egui::vec2(150.0, 44.0))
            .sense(egui::Sense::click());
        let response = ui.add(button);
        if response.clicked() {
            self.current_folder = folder.to_string();
        }
        accept_drop(shared, &response, folder);
        if response.dnd_hover_payload::<String>().is_some() {
            let stroke = egui::Stroke::new(2.0_f32, ui.visuals().selection.bg_fill);
            ui.painter()
                .rect_stroke(response.rect, 4.0, stroke, egui::StrokeKind::Inside);
        }
        let mut chosen = None;
        response.context_menu(|ui| {
            chosen = shared
                .organiser
                .item_menu(ui, folder, true, &shared.snapshot.folders)
        });
        if let Some(command) = chosen {
            shared.command(command);
        }
    }
}

/// The Liked screen: every liked file, wherever it is in the folder.
pub fn liked_ui(ui: &mut egui::Ui, shared: &mut Shared) {
    let items: Vec<Shown> = shared
        .snapshot
        .files
        .iter()
        .filter(|e| e.info.liked)
        .cloned()
        .map(Shown::Local)
        .collect();
    if items.is_empty() {
        empty_state(
            ui,
            icons::LIKED,
            "Nothing liked yet",
            "Like a wallpaper in Browse or the Library to see it here.",
        );
        return;
    }
    shared.shown = items.clone();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| shared.gallery_rows(ui, &items));
}

fn files_in(shared: &Shared, folder: &str) -> Vec<Shown> {
    shared
        .snapshot
        .files
        .iter()
        .filter(|e| e.folder() == folder)
        .cloned()
        .map(Shown::Local)
        .collect()
}

fn choose_folder_state(ui: &mut egui::Ui, shared: &Shared) {
    empty_state(
        ui,
        icons::BROWSE,
        "Choose your wallpaper folder",
        "Downloads go to its root, and you can organise it from here.",
    );
    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        if icons::primary(ui, icons::CHOOSE, "Choose folder").clicked() {
            shared.picker.open();
        }
    });
}

/// Moves an item dropped on this folder's widget into the folder.
fn accept_drop(shared: &Shared, response: &egui::Response, folder: &str) {
    if let Some(dragged) = response.dnd_release_payload::<String>() {
        if parent(&dragged) != folder {
            shared.command(Command::Move {
                path: (*dragged).clone(),
                folder: folder.to_string(),
            });
        }
    }
}
