# Desktop support

The app copies the image to `<data folder>/cross-wall/wallpaper/` and points the desktop at that copy, so unliking the photo leaves the wallpaper in place.

| Desktop | How it is set | Needs |
| --- | --- | --- |
| Windows | `SystemParametersInfoW` | Nothing extra |
| KDE Plasma | `plasma-apply-wallpaperimage` | Plasma 5.24 or newer |
| GNOME | `gsettings`, light and dark keys | `gsettings` |
| COSMIC | Writes the `all` and `same-on-all` entries of `com.system76.CosmicBackground/v1`, which `cosmic-bg` watches | Nothing extra |

On Linux the desktop is read from `XDG_CURRENT_DESKTOP`. Any other desktop gets a message naming that value.

Applying replaces the COSMIC setting on all monitors and turns its per-monitor choice off. On Windows the image keeps the fit you last set in Windows Settings.

## Appearance

The interface is drawn by egui, so it resembles each desktop without using its real controls ([ADR-001](adr/ADR-001-app-owned-egui-module-over-draw-target.md)). It takes these from the system, checking mode and accent every 5 seconds and display scale every second:

| Desktop | Font | Light or dark | Accent |
| --- | --- | --- | --- |
| Windows | Segoe UI | `AppsUseLightTheme` | `AccentColor` |
| KDE Plasma | `kdeglobals` font | `kdeglobals` window colour | `kdeglobals` selection colour |
| GNOME | `font-name` | `color-scheme` | `accent-color` |
| COSMIC | Open Sans | `is_dark` | COSMIC's standard accent, not the user's choice |

The system frame is replaced by the app's own title bar. Moving, resizing, and double-clicking it still go through the window manager, and if the desktop refuses, the system frame stays. Spacing and corners follow each desktop's usual proportions. Light or dark from the system applies only when Theme in Settings is System; the default is Dark. Any desktop not listed uses the GNOME look. On Wayland the compositor scales the window, which can look slightly soft on a high-DPI screen. Interface zoom in Settings adjusts the result on any desktop.
