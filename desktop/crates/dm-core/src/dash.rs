//! MPEG-DASH manifests (`.mpd`), read by dm itself.
//!
//! A manifest lists, for each picture and sound quality, where the pieces of
//! a fragmented MP4 or WebM are: a template that numbers them, an explicit
//! list, or one file with an index. This reads that into the addresses to
//! fetch (an initialisation piece first, then the media pieces in order),
//! picks one picture and one sound by the quality asked for, and leaves the
//! fetching and the joining to the download (`download::dash`, `mp4mux`,
//! `mkvmux`). Nothing is run; the manifest is outside input and every access
//! is checked.
//!
//! Not read, and said so rather than saved wrong: a live manifest
//! (`type="dynamic"`), one that changes period part-way, and one whose
//! pieces are encrypted by a rights system (`ContentProtection`).

// Reads outside input: an index out of range here is a crash a server could
// cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use url::Url;

use crate::stream::Quality;
use crate::Error;

/// Most pieces a track may list. A template over a long duration with a
/// tiny segment length would otherwise be a list of billions.
const MOST_PIECES: usize = 200_000;

/// One request: an address, and the bytes of it where it is part of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub url: String,
    /// First and last byte, inclusive.
    pub range: Option<(u64, u64)>,
    /// The key and vector the bytes are scrambled with (an HLS stream's
    /// AES-128), to be undone as the piece is fetched.
    pub lock: Option<Lock>,
}

/// An AES-128 key and the initialisation vector one piece is scrambled with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    pub key: Vec<u8>,
    pub iv: [u8; 16],
}

/// Whether a track carries picture or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    Video,
    Audio,
}

/// One quality of one medium, as the pieces to fetch in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub medium: Medium,
    pub id: String,
    pub bandwidth: u64,
    pub height: u32,
    /// WebM (VP8, VP9, AV1 with Opus or Vorbis) rather than MP4.
    pub webm: bool,
    pub pieces: Vec<Piece>,
}

/// What to fetch for a wanted quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub video: Option<Track>,
    pub audio: Option<Track>,
}

impl Plan {
    /// A string that changes whenever what would be fetched does, so parts
    /// staged for another quality are not adopted.
    pub fn signature(&self) -> String {
        let one = |t: &Option<Track>| match t {
            Some(t) => format!("{}:{}:{}", t.id, t.bandwidth, t.pieces.len()),
            None => "-".into(),
        };
        format!("dash {} {}", one(&self.video), one(&self.audio))
    }
}

// ---- the document ---------------------------------------------------------

#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
    text: String,
}

impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn num(&self, name: &str) -> Option<u64> {
        self.attr(name).and_then(|v| v.trim().parse().ok())
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name == name)
    }
    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.kids.iter().filter(move |k| k.name == name)
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or_default().to_string()
}

fn node_of(tag: &BytesStart) -> Node {
    let attrs = tag
        .attributes()
        .flatten()
        .map(|a| {
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| a.value.to_string());
            (local(a.key.as_ref()), value)
        })
        .collect();
    Node {
        name: local(tag.name().as_ref()),
        attrs,
        ..Node::default()
    }
}

/// The manifest as a tree. A document nested absurdly deep is refused.
fn parse(text: &str) -> Result<Node, Error> {
    const DEEPEST: usize = 64;
    let bad =
        |why: &str| Error::Internal(format!("that is not a DASH manifest dm can read: {why}"));
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    loop {
        match reader.read_event() {
            Err(e) => return Err(bad(&e.to_string())),
            Ok(Event::Eof) => break,
            Ok(Event::Start(tag)) => {
                if stack.len() >= DEEPEST {
                    return Err(bad("nested too deeply"));
                }
                stack.push(node_of(&tag));
            }
            Ok(Event::Empty(tag)) => {
                let node = node_of(&tag);
                match stack.last_mut() {
                    Some(parent) => parent.kids.push(node),
                    None => root = Some(node),
                }
            }
            Ok(Event::Text(chunk)) => {
                if let Some(top) = stack.last_mut() {
                    let raw = chunk.xml10_content();
                    match quick_xml::escape::unescape(&raw) {
                        Ok(decoded) => top.text.push_str(&decoded),
                        Err(_) => top.text.push_str(&raw),
                    }
                }
            }
            Ok(Event::End(_)) => {
                if let Some(done) = stack.pop() {
                    match stack.last_mut() {
                        Some(parent) => parent.kids.push(done),
                        None => root = Some(done),
                    }
                }
            }
            Ok(_) => {}
        }
    }
    root.filter(|r| r.name == "MPD")
        .ok_or_else(|| bad("no MPD element"))
}

// ---- reading it -----------------------------------------------------------

