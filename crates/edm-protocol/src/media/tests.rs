// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The panel's list, as both sides make it: the app for an extension that asks
//! (`media.streams`), and the extension itself for an older app.

use super::*;

fn line(url: &str, kind: StreamKind, size: Option<u64>, label: &str) -> Stream {
    Stream {
        url: url.into(),
        kind,
        size,
        label: label.into(),
        sound: None,
        members: Vec::new(),
        length: None,
        by_ranges: false,
    }
}

fn labels(list: &[Stream]) -> Vec<&str> {
    list.iter().map(|s| s.label.as_str()).collect()
}

const MB: u64 = 1024 * 1024;

#[test]
fn a_label_says_its_height_and_a_manifest_named_by_nothing_is_tallest() {
    assert_eq!(label_height("720p HLS stream"), 720);
    assert_eq!(label_height("1080p60 DASH stream"), 1080);
    assert_eq!(label_height("audio file"), 0);
    assert_eq!(label_height("720 wide"), 0, "a number that is not a height");
    assert_eq!(label_height(""), 0);
    assert_eq!(label_height(HLS_PLAIN), u32::MAX);
    assert_eq!(label_height(DASH_PLAIN), u32::MAX);
}

#[test]
fn nothing_loaded_is_nothing_listed() {
    assert!(panel_list(Vec::new()).is_empty());
}

#[test]
fn a_picture_and_a_sound_fetched_apart_are_one_line_with_the_largest_of_each() {
    let list = panel_list(vec![
        line(
            "https://c.example/v360.mp4",
            StreamKind::Picture,
            Some(2 * MB),
            "360p video",
        ),
        line(
            "https://c.example/v720.mp4",
            StreamKind::Picture,
            Some(9 * MB),
            "720p video",
        ),
        line(
            "https://c.example/a1.mp4",
            StreamKind::Sound,
            Some(MB),
            "sound",
        ),
        line("https://c.example/a2.mp4", StreamKind::Sound, None, "sound"),
    ]);
    assert_eq!(list.len(), 1);
    let joined = &list[0];
    assert_eq!(joined.url, "https://c.example/v720.mp4");
    assert_eq!(joined.sound.as_deref(), Some("https://c.example/a1.mp4"));
    assert_eq!(joined.kind, StreamKind::File);
    assert_eq!(joined.size, Some(10 * MB));
    assert_eq!(joined.label, "720p video + sound");
    // A size unknown on either side is a size unknown for the pair.
    let unknown = panel_list(vec![
        line(
            "https://c.example/v.mp4",
            StreamKind::Picture,
            None,
            "video",
        ),
        line(
            "https://c.example/a.mp4",
            StreamKind::Sound,
            Some(MB),
            "sound",
        ),
    ]);
    assert_eq!(unknown[0].size, None);
}

#[test]
fn a_file_found_by_its_ranges_with_no_partner_is_a_whole_file() {
    let picture = line(
        "https://c.example/v.mp4",
        StreamKind::Picture,
        Some(5 * MB),
        "720p video file",
    );
    let alone = panel_list(vec![picture.clone()]);
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].kind, StreamKind::File);
    let sound = line(
        "https://c.example/a.mp4",
        StreamKind::Sound,
        Some(2 * MB),
        RANGED_SOUND,
    );
    assert_eq!(panel_list(vec![sound])[0].kind, StreamKind::File);
    // A picture or a sound not found by its ranges is never offered alone.
    let player_picture = line(
        "https://c.example/p.mp4",
        StreamKind::Picture,
        Some(5 * MB),
        "720p video",
    );
    assert!(panel_list(vec![player_picture]).is_empty());
    let player_sound = line(
        "https://c.example/s.mp4",
        StreamKind::Sound,
        Some(5 * MB),
        "sound",
    );
    assert!(panel_list(vec![player_sound]).is_empty());
}

#[test]
fn a_social_sites_picture_with_no_sound_is_a_silent_post_offered_whole() {
    let reel = line(
        "https://instagram.fktm18-1.fna.fbcdn.net/o1/v/t2/f2/m86/AQ.mp4?efg=x&oh=sig",
        StreamKind::Picture,
        None,
        "video",
    );
    let alone = panel_list(vec![reel.clone()]);
    assert_eq!(alone.len(), 1, "{alone:?}");
    assert_eq!(
        (alone[0].kind, alone[0].url.as_str()),
        (StreamKind::File, reel.url.as_str())
    );
    // Beside its sound it is still one line of both.
    let sound = line(
        "https://scontent.cdninstagram.com/v/a.mp4?efg=y",
        StreamKind::Sound,
        None,
        "sound",
    );
    let both = panel_list(vec![reel, sound.clone()]);
    assert_eq!(both.len(), 1);
    assert_eq!(both[0].sound.as_deref(), Some(sound.url.as_str()));
    // Only those CDNs: a host that merely ends in their name is not one.
    assert!(social_cdn("https://video-sjc.xx.fbcdn.net/v.mp4"));
    assert!(social_cdn("https://cdninstagram.com/v.mp4"));
    assert!(!social_cdn("https://notfbcdn.net/v.mp4"));
    assert!(!social_cdn("https://fbcdn.net.example.com/v.mp4"));
}

