//! The screens. Slow work runs on threads; results return through channels
//! drained each frame, so the frame thread never waits on network or disk.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use bones_kernel::messages::upgrader::Action;
use cross_wall_apply::Desktop;
use cross_wall_library::{Library, Like};
use cross_wall_sources::{Client, MinSize, Orientation, Photo, Source};

use egui_animation::{animate_bool_eased, easing};

use crate::browse::Browse;
use crate::chrome;
use crate::egui_module::App;
use crate::gallery;
use crate::images::{self, Images, Slot};
use crate::installation::{self, UpdateState};
use crate::librarian::{Command, Librarian};
use crate::settings::{Settings, ThemeMode};
use crate::theme::{self, System};

const ROW_HEIGHT: f32 = 210.0;
const TILE_GAP: f32 = 8.0;
const TOAST_SECONDS: f64 = 8.0;
const AUTO_LOAD_MARGIN: f32 = 500.0;
const RESOLUTIONS: [(u32, u32); 5] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160), (5120, 2880)];
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
    pending: HashSet<String>,
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
    display_size: Option<(u32, u32)>,
    /// The photos last shown, which the preview steps through.
    shown: Vec<Photo>,
    toast: (String, f64),
    custom_frame: bool,
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
        let tab = Tab::Browse;
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
            display_size: None,
            shown: Vec::new(),
            toast: (String::new(), 0.0),
            custom_frame: false,
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
            client: Client::new(),
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
            ui.set_max_width(ui.available_width().min(680.0));
            ui.heading("Settings");
            card(ui, "Sync", |ui| {
                ui.label("Folder kept in sync by Syncthing, Dropbox, OneDrive, or a similar tool");
                ui.add(egui::TextEdit::singleline(&mut self.draft.sync_folder).desired_width(f32::INFINITY));
            });
            card(ui, "Appearance", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Theme");
                    let mut mode = self.settings.theme;
                    for (value, name) in [(ThemeMode::Dark, "Dark"), (ThemeMode::Light, "Light"), (ThemeMode::System, "System")] {
                        ui.selectable_value(&mut mode, value, name);
                    }
                    if mode != self.settings.theme {
                        self.settings.theme = mode;
                        self.draft.theme = mode;
                        self.apply_theme(ui.ctx());
                        if let Err(error) = self.settings.save(&self.settings_path) {
                            self.status = format!("Could not save settings: {error}");
                        }
                    }
                });
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

    fn is_liked(&self, id: &str) -> bool {
        self.likes.iter().any(|like| like.photo.id == id)
    }

    fn thumb_slot(&mut self, photo: &Photo) -> &Slot {
        // A liked photo needs no network, and thumbnails for it must work offline.
        if self.is_liked(&photo.id) {
            let path = self.library.image(&photo.id);
            return self.images.get(&format!("liked/{}", photo.id), || {
                Box::new(move || std::fs::read(path).map_err(|e| e.to_string()))
            });
        }
        let (client, wanted, path) =
            (self.client.clone(), photo.clone(), self.cache.join(format!("{}-thumb.jpg", photo.id)));
        self.images.get(&format!("thumb/{}", photo.id), || {
            Box::new(move || images::cached(path, || client.download_thumbnail(&wanted).map_err(|e| e.to_string())))
        })
    }

    fn liked_ui(&mut self, ui: &mut egui::Ui) {
        if self.likes.is_empty() {
            empty_state(ui, "♥", "Nothing liked yet", "Photos you like are kept here and work offline.");
            return;
        }
        let photos: Vec<Photo> = self.likes.iter().map(|like| like.photo.clone()).collect();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.gallery(ui, &photos));
    }

    fn like_button(&mut self, ui: &mut egui::Ui, photo: &Photo) {
        if self.pending.contains(&photo.id) {
            ui.spinner();
            ui.label("Saving");
        } else if self.is_liked(&photo.id) {
            if ui.button("Unlike").clicked() {
                self.pending.insert(photo.id.clone());
                let _ = self.librarian.commands.send(Command::Unlike(photo.id.clone()));
            }
        } else if primary_button(ui, "Like").clicked() {
            self.pending.insert(photo.id.clone());
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
        let liked = self.is_liked(&photo.id).then(|| self.library.image(&photo.id));
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

    fn effective_min(&self) -> MinSize {
        let size = match self.settings.min_resolution {
            Some([width, height]) => (width, height),
            None => self.display_size.unwrap_or((0, 0)),
        };
        MinSize { width: size.0, height: size.1 }
    }

    fn resolution_label(&self) -> String {
        match (self.settings.min_resolution, self.display_size) {
            (None, Some((w, h))) => format!("≥ {w} × {h}"),
            (None, None) | (Some([0, 0]), _) => "Any size".to_string(),
            (Some([w, h]), _) => format!("≥ {w} × {h}"),
        }
    }

    /// The minimum-resolution menu. Says whether the choice changed.
    fn resolution_menu(&mut self, ui: &mut egui::Ui) -> bool {
        let before = self.settings.min_resolution;
        let mut chosen = before;
        let display = self.display_size.map_or("Main display".to_string(), |(w, h)| format!("Main display ({w} × {h})"));
        egui::ComboBox::from_id_salt("min-resolution").selected_text(self.resolution_label()).width(150.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, display);
            ui.selectable_value(&mut chosen, Some([0, 0]), "Any size");
            for (w, h) in RESOLUTIONS {
                ui.selectable_value(&mut chosen, Some([w, h]), format!("{w} × {h}"));
            }
        });
        if chosen == before {
            return false;
        }
        self.settings.min_resolution = chosen;
        self.draft.min_resolution = chosen;
        self.browse.min = self.effective_min();
        if let Err(error) = self.settings.save(&self.settings_path) {
            self.status = format!("Could not save settings: {error}");
        }
        true
    }

    fn browse_ui(&mut self, ui: &mut egui::Ui) {
        self.browse.start_once(&self.client);
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            let searchable = self.browse.source.has_search();
            let hint = if searchable { "Search wallpapers" } else { "This source has no search" };
            let edit = egui::TextEdit::singleline(&mut self.browse.text)
                .hint_text(hint)
                .margin(egui::Margin::symmetric(10, 5))
                .desired_width((ui.available_width() - 450.0).max(130.0));
            let edit = ui.add_enabled(searchable, edit);
            refresh |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            egui::ComboBox::from_id_salt("source").selected_text(self.browse.source.name()).show_ui(ui, |ui| {
                for source in Source::ALL {
                    refresh |= ui.selectable_value(&mut self.browse.source, source, source.name()).changed();
                }
            });
            egui::ComboBox::from_id_salt("orientation")
                .selected_text(orientation_name(self.browse.orientation))
                .show_ui(ui, |ui| {
                    for o in [Orientation::Any, Orientation::Landscape, Orientation::Portrait, Orientation::Square] {
                        refresh |= ui.selectable_value(&mut self.browse.orientation, o, orientation_name(o)).changed();
                    }
                });
            refresh |= self.resolution_menu(ui);
        });
        ui.add_space(6.0);
        if refresh {
            self.images.forget("thumb/");
            self.browse.search(&self.client);
        }
        self.progress_strip(ui);
        if self.browse.photos.is_empty() && self.browse.loading {
            empty_state(ui, "🖼", "Finding wallpapers", "Asking the source for photos...");
            return;
        }
        if self.browse.photos.is_empty() && !self.browse.message.is_empty() {
            let message = self.browse.message.clone();
            empty_state(ui, "🖼", "No wallpapers to show", &message);
            if self.browse.has_more && centered_button(ui, "Try again") {
                self.browse.load_more(&self.client);
            }
            return;
        }
        let photos = self.browse.photos.clone();
        let scroll = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            self.gallery(ui, &photos);
            if !self.browse.message.is_empty() {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| ui.label(self.browse.message.clone()));
            }
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if self.browse.loading {
                    ui.spinner();
                } else if self.browse.is_stalled() && ui.button("Try again").clicked() {
                    self.browse.load_more(&self.client);
                }
                ui.add_space(4.0);
                let source = self.browse.source;
                ui.hyperlink_to(egui::RichText::new(format!("Photos provided by {}", source.name())).small(), source.home());
                ui.add_space(8.0);
            });
        });
        // Starts before the very end, so scrolling rarely reaches a gap.
        let bottom = scroll.state.offset.y + scroll.inner_rect.height();
        if bottom + AUTO_LOAD_MARGIN >= scroll.content_size.y && self.browse.can_auto_load() {
            self.browse.load_more(&self.client);
        }
    }

    /// A thin bar under the toolbar. It slides while asking for a page and fills
    /// as thumbnails arrive. It always keeps its height, so the grid never jumps.
    fn progress_strip(&mut self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), egui::Sense::hover());
        let progress = self.images.progress();
        let accent = ui.visuals().selection.bg_fill;
        let track = ui.visuals().faint_bg_color;
        let time = ui.input(|i| i.time) as f32;
        let segment = if let Some((done, total)) = progress {
            Some((0.0, (done as f32 + 0.5) / total as f32))
        } else if self.browse.loading {
            let start = (time * 0.7).fract() * 1.25 - 0.25;
            Some((start.max(0.0), (start + 0.25).min(1.0)))
        } else {
            None
        };
        let Some((from, to)) = segment else { return };
        let radius = egui::CornerRadius::same(2);
        ui.painter().rect_filled(rect, radius, track);
        let bar = egui::Rect::from_x_y_ranges(rect.left() + rect.width() * from..=rect.left() + rect.width() * to, rect.y_range());
        ui.painter().rect_filled(bar, radius, accent);
        if let Some((done, total)) = progress {
            let text = format!("Loading wallpapers {done} / {total}");
            let font = egui::TextStyle::Small.resolve(ui.style());
            let colour = ui.visuals().weak_text_color();
            let at = rect.right_bottom() + egui::vec2(0.0, 3.0);
            ui.painter().text(at, egui::Align2::RIGHT_TOP, text, font, colour);
        }
    }

    fn gallery(&mut self, ui: &mut egui::Ui, photos: &[Photo]) {
        self.shown = photos.to_vec();
        let aspects: Vec<f32> = photos.iter().map(|p| gallery::aspect(p.width, p.height)).collect();
        let width = ui.available_width();
        for row in gallery::rows(&aspects, width, ROW_HEIGHT, TILE_GAP) {
            let (row_rect, _) = ui.allocate_exact_size(egui::vec2(width, row.height), egui::Sense::hover());
            // Off-screen rows request nothing, so scrolling is what loads images.
            if ui.is_rect_visible(row_rect) {
                let mut x = row_rect.left();
                for (index, tile_width) in row.tiles {
                    let rect = egui::Rect::from_min_size(egui::pos2(x, row_rect.top()), egui::vec2(tile_width, row.height));
                    self.tile(ui, &photos[index], rect);
                    x += tile_width + TILE_GAP;
                }
            }
            ui.add_space(TILE_GAP - ui.spacing().item_spacing.y);
        }
    }

    fn tile(&mut self, ui: &mut egui::Ui, photo: &Photo, rect: egui::Rect) {
        let id = ui.id().with(("tile", &photo.id));
        let response = ui.interact(rect, id, egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let radius = ui.visuals().widgets.inactive.corner_radius;
        let time = ui.input(|i| i.time) as f32;
        let texture = match self.thumb_slot(photo) {
            Slot::Ready(texture) => Some(texture.clone()),
            _ => None,
        };
        match texture {
            Some(texture) => {
                let uv = images::cover_uv(texture.size(), rect.size());
                let appear = animate_bool_eased(ui.ctx(), id.with("appear"), true, easing::cubic_out, 0.25);
                ui.painter().rect_filled(rect, radius, ui.visuals().faint_bg_color);
                let tint = egui::Color32::WHITE.gamma_multiply(appear);
                egui::Image::new(&texture).uv(uv).tint(tint).corner_radius(radius).maintain_aspect_ratio(false).paint_at(ui, rect);
            }
            None => {
                // A soft pulse while the photo loads, rather than a row of spinners.
                let base = ui.visuals().faint_bg_color;
                let pulse = 0.65 + 0.35 * (time * 2.5).sin().abs();
                ui.painter().rect_filled(rect, radius, base.gamma_multiply(pulse));
            }
        }
        let hovered = ui.rect_contains_pointer(rect);
        let shown = animate_bool_eased(ui.ctx(), id.with("hover"), hovered, easing::cubic_out, 0.16);
        if self.is_liked(&photo.id) && shown < 0.5 {
            let badge = rect.right_top() + egui::vec2(-16.0, 16.0);
            ui.painter().circle_filled(badge, 11.0, egui::Color32::from_black_alpha(140));
            ui.painter().text(badge, egui::Align2::CENTER_CENTER, "♥", egui::FontId::proportional(13.0), egui::Color32::WHITE);
        }
        if shown > 0.0 {
            let bar = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 40.0), rect.right_bottom());
            let bar_radius = egui::CornerRadius { nw: 0, ne: 0, sw: radius.sw, se: radius.se };
            ui.painter().rect_filled(bar, bar_radius, egui::Color32::from_black_alpha((170.0 * shown) as u8));
        }
        if shown > 0.5 {
            let bar = egui::Rect::from_min_max(egui::pos2(rect.left() + 8.0, rect.bottom() - 40.0), rect.right_bottom() - egui::vec2(8.0, 0.0));
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.scope_builder(egui::UiBuilder::new().id_salt(("tile-bar", &photo.id)).max_rect(bar).layout(layout), |ui| {
                ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::from_white_alpha(40);
                ui.visuals_mut().widgets.hovered.weak_bg_fill = egui::Color32::from_white_alpha(70);
                let credit = egui::RichText::new(&photo.photographer).small().color(egui::Color32::WHITE);
                if rect.width() > 230.0 {
                    ui.add(egui::Hyperlink::from_label_and_url(credit, &photo.photographer_url).open_in_new_tab(true));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.applying {
                        ui.spinner();
                    } else if ui.button("Apply").clicked() {
                        self.start_apply(photo);
                    }
                    self.like_button(ui, photo);
                });
            });
        }
        if response.clicked() {
            self.preview = Some(photo.clone());
        }
    }

    /// A full-window view that steps through the photos with the arrow keys.
    fn preview_ui(&mut self, ctx: &egui::Context) {
        let opened = animate_bool_eased(ctx, "preview-open", self.preview.is_some(), easing::cubic_out, 0.18);
        let Some(photo) = self.preview.clone() else { return };
        let (client, url, path) =
            (self.client.clone(), photo.src.large2x.clone(), self.cache.join(format!("{}-large.jpg", photo.id)));
        let liked_image = self.is_liked(&photo.id).then(|| self.library.image(&photo.id));
        let screen = ctx.screen_rect();
        let position = self.shown.iter().position(|p| p.id == photo.id);
        let mut step = ctx.input(|i| i32::from(i.key_pressed(egui::Key::ArrowRight)) - i32::from(i.key_pressed(egui::Key::ArrowLeft)));
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Area::new(egui::Id::new("preview")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            // Takes the pointer, so nothing behind the preview reacts.
            ui.allocate_rect(screen, egui::Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha((242.0 * opened) as u8));
            let stage = egui::Rect::from_min_max(screen.min + egui::vec2(64.0, 20.0), screen.max - egui::vec2(64.0, 76.0));
            let slot = self.images.get(&format!("large/{}", photo.id), || {
                Box::new(move || match liked_image {
                    Some(file) => std::fs::read(file).map_err(|e| e.to_string()),
                    None => images::cached(path, || client.download(&url).map_err(|e| e.to_string())),
                })
            });
            let large = match slot {
                Slot::Ready(texture) => Some(texture.clone()),
                Slot::Failed(error) => {
                    let error = error.clone();
                    ui.put(stage, egui::Label::new(egui::RichText::new(error).color(egui::Color32::WHITE)));
                    None
                }
                Slot::Loading => {
                    ui.put(stage, egui::Spinner::new().color(egui::Color32::WHITE).size(32.0));
                    let below = stage.center() + egui::vec2(0.0, 34.0);
                    ui.painter().text(below, egui::Align2::CENTER_TOP, "Loading full image", egui::TextStyle::Body.resolve(ui.style()), egui::Color32::WHITE);
                    None
                }
            };
            if let Some(texture) = large {
                let size = texture.size_vec2();
                let fit = (stage.width() / size.x).min(stage.height() / size.y);
                let rect = egui::Rect::from_center_size(stage.center(), size * fit);
                let radius = ui.visuals().widgets.inactive.corner_radius;
                let tint = egui::Color32::WHITE.gamma_multiply(opened);
                egui::Image::new(&texture).tint(tint).corner_radius(radius).maintain_aspect_ratio(false).paint_at(ui, rect);
            }
            let visuals = ui.visuals_mut();
            visuals.override_text_color = Some(egui::Color32::WHITE);
            visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_white_alpha(30);
            visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_white_alpha(60);
            visuals.widgets.active.weak_bg_fill = egui::Color32::from_white_alpha(80);
            let round = |ui: &mut egui::Ui, centre: egui::Pos2, text: &str| {
                let rect = egui::Rect::from_center_size(centre, egui::vec2(38.0, 38.0));
                ui.put(rect, egui::Button::new(egui::RichText::new(text).size(18.0)).corner_radius(19)).clicked()
            };
            close |= round(ui, screen.right_top() + egui::vec2(-32.0, 32.0), "×");
            if position.is_some_and(|p| p > 0) && round(ui, egui::pos2(screen.left() + 32.0, stage.center().y), "‹") {
                step = -1;
            }
            if position.is_some_and(|p| p + 1 < self.shown.len()) && round(ui, egui::pos2(screen.right() - 32.0, stage.center().y), "›") {
                step = 1;
            }
            let bar = egui::Rect::from_min_max(egui::pos2(screen.left() + 24.0, screen.bottom() - 60.0), screen.right_bottom() - egui::vec2(24.0, 12.0));
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(layout), |ui| {
                ui.hyperlink_to(egui::RichText::new(&photo.photographer).strong(), &photo.photographer_url);
                if photo.width > 0 {
                    ui.label(format!("{} × {}", photo.width, photo.height));
                }
                if !photo.license.is_empty() {
                    ui.label(&photo.license);
                }
                ui.hyperlink_to("View at the source", &photo.url);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.applying {
                        ui.spinner();
                    } else if primary_button(ui, "Apply").clicked() {
                        self.start_apply(&photo);
                    }
                    self.like_button(ui, &photo);
                });
            });
        });
        let next = position
            .and_then(|p| p.checked_add_signed(step as isize))
            .filter(|_| step != 0)
            .and_then(|p| self.shown.get(p).cloned());
        if close || next.is_some() {
            self.preview = next;
            self.images.forget(&format!("large/{}", photo.id));
        }
    }

    /// Shows the latest message over the window for a few seconds.
    fn toast_ui(&mut self, ctx: &egui::Context) {
        let time = ctx.input(|i| i.time);
        if self.status != self.toast.0 {
            self.toast = (self.status.clone(), time);
        }
        if self.status.is_empty() {
            return;
        }
        if time - self.toast.1 > TOAST_SECONDS {
            self.status.clear();
            return;
        }
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -20.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(10).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(&self.status);
                        if ui.small_button("×").clicked() {
                            self.status.clear();
                        }
                    });
                });
            });
    }
    fn apply_theme(&self, ctx: &egui::Context) {
        let dark = theme::resolve_dark(self.settings.theme, self.system);
        theme::apply(ctx, self.desktop, System { dark, ..self.system });
    }

    /// Follows mode, accent, and display changes while the app runs.
    fn follow_system(&mut self, ctx: &egui::Context) {
        if !self.styled {
            self.styled = true;
            self.apply_theme(ctx);
            self.display_size = theme::primary_display_size();
            self.custom_frame = chrome::install();
            self.browse.min = self.effective_min();
            let (ctx, desktop) = (ctx.clone(), self.desktop);
            // The font lookup can start a process, so it stays off the frame thread.
            std::thread::spawn(move || theme::install_font(&ctx, desktop));
        }
        while let Ok(system) = self.system_rx.try_recv() {
            if system != self.system {
                self.system = system;
                self.apply_theme(ctx);
            }
        }
        let time = ctx.input(|i| i.time);
        if time - self.scale_checked_at >= SCALE_POLL {
            self.scale_checked_at = time;
            self.display_scale = theme::display_scale();
            chrome::publish_scale(self.scale());
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

/// A centred icon and message where a screen has nothing to show.
fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, detail: &str) {
    ui.add_space((ui.available_height() * 0.28).max(16.0));
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(icon).size(44.0).weak());
        ui.add_space(6.0);
        ui.heading(title);
        ui.weak(detail);
    });
}