/// `PT1H2M3.5S` in seconds.
fn iso_seconds(text: &str) -> Option<f64> {
    let rest = text.trim().strip_prefix('P')?;
    let (date, time) = rest.split_once('T').unwrap_or((rest, ""));
    let mut total = 0.0;
    let mut number = String::new();
    for c in date.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
        } else {
            let n: f64 = number.parse().ok()?;
            number.clear();
            total += match c {
                'D' => n * 86_400.0,
                'W' => n * 604_800.0,
                'Y' | 'M' => return None,
                _ => return None,
            };
        }
    }
    for c in time.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
        } else {
            let n: f64 = number.parse().ok()?;
            number.clear();
            total += match c {
                'H' => n * 3600.0,
                'M' => n * 60.0,
                'S' => n,
                _ => return None,
            };
        }
    }
    Some(total)
}

/// The settings a Representation inherits from its AdaptationSet and
/// Period: its own template or list wins, then its set's.
struct Context<'a> {
    base: String,
    mpd: &'a Node,
    period: &'a Node,
    set: &'a Node,
    rep: &'a Node,
    duration: Option<f64>,
}

impl Context<'_> {
    /// A child element looked for on the representation, then its set, then
    /// the period.
    fn inherited(&self, name: &str) -> Option<&Node> {
        self.rep
            .child(name)
            .or_else(|| self.set.child(name))
            .or_else(|| self.period.child(name))
    }
}

fn join(base: &str, relative: &str) -> Result<String, Error> {
    Url::parse(base)
        .and_then(|b| b.join(relative.trim()))
        .map(|u| u.to_string())
        .map_err(|e| Error::Internal(format!("a manifest listed an address dm cannot read: {e}")))
}

/// `$Number%05d$`-style substitution. `$$` is a dollar sign. Unknown
/// identifiers make the template unreadable.
fn fill(template: &str, id: &str, bandwidth: u64, number: u64, time: u64) -> Option<String> {
    let mut out = String::new();
    let mut parts = template.split('$');
    out.push_str(parts.next()?);
    while let Some(ident) = parts.next() {
        let after = parts.next()?;
        if ident.is_empty() {
            out.push('$');
        } else {
            let (name, width) = match ident.split_once('%') {
                Some((n, f)) => {
                    let digits = f.strip_prefix('0')?.strip_suffix('d')?;
                    (n, digits.parse::<usize>().ok()?.min(32))
                }
                None => (ident, 0),
            };
            let value = match name {
                "RepresentationID" => {
                    out.push_str(id);
                    out.push_str(after);
                    continue;
                }
                "Number" => number,
                "Bandwidth" => bandwidth,
                "Time" => time,
                _ => return None,
            };
            out.push_str(&format!("{value:0width$}"));
        }
        out.push_str(after);
    }
    Some(out)
}

fn byte_range(text: &str) -> Option<(u64, u64)> {
    let (a, b) = text.trim().split_once('-')?;
    let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a <= b).then_some((a, b))
}

