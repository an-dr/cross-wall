//! The Browse screen: search a source, filter the results, and scroll for more.
use bones_kernel::messages::ui::kinds::{
    EmptyState, Hyperlink, IconButton, Label, Select, Space, Spinner, TextEdit,
};
use bones_kernel::messages::ui::{Event, Layout, Node, WidgetProps};
use cross_wall_sources::{MinSize, Orientation, Source};

use super::kit::{stable_id, Kit};
use super::shared::Shared;
use super::shown::Shown;
use super::CrossWall;
use crate::browse::Browse;
use crate::settings::Settings;

const RESOLUTIONS: [(u32, u32); 5] = [
    (1280, 720),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
    (5120, 2880),
];
const ORIENTATIONS: [Orientation; 4] = [
    Orientation::Any,
    Orientation::Landscape,
    Orientation::Portrait,
    Orientation::Square,
];

pub struct BrowseView {
    browse: Browse,
}

impl BrowseView {
    pub fn new() -> Self {
        Self {
            browse: Browse::new(),
        }
    }

    pub fn poll(&mut self, shared: &Shared) {
        self.browse.poll(&shared.client);
    }

    /// Starts a new search with the current filters, dropping old thumbnails.
    fn refresh(&mut self, shared: &mut Shared) {
        shared.images.forget("thumb/remote:");
        self.browse.search(&shared.client);
    }
}

impl CrossWall {
    pub fn browse_view(&mut self, kit: &mut Kit) -> Vec<Node> {
        let min = effective_min(self.shared.settings(), self.display_size);
        let browse = &mut self.browse.browse;
        browse.min = min;
        browse.start_once(&self.shared.client);

        let mut nodes = vec![self.browse_toolbar(kit), Space { amount: 6.0 }.node(0)];
        let browse = &self.browse.browse;
        nodes.extend(self.progress_strip(browse.loading));
        let browse = &self.browse.browse;
        if browse.photos.is_empty() && browse.loading {
            nodes.push(
                EmptyState {
                    icon: "compass".into(),
                    title: "Finding wallpapers".into(),
                    detail: "Asking the source for photos...".into(),
                }
                .node(0),
            );
            return nodes;
        }
        if browse.photos.is_empty() && !browse.message.is_empty() {
            nodes.push(
                EmptyState {
                    icon: "compass".into(),
                    title: "No wallpapers to show".into(),
                    detail: browse.message.clone(),
                }
                .node(0),
            );
            if browse.has_more {
                let id = kit.on(|app, _| app.browse.browse.load_more(&app.shared.client));
                nodes.push(Node::container(
                    Layout::Centred,
                    vec![IconButton::new("rotate-cw", "Try again").node(id)],
                ));
            }
            return nodes;
        }
        let items: Vec<Shown> = browse.photos.iter().cloned().map(Shown::Remote).collect();
        let message = browse.message.clone();
        let (loading, stalled, source) = (browse.loading, browse.is_stalled(), browse.source);
        self.shared.shown = items.clone();
        let mut list = vec![self.gallery(kit, &items)];
        if !message.is_empty() {
            list.push(Space { amount: 8.0 }.node(0));
            list.push(Node::container(
                Layout::Centred,
                vec![Label { text: message }.node(0)],
            ));
        }
        let mut footer = vec![Space { amount: 8.0 }.node(0)];
        if loading {
            footer.push(Spinner.node(0));
        } else if stalled {
            let id = kit.on(|app, _| app.browse.browse.load_more(&app.shared.client));
            footer.push(IconButton::new("rotate-cw", "Try again").node(id));
        }
        footer.push(
            Hyperlink {
                label: format!("Photos provided by {}", source.name()),
                url: source.home().to_string(),
            }
            .node(0),
        );
        footer.push(Space { amount: 8.0 }.node(0));
        list.push(Node::container(Layout::Centred, footer));
        // Loads more before the very end, so scrolling rarely reaches a gap.
        let scroll = kit.keyed("browse-scroll", |app, event| {
            if let Event::ReachedEnd { .. } = event {
                if app.browse.browse.can_auto_load() {
                    app.browse.browse.load_more(&app.shared.client);
                }
            }
        });
        nodes.push(Node::Container {
            id: scroll,
            layout: Layout::Scroll,
            children: list,
        });
        nodes
    }

