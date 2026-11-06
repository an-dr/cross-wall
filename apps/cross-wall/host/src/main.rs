// A release build is a desktop app, so Windows must not open a console behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod browse;
mod egui_module;
mod images;
mod installation;
mod librarian;
mod settings;
mod theme;

use std::sync::mpsc::channel;

use bones_kernel::logging::Logger;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(result) = installation::command_line(&args) {
        match result {
            Ok(message) => println!("{message}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Err(error) = run() {
        eprintln!("fatal: {error}");
        std::process::exit(1);
    }
}

fn run() -> bones_kernel::Result<()> {
    installation::refresh_launcher();
    let logger = Logger::default();
    let (request_tx, request_rx) = channel();
    let (result_tx, result_rx) = channel();
    let screens = app::CrossWall::new(settings::settings_path(), settings::data_dir(), request_tx, result_rx);
    bones_kernel::Kernel::new()
        .logger(logger.clone())
        .window("cross-wall", 1200, 800)
        .renderer()
        .upgrade(installation::config())
        .module(installation::UpdateBridge::new(request_rx, result_tx))
        .module(egui_module::EguiModule::new(screens, logger))
        .run()
}
