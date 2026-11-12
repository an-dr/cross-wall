//! The two things the app asks of its window directly: its icon in the
//! taskbar, and the main display's size for the minimum-resolution filter.
use std::ptr::null_mut;

use sdl3::sys::video;

use crate::assets::ICON_PNG;

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

/// Gives the window the app's icon, which the taskbar and window switcher show.
/// SDL copies the pixels. A failure only leaves the default icon.
pub fn set_icon() {
    let (window, Ok(icon)) = (window(), image::load_from_memory(ICON_PNG)) else {
        return;
    };
    if window.is_null() {
        return;
    }
    let mut icon = icon.to_rgba8();
    let (width, height) = (icon.width() as i32, icon.height() as i32);
    // SAFETY: the surface borrows `icon`, which outlives it, and the window is live on its own thread.
    unsafe {
        let surface = sdl3::sys::surface::SDL_CreateSurfaceFrom(
            width,
            height,
            sdl3::sys::pixels::SDL_PIXELFORMAT_RGBA32,
            icon.as_mut_ptr().cast(),
            width * 4,
        );
        if !surface.is_null() {
            video::SDL_SetWindowIcon(window, surface);
            sdl3::sys::surface::SDL_DestroySurface(surface);
        }
    }
}

/// The main display's resolution in physical pixels, once SDL has started.
pub fn primary_display_size() -> Option<(u32, u32)> {
    // SAFETY: called on the frame thread after the kernel has started SDL's
    // video system. SDL owns the returned mode, which is read at once and checked for null.
    unsafe {
        let mode = video::SDL_GetDesktopDisplayMode(video::SDL_GetPrimaryDisplay());
        if mode.is_null() {
            return None;
        }
        let density = (*mode).pixel_density.max(1.0);
        Some((
            ((*mode).w as f32 * density).round() as u32,
            ((*mode).h as f32 * density).round() as u32,
        ))
    }
}