#[test]
fn beside_a_playlist_its_own_ranged_files_and_its_sound_alone_are_not_lines() {
    let mut ranged = line(
        "https://c.example/r.mp4",
        StreamKind::File,
        Some(5 * MB),
        "video file",
    );
    ranged.by_ranges = true;
    let list = panel_list(vec![
        line(
            "https://c.example/m.m3u8",
            StreamKind::Hls,
            None,
            "1080p HLS stream",
        ),
        line("https://c.example/s.m3u8", StreamKind::Hls, None, HLS_SOUND),
        line(
            "https://c.example/m.mpd",
            StreamKind::Dash,
            None,
            "720p DASH stream",
        ),
        line(
            "https://c.example/s.mpd",
            StreamKind::Dash,
            None,
            DASH_SOUND,
        ),
        ranged,
    ]);
    assert_eq!(labels(&list), ["1080p HLS stream", "720p DASH stream"]);
    // Sound alone, with nothing to go beside, is still offered.
    let sound_only = panel_list(vec![
        line("https://c.example/s.m3u8", StreamKind::Hls, None, HLS_SOUND),
        line(
            "https://c.example/s.mpd",
            StreamKind::Dash,
            None,
            DASH_SOUND,
        ),
    ]);
    assert_eq!(sound_only.len(), 2);
}

#[test]
fn a_small_clip_goes_beside_something_larger_and_stays_alone() {
    let clip = line(
        "https://c.example/clip.mp4",
        StreamKind::File,
        Some(100 * 1024),
        "clip",
    );
    assert_eq!(
        panel_list(vec![clip.clone()]).len(),
        1,
        "alone, it is the post"
    );
    let video = line(
        "https://c.example/v.mp4",
        StreamKind::File,
        Some(20 * MB),
        "video",
    );
    assert_eq!(labels(&panel_list(vec![clip.clone(), video])), ["video"]);
    // Beside a stream, or a file of no known size, it goes too.
    let stream = line(
        "https://c.example/m.m3u8",
        StreamKind::Hls,
        None,
        "HLS stream",
    );
    assert_eq!(
        labels(&panel_list(vec![clip.clone(), stream])),
        ["HLS stream"]
    );
    let no_size = line("https://c.example/u.mp4", StreamKind::File, None, "unsized");
    assert_eq!(
        labels(&panel_list(vec![clip.clone(), no_size])),
        ["unsized"]
    );
    // Two small clips: neither is larger, both stay.
    let other = line(
        "https://c.example/clip2.mp4",
        StreamKind::File,
        Some(200 * 1024),
        "clip 2",
    );
    assert_eq!(panel_list(vec![clip, other]).len(), 2);
}

#[test]
fn manifests_come_first_tallest_first_then_files_largest_first() {
    let list = panel_list(vec![
        line(
            "https://c.example/small.mp4",
            StreamKind::File,
            Some(2 * MB),
            "small",
        ),
        line(
            "https://c.example/480.m3u8",
            StreamKind::Hls,
            None,
            "480p HLS stream",
        ),
        line(
            "https://c.example/big.mp4",
            StreamKind::File,
            Some(40 * MB),
            "big",
        ),
        line(
            "https://c.example/1080.mpd",
            StreamKind::Dash,
            None,
            "1080p DASH stream",
        ),
        line(
            "https://c.example/master.m3u8",
            StreamKind::Hls,
            None,
            HLS_PLAIN,
        ),
    ]);
    assert_eq!(
        labels(&list),
        [
            HLS_PLAIN,
            "1080p DASH stream",
            "480p HLS stream",
            "big",
            "small"
        ]
    );
}

#[test]
fn two_lines_of_one_label_are_told_apart_by_their_file_names() {
    let list = panel_list(vec![
        line(
            "https://c.example/a/first.mp4?sig=1",
            StreamKind::File,
            Some(9 * MB),
            "MP4 file",
        ),
        line(
            "https://c.example/b/second.mp4#t=1",
            StreamKind::File,
            Some(8 * MB),
            "MP4 file",
        ),
        line(
            "https://c.example/",
            StreamKind::File,
            Some(7 * MB),
            "MP4 file",
        ),
        line(
            "https://c.example/c/other.webm",
            StreamKind::File,
            Some(6 * MB),
            "WebM file",
        ),
    ]);
    assert_eq!(
        labels(&list),
        [
            "MP4 file · first.mp4",
            "MP4 file · second.mp4",
            "MP4 file",
            "WebM file"
        ]
    );
}

