//! The latest message, shown over the window for a few seconds.
use std::time::{Duration, Instant};

use bones_kernel::messages::ui::kinds::Toast as ToastWidget;
use bones_kernel::messages::ui::{Layout, Node, WidgetProps};

use super::kit::{stable_id, Kit};
use super::shared::Shared;
use crate::logs;

const TOAST_TIME: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct Toast {
    /// The message on screen and when it appeared.
    shown: Option<(String, Instant)>,
}

impl Toast {
    /// The toast for this frame, if a message is current.
    pub fn view(&mut self, kit: &mut Kit, shared: &mut Shared) -> Option<Node> {
        let applying = shared.apply.is_busy();
        if applying {
            shared.status = shared.apply.progress_text();
        }
        let changed = self.shown.as_ref().map(|(text, _)| text) != Some(&shared.status);
        if changed {
            if !applying && !shared.status.is_empty() {
                logs::info("status", &shared.status);
            }
            self.shown = Some((shared.status.clone(), Instant::now()));
        }
        if shared.status.is_empty() {
            return None;
        }
        let started = self.shown.as_ref().map(|(_, at)| *at)?;
        if !applying && started.elapsed() > TOAST_TIME {
            shared.status.clear();
            return None;
        }
        let close = kit.on(|app, _| app.shared.status.clear());
        let toast = ToastWidget {
            text: shared.status.clone(),
        }
        .node(close);
        // A new message fades in. Applying updates its text as it goes, which
        // is one toast, so it is not wrapped and does not flicker.
        if applying {
            return Some(toast);
        }
        Some(Node::Container {
            id: stable_id("toast"),
            layout: Layout::Fade,
            children: vec![toast],
        })
    }
}
