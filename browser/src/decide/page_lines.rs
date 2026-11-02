//! The video files a page lists in its own data, for the panel's list.
//!
//! Some sites put the addresses of a video's files in the page itself, not only
//! in what the player fetches: Facebook's markup holds `progressive_urls` (the
//! SD and HD files) and `dash_manifest_urls` for each video on the page. The
//! player's own requests name one rendition; the page names them all.

/// One line the page offers: the address of a file or stream, and how it is named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLine {
    /// The address to download.
    pub url: String,
    /// What the line says it is: "MP4 file, quality HD".
    pub label: String,
    /// The sound that goes with a picture offered on its own, to be joined to it.
    pub sound: Option<String>,
}

/// How far either side of a video's `progressive_urls` its other fields are looked for.
const NEAR: usize = 150_000;

/// A key that names a file in an address: the part after the last slash, up to
/// its extension. Long enough to be this file's and no other's.
fn file_id(url: &str) -> Option<String> {
    let name = url.split(['?', '#']).next()?.rsplit('/').next()?;
    let id = name.split('.').next()?;
    (id.len() >= 24).then(|| id.to_string())
}

/// The JSON array that follows `"key":` at `at`, read as the page wrote it
/// (its slashes and `\u` escapes are JSON's).
fn array_after(text: &str, at: usize, key: &str) -> Option<Vec<serde_json::Value>> {
    let rest = text.get(at + key.len()..)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let mut values = serde_json::Deserializer::from_str(rest).into_iter::<serde_json::Value>();
    match values.next()? {
        Ok(serde_json::Value::Array(items)) => Some(items),
        _ => None,
    }
}

/// Where `key` starts, for each place `text` has it.
fn places(text: &str, key: &str) -> Vec<usize> {
    let quoted = format!("\"{key}\"");
    text.match_indices(&quoted).map(|(at, _)| at).collect()
}

/// The lines a page offers for the video the player is playing, given the
/// addresses the player has been seen to fetch (`seen`): the video whose data
/// holds one of those files is the one, and its progressive files and DASH
/// manifest are the lines. Nothing when the page names no video that matches.
pub fn page_lines(text: &str, seen: &[String]) -> Vec<PageLine> {
    let ids: Vec<String> = seen.iter().filter_map(|u| file_id(u)).collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let key = "progressive_urls";
    for at in places(text, key) {
        let from = floor(text, at.saturating_sub(NEAR));
        let to = floor(text, (at + NEAR).min(text.len()));
        let window = &text[from..to];
        if !ids.iter().any(|id| window.contains(id.as_str())) {
            continue;
        }
        let mut lines = renditions(text, at);
        // The renditions of the manifest come first, each with its height, bitrate and
        // the video's length; the page's own SD and HD files follow them.
        let manifest_renditions = !lines.is_empty();
        for item in array_after(text, at, &format!("\"{key}\"")).unwrap_or_default() {
            let url = item.get("progressive_url").and_then(|v| v.as_str());
            let quality = item
                .get("metadata")
                .and_then(|m| m.get("quality"))
                .and_then(|q| q.as_str());
            if let Some(url) = url.filter(|u| u.starts_with("https://") || u.starts_with("http://")) {
                lines.push(PageLine {
                    url: url.to_string(),
                    label: match quality {
                        Some(quality) => format!("MP4 file, quality {quality}"),
                        None => "MP4 file".to_string(),
                    },
                    sound: None,
                });
            }
        }
        // The manifest of the same video, the nearest one before or after: offered as a
        // line of its own only where its renditions could not be read.
        let manifests = places(text, "dash_manifest_urls")
            .into_iter()
            .filter(|m| m.abs_diff(at) <= NEAR)
            .min_by_key(|m| m.abs_diff(at));
        if let Some(m) = manifests.filter(|_| !manifest_renditions) {
            for item in array_after(text, m, "\"dash_manifest_urls\"").unwrap_or_default() {
                if let Some(url) = item
                    .get("manifest_url")
                    .and_then(|v| v.as_str())
                    .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
                {
                    lines.push(PageLine {
                        url: url.to_string(),
                        label: "DASH stream".to_string(),
                        sound: None,
                    });
                }
            }
        }
        return lines;
    }
    instagram_lines(text, &ids)
}

/// Instagram's shape: each video's data holds `video_dash_manifest` (the MPD as a JSON
/// string) and `video_versions` (the plain files, each with its width and height). The
/// video whose manifest or files hold one of the files the player fetched is the one.
fn instagram_lines(text: &str, ids: &[String]) -> Vec<PageLine> {
    for at in places(text, "video_dash_manifest") {
        let rest = &text[at + "\"video_dash_manifest\"".len()..];
        let Some(rest) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let Some(serde_json::Value::String(xml)) = serde_json::Deserializer::from_str(rest.trim_start())
            .into_iter::<serde_json::Value>()
            .next()
            .and_then(|v| v.ok())
        else {
            continue;
        };
        let from = floor(text, at.saturating_sub(NEAR));
        let to = floor(text, (at + NEAR).min(text.len()));
        let window = &text[from..to];
        if !ids.iter().any(|id| xml.contains(id.as_str()) || window.contains(id.as_str())) {
            continue;
        }
        let mut lines = manifest_lines(&xml);
        let versions = places(text, "video_versions")
            .into_iter()
            .filter(|v| v.abs_diff(at) <= NEAR)
            .min_by_key(|v| v.abs_diff(at));
        if let Some(v) = versions {
            for item in array_after(text, v, "\"video_versions\"").unwrap_or_default() {
                let Some(url) = item
                    .get("url")
                    .and_then(|u| u.as_str())
                    .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
                else {
                    continue;
                };
                let height = item.get("height").and_then(|h| h.as_u64()).filter(|h| *h > 0);
                lines.push(PageLine {
                    url: url.to_string(),
                    label: match height {
                        Some(h) => format!("MP4 file, quality {h}p"),
                        None => "MP4 file".to_string(),
                    },
                    sound: None,
                });
            }
        }
        return lines;
    }
    Vec::new()
}

