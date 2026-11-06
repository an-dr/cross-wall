//! Each source's request URL and answer format. Only Pexels needs a key. Every
//! ID is prefixed by its source, except Pexels', which keeps the plain numbers
//! that existing likes were stored under.
use serde_json::Value;

use crate::{Error, Orientation, Page, Photo, Source, Sources};

pub const PER_PAGE: u32 = 30;
/// Openverse caps anonymous requests at 20 results per page.
const OPENVERSE_PER_PAGE: u32 = 20;

pub fn url(source: Source, query: &str, orientation: Orientation, page: u32) -> String {
    let q = encode(query.trim());
    let empty = query.trim().is_empty();
    match source {
        Source::Pexels if empty => format!("https://api.pexels.com/v1/curated?per_page={PER_PAGE}&page={page}"),
        Source::Pexels => {
            let shape = match orientation {
                Orientation::Any => "",
                Orientation::Landscape => "&orientation=landscape",
                Orientation::Portrait => "&orientation=portrait",
                Orientation::Square => "&orientation=square",
            };
            format!("https://api.pexels.com/v1/search?query={q}&per_page={PER_PAGE}&page={page}{shape}")
        }
        // purity=100 is Wallhaven's safe-for-work filter and categories=100 its General
        // category, whose results look like wallpapers. Neither is a setting.
        Source::Wallhaven => {
            let sorting = if empty { "toplist" } else { "relevance" };
            let shape = match orientation {
                Orientation::Any => "",
                Orientation::Landscape => "&ratios=landscape",
                Orientation::Portrait => "&ratios=portrait",
                Orientation::Square => "&ratios=1x1",
            };
            format!("https://wallhaven.cc/api/v1/search?q={q}&categories=100&purity=100&atleast=1920x1080&sorting={sorting}&page={page}{shape}")
        }
        Source::Openverse => {
            let q = if empty { encode("landscape wallpaper") } else { q };
            let shape = match orientation {
                Orientation::Any => "",
                Orientation::Landscape => "&aspect_ratio=wide",
                Orientation::Portrait => "&aspect_ratio=tall",
                Orientation::Square => "&aspect_ratio=square",
            };
            format!("https://api.openverse.org/v1/images/?q={q}&size=large&page_size={OPENVERSE_PER_PAGE}&page={page}{shape}")
        }
        Source::Commons => {
            let term = if empty { "landscape".to_string() } else { query.trim().to_string() };
            let search = encode(&format!("{term} filew:>1920 filetype:bitmap"));
            let offset = (page.saturating_sub(1)) * PER_PAGE;
            format!("https://commons.wikimedia.org/w/api.php?action=query&format=json&generator=search&gsrnamespace=6&gsrsearch={search}&gsrlimit={PER_PAGE}&gsroffset={offset}&prop=imageinfo&iiprop=url%7Csize%7Cextmetadata&iiurlwidth=640")
        }
        // Picsum has no search: it is one fixed list.
        Source::Picsum => format!("https://picsum.photos/v2/list?page={page}&limit={PER_PAGE}"),
        Source::Nasa => {
            let q = if empty { encode("earth") } else { q };
            format!("https://images-api.nasa.gov/search?q={q}&media_type=image&page={page}&page_size={PER_PAGE}")
        }
    }
}

