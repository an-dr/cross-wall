//! The Library screen: folders, groups, and organising files. All changes go
//! through the librarian thread and come back as the next snapshot.
use cross_wall_library::{check_name, file_name, join, parent, Entry};
use cross_wall_sources::{Photo, Sources};

use super::{primary_button, CrossWall};
use crate::librarian::Command;

/// A small form over the window, for an action that needs an answer first.
pub(super) enum Dialog {
    NewFolder { parent: String, name: String },
    Rename { path: String, name: String },
    Delete { path: String },
}

/// A kept file, shaped so the gallery and preview treat every photo alike.
pub(super) fn entry_photo(entry: &Entry) -> Photo {
    let info = entry.info.photo.as_ref();
    let text = |value: fn(&Photo) -> &String| info.map(value).cloned().unwrap_or_default();
    Photo {
        id: format!("file:{}", entry.path),
        width: entry.width,
        height: entry.height,
        url: text(|p| &p.url),
        photographer: info.map_or_else(|| entry.name().to_string(), |p| p.photographer.clone()),
        photographer_url: text(|p| &p.photographer_url),
        license: text(|p| &p.license),
        src: Sources { original: String::new(), large2x: String::new(), medium: String::new() },
    }
}

fn is_inside(folder: &str, path: &str) -> bool {
    folder == path || folder.starts_with(&format!("{path}/"))
}

