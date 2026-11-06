use super::*;
use std::sync::Mutex;

struct Canned {
    answer: Result<(u16, Vec<u8>), String>,
    asked: Mutex<Vec<String>>,
}
impl Transport for Canned {
    fn get(&self, url: &str, _: &Progress) -> Result<(u16, Vec<u8>), String> {
        self.asked.lock().unwrap().push(url.into());
        self.answer.clone()
    }
}
fn client(answer: Result<(u16, Vec<u8>), String>) -> (Client, Arc<Canned>) {
    let canned = Arc::new(Canned { answer, asked: Mutex::new(Vec::new()) });
    (Client::with_transport(canned.clone()), canned)
}
fn parsed(source: Source, json: &str) -> Page {
    client(Ok((200, json.into()))).0.search(source, "x", Orientation::Any, MinSize::ANY, 1).unwrap()
}

#[test]
fn wallhaven_is_always_safe_for_work_and_encodes_the_query() {
    let (c, canned) = client(Ok((200, b"{}".to_vec())));
    c.search(Source::Wallhaven, "sea & sky/é", Orientation::Landscape, MinSize::ANY, 1).unwrap();
    let url = canned.asked.lock().unwrap()[0].clone();
    assert!(url.contains("purity=100") && url.contains("categories=100"), "{url}");
    assert!(url.contains("q=sea%20%26%20sky%2F%C3%A9&"), "{url}");
    assert!(url.ends_with("&ratios=landscape"), "{url}");
}

#[test]
fn parses_wallhaven() {
    let page = parsed(Source::Wallhaven, r#"{"data":[{"id":"ab1","url":"https://w/ab1","source":"","dimension_x":4000,"dimension_y":3000,
      "path":"https://f/a.jpg","thumbs":{"large":"https://t/a.jpg"}}],"meta":{"current_page":1,"last_page":2}}"#);
    let p = &page.photos[0];
    assert_eq!((p.id.as_str(), p.src.medium.as_str(), p.src.original.as_str()), ("wallhaven-ab1", "https://t/a.jpg", "https://f/a.jpg"));
    assert_eq!(p.photographer_url, "https://w/ab1");
    assert!(page.has_more);
}

#[test]
fn parses_openverse_with_licence_and_missing_creator() {
    let page = parsed(Source::Openverse, r#"{"page_count":3,"results":[{"id":"u-1","url":"https://o/1.jpg","thumbnail":"https://t/1",
      "width":2000,"height":1000,"creator":null,"foreign_landing_url":"https://l/1","license":"by","license_version":"4.0"}]}"#);
    let p = &page.photos[0];
    assert_eq!(p.id, "openverse-u-1");
    assert_eq!(p.license, "CC BY 4.0");
    assert_eq!(p.photographer, "Unknown creator");
    assert_eq!(p.photographer_url, "https://l/1");
}

#[test]
fn parses_commons_in_rank_order_and_strips_artist_markup() {
    let page = parsed(Source::Commons, r#"{"continue":{"gsroffset":30},"query":{"pages":{
      "9":{"pageid":9,"index":2,"imageinfo":[{"url":"https://c/b.jpg","width":1,"height":1}]},
      "5":{"pageid":5,"index":1,"imageinfo":[{"url":"https://c/a.jpg","thumburl":"https://c/t.jpg","descriptionurl":"https://c/d","width":4000,"height":3000,
        "extmetadata":{"Artist":{"value":"<a href=\"x\">Ann  Lee</a>"},"LicenseShortName":{"value":"CC BY 4.0"}}}]}}}}"#);
    assert_eq!(page.photos[0].id, "commons-5");
    assert_eq!(page.photos[0].photographer, "Ann Lee");
    assert_eq!(page.photos[1].id, "commons-9");
    assert!(page.has_more);
}

#[test]
fn parses_picsum_and_builds_sized_urls() {
    let page = parsed(Source::Picsum, r#"[{"id":"10","author":"P","width":2500,"height":1667,"url":"https://u/1","download_url":"https://picsum.photos/id/10/2500/1667"}]"#);
    assert_eq!(page.photos[0].id, "picsum-10");
    assert_eq!(page.photos[0].src.medium, "https://picsum.photos/id/10/640/400");
    assert!(!page.has_more);
}

#[test]
fn parses_nasa_preferring_the_canonical_image() {
    let page = parsed(Source::Nasa, r#"{"collection":{"links":[{"rel":"next"}],"items":[{"data":[{"nasa_id":"PIA 1","center":"JPL"}],"links":[
      {"rel":"preview","href":"https://n/t.jpg","width":640,"height":480},{"rel":"canonical","href":"https://n/o.jpg","width":4000,"height":3000}]}]}}"#);
    let p = &page.photos[0];
    assert_eq!(p.id, "nasa-PIA_1");
    assert_eq!((p.src.original.as_str(), p.src.medium.as_str()), ("https://n/o.jpg", "https://n/t.jpg"));
    assert_eq!(p.photographer, "JPL");
    assert!(page.has_more);
}

