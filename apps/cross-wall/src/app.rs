//! The window: side navigation and the screens, composed. Each screen keeps its
//! own state in its own module and shares the services in `shared`. Slow work
//! runs on threads; results return through channels drained each frame, so the
//! frame thread never waits on the network or the disk.
//!
//! Every frame the app describes its interface as a bones widget tree and
//! handles the events bones returns (ADR-004); bones draws it.
mod apply_worker;
mod browse_view;
mod kit;
mod library_view;
mod organise;
mod preview;
mod settings_view;
mod shared;
mod shown;
mod tiles;
mod toast;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use bones_kernel::bus::{Envelope, Handler, Module, ModuleContext};
use bones_kernel::messages::ui::kinds::{Selectable, TitleBar};
use bones_kernel::messages::ui::{icons, Event, Layout, Node, PanelSide, Root, Spec, WidgetProps};
use bones_kernel::messages::upgrader::Action;
use bones_kernel::ui::{Theme, UiPort, UiService};

use browse_view::BrowseView;
use kit::Kit;
use library_view::LibraryView;
use preview::Preview;
use settings_view::SettingsView;
use shared::Shared;
use toast::Toast;

use crate::installation::{self, UpdateState};
use crate::logs;
use crate::settings::{Store, ThemeMode};

/// The name the app's trees and images are shown under.
const OWNER: &str = "cross-wall";
/// A second owner for dialogs, which bones draws above the screen.
const DIALOG_OWNER: &str = "cross-wall/dialog";
/// The app icon's image handle, far from the ones `Images` hands out.
const ICON_HANDLE: u32 = u32::MAX;

#[derive(PartialEq, Clone, Copy, Hash)]
enum Tab {
    Browse,
    Library,
    Liked,
    Settings,
}

pub struct CrossWall {
    tab: Tab,
    shared: Shared,
    browse: BrowseView,
    library: LibraryView,
    settings: SettingsView,
    preview: Preview,
    toast: Toast,
    /// The main display's resolution, read once the window exists.
    display_size: Option<(u32, u32)>,
    /// What each draggable tile and folder carries, by its container id.
    dragged: HashMap<u32, String>,
}

impl CrossWall {
    pub fn new(
        settings_path: PathBuf,
        data_dir: PathBuf,
        update_requests: Sender<Action>,
        update_results: Receiver<UpdateState>,
    ) -> Self {
        let store = Store::open(settings_path);
        logs::set_enabled(store.get().logging.unwrap_or_else(logs::default_enabled));
        Self {
            tab: Tab::Browse,
            shared: Shared::new(store, data_dir),
            browse: BrowseView::new(),
            library: LibraryView::default(),
            settings: SettingsView::new(update_requests, update_results),
            preview: Preview::default(),
            toast: Toast::default(),
            display_size: None,
            dragged: HashMap::new(),
        }
    }

    fn poll(&mut self) {
        self.shared.poll();
        self.browse.poll(&self.shared);
        self.settings.poll(&mut self.shared);
    }

    /// A key no widget took: the preview's arrows and Escape.
    fn on_key(&mut self, key: &str) {
        self.preview.on_key(&mut self.shared, key);
    }

    /// The whole window for this frame.
    fn view(&mut self, kit: &mut Kit) -> Spec {
        self.dragged.clear();
        let title = Node::Container {
            id: 0,
            layout: Layout::Panel {
                side: PanelSide::Top,
            },
            children: vec![TitleBar {
                title: installation::NAME.into(),
                icon: ICON_HANDLE,
            }
            .node(0)],
        };
        let navigation = Node::Container {
            id: 0,
            layout: Layout::Panel {
                side: PanelSide::Left,
            },
            children: self.navigation(kit),
        };
        let screen = match self.tab {
            Tab::Browse => self.browse_view(kit),
            Tab::Library => self.library_view(kit),
            Tab::Liked => self.liked_view(kit),
            Tab::Settings => self.settings_view(kit),
        };
        let content = Node::Container {
            id: kit::stable_id(("tab", self.tab)),
            layout: Layout::Fade,
            children: screen,
        };
        let mut central = vec![content];
        central.extend(self.preview_view(kit));
        central.extend(self.toast.view(kit, &mut self.shared));
        let central = Node::Container {
            id: 0,
            layout: Layout::Panel {
                side: PanelSide::Central,
            },
            children: central,
        };
        Spec {
            root: Root::Screen,
            body: Node::container(Layout::Vertical, vec![title, navigation, central]),
        }
    }