impl CrossWall {
    pub(super) fn library_ui(&mut self, ui: &mut egui::Ui) {
        if self.settings.wallpaper_folder.trim().is_empty() {
            self.choose_folder_state(ui);
            return;
        }
        if !self.snapshot.folders.contains(&self.current_folder) {
            self.current_folder.clear();
        }
        ui.horizontal(|ui| {
            self.breadcrumb(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut groups = self.settings.library_groups;
                ui.selectable_value(&mut groups, true, "Groups");
                ui.selectable_value(&mut groups, false, "Folders");
                if groups != self.settings.library_groups {
                    self.settings.library_groups = groups;
                    self.draft.library_groups = groups;
                    self.save_quietly();
                }
                if ui.button("New folder").clicked() {
                    let parent = if groups { String::new() } else { self.current_folder.clone() };
                    self.dialog = Some(Dialog::NewFolder { parent, name: String::new() });
                }
            });
        });
        ui.add_space(6.0);
        self.progress_strip(ui);
        ui.add_space(6.0);
        if self.snapshot.files.is_empty() && self.snapshot.folders.is_empty() {
            super::empty_state(ui, "🖼", "This folder is empty", "Download wallpapers from Browse, or add images with your file manager.");
            return;
        }
        let view = format!("{}/{}", self.settings.library_groups, self.current_folder);
        let appear = self.transition(ui.ctx(), "library-view", view);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.set_opacity(appear);
            ui.add_space((1.0 - appear) * 8.0);
            if self.settings.library_groups {
                self.groups_view(ui);
            } else {
                self.folder_view(ui);
            }
            ui.add_space(8.0);
        });
    }

    fn choose_folder_state(&mut self, ui: &mut egui::Ui) {
        super::empty_state(ui, "🖼", "Choose your wallpaper folder", "Downloads go to its root, and you can organise it from here.");
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            if primary_button(ui, "Choose folder").clicked() {
                self.pick_folder();
            }
        });
    }

    /// The path to the open folder. Each part opens that folder and accepts a dropped item.
    fn breadcrumb(&mut self, ui: &mut egui::Ui) {
        if self.settings.library_groups {
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
                ui.weak("›");
            }
            let response = ui.selectable_label(*path == current, name);
            if response.clicked() {
                self.current_folder = path.clone();
            }
            self.accept_drop(&response, path);
        }
    }

    /// Moves an item dropped on this folder's widget into the folder.
    fn accept_drop(&mut self, response: &egui::Response, folder: &str) {
        if let Some(dragged) = response.dnd_release_payload::<String>() {
            if parent(&dragged) != folder {
                self.command(Command::Move { path: (*dragged).clone(), folder: folder.to_string() });
            }
        }
    }

    pub(super) fn command(&self, command: Command) {
        let _ = self.librarian.commands.send(command);
    }

    fn folder_view(&mut self, ui: &mut egui::Ui) {
        let current = self.current_folder.clone();
        let folders: Vec<String> = self.snapshot.folders.iter().filter(|f| parent(f) == current && **f != current).cloned().collect();
        if !folders.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for folder in &folders {
                    self.folder_chip(ui, folder);
                }
            });
            ui.add_space(10.0);
        }
        let photos: Vec<Photo> = self.snapshot.files.iter().filter(|e| e.folder() == current).map(entry_photo).collect();
        self.shown = photos.clone();
        self.gallery_rows(ui, &photos);
    }

    /// Every folder that holds images as one titled section, the root first.
    fn groups_view(&mut self, ui: &mut egui::Ui) {
        let mut all = Vec::new();
        for folder in std::iter::once(String::new()).chain(self.snapshot.folders.clone()) {
            let photos: Vec<Photo> = self.snapshot.files.iter().filter(|e| e.folder() == folder).map(entry_photo).collect();
            let response = ui.horizontal(|ui| {
                let title = if folder.is_empty() { "Library".to_string() } else { folder.replace('/', " › ") };
                let heading = ui.add(egui::Label::new(egui::RichText::new(title).strong().size(16.0)).sense(egui::Sense::click()));
                ui.weak(format!("{}", photos.len()));
                heading
            });
            let heading = response.inner.on_hover_text("Open this folder");
            if heading.clicked() {
                self.current_folder = folder.clone();
                self.settings.library_groups = false;
                self.draft.library_groups = false;
                self.save_quietly();
            }
            self.accept_drop(&heading, &folder);
            if !folder.is_empty() {
                let path = folder.clone();
                heading.context_menu(|ui| self.item_menu(ui, &path, true));
            }
            ui.add_space(4.0);
            self.gallery_rows(ui, &photos);
            ui.add_space(12.0);
            all.extend(photos);
        }
        self.shown = all;
    }

    fn folder_chip(&mut self, ui: &mut egui::Ui, folder: &str) {
        let count = self.snapshot.files.iter().filter(|e| e.folder() == folder).count();
        let text = format!("🗀  {}  ·  {count}", file_name(folder));
        let button = egui::Button::new(text).min_size(egui::vec2(150.0, 44.0)).sense(egui::Sense::click());
        let response = ui.add(button);
        if response.clicked() {
            self.current_folder = folder.to_string();
        }
        self.accept_drop(&response, folder);
        let dragging = response.dnd_hover_payload::<String>().is_some();
        if dragging {
            let stroke = egui::Stroke::new(2.0, ui.visuals().selection.bg_fill);
            ui.painter().rect_stroke(response.rect, 4.0, stroke, egui::StrokeKind::Inside);
        }
        response.context_menu(|ui| self.item_menu(ui, folder, true));
    }

    /// Rename, move, and delete for one file or folder.
    pub(super) fn item_menu(&mut self, ui: &mut egui::Ui, path: &str, is_folder: bool) {
        if ui.button("Rename").clicked() {
            self.dialog = Some(Dialog::Rename { path: path.to_string(), name: file_name(path).to_string() });
            ui.close();
        }
        ui.menu_button("Move to", |ui| {
            let here = parent(path).to_string();
            let targets: Vec<String> = std::iter::once(String::new()).chain(self.snapshot.folders.clone()).collect();
            for target in targets {
                let blocked = target == here || (is_folder && is_inside(&target, path));
                let name = if target.is_empty() { "Library (root)".to_string() } else { target.replace('/', " › ") };
                if ui.add_enabled(!blocked, egui::Button::new(name)).clicked() {
                    self.command(Command::Move { path: path.to_string(), folder: target });
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button("Delete").clicked() {
            self.dialog = Some(Dialog::Delete { path: path.to_string() });
            ui.close();
        }
    }

    pub(super) fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::new("library-dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            let (title, confirm) = match &dialog {
                Dialog::NewFolder { .. } => ("New folder".to_string(), "Create"),
                Dialog::Rename { .. } => ("Rename".to_string(), "Rename"),
                Dialog::Delete { path } => (format!("Delete {}?", file_name(path)), "Move to trash"),
            };
            ui.heading(title);
            ui.add_space(6.0);
            let mut entered = false;
            let problem = match &mut dialog {
                Dialog::NewFolder { name, .. } | Dialog::Rename { name, .. } => {
                    let edit = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    edit.request_focus();
                    entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    check_name(name).err()
                }
                Dialog::Delete { path } => {
                    let what = if self.snapshot.folders.contains(path) { "This folder and everything in it go" } else { "It goes" };
                    ui.label(format!("{what} to the system trash, where you can restore it."));
                    None
                }
            };
            if let Some(problem) = &problem {
                ui.weak(problem);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(problem.is_none(), egui::Button::new(confirm)).clicked() || (entered && problem.is_none()) {
                    self.finish_dialog(&dialog);
                    done = true;
                }
                done |= ui.button("Cancel").clicked();
            });
        });
        if !done && !modal.should_close() {
            self.dialog = Some(dialog);
        }
    }

    fn finish_dialog(&mut self, dialog: &Dialog) {
        match dialog {
            Dialog::NewFolder { parent, name } => {
                self.command(Command::CreateFolder { parent: parent.clone(), name: name.clone() });
            }
            Dialog::Rename { path, name } => {
                self.command(Command::Rename { path: path.clone(), name: name.clone() });
            }
            Dialog::Delete { path } => self.command(Command::Delete { path: path.clone() }),
        }
    }
}
