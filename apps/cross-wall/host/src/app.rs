//! The screens. Slow work runs on threads; results return through channels
//! drained each frame, so the frame thread never waits on network or disk.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use bones_kernel::messages::upgrader::Action;
use cross_wall_apply::Desktop;
use cross_wall_library::{Library, Like};
use cross_wall_pexels::{Client, Orientation, Photo};

use crate::browse::Browse;
use crate::egui_module::App;
use crate::images::{self, Images, Slot};
use crate::installation::{self, UpdateState};
use crate::librarian::{Command, Librarian};
use crate::settings::Settings;
use crate::theme::{self, System};

const THUMB_WIDTH: f32 = 260.0;
const THUMB_GAP: f32 = 12.0;
const THEME_POLL: std::time::Duration = std::time::Duration::from_secs(5);
const SCALE_POLL: f64 = 1.0;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Browse,
    Liked,
    Settings,
}

fn shared_folder(settings: &Settings) -> Option<PathBuf> {
    let folder = settings.sync_folder.trim();
    (!folder.is_empty()).then(|| PathBuf::from(folder))
}

pub struct CrossWall {
    tab: Tab,
    settings: Settings,
    settings_path: PathBuf,
    draft: Settings,
    status: String,
    client: Client,
    cache: PathBuf,
    images: Images,
    browse: Browse,
    preview: Option<Photo>,
    /// Only names image files here; the librarian thread owns all writes.
    library: Library,
    librarian: Librarian,
    likes: Vec<Like>,
    pending: HashSet<u64>,
    update_requests: Sender<Action>,
    update_results: Receiver<UpdateState>,
    update_available: Option<String>,
    installed: bool,
    registered: bool,
    applying: bool,
    wallpaper_dir: PathBuf,
    applied_tx: Sender<Result<(), String>>,
    applied_rx: Receiver<Result<(), String>>,
    desktop: Desktop,
    system: System,
    system_rx: Receiver<System>,
    styled: bool,
    display_scale: f32,
    scale_checked_at: f64,
}

impl CrossWall {
    pub fn new(
        settings_path: PathBuf,
        data_dir: PathBuf,
        update_requests: Sender<Action>,
        update_results: Receiver<UpdateState>,
    ) -> Self {
        let settings = Settings::load(&settings_path);
        let cache = data_dir.join("cache");
        let cache_to_prune = cache.clone();
        std::thread::spawn(move || images::prune_cache(&cache_to_prune));
        let tab = if settings.pexels_key.is_empty() { Tab::Settings } else { Tab::Browse };
        let local = data_dir.join("library");
        let (applied_tx, applied_rx) = channel();
        // An unsupported desktop still needs a look; applying says it is unsupported.
        let desktop = cross_wall_apply::detect().unwrap_or(Desktop::Gnome);
        // Read before the window opens, so the first frame is already right.
        let system = theme::read_system(desktop);
        let (system_tx, system_rx) = channel();
        std::thread::spawn(move || loop {
            std::thread::sleep(THEME_POLL);
            if system_tx.send(theme::read_system(desktop)).is_err() {
                return;
            }
        });
        Self {
            desktop,
            system,
            system_rx,
            styled: false,
            display_scale: 1.0,
            scale_checked_at: f64::NEG_INFINITY,
            update_requests,
            update_results,
            update_available: None,
            installed: installation::running_installed(),
            registered: installation::is_registered(),
            applying: false,
            wallpaper_dir: data_dir.join("wallpaper"),
            applied_tx,
            applied_rx,
            library: Library::new(local.clone(), None),
            librarian: Librarian::start(local, shared_folder(&settings)),
            likes: Vec::new(),
            pending: HashSet::new(),
            tab,
            client: Client::new(settings.pexels_key.clone()),
            draft: settings.clone(),
            settings,
            settings_path,
            status: String::new(),
            cache,
            images: Images::new(),
            browse: Browse::new(),
            preview: None,
        }
    }

