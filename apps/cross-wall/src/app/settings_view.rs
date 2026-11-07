//! The Settings screen. Every control takes effect and is saved at once.
use std::sync::mpsc::{channel, Receiver, Sender};

use bones_kernel::messages::upgrader::Action;

use super::look::Look;
use super::shared::Shared;
use super::widgets::card;
use crate::icons;
use crate::installation::{self, UpdateState};
use crate::logs;
use crate::settings::ThemeMode;

pub struct SettingsView {
    /// What the folder field shows while it is being edited.
    folder_text: String,
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
            update_requests,
            update_results,
            update_available: None,
            installed: installation::running_installed(),
            registered: installation::is_registered(),
            registration_tx,
            registration_rx,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared, look: &mut Look) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(680.0));
            ui.heading("Settings");
            card(ui, "Wallpaper folder", |ui| {
                ui.label("Downloads go to the root of this folder. Your images and subfolders in it appear in the Library.");
                ui.horizontal(|ui| {
                    if icons::button(ui, icons::CHOOSE, "Choose").clicked() {
                        shared.picker.open();
                    }
                    let edit = ui.add(egui::TextEdit::singleline(&mut self.folder_text).desired_width(f32::INFINITY));
                    // A typed path is used once the field is left, not at every key.
                    if edit.lost_focus() {
                        shared.set_folder(self.folder_text.clone());
                    } else if !edit.has_focus() {
                        self.folder_text.clone_from(&shared.settings().wallpaper_folder);
                    }
                });
            });
            card(ui, "Appearance", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Theme");
                    let mut mode = shared.settings().theme;
                    for (value, name) in [(ThemeMode::Dark, "Dark"), (ThemeMode::Light, "Light"), (ThemeMode::System, "System")] {
                        ui.selectable_value(&mut mode, value, name);
                    }
                    if shared.store.change(|s| s.theme = mode) {
                        look.apply(ui.ctx(), mode);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Interface zoom");
                    let mut zoom = shared.settings().ui_zoom;
                    ui.add(egui::Slider::new(&mut zoom, 0.5..=2.0));
                    shared.store.change(|s| s.ui_zoom = zoom);
                });
            });
            ui.add_space(8.0);
            card(ui, "App", |ui| {
                self.install_ui(ui, shared);
                ui.add_space(4.0);
                logging_ui(ui, shared);
            });
        });
    }

    fn install_ui(&mut self, ui: &mut egui::Ui, shared: &mut Shared) {
        ui.label(format!("Version {}", installation::VERSION));
        ui.horizontal(|ui| {
            if icons::button(ui, icons::UPDATE, "Check for updates").clicked() {
                shared.status = "Checking for updates...".into();
                let _ = self.update_requests.send(Action::Check);
            }
            if let Some(version) = self.update_available.clone() {
                if icons::primary(ui, icons::DOWNLOAD, &format!("Update to {version}")).clicked() {
                    shared.status = "Downloading the update...".into();
                    let _ = self.update_requests.send(Action::Stage);
                }
            }
            if !self.installed && icons::button(ui, icons::INSTALL, "Install").clicked() {
                shared.status = "Installing...".into();
                let _ = self.update_requests.send(Action::Install);
            }
            if self.registered && ui.button("Unregister").clicked() {
                self.register_in_background(false);
            }
        });
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

fn logging_ui(ui: &mut egui::Ui, shared: &mut Shared) {
    ui.horizontal(|ui| {
        let mut enabled = shared
            .settings()
            .logging
            .unwrap_or_else(logs::default_enabled);
        if ui
            .checkbox(&mut enabled, "Write a log file for troubleshooting")
            .changed()
        {
            shared.store.change(|s| s.logging = Some(enabled));
            logs::set_enabled(enabled);
        }
        if icons::button(ui, icons::CHOOSE, "Open log folder").clicked() {
            let folder = crate::settings::data_dir();
            std::thread::spawn(move || {
                let _ = std::fs::create_dir_all(&folder);
                let _ = open::that_detached(folder);
            });
        }
    });
}