fn centered_button(ui: &mut egui::Ui, text: &str) -> bool {
    ui.vertical_centered(|ui| ui.button(text).clicked()).inner
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
        self.browse.poll(&self.client);
        self.poll_library();
        self.poll_updates();
        let nav_fill = ctx.style().visuals.faint_bg_color;
        let nav = egui::Frame::new().fill(nav_fill).inner_margin(10);
        if self.custom_frame {
            egui::TopBottomPanel::top("title-bar")
                .exact_height(chrome::TITLE_HEIGHT)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(nav_fill))
                .show(ctx, chrome::title_bar);
        }
        egui::SidePanel::left("navigation").resizable(false).show_separator_line(false).exact_width(180.0).frame(nav).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.style_mut().spacing.button_padding.y += 4.0;
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                ui.selectable_value(&mut self.tab, Tab::Browse, "🖼  Browse");
                ui.selectable_value(&mut self.tab, Tab::Liked, format!("♥  Liked  ·  {}", self.likes.len()));
                ui.selectable_value(&mut self.tab, Tab::Settings, "⚙  Settings");
            });
        });
        let margin = egui::Frame::central_panel(&ctx.style()).inner_margin(16);
        egui::CentralPanel::default().frame(margin).show(ctx, |ui| match self.tab {
            Tab::Browse => self.browse_ui(ui),
            Tab::Liked => self.liked_ui(ui),
            Tab::Settings => self.settings_ui(ui),
        });
        self.preview_ui(ctx);
        self.toast_ui(ctx);
    }

    fn scale(&self) -> f32 {
        (self.display_scale * self.settings.ui_zoom.clamp(0.5, 2.0)).clamp(0.5, 6.0)
    }
}
