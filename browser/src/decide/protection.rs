// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

/// Whether one playlist or manifest says its media is encrypted by a DRM system.
///
/// Rules:
/// - An MPD (`<MPD`) is DRM if it contains a `ContentProtection` element whose
///   `schemeIdUri` attribute (case-insensitively) is one of the known DRM system
///   identifiers (Widevine, PlayReady, FairPlay). A `ContentProtection` with
///   only the MPEG-DASH `mp4protection:2011` marker is NOT DRM on its own.
/// - An HLS text (`#EXTM3U`) is DRM when it has an `#EXT-X-KEY` line whose
///   `METHOD` is `SAMPLE-AES` or `SAMPLE-AES-CTR`, or whose `KEYFORMAT` is
///   anything other than `identity` (or absent). `METHOD=AES-128` with no
///   `KEYFORMAT` or `KEYFORMAT="identity"` is standard encryption, not DRM.
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

/// The known DRM `schemeIdUri` values (lower-cased).
const DRM_SCHEME_IDS: &[&str] = &[
    // Widevine
    "urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed",
    // PlayReady (two common URNs)
    "urn:uuid:9a04f079-9840-4286-ab92-e65be0885f95",
    "urn:uuid:79f0049a-4098-8642-ab92-e65be0885f95",
    // FairPlay
    "urn:uuid:94ce86fb-07ff-4f43-adb8-93d2fa968ca2",
];

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
            if DRM_SCHEME_IDS.iter().any(|&id| lower == id) {
                return true;
            }
        }
    }
    false
}

fn is_hls_drm(text: &str) -> bool {
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("#EXT-X-KEY:") {
            continue;
        }
        let attrs = &line["#EXT-X-KEY:".len()..];
        let method = attr_value(attrs, "METHOD").unwrap_or_default();
        let keyformat = attr_value(attrs, "KEYFORMAT");
        match method.to_ascii_uppercase().as_str() {
            "SAMPLE-AES" | "SAMPLE-AES-CTR" => return true,
            "AES-128" => {
                // AES-128 with a non-identity KEYFORMAT is effectively DRM.
                if let Some(kf) = keyformat {
                    if !kf.eq_ignore_ascii_case("identity") {
                        return true;
                    }
                }
            }
            _ => {
                // Unknown method with a keyformat — treat as DRM.
                if keyformat.is_some() {
                    return true;
                }
            }
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
