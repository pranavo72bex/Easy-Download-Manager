// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
//! A manifest of several periods has them joined where each period's
//! picture and sound are alike (`periods`); a live one (`type="dynamic"`) is
//! read afresh as it grows (`periods::live`). Not read, and said so rather
//! than saved wrong: pieces encrypted by a rights system (`ContentProtection`).

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
    /// The address to fetch.
    pub url: String,
    /// First and last byte, inclusive.
    pub range: Option<(u64, u64)>,
    /// The key and vector the bytes are scrambled with (an HLS stream's
    /// AES-128), to be undone as the piece is fetched.
    pub lock: Option<Lock>,
    /// How long it plays, in milliseconds; 0 for an initialisation piece, and
    /// where the manifest says nothing of it.
    pub millis: u64,
}

/// An AES-128 key and the initialisation vector one piece is scrambled with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    /// The AES-128 key, 16 bytes.
    pub key: Vec<u8>,
    /// The initialisation vector, 16 bytes.
    pub iv: [u8; 16],
}

/// Whether a track carries picture or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    /// A picture track.
    Video,
    /// A sound track.
    Audio,
}

/// One quality of one medium, as the pieces to fetch in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Whether the track is picture or sound.
    pub medium: Medium,
    /// The id the manifest gave the track.
    pub id: String,
    /// Bits per second the manifest says the track needs.
    pub bandwidth: u64,
    /// The picture's shorter side, which is what a quality names (a
    /// portrait video's width); 0 for sound.
    pub height: u32,
    /// WebM (VP8, VP9, AV1 with Opus or Vorbis) rather than MP4.
    pub webm: bool,
    /// The pieces to fetch, in playing order.
    pub pieces: Vec<Piece>,
    /// Set where the track is one file whose own index (`sidx`) says where its
    /// pieces lie: a part of it is then cut by that index, not by the list.
    pub indexed: Option<Indexed>,
    /// Where each later period begins, in a track joined from several periods
    /// of a manifest; empty for one period.
    pub seams: Vec<Seam>,
}

/// Where a later period begins in a track joined from several (`pieces[at..]`
/// are its pieces), with the initialisation piece that period lists. That
/// piece is not in `pieces`: the first period's stands for it, once the
/// download has found the two alike (`download::dash`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seam {
    /// The index in `pieces` of the period's first piece.
    pub at: usize,
    /// The period's own initialisation piece.
    pub init: Piece,
}

/// Where the first boxes and the index of a one-file track are, where the
/// manifest said (otherwise they are looked for at the file's start).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indexed {
    /// The bytes of the file's first boxes (`init`), first and last, inclusive.
    pub init: Option<(u64, u64)>,
    /// The bytes of the file's index, first and last, inclusive.
    pub index: Option<(u64, u64)>,
}

/// What to fetch for a wanted quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The picture track to fetch, where the quality has one.
    pub video: Option<Track>,
    /// The sound track to fetch, where it is listed apart from the picture.
    pub audio: Option<Track>,
    /// Whether a taller picture than the one taken for the best was listed copy-protected, and
    /// passed over: the row says so (`download::NOTES`, gaps row Y19).
    pub protected_above: bool,
    /// How many of the manifest's periods are not in the plan, as their picture or sound is
    /// not like the rest's (an advert in another format): the row says so (gaps row Y18).
    pub left_out: usize,
    /// The subtitles listed as WebVTT, one file or pieces, one a language (`texts::texts_of`):
    /// put inside the file where Options says so (gaps row Y20).
    pub texts: Vec<Text>,
}

/// A subtitle track a manifest lists as WebVTT: one file, or pieces of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// Its language (`lang`), as a file's name can carry it; `und` where it names none.
    pub language: String,
    /// The file's address, or the one its pieces are named from.
    pub url: String,
    /// What to fetch, in order: the one file, or each piece a `SegmentTemplate` or
    /// `SegmentList` names (an initialisation piece first, where it names one).
    pub pieces: Vec<Piece>,
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
            // `&amp;` and the like in text come apart from it, as references of their own.
            Ok(Event::GeneralRef(entity)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&entity_text(&entity));
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

