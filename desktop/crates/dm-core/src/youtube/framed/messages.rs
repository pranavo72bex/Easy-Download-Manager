// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The messages of YouTube's framed stream dm reads and writes, by their parts' kinds and their
//! fields' numbers. The numbers are those of the published descriptions of the protocol, as
//! they stood in 2026; none of them was checked against YouTube's own servers here.

use super::context::streamer;
use super::wire::{Message, Writer};
use super::Pick;

/// The kinds of part dm reads; any other is passed over.
pub mod kind {
    /// Says what the media parts under one header id are: format, piece number, time.
    pub const MEDIA_HEADER: u64 = 20;
    /// Bytes of a piece: the header id, then the bytes.
    pub const MEDIA: u64 = 21;
    /// A piece is whole: the header id.
    pub const MEDIA_END: u64 = 22;
    /// When to ask next, and the cookie to ask with.
    pub const NEXT_REQUEST_POLICY: u64 = 35;
    /// A format's first word: how many pieces it has and where it ends.
    pub const FORMAT_INITIALIZATION_METADATA: u64 = 42;
    /// Ask somewhere else from now on.
    pub const REDIRECT: u64 = 43;
    /// The server refuses to go on.
    pub const ERROR: u64 = 44;
    /// The player's answer is to be asked again.
    pub const RELOAD_PLAYER_RESPONSE: u64 = 46;
    /// A context to keep and send back in later requests (`SabrContextUpdate`, [`context`]).
    ///
    /// [`context`]: super::super::context
    pub const SABR_CONTEXT_UPDATE: u64 = 57;
    /// Whether the stream wants a proof-of-origin token.
    pub const STREAM_PROTECTION_STATUS: u64 = 58;
    /// Which kept contexts to send, stop sending or forget (`SabrContextSendingPolicy`).
    pub const SABR_CONTEXT_SENDING_POLICY: u64 = 59;
}

/// A format as the stream names it (`FormatId`: 1 itag, 2 last modified, 3 xtags).
fn format_of(id: Message<'_>) -> (Option<u64>, Option<u64>, Option<String>) {
    (id.uint(1), id.uint(2), id.text(3).map(str::to_string))
}

fn put_format(w: &mut Writer, pick: &Pick) {
    w.uint(1, u64::from(pick.itag));
    if let Some(lmt) = pick.lmt {
        w.uint(2, lmt);
    }
    if let Some(xtags) = pick.xtags.as_deref().filter(|x| !x.is_empty()) {
        w.text(3, xtags);
    }
}

/// What a media header says (`MediaHeader`: 1 header id, 3 itag, 4 last modified, 5 xtags,
/// 7 compression, 8 first boxes, 9 piece number, 11 start, 12 length, 13 the format, 14 size).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaHeader {
    /// The id the media parts that follow name it by.
    pub header_id: u64,
    /// The format's number.
    pub itag: Option<u64>,
    /// The format's last-modified time.
    pub lmt: Option<u64>,
    /// The format's xtags.
    pub xtags: Option<String>,
    /// The format's first boxes, not a piece of its media.
    pub init: bool,
    /// The piece's number, counted from 1.
    pub sequence: Option<u64>,
    /// Where the piece starts and how long it plays, in milliseconds.
    pub start_ms: Option<u64>,
    /// How long the piece plays, in milliseconds.
    pub duration_ms: Option<u64>,
    /// The piece is compressed, which dm does not read.
    pub compressed: bool,
}

impl MediaHeader {
    /// The header in `data`, where it names its id.
    pub fn read(data: &[u8]) -> Option<MediaHeader> {
        let m = Message(data);
        let (itag, lmt, xtags) = m.message(13).map(format_of).unwrap_or_default();
        Some(MediaHeader {
            header_id: m.uint(1)?,
            itag: itag.or_else(|| m.uint(3)),
            lmt: lmt.or_else(|| m.uint(4)),
            xtags: xtags.or_else(|| m.text(5).map(str::to_string)),
            init: m.flag(8).unwrap_or(false),
            sequence: m.uint(9),
            start_ms: m.uint(11),
            duration_ms: m.uint(12),
            compressed: m.uint(7).is_some_and(|c| c != 0),
        })
    }
}

/// The header id a media or media-end part starts with, and the bytes after it.
pub fn media_of(data: &[u8]) -> Option<(u64, &[u8])> {
    let (id, used) = super::ump::varint(data)?;
    Some((id, data.get(used..)?))
}

/// A format's first word (`FormatInitializationMetadata`: 2 the format, 3 where it ends in
/// milliseconds, 4 its last piece's number).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatStart {
    /// The format's number.
    pub itag: Option<u64>,
    /// The format's xtags.
    pub xtags: Option<String>,
    /// Where it ends, in milliseconds.
    pub end_ms: Option<u64>,
    /// Its last piece's number.
    pub end_segment: Option<u64>,
}

impl FormatStart {
    /// The word in `data`.
    pub fn read(data: &[u8]) -> FormatStart {
        let m = Message(data);
        let (itag, _, xtags) = m.message(2).map(format_of).unwrap_or_default();
        FormatStart {
            itag,
            xtags,
            end_ms: m.uint(3),
            end_segment: m.uint(4),
        }
    }
}

