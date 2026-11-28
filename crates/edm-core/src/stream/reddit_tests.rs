// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Reddit's master playlists (`v.redd.it/<id>/HLSPlaylist.m3u8`), as gaps row 205 describes
//! them: each rendition the picture alone (its `CODECS` a picture's), the sound a rendition of
//! its own listed after them, its address first, marked AUTOSELECT and not DEFAULT.

use super::*;

const BASE: &str = "https://v.redd.it/abc123/HLSPlaylist.m3u8?a=1&v=1&f=sd";
const SOUND: &str = "https://v.redd.it/abc123/HLS_AUDIO_128.m3u8";

/// Each rendition names its audio group.
const GROUPED: &str = r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-STREAM-INF:BANDWIDTH=612000,AVERAGE-BANDWIDTH=540000,CODECS="avc1.4d401e",RESOLUTION=270x480,FRAME-RATE=30.000,AUDIO="audio-0",CLOSED-CAPTIONS=NONE
HLS_270.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=1410000,AVERAGE-BANDWIDTH=1250000,CODECS="avc1.4d401f",RESOLUTION=360x640,FRAME-RATE=30.000,AUDIO="audio-0",CLOSED-CAPTIONS=NONE
HLS_360.m3u8
#EXT-X-MEDIA:URI="HLS_AUDIO_128.m3u8",TYPE=AUDIO,GROUP-ID="audio-0",LANGUAGE="und",NAME="Audio",AUTOSELECT=YES,CHANNELS="2"
"#;

/// The same, its renditions naming no group: a browser's player plays them with the sound.
const UNGROUPED: &str = r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-STREAM-INF:BANDWIDTH=612000,CODECS="avc1.4d401e",RESOLUTION=270x480
HLS_270.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=1410000,CODECS="avc1.4d401f",RESOLUTION=360x640
HLS_360.m3u8
#EXT-X-MEDIA:URI="HLS_AUDIO_128.m3u8",TYPE=AUDIO,GROUP-ID="audio-0",NAME="Audio",DEFAULT=NO
"#;

/// A post with no sound: its playlist lists none.
const SILENT: &str = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1410000,CODECS="avc1.4d401f",RESOLUTION=360x640
HLS_360.m3u8
"#;

#[test]
fn a_reddit_rendition_is_picked_with_its_sound_rendition() {
    let picked = pick(variants(GROUPED, BASE), Quality::Best).unwrap();
    assert_eq!(picked.url, "https://v.redd.it/abc123/HLS_360.m3u8");
    assert_eq!(picked.height, 360, "a portrait video's shorter side");
    assert_eq!(picked.audio.as_deref(), Some("audio-0"));
    assert_eq!(picked.codecs.as_deref(), Some("avc1.4d401f"));
    // Neither DEFAULT nor first in the master: the group's only track is taken.
    assert_eq!(
        audio_playlist(GROUPED, "audio-0", BASE).as_deref(),
        Some(SOUND)
    );
    // A cap the person set, "640" as the shorter side.
    let capped = pick(variants(GROUPED, BASE), Quality::named("640")).unwrap();
    assert_eq!(capped.url, picked.url);
    // Never its picture alone: where the pair were not EDM's to join, it is refused.
    let media = "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:4,\nv.mp4\n#EXT-X-ENDLIST\n";
    assert_eq!(
        beyond_dm(Some(GROUPED), Some("audio-0"), media, BASE),
        Some("this stream carries its sound separately from its picture")
    );
    // Its group named without quotes, or marked DEFAULT=NO, is found all the same.
    let unquoted = GROUPED
        .replace("AUDIO=\"audio-0\"", "AUDIO=audio-0")
        .replace("AUTOSELECT=YES", "DEFAULT=NO");
    let picked = pick(variants(&unquoted, BASE), Quality::Best).unwrap();
    let group = picked.audio.unwrap();
    assert_eq!(
        audio_playlist(&unquoted, &group, BASE).as_deref(),
        Some(SOUND)
    );
}

#[test]
fn a_silent_rendition_naming_no_group_plays_with_the_masters_sound() {
    let picked = pick(variants(UNGROUPED, BASE), Quality::Best).unwrap();
    assert_eq!(picked.audio, None);
    assert_eq!(
        ungrouped_audio_playlist(UNGROUPED, &picked, BASE).as_deref(),
        Some(SOUND)
    );
    // Its own sound named in its codecs, or no codecs said: its segments carry it.
    let with = |codecs: Option<&str>| Variant {
        codecs: codecs.map(str::to_string),
        ..picked.clone()
    };
    let muxed = with(Some("avc1.4d401f,mp4a.40.2"));
    assert_eq!(ungrouped_audio_playlist(UNGROUPED, &muxed, BASE), None);
    let bare = with(None);
    assert_eq!(ungrouped_audio_playlist(UNGROUPED, &bare, BASE), None);
    // A rendition that names its group is that group's (`audio_playlist`).
    let grouped = pick(variants(GROUPED, BASE), Quality::Best).unwrap();
    assert_eq!(ungrouped_audio_playlist(GROUPED, &grouped, BASE), None);
    // A post with no sound has none to take.
    let silent = pick(variants(SILENT, BASE), Quality::Best).unwrap();
    assert_eq!(ungrouped_audio_playlist(SILENT, &silent, BASE), None);
    assert_eq!(beyond_dm(Some(SILENT), None, "#EXTM3U\n", BASE), None);
    // Among several, the language asked for, else the default.
    let two = format!(
        "{UNGROUPED}#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"b\",LANGUAGE=\"es\",URI=\"es.m3u8\"\n\
         #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"b\",LANGUAGE=\"en\",DEFAULT=YES,URI=\"en.m3u8\"\n\
         #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"c\",LANGUAGE=\"fr\"\n"
    );
    let uri = |want| master::ungrouped_audio_uri(&two, want);
    assert_eq!(uri(None).as_deref(), Some("en.m3u8"));
    assert_eq!(uri(Some("es")).as_deref(), Some("es.m3u8"));
    // French is listed with no address.
    assert_eq!(uri(Some("fr")).as_deref(), Some("en.m3u8"));
}

#[test]
fn codecs_of_a_picture_alone_are_told_from_ones_with_a_sound() {
    for codecs in [
        "avc1.4d401f",
        "hvc1.1.6.L93.B0",
        "AVC1.64001F, wvtt",
        "vp09.00.10.08,av01",
    ] {
        assert!(master::picture_only(codecs), "{codecs}");
    }
    for codecs in [
        "avc1.4d401f,mp4a.40.2",
        "hvc1.1.6.L93.B0,ec-3",
        "avc1.4d401f,opus",
        "mp4a.40.2",
        "wvtt",
        "avc1.4d401f,something.1",
        "",
    ] {
        assert!(!master::picture_only(codecs), "{codecs}");
    }
}
