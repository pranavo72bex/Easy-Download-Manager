// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The media a page's player loaded, as the "Download video" panel lists it.
//!
//! The extension hears the browser's responses and keeps the streams among them
//! ([`Stream`]); which of them the panel offers, joined, ordered and named, is
//! [`panel_list`]. It is written here so that the app, which a person updates
//! more often than a store lets the extension be, makes the list
//! (`media.streams`, gaps row A22 d) with its own, newer copy, and an extension
//! talking to an older app makes it with the copy it was built with.

use serde::{Deserialize, Serialize};

/// What kind of media a stream is, which decides how the app fetches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamKind {
    /// An HLS playlist: the app fetches its segments and joins them.
    Hls,
    /// A DASH manifest: the app reads it, fetches picture and sound and
    /// joins them itself.
    Dash,
    /// A whole audio or video file.
    File,
    /// The picture of a video a player fetches apart from its sound: not
    /// offered alone, but with a sound as one line (`Stream::sound`).
    Picture,
    /// The sound that goes with a [`StreamKind::Picture`].
    Sound,
}

/// Media a page loaded, offered in the panel beside what the app reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stream {
    /// The stream's address.
    pub url: String,
    /// What kind of media the stream is.
    pub kind: StreamKind,
    /// Its size, where the response said.
    pub size: Option<u64>,
    /// The label the panel lists it under.
    pub label: String,
    /// For a picture and a sound a player fetched apart, joined into one
    /// line of the list: the address of the sound, which dm fetches with the
    /// picture at `url` and joins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    /// For a playlist or manifest read: the addresses of the renditions and
    /// sound tracks it lists, which a player fetches one by one and which are
    /// not lines of their own beside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// How long it plays, in seconds, where its playlist or manifest said: said on its line
    /// (gaps row 155), so two videos' streams of one page are told apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// A whole MP4 file a player fetched in byte ranges asked for in its `Range` header, not
    /// in its address (Bilibili's picture and sound, gaps row 155): picture or sound by its
    /// type until its first bytes are read (the extension's `mp4_track`), as such a server may call both
    /// `video/mp4`; and not a line beside a playlist or manifest, whose own files it may be.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_ranges: bool,
}

/// The label of a playlist that carries only sound.
pub const HLS_SOUND: &str = "HLS sound";

/// The label of a manifest that carries only sound.
pub const DASH_SOUND: &str = "DASH sound";

/// The labels of a playlist or manifest nothing is known of yet; the ones
/// the extension's `Streams::wants_description` asks to have read.
pub const HLS_PLAIN: &str = "HLS stream";
/// The label of a manifest nothing is known of yet.
pub const DASH_PLAIN: &str = "DASH stream";

/// A file smaller than this, beside something larger, is a sign-in page's
/// clip, an advert or a preview, not the video the person came for.
pub const CLIP_FLOOR: u64 = 512 * 1024;

/// What a picture and a sound found by the extension's `ranged_of` are called. A file with one of these names
/// and no partner is a whole file all the same, and is offered as one.
pub const RANGED_PICTURE: &str = "video file";
/// What a sound found by its byte ranges is called.
pub const RANGED_SOUND: &str = "audio file";

/// The picture height a line's label names (`720p HLS stream`, `1080p60 …`),
/// for sorting the best first.
pub fn label_height(label: &str) -> u32 {
    // A manifest named by nothing but its kind is not known to be less than
    // the renditions named beside it: it is usually their master.
    if label == HLS_PLAIN || label == DASH_PLAIN {
        return u32::MAX;
    }
    let digits: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
    if label
        .get(digits.len()..)
        .is_some_and(|rest| rest.starts_with('p'))
    {
        digits.parse().unwrap_or(0)
    } else {
        0
    }
}

