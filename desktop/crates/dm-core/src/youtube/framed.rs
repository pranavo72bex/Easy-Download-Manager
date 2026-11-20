// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! YouTube's framed stream (gaps row Y2): the formats a player's answer lists with neither an
//! address nor a cipher, served only by posting to the one address the answer names
//! (`streamingData.serverAbrStreamingUrl`) a Protocol Buffers message of the formats wanted and
//! what is held of each, and reading a framed answer of their pieces (UMP) back. Asked again
//! until every format has its last piece.
//!
//! dm's own reader and writer of the wire ([`wire`]), of the framing ([`ump`]) and of the few
//! messages it needs ([`messages`]): no crate. Each format's pieces are written to the file a
//! format with an address is written to (`name.f137.mp4`), so the join is the same.
//!
//! A stream that wants a proof-of-origin token is refused in words: dm mints none (minting one
//! is a decision of its own, `docs/decisions.md`).

use serde_json::{json, Value};

use super::{Format, PlayerClient};

pub mod messages;
mod record;
mod session;
pub mod ump;
pub mod wire;

pub use session::fetch;

/// The protocol a reading names a framed format by (`videopage` fetches it here).
pub const PROTOCOL: &str = "dm_framed";

/// Said where the stream wants a proof-of-origin token.
pub const WANTS_TOKEN: &str =
    "YouTube asks this video's stream for a proof-of-origin token, which dm does not make";

/// The framed stream of one video, as the player's answer names it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Framed {
    /// Where every request is posted, its `n` value answered.
    pub url: String,
    /// The player's `videoPlaybackUstreamerConfig`, as it gave it (base64).
    pub config: String,
    /// The number of the client the player was asked as (`X-Youtube-Client-Name`).
    pub client_name: u32,
    /// Its version.
    pub client_version: String,
    /// The User-Agent it was asked with.
    pub agent: String,
}

impl Framed {
    /// The config's bytes, where it is base64.
    pub fn config_bytes(&self) -> Option<Vec<u8>> {
        decoded(&self.config)
    }
}

/// One format of a framed stream, as the requests name it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pick {
    /// Its format number.
    pub itag: u32,
    /// Its last-modified time (`lastModified`), where the answer gave one.
    #[serde(default)]
    pub lmt: Option<u64>,
    /// Its `xtags`, as the answer wrote them.
    #[serde(default)]
    pub xtags: Option<String>,
    /// A picture, not a sound.
    #[serde(default)]
    pub video: bool,
    /// A picture's height.
    #[serde(default)]
    pub height: u32,
}

/// A format to fetch, and the file its pieces go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// The format.
    pub pick: Pick,
    /// Where it is put whole.
    pub target: std::path::PathBuf,
    /// Its size, where the answer stated it.
    pub size: Option<u64>,
}

/// `text` as base64, URL-safe or not, padded or not.
fn decoded(text: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let trimmed = text.trim().trim_end_matches('=');
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
        .ok()
}

/// The framed stream `player`'s answer names, asked as `client`. `None` where it names none,
/// no config to ask with, or an address that is not on the web: plain `http` only where
/// YouTube is a test's stand-in at such an address.
pub(super) fn session_of(player: &Value, client: &PlayerClient) -> Option<Framed> {
    let url = player
        .pointer("/streamingData/serverAbrStreamingUrl")?
        .as_str()?;
    let plain_allowed = super::origin().starts_with("http://");
    if !(url.starts_with("https://") || (plain_allowed && url.starts_with("http://"))) {
        return None;
    }
    let config = player
        .pointer("/playerConfig/mediaCommonConfig/mediaUstreamerRequestConfig")?
        .get("videoPlaybackUstreamerConfig")?
        .as_str()?;
    if decoded(config).is_none_or(|bytes| bytes.is_empty()) {
        return None;
    }
    Some(Framed {
        url: url.to_string(),
        config: config.to_string(),
        client_name: client.id.trim().parse().unwrap_or(0),
        client_version: client.version.clone(),
        agent: client.agent.clone(),
    })
}

/// Whether `player`'s answer names a framed stream dm can ask.
pub(super) fn named(player: &Value) -> bool {
    session_of(player, &PlayerClient::default()).is_some()
}

/// Whether `a` (with an address) offers the quality `b` (framed) does: the same kind and
/// container, a picture the same size and on the same side of 30 frames a second, a sound in
/// the same language at nine tenths of the rate or more.
fn same_quality(a: &Format, b: &Format) -> bool {
    if a.video != b.video || a.container != b.container {
        return false;
    }
    if a.video {
        a.height == b.height && a.width == b.width && (a.fps > 30) == (b.fps > 30)
    } else {
        a.lang == b.lang && a.bitrate.saturating_mul(10) >= b.bitrate.saturating_mul(9)
    }
}

/// The framed formats of `formats` kept only where the answer names a stream to fetch them
/// from (`streamed`), and only where no format with an address offers the same quality.
pub(super) fn keep_framed(formats: &mut Vec<Format>, streamed: bool) {
    let keep: Vec<bool> = formats
        .iter()
        .map(|f| {
            !f.framed
                || (streamed
                    && !formats
                        .iter()
                        .any(|other| !other.framed && same_quality(other, f)))
        })
        .collect();
    let mut keep = keep.into_iter();
    formats.retain(|_| keep.next().unwrap_or(true));
}

/// The format as the stream's requests name it.
pub(super) fn pick_of(f: &Format) -> Pick {
    Pick {
        itag: u32::try_from(f.itag).unwrap_or(0),
        lmt: f.lmt,
        xtags: f.tags_written.clone(),
        video: f.video,
        height: u32::try_from(f.height).unwrap_or(0),
    }
}

/// The reading marked for the framed stream where any format `chosen` is framed: the stream
/// beside it (`dm_framed`), and each chosen format named by it (its protocol [`PROTOCOL`],
/// its pick), a format with an address among them too: one stream serves them all.
pub(super) fn mark_reading(reading: &mut Value, framed: Option<&Framed>, chosen: &[Format]) {
    let Some(framed) = framed.filter(|_| chosen.iter().any(|f| f.framed)) else {
        return;
    };
    reading["dm_framed"] = serde_json::to_value(framed).unwrap_or_default();
    let mark = |entry: &mut Value, f: &Format| {
        entry["protocol"] = json!(PROTOCOL);
        entry["url"] = Value::Null;
        entry["dm_framed"] = serde_json::to_value(pick_of(f)).unwrap_or_default();
    };
    match reading
        .get_mut("requested_formats")
        .and_then(Value::as_array_mut)
    {
        Some(pair) => {
            for (entry, f) in pair.iter_mut().zip(chosen) {
                mark(entry, f);
            }
        }
        None => {
            if let Some(f) = chosen.first() {
                mark(reading, f);
            }
        }
    }
}

#[cfg(test)]
mod tests;
