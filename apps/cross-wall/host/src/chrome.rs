//! The app's own title bar. SDL removes the system frame, and a hit-test callback
//! gives moving, resizing, and the desktop's snapping back to the window manager.
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};

use sdl3::sys::{events, rect::SDL_Point, video};

pub const TITLE_HEIGHT: f32 = 36.0;
pub const BUTTON_WIDTH: f32 = 46.0;
const BUTTONS: f32 = 3.0;
const RESIZE_EDGE: i32 = 6;

// Read by the callback, which SDL calls outside any frame. In window pixels.
static TITLE_PIXELS: AtomicU32 = AtomicU32::new(0);
static BUTTON_PIXELS: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq)]
enum Zone {
    Normal,
    Drag,
    Resize(i8, i8),
}

/// What a window point does. Caption buttons stay normal so egui receives their clicks.
fn zone(x: i32, y: i32, width: i32, height: i32, title: i32, buttons: i32, maximized: bool) -> Zone {
    if !maximized {
        let horizontal = if x < RESIZE_EDGE { -1 } else if x >= width - RESIZE_EDGE { 1 } else { 0 };
        let vertical = if y < RESIZE_EDGE { -1 } else if y >= height - RESIZE_EDGE { 1 } else { 0 };
        if horizontal != 0 || vertical != 0 {
            return Zone::Resize(horizontal, vertical);
        }
    }
    if y < title && x < width - buttons {
        return Zone::Drag;
    }
    Zone::Normal
}

unsafe extern "C" fn hit_test(
    window: *mut video::SDL_Window,
    area: *const sdl3::sys::rect::SDL_Point,
    _data: *mut std::ffi::c_void,
) -> video::SDL_HitTestResult {
    let (mut width, mut height) = (0, 0);
    // SAFETY: SDL passes a live window and a valid point for this call.
    let (point, maximized): (SDL_Point, bool) = unsafe {
        video::SDL_GetWindowSizeInPixels(window, &mut width, &mut height);
        (*area, video::SDL_GetWindowFlags(window).0 & video::SDL_WINDOW_MAXIMIZED.0 != 0)
    };
    let title = TITLE_PIXELS.load(Ordering::Relaxed) as i32;
    let buttons = BUTTON_PIXELS.load(Ordering::Relaxed) as i32;
    match zone(point.x, point.y, width, height, title, buttons, maximized) {
        Zone::Normal => video::SDL_HITTEST_NORMAL,
        Zone::Drag => video::SDL_HITTEST_DRAGGABLE,
        Zone::Resize(-1, -1) => video::SDL_HITTEST_RESIZE_TOPLEFT,
        Zone::Resize(0, -1) => video::SDL_HITTEST_RESIZE_TOP,
        Zone::Resize(1, -1) => video::SDL_HITTEST_RESIZE_TOPRIGHT,
        Zone::Resize(1, 0) => video::SDL_HITTEST_RESIZE_RIGHT,
        Zone::Resize(1, 1) => video::SDL_HITTEST_RESIZE_BOTTOMRIGHT,
        Zone::Resize(0, 1) => video::SDL_HITTEST_RESIZE_BOTTOM,
        Zone::Resize(-1, 1) => video::SDL_HITTEST_RESIZE_BOTTOMLEFT,
        Zone::Resize(..) => video::SDL_HITTEST_RESIZE_LEFT,
    }
}

/// The app's one window. bones keeps the handle, and SDL lists it once started.
fn window() -> *mut video::SDL_Window {
    let mut count = 0;
    // SAFETY: SDL returns a null-terminated list or null, which it asks the caller to free.
    unsafe {
        let list = video::SDL_GetWindows(&mut count);
        if list.is_null() {
            return null_mut();
        }
        let first = if count > 0 { *list } else { null_mut() };
        sdl3::sys::stdinc::SDL_free(list.cast());
        first
    }
}

/// Replaces the system frame. On failure the system frame stays, and the app
/// must not draw a second title bar.
pub fn install() -> bool {
    let window = window();
    if window.is_null() {
        return false;
    }
    // SAFETY: `window` is live, and this runs on the frame thread that owns it.
    unsafe {
        if !video::SDL_SetWindowHitTest(window, Some(hit_test), null_mut()) {
            return false;
        }
        if !video::SDL_SetWindowBordered(window, false) {
            video::SDL_SetWindowHitTest(window, None, null_mut());
            return false;
        }
    }
    true
}

