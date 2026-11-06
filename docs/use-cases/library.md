# Library

### UC-LIBRARY-01: Download a wallpaper

- **Actor**: Wallpaper user.
- **Goal**: Keep a photo in my wallpaper folder.
- **Flow**: Press Download on a photo in Browse.
- **Success**: Progress shows while it downloads, then the image is in the root of the wallpaper folder and in the Library, and opens with no network. A photo kept earlier is not saved twice, even after being moved.

### UC-LIBRARY-02: Like a wallpaper

- **Actor**: Wallpaper user.
- **Goal**: Mark my favourites, among downloads and my own images.
- **Flow**: Press Like on a photo. In Browse this downloads it too. Press it again to remove the like.
- **Success**: It appears under Liked, and removing the like keeps the file. The like is stored in the folder, so it stays with the folder.

### UC-LIBRARY-03: See what is in my folder

- **Actor**: Wallpaper user.
- **Goal**: Browse all my wallpapers, including ones I added myself.
- **Flow**: Open Library, and open folders one at a time, or switch to Groups to see every folder as a section.
- **Success**: Every image and subfolder appears. Changes made in a file manager appear within a few seconds.

### UC-LIBRARY-04: Organise my folder

- **Actor**: Wallpaper user.
- **Goal**: Arrange wallpapers into folders.
- **Flow**: Press New folder. Right-click a wallpaper or folder to rename, move, or delete it, or drag a wallpaper onto a folder.
- **Success**: The files change on disk, likes and credits stay with them, and nothing is overwritten. Deleted items are in the system trash.
