//! Sets the desktop wallpaper on Windows, KDE Plasma, GNOME, and COSMIC.
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    Windows,
    Kde,
    Gnome,
    Cosmic,
}

/// Reads a `XDG_CURRENT_DESKTOP` value, whose entries are `:`-separated, as in `ubuntu:GNOME`.
pub fn desktop_from_xdg(value: &str) -> Option<Desktop> {
    value.split(':').find_map(|name| match name.to_ascii_uppercase().as_str() {
        "COSMIC" => Some(Desktop::Cosmic),
        "KDE" => Some(Desktop::Kde),
        "GNOME" => Some(Desktop::Gnome),
        _ => None,
    })
}

pub fn detect() -> Result<Desktop, String> {
    if cfg!(windows) {
        return Ok(Desktop::Windows);
    }
    let value = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    desktop_from_xdg(&value).ok_or_else(|| {
        format!("This desktop is not supported (XDG_CURRENT_DESKTOP is \"{value}\"). Supported: KDE Plasma, GNOME, COSMIC.")
    })
}

/// Sets `image`, which must be an absolute path that stays in place, on all monitors.
pub fn apply(image: &Path) -> Result<(), String> {
    match detect()? {
        Desktop::Windows => windows(image),
        Desktop::Cosmic => {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
                .ok_or("Cannot find the configuration folder: HOME is not set.")?;
            cosmic(&config, image)
        }
        Desktop::Kde => {
            let mut command = Command::new("plasma-apply-wallpaperimage");
            command.arg(image);
            run(command)
        }
        Desktop::Gnome => {
            for (key, value) in gnome_settings(image) {
                let mut command = Command::new("gsettings");
                command.args(["set", "org.gnome.desktop.background", key, &value]);
                run(command)?;
            }
            Ok(())
        }
    }
}

fn run(mut command: Command) -> Result<(), String> {
    let name = command.get_program().to_string_lossy().into_owned();
    let output = command.output().map_err(|e| format!("Could not run {name}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// GNOME reads a separate key in dark mode, so both must change.
fn gnome_settings(image: &Path) -> [(&'static str, String); 3] {
    let uri = file_uri(image);
    [("picture-uri", uri.clone()), ("picture-uri-dark", uri), ("picture-options", "zoom".to_string())]
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => uri.push(byte as char),
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// Writes what cosmic-bg watches: the `all` entry, used on every output when `same-on-all` is set.
fn cosmic(config_home: &Path, image: &Path) -> Result<(), String> {
    let dir = config_home.join("cosmic/com.system76.CosmicBackground/v1");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = image.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let entry = format!(
        "(\n    output: \"all\",\n    source: Path(\"{path}\"),\n    filter_by_theme: false,\n    rotation_frequency: 300,\n    filter_method: Lanczos,\n    scaling_mode: Zoom,\n    sampling_method: Alphanumeric,\n)"
    );
    for (name, text) in [("same-on-all", "true"), ("all", entry.as_str())] {
        let target = dir.join(name);
        let temporary = dir.join(format!(".{name}.tmp"));
        std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
        std::fs::rename(&temporary, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
}

#[cfg(windows)]
fn windows(image: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
    };
    let mut wide: Vec<u16> = image.as_os_str().encode_wide().collect();
    wide.push(0);
    // SAFETY: `wide` is a NUL-terminated string that outlives the call, which only reads it.
    let done = unsafe {
        SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, wide.as_mut_ptr().cast(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE)
    };
    if done == 0 {
        return Err(format!("Windows refused the wallpaper: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn windows(_image: &Path) -> Result<(), String> {
    Err("Not running on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_desktops_from_xdg_values() {
        assert_eq!(desktop_from_xdg("ubuntu:GNOME"), Some(Desktop::Gnome));
        assert_eq!(desktop_from_xdg("KDE"), Some(Desktop::Kde));
        assert_eq!(desktop_from_xdg("COSMIC"), Some(Desktop::Cosmic));
        assert_eq!(desktop_from_xdg("pop:COSMIC"), Some(Desktop::Cosmic));
        assert_eq!(desktop_from_xdg("XFCE"), None);
        assert_eq!(desktop_from_xdg(""), None);
    }

    #[test]
    fn gnome_gets_both_modes_and_an_encoded_uri() {
        let settings = gnome_settings(Path::new("/home/a b/é.jpg"));
        assert_eq!(settings[0], ("picture-uri", "file:///home/a%20b/%C3%A9.jpg".to_string()));
        assert_eq!(settings[1].0, "picture-uri-dark");
        assert_eq!(settings[1].1, settings[0].1);
    }

    #[test]
    fn cosmic_writes_the_entry_and_escapes_the_path() {
        let dir = tempfile::tempdir().unwrap();
        cosmic(dir.path(), Path::new("/home/a\"b.jpg")).unwrap();
        let base = dir.path().join("cosmic/com.system76.CosmicBackground/v1");
        let all = std::fs::read_to_string(base.join("all")).unwrap();
        assert!(all.contains(r#"source: Path("/home/a\"b.jpg")"#), "{all}");
        assert!(all.contains("scaling_mode: Zoom"));
        assert_eq!(std::fs::read_to_string(base.join("same-on-all")).unwrap(), "true");
    }
}
