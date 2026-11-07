//! Rename, move, and delete for items in the wallpaper folder, from any screen
//! that shows them. Each action becomes a librarian command.
use cross_wall_library::{check_name, file_name, is_at_or_under, parent, Snapshot};

use crate::icons;
use crate::librarian::Command;

/// A small form over the window, for an action that needs an answer first.
pub enum Dialog {
    NewFolder { parent: String, name: String },
    Rename { path: String, name: String },
    Delete { path: String },
}

#[derive(Default)]
pub struct Organiser {
    dialog: Option<Dialog>,
}

impl Organiser {
    pub fn new_folder(&mut self, parent: String) {
        self.dialog = Some(Dialog::NewFolder {
            parent,
            name: String::new(),
        });
    }

    /// The context menu of one file or folder. Returns a move chosen in it.
    pub fn item_menu(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        is_folder: bool,
        folders: &[String],
    ) -> Option<Command> {
        let mut chosen = None;
        if icons::button(ui, icons::RENAME, "Rename").clicked() {
            self.dialog = Some(Dialog::Rename {
                path: path.to_string(),
                name: file_name(path).to_string(),
            });
            ui.close();
        }
        ui.menu_button(icons::with(icons::MOVE, "Move to"), |ui| {
            let here = parent(path);
            for target in std::iter::once("").chain(folders.iter().map(String::as_str)) {
                let blocked = target == here || (is_folder && is_at_or_under(target, path));
                let name = if target.is_empty() {
                    "Library (root)".to_string()
                } else {
                    target.replace('/', " › ")
                };
                if ui.add_enabled(!blocked, egui::Button::new(name)).clicked() {
                    chosen = Some(Command::Move {
                        path: path.to_string(),
                        folder: target.to_string(),
                    });
                    ui.close();
                }
            }
        });
        ui.separator();
        if icons::button(ui, icons::DELETE, "Delete").clicked() {
            self.dialog = Some(Dialog::Delete {
                path: path.to_string(),
            });
            ui.close();
        }
        chosen
    }

    /// The open dialog, if any. Returns the command once the user confirms.
    pub fn dialog_ui(&mut self, ctx: &egui::Context, snapshot: &Snapshot) -> Option<Command> {
        let mut dialog = self.dialog.take()?;
        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::new("library-dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            let (title, confirm) = match &dialog {
                Dialog::NewFolder { .. } => ("New folder".to_string(), "Create"),
                Dialog::Rename { .. } => ("Rename".to_string(), "Rename"),
                Dialog::Delete { path } => {
                    (format!("Delete {}?", file_name(path)), "Move to trash")
                }
            };
            ui.heading(title);
            ui.add_space(6.0);
            let mut entered = false;
            let problem = match &mut dialog {
                Dialog::NewFolder { name, .. } | Dialog::Rename { name, .. } => {
                    let edit =
                        ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    edit.request_focus();
                    entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    check_name(name).err()
                }
                Dialog::Delete { path } => {
                    let what = if snapshot.folders.contains(path) {
                        "This folder and everything in it go"
                    } else {
                        "It goes"
                    };
                    ui.label(format!(
                        "{what} to the system trash, where you can restore it."
                    ));
                    None
                }
            };
            if let Some(problem) = &problem {
                ui.weak(problem);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let clicked = ui
                    .add_enabled(problem.is_none(), egui::Button::new(confirm))
                    .clicked();
                confirmed = clicked || (entered && problem.is_none());
                cancelled = icons::button(ui, icons::CANCEL, "Cancel").clicked();
            });
        });
        if confirmed {
            return Some(match dialog {
                Dialog::NewFolder { parent, name } => Command::CreateFolder { parent, name },
                Dialog::Rename { path, name } => Command::Rename { path, name },
                Dialog::Delete { path } => Command::Delete { path },
            });
        }
        if !cancelled && !modal.should_close() {
            self.dialog = Some(dialog);
        }
        None
    }
}
