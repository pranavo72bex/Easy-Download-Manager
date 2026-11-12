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
mod tests {
    use super::*;

    const WIDEVINE_MPD: &str = r#"<?xml version="1.0"?>
<MPD>
  <Period>
    <AdaptationSet>
      <ContentProtection schemeIdUri="urn:uuid:EDEF8BA9-79D6-4ACE-A3C8-27DCD51D21ED" value="Widevine"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    const PLAYREADY_MPD: &str = r#"<?xml version="1.0"?>
<MPD>
  <Period>
    <AdaptationSet>
      <ContentProtection schemeIdUri="urn:uuid:9a04f079-9840-4286-ab92-e65be0885f95"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    const MP4PROTECTION_ONLY_MPD: &str = r#"<?xml version="1.0"?>
<MPD>
  <Period>
    <AdaptationSet>
      <ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" value="cenc"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    const CLEAR_MPD: &str = r#"<?xml version="1.0"?>
<MPD>
  <Period>
    <AdaptationSet mimeType="video/mp4">
      <Representation bandwidth="1500000"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    const SAMPLE_AES_HLS: &str = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:10.0,\nseg0.ts";
    const AES128_HLS: &str =
        "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://k/1\"\n#EXTINF:10.0,\nseg0.ts";
    const CLEAR_HLS: &str = "#EXTM3U\n#EXTINF:10.0,\nseg0.ts";

    /// Each scheme judged as the engine judges it (gaps row Y19).
    #[test]
    fn each_scheme_is_judged_as_the_engine_judges_it() {
        let mpd = |scheme: &str| {
            format!(
                r#"<MPD><Period><AdaptationSet><ContentProtection schemeIdUri="{scheme}"/></AdaptationSet></Period></MPD>"#
            )
        };
        // ClearKey (both its names) and a system not known are protected.
        assert!(is_drm(&mpd(
            "urn:uuid:e2719d58-a985-b3c9-781a-b030af78d30e"
        )));
        assert!(is_drm(&mpd(
            "urn:uuid:1077efec-c0b2-4d02-ace3-3c1e52e2fb4b"
        )));
        assert!(is_drm(&mpd(
            "urn:uuid:00000000-1111-2222-3333-444444444444"
        )));
        // A master playlist's session key from a rights system is protected.
        assert!(is_drm(
            "#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"skd://k\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8"
        ));
        // SAMPLE-AES with HLS's own key is not DRM (the engine refuses it otherwise).
        assert!(!is_drm(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"https://k/1\",KEYFORMAT=\"identity\"\n#EXTINF:10.0,\nseg0.ts"
        ));
        // A data: licence is protected, whatever its format says; METHOD=NONE is clear.
        assert!(is_drm(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"data:text/plain;base64,AAAA\"\n#EXTINF:10.0,\nseg0.ts"
        ));
        assert!(!is_drm(
            "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:10.0,\nseg0.ts"
        ));
    }

    #[test]
    fn widevine_mpd_is_drm() {
        assert!(is_drm(WIDEVINE_MPD));
    }

    #[test]
    fn playready_mpd_is_drm() {
        assert!(is_drm(PLAYREADY_MPD));
    }

    #[test]
    fn mp4protection_only_is_not_drm() {
        assert!(!is_drm(MP4PROTECTION_ONLY_MPD));
    }

    #[test]
    fn clear_mpd_is_not_drm() {
        assert!(!is_drm(CLEAR_MPD));
    }

    #[test]
    fn sample_aes_hls_is_drm() {
        assert!(is_drm(SAMPLE_AES_HLS));
    }

    #[test]
    fn aes128_hls_is_not_drm() {
        assert!(!is_drm(AES128_HLS));
    }

    #[test]
    fn clear_hls_is_not_drm() {
        assert!(!is_drm(CLEAR_HLS));
    }

    #[test]
    fn empty_and_non_manifest_are_not_drm() {
        assert!(!is_drm(""));
        assert!(!is_drm("{ \"type\": \"video\" }"));
        assert!(!is_drm("<html><body></body></html>"));
    }

    #[test]
    fn stream_protection_no_texts_is_none() {
        assert_eq!(stream_protection(&[]), None);
    }

    #[test]
    fn stream_protection_one_clear_playlist_is_some_false() {
        assert_eq!(stream_protection(&[CLEAR_HLS.to_string()]), Some(false));
    }

    #[test]
    fn stream_protection_clear_and_drm_is_some_true() {
        assert_eq!(
            stream_protection(&[CLEAR_HLS.to_string(), WIDEVINE_MPD.to_string()]),
            Some(true)
        );
    }

    #[test]
    fn stream_protection_ordinary_json_is_none() {
        assert_eq!(
            stream_protection(&[r#"{"video": "https://example.com/v.mp4"}"#.to_string()]),
            None
        );
    }
}