    fn save_settings(&mut self) {
        self.status = match self.draft.save(&self.settings_path) {
            Ok(()) => {
                if self.draft.pexels_key != self.settings.pexels_key {
                    self.client = Client::new(self.draft.pexels_key.clone());
                    self.browse.reset();
                }
                if self.draft.sync_folder != self.settings.sync_folder {
                    let _ = self.librarian.commands.send(Command::SetShared(shared_folder(&self.draft)));
                }
                self.settings = self.draft.clone();
                "Saved.".into()
            }
            Err(error) => format!("Could not save settings: {error}"),
        };
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.heading("Settings");
            card(ui, "Pexels", |ui| {
                ui.label("API key");
                ui.add(egui::TextEdit::singleline(&mut self.draft.pexels_key).password(true).desired_width(f32::INFINITY));
                ui.hyperlink_to("Get a free API key", "https://www.pexels.com/api/");
            });
            card(ui, "Sync", |ui| {
                ui.label("Folder kept in sync by Syncthing, Dropbox, OneDrive, or a similar tool");
                ui.add(egui::TextEdit::singleline(&mut self.draft.sync_folder).desired_width(f32::INFINITY));
            });
            card(ui, "Appearance", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Interface zoom");
                    ui.add(egui::Slider::new(&mut self.draft.ui_zoom, 0.5..=2.0));
                });
            });
            if primary_button(ui, "Save").clicked() {
                self.save_settings();
            }
            ui.add_space(8.0);
            card(ui, "App", |ui| self.install_ui(ui));
        });
    }

    fn install_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(format!("Version {}", installation::VERSION));
        ui.horizontal(|ui| {
            if ui.button("Check for updates").clicked() {
                self.status = "Checking for updates...".into();
                let _ = self.update_requests.send(Action::Check);
            }
            if let Some(version) = self.update_available.clone() {
                if primary_button(ui, &format!("Update to {version}")).clicked() {
                    self.status = "Downloading the update...".into();
                    let _ = self.update_requests.send(Action::Stage);
                }
            }
            if !self.installed && ui.button("Install").clicked() {
                self.status = "Installing...".into();
                let _ = self.update_requests.send(Action::Install);
            }
            if self.registered && ui.button("Unregister").clicked() {
                self.status = match installation::unregister() {
                    Ok(()) => {
                        self.registered = false;
                        let folder = installation::install_dir().unwrap_or_default();
                        format!("Removed from the application list. Delete {} to finish.", folder.display())
                    }
                    Err(error) => error,
                };
            }
        });
    }

    fn poll_updates(&mut self) {
        while let Ok(state) = self.update_results.try_recv() {
            self.status = match state {
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
                UpdateState::Installed => match installation::register() {
                    Ok(()) => {
                        self.registered = true;
                        self.installed = true;
                        "Installed. Start it from your application list.".into()
                    }
                    Err(error) => format!("Installed, but not added to the application list: {error}"),
                },
                UpdateState::Failed(reason) => format!("Update failed: {reason}"),
            };
        }
    }

    fn is_liked(&self, id: u64) -> bool {
        self.likes.iter().any(|like| like.photo.id == id)
    }

    fn thumb_slot(&mut self, photo: &Photo) -> &Slot {
        // A liked photo needs no network, and thumbnails for it must work offline.
        if self.is_liked(photo.id) {
            let path = self.library.image(photo.id);
            return self.images.get(&format!("liked/{}", photo.id), || {
                Box::new(move || std::fs::read(path).map_err(|e| e.to_string()))
            });
        }
        let (client, url, path) =
            (self.client.clone(), photo.src.medium.clone(), self.cache.join(format!("{}-thumb.jpg", photo.id)));
        self.images.get(&format!("thumb/{}", photo.id), || {
            Box::new(move || images::cached(path, || client.download(&url).map_err(|e| e.to_string())))
        })
    }

    fn liked_ui(&mut self, ui: &mut egui::Ui) {
        if self.likes.is_empty() {
            ui.label("Photos you like appear here.");
            return;
        }
        let photos: Vec<Photo> = self.likes.iter().map(|like| like.photo.clone()).collect();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.grid(ui, &photos));
    }

    fn like_button(&mut self, ui: &mut egui::Ui, photo: &Photo) {
        if self.pending.contains(&photo.id) {
            ui.spinner();
            ui.label("Saving");
        } else if self.is_liked(photo.id) {
            if ui.button("Unlike").clicked() {
                self.pending.insert(photo.id);
                let _ = self.librarian.commands.send(Command::Unlike(photo.id));
            }
        } else if primary_button(ui, "Like").clicked() {
            self.pending.insert(photo.id);
            let _ = self.librarian.commands.send(Command::Like(self.client.clone(), photo.clone()));
        }
    }

    /// Copies the image to its own folder first: the desktop keeps reading the
    /// file, so unliking the photo or clearing the cache must not remove it.
    fn start_apply(&mut self, photo: &Photo) {
        self.applying = true;
        self.status = "Applying wallpaper...".into();
        let tx = self.applied_tx.clone();
        let client = self.client.clone();
        let url = photo.src.original.clone();
        let liked = self.is_liked(photo.id).then(|| self.library.image(photo.id));
        let folder = self.wallpaper_dir.clone();
        let target = folder.join(format!("{}.jpg", photo.id));
        std::thread::spawn(move || {
            let result = (|| {
                let image = match liked {
                    Some(file) => std::fs::read(file).map_err(|e| e.to_string())?,
                    None => client.download(&url).map_err(|e| e.to_string())?,
                };
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                std::fs::write(&target, image).map_err(|e| e.to_string())?;
                cross_wall_apply::apply(&target)?;
                for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                    if entry.path() != target {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
                Ok(())
            })();
            let _ = tx.send(result);
        });
    }

    fn poll_library(&mut self) {
        while let Ok(result) = self.applied_rx.try_recv() {
            self.applying = false;
            self.status = match result {
                Ok(()) => "Wallpaper applied.".into(),
                Err(error) => error,
            };
        }
        while let Ok(update) = self.librarian.updates.try_recv() {
            self.likes = update.likes;
            if let Some(id) = update.finished {
                self.pending.remove(&id);
            }
            if let Some(problem) = update.problem {
                self.status = problem;
            }
        }
    }

    fn browse_ui(&mut self, ui: &mut egui::Ui) {
        if self.settings.pexels_key.is_empty() {
            ui.label("Enter your Pexels API key in Settings to browse.");
            return;
        }
        self.browse.start_once(&self.client);
        ui.horizontal(|ui| {
            let reserved = 300.0;
            let width = (ui.available_width() - reserved).max(120.0);
            let edit = ui.add(egui::TextEdit::singleline(&mut self.browse.text).hint_text("Search photos").desired_width(width));
            let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            egui::ComboBox::from_id_salt("orientation")
                .selected_text(orientation_name(self.browse.orientation))
                .show_ui(ui, |ui| {
                    for o in [Orientation::Any, Orientation::Landscape, Orientation::Portrait, Orientation::Square] {
                        ui.selectable_value(&mut self.browse.orientation, o, orientation_name(o));
                    }
                });
            if primary_button(ui, "Search").clicked() || entered {
                self.images.forget("thumb/");
                self.browse.search(&self.client);
            }
        });
        if !self.browse.message.is_empty() {
            ui.label(self.browse.message.clone());
        }
        let photos = self.browse.photos.clone();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            self.grid(ui, &photos);
            ui.add_space(4.0);
            ui.vertical_centered(|ui| {
                if self.browse.loading {
                    ui.spinner();
                } else if self.browse.has_more && ui.button("Load more").clicked() {
                    self.browse.load_more(&self.client);
                }
                ui.add_space(4.0);
                ui.hyperlink_to(egui::RichText::new("Photos provided by Pexels").small(), "https://www.pexels.com");
            });
        });
    }

    fn grid(&mut self, ui: &mut egui::Ui, photos: &[Photo]) {
        let available = ui.available_width();
        let columns = (((available + THUMB_GAP) / (THUMB_WIDTH + THUMB_GAP)) as usize).max(1);
        let width = (available - THUMB_GAP * (columns - 1) as f32) / columns as f32;
        let size = egui::vec2(width, (width * 0.625).round());
        for row in photos.chunks(columns) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = THUMB_GAP;
                for photo in row {
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        if self.thumbnail(ui, photo, size) {
                            self.preview = Some(photo.clone());
                        }
                        ui.add(egui::Hyperlink::from_label_and_url(
                            egui::RichText::new(&photo.photographer).small(),
                            &photo.photographer_url,
                        ));
                    });
                }
            });
            ui.add_space(THUMB_GAP / 2.0);
        }
    }

    /// Draws one thumbnail and says whether it was clicked.
    fn thumbnail(&mut self, ui: &mut egui::Ui, photo: &Photo, size: egui::Vec2) -> bool {
        let radius = ui.visuals().widgets.inactive.corner_radius;
        let placeholder = ui.visuals().faint_bg_color;
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        ui.painter().rect_filled(rect, radius, placeholder);
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let centre = egui::Rect::from_center_size(rect.center(), egui::vec2(20.0, 20.0));
        match self.thumb_slot(photo) {
            Slot::Ready(texture) => {
                let uv = images::cover_uv(texture.size(), size);
                let image = egui::Image::new(texture).uv(uv).corner_radius(radius);
                ui.put(rect, image.fit_to_exact_size(size));
            }
            Slot::Loading => {
                ui.put(centre, egui::Spinner::new());
            }
            Slot::Failed(error) => {
                let error = error.clone();
                ui.put(rect.shrink(8.0), egui::Label::new(egui::RichText::new(error).small()));
            }
        }
        if response.hovered() {
            let stroke = egui::Stroke::new(2.0, ui.visuals().selection.bg_fill);
            ui.painter().rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        }
        response.clicked()
    }

    fn preview_ui(&mut self, ctx: &egui::Context) {
        let Some(photo) = self.preview.clone() else { return };
        let (client, url, path) =
            (self.client.clone(), photo.src.large2x.clone(), self.cache.join(format!("{}-large.jpg", photo.id)));
        let liked_image = self.is_liked(photo.id).then(|| self.library.image(photo.id));
        let screen = ctx.screen_rect().size();
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("preview")).show(ctx, |ui| {
            let slot = self.images.get(&format!("large/{}", photo.id), || {
                Box::new(move || match liked_image {
                    Some(file) => std::fs::read(file).map_err(|e| e.to_string()),
                    None => images::cached(path, || client.download(&url).map_err(|e| e.to_string())),
                })
            });
            match slot {
                Slot::Ready(texture) => {
                    let size = texture.size_vec2();
                    let fit = (screen.x * 0.8 / size.x).min(screen.y * 0.7 / size.y).min(1.0);
                    let radius = ui.visuals().widgets.inactive.corner_radius;
                    ui.add(egui::Image::new(texture).corner_radius(radius).fit_to_exact_size(size * fit));
                }
                Slot::Loading => {
                    ui.add_sized([screen.x * 0.5, screen.y * 0.5], egui::Spinner::new());
                }
                Slot::Failed(error) => {
                    ui.label(error.clone());
                }
            }
            ui.horizontal(|ui| {
                ui.label("Photo by");
                ui.hyperlink_to(&photo.photographer, &photo.photographer_url);
                ui.weak(format!("{} x {}", photo.width, photo.height));
                ui.hyperlink_to("View on Pexels", &photo.url);
            });
            ui.horizontal(|ui| {
                self.like_button(ui, &photo);
                if self.applying {
                    ui.spinner();
                } else if primary_button(ui, "Apply").clicked() {
                    self.start_apply(&photo);
                }
                close = ui.button("Close").clicked();
            });
        });
        if close || modal.should_close() {
            self.preview = None;
            self.images.forget(&format!("large/{}", photo.id));
        }
    }

    /// Follows mode, accent, and display changes while the app runs.
    fn follow_system(&mut self, ctx: &egui::Context) {
        if !self.styled {
            self.styled = true;
            theme::apply(ctx, self.desktop, self.system);
            let (ctx, desktop) = (ctx.clone(), self.desktop);
            // The font lookup can start a process, so it stays off the frame thread.
            std::thread::spawn(move || theme::install_font(&ctx, desktop));
        }
        while let Ok(system) = self.system_rx.try_recv() {
            if system != self.system {
                self.system = system;
                theme::apply(ctx, self.desktop, system);
            }
        }
        let time = ctx.input(|i| i.time);
        if time - self.scale_checked_at >= SCALE_POLL {
            self.scale_checked_at = time;
            self.display_scale = theme::display_scale();
        }
    }
}