/// The pieces of one representation, initialisation first.
fn pieces_of(c: &Context) -> Result<Vec<Piece>, Error> {
    let id = c.rep.attr("id").unwrap_or_default();
    let bandwidth = c.rep.num("bandwidth").unwrap_or(0);
    let unreadable =
        |what: &str| Error::Internal(format!("a manifest's {what} is not one dm can read"));
    let mut pieces = Vec::new();

    if let Some(t) = c.inherited("SegmentTemplate") {
        // A template's attributes may be split between levels: the timeline
        // on the set and the media address on the representation, say.
        let attr = |name: &str| {
            c.rep
                .child("SegmentTemplate")
                .and_then(|n| n.attr(name))
                .or_else(|| c.set.child("SegmentTemplate").and_then(|n| n.attr(name)))
                .or_else(|| c.period.child("SegmentTemplate").and_then(|n| n.attr(name)))
        };
        let timescale = attr("timescale")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(1);
        let start = attr("startNumber")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);
        if let Some(init) = attr("initialization") {
            let url = fill(init, id, bandwidth, 0, 0)
                .ok_or_else(|| unreadable("initialization template"))?;
            pieces.push(Piece {
                url: join(&c.base, &url)?,
                range: None,
                lock: None,
            });
        }
        let media = attr("media").ok_or_else(|| unreadable("media template"))?;
        let at = |number: u64, time: u64| -> Result<Piece, Error> {
            let url = fill(media, id, bandwidth, number, time)
                .ok_or_else(|| unreadable("media template"))?;
            Ok(Piece {
                url: join(&c.base, &url)?,
                range: None,
                lock: None,
            })
        };
        let timeline = c
            .rep
            .child("SegmentTemplate")
            .and_then(|n| n.child("SegmentTimeline"))
            .or_else(|| {
                c.set
                    .child("SegmentTemplate")
                    .and_then(|n| n.child("SegmentTimeline"))
            })
            .or_else(|| t.child("SegmentTimeline"));
        if let Some(timeline) = timeline {
            let mut time = 0u64;
            let mut number = start;
            for (n, s) in timeline.children("S").enumerate() {
                if let Some(t0) = s.num("t") {
                    time = t0;
                } else if n == 0 {
                    time = 0;
                }
                let d = s
                    .num("d")
                    .filter(|d| *d > 0)
                    .ok_or_else(|| unreadable("timeline"))?;
                let repeats = match s.attr("r").and_then(|r| r.trim().parse::<i64>().ok()) {
                    Some(r) if r >= 0 => r as u64,
                    // A negative repeat runs to the period's end.
                    Some(_) => {
                        let total =
                            c.duration.ok_or_else(|| unreadable("timeline"))? * timescale as f64;
                        (((total - time as f64) / d as f64).ceil() as i64 - 1).max(0) as u64
                    }
                    None => 0,
                };
                for _ in 0..=repeats {
                    if pieces.len() >= MOST_PIECES {
                        return Err(unreadable("timeline (far too many pieces)"));
                    }
                    pieces.push(at(number, time)?);
                    time = time.saturating_add(d);
                    number += 1;
                }
            }
        } else {
            let d = attr("duration")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|d| *d > 0.0)
                .ok_or_else(|| unreadable("segment duration"))?;
            let seconds = c.duration.ok_or_else(|| unreadable("duration"))?;
            let count = (seconds * timescale as f64 / d).ceil();
            if !(count.is_finite() && count > 0.0 && count <= MOST_PIECES as f64) {
                return Err(unreadable("segment count"));
            }
            for n in 0..count as u64 {
                pieces.push(at(start + n, n * d as u64)?);
            }
        }
        return Ok(pieces);
    }

    if let Some(list) = c.inherited("SegmentList") {
        if let Some(init) = list.child("Initialization") {
            if let Some(src) = init.attr("sourceURL") {
                pieces.push(Piece {
                    url: join(&c.base, src)?,
                    range: init.attr("range").and_then(byte_range),
                    lock: None,
                });
            } else {
                pieces.push(Piece {
                    url: c.base.clone(),
                    range: init.attr("range").and_then(byte_range),
                    lock: None,
                });
            }
        }
        for s in list.children("SegmentURL") {
            if pieces.len() >= MOST_PIECES {
                return Err(unreadable("segment list (far too many pieces)"));
            }
            let url = match s.attr("media") {
                Some(m) => join(&c.base, m)?,
                None => c.base.clone(),
            };
            pieces.push(Piece {
                url,
                range: s.attr("mediaRange").and_then(byte_range),
                lock: None,
            });
        }
        return Ok(pieces);
    }

    // One file: a fragmented MP4 whose index sits in `indexRange` is still
    // fetched whole, its fragments and index in the order they lie.
    if c.rep.child("BaseURL").is_some() || c.inherited("SegmentBase").is_some() {
        return Ok(vec![Piece {
            url: c.base.clone(),
            range: None,
            lock: None,
        }]);
    }
    Err(unreadable("segment addressing"))
}

/// What a representation says about itself.
fn mime_of<'a>(c: &Context<'a>) -> &'a str {
    c.rep
        .attr("mimeType")
        .or_else(|| c.set.attr("mimeType"))
        .unwrap_or_default()
}

fn medium_of(c: &Context) -> Option<Medium> {
    let kind = c
        .set
        .attr("contentType")
        .or_else(|| c.rep.attr("contentType"))
        .unwrap_or_default();
    let mime = mime_of(c);
    let codecs = c
        .rep
        .attr("codecs")
        .or_else(|| c.set.attr("codecs"))
        .unwrap_or_default();
    if kind == "video" || mime.starts_with("video/") {
        Some(Medium::Video)
    } else if kind == "audio" || mime.starts_with("audio/") {
        Some(Medium::Audio)
    } else if c.rep.attr("height").is_some() || c.set.attr("height").is_some() {
        Some(Medium::Video)
    } else if codecs.starts_with("mp4a") || codecs == "opus" {
        Some(Medium::Audio)
    } else {
        None
    }
}

/// A base address with the BaseURLs of each level joined on.
fn base_of(base: &str, node: &Node) -> Result<String, Error> {
    match node.child("BaseURL") {
        Some(b) if !b.text.trim().is_empty() => join(base, &b.text),
        _ => Ok(base.to_string()),
    }
}

struct Candidate {
    track: Track,
    /// A main-role (or role-less) sound is preferred over a commentary.
    main: bool,
}

