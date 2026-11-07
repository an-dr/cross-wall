//! Small pieces of interface that several screens draw.
use crate::icons;

/// "Saving 12.3 / 45.0 MB", or just the label until any bytes have arrived.
pub fn progress_text((done, total): (u64, Option<u64>), label: &str) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let done_mb = done as f64 / MB;
    match total {
        Some(total) if done < total => {
            format!("{label} {done_mb:.1} / {:.1} MB", total as f64 / MB)
        }
        _ if done > 0 => format!("{label} {done_mb:.1} MB"),
        _ => format!("{label}..."),
    }
}

/// A titled group of related settings.
pub fn card(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
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
pub fn empty_state(ui: &mut egui::Ui, icon: icons::Icon, title: &str, detail: &str) {
    ui.add_space((ui.available_height() * 0.28).max(16.0));
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
        icons::paint(
            ui.painter(),
            rect.center(),
            52.0,
            icon,
            ui.visuals().weak_text_color(),
            ui.visuals().selection.bg_fill,
        );
        ui.add_space(6.0);
        ui.heading(title);
        ui.weak(detail);
    });
}

pub fn centered_button(ui: &mut egui::Ui, icon: icons::Icon, text: &str) -> bool {
    ui.vertical_centered(|ui| icons::button(ui, icon, text).clicked())
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_text_shows_what_is_known() {
        assert_eq!(progress_text((0, None), "Saving"), "Saving...");
        assert_eq!(
            progress_text((1024 * 1024, None), "Saving"),
            "Saving 1.0 MB"
        );
        assert_eq!(
            progress_text((1024 * 1024, Some(2 * 1024 * 1024)), "Saving"),
            "Saving 1.0 / 2.0 MB"
        );
    }
}
