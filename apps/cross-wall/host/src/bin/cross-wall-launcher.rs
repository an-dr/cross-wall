//! The permanent entry point, installed at the install root as `cross-wall`.
//! All the work is `bones_launcher`'s: it starts the newest installed version.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bones_launcher::report_error;

fn main() {
    let Some(install_dir) = std::env::current_exe().ok().and_then(|path| path.parent().map(std::path::Path::to_path_buf)) else {
        report_error("could not resolve the launcher's own directory\n");
        std::process::exit(1);
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = if cfg!(windows) { "cross-wall-app.exe" } else { "cross-wall-app" };
    if let Err(error) = bones_launcher::run(app, &install_dir, &args) {
        report_error(&format!("{error}\n"));
        std::process::exit(1);
    }
}
