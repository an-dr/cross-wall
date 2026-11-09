//! Organising the wallpaper folder: the menu on a file or folder, and the
//! dialogs for a new folder, a new name, or moving to the trash.
use bones_kernel::messages::ui::kinds::{Button, IconButton, Label, Separator, TextEdit, Weak};
use bones_kernel::messages::ui::{Event, Layout, Node, Root, Spec, WidgetProps};
use cross_wall_library::{check_name, file_name, is_at_or_under, parent};

use super::kit::{stable_id, Kit};
use super::CrossWall;
use crate::librarian::Command;

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

    /// The command the open dialog confirms, if its input is acceptable.
    fn confirmed(&self) -> Option<Command> {
        match self.dialog.as_ref()? {
            Dialog::NewFolder { parent, name } => {
                check_name(name).ok()?;
                Some(Command::CreateFolder {
                    parent: parent.clone(),
                    name: name.clone(),
                })
            }
            Dialog::Rename { path, name } => {
                check_name(name).ok()?;
                Some(Command::Rename {
                    path: path.clone(),
                    name: name.clone(),
                })
            }
            Dialog::Delete { path } => Some(Command::Delete { path: path.clone() }),
        }
    }
}

impl CrossWall {
    /// The menu on a file or folder at `path`: rename, move, delete.
    pub fn item_menu(&mut self, kit: &mut Kit, path: &str, is_folder: bool) -> Vec<Node> {
        let mut menu = Vec::new();
        let renamed = path.to_string();
        let rename = kit.on(move |app, _| {
            app.shared.organiser.dialog = Some(Dialog::Rename {
                name: file_name(&renamed).to_string(),
                path: renamed,
            });
        });
        menu.push(IconButton::new("pencil", "Rename").node(rename));

        let here = parent(path);
        let mut move_to = vec![Label {
            text: "Move to".into(),
        }
        .node(0)];
        let targets: Vec<String> = std::iter::once(String::new())
            .chain(self.shared.snapshot.folders.iter().cloned())
            .collect();
        for target in targets {
            let blocked = target == here || (is_folder && is_at_or_under(&target, path));
            let name = if target.is_empty() {
                "Library (root)".to_string()
            } else {
                target.replace('/', " \u{203a} ")
            };
            let command = Command::Move {
                path: path.to_string(),
                folder: target,
            };
            let id = kit.on(move |app, _| app.shared.command(command));
            move_to.push(Button::new(name).disabled(blocked).node(id));
        }
        menu.push(Node::container(Layout::Submenu, move_to));
        menu.push(Separator.node(0));
        let deleted = path.to_string();
        let delete = kit.on(move |app, _| {
            app.shared.organiser.dialog = Some(Dialog::Delete { path: deleted });
        });
        menu.push(IconButton::new("trash-2", "Delete").node(delete));
        menu
    }

    /// The open dialog, if any, as a modal tree.
    pub fn dialog_view(&mut self, kit: &mut Kit) -> Option<Spec> {
        let dialog = self.shared.organiser.dialog.as_ref()?;
        let (title, confirm) = match dialog {
            Dialog::NewFolder { .. } => ("New folder".to_string(), "Create"),
            Dialog::Rename { .. } => ("Rename".to_string(), "Rename"),
            Dialog::Delete { path } => (format!("Delete {}?", file_name(path)), "Move to trash"),
        };
        let mut body = Vec::new();
        let mut problem = None;
        match dialog {
            Dialog::NewFolder { name, .. } | Dialog::Rename { name, .. } => {
                let field = kit.keyed("dialog-name", |app, event| match event {
                    Event::Changed { text, .. } => {
                        if let Some(Dialog::NewFolder { name, .. } | Dialog::Rename { name, .. }) =
                            &mut app.shared.organiser.dialog
                        {
                            name.clone_from(text);
                        }
                    }
                    Event::Submitted { .. } => app.confirm_dialog(),
                    _ => {}
                });
                body.push(TextEdit::new(name.clone()).focused(true).node(field));
                problem = check_name(name).err();
            }
            Dialog::Delete { path } => {
                let what = if self.shared.snapshot.folders.contains(path) {
                    "This folder and everything in it go"
                } else {
                    "It goes"
                };
                body.push(
                    Label {
                        text: format!("{what} to the system trash, where you can restore it."),
                    }
                    .node(0),
                );
            }
        }
        if let Some(problem) = &problem {
            body.push(
                Weak {
                    text: problem.to_string(),
                }
                .node(0),
            );
        }
        let confirm_id = kit.on(|app, _| app.confirm_dialog());
        let cancel_id = kit.on(|app, _| app.shared.organiser.dialog = None);
        body.push(Node::container(
            Layout::Horizontal,
            vec![
                Button::new(confirm)
                    .disabled(problem.is_some())
                    .node(confirm_id),
                IconButton::new("x", "Cancel").node(cancel_id),
            ],
        ));
        Some(Spec {
            root: Root::Modal { title },
            body: Node::Container {
                id: stable_id("dialog"),
                layout: Layout::Vertical,
                children: body,
            },
        })
    }

    fn confirm_dialog(&mut self) {
        if let Some(command) = self.shared.organiser.confirmed() {
            self.shared.organiser.dialog = None;
            self.shared.command(command);
        }
    }
}