pub fn parse(source: Source, body: &[u8]) -> Result<Page, Error> {
    let bad = |e: serde_json::Error| Error::Malformed(e.to_string());
    if source == Source::Pexels {
        #[derive(serde::Deserialize)]
        struct Answer {
            photos: Vec<Photo>,
            next_page: Option<String>,
        }
        let answer: Answer = serde_json::from_slice(body).map_err(bad)?;
        return Ok(Page { photos: answer.photos, has_more: answer.next_page.is_some() });
    }
    let json: Value = serde_json::from_slice(body).map_err(bad)?;
    Ok(match source {
        Source::Pexels => unreachable!("handled above"),
        Source::Wallhaven => wallhaven(&json),
        Source::Openverse => openverse(&json),
        Source::Commons => commons(&json),
        Source::Picsum => picsum(&json),
        Source::Nasa => nasa(&json),
    })
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

fn number(value: &Value, key: &str) -> u32 {
    value[key].as_u64().unwrap_or(0) as u32
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn wallhaven(json: &Value) -> Page {
    let photos = items(&json["data"])
        .iter()
        .filter_map(|w| {
            let id = text(w, "id");
            let original = text(w, "path");
            let medium = text(&w["thumbs"], "large");
            let source = text(w, "source");
            (!id.is_empty() && !original.is_empty()).then(|| Photo {
                id: format!("wallhaven-{}", safe_id(&id)),
                width: number(w, "dimension_x"),
                height: number(w, "dimension_y"),
                url: text(w, "url"),
                photographer: "Wallhaven upload".into(),
                photographer_url: if source.starts_with("http") { source } else { text(w, "url") },
                license: "Unknown; uploaded by users".into(),
                src: Sources { large2x: original.clone(), original, medium },
            })
        })
        .collect();
    let (current, last) = (json["meta"]["current_page"].as_u64(), json["meta"]["last_page"].as_u64());
    Page { photos, has_more: matches!((current, last), (Some(c), Some(l)) if c < l) }
}

fn openverse(json: &Value) -> Page {
    let photos = items(&json["results"])
        .iter()
        .filter_map(|r| {
            let id = text(r, "id");
            let original = text(r, "url");
            (!id.is_empty() && !original.is_empty()).then(|| Photo {
                id: format!("openverse-{}", safe_id(&id)),
                width: number(r, "width"),
                height: number(r, "height"),
                url: text(r, "foreign_landing_url"),
                photographer: r["creator"].as_str().unwrap_or("Unknown creator").to_string(),
                photographer_url: r["creator_url"].as_str().map_or_else(|| text(r, "foreign_landing_url"), String::from),
                license: format!("CC {} {}", text(r, "license").to_uppercase(), text(r, "license_version")).trim().to_string(),
                src: Sources { large2x: original.clone(), original, medium: text(r, "thumbnail") },
            })
        })
        .collect();
    let page_count = json["page_count"].as_u64().unwrap_or(0);
    Page { photos, has_more: page_count > 1 }
}

fn commons(json: &Value) -> Page {
    let mut pages: Vec<&Value> = json["query"]["pages"].as_object().map_or(Vec::new(), |m| m.values().collect());
    pages.sort_by_key(|p| p["index"].as_u64().unwrap_or(u64::MAX));
    let photos = pages
        .into_iter()
        .filter_map(|page| {
            let info = &page["imageinfo"][0];
            let original = text(info, "url");
            let id = page["pageid"].as_u64()?;
            let meta = &info["extmetadata"];
            (!original.is_empty()).then(|| Photo {
                id: format!("commons-{id}"),
                width: number(info, "width"),
                height: number(info, "height"),
                url: text(info, "descriptionurl"),
                photographer: strip_tags(meta["Artist"]["value"].as_str().unwrap_or("Wikimedia Commons")),
                photographer_url: text(info, "descriptionurl"),
                license: text(&meta["LicenseShortName"], "value"),
                src: Sources { large2x: original.clone(), original, medium: text(info, "thumburl") },
            })
        })
        .collect();
    Page { photos, has_more: !json["continue"].is_null() }
}

fn picsum(json: &Value) -> Page {
    let photos: Vec<Photo> = items(json)
        .iter()
        .filter_map(|p| {
            let id = text(p, "id");
            (!id.is_empty()).then(|| Photo {
                id: format!("picsum-{}", safe_id(&id)),
                width: number(p, "width"),
                height: number(p, "height"),
                url: text(p, "url"),
                photographer: text(p, "author"),
                photographer_url: text(p, "url"),
                license: "Unsplash License".into(),
                src: Sources {
                    original: text(p, "download_url"),
                    large2x: format!("https://picsum.photos/id/{id}/1920/1200"),
                    medium: format!("https://picsum.photos/id/{id}/640/400"),
                },
            })
        })
        .collect();
    let has_more = photos.len() as u32 == PER_PAGE;
    Page { photos, has_more }
}

fn nasa(json: &Value) -> Page {
    let collection = &json["collection"];
    let photos = items(&collection["items"])
        .iter()
        .filter_map(|item| {
            let data = &item["data"][0];
            let id = text(data, "nasa_id");
            let links = items(&item["links"]);
            let with_rel = |rel: &str| links.iter().find(|l| l["rel"].as_str() == Some(rel));
            // The canonical link is the original image when NASA publishes one.
            let best = with_rel("canonical").or_else(|| links.iter().max_by_key(|l| number(l, "width")))?;
            let preview = with_rel("preview").or(links.first())?;
            let original = text(best, "href");
            (!id.is_empty() && !original.is_empty()).then(|| Photo {
                id: format!("nasa-{}", safe_id(&id)),
                width: number(best, "width"),
                height: number(best, "height"),
                url: format!("https://images.nasa.gov/details/{}", encode(&id)),
                photographer: data["photographer"].as_str().or(data["center"].as_str()).unwrap_or("NASA").to_string(),
                photographer_url: format!("https://images.nasa.gov/details/{}", encode(&id)),
                license: "NASA media; check the details page".into(),
                src: Sources { large2x: original.clone(), original, medium: text(preview, "href") },
            })
        })
        .collect();
    let has_more = items(&collection["links"]).iter().any(|l| l["rel"].as_str() == Some("next"));
    Page { photos, has_more }
}

/// IDs become file names, and they come from a remote service.
fn safe_id(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
