//! The Settings screen. Every control takes effect and is saved at once.
use std::sync::mpsc::{channel, Receiver, Sender};

use bones_kernel::messages::upgrader::Action;

use bones_kernel::messages::ui::kinds::{
    Button, Checkbox, Heading, IconButton, Label, Selectable, Slider, Space, Strong, TextEdit,
};
use bones_kernel::messages::ui::{Event, Layout, Node, WidgetProps};

use super::kit::{stable_id, Kit};
use super::shared::Shared;
use super::CrossWall;
use crate::installation::{self, UpdateState};
use crate::logs;
use crate::settings::ThemeMode;

pub struct SettingsView {
    /// What the folder field shows while it is being edited.
    folder_text: String,
    /// Whether the folder field holds typed text not yet applied.
    editing_folder: bool,
    update_requests: Sender<Action>,
    update_results: Receiver<UpdateState>,
    update_available: Option<String>,
    installed: bool,
    registered: bool,
    registration_tx: Sender<Registration>,
    registration_rx: Receiver<Registration>,
}

/// The outcome of adding to or removing from the application list, which
/// touches shortcuts and the registry, so it runs on a thread of its own.
enum Registration {
    Added(Result<(), String>),
    Removed(Result<(), String>),
}

impl SettingsView {
    pub fn new(update_requests: Sender<Action>, update_results: Receiver<UpdateState>) -> Self {
        let (registration_tx, registration_rx) = channel();
        Self {
            folder_text: String::new(),
            editing_folder: false,
            update_requests,
            update_results,
            update_available: None,
            installed: installation::running_installed(),
            registered: installation::is_registered(),
            registration_tx,
            registration_rx,
        }
    }

    fn register_in_background(&self, add: bool) {
        let done = self.registration_tx.clone();
        std::thread::spawn(move || {
            let outcome = if add {
                Registration::Added(installation::register())
            } else {
                Registration::Removed(installation::unregister())
            };
            let _ = done.send(outcome);
        });
    }

    /// Takes update and registration results that arrived since the last frame.
    pub fn poll(&mut self, shared: &mut Shared) {
        while let Ok(outcome) = self.registration_rx.try_recv() {
            shared.status = match outcome {
                Registration::Added(Ok(())) => {
                    self.registered = true;
                    "Installed. Start it from your application list.".into()
                }
                Registration::Added(Err(error)) => {
                    format!("Installed, but not added to the application list: {error}")
                }
                Registration::Removed(Ok(())) => {
                    self.registered = false;
                    let folder = installation::install_dir().unwrap_or_default();
                    format!(
                        "Removed from the application list. Delete {} to finish.",
                        folder.display()
                    )
                }
                Registration::Removed(Err(error)) => error,
            };
        }
        while let Ok(state) = self.update_results.try_recv() {
            shared.status = match state {
                UpdateState::Available(version) => {
                    let text = format!("Version {version} is available.");
                    self.update_available = Some(version);
                    text
                }
                UpdateState::UpToDate => "This is the newest version.".into(),
                UpdateState::Staged(version) => {
                    self.update_available = None;
                    format!("Version {version} is ready. Restart the app to use it.")
                }
                UpdateState::Installed => {
                    self.installed = true;
                    self.register_in_background(true);
                    "Adding to the application list...".into()
                }
                UpdateState::Failed(reason) => format!("Update failed: {reason}"),
            };
        }
    }
}

/// A titled group of related settings: a card with a strong title.
fn card(title: &str, mut content: Vec<Node>) -> Node {
    content.insert(
        0,
        Strong {
            text: title.to_string(),
        }
        .node(0),
    );
    Node::container(Layout::Frame, content)
}

