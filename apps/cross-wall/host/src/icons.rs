//! Interface icons from the Phosphor set. Its egui crate has no duotone font, so
//! duotone is drawn here: the Fill glyph in a soft tint under the Regular outline.
use egui::{Color32, FontFamily, FontId, Painter, Pos2, Response, RichText, Ui};
use egui_phosphor::{fill, regular, Variant};

const FILL_FAMILY: &str = "phosphor-fill";
/// How strong the tint layer is, which is what gives the icons their warmth.
const TINT_STRENGTH: f32 = 0.38;
const BUTTON_ICON_SIZE: f32 = 17.0;
const BUTTON_ICON_GAP: f32 = 6.0;

#[derive(Clone, Copy)]
pub struct Icon {
    regular: &'static str,
    fill: &'static str,
    /// Drawn fully filled, for an active state such as a like.
    solid: bool,
}

macro_rules! icon {
    ($name:ident, $glyph:ident) => {
        pub const $name: Icon = Icon { regular: regular::$glyph, fill: fill::$glyph, solid: false };
    };
}

icon!(BROWSE, IMAGES);
icon!(LIBRARY, FOLDER);
icon!(LIKE, HEART);
pub const LIKED: Icon = Icon { regular: regular::HEART, fill: fill::HEART, solid: true };
icon!(SETTINGS, GEAR_SIX);
icon!(DOWNLOAD, DOWNLOAD_SIMPLE);
icon!(APPLY, MONITOR);
icon!(NEW_FOLDER, FOLDER_PLUS);
icon!(RENAME, PENCIL_SIMPLE);
icon!(MOVE, FOLDER_OPEN);
icon!(DELETE, TRASH);
icon!(SAVE, FLOPPY_DISK);
icon!(UPDATE, ARROW_CLOCKWISE);
icon!(CHOOSE, FOLDER_OPEN);
icon!(OPEN_LINK, ARROW_SQUARE_OUT);
icon!(RETRY, ARROW_COUNTER_CLOCKWISE);
icon!(INSTALL, PACKAGE);
icon!(CANCEL, X_CIRCLE);
icon!(FOLDERS_VIEW, FOLDERS);
icon!(GROUPS_VIEW, ROWS);

/// Adds the icon fonts. egui's own fonts stay as fallbacks.
pub fn add_fonts(fonts: &mut egui::FontDefinitions) {
    egui_phosphor::add_to_fonts(fonts, Variant::Regular);
    fonts.font_data.insert(FILL_FAMILY.into(), Variant::Fill.font_data().into());
    fonts.families.insert(FontFamily::Name(FILL_FAMILY.into()), vec![FILL_FAMILY.into()]);
}

/// The icon as plain text, for places that only take text, such as menus.
pub fn with(icon: Icon, text: &str) -> String {
    format!("{}  {text}", icon.regular)
}

pub fn paint(painter: &Painter, centre: Pos2, size: f32, icon: Icon, ink: Color32, tint: Color32) {
    let align = egui::Align2::CENTER_CENTER;
    let fill_font = FontId::new(size, FontFamily::Name(FILL_FAMILY.into()));
    if icon.solid {
        painter.text(centre, align, icon.fill, fill_font, tint);
        return;
    }
    painter.text(centre, align, icon.fill, fill_font, tint.gamma_multiply(TINT_STRENGTH));
    painter.text(centre, align, icon.regular, FontId::proportional(size), ink);
}

fn labelled(ui: &mut Ui, icon: Icon, label: &str, fill: Option<Color32>, ink: Option<Color32>, tint: Color32) -> Response {
    // Spaces hold the room the icon is then painted into, so the button sizes itself.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let space = ui.fonts(|f| f.glyph_width(&font, ' ')).max(1.0);
    let spaces = " ".repeat(((BUTTON_ICON_SIZE + BUTTON_ICON_GAP) / space).ceil() as usize);
    let mut text = RichText::new(format!("{spaces}{label}"));
    if let Some(ink) = ink {
        text = text.color(ink);
    }
    let mut button = egui::Button::new(text);
    if let Some(fill) = fill {
        button = button.fill(fill);
    }
    let response = ui.add(button);
    let ink = ink.unwrap_or_else(|| ui.style().interact(&response).text_color());
    let padding = ui.spacing().button_padding.x;
    let centre = egui::pos2(response.rect.left() + padding + BUTTON_ICON_SIZE / 2.0, response.rect.center().y);
    paint(ui.painter(), centre, BUTTON_ICON_SIZE, icon, ink, tint);
    response
}

pub fn button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let tint = ui.visuals().selection.bg_fill;
    labelled(ui, icon, label, None, None, tint)
}

/// The screen's main action, in the system accent colour.
pub fn primary(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let accent = ui.visuals().selection.bg_fill;
    let ink = crate::theme::on_colour(accent);
    labelled(ui, icon, label, Some(accent), Some(ink), ink)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first frame is what crashes if the fonts arrive too late.
    #[test]
    fn icons_paint_on_the_very_first_frame() {
        let ctx = egui::Context::default();
        crate::theme::install_font(&ctx, None);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                paint(ui.painter(), Pos2::ZERO, 16.0, BROWSE, Color32::WHITE, Color32::WHITE);
                paint(ui.painter(), Pos2::ZERO, 16.0, LIKED, Color32::WHITE, Color32::WHITE);
            });
        });
    }

    #[test]
    fn every_icon_exists_in_both_layers() {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        add_fonts(&mut fonts);
        ctx.set_fonts(fonts);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let line = FontId::proportional(14.0);
        let solid = FontId::new(14.0, FontFamily::Name(FILL_FAMILY.into()));
        for icon in [
            BROWSE, LIBRARY, LIKE, LIKED, SETTINGS, DOWNLOAD, APPLY, NEW_FOLDER, RENAME, MOVE, DELETE, SAVE, UPDATE,
            CHOOSE, OPEN_LINK, RETRY, INSTALL, CANCEL, FOLDERS_VIEW, GROUPS_VIEW,
        ] {
            ctx.fonts(|f| {
                assert!(icon.regular.chars().all(|c| f.has_glyph(&line, c)));
                assert!(icon.fill.chars().all(|c| f.has_glyph(&solid, c)));
            });
        }
    }
}
