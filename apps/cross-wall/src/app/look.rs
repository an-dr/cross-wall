//! The window's look: theme, accent, font, scale, and frame, following the
//! desktop while the app runs.
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use cross_wall_apply::Desktop;

use super::motion::eased_since;
use crate::chrome;
use crate::settings::ThemeMode;
use crate::theme::{self, System};

const THEME_POLL: Duration = Duration::from_secs(5);
const SCALE_POLL: f64 = 1.0;

pub struct Look {
    desktop: Desktop,
    system: System,
    system_rx: Receiver<System>,
    styled: bool,
    display_scale: f32,
    scale_checked_at: f64,
    pub display_size: Option<(u32, u32)>,
    /// The panel colour before a theme change, and when it changed.
    fade: Option<(egui::Color32, f64)>,
    pub custom_frame: bool,
}

impl Look {
    pub fn new() -> Self {
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
            fade: None,
            custom_frame: false,
        }
    }

    pub fn apply(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        let dark = theme::resolve_dark(mode, self.system);
        let before = ctx.style().visuals.clone();
        if before.dark_mode != dark {
            self.fade = Some((before.panel_fill, ctx.input(|i| i.time)));
        }
        theme::apply(
            ctx,
            self.desktop,
            System {
                dark,
                ..self.system
            },
        );
    }

    /// Follows mode, accent, and display changes while the app runs.
    pub fn follow(&mut self, ctx: &egui::Context, mode: ThemeMode, zoom: f32) {
        if !self.styled {
            self.styled = true;
            self.apply(ctx, mode);
            self.display_size = theme::primary_display_size();
            chrome::set_window_icon();
            self.custom_frame = chrome::install();
            let (ctx, desktop) = (ctx.clone(), self.desktop);
            // The font lookup can start a process, so it stays off the frame thread.
            std::thread::spawn(move || {
                let system = theme::read_font(desktop);
                if system.is_some() {
                    theme::install_font(&ctx, system);
                }
            });
        }
        while let Ok(system) = self.system_rx.try_recv() {
            if system != self.system {
                self.system = system;
                self.apply(ctx, mode);
            }
        }
        let time = ctx.input(|i| i.time);
        if time - self.scale_checked_at >= SCALE_POLL {
            self.scale_checked_at = time;
            self.display_scale = theme::display_scale();
            chrome::publish_scale(self.scale(zoom));
        }
    }

    /// The display's scale times the user's interface zoom.
    pub fn scale(&self, zoom: f32) -> f32 {
        (self.display_scale * zoom.clamp(0.5, 2.0)).clamp(0.5, 6.0)
    }

    /// Covers the window in the old colour and lifts it, so a theme change is not a flash.
    pub fn fade_ui(&mut self, ctx: &egui::Context) {
        let Some((colour, started)) = self.fade else {
            return;
        };
        let progress = eased_since(started, ctx.input(|i| i.time));
        if progress >= 1.0 {
            self.fade = None;
            return;
        }
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("theme-fade"));
        ctx.layer_painter(layer).rect_filled(
            ctx.screen_rect(),
            0.0,
            colour.gamma_multiply(1.0 - progress),
        );
    }
}
