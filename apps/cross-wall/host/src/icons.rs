//! Interface icons, from the symbol font bundled with egui, so every system has
//! them. A test checks that each one exists, because a missing glyph draws a box.
pub const BROWSE: &str = "🖼";
pub const LIBRARY: &str = "🗀";
pub const LIKE: &str = "♡";
pub const LIKED: &str = "♥";
pub const SETTINGS: &str = "⚙";
pub const DOWNLOAD: &str = "⬇";
pub const APPLY: &str = "🖵";
pub const NEW_FOLDER: &str = "➕";
pub const RENAME: &str = "✏";
pub const MOVE: &str = "➡";
pub const DELETE: &str = "🗑";
pub const SAVE: &str = "💾";
pub const UPDATE: &str = "⟳";
pub const CHOOSE: &str = "🗁";
pub const OPEN_LINK: &str = "↗";
pub const RETRY: &str = "⟲";
pub const INSTALL: &str = "📥";
pub const CANCEL: &str = "✖";
pub const FOLDERS_VIEW: &str = "🗀";
pub const GROUPS_VIEW: &str = "☰";

pub fn with(icon: &str, text: &str) -> String {
    format!("{icon}  {text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_exists_in_the_bundled_fonts() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(14.0);
        for icon in [
            BROWSE, LIBRARY, LIKE, LIKED, SETTINGS, DOWNLOAD, APPLY, NEW_FOLDER, RENAME, MOVE, DELETE, SAVE, UPDATE,
            CHOOSE, OPEN_LINK, RETRY, INSTALL, CANCEL, FOLDERS_VIEW, GROUPS_VIEW,
        ] {
            let found = ctx.fonts(|f| icon.chars().all(|c| f.has_glyph(&font, c)));
            assert!(found, "missing glyph: {icon}");
        }
    }
}
