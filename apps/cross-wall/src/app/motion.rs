//! Timing for the small animations between states.
use std::collections::HashMap;

use egui_animation::easing;

/// Short enough that nothing feels like it is waiting for the animation.
pub const TRANSITION_SECONDS: f64 = 0.18;

/// Eased progress from 0 to 1 since `started`.
pub fn eased_since(started: f64, time: f64) -> f32 {
    let linear = ((time - started) / TRANSITION_SECONDS).clamp(0.0, 1.0) as f32;
    easing::cubic_out(linear)
}

/// When each animated area last changed, and to what.
#[derive(Default)]
pub struct Motion {
    changed: HashMap<&'static str, (String, f64)>,
}

impl Motion {
    /// Progress from 0 to 1 since `key` last changed for this area, eased.
    pub fn transition(&mut self, ctx: &egui::Context, area: &'static str, key: String) -> f32 {
        let time = ctx.input(|i| i.time);
        let entry = self
            .changed
            .entry(area)
            .or_insert_with(|| (key.clone(), f64::NEG_INFINITY));
        if entry.0 != key {
            *entry = (key, time);
        }
        eased_since(entry.1, time)
    }
}
