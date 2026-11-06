use super::*;
use cross_wall_sources::Sources;

fn photo(id: &str) -> Photo {
    let s = String::new;
    Photo {
        id: id.into(),
        width: 1,
        height: 1,
        url: s(),
        photographer: "Ann".into(),
        photographer_url: s(),
        license: s(),
        src: Sources { original: s(), large2x: s(), medium: s() },
    }
}

fn setup() -> (tempfile::TempDir, WallpaperFolder) {
    let dir = tempfile::tempdir().unwrap();
    let folder = WallpaperFolder::open(dir.path().to_path_buf()).unwrap();
    (dir, folder)
}

fn paths(folder: &mut WallpaperFolder) -> Vec<String> {
    folder.snapshot().unwrap().files.into_iter().map(|e| e.path).collect()
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3];

#[test]
fn downloads_land_in_the_root_with_their_real_type() {
    let (dir, mut f) = setup();
    assert_eq!(f.add_photo(&photo("a-1"), b"jpeg", false, 5).unwrap(), "a-1.jpg");
    assert_eq!(f.add_photo(&photo("b-1"), PNG, false, 5).unwrap(), "b-1.png");
    assert!(dir.path().join("a-1.jpg").is_file() && dir.path().join("b-1.png").is_file());
}

#[test]
fn a_photo_kept_elsewhere_is_not_saved_again_and_can_gain_a_like() {
    let (_dir, mut f) = setup();
    f.add_photo(&photo("a"), b"x", false, 1).unwrap();
    f.create_folder("", "Nature").unwrap();
    f.move_to("a.jpg", "Nature").unwrap();
    assert_eq!(f.add_photo(&photo("a"), b"x", true, 2).unwrap(), "Nature/a.jpg");
    let snapshot = f.snapshot().unwrap();
    assert_eq!(snapshot.files.len(), 1);
    assert!(snapshot.files[0].info.liked);
}

#[test]
fn a_name_taken_by_my_own_file_is_left_alone() {
    let (dir, mut f) = setup();
    std::fs::write(dir.path().join("a.jpg"), b"mine").unwrap();
    assert_eq!(f.add_photo(&photo("a"), b"new", false, 1).unwrap(), "a-2.jpg");
    assert_eq!(std::fs::read(dir.path().join("a.jpg")).unwrap(), b"mine");
}

#[test]
fn files_the_app_did_not_download_are_listed_and_can_be_liked() {
    let (dir, mut f) = setup();
    std::fs::create_dir(dir.path().join("Old")).unwrap();
    std::fs::write(dir.path().join("Old/mine.PNG"), b"x").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
    assert_eq!(paths(&mut f), ["Old/mine.PNG"]);
    f.set_liked("Old/mine.PNG", true).unwrap();
    assert!(f.snapshot().unwrap().files[0].info.liked);
}

#[test]
fn metadata_is_hidden_and_never_listed() {
    let (dir, mut f) = setup();
    f.add_photo(&photo("a"), b"x", true, 1).unwrap();
    f.set_liked("a.jpg", false).unwrap();
    let file = dir.path().join(METADATA);
    assert!(file.is_file());
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_ne!(std::fs::metadata(&file).unwrap().file_attributes() & 2, 0);
    }
    assert_eq!(paths(&mut f), ["a.jpg"]);
    // Rewriting over the hidden file must keep working.
    f.set_liked("a.jpg", true).unwrap();
    assert!(f.snapshot().unwrap().files[0].info.liked);
}

#[test]
fn renaming_and_moving_keep_likes_and_credits() {
    let (_dir, mut f) = setup();
    f.add_photo(&photo("a"), b"x", true, 1).unwrap();
    assert_eq!(f.rename("a.jpg", "sea.jpg").unwrap(), "sea.jpg");
    f.create_folder("", "One").unwrap();
    f.create_folder("One", "Two").unwrap();
    f.move_to("sea.jpg", "One/Two").unwrap();
    f.rename("One", "First").unwrap();
    let snapshot = f.snapshot().unwrap();
    assert_eq!(snapshot.folders, ["First", "First/Two"]);
    let entry = &snapshot.files[0];
    assert_eq!(entry.path, "First/Two/sea.jpg");
    assert!(entry.info.liked);
    assert_eq!(entry.info.photo.as_ref().unwrap().photographer, "Ann");
}

