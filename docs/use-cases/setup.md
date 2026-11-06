# Setup

### UC-SETUP-02: Choose my wallpaper folder

- **Actor**: Wallpaper user.
- **Goal**: Decide where my wallpapers live.
- **Flow**: Press Choose in Settings or Library and pick a folder.
- **Success**: Its images and subfolders appear in the Library. Until one is chosen, Browse still works, and Download says to choose one.

### UC-SETUP-04: Turn on a log file

- **Actor**: Wallpaper user.
- **Goal**: Capture what the app does when reporting a problem.
- **Flow**: Tick the log option in Settings, and press Open log folder to find it.
- **Success**: `app.log` records activity from then on. It is off by default in release builds. A failure is always written to `crash.log`.

### UC-SETUP-03: Choose light or dark

- **Actor**: Wallpaper user.
- **Goal**: Use the theme I prefer. The app starts dark.
- **Flow**: Open Settings and pick Dark, Light, or System under Theme.
- **Success**: The window changes at once and stays that way next time.

