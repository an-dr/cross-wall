//! `cargo xtask package --platform <p> [--target <triple>] [--tag v<version>]`
//! builds a release and writes `release/cross-wall-<version>-<p>.zip` with the
//! `cross-wall-<p>.json` manifest the self-updater reads.
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use zip::{write::SimpleFileOptions, ZipWriter};

const PLATFORMS: [&str; 3] = ["windows-x64", "windows-arm64", "linux-x64"];
const BINARIES: [&str; 2] = ["cross-wall-app", "cross-wall-launcher"];
const REPOSITORY: &str = "an-dr/cross-wall";

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("package") {
        return Err(
            "usage: cargo xtask package --platform <platform> [--target <triple>] [--tag <tag>]"
                .into(),
        );
    }
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let platform = option("--platform").ok_or("--platform is required")?;
    if !PLATFORMS.contains(&platform.as_str()) {
        return Err(format!("unsupported platform {platform}; use one of {PLATFORMS:?}").into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let version = app_version(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let launcher = launcher_version(&fs::read_to_string(root.join(".cargo/config.toml"))?)?;
    if launcher != version {
        let message = format!(
            "UPGRADER_LAUNCHER_VERSION in .cargo/config.toml is {launcher} but the app is {version}; set both to the same value"
        );
        return Err(message.into());
    }
    let tag = format!("v{version}");
    if option("--tag").is_some_and(|given| given != tag) {
        return Err(format!("release tag must match the app version: {tag}").into());
    }
    let target = option("--target");
    build(&root, target.as_deref())?;
    let bin_dir = match &target {
        Some(triple) => root.join("target").join(triple).join("release"),
        None => root.join("target/release"),
    };
    let suffix = if platform.starts_with("windows") {
        ".exe"
    } else {
        ""
    };
    let output = root.join("release");
    fs::create_dir_all(&output)?;
    let zip_name = format!("cross-wall-{version}-{platform}.zip");
    let zip_path = output.join(&zip_name);
    write_zip(&bin_dir, suffix, &zip_path)?;
    let mut hash = Sha256::new();
    io::copy(&mut File::open(&zip_path)?, &mut hash)?;
    let sha256 = format!("{:x}", hash.finalize());
    let manifest = serde_json::json!({
        "version": version,
        "url": format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{zip_name}"),
        "sha256": sha256,
    });
    fs::write(
        output.join(format!("cross-wall-{platform}.json")),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    println!("{}", zip_path.display());
    Ok(())
}

fn build(root: &Path, target: Option<&str>) -> Result {
    let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo.current_dir(root).args([
        "build",
        "--release",
        "-p",
        "cross-wall",
        "-p",
        "cross-wall-launcher",
    ]);
    if let Some(triple) = target {
        cargo.args(["--target", triple]);
    }
    if !cargo.status()?.success() {
        return Err("cargo build failed".into());
    }
    Ok(())
}

fn write_zip(bin_dir: &Path, suffix: &str, path: &Path) -> Result {
    let mut zip = ZipWriter::new(File::create(path)?);
    // Linux needs the execute bit to survive the archive.
    let options = SimpleFileOptions::default().unix_permissions(0o755);
    for name in BINARIES {
        let file = format!("{name}{suffix}");
        zip.start_file(file.as_str(), options)?;
        io::copy(&mut File::open(bin_dir.join(&file))?, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

fn app_version(manifest: &str) -> Result<String> {
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(String::from)
        .ok_or_else(|| "no version in the workspace manifest".into())
}

/// The version the launcher reports to the updater, from `.cargo/config.toml`.
fn launcher_version(config: &str) -> Result<String> {
    config
        .lines()
        .find_map(|line| line.trim().strip_prefix("UPGRADER_LAUNCHER_VERSION"))
        .and_then(|rest| rest.trim().strip_prefix('='))
        .map(|value| value.trim().trim_matches('"').to_string())
        .ok_or_else(|| "no UPGRADER_LAUNCHER_VERSION in .cargo/config.toml".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_package_version() {
        assert_eq!(
            app_version("[package]\nname = \"a\"\nversion = \"1.2.3\"\n").unwrap(),
            "1.2.3"
        );
        assert!(app_version("[package]\n").is_err());
    }

    #[test]
    fn reads_the_launcher_version() {
        assert_eq!(
            launcher_version("[env]\nUPGRADER_LAUNCHER_VERSION = \"0.2.0\"\n").unwrap(),
            "0.2.0"
        );
        assert!(launcher_version("[env]\n").is_err());
    }

    /// Fails `cargo test` as soon as the two versions drift, long before a release.
    #[test]
    fn the_repository_keeps_both_versions_equal() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let app = app_version(&fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
        let launcher =
            launcher_version(&fs::read_to_string(root.join(".cargo/config.toml")).unwrap())
                .unwrap();
        assert_eq!(
            app, launcher,
            "set UPGRADER_LAUNCHER_VERSION in .cargo/config.toml to the app version"
        );
    }
}
