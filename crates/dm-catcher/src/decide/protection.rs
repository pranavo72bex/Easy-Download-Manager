// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

/// Whether one playlist or manifest says its media is encrypted by a DRM system.
///
/// Rules:
/// The same judgement as the app's engine makes when it fetches the stream (`dm-core`'s
/// `stream::protected_key`, `dash.rs`), so the panel never says "protected" of what the engine
/// would take, nor the other way round (gaps row Y19):
/// - An MPD (`<MPD`) is DRM if it contains a `ContentProtection` element with any
///   `schemeIdUri` but the bare MPEG-DASH `mp4protection:2011` marker: a known system
///   (Widevine, PlayReady, FairPlay), ClearKey, or one this does not know.
/// - An HLS text (`#EXTM3U`) is DRM when an `#EXT-X-KEY` or `#EXT-X-SESSION-KEY` line (a
///   master playlist names its keys in the second) has a `KEYFORMAT` other than `identity`, or
///   a key address that is not a web address (FairPlay's `skd://`, a `data:` licence). A key
///   of HLS's own (`identity`, a web address) is not, whatever its `METHOD`: AES-128 the
///   engine decrypts, and SAMPLE-AES it refuses as encryption it cannot undo, not as DRM.
pub fn is_drm(text: &str) -> bool {
    if text.contains("<MPD") {
        is_mpd_drm(text)
    } else if text.starts_with("#EXTM3U") {
        is_hls_drm(text)
    } else {
        false
    }
}

/// What the texts the page loaded say about DRM.
/// - `Some(true)` if any text is a manifest/playlist and at least one is DRM.
/// - `Some(false)` if at least one manifest/playlist was read and none is DRM.
/// - `None` if no text was a recognisable manifest or playlist.
pub fn stream_protection(texts: &[String]) -> Option<bool> {
    let mut any_manifest = false;
    for text in texts {
        if text.contains("<MPD") || text.starts_with("#EXTM3U") {
            any_manifest = true;
            if is_drm(text) {
                return Some(true);
            }
        }
    }
    if any_manifest {
        Some(false)
    } else {
        None
    }
}

/// The common encryption marker — present in every CENC stream, with or
/// without a real DRM system above it.
const MP4_PROTECTION_MARKER: &str = "urn:mpeg:dash:mp4protection:2011";

fn is_mpd_drm(text: &str) -> bool {
    // Find each ContentProtection element and check its schemeIdUri attribute.
    let mut rest = text;
    while let Some(pos) = rest.find("ContentProtection") {
        rest = &rest[pos + "ContentProtection".len()..];
        // Extract the tag up to the next `>`.
        let tag_end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..tag_end];
        if let Some(scheme) = attr_value(tag, "schemeIdUri") {
            let lower = scheme.to_ascii_lowercase();
            if lower == MP4_PROTECTION_MARKER {
                continue;
            }
            // A known system (Widevine, PlayReady, FairPlay), ClearKey, or one not known: the
            // engine takes none of them.
            return true;
        }
    }
    false
}

fn is_hls_drm(text: &str) -> bool {
    for line in text.lines() {
        let line = line.trim();
        let Some(attrs) = line
            .strip_prefix("#EXT-X-KEY:")
            .or_else(|| line.strip_prefix("#EXT-X-SESSION-KEY:"))
        else {
            continue;
        };
        let method = attr_value(attrs, "METHOD").unwrap_or_default();
        if method.eq_ignore_ascii_case("NONE") {
            continue;
        }
        let keyformat = attr_value(attrs, "KEYFORMAT").unwrap_or_default().trim();
        let uri = attr_value(attrs, "URI")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let own_format = keyformat.is_empty() || keyformat.eq_ignore_ascii_case("identity");
        let web_key =
            uri.starts_with("http://") || uri.starts_with("https://") || !uri.contains(':');
        if !own_format || !web_key {
            return true;
        }
    }
    false
}

/// Extracts the value of `key` from a tag's attribute string.
///
/// Handles both `key=value` and `key="value"` (quoted).
fn attr_value<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let key_eq = &format!("{key}=");
    let pos = attrs.find(key_eq.as_str())?;
    let after = &attrs[pos + key_eq.len()..];
    if let Some(quoted) = after.strip_prefix('"') {
        let end = quoted.find('"')?;
        quoted.get(..end)
    } else {
        let end = after
            .find([' ', ',', '>', '\n', '\r'])
            .unwrap_or(after.len());
        Some(&after[..end])
    }
}

#[cfg(test)]
mod tests;
