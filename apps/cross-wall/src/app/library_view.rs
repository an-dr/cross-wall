//! The Library screen: the wallpaper folder, one folder at a time or every
//! folder as a section, with dragging between folders and organising.
use bones_kernel::messages::ui::kinds::{
    EmptyState, Heading, IconButton, Selectable, Space, Strong, Weak,
};
use bones_kernel::messages::ui::{icons, Event, Layout, Node, WidgetProps};
use cross_wall_library::{file_name, join, parent};

use super::kit::{stable_id, Kit};
use super::shown::Shown;
use super::CrossWall;
use crate::librarian::Command;

#[derive(Default)]
pub struct LibraryView {
    current_folder: String,
}

impl CrossWall {
    pub fn library_view(&mut self, kit: &mut Kit) -> Vec<Node> {
        if self.shared.settings().wallpaper_folder.trim().is_empty() {
            return choose_folder_state(kit);
        }
        if !self
            .shared
            .snapshot
            .folders
            .contains(&self.library.current_folder)
        {
            self.library.current_folder.clear();
        }
        let groups = self.shared.settings().library_groups;
        let mut nodes = vec![
            self.library_toolbar(kit, groups),
            Space { amount: 6.0 }.node(0),
        ];
        nodes.extend(self.progress_strip(false));
        nodes.push(Space { amount: 6.0 }.node(0));
        if self.shared.snapshot.files.is_empty() && self.shared.snapshot.folders.is_empty() {
            nodes.push(
                EmptyState {
                    icon: "compass".into(),
                    title: "This folder is empty".into(),
                    detail:
                        "Download wallpapers from Browse, or add images with your file manager."
                            .into(),
                }
                .node(0),
            );
            return nodes;
        }
        let content = if groups {
            self.groups_view(kit)
        } else {
            self.folder_view(kit)
        };
        let view = format!("{groups}/{}", self.library.current_folder);
        let faded = Node::Container {
            id: stable_id("library-view"),
            layout: Layout::Fade,
            children: vec![Node::container(Layout::Vertical, content)],
        };
        nodes.push(Node::Container {
            id: stable_id(("library-scroll", view)),
            layout: Layout::Scroll,
            children: vec![faded, Space { amount: 8.0 }.node(0)],
        });
        nodes
    }

    fn library_toolbar(&mut self, kit: &mut Kit, groups: bool) -> Node {
        let current = self.library.current_folder.clone();
        let new_folder = kit.on(move |app, _| {
            let parent = if groups { String::new() } else { current };
            app.shared.organiser.new_folder(parent);
        });
        let as_groups = kit.on(|app, _| {
            app.shared.store.change(|s| s.library_groups = true);
        });
        let as_folders = kit.on(|app, _| {
            app.shared.store.change(|s| s.library_groups = false);
        });
        let trailing = Node::container(
            Layout::RightToLeft,
            vec![
                IconButton::new("folder-plus", "New folder").node(new_folder),
                Selectable {
                    label: format!("{}  Folders", icons::FOLDER_TREE),
                    selected: !groups,
                }
                .node(as_folders),
                Selectable {
                    label: format!("{}  Groups", icons::LAYOUT_GRID),
                    selected: groups,
                }
                .node(as_groups),
            ],
        );
        let mut row = self.breadcrumb(kit, groups);
        row.push(trailing);
        Node::container(Layout::Horizontal, row)
    }

    fn breadcrumb(&mut self, kit: &mut Kit, groups: bool) -> Vec<Node> {
        if groups {
            return vec![Heading {
                text: "Library".into(),
            }
            .node(0)];
        }
        let current = self.library.current_folder.clone();
        let mut parts = vec![(String::new(), "Library".to_string())];
        let mut built = String::new();
        for part in current.split('/').filter(|p| !p.is_empty()) {
            built = join(&built, part);
            parts.push((built.clone(), part.to_string()));
        }
        let mut nodes = Vec::new();
        for (index, (path, name)) in parts.into_iter().enumerate() {
            if index > 0 {
                nodes.push(
                    Weak {
                        text: "\u{203a}".into(),
                    }
                    .node(0),
                );
            }
            let selected = path == current;
            let opened = path.clone();
            let id = kit.on(move |app, _| app.library.current_folder = opened);
            let crumb = Selectable {
                label: name,
                selected,
            }
            .node(id);
            nodes.push(self.drop_target(kit, &path, vec![crumb]));
        }
        nodes
    }

    fn folder_view(&mut self, kit: &mut Kit) -> Vec<Node> {
        let current = self.library.current_folder.clone();
        let folders: Vec<String> = self
            .shared
            .snapshot
            .folders
            .iter()
            .filter(|f| parent(f) == current && **f != current)
            .cloned()
            .collect();
        let mut nodes = Vec::new();
        if !folders.is_empty() {
            let chips = folders
                .iter()
                .map(|folder| self.folder_chip(kit, folder))
                .collect();
            nodes.push(Node::container(Layout::Wrap, chips));
            nodes.push(Space { amount: 10.0 }.node(0));
        }
        let items = self.files_in(&current);
        self.shared.shown = items.clone();
        nodes.push(self.gallery(kit, &items));
        nodes
    }

