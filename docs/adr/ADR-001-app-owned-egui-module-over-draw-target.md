# ADR-001: App-owned egui module over draw-target

## Problem

The app needs a native interface with image thumbnails. The bones `ui/*` vocabulary offers only label, text-edit, and button.

## Decision

Embed bones natively, with no WASM runtime. A module in this application owns an `egui::Context`, consumes the public `draw-target` service, and handles raw platform events itself.

## Rationale

`bones-module-ui` is itself built on only these public seams, so no engine change is needed. Screens are ordinary Rust with the whole egui widget set.

## Rejected alternatives

- An image widget in bones: changes the engine and its guest ABI for one application.
- `gfx/*` sprites: every widget and all input handling would be hand-built.
- Web panels: the user excluded a web interface.