    /// The side navigation.
    fn navigation(&mut self, kit: &mut Kit) -> Vec<Node> {
        let files = &self.shared.snapshot.files;
        let liked = files.iter().filter(|e| e.info.liked).count();
        let items = [
            (Tab::Browse, icons::COMPASS, "Browse".to_string()),
            (
                Tab::Library,
                icons::FOLDER,
                format!("Library  \u{b7}  {}", files.len()),
            ),
            (Tab::Liked, icons::HEART, format!("Liked  \u{b7}  {liked}")),
            (Tab::Settings, icons::SETTINGS, "Settings".to_string()),
        ];
        items
            .into_iter()
            .map(|(tab, icon, label)| {
                let id = kit.on(move |app, _| app.tab = tab);
                Selectable {
                    label: format!("{icon}   {label}"),
                    selected: self.tab == tab,
                }
                .node(id)
            })
            .collect()
    }

    fn theme(&self) -> Theme {
        match self.shared.settings().theme {
            ThemeMode::Dark => Theme::Dark,
            ThemeMode::Light => Theme::Light,
            ThemeMode::System => Theme::System,
        }
    }
}

/// Runs the app as a bones native module: each frame it hands the app the
/// events of the last frame's widgets, then shows the tree the app builds.
pub struct AppModule {
    app: CrossWall,
    kit: Kit,
    port: Option<UiService>,
    started: bool,
}

impl AppModule {
    pub fn new(app: CrossWall) -> Self {
        Self {
            app,
            kit: Kit::default(),
            port: None,
            started: false,
        }
    }

    /// Once the window exists: its icon, the display size, and the icon image.
    fn start(&mut self, port: &mut dyn UiPort) {
        self.started = true;
        crate::window::set_icon();
        self.app.display_size = crate::window::primary_display_size();
        if let Ok(icon) = image::load_from_memory(crate::assets::ICON_PNG) {
            let icon = icon.thumbnail(64, 64).to_rgba8();
            let (width, height) = (icon.width(), icon.height());
            port.set_image(OWNER, ICON_HANDLE, width, height, icon.into_raw());
        }
    }
}

impl Handler for AppModule {
    fn handle(&mut self, _envelope: &Envelope) {}
}

impl Module for AppModule {
    fn name(&self) -> &str {
        OWNER
    }

    fn init(&mut self, ctx: &mut ModuleContext) -> Result<(), String> {
        self.port = Some(
            ctx.get_service::<UiService>()
                .cloned()
                .ok_or("the app needs bones' ui service (compose .ui() first)")?,
        );
        Ok(())
    }

    fn after_tick(&mut self) {
        let Some(port) = self.port.clone() else {
            return;
        };
        let mut port = port.lock().unwrap();
        if !self.started {
            self.start(&mut *port);
        }
        let mut events = port.take_events(OWNER);
        events.extend(port.take_events(DIALOG_OWNER));
        for event in events {
            if let Event::KeyPressed { key } = &event {
                self.app.on_key(key);
            } else if let Some(action) = self.kit.take(event.id()) {
                action(&mut self.app, &event);
            }
        }
        self.app.poll();
        self.app.shared.images.poll(&mut *port, OWNER);
        let mut kit = Kit::default();
        let spec = self.app.view(&mut kit);
        port.show(OWNER, spec);
        if let Some(dialog) = self.app.dialog_view(&mut kit) {
            port.show(DIALOG_OWNER, dialog);
        }
        port.set_zoom(self.app.shared.settings().ui_zoom);
        port.set_theme(self.app.theme());
        self.kit = kit;
    }
}