/// A master playlist's address as Dailymotion's player asks for it, signed anew each time it
/// reads the video's details (issue #45: the panel listed "HLS stream · xby1uq2.m3u8" twice).
fn signed(sec: &str, ts: &str, first: &str) -> String {
    format!(
        "https://cdndirector.dailymotion.com/cdn/manifest/video/xby1uq2.m3u8?sec={sec}\
         &dmTs={ts}&dmV1st={first}"
    )
}

#[test]
fn a_playlist_asked_for_again_under_a_fresh_signature_is_one_line_at_its_newest_address() {
    let older = signed("_Cs7QACCO7w4h0_d045clRZUK", "385853", "54F0E6B397CC4AF5");
    let newer = signed("Lsvh5FLSRL3b0_lDsn3Xj6RY", "97888", "AA31321CEE6F20D0");
    let list = panel_list(vec![
        line(&older, StreamKind::Hls, None, HLS_PLAIN),
        line(&newer, StreamKind::Hls, None, HLS_PLAIN),
    ]);
    assert_eq!(
        labels(&list),
        [HLS_PLAIN],
        "one line, named by nothing else"
    );
    assert_eq!(
        list[0].url, newer,
        "the newest signature, which is still good"
    );

    // The one read keeps its name, whichever it was.
    let list = panel_list(vec![
        line(&older, StreamKind::Hls, None, "1080p HLS stream"),
        line(&newer, StreamKind::Hls, None, HLS_PLAIN),
    ]);
    assert_eq!(labels(&list), ["1080p HLS stream"]);
    assert_eq!(list[0].url, newer);
    let list = panel_list(vec![
        line(&older, StreamKind::Hls, None, HLS_PLAIN),
        line(&newer, StreamKind::Hls, None, "1080p HLS stream"),
    ]);
    assert_eq!(labels(&list), ["1080p HLS stream"]);
    assert_eq!(list[0].url, newer);

    // Another video's playlist stays a line of its own: another path, another `id`, other
    // query names, or a whole file rather than a playlist.
    let list = panel_list(vec![
        line(&older, StreamKind::Hls, None, HLS_PLAIN),
        line(
            "https://cdndirector.dailymotion.com/cdn/manifest/video/x9other.m3u8?sec=a&dmTs=1&dmV1st=b",
            StreamKind::Hls,
            None,
            HLS_PLAIN,
        ),
        line(
            "https://cdndirector.dailymotion.com/cdn/manifest/video/xby1uq2.m3u8?sec=a",
            StreamKind::Hls,
            None,
            HLS_PLAIN,
        ),
    ]);
    assert_eq!(list.len(), 3, "{list:?}");
    assert!(!signed_again(
        "https://c.example/live.m3u8?id=1&sig=a",
        "https://c.example/live.m3u8?id=2&sig=b"
    ));
    assert!(signed_again(
        "https://c.example/live.m3u8?id=1&sig=a",
        "https://c.example/live.m3u8?sig=b&id=1#t=3"
    ));
    let list = panel_list(vec![
        line(
            "https://c.example/v.mp4?sig=a",
            StreamKind::File,
            Some(9 * MB),
            "MP4 file",
        ),
        line(
            "https://c.example/v.mp4?sig=b",
            StreamKind::File,
            Some(9 * MB),
            "MP4 file",
        ),
    ]);
    assert_eq!(list.len(), 2, "files are not playlists: {list:?}");
}

#[test]
fn a_stream_goes_over_the_wire_as_the_extension_keeps_it() {
    let mut stream = line(
        "https://c.example/m.m3u8",
        StreamKind::Hls,
        Some(1),
        "HLS stream",
    );
    stream.members = vec!["https://c.example/720.m3u8".into()];
    stream.length = Some(95);
    let value = serde_json::to_value(&stream).unwrap();
    assert_eq!(value["kind"], "Hls");
    assert!(value.get("sound").is_none() && value.get("by_ranges").is_none());
    assert_eq!(serde_json::from_value::<Stream>(value).unwrap(), stream);
    // As an older extension kept it: no members, no length, no ranges.
    let older: Stream = serde_json::from_str(
        r#"{"url":"https://c.example/v.mp4","kind":"File","size":null,"label":"MP4 file"}"#,
    )
    .unwrap();
    assert_eq!(
        older,
        line(
            "https://c.example/v.mp4",
            StreamKind::File,
            None,
            "MP4 file"
        )
    );
}
