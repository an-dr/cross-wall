//! This app's install identity, desktop registration, and the bridge between
//! the screens and the bones upgrade module, which does the actual work.
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use bones_kernel::bus::{Bus, Envelope, Handler, Module, ModuleContext};
use bones_kernel::desktop::{Integration, Locations};
use bones_kernel::messages::upgrader::{Action, Outcome, Request, Result as UpgradeResult};
use bones_kernel::messages::{DecodeMessage, EncodeMessage, Message};
use bones_kernel::upgrade::{AppIdentity, ReleaseSource, UpgradeConfig, UpgradeModule};

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn identity() -> AppIdentity {
    AppIdentity {
        install_dir_name: ".cross-wall",
        install_env_var: "CROSS_WALL_INSTALL_DIR",
        launcher_stem: "cross-wall",
        app_stem: "cross-wall-app",
    }
}

pub fn config() -> UpgradeConfig {
    let source = ReleaseSource::GitHub { owner: "an-dr".into(), repo: "cross-wall".into(), app: "cross-wall".into() };
    UpgradeConfig::new(identity(), source, VERSION)
}

pub fn install_dir() -> Option<PathBuf> {
    UpgradeModule::new(config()).install_dir()
}

/// Whether this process runs from the install folder, not from an unpacked build.
pub fn running_installed() -> bool {
    match (std::env::current_exe(), install_dir()) {
        (Ok(exe), Some(dir)) => exe.starts_with(dir),
        _ => false,
    }
}

/// Updates the entry point to the launcher this version carries.
pub fn refresh_launcher() {
    if running_installed() {
        let _ = UpgradeModule::new(config()).refresh_launcher();
    }
}

fn integration() -> Result<(Integration<'static>, Locations), String> {
    let dir = install_dir().ok_or("Could not find the install folder.")?;
    let locations = Locations::for_current_user().ok_or("Registration is not available on this system.")?;
    Ok((
        Integration {
            id: "cross-wall",
            name: "cross-wall",
            comment: "Browse Pexels photos and set them as your wallpaper",
            open_label: "Open in cross-wall",
            categories: "Graphics;Utility;",
            // The launcher path survives updates; a version folder's does not.
            executable: dir.join(identity().launcher_exe()),
            icon_png: ICON_PNG,
        },
        locations,
    ))
}

pub fn register() -> Result<(), String> {
    let (integration, locations) = integration()?;
    bones_kernel::desktop::register(&integration, &locations).map_err(|e| e.to_string())
}

pub fn unregister() -> Result<(), String> {
    let (integration, locations) = integration()?;
    bones_kernel::desktop::unregister(&integration, &locations).map_err(|e| e.to_string())
}

pub fn is_registered() -> bool {
    integration().is_ok_and(|(i, l)| bones_kernel::desktop::is_registered(&i, &l))
}

/// `--install` and `--uninstall`, for the install scripts. `None` means start the app.
pub fn command_line(args: &[String]) -> Option<Result<String, String>> {
    match args.first().map(String::as_str) {
        Some("--install") => Some(UpgradeModule::new(config()).install().and_then(|_| {
            register()?;
            Ok(format!("Installed to {}", install_dir().unwrap_or_default().display()))
        })),
        Some("--uninstall") => Some(unregister().map(|()| {
            format!("Removed from the application list. Delete {} to finish.", install_dir().unwrap_or_default().display())
        })),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Available(String),
    UpToDate,
    Staged(String),
    Installed,
    Failed(String),
}

/// Carries requests onto the bus and results back to the screens.
pub struct UpdateBridge {
    bus: Option<Bus>,
    requests: Receiver<Action>,
    results: Sender<UpdateState>,
    next_id: u32,
}

impl UpdateBridge {
    pub fn new(requests: Receiver<Action>, results: Sender<UpdateState>) -> Self {
        Self { bus: None, requests, results, next_id: 1 }
    }
}

impl Module for UpdateBridge {
    fn name(&self) -> &str {
        "update-bridge"
    }

    fn init(&mut self, ctx: &mut ModuleContext) -> Result<(), String> {
        self.bus = Some(ctx.get_service::<Bus>().ok_or("the kernel provides no bus")?.clone());
        ctx.subscribe(UpgradeResult::TOPIC);
        Ok(())
    }

    fn render(&mut self) {
        while let Ok(action) = self.requests.try_recv() {
            let request = Request { request_id: self.next_id, action };
            self.next_id += 1;
            if let Some(bus) = &self.bus {
                bus.publish(Envelope {
                    topic: Request::TOPIC.to_string(),
                    sender: "update-bridge".to_string(),
                    correlation: None,
                    payload: request.encode(),
                });
            }
        }
    }
}

impl Handler for UpdateBridge {
    fn handle(&mut self, envelope: &Envelope) {
        let Ok(result) = UpgradeResult::decode(&envelope.payload) else { return };
        let state = match result.outcome {
            Outcome::Available { version } => UpdateState::Available(version.into()),
            Outcome::UpToDate => UpdateState::UpToDate,
            Outcome::Staged { version } => UpdateState::Staged(version.into()),
            Outcome::Installed { .. } => UpdateState::Installed,
            Outcome::Failed { reason } => UpdateState::Failed(reason.into()),
            Outcome::LastApplied { .. } => return,
        };
        let _ = self.results.send(state);
    }
}