#[test]
fn remote_ids_cannot_escape_a_folder() {
    let page = parsed(Source::Wallhaven, r#"{"data":[{"id":"../../x","path":"https://f/a.jpg"}]}"#);
    assert_eq!(page.photos[0].id, "wallhaven-______x");
}

#[test]
fn filters_shape_where_the_source_cannot() {
    let json = r#"[{"id":"1","author":"a","width":100,"height":50,"url":"u","download_url":"d"},{"id":"2","author":"a","width":50,"height":100,"url":"u","download_url":"d"}]"#;
    let page = client(Ok((200, json.into()))).0.search(Source::Picsum, "", Orientation::Portrait, MinSize::ANY, 1).unwrap();
    assert_eq!(page.photos.len(), 1);
    assert_eq!(page.photos[0].id, "picsum-2");
}

#[test]
fn minimum_size_reaches_the_server_and_is_enforced_here() {
    let (c, canned) = client(Ok((200, b"{}".to_vec())));
    c.search(Source::Wallhaven, "", Orientation::Any, MinSize { width: 2560, height: 1440 }, 1).unwrap();
    assert!(canned.asked.lock().unwrap()[0].contains("&atleast=2560x1440&"));
    let json = r#"[{"id":"1","author":"a","width":1920,"height":1080,"url":"u","download_url":"d"},{"id":"2","author":"a","width":3840,"height":2160,"url":"u","download_url":"d"}]"#;
    let page = client(Ok((200, json.into()))).0.search(Source::Picsum, "", Orientation::Any, MinSize { width: 2560, height: 1440 }, 1).unwrap();
    assert_eq!(page.photos.len(), 1);
    assert_eq!(page.photos[0].id, "picsum-2");
}

#[test]
fn portrait_searches_turn_the_minimum() {
    let (c, canned) = client(Ok((200, b"{}".to_vec())));
    c.search(Source::Wallhaven, "", Orientation::Portrait, MinSize { width: 2560, height: 1440 }, 1).unwrap();
    assert!(canned.asked.lock().unwrap()[0].contains("&atleast=1440x2560&"));
}

#[test]
fn likes_stored_with_numeric_ids_still_load() {
    let photo: Photo = serde_json::from_str(r#"{"id":12,"width":1,"height":1,"url":"","photographer":"","photographer_url":"","src":{"original":"","large2x":"","medium":""}}"#).unwrap();
    assert_eq!(photo.id, "12");
    assert_eq!(photo.license, "");
}

#[test]
fn progress_reports_bytes_and_an_optional_total() {
    let progress = Progress::default();
    assert_eq!(progress.read(), (0, None));
    progress.set_total(100);
    progress.advance(30);
    progress.advance(20);
    assert_eq!(progress.read(), (50, Some(100)));
}

#[test]
fn maps_failures() {
    let search = |c: Client| c.search(Source::Openverse, "x", Orientation::Any, MinSize::ANY, 1);
    for (status, expected) in [(401, Error::Refused), (429, Error::RateLimited), (500, Error::Status(500))] {
        assert_eq!(search(client(Ok((status, vec![]))).0).unwrap_err(), expected);
    }
    assert!(matches!(search(client(Err("down".into())).0), Err(Error::Network(_))));
    assert!(matches!(search(client(Ok((200, b"nope".to_vec()))).0), Err(Error::Malformed(_))));
}

/// Runs against the real services: `cargo test -p cross-wall-sources -- --ignored`.
#[test]
#[ignore]
fn live_keyless_sources_answer_and_their_images_download() {
    let client = Client::new();
    for source in Source::ALL {
        let page = client.search(source, "mountain", Orientation::Any, MinSize::ANY, 1).unwrap_or_else(|e| panic!("{source:?}: {e}"));
        let photo = page.photos.first().unwrap_or_else(|| panic!("{source:?}: no photos"));
        println!("{source:?}: {} photos, more={}, {} x {}, by {}, {}", page.photos.len(), page.has_more, photo.width, photo.height, photo.photographer, photo.license);
        let thumbnail = client.download_thumbnail(photo).unwrap_or_else(|e| panic!("{source:?} thumbnail: {e}"));
        let original = client.download(&photo.src.original).unwrap_or_else(|e| panic!("{source:?} original: {e}"));
        assert!(thumbnail.len() > 1000 && original.len() > 1000, "{source:?}");
    }
}
