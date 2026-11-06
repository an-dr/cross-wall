# Browse

### UC-BROWSE-01: See photos with no setup

- **Actor**: Wallpaper user.
- **Goal**: Find a wallpaper without knowing what to search for.
- **Flow**: Open the app, optionally pick another source, and scroll; more photos load as the end approaches. After a failure, Try again resumes.
- **Success**: A thumbnail grid fills in while the window stays responsive. No key or account is needed. A bar under the toolbar shows loading progress. Photos credit their photographer where the source names one, and the screen links to the source. Network failures and rate limits show a message.

### UC-BROWSE-02: Search photos

- **Actor**: Wallpaper user.
- **Goal**: Find photos of a subject in the shape of my screen.
- **Flow**: Type a query and press Enter, or change the source, shape, or minimum resolution.
- **Success**: The grid shows the matching photos, or says there are none. Lorem Picsum has no search, and its search box says so.

### UC-BROWSE-04: Show only photos big enough for my screen

- **Actor**: Wallpaper user.
- **Goal**: Avoid wallpapers that would look soft on my display.
- **Flow**: Browse with the minimum resolution, which starts at my main display's resolution, or pick another size or Any size from its menu.
- **Success**: Results are at least that size, and the choice is still set next time. A photo whose source reports no size is still shown.

### UC-BROWSE-03: Preview a photo

- **Actor**: Wallpaper user.
- **Goal**: Judge a photo before keeping it.
- **Flow**: Click a thumbnail, and step through photos with the arrow keys. The three photos on each side load ahead, so stepping is quick. A big image still loading shows download progress, then a message while it is prepared.
- **Success**: A full-window view shows the photo, its creator, size, and licence where known, with Like and Apply buttons.