impl CrossWall {
    pub fn settings_view(&mut self, kit: &mut Kit) -> Vec<Node> {
        if !self.settings.editing_folder {
            self.settings
                .folder_text
                .clone_from(&self.shared.settings().wallpaper_folder);
        }
        let choose = kit.on(|app, _| app.shared.picker.open());
        let folder = kit.keyed("settings-folder", |app, event| match event {
            Event::Changed { text, .. } => {
                app.settings.folder_text.clone_from(text);
                app.settings.editing_folder = true;
            }
            Event::Submitted { .. } => {
                app.settings.editing_folder = false;
                let folder = app.settings.folder_text.clone();
                app.shared.set_folder(folder);
            }
            _ => {}
        });
        let folder_card = card(
            "Wallpaper folder",
            vec![
                Label {
                    text: "Downloads go to the root of this folder. Your images and subfolders in it appear in the Library. Press Enter to use a typed path.".into(),
                }
                .node(0),
                Node::container(
                    Layout::Horizontal,
                    vec![
                        IconButton::new("folder-open", "Choose").node(choose),
                        TextEdit::new(self.settings.folder_text.clone()).node(folder),
                    ],
                ),
            ],
        );

        let mode = self.shared.settings().theme;
        let mut theme_row = vec![Label {
            text: "Theme".into(),
        }
        .node(0)];
        for (value, name) in [
            (ThemeMode::Dark, "Dark"),
            (ThemeMode::Light, "Light"),
            (ThemeMode::System, "System"),
        ] {
            let id = kit.on(move |app, _| {
                app.shared.store.change(|s| s.theme = value);
            });
            theme_row.push(
                Selectable {
                    label: name.into(),
                    selected: mode == value,
                }
                .node(id),
            );
        }
        let zoom = kit.keyed("settings-zoom", |app, event| {
            if let Event::ValueChanged { value, .. } = event {
                let value = *value;
                app.shared.store.change(|s| s.ui_zoom = value);
            }
        });
        let appearance = card(
            "Appearance",
            vec![
                Node::container(Layout::Horizontal, theme_row),
                Node::container(
                    Layout::Horizontal,
                    vec![
                        Label {
                            text: "Interface zoom".into(),
                        }
                        .node(0),
                        Slider {
                            value: self.shared.settings().ui_zoom,
                            min: 0.5,
                            max: 2.0,
                        }
                        .node(zoom),
                    ],
                ),
            ],
        );
        let mut app_card = self.install_view(kit);
        app_card.push(Space { amount: 4.0 }.node(0));
        app_card.push(self.logging_view(kit));
        vec![Node::Container {
            id: stable_id("settings-scroll"),
            layout: Layout::Scroll,
            children: vec![
                Heading {
                    text: "Settings".into(),
                }
                .node(0),
                folder_card,
                appearance,
                Space { amount: 8.0 }.node(0),
                card("App", app_card),
            ],
        }]
    }

    fn install_view(&mut self, kit: &mut Kit) -> Vec<Node> {
        let mut buttons = Vec::new();
        let check = kit.on(|app, _| {
            app.shared.status = "Checking for updates...".into();
            let _ = app.settings.update_requests.send(Action::Check);
        });
        buttons.push(IconButton::new("refresh-cw", "Check for updates").node(check));
        if let Some(version) = self.settings.update_available.clone() {
            let stage = kit.on(|app, _| {
                app.shared.status = "Downloading the update...".into();
                let _ = app.settings.update_requests.send(Action::Stage);
            });
            buttons.push(
                IconButton::new("download", format!("Update to {version}"))
                    .primary(true)
                    .node(stage),
            );
        }
        if !self.settings.installed {
            let install = kit.on(|app, _| {
                app.shared.status = "Installing...".into();
                let _ = app.settings.update_requests.send(Action::Install);
            });
            buttons.push(IconButton::new("hard-drive-download", "Install").node(install));
        }
        if self.settings.registered {
            let unregister = kit.on(|app, _| app.settings.register_in_background(false));
            buttons.push(Button::new("Unregister").node(unregister));
        }
        vec![
            Label {
                text: format!("Version {}", installation::VERSION),
            }
            .node(0),
            Node::container(Layout::Horizontal, buttons),
        ]
    }

    fn logging_view(&mut self, kit: &mut Kit) -> Node {
        let enabled = self
            .shared
            .settings()
            .logging
            .unwrap_or_else(logs::default_enabled);
        let toggle = kit.on(|app, event| {
            if let Event::Toggled { on, .. } = event {
                let on = *on;
                app.shared.store.change(|s| s.logging = Some(on));
                logs::set_enabled(on);
            }
        });
        let open = kit.on(|_, _| {
            let folder = crate::settings::data_dir();
            std::thread::spawn(move || {
                let _ = std::fs::create_dir_all(&folder);
                let _ = open::that_detached(folder);
            });
        });
        Node::container(
            Layout::Horizontal,
            vec![
                Checkbox {
                    label: "Write a log file for troubleshooting".into(),
                    checked: enabled,
                }
                .node(toggle),
                IconButton::new("folder-open", "Open log folder").node(open),
            ],
        )
    }
}