/// What a reference in a document's text stands for: a character, one of XML's five, or (one
/// dm does not know) the reference as it was written.
pub(crate) fn entity_text(entity: &quick_xml::events::BytesRef) -> String {
    if let Ok(Some(c)) = entity.resolve_char_ref() {
        return c.to_string();
    }
    let name: &str = entity;
    match quick_xml::escape::resolve_predefined_entity(name) {
        Some(text) => text.to_string(),
        None => format!("&{name};"),
    }
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
    /// Set for a live manifest: how much of the period has been made so far.
    live: Option<periods::Clock>,
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
    let joined = Url::parse(base)
        .and_then(|b| b.join(relative.trim()))
        .map(|u| u.to_string())
        .map_err(|e| {
            Error::Internal(format!("a manifest listed an address dm cannot read: {e}"))
        })?;
    // A public manifest may not send dm inside, as a redirect may not (`redirects::named_inside`).
    if crate::redirects::named_inside(base, &joined) {
        return Err(Error::Internal(crate::http_client::INWARD_REDIRECT.into()));
    }
    Ok(joined)
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

/// The pieces of one representation, initialisation first, and whether there
/// is one to lead with.
fn pieces_of(c: &Context) -> Result<(Vec<Piece>, bool), Error> {
    let unreadable =
        |what: &str| Error::Internal(format!("a manifest's {what} is not one dm can read"));

    if let Some(t) = c.inherited("SegmentTemplate") {
        return pieces::template_pieces(c, t);
    }

    if let Some(list) = c.inherited("SegmentList") {
        return pieces::list_pieces(c, list);
    }

    // One file: a fragmented MP4 whose index sits in `indexRange` is still
    // fetched whole, its fragments and index in the order they lie.
    if c.rep.child("BaseURL").is_some() || c.inherited("SegmentBase").is_some() {
        let whole = Piece {
            url: c.base.clone(),
            range: None,
            lock: None,
            millis: 0,
        };
        return Ok((vec![whole], false));
    }
    Err(unreadable("segment addressing"))
}

/// Whether a representation is one file with an index of its own
/// (`SegmentBase`), and where the manifest says the first boxes and the
/// index lie.
fn indexed_of(c: &Context) -> Option<Indexed> {
    if c.inherited("SegmentTemplate").is_some() || c.inherited("SegmentList").is_some() {
        return None;
    }
    let base = c.inherited("SegmentBase")?;
    Some(Indexed {
        init: base
            .child("Initialization")
            .and_then(|i| i.attr("range"))
            .and_then(byte_range),
        index: base.attr("indexRange").and_then(byte_range),
    })
}

/// The part of a track that plays within `from`..`to` seconds: its
/// initialisation pieces (which play for no time) and every timed piece that
/// overlaps, with the seconds where the first starts and the last ends.
/// `None` where the pieces carry no timing or none overlaps. A one-file track
/// is cut by its own index at fetch time, not here.
pub fn cut(track: &Track, from: f64, to: f64) -> Option<(Track, f64, f64)> {
    let mut pieces: Vec<Piece> = Vec::new();
    let (mut at, mut starts, mut ends, mut any) = (0.0f64, 0.0f64, 0.0f64, false);
    for piece in &track.pieces {
        if piece.millis == 0 && !any {
            pieces.push(piece.clone());
            continue;
        }
        let end = at + piece.millis as f64 / 1000.0;
        if piece.millis > 0 && at < to && end > from {
            if !any {
                starts = at;
                any = true;
            }
            ends = end;
            pieces.push(piece.clone());
        }
        at = end;
    }
    any.then(|| {
        (
            Track {
                pieces,
                ..track.clone()
            },
            starts,
            ends,
        )
    })
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
    /// The language its sound speaks (`lang`), where it names one: for a sound language asked
    /// for (gaps row Y10).
    lang: Option<String>,
    /// A sound describing the picture (`Role` `description`, or the TV-Anytime purpose `1`):
    /// taken in its language only where no plain track speaks it.
    described: bool,
    /// What a later period's track must say alike to be joined after this
    /// one: its medium, container, codec, picture size and sound rate. `None`
    /// where it is not joined to another period at all: a WebM, whose times
    /// each period starts again, or a track with no initialisation piece.
    shape: Option<String>,
    /// Whether `track.pieces` leads with its initialisation piece.
    init: bool,
}

/// Every representation of one period, readable or not.
fn candidates(root: &Node, period: &periods::PeriodAt, url: &str) -> Result<Vec<Candidate>, Error> {
    let top = base_of(url, root)?;
    let node = period.node;
    let period_base = base_of(&top, node)?;

    let mut out = Vec::new();
    for set in node.children("AdaptationSet") {
        if set.child("ContentProtection").is_some() {
            // Copy protection, not scrambling that is handed to any player.
            continue;
        }
        let set_base = base_of(&period_base, set)?;
        let main = set
            .children("Role")
            .all(|r| r.attr("value").is_none_or(|v| v == "main"));
        let described = set
            .children("Role")
            .any(|r| r.attr("value") == Some("description"))
            || set.children("Accessibility").any(|a| {
                a.attr("schemeIdUri")
                    .is_some_and(|s| s.contains("AudioPurposeCS"))
                    && a.attr("value") == Some("1")
            });
        for rep in set.children("Representation") {
            if rep.child("ContentProtection").is_some() {
                continue;
            }
            let c = Context {
                base: base_of(&set_base, rep)?,
                mpd: root,
                period: node,
                set,
                rep,
                duration: period.duration,
                live: period.clock,
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
            let (pieces, init) = pieces_of(&c)?;
            if pieces.is_empty() {
                continue;
            }
            let said = |name: &str| rep.attr(name).or_else(|| set.attr(name)).unwrap_or("");
            let shape = (init && !webm).then(|| {
                format!(
                    "{medium:?} {mime} {} {}x{} {}",
                    said("codecs"),
                    said("width"),
                    said("height"),
                    said("audioSamplingRate")
                )
            });
            out.push(Candidate {
                track: Track {
                    medium,
                    id: rep.attr("id").unwrap_or_default().to_string(),
                    bandwidth: rep.num("bandwidth").unwrap_or(0),
                    height: {
                        let h = rep.num("height").or_else(|| set.num("height")).unwrap_or(0);
                        let w = rep.num("width").or_else(|| set.num("width")).unwrap_or(0);
                        (if w > 0 && w < h { w } else { h }) as u32
                    },
                    webm,
                    indexed: indexed_of(&c),
                    pieces,
                    seams: Vec::new(),
                },
                main,
                lang: rep
                    .attr("lang")
                    .or_else(|| set.attr("lang"))
                    .map(str::to_string),
                described,
                shape,
                init,
            });
        }
    }
    if out.is_empty() {
        let protected = node
            .children("AdaptationSet")
            .any(|s| s.child("ContentProtection").is_some());
        return Err(Error::Internal(
            // The same words as HLS's (`stream::PROTECTED`, gaps row Y19).
            if protected {
                crate::stream::PROTECTED
            } else {
                NOTHING_FETCHABLE
            }
            .into(),
        ));
    }
    Ok(out)
}

/// What a manifest of nothing dm can fetch is refused with.
const NOTHING_FETCHABLE: &str = "that manifest lists no picture or sound dm can fetch";

/// The picture to take at `quality`: the tallest at or below a height (the
/// shortest above it when nothing is that small), the tallest for best.
/// `container` ("webm") prefers that container among equals; otherwise MP4.
fn pick_video(
    mut videos: Vec<Candidate>,
    quality: Quality,
    container: Option<&str>,
) -> Option<Candidate> {
    let want_webm = container == Some("webm");
    if let Quality::UpTo(limit) = quality {
        if videos.iter().any(|c| c.track.height <= limit) {
            videos.retain(|c| c.track.height <= limit);
        } else {
            let lowest = videos.iter().map(|c| c.track.height).min()?;
            videos.retain(|c| c.track.height == lowest);
        }
    }
    videos
        .into_iter()
        .max_by_key(|c| (c.track.height, c.track.webm == want_webm, c.track.bandwidth))
}

/// The sound to take: one speaking the language `want`ed (Options, gaps row Y10) where any
/// does, a plain one before one describing the picture; then a main track's, in the picture's
/// container where there is one, the best bandwidth.
fn pick_audio(audios: Vec<Candidate>, with_webm: bool, want: Option<&str>) -> Option<Candidate> {
    let speaks = |c: &Candidate| {
        want.zip(c.lang.as_deref())
            .is_some_and(|(want, lang)| crate::sound_language::speaks(lang, want))
    };
    audios.into_iter().max_by_key(|c| {
        (
            speaks(c),
            speaks(c) && !c.described,
            c.main,
            c.track.webm == with_webm,
            c.track.bandwidth,
        )
    })
}

/// The picture and the sound to take of one period's representations.
fn choose(
    all: Vec<Candidate>,
    quality: Quality,
    container: Option<&str>,
) -> (Option<Candidate>, Option<Candidate>) {
    let (videos, audios): (Vec<Candidate>, Vec<Candidate>) = all
        .into_iter()
        .partition(|c| c.track.medium == Medium::Video);
    let video = if quality == Quality::Audio {
        None
    } else {
        pick_video(videos, quality, container)
    };
    let want = crate::sound_language::wanted();
    let audio = pick_audio(
        audios,
        video.as_ref().is_some_and(|v| v.track.webm),
        want.as_deref(),
    );
    (video, audio)
}

/// What to fetch from a manifest's text, served at `url`. A manifest of
/// several periods has the longest run of alike ones joined (`periods`).
pub fn plan(
    text: &str,
    url: &str,
    quality: Quality,
    container: Option<&str>,
) -> Result<Plan, Error> {
    let root = parse(text)?;
    if root.attr("type") == Some("dynamic") {
        // A live manifest is read afresh as it grows (`live`), not planned whole.
        return Err(Error::Internal(
            "this DASH stream is live, so it is recorded as it goes rather than planned whole"
                .into(),
        ));
    }
    let listed = periods::periods_of(&root, None);
    let (video, audio, left_out) = periods::joined(&root, &listed, url, quality, container)?;
    let protected_above = quality == Quality::Best
        && video
            .as_ref()
            .is_some_and(|v| v.height < protected_tallest(&root));
    Ok(Plan {
        video,
        audio,
        protected_above,
        left_out,
        texts: texts_of(&root, url),
    })
}

/// The tallest picture the manifest lists copy-protected (`ContentProtection` on its set or
/// itself); 0 for none.
fn protected_tallest(root: &Node) -> u32 {
    let Some(period) = root.children("Period").next() else {
        return 0;
    };
    period
        .children("AdaptationSet")
        .flat_map(|set| {
            let set_protected = set.child("ContentProtection").is_some();
            set.children("Representation")
                .filter(move |rep| set_protected || rep.child("ContentProtection").is_some())
                .map(move |rep| {
                    let h = rep.num("height").or_else(|| set.num("height")).unwrap_or(0);
                    let w = rep.num("width").or_else(|| set.num("width")).unwrap_or(0);
                    (if w > 0 && w < h { w } else { h }) as u32
                })
        })
        .max()
        .unwrap_or(0)
}

mod periods;
mod pieces;
mod texts;
pub(crate) use periods::heaviest;
pub use periods::{is_live, keep_periods, live, Live, LivePeriod, LiveTrack};
use texts::texts_of;

#[cfg(test)]
mod tests;
