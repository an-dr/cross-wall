# ADR-003: The wallpaper folder is the library

Supersedes [ADR-002](ADR-002-sync-through-a-shared-folder.md).

## Problem

Keeping likes in an app-owned layout hid photos from the user and could not be organised. The real need is a folder of wallpapers the user controls.

## Decision

The user chooses a local folder, and its contents are the library. Downloads land in its root, and every image and subfolder in it is shown and can be organised from the app. Likes and photo credits are metadata in a hidden `.cross-wall.json` at its root, keyed by relative path. The app no longer syncs anything. Deleting sends items to the system trash.

## Rationale

The file system stays the single authority, so a file manager and the app can both work on the folder. Nothing is lost if the metadata goes: only likes and credits depend on it.

## Rejected alternatives

- Only showing the app's own downloads: the user's own images could not be applied or organised.
- A metadata file beside each image: it clutters the folder the user organises.
- A database outside the folder: likes would not travel with the folder.
