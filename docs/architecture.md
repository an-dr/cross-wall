# Architecture

cross-wall is a native desktop application that embeds the [bones](https://github.com/an-dr/bones) engine. It talks to photo source APIs over HTTPS, reads and writes a folder that a third-party tool keeps in sync, and asks the desktop environment to change its wallpaper.

```mermaid
flowchart TD
  host["apps/cross-wall/host: composition and screens"] --> bones["vendor/bones: window, renderer, upgrade, desktop"]
  host --> sources["crates/sources: photo source APIs"]
  host --> library["crates/library: likes and merge"]
  host --> apply["crates/apply: set the wallpaper"]
```

| Part | Responsibility | Use cases |
| --- | --- | --- |
| host | Composes bones; draws every screen with egui; owns settings and paths | all |
| sources | One search and download call over five keyless photo sources; no UI or bones dependency | BROWSE, LIBRARY-01 |
| library | Local store of likes and merging with the sync folder | LIBRARY, SETUP-02 |
| apply | Detects the desktop and sets the wallpaper | APPLY-01 |
| launcher | Stable entry point selecting the newest installed version | INSTALL, UPDATE |

## Guarantees

- The frame thread never waits on network or disk.
- Liked photos work offline.
- Concurrent changes on two computers never lose a like.
- An interrupted download is never shown as liked.
- The app writes only to its own data folders, the chosen sync folder, and the desktop's wallpaper setting.