/// When to ask next (`NextRequestPolicy`: 4 the wait in milliseconds, 7 the cookie to send
/// back, kept whole).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NextAsk {
    /// How long to wait before the next request, in milliseconds.
    pub backoff_ms: Option<u64>,
    /// The playback cookie, sent back as it came.
    pub cookie: Option<Vec<u8>>,
}

impl NextAsk {
    /// The policy in `data`.
    pub fn read(data: &[u8]) -> NextAsk {
        let m = Message(data);
        NextAsk {
            backoff_ms: m.uint(4),
            cookie: m.bytes(7).map(<[u8]>::to_vec),
        }
    }
}

/// Where to ask from now on (1, the address).
pub fn redirect_of(data: &[u8]) -> Option<String> {
    Message(data).text(1).map(str::to_string)
}

/// The server's refusal (1 its kind, 2 its code), in words.
pub fn error_of(data: &[u8]) -> String {
    let m = Message(data);
    let kind = m.text(1).filter(|k| !k.is_empty()).unwrap_or("unnamed");
    match m.uint(2) {
        Some(code) => format!("{kind} ({code})"),
        None => kind.to_string(),
    }
}

/// The stream's protection status (1): 1 nothing wanted, 2 a token wanted soon, 3 a token
/// wanted now.
pub fn protection_of(data: &[u8]) -> Option<u64> {
    Message(data).uint(1)
}

/// How much of one format dm holds, for the request (`BufferedRange`: 1 the format, 2 its
/// start, 3 its length in milliseconds, 4 the first piece's number, 5 the last's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held<'a> {
    /// The format.
    pub pick: &'a Pick,
    /// How long what is held plays, in milliseconds from the start.
    pub duration_ms: u64,
    /// The first and the last piece held.
    pub first: u64,
    /// The last piece held.
    pub last: u64,
}

/// One request of the framed stream (`VideoPlaybackAbrRequest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask<'a> {
    /// The player's `videoPlaybackUstreamerConfig`, decoded (5).
    pub config: &'a [u8],
    /// The client the player was asked as, by number and version (19.1: 16 and 17).
    pub client_name: u32,
    /// Its version.
    pub client_version: &'a str,
    /// Where the player is, in milliseconds (4, and the state's 28).
    pub player_time_ms: u64,
    /// The picture's height, the quality chosen by hand (the state's 16 and 21).
    pub height: u32,
    /// The sound and the picture asked for (16 and 17).
    pub audio: Vec<&'a Pick>,
    /// The pictures asked for.
    pub video: Vec<&'a Pick>,
    /// The formats whose first word was heard (2).
    pub started: Vec<&'a Pick>,
    /// What is held of each (3).
    pub held: Vec<Held<'a>>,
    /// The cookie the last answer gave (19.3).
    pub cookie: Option<&'a [u8]>,
    /// The contexts the stream gave that are to be sent back, by type (19.5).
    pub contexts: Vec<(u64, &'a [u8])>,
    /// The types of those held but not sent (19.6).
    pub unsent: Vec<u64>,
}

impl Ask<'_> {
    /// The request's bytes.
    pub fn body(&self) -> Vec<u8> {
        let mut w = Writer::new();
        // The player's state (`ClientAbrState`): 16 the quality picked, 21 kept to, 28 where it
        // is, 40 the kinds of track on (1 the sound alone).
        w.message(1, |s| {
            if self.height > 0 {
                s.uint(16, u64::from(self.height));
                s.uint(21, u64::from(self.height));
            }
            s.uint(28, self.player_time_ms);
            s.uint(40, u64::from(self.video.is_empty()));
        });
        for pick in &self.started {
            w.message(2, |f| put_format(f, pick));
        }
        for held in &self.held {
            w.message(3, |r| {
                r.message(1, |f| put_format(f, held.pick));
                r.uint(2, 0);
                r.uint(3, held.duration_ms);
                r.uint(4, held.first);
                r.uint(5, held.last);
            });
        }
        w.uint(4, self.player_time_ms);
        w.bytes(5, self.config);
        for pick in &self.audio {
            w.message(16, |f| put_format(f, pick));
        }
        for pick in &self.video {
            w.message(17, |f| put_format(f, pick));
        }
        // Who asks (`StreamerContext`): 1 the client (16 its number, 17 its version), 3 the
        // cookie, 5 and 6 the contexts ([`streamer`]). No proof-of-origin token (2): dm mints
        // none.
        w.message(19, |c| {
            c.message(1, |client| {
                client.uint(16, u64::from(self.client_name));
                client.text(17, self.client_version);
            });
            if let Some(cookie) = self.cookie {
                c.bytes(3, cookie);
            }
            for (ty, value) in &self.contexts {
                c.message(streamer::SENT, |sent| {
                    sent.uint(streamer::TYPE, *ty);
                    sent.bytes(streamer::VALUE, value);
                });
            }
            for ty in &self.unsent {
                c.uint(streamer::UNSENT, *ty);
            }
        });
        w.finish()
    }
}

/// The pairs a format's `xtags` of the player's answer hold, where it is written as the
/// stream writes it (base64 of a message of repeated 1: {1 the name, 2 the value}).
pub fn xtag_pairs(written: &str) -> Vec<(String, String)> {
    let Some(bytes) = super::decoded(written) else {
        return Vec::new();
    };
    Message(&bytes)
        .all_bytes(1)
        .map(Message)
        .filter_map(|pair| Some((pair.text(1)?.to_string(), pair.text(2)?.to_string())))
        .collect()
}
