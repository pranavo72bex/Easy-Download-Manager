//! The video files a page lists in its own data, for the panel's list.
//!
//! Some sites put the addresses of a video's files in the page itself, not only
//! in what the player fetches. The player's own requests name one rendition; the
//! page names them all. No site is named here: the files the player fetched are
//! looked for in the page's JSON, the list of renditions around them is read by
//! the names of its fields (a height, a bitrate, a quality), and a DASH manifest
//! held as a string is read for its representations.

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

/// A key that names a file in an address: the part after the last slash, up to
/// its extension. Long enough to be this file's and no other's.
fn file_id(url: &str) -> Option<String> {
    // The last non-empty part of the path: some addresses end in a slash.
    let name = url
        .split(['?', '#'])
        .next()?
        .trim_end_matches('/')
        .rsplit('/')
        .next()?;
    let id = name.split('.').next()?;
    (id.len() >= 24).then(|| id.to_string())
}

use serde_json::Value;

/// One enclosing JSON container at a spot: whether it is an array, and where it starts.
type Open = (bool, usize);

/// A string of the region that holds one of the files, with the containers around it.
struct Hit {
    open: Vec<Open>,
    string: (usize, usize),
}

/// How many hits one page is read for, and how deep the containers are followed.
const MOST_HITS: usize = 40;
const MOST_DEPTH: usize = 64;
/// How many enclosing objects are searched for the video's other files.
const WIDEN: usize = 4;

