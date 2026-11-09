//! Building a frame's widget tree: every interactive widget gets an id, and
//! the id leads back to what should happen when bones reports an event for
//! it. The tree is rebuilt each frame from the app's state (bones ADR-048).
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use bones_kernel::messages::ui::Event;

use super::CrossWall;

/// What to do when a widget reports an event; it gets the app and the event.
pub type Action = Box<dyn FnOnce(&mut CrossWall, &Event) + Send>;

/// The actions of the frame being built, by widget id.
#[derive(Default)]
pub struct Kit {
    actions: HashMap<u32, Action>,
    next: u32,
}

/// Ids below this are handed out in order; keyed ids hash above it.
const ORDERED: u32 = 1 << 20;

impl Kit {
    /// An id for a widget whose state need not survive between frames, such
    /// as a button, running `action` when it reports an event.
    pub fn on(&mut self, action: impl FnOnce(&mut CrossWall, &Event) + Send + 'static) -> u32 {
        self.next += 1;
        let id = self.next;
        self.actions.insert(id, Box::new(action));
        id
    }

    /// A stable id for `key`, the same every frame, for widgets bones keeps
    /// state for: tiles, scroll areas, and text fields.
    pub fn keyed(
        &mut self,
        key: impl Hash,
        action: impl FnOnce(&mut CrossWall, &Event) + Send + 'static,
    ) -> u32 {
        let id = stable_id(key);
        self.actions.insert(id, Box::new(action));
        id
    }

    /// The action behind `id`, taken: each frame's actions answer only the
    /// events of that frame's tree.
    pub fn take(&mut self, id: u32) -> Option<Action> {
        self.actions.remove(&id)
    }
}

/// A stable id at or above `ORDERED`, so it never meets an ordered one.
pub fn stable_id(key: impl Hash) -> u32 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as u32) | ORDERED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_ids_are_stable_and_never_ordered_ones() {
        assert_eq!(stable_id("tile:a"), stable_id("tile:a"));
        assert_ne!(stable_id("tile:a"), stable_id("tile:b"));
        let mut kit = Kit::default();
        let ordered = kit.on(|_, _| {});
        assert!(ordered < ORDERED && stable_id("x") >= ORDERED);
    }
}
