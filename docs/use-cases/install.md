# Install and update

### UC-INSTALL-01: Install the app

- **Actor**: Wallpaper user.
- **Goal**: Have the app in my Start Menu or application list, without administrator rights.
- **Flow**: Run the install script, or press Install in Settings of an unpacked build.
- **Success**: The app is listed in the system's application list and starts from there.

### UC-INSTALL-02: Remove the app

- **Actor**: Wallpaper user.
- **Goal**: Remove the app and keep my photos.
- **Flow**: Press Unregister in Settings, then delete the install folder the app names.
- **Success**: The app leaves the application list. Liked photos and the sync folder are untouched.

### UC-UPDATE-01: Update the app

- **Actor**: Wallpaper user.
- **Goal**: Move to the newest release.
- **Flow**: Press Check for updates in Settings, then Update, then restart the app.
- **Success**: The new version number shows in Settings. A failed download leaves the running version as it was, and the previous version stays installed for recovery.
