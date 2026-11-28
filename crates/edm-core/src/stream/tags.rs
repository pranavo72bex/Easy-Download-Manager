// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Three of a media playlist's tags read (`media_playlist`): a key, an initialisation
//! segment's bytes, and the container the segments make.

use super::*;

/// The key an `#EXT-X-KEY` row puts in force for the segments after it: none for
/// `METHOD=NONE`. A scrambling EDM cannot undo, or a key a rights system keeps, is refused.
pub(super) fn key_of(row: &str, url: &str) -> Result<Option<Key>, Error> {
    let method = attribute(row, "METHOD")
        .unwrap_or_default()
        .to_ascii_uppercase();
    if method == "NONE" {
        return Ok(None);
    }
    // SAMPLE-AES with a clear key: the samples are put in the clear once the segments
    // are joined (gaps row Y16).
    if method != "AES-128" && method != "SAMPLE-AES" {
        return Err(Error::Internal(format!(
            "this stream uses {method} encryption, which EDM cannot decrypt"
        )));
    }
    let uri = attribute(row, "URI")
        .ok_or_else(|| Error::Internal("the stream is encrypted but names no key".into()))?;
    /* Plain AES-128 is HLS's own scrambling, its key handed to any
     * player that asks, as every player reads it. A key
     * kept by a rights system (FairPlay's skd://, Widevine's or
     * PlayReady's key formats) is copy protection, and EDM does not
     * get around copy protection: the stream is refused. */
    let format = attribute(row, "KEYFORMAT").unwrap_or_default();
    if protected_key(&format, &uri) {
        return Err(Error::Internal(PROTECTED.into()));
    }
    Ok(Some(Key {
        uri: join(url, &uri)?,
        iv: attribute(row, "IV"),
        sample: method == "SAMPLE-AES",
    }))
}

/// The bytes an `#EXT-X-MAP` row's `BYTERANGE` names, first and last, where it names
/// any: an initialisation segment's must say where it starts.
pub(super) fn map_range(row: &str) -> Result<Option<(u64, u64)>, Error> {
    let range = match attribute(row, "BYTERANGE") {
        Some(text) => match byte_range(&text)? {
            (len, Some(first)) => Some((first, first.saturating_add(len) - 1)),
            (_, None) => {
                return Err(Error::Internal(
                    "the initialisation segment's byte range has no start".into(),
                ))
            }
        },
        None => None,
    };
    Ok(range)
}

/// The container the segments make, from what the tags said (`container`), the segments'
/// names and the quality asked for.
pub(super) fn container_of(
    parts: &[Part],
    container: &'static str,
    quality: Quality,
) -> &'static str {
    let mut container = container;
    // fMP4 segments concatenate into a valid MP4; MPEG-TS into a valid TS.
    if parts.iter().any(|p| {
        p.url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .ends_with(".m4s")
    }) {
        container = "mp4";
    }
    /* Sound alone is named for what its segments are, not for the quality
     * asked: fMP4 sound is an `.m4a`, and sound in a transport stream stays
     * a `.ts`, which the remux puts into an MP4 where it can. Called `.aac`
     * whatever it was, neither opened as what its name said. Packed audio
     * (`.aac` segments) joins into an `.aac` and is one. */
    let packed_audio = parts.iter().all(|p| {
        p.url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .ends_with(".aac")
    });
    if packed_audio {
        container = "aac";
    } else if quality == Quality::Audio && container == "mp4" {
        container = "m4a";
    }
    container
}