/// Every representation of the first period, readable or not.
fn candidates(root: &Node, url: &str) -> Result<Vec<Candidate>, Error> {
    if root.attr("type") == Some("dynamic") {
        return Err(Error::Internal(
            "this DASH stream is live, which dm does not record".into(),
        ));
    }
    let periods: Vec<&Node> = root.children("Period").collect();
    if periods.len() > 1 {
        return Err(Error::Internal(
            "this stream changes period part-way through, which dm does not join".into(),
        ));
    }
    let period = periods
        .first()
        .copied()
        .ok_or_else(|| Error::Internal("that manifest lists nothing to download".into()))?;
    let duration = period
        .attr("duration")
        .and_then(iso_seconds)
        .or_else(|| root.attr("mediaPresentationDuration").and_then(iso_seconds));
    let top = base_of(url, root)?;
    let period_base = base_of(&top, period)?;

    let mut out = Vec::new();
    for set in period.children("AdaptationSet") {
        if set.child("ContentProtection").is_some() {
            // Copy protection, not scrambling that is handed to any player.
            continue;
        }
        let set_base = base_of(&period_base, set)?;
        let main = set
            .children("Role")
            .all(|r| r.attr("value").is_none_or(|v| v == "main"));
        for rep in set.children("Representation") {
            if rep.child("ContentProtection").is_some() {
                continue;
            }
            let c = Context {
                base: base_of(&set_base, rep)?,
                mpd: root,
                period,
                set,
                rep,
                duration,
            };
            let _ = c.mpd;
            let Some(medium) = medium_of(&c) else {
                continue;
            };
            let mime = mime_of(&c);
            let webm = mime.ends_with("webm");
            if !(mime.is_empty() || mime.ends_with("mp4") || webm) {
                continue; // subtitles, thumbnails
            }
            let pieces = pieces_of(&c)?;
            if pieces.is_empty() {
                continue;
            }
            out.push(Candidate {
                track: Track {
                    medium,
                    id: rep.attr("id").unwrap_or_default().to_string(),
                    bandwidth: rep.num("bandwidth").unwrap_or(0),
                    height: rep.num("height").or_else(|| set.num("height")).unwrap_or(0) as u32,
                    webm,
                    pieces,
                },
                main,
            });
        }
    }
    if out.is_empty() {
        let protected = period
            .children("AdaptationSet")
            .any(|s| s.child("ContentProtection").is_some());
        return Err(Error::Internal(
            if protected {
                "this stream is protected by a rights system, which dm does not decrypt"
            } else {
                "that manifest lists no picture or sound dm can fetch"
            }
            .into(),
        ));
    }
    Ok(out)
}

/// The picture to take at `quality`: the tallest at or below a height (the
/// shortest above it when nothing is that small), the tallest for best.
/// `container` ("webm") prefers that container among equals; otherwise MP4.
fn pick_video(mut videos: Vec<Track>, quality: Quality, container: Option<&str>) -> Option<Track> {
    let want_webm = container == Some("webm");
    if let Quality::UpTo(limit) = quality {
        let under: Vec<Track> = videos
            .iter()
            .filter(|t| t.height <= limit)
            .cloned()
            .collect();
        if !under.is_empty() {
            videos = under;
        } else {
            let lowest = videos.iter().map(|t| t.height).min()?;
            videos.retain(|t| t.height == lowest);
        }
    }
    videos
        .into_iter()
        .max_by_key(|t| (t.height, t.webm == want_webm, t.bandwidth))
}

/// The sound to take: a main track's, in the picture's container where
/// there is one, the best bandwidth.
fn pick_audio(audios: Vec<Candidate>, with_webm: bool) -> Option<Track> {
    audios
        .into_iter()
        .max_by_key(|c| (c.main, c.track.webm == with_webm, c.track.bandwidth))
        .map(|c| c.track)
}

/// What to fetch from a manifest's text, served at `url`.
pub fn plan(
    text: &str,
    url: &str,
    quality: Quality,
    container: Option<&str>,
) -> Result<Plan, Error> {
    let root = parse(text)?;
    let all = candidates(&root, url)?;
    let (videos, audios): (Vec<Candidate>, Vec<Candidate>) = all
        .into_iter()
        .partition(|c| c.track.medium == Medium::Video);
    let video = if quality == Quality::Audio {
        None
    } else {
        pick_video(
            videos.into_iter().map(|c| c.track).collect(),
            quality,
            container,
        )
    };
    let audio = pick_audio(audios, video.as_ref().is_some_and(|v| v.webm));
    if video.is_none() && audio.is_none() {
        return Err(Error::Internal(
            "that manifest lists no picture or sound dm can fetch".into(),
        ));
    }
    Ok(Plan { video, audio })
}

#[cfg(test)]
mod tests;
