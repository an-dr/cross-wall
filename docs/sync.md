# Sync

cross-wall does not move files between computers. You choose a folder that Syncthing, Dropbox, OneDrive, or a similar tool already syncs, and the app merges with a `cross-wall` folder inside it every 30 seconds and after every change. [ADR-002](adr/ADR-002-sync-through-a-shared-folder.md) says why.

```text
<sync folder>/cross-wall/
  likes/<photo id>.json     who took it, and when you liked it
  images/<photo id>.jpg     the original image
  removed/<photo id>.json   when you unliked it
```

- **Which side wins:** for one photo, the newer of its like and its removal, and a removal wins a tie. Times come from each computer's clock, so a badly wrong clock can decide this wrongly.
- **Late images:** a like appears only when its image is the recorded size, so a half-synced image waits for a later merge.
- **Missing folder:** if the folder is not there, such as an unmounted drive, the app reports it and keeps your likes locally. It never creates the folder.
- **Other files:** conflict copies and other names the app did not write are ignored and left in place.
- **Storage:** removal records are small and are kept, so that a computer that was off for months cannot bring an unliked photo back.