    fn browse_toolbar(&mut self, kit: &mut Kit) -> Node {
        let resolution = self.resolution_menu(kit);
        let browse = &self.browse.browse;
        let searchable = browse.source.has_search();
        let hint = if searchable {
            "Search wallpapers"
        } else {
            "This source has no search"
        };
        let search = kit.keyed("browse-search", |app, event| match event {
            Event::Changed { text, .. } => app.browse.browse.text.clone_from(text),
            Event::Submitted { .. } => app.browse.refresh(&mut app.shared),
            _ => {}
        });
        let source = kit.keyed("browse-source", |app, event| {
            if let Event::Selected { index, .. } = event {
                if let Some(source) = Source::ALL.get(*index as usize) {
                    app.browse.browse.source = *source;
                    app.browse.refresh(&mut app.shared);
                }
            }
        });
        let orientation = kit.keyed("browse-orientation", |app, event| {
            if let Event::Selected { index, .. } = event {
                if let Some(o) = ORIENTATIONS.get(*index as usize) {
                    app.browse.browse.orientation = *o;
                    app.browse.refresh(&mut app.shared);
                }
            }
        });
        Node::container(
            Layout::Wrap,
            vec![
                TextEdit::new(browse.text.clone())
                    .hint(hint)
                    .disabled(!searchable)
                    .node(search),
                Select {
                    selected: Source::ALL
                        .iter()
                        .position(|s| *s == browse.source)
                        .map(|i| i as u32),
                    placeholder: String::new(),
                    options: Source::ALL.iter().map(|s| s.name().to_string()).collect(),
                }
                .node(source),
                Select {
                    selected: ORIENTATIONS
                        .iter()
                        .position(|o| *o == browse.orientation)
                        .map(|i| i as u32),
                    placeholder: String::new(),
                    options: ORIENTATIONS
                        .iter()
                        .map(|o| orientation_name(*o).to_string())
                        .collect(),
                }
                .node(orientation),
                resolution,
            ],
        )
    }

    /// The minimum-resolution choice: the main display, any size, or a size.
    fn resolution_menu(&mut self, kit: &mut Kit) -> Node {
        let display_size = self.display_size;
        let mut choices: Vec<Option<[u32; 2]>> = vec![None, Some([0, 0])];
        choices.extend(RESOLUTIONS.iter().map(|&(w, h)| Some([w, h])));
        let display = display_size.map_or("Main display".to_string(), |(w, h)| {
            format!("Main display ({w} \u{d7} {h})")
        });
        let mut options = vec![display, "Any size".to_string()];
        options.extend(RESOLUTIONS.iter().map(|(w, h)| format!("{w} \u{d7} {h}")));
        let chosen = self.shared.settings().min_resolution;
        let selected = choices.iter().position(|c| *c == chosen).map(|i| i as u32);
        let id = kit.keyed(stable_id("browse-resolution"), move |app, event| {
            if let Event::Selected { index, .. } = event {
                let Some(choice) = choices.get(*index as usize).copied() else {
                    return;
                };
                if app.shared.store.change(|s| s.min_resolution = choice) {
                    app.browse.browse.min = effective_min(app.shared.settings(), display_size);
                    app.browse.refresh(&mut app.shared);
                }
            }
        });
        Select {
            selected,
            placeholder: resolution_label(chosen, display_size),
            options,
        }
        .node(id)
    }
}

/// The minimum size asked for: the chosen one, or the main display's.
fn effective_min(settings: &Settings, display_size: Option<(u32, u32)>) -> MinSize {
    let (width, height) = match settings.min_resolution {
        Some([width, height]) => (width, height),
        None => display_size.unwrap_or((0, 0)),
    };
    MinSize { width, height }
}

fn resolution_label(min_resolution: Option<[u32; 2]>, display_size: Option<(u32, u32)>) -> String {
    match (min_resolution, display_size) {
        (None, Some((w, h))) => format!("\u{2265} {w} \u{d7} {h}"),
        (None, None) | (Some([0, 0]), _) => "Any size".to_string(),
        (Some([w, h]), _) => format!("\u{2265} {w} \u{d7} {h}"),
    }
}

fn orientation_name(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Any => "Any shape",
        Orientation::Landscape => "Landscape",
        Orientation::Portrait => "Portrait",
        Orientation::Square => "Square",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minimum_follows_the_display_unless_chosen() {
        let mut settings = Settings::default();
        assert_eq!(
            effective_min(&settings, Some((2560, 1440))),
            MinSize {
                width: 2560,
                height: 1440
            }
        );
        assert_eq!(effective_min(&settings, None), MinSize::ANY);
        settings.min_resolution = Some([1920, 1080]);
        assert_eq!(
            effective_min(&settings, Some((2560, 1440))),
            MinSize {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(resolution_label(Some([0, 0]), Some((1, 1))), "Any size");
    }
}
