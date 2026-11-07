//! The latest message, shown over the window for a few seconds.
use super::shared::Shared;
use crate::logs;

const TOAST_SECONDS: f64 = 8.0;

#[derive(Default)]
pub struct Toast {
    /// The message on screen and when it appeared.
    shown: (String, f64),
}

impl Toast {
    pub fn ui(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        let time = ctx.input(|i| i.time);
        let applying = shared.apply.is_busy();
        if applying {
            shared.status = shared.apply.progress_text();
        } else if shared.status != self.shown.0 && !shared.status.is_empty() {
            logs::info("status", &shared.status);
        }
        if shared.status != self.shown.0 {
            self.shown = (shared.status.clone(), time);
        }
        if shared.status.is_empty() {
            return;
        }
        if time - self.shown.1 > TOAST_SECONDS {
            shared.status.clear();
            return;
        }
        let phase_key = if applying {
            "applying".to_string()
        } else {
            shared.status.clone()
        };
        let appear = shared.motion.transition(ctx, "toast", phase_key);
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Tooltip)
            .anchor(
                egui::Align2::CENTER_BOTTOM,
                egui::vec2(0.0, -20.0 + (1.0 - appear) * 12.0),
            )
            .show(ctx, |ui| {
                ui.set_opacity(appear);
                egui::Frame::popup(ui.style())
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(&shared.status);
                            if ui.small_button("\u{d7}").clicked() {
                                shared.status.clear();
                            }
                        });
                    });
            });
    }
}
