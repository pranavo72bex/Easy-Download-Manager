// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
