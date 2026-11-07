//! Contrast helpers shared by the theme and the icons.
use egui::Color32;

/// Perceived brightness from 0 (black) to 1 (white).
pub fn luminance(c: Color32) -> f32 {
    (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0
}

/// Black or white, whichever reads on `background`.
pub fn on_colour(background: Color32) -> Color32 {
    if luminance(background) > 0.55 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_on_the_accent_stay_readable() {
        assert_eq!(on_colour(Color32::from_rgb(0, 120, 212)), Color32::WHITE);
        assert_eq!(on_colour(Color32::from_rgb(99, 208, 223)), Color32::BLACK);
    }
}
