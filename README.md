# cross-wall

Browse wallpapers from Wallhaven, Openverse, Wikimedia Commons, Lorem Picsum, or NASA Images, keep them organised in a folder of your own, and set them as your wallpaper. Runs on Windows (x64, ARM64) and Linux (x64), and applies wallpapers on Windows, KDE Plasma, GNOME, and COSMIC. Built on [bones](https://github.com/an-dr/bones) with a native egui interface.

## Install

Windows, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/an-dr/cross-wall/main/scripts/install.ps1 | iex
```

Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/an-dr/cross-wall/main/scripts/install.sh | sh
```

Both install for the current user into `~/.cross-wall`, with no administrator rights, and add the app to the application list. They need a published release. Update from Settings; remove the app with Unregister in Settings, then delete that folder. Your wallpaper folder stays.

## Build from source

```sh
git clone --recurse-submodules https://github.com/an-dr/cross-wall
cargo run -p cross-wall-app
```

Needs a Rust toolchain, a C compiler, CMake, and Ninja, because bones builds SDL3 from source. On Linux, also the packages `cargo xtask linux-packages` in `vendor/bones` lists.

## Use it

Browse works straight away with no account or key, showing photos at least as big as your main display until you pick another minimum resolution. Check each photo's licence before using it beyond your own desktop: Wallhaven images are uploaded by users and often state none. Choose a wallpaper folder in Settings: downloads go to its root, and the Library shows and organises everything in it.

## Documentation

Start at [docs/index.md](docs/index.md).

## License

MIT. See [LICENSE](LICENSE). Photos belong to their creators and sources.