#[test]
fn nothing_is_overwritten_by_a_move_or_rename() {
    let (dir, mut f) = setup();
    f.add_photo(&photo("a"), b"first", false, 1).unwrap();
    f.add_photo(&photo("b"), b"second", false, 1).unwrap();
    assert!(f.rename("a.jpg", "b.jpg").unwrap_err().contains("already exists"));
    f.create_folder("", "X").unwrap();
    std::fs::write(dir.path().join("X/a.jpg"), b"other").unwrap();
    assert!(f.move_to("a.jpg", "X").unwrap_err().contains("already exists"));
    assert_eq!(std::fs::read(dir.path().join("a.jpg")).unwrap(), b"first");
    assert_eq!(std::fs::read(dir.path().join("X/a.jpg")).unwrap(), b"other");
}

#[test]
fn a_folder_cannot_move_into_itself() {
    let (_dir, mut f) = setup();
    f.create_folder("", "A").unwrap();
    f.create_folder("A", "B").unwrap();
    assert!(f.move_to("A", "A").is_err());
    assert!(f.move_to("A", "A/B").is_err());
    f.create_folder("", "AB").unwrap();
    assert!(f.move_to("AB", "A").is_ok());
}

#[test]
fn bad_names_and_escaping_paths_are_refused() {
    let (_dir, mut f) = setup();
    for name in ["", " ", ".hidden", "a/b", "a\\b", "..", "end.", "c:d"] {
        assert!(f.create_folder("", name).is_err(), "{name}");
    }
    f.add_photo(&photo("a"), b"x", false, 1).unwrap();
    assert!(f.rename("a.jpg", "a.txt").is_err());
    for path in ["../x", "a/../../x", "/x", "c:\\x"] {
        assert!(f.resolve(path).is_err(), "{path}");
    }
    assert!(f.delete("").is_err());
}

#[test]
fn a_damaged_metadata_file_is_kept_aside() {
    let (dir, mut f) = setup();
    std::fs::write(dir.path().join(METADATA), "{broken").unwrap();
    f.add_photo(&photo("a"), b"x", true, 1).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join(".cross-wall.damaged.json")).unwrap(), "{broken");
    assert!(f.snapshot().unwrap().files[0].info.liked);
}

#[test]
fn metadata_forgets_files_removed_outside_the_app() {
    let (dir, mut f) = setup();
    f.add_photo(&photo("a"), b"x", true, 1).unwrap();
    f.add_photo(&photo("b"), b"x", true, 1).unwrap();
    std::fs::remove_file(dir.path().join("a.jpg")).unwrap();
    f.set_liked("b.jpg", true).unwrap();
    let text = std::fs::read_to_string(dir.path().join(METADATA)).unwrap();
    assert!(!text.contains("a.jpg") && text.contains("b.jpg"));
}

#[test]
fn a_missing_folder_is_an_error_and_is_not_created() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("unmounted");
    assert!(WallpaperFolder::open(missing.clone()).unwrap_err().contains("unavailable"));
    assert!(!missing.exists());
}

#[test]
fn image_sizes_are_read_and_a_corrupt_image_still_lists() {
    let (dir, mut f) = setup();
    image::RgbImage::new(4, 3).save(dir.path().join("real.png")).unwrap();
    std::fs::write(dir.path().join("bad.jpg"), b"nope").unwrap();
    let snapshot = f.snapshot().unwrap();
    let size = |name: &str| snapshot.files.iter().find(|e| e.path == name).map(|e| (e.width, e.height)).unwrap();
    assert_eq!(size("real.png"), (4, 3));
    assert_eq!(size("bad.jpg"), (0, 0));
}

/// Sends one tiny file to the real trash: `cargo test -p cross-wall-library -- --ignored`.
#[test]
#[ignore]
fn delete_uses_the_real_trash_and_forgets_the_file() {
    let (dir, mut f) = setup();
    f.add_photo(&photo("cross-wall-trash-test"), b"x", true, 1).unwrap();
    f.delete("cross-wall-trash-test.jpg").unwrap();
    assert!(!dir.path().join("cross-wall-trash-test.jpg").exists());
    assert!(f.snapshot().unwrap().files.is_empty());
}
