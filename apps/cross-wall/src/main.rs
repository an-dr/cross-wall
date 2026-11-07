// A release build is a desktop app, so Windows must not open a console behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod browse;
mod chrome;
mod clock;
mod colour;
mod egui_module;
mod gallery;
mod icons;
mod images;
mod installation;
mod librarian;
mod logs;
mod settings;
mod theme;

use std::sync::mpsc::channel;

use bones_kernel::logging::Logger;

/// Release builds have no console, so failures must reach a file to be diagnosable.
fn record_failure(text: &str) {
    use std::io::Write;
    let folder = settings::data_dir();
    let _ = std::fs::create_dir_all(&folder);
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("crash.log"))
    {
        let _ = writeln!(log, "[{} ms since epoch] {text}\n", clock::now());
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        let text = format!("panic on thread {:?}: {info}\n{backtrace}", thread.name());
        eprintln!("{text}");
        record_failure(&text);
    }));
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
        record_failure(&format!("fatal: {error}"));
        std::process::exit(1);
    }
}

fn run() -> bones_kernel::Result<()> {
    installation::refresh_launcher();
    let logger = Logger::default().tee(std::sync::Arc::new(logs::FileSink));
    let (request_tx, request_rx) = channel();
    let (result_tx, result_rx) = channel();
    let screens = app::CrossWall::new(
        settings::settings_path(),
        settings::data_dir(),
        request_tx,
        result_rx,
    );
    bones_kernel::Kernel::new()
        .logger(logger.clone())
        .window(installation::NAME, 1200, 800)
        .renderer()
        .upgrade(installation::config())
        .module(installation::UpdateBridge::new(request_rx, result_tx))
        .module(egui_module::EguiModule::new(screens, logger))
        .run()
}