/// Whether `url` is on a social site's CDN (`fbcdn.net`, `cdninstagram.com`: Facebook, Instagram,
/// Threads), whose players fetch each file in pieces named by `bytestart` and `byteend`.
pub fn social_cdn(url: &str) -> bool {
    let Some(authority) = url
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
    else {
        return false;
    };
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    ["fbcdn.net", "cdninstagram.com"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// The panel's list of what a tab has loaded: manifests first, which
/// are whole videos, then files, largest first. Two of a kind are told
/// apart by their file name.
pub fn panel_list(list: Vec<Stream>) -> Vec<Stream> {
    // One playlist or manifest asked for twice, each time under a fresh signature, is one line.
    let mut list = one_line_a_stream(list);
    // Beside a playlist or manifest, the whole files a player fetched by ranges are that
    // stream's own (an HLS or DASH stream's single-file renditions), not lines of their own.
    if list
        .iter()
        .any(|s| matches!(s.kind, StreamKind::Hls | StreamKind::Dash))
    {
        list.retain(|s| !s.by_ranges);
    }
    // A picture and a sound a player fetched apart are one line, with
    // the largest of each: the best the page has loaded.
    let largest = |list: &[Stream], kind: StreamKind| {
        list.iter()
            .filter(|s| s.kind == kind)
            .max_by_key(|s| s.size.unwrap_or(0))
            .cloned()
    };
    let (picture, sound) = (
        largest(&list, StreamKind::Picture),
        largest(&list, StreamKind::Sound),
    );
    list.retain(|s| !matches!(s.kind, StreamKind::Picture | StreamKind::Sound));
    // A file found by its byte ranges that has no partner is a whole file on its own; so is a
    // social site's picture with no sound, as its player fetches a post's sound whenever it has
    // one, muted or not: a silent reel ("Video has no audio", gaps row 220's site watch).
    let alone = match (&picture, &sound) {
        (Some(p), None) if p.label.ends_with(RANGED_PICTURE) || social_cdn(&p.url) => {
            Some(p.clone())
        }
        (None, Some(s)) if s.label == RANGED_SOUND => Some(s.clone()),
        _ => None,
    };
    if let Some(mut file) = alone {
        file.kind = StreamKind::File;
        list.push(file);
    }
    // A playlist of sound alone is not offered beside a stream with a
    // picture: it would save the sound and nothing else.
    if list
        .iter()
        .any(|s| s.kind == StreamKind::Hls && s.label != HLS_SOUND)
    {
        list.retain(|s| !(s.kind == StreamKind::Hls && s.label == HLS_SOUND));
    }
    if list
        .iter()
        .any(|s| s.kind == StreamKind::Dash && s.label != DASH_SOUND)
    {
        list.retain(|s| !(s.kind == StreamKind::Dash && s.label == DASH_SOUND));
    }
    if let (Some(picture), Some(sound)) = (picture, sound) {
        list.push(Stream {
            url: picture.url,
            kind: StreamKind::File,
            size: picture.size.zip(sound.size).map(|(a, b)| a + b),
            label: format!("{} + sound", picture.label),
            sound: Some(sound.url),
            members: Vec::new(),
            length: None,
            by_ranges: false,
        });
    }
    // A small clip beside something larger (a sign-in page's, an advert,
    // a preview) is not what the person came for. Alone, it is the post.
    let snapshot = list.clone();
    list.retain(|s| {
        let small = s.kind == StreamKind::File && s.size.is_some_and(|z| z < CLIP_FLOOR);
        !small
            || !snapshot.iter().any(|other| {
                other.url != s.url
                    && (other.kind != StreamKind::File
                        || other.size.is_none_or(|z| z >= CLIP_FLOOR))
            })
    });
    // Manifests first (whole videos), the tallest picture first among
    // them, then files, the largest first.
    list.sort_by_key(|s| {
        (
            s.kind == StreamKind::File,
            std::cmp::Reverse(s.size.unwrap_or(0)),
            std::cmp::Reverse(label_height(&s.label)),
        )
    });
    let labels: Vec<String> = list.iter().map(|s| s.label.clone()).collect();
    for stream in &mut list {
        if labels.iter().filter(|l| **l == stream.label).count() > 1 {
            let name = stream
                .url
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>();
            if !name.is_empty() {
                stream.label = format!("{} · {name}", stream.label);
            }
        }
    }
    list
}

/// Whether two addresses are one stream signed twice: the same address before its query, the
/// same names in their queries, and the same values for the names that say which stream it is
/// (`itag`, `id`, `mime`), as the extension's `RequestHeaders::renewed` takes the newer of two
/// for the older when it hands a stream over.
pub fn signed_again(a: &str, b: &str) -> bool {
    fn split(url: &str) -> (&str, Vec<(&str, &str)>) {
        let url = url.split('#').next().unwrap_or(url);
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let pairs = query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| p.split_once('=').unwrap_or((p, "")))
            .collect();
        (path, pairs)
    }
    let ((path_a, query_a), (path_b, query_b)) = (split(a), split(b));
    if path_a != path_b || !path_a.starts_with("http") {
        return false;
    }
    fn names<'a>(query: &[(&'a str, &'a str)]) -> Vec<&'a str> {
        let mut names: Vec<&str> = query.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names
    }
    let value = |query: &[(&str, &str)], name: &str| {
        query
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.to_string())
    };
    names(&query_a) == names(&query_b)
        && ["itag", "id", "mime"]
            .iter()
            .all(|name| value(&query_a, name) == value(&query_b, name))
}

/// A playlist or manifest the player asked for again under a fresh signature (Dailymotion's
/// player reads its video's details twice and is handed a master playlist signed anew each
/// time, `?sec=…&dmTs=…&dmV1st=…`) as one line, where it was two of the same name: where the
/// first was, with the newest address, which is still good, and the name of whichever was read
/// (the newer where both were).
fn one_line_a_stream(list: Vec<Stream>) -> Vec<Stream> {
    let mut out: Vec<Stream> = Vec::with_capacity(list.len());
    for stream in list {
        let manifest = matches!(stream.kind, StreamKind::Hls | StreamKind::Dash);
        let known = out.iter_mut().find(|known| {
            manifest && known.kind == stream.kind && signed_again(&known.url, &stream.url)
        });
        match known {
            Some(known) if stream.label == HLS_PLAIN || stream.label == DASH_PLAIN => {
                known.url = stream.url;
            }
            Some(known) => *known = stream,
            None => out.push(stream),
        }
    }
    out
}

#[cfg(test)]
#[path = "media/tests.rs"]
mod tests;