/// The parts of `text` that are scripts, or the whole of it when it has none.
fn regions(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("<script") {
        let after = &rest[at..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let body = &after[open_end + 1..];
        let end = body.find("</script>").unwrap_or(body.len());
        found.push(&body[..end]);
        rest = &body[end..];
    }
    if found.is_empty() {
        found.push(text);
    }
    found
}

/// Each string of `region` that holds one of `ids`, with the containers it sits in.
fn hits(region: &str, ids: &[String]) -> Vec<Hit> {
    let spots: Vec<usize> = ids
        .iter()
        .flat_map(|id| region.match_indices(id.as_str()).map(|(at, _)| at))
        .collect();
    let mut found = Vec::new();
    if spots.is_empty() {
        return found;
    }
    let bytes = region.as_bytes();
    let mut open: Vec<Open> = Vec::new();
    let mut string: Option<usize> = None;
    let mut i = 0;
    while found.len() < MOST_HITS {
        let Some(&b) = bytes.get(i) else {
            break;
        };
        match (string, b) {
            (Some(_), b'\\') => i += 1,
            (Some(start), b'"') => {
                if spots.iter().any(|&p| p > start && p < i) {
                    found.push(Hit {
                        open: open.clone(),
                        string: (start, i + 1),
                    });
                }
                string = None;
            }
            (Some(_), _) => {}
            (None, b'"') => string = Some(i),
            (None, b'{' | b'[') if open.len() < MOST_DEPTH => open.push((b == b'[', i)),
            (None, b'}' | b']') => {
                open.pop();
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// The JSON value that starts at `at` in `region`.
fn value_at(region: &str, at: usize) -> Option<Value> {
    serde_json::Deserializer::from_str(region.get(at..)?)
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// Whether an address names a picture, a caption file or a script, not a video.
fn not_video(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        ".jpg", ".jpeg", ".png", ".webp", ".gif", ".svg", ".vtt", ".srt", ".json", ".js", ".css",
        ".html",
    ]
    .iter()
    .any(|ext| path.ends_with(ext))
}

fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Whether an address looks like a video, audio or stream by itself.
fn looks_like_media(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    is_web(url)
        && !not_video(url)
        && ([
            ".mp4", ".webm", ".m3u8", ".mpd", ".m4v", ".mov", ".mkv", ".m4a", ".mp3",
        ]
        .iter()
        .any(|ext| path.ends_with(ext))
            || lower.contains("mime_type=video")
            || lower.contains("mime_type=audio"))
}

/// What one entry of a list of renditions says about its file.
#[derive(Default)]
struct Found {
    url: String,
    height: u64,
    bitrate: u64,
    quality: Option<String>,
}

/// Whether `text` is an address this read takes: any that is not a picture, or only
/// one that looks like media.
fn takes(text: &str, any_url: bool) -> bool {
    if any_url {
        is_web(text) && !not_video(text)
    } else {
        looks_like_media(text)
    }
}

/// An entry read by the names of its fields: the first address in it (and in the
/// objects and lists inside it), a field named height, one named for a bitrate or
/// bandwidth, and a short quality or label.
fn describe(value: &Value, any_url: bool, depth: u8, found: &mut Found) {
    let Value::Object(map) = value else { return };
    for (key, field) in map {
        let key = key.to_ascii_lowercase();
        match field {
            Value::String(text) => {
                if takes(text, any_url) {
                    if found.url.is_empty() {
                        found.url.clone_from(text);
                    }
                } else if (key.contains("quality") || key.contains("label"))
                    && found.quality.is_none()
                    && !text.is_empty()
                    && text.len() <= 12
                    && !is_web(text)
                {
                    found.quality = Some(text.clone());
                }
            }
            Value::Number(n) => {
                let n = n.as_u64().unwrap_or(0);
                if key.ends_with("height") && found.height == 0 {
                    found.height = n;
                } else if (key.contains("bitrate") || key.contains("bandwidth"))
                    && found.bitrate == 0
                {
                    found.bitrate = n;
                }
            }
            Value::Array(items) if depth < 3 => {
                for item in items {
                    match item {
                        Value::String(text) => {
                            if takes(text, any_url) && found.url.is_empty() {
                                found.url.clone_from(text);
                            }
                        }
                        other => describe(other, any_url, depth + 1, found),
                    }
                }
            }
            Value::Object(_) if depth < 3 => describe(field, any_url, depth + 1, found),
            _ => {}
        }
    }
}

fn is_stream(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    path.ends_with(".m3u8") || path.ends_with(".mpd")
}

/// The line for one entry: what kind of file, its quality, its bitrate.
fn line_of(found: Found) -> PageLine {
    let path = found
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut label = String::from(if path.ends_with(".m3u8") {
        "HLS stream"
    } else if path.ends_with(".mpd") {
        "DASH stream"
    } else if path.ends_with(".webm") {
        "WebM file"
    } else {
        "MP4 file"
    });
    if found.height > 0 {
        label.push_str(&format!(", quality {}p", found.height));
    } else if let Some(quality) = &found.quality {
        label.push_str(&format!(", quality {quality}"));
    }
    if found.bitrate > 0 {
        label.push_str(&format!(", {} kbps", found.bitrate / 1000));
    }
    PageLine {
        url: found.url,
        label,
        sound: None,
    }
}

/// What was read from a page for one video.
#[derive(Default)]
struct Gathered {
    /// The representations of a DASH manifest, each with its sound.
    manifests: Vec<PageLine>,
    /// The page's own files.
    files: Vec<PageLine>,
    /// Streams the page names, offered only where no manifest could be read.
    streams: Vec<PageLine>,
    /// The video's caption files, offered after its files (gaps row 72c).
    captions: Vec<PageLine>,
}

impl Gathered {
    fn add_file(&mut self, line: PageLine) {
        let list = if is_stream(&line.url) {
            &mut self.streams
        } else {
            &mut self.files
        };
        if !list.iter().any(|l| l.url == line.url) {
            list.push(line);
        }
    }

    fn add_manifest(&mut self, xml: &str) {
        for line in manifest_lines(xml) {
            if !self.manifests.iter().any(|l| l.url == line.url) {
                self.manifests.push(line);
            }
        }
    }

    fn has(&self, url: &str) -> bool {
        self.manifests
            .iter()
            .chain(&self.files)
            .chain(&self.streams)
            .any(|l| l.url == url)
    }

    fn count(&self) -> usize {
        self.manifests.len() + self.files.len() + self.streams.len()
    }

    fn into_lines(self) -> Vec<PageLine> {
        let mut lines = self.manifests;
        let manifests = !lines.is_empty();
        lines.extend(self.files);
        if !manifests {
            lines.extend(self.streams);
        }
        lines.extend(self.captions);
        lines
    }

    /// The caption files in the video's record, however deep: an address ending `.srt`, `.vtt`
    /// or `.ttml`, or one under a key naming a caption or subtitle (gaps row 72c).
    fn add_captions(&mut self, value: &Value, key: &str, depth: usize) {
        if depth > 6 || self.captions.len() >= 20 {
            return;
        }
        match value {
            Value::String(url) if is_web(url) => {
                let named = {
                    let key = key.to_ascii_lowercase();
                    key.contains("caption") || key.contains("subtitle")
                };
                if let Some(kind) = caption_kind(url, named) {
                    if !self.captions.iter().any(|l| l.url == *url) {
                        self.captions.push(PageLine {
                            url: url.clone(),
                            label: format!("{kind} file"),
                            sound: None,
                        });
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.add_captions(item, key, depth + 1);
                }
            }
            Value::Object(map) => {
                for (name, field) in map {
                    // A field under a caption key keeps that key's meaning.
                    let named = key.to_ascii_lowercase();
                    let carried = if named.contains("caption") || named.contains("subtitle") {
                        key
                    } else {
                        name.as_str()
                    };
                    self.add_captions(field, carried, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// What kind of caption file an address is, by its ending ("SRT", "VTT", "TTML"); one under a
/// caption or subtitle key whose last part names no type at all is taken as SRT, the kind such
/// keys hold. Anything else (a picture, a page, a script, a video) is not a caption file.
fn caption_kind(url: &str, named: bool) -> Option<&'static str> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if path.ends_with(".srt") {
        Some("SRT")
    } else if path.ends_with(".vtt") {
        Some("VTT")
    } else if path.ends_with(".ttml") || path.ends_with(".dfxp") {
        Some("TTML")
    } else if named && !last.is_empty() && !last.contains('.') {
        Some("SRT")
    } else {
        None
    }
}

/// A list of entries each carrying a file, read as renditions. `strict` takes only
/// addresses that look like media; sorted from the largest bitrate down where all have one.
fn rendition_list(items: &[Value], strict: bool) -> Vec<PageLine> {
    let mut entries: Vec<Found> = Vec::new();
    for item in items {
        let mut found = Found::default();
        describe(item, !strict, 0, &mut found);
        if !found.url.is_empty() {
            entries.push(found);
        }
    }
    if entries.iter().all(|e| e.bitrate > 0) {
        // The best first, as the other manager lists them.
        entries.sort_by_key(|e| std::cmp::Reverse(e.bitrate));
    }
    entries.into_iter().map(line_of).collect()
}

/// What a value of the video's record holds: a manifest as a string, a list of entries
/// with manifests in them, or a list of files that look like media.
fn gather_value(value: &Value, into: &mut Gathered) {
    match value {
        Value::String(text) if text.contains("<MPD") => into.add_manifest(text),
        Value::Array(items) if !items.is_empty() && items.iter().all(|i| i.is_object()) => {
            let mut manifest = false;
            for item in items {
                if let Value::Object(map) = item {
                    for field in map.values() {
                        if let Value::String(text) = field {
                            if text.contains("<MPD") {
                                into.add_manifest(text);
                                manifest = true;
                            }
                        }
                    }
                }
            }
            if !manifest {
                for line in rendition_list(items, true) {
                    into.add_file(line);
                }
            }
        }
        _ => {}
    }
}

/// The lines a page offers for the video the player is playing, given the
/// addresses the player has been seen to fetch (`seen`): the page's list of
/// renditions that holds one of those files, and the DASH manifest beside it.
/// Nothing when the page names no video that matches.
pub fn page_lines(text: &str, seen: &[String]) -> Vec<PageLine> {
    let ids: Vec<String> = seen.iter().filter_map(|u| file_id(u)).collect();
    lines_for_ids(text, &ids)
}

/// The lines a page offers for the video a site's finder said a player is (`id`, its id on that
/// site): what the page's answers hold around that id. Nothing for an id too short to be told
/// from any other number in the text.
pub fn page_lines_for_player(text: &str, id: &str) -> Vec<PageLine> {
    let id = id.trim();
    if id.len() < 6 || id.len() > 200 || id.contains(['"', '\\']) {
        return Vec::new();
    }
    for region in regions(text) {
        for hit in hits(region, &[id.to_string()]) {
            // The innermost objects around the id: their own fields are the video's files.
            for (_, start) in hit
                .open
                .iter()
                .rev()
                .filter(|(array, _)| !*array)
                .take(WIDEN)
            {
                let Some(Value::Object(map)) = value_at(region, *start) else {
                    continue;
                };
                let mut gathered = Gathered::default();
                for field in map.values() {
                    gather_value(field, &mut gathered);
                }
                if gathered.count() > 0 {
                    for (name, field) in &map {
                        gathered.add_captions(field, name, 0);
                    }
                    return gathered.into_lines();
                }
            }
        }
    }
    Vec::new()
}

fn lines_for_ids(text: &str, ids: &[String]) -> Vec<PageLine> {
    if ids.is_empty() {
        return Vec::new();
    }
    let ids = ids.to_vec();
    for region in regions(text) {
        for hit in hits(region, &ids) {
            let Some(Value::String(own)) = value_at(region, hit.string.0) else {
                continue;
            };
            let mut gathered = Gathered::default();
            if own.contains("<MPD") {
                gathered.add_manifest(&own);
            } else {
                // The innermost list of objects around the file is the renditions.
                for (_, start) in hit.open.iter().rev().filter(|(array, _)| *array) {
                    if let Some(Value::Array(items)) = value_at(region, *start) {
                        if !items.is_empty() && items.iter().all(|i| i.is_object()) {
                            for line in rendition_list(&items, false) {
                                gathered.add_file(line);
                            }
                            break;
                        }
                    }
                }
            }
            // The video's other files: the neighbours of the hit in the objects around it.
            for (_, start) in hit
                .open
                .iter()
                .rev()
                .filter(|(array, _)| !*array)
                .take(WIDEN)
            {
                let Some(Value::Object(map)) = value_at(region, *start) else {
                    continue;
                };
                let mut more = Gathered::default();
                for field in map.values() {
                    gather_value(field, &mut more);
                }
                // The video's caption files, from the innermost record around it that has any:
                // a wider one can hold other videos' too.
                if gathered.captions.is_empty() {
                    for (name, field) in &map {
                        gathered.add_captions(field, name, 0);
                    }
                }
                let mut extra = 0;
                for line in more.manifests {
                    if !gathered.has(&line.url) {
                        gathered.manifests.push(line);
                        extra += 1;
                    }
                }
                for line in more.files.into_iter().chain(more.streams) {
                    if extra < 12 && !gathered.has(&line.url) {
                        gathered.add_file(line);
                        extra += 1;
                    }
                }
                if extra > 0 {
                    break;
                }
            }
            if gathered.count() > 0 {
                return gathered.into_lines();
            }
        }
    }
    Vec::new()
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
    pictures.sort_by_key(|(bandwidth, ..)| std::cmp::Reverse(*bandwidth));
    pictures
        .into_iter()
        .map(|(bandwidth, height, hd, url)| {
            let mut label = String::from("MP4 file");
            if let Some(seconds) = length {
                label.push_str(&format!(", {} sec", seconds.round() as u64));
            }
            label.push_str(&format!(
                ", quality {height}p{}",
                if hd { " HD" } else { "" }
            ));
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
