# ADR-004: Screens are built from bones widgets

Supersedes [ADR-001](ADR-001-app-owned-egui-module-over-draw-target.md).

## Problem

ADR-001 gave the app its own egui context because the bones `ui/*` vocabulary had only labels, text fields, and buttons. The app then carried its own widgets, theme, title bar, and icons, all of which other bones applications would need too.

## Decision

The screens are a bones native view ([bones ADR-048](https://github.com/an-dr/bones/blob/main/docs/adr/ADR-048-one-widget-tree-for-native-modules-and-extensions.md)): each frame the app describes its interface as a bones widget tree from its state and handles the events bones returns. bones draws it in the desktop's look ([bones ADR-049](https://github.com/an-dr/bones/blob/main/docs/adr/ADR-049-the-ui-module-follows-the-desktop-look.md)), with its title bar and Lucide icons. The app keeps no egui code of its own.

## Rationale

- The widgets, theme, and title bar moved into bones, where every bones application gets them; the app is left with what is specific to wallpapers.
- Describing screens from state separates them from drawing, so they can be tested as data.

## Rejected alternatives

- **Keep the app's own egui module**: the code would stay duplicated, and the app would not follow bones' widget fixes.
- **Register the wallpaper tile and preview as custom widget kinds**: both turned out general enough to become bones' `tile` and `stage`.