/// A titled group of related settings.
fn card(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    let visuals = ui.visuals();
    let frame = egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .corner_radius(visuals.widgets.inactive.corner_radius)
        .inner_margin(12);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.strong(title);
        content(ui);
    });
}

/// The screen's main action, in the system accent colour.
fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let accent = ui.visuals().selection.bg_fill;
    ui.add(egui::Button::new(egui::RichText::new(text).color(theme::on_colour(accent))).fill(accent))
}

fn orientation_name(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Any => "Any shape",
        Orientation::Landscape => "Landscape",
        Orientation::Portrait => "Portrait",
        Orientation::Square => "Square",
    }
}

impl App for CrossWall {
    fn ui(&mut self, ctx: &egui::Context) {
        self.follow_system(ctx);
        self.images.poll(ctx);
        self.browse.poll();
        self.poll_library();
        self.poll_updates();
        egui::SidePanel::left("navigation").resizable(false).exact_width(170.0).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.style_mut().spacing.button_padding.y += 3.0;
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                ui.selectable_value(&mut self.tab, Tab::Browse, "Browse");
                ui.selectable_value(&mut self.tab, Tab::Liked, format!("Liked ({})", self.likes.len()));
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
            });
        });
        if !self.status.is_empty() {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Dismiss").clicked() {
                            self.status.clear();
                        }
                    });
                });
            });
        }
        let margin = egui::Frame::central_panel(&ctx.style()).inner_margin(16);
        egui::CentralPanel::default().frame(margin).show(ctx, |ui| match self.tab {
            Tab::Browse => self.browse_ui(ui),
            Tab::Liked => self.liked_ui(ui),
            Tab::Settings => self.settings_ui(ui),
        });
        self.preview_ui(ctx);
    }

    fn scale(&self) -> f32 {
        (self.display_scale * self.settings.ui_zoom.clamp(0.5, 2.0)).clamp(0.5, 6.0)
    }
}