/// `at` moved back to a character boundary.
fn floor(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The value of `name="…"` in the start tag `tag`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!(" {name}=\"");
    let from = tag.find(&marker)? + marker.len();
    let len = tag[from..].find('"')?;
    Some(&tag[from..from + len])
}

/// `text` with the five XML entities read.
fn unescaped(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Seconds in an ISO 8601 duration such as `PT22.185099S` or `PT1M2.5S`.
fn seconds_of(duration: &str) -> Option<f64> {
    let mut rest = duration.strip_prefix("PT")?;
    let mut total = 0.0;
    while !rest.is_empty() {
        let end = rest.find(|c: char| c.is_ascii_alphabetic())?;
        let amount: f64 = rest[..end].parse().ok()?;
        total += amount
            * match rest[end..].chars().next()? {
                'H' => 3600.0,
                'M' => 60.0,
                'S' => 1.0,
                _ => return None,
            };
        rest = &rest[end + 1..];
    }
    Some(total)
}

/// The lines for the renditions of the DASH manifest the page holds for the video
/// whose `progressive_urls` are at `at`: each picture with its height, bitrate and the
/// video's length, and the best sound beside it to be joined. Empty when the page has
/// no such manifest or it is not one of this shape.
fn renditions(text: &str, at: usize) -> Vec<PageLine> {
    let Some(at_manifests) = places(text, "dash_manifests")
        .into_iter()
        .filter(|m| m.abs_diff(at) <= NEAR)
        .min_by_key(|m| m.abs_diff(at))
    else {
        return Vec::new();
    };
    let Some(items) = array_after(text, at_manifests, "\"dash_manifests\"") else {
        return Vec::new();
    };
    let Some(xml) = items
        .iter()
        .find_map(|item| item.get("manifest_xml").and_then(|x| x.as_str()))
    else {
        return Vec::new();
    };
    manifest_lines(xml)
}

/// The lines for the renditions a DASH manifest names: each picture with its height,
/// bitrate and the video's length, and the best sound beside it to be joined.
fn manifest_lines(xml: &str) -> Vec<PageLine> {
    let length = xml
        .find("<MPD ")
        .and_then(|i| xml[i..].find('>').map(|e| &xml[i..i + e]))
        .and_then(|tag| attr(tag, "mediaPresentationDuration"))
        .and_then(seconds_of);
    // Each representation, with the kind of its adaptation set.
    let mut pictures: Vec<(u64, u32, bool, String)> = Vec::new();
    let mut sounds: Vec<(u64, String)> = Vec::new();
    for (start, _) in xml.match_indices("<Representation ") {
        let Some(open_end) = xml[start..].find('>') else {
            continue;
        };
        let tag = &xml[start..start + open_end];
        let Some(close) = xml[start..].find("</Representation>") else {
            continue;
        };
        let body = &xml[start..start + close];
        let Some(base) = body
            .split_once("<BaseURL>")
            .and_then(|(_, rest)| rest.split_once("</BaseURL>"))
            .map(|(url, _)| unescaped(url))
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        else {
            continue;
        };
        let bandwidth: u64 = attr(tag, "bandwidth")
            .and_then(|b| b.parse().ok())
            .unwrap_or(0);
        let set = xml[..start].rfind("<AdaptationSet");
        let audio = set.is_some_and(|i| xml[i..start].contains("contentType=\"audio\""));
        if audio {
            sounds.push((bandwidth, base));
        } else if let Some(height) = attr(tag, "height").and_then(|h| h.parse::<u32>().ok()) {
            let hd = attr(tag, "FBQualityClass") == Some("hd");
            pictures.push((bandwidth, height, hd, base));
        }
    }
    sounds.sort_by_key(|(bandwidth, _)| std::cmp::Reverse(*bandwidth));
    let sound = sounds.into_iter().next().map(|(_, url)| url);
    pictures.sort_by_key(|(bandwidth, ..)| *bandwidth);
    pictures
        .into_iter()
        .map(|(bandwidth, height, hd, url)| {
            let mut label = String::from("MP4 file");
            if let Some(seconds) = length {
                label.push_str(&format!(", {} sec", seconds.round() as u64));
            }
            label.push_str(&format!(", quality {height}p{}", if hd { " HD" } else { "" }));
            if bandwidth > 0 {
                label.push_str(&format!(", {} kbps", bandwidth / 1000));
            }
            PageLine {
                url,
                label,
                sound: sound.clone(),
            }
        })
        .collect()
}