pub fn publish_scale(scale: f32) {
    TITLE_PIXELS.store((TITLE_HEIGHT * scale) as u32, Ordering::Relaxed);
    BUTTON_PIXELS.store((BUTTON_WIDTH * BUTTONS * scale) as u32, Ordering::Relaxed);
}

pub fn is_maximized() -> bool {
    let window = window();
    // SAFETY: `window` is checked and live on its own thread.
    !window.is_null() && unsafe { video::SDL_GetWindowFlags(window).0 & video::SDL_WINDOW_MAXIMIZED.0 != 0 }
}

pub fn minimize() {
    let window = window();
    if !window.is_null() {
        // SAFETY: as above.
        unsafe { video::SDL_MinimizeWindow(window) };
    }
}

pub fn toggle_maximize() {
    let (window, maximized) = (window(), is_maximized());
    if !window.is_null() {
        // SAFETY: as above.
        unsafe {
            if maximized {
                video::SDL_RestoreWindow(window);
            } else {
                video::SDL_MaximizeWindow(window);
            }
        };
    }
}

/// Asks for the same orderly shutdown as the system's close button.
pub fn close() {
    // SAFETY: an all-zero SDL_Event is valid once its type is set, and SDL copies it.
    unsafe {
        let mut event: events::SDL_Event = std::mem::zeroed();
        event.r#type = events::SDL_EVENT_QUIT.0;
        events::SDL_PushEvent(&mut event);
    }
}

/// Draws the bar's name and caption buttons.
pub fn title_bar(ui: &mut egui::Ui) {
    let bar = ui.max_rect();
    ui.painter().text(
        bar.left_center() + egui::vec2(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "cross-wall",
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().strong_text_color(),
    );
    let size = egui::vec2(BUTTON_WIDTH, bar.height());
    let at = |index: f32| egui::Rect::from_min_size(egui::pos2(bar.right() - BUTTON_WIDTH * (BUTTONS - index), bar.top()), size);
    if caption(ui, at(0.0), Icon::Minimize, false) {
        minimize();
    }
    if caption(ui, at(1.0), Icon::Maximize, false) {
        toggle_maximize();
    }
    if caption(ui, at(2.0), Icon::Close, true) {
        close();
    }
}

#[derive(Clone, Copy, Hash)]
enum Icon {
    Minimize,
    Maximize,
    Close,
}

/// Drawn as lines: these glyphs are missing from some system fonts.
fn caption(ui: &mut egui::Ui, rect: egui::Rect, icon: Icon, danger: bool) -> bool {
    let response = ui.interact(rect, ui.id().with(("caption", icon)), egui::Sense::click());
    let mut ink = ui.visuals().text_color();
    if response.hovered() {
        if danger {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(196, 43, 28));
            ink = egui::Color32::WHITE;
        } else {
            ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
    }
    let stroke = egui::Stroke::new(1.2, ink);
    let c = rect.center();
    let r = 5.0;
    let painter = ui.painter();
    match icon {
        Icon::Minimize => {
            painter.hline((c.x - r)..=(c.x + r), c.y, stroke);
        }
        Icon::Maximize => {
            let square = egui::Rect::from_center_size(c, egui::vec2(2.0 * r, 2.0 * r));
            painter.rect_stroke(square, 1.0, stroke, egui::StrokeKind::Middle);
        }
        Icon::Close => {
            painter.line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], stroke);
            painter.line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, -r)], stroke);
        }
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32, maximized: bool) -> Zone {
        zone(x, y, 1000, 800, 40, 150, maximized)
    }

    #[test]
    fn the_title_drags_except_over_its_buttons() {
        assert_eq!(at(300, 20, false), Zone::Drag);
        assert_eq!(at(900, 20, false), Zone::Normal);
        assert_eq!(at(300, 60, false), Zone::Normal);
    }

    #[test]
    fn edges_and_corners_resize() {
        assert_eq!(at(0, 400, false), Zone::Resize(-1, 0));
        assert_eq!(at(999, 799, false), Zone::Resize(1, 1));
        assert_eq!(at(500, 0, false), Zone::Resize(0, -1));
    }

    #[test]
    fn a_maximized_window_has_no_resize_edges() {
        assert_eq!(at(0, 400, true), Zone::Normal);
        assert_eq!(at(500, 0, true), Zone::Drag);
    }
}