    fn groups_view(&mut self, kit: &mut Kit) -> Vec<Node> {
        let mut all = Vec::new();
        let mut nodes = Vec::new();
        let folders: Vec<String> = std::iter::once(String::new())
            .chain(self.shared.snapshot.folders.clone())
            .collect();
        for folder in folders {
            let items = self.files_in(&folder);
            let title = if folder.is_empty() {
                "Library".to_string()
            } else {
                folder.replace('/', " \u{203a} ")
            };
            let opened = folder.clone();
            let open = kit.on(move |app, _| {
                app.library.current_folder = opened;
                app.shared.store.change(|s| s.library_groups = false);
            });
            let heading = Node::container(
                Layout::Horizontal,
                vec![
                    Selectable {
                        label: title,
                        selected: false,
                    }
                    .node(open),
                    Weak {
                        text: format!("{}", items.len()),
                    }
                    .node(0),
                ],
            );
            let heading = Node::container(
                Layout::Tooltip,
                vec![
                    heading,
                    Strong {
                        text: "Open this folder".into(),
                    }
                    .node(0),
                ],
            );
            let mut heading = self.drop_target(kit, &folder, vec![heading]);
            if !folder.is_empty() {
                let mut menu = vec![heading];
                menu.extend(self.item_menu(kit, &folder, true));
                heading = Node::container(Layout::ContextMenu, menu);
            }
            nodes.push(heading);
            nodes.push(Space { amount: 4.0 }.node(0));
            nodes.push(self.gallery(kit, &items));
            nodes.push(Space { amount: 12.0 }.node(0));
            all.extend(items);
        }
        self.shared.shown = all;
        nodes
    }

    fn folder_chip(&mut self, kit: &mut Kit, folder: &str) -> Node {
        let count = self
            .shared
            .snapshot
            .files
            .iter()
            .filter(|e| e.folder() == folder)
            .count();
        let label = format!("{}  {}  \u{b7}  {count}", icons::FOLDER, file_name(folder));
        let opened = folder.to_string();
        let open = kit.on(move |app, _| app.library.current_folder = opened);
        let chip = Selectable {
            label,
            selected: false,
        }
        .node(open);
        let target = self.drop_target(kit, folder, vec![chip]);
        let mut menu = vec![target];
        menu.extend(self.item_menu(kit, folder, true));
        Node::container(Layout::ContextMenu, menu)
    }

    /// Wraps `children` so a file or folder dragged onto them moves into `folder`.
    fn drop_target(&mut self, kit: &mut Kit, folder: &str, children: Vec<Node>) -> Node {
        let target = folder.to_string();
        let id = kit.keyed(("drop", folder), move |app, event| {
            let Event::Dropped { payload, .. } = event else {
                return;
            };
            let Some(dragged) = app.dragged.get(payload).cloned() else {
                return;
            };
            if parent(&dragged) != target && dragged != target {
                app.shared.command(Command::Move {
                    path: dragged,
                    folder: target,
                });
            }
        });
        Node::Container {
            id,
            layout: Layout::DropTarget,
            children,
        }
    }

    /// The Liked screen: every liked file, wherever it is in the folder.
    pub fn liked_view(&mut self, kit: &mut Kit) -> Vec<Node> {
        let items: Vec<Shown> = self
            .shared
            .snapshot
            .files
            .iter()
            .filter(|e| e.info.liked)
            .cloned()
            .map(Shown::Local)
            .collect();
        if items.is_empty() {
            return vec![EmptyState {
                icon: "heart".into(),
                title: "Nothing liked yet".into(),
                detail: "Like a wallpaper in Browse or the Library to see it here.".into(),
            }
            .node(0)];
        }
        self.shared.shown = items.clone();
        vec![Node::Container {
            id: stable_id("liked-scroll"),
            layout: Layout::Scroll,
            children: vec![self.gallery(kit, &items)],
        }]
    }

    fn files_in(&self, folder: &str) -> Vec<Shown> {
        self.shared
            .snapshot
            .files
            .iter()
            .filter(|e| e.folder() == folder)
            .cloned()
            .map(Shown::Local)
            .collect()
    }
}

fn choose_folder_state(kit: &mut Kit) -> Vec<Node> {
    let choose = kit.on(|app, _| app.shared.picker.open());
    vec![
        EmptyState {
            icon: "compass".into(),
            title: "Choose your wallpaper folder".into(),
            detail: "Downloads go to its root, and you can organise it from here.".into(),
        }
        .node(0),
        Node::container(
            Layout::Centred,
            vec![
                Space { amount: 8.0 }.node(0),
                IconButton::new("folder-open", "Choose folder")
                    .primary(true)
                    .node(choose),
            ],
        ),
    ]
}
