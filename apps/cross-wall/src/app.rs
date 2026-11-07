//! The window: side navigation and the screens, composed. Each screen keeps its
//! own state in its own module and shares the services in `shared`. Slow work
//! runs on threads; results return through channels drained each frame, so the
//! frame thread never waits on the network or the disk.
mod apply_worker;
mod browse_view;
mod library_view;
mod look;
mod motion;
mod organise;
mod preview;
mod settings_view;
mod shared;
mod shown;
mod tiles;
mod toast;
mod widgets;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use bones_kernel::messages::upgrader::Action;
use egui_animation::{animate_eased, easing};

use browse_view::BrowseView;
use library_view::LibraryView;
use look::Look;
use preview::Preview;
use settings_view::SettingsView;
use shared::Shared;
use toast::Toast;

use crate::chrome;
use crate::colour::on_colour;
use crate::egui_module::App;
use crate::icons;
use crate::installation::UpdateState;
use crate::logs;
use crate::settings::Store;
use crate::theme;

const NAV_ROW_HEIGHT: f32 = 40.0;

#[derive(PartialEq, Clone, Copy)]
#[repr(u8)]
enum Tab {
    Browse,
    Library,
    Liked,
    Settings,
}

pub struct CrossWall {
    tab: Tab,
    shared: Shared,
    look: Look,
    browse: BrowseView,
    library: LibraryView,
    settings: SettingsView,
    preview: Preview,
    toast: Toast,
}

impl CrossWall {
    pub fn new(
        settings_path: PathBuf,
        data_dir: PathBuf,
        update_requests: Sender<Action>,
        update_results: Receiver<UpdateState>,
    ) -> Self {
        let store = Store::open(settings_path);
        logs::set_enabled(store.get().logging.unwrap_or_else(logs::default_enabled));
        Self {
            tab: Tab::Browse,
            shared: Shared::new(store, data_dir),
            look: Look::new(),
            browse: BrowseView::new(),
            library: LibraryView::default(),
            settings: SettingsView::new(update_requests, update_results),
            preview: Preview::default(),
            toast: Toast::default(),
        }
    }

    /// The side navigation, with one highlight that slides between items.
    fn navigation_ui(&mut self, ui: &mut egui::Ui) {
        let files = &self.shared.snapshot.files;
        let liked = files.iter().filter(|e| e.info.liked).count();
        let items = [
            (Tab::Browse, icons::BROWSE, "Browse".to_string()),
            (
                Tab::Library,
                icons::LIBRARY,
                format!("Library  \u{b7}  {}", files.len()),
            ),
            (Tab::Liked, icons::LIKE, format!("Liked  \u{b7}  {liked}")),
            (Tab::Settings, icons::SETTINGS, "Settings".to_string()),
        ];
        let selected = items
            .iter()
            .position(|item| item.0 == self.tab)
            .unwrap_or(0);
        let position = animate_eased(
            ui.ctx(),
            "nav-highlight",
            selected as f32,
            motion::TRANSITION_SECONDS as f32,
            easing::cubic_out,
        );
        let width = ui.available_width();
        let spacing = 4.0;
        let top = ui.cursor().top();
        let radius = ui.visuals().widgets.inactive.corner_radius;
        let accent = ui.visuals().selection.bg_fill;
        let highlight = egui::Rect::from_min_size(
            egui::pos2(
                ui.cursor().left(),
                top + position * (NAV_ROW_HEIGHT + spacing),
            ),
            egui::vec2(width, NAV_ROW_HEIGHT),
        );
        let highlight_shape = ui.painter().add(egui::Shape::Noop);
        for (index, (tab, icon, label)) in items.iter().enumerate() {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, NAV_ROW_HEIGHT), egui::Sense::click());
            if response.hovered() && index != selected {
                ui.painter()
                    .rect_filled(rect, radius, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            // The label turns as the highlight passes under it.
            let over = 1.0 - (position - index as f32).abs().min(1.0);
            let colour = ui
                .visuals()
                .text_color()
                .lerp_to_gamma(on_colour(accent), over);
            let font = egui::TextStyle::Button.resolve(ui.style());
            let tint = accent.lerp_to_gamma(colour, over);
            icons::paint(
                ui.painter(),
                rect.left_center() + egui::vec2(22.0, 0.0),
                20.0,
                *icon,
                colour,
                tint,
            );
            ui.painter().text(
                rect.left_center() + egui::vec2(40.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                font,
                colour,
            );
            if response.clicked() {
                self.tab = *tab;
            }
            ui.add_space(spacing - ui.spacing().item_spacing.y);
        }
        ui.painter().set(
            highlight_shape,
            egui::Shape::rect_filled(highlight, radius, accent),
        );
    }
}

impl App for CrossWall {
    fn setup(&mut self, ctx: &egui::Context) {
        theme::install_font(ctx, None);
    }

    fn ui(&mut self, ctx: &egui::Context) {
        let settings = self.shared.settings();
        self.look.follow(ctx, settings.theme, settings.ui_zoom);
        self.shared.images.poll(ctx);
        self.shared.poll();
        self.browse.poll(&self.shared);
        self.settings.poll(&mut self.shared);
        let nav_fill = ctx.style().visuals.faint_bg_color;
        let nav = egui::Frame::new().fill(nav_fill).inner_margin(10);
        if self.look.custom_frame {
            egui::TopBottomPanel::top("title-bar")
                .exact_height(chrome::TITLE_HEIGHT)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(nav_fill))
                .show(ctx, chrome::title_bar);
        }
        egui::SidePanel::left("navigation")
            .resizable(false)
            .show_separator_line(false)
            .exact_width(180.0)
            .frame(nav)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                self.navigation_ui(ui);
            });
        let margin = egui::Frame::central_panel(&ctx.style()).inner_margin(16);
        let appear = self
            .shared
            .motion
            .transition(ctx, "tab", format!("{}", self.tab as u8));
        egui::CentralPanel::default().frame(margin).show(ctx, |ui| {
            ui.set_opacity(appear);
            ui.add_space((1.0 - appear) * 10.0);
            match self.tab {
                Tab::Browse => self.browse.ui(ui, &mut self.shared, self.look.display_size),
                Tab::Library => self.library.ui(ui, &mut self.shared),
                Tab::Liked => library_view::liked_ui(ui, &mut self.shared),
                Tab::Settings => self.settings.ui(ui, &mut self.shared, &mut self.look),
            }
        });
        self.preview.ui(ctx, &mut self.shared);
        if let Some(command) = self.shared.organiser.dialog_ui(ctx, &self.shared.snapshot) {
            self.shared.command(command);
        }
        self.toast.ui(ctx, &mut self.shared);
        self.look.fade_ui(ctx);
    }

    fn scale(&self) -> f32 {
        self.look.scale(self.shared.settings().ui_zoom)
    }
}
