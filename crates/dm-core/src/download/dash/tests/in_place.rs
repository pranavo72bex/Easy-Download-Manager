// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A stream joined into its picture where the disk has no room for the join beside its tracks
//! (gaps row Y21).

use super::*;

/// A picture of sixty fragments listed by byte range and a sound a small part of it, in an MPD
/// of their own on 127.0.0.1; and the two files' bytes together.
async fn serve_long() -> (String, tempfile::TempDir, u64) {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 60);
    let (audio, _) = plain(dir.path(), "a.m4a", 40, 7);
    let video = std::fs::read(video).unwrap();
    let audio = std::fs::read(audio).unwrap();
    let both = (video.len() + audio.len()) as u64;
    let starts = box_starts(&video);
    let mut list = format!(r#"<Initialization range="0-{}"/>"#, starts[2] - 1);
    for f in 0..60 {
        let (a, b) = (starts[2 + f * 2], starts[4 + f * 2] - 1);
        list.push_str(&format!(r#"<SegmentURL mediaRange="{a}-{b}"/>"#));
    }
    let mpd = format!(
        r#"<MPD type="static" mediaPresentationDuration="PT2S"><Period>
            <AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="9" height="360">
              <BaseURL>v.mp4</BaseURL><SegmentList>{list}</SegmentList></Representation></AdaptationSet>
            <AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1">
              <BaseURL>a.m4a</BaseURL><SegmentBase indexRange="0-1"/></Representation></AdaptationSet>
            </Period></MPD>"#
    );
    let app = axum::Router::new()
        .route(
            "/show/video.mpd",
            get(move || {
                let mpd = mpd.clone();
                async move {
                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/dash+xml")
                        .body(Body::from(mpd))
                        .unwrap()
                }
            }),
        )
        .route("/show/v.mp4", ranged(video, true))
        .route("/show/a.m4a", ranged(audio, true));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/show/video.mpd"), dir, both)
}

/// The kinds of the top-level boxes in `data`, a 64-bit size read where a box has one.
fn box_kinds(data: &[u8]) -> Vec<[u8; 4]> {
    let mut at = 0;
    let mut kinds = Vec::new();
    while at + 8 <= data.len() {
        let mut size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        if size == 1 {
            size = u64::from_be_bytes(data[at + 8..at + 16].try_into().unwrap()) as usize;
        }
        assert!(size >= 8, "a box of no length at {at}");
        kinds.push(data[at + 4..at + 8].try_into().unwrap());
        at += size;
    }
    kinds
}

#[tokio::test]
async fn a_join_with_no_room_beside_its_tracks_is_written_into_the_picture() {
    let client = crate::reqwest_defaults();
    // With room: the usual join, for the samples it holds.
    let (url, dir, _) = serve_long().await;
    let usual = crate::download::download(&client, &url, &options(dir.path()))
        .await
        .unwrap();
    let made = crate::mp4mux::testing::read_back(&usual.path);

    // Room for the tracks' bytes but one: not for the join beside them, but for what joining
    // into the picture adds.
    let (url, dir, both) = serve_long().await;
    let tight = Options {
        room: crate::Room::with_free_space(move |_| Some(both - 1)),
        ..options(dir.path())
    };
    let done = crate::download::download(&client, &url, &tight)
        .await
        .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    let joined = crate::mp4mux::testing::read_back(&done.path);
    assert_eq!(joined.len(), 2, "a picture and a sound");
    for (ours, theirs) in joined.iter().zip(&made) {
        assert_eq!(ours.samples, theirs.samples);
        assert_eq!(ours.durations, theirs.durations);
        assert_eq!(ours.entries, theirs.entries);
    }
    let kinds = box_kinds(&std::fs::read(&done.path).unwrap());
    assert_eq!(
        kinds.last(),
        Some(b"moov"),
        "the index last: written into the picture"
    );
    assert!(
        !kinds.contains(b"moof"),
        "its fragments made free: {kinds:?}"
    );
    let left: Vec<String> = std::fs::read_dir(&tight.parts_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("track") || name.ends_with(".dm-joining"))
        .collect();
    assert!(
        left.is_empty(),
        "the tracks and their marks are gone: {left:?}"
    );

    // No room even for that: refused in the words of the check before fetching, as before.
    let (url, dir, _) = serve_long().await;
    let full = Options {
        room: crate::Room::with_free_space(|_| Some(100)),
        ..options(dir.path())
    };
    let err = crate::download::download(&client, &url, &full)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("not enough room: the file needs "), "{err}");
}

/// A track a join in place was cut short in is fetched again, its mark gone with it.
#[tokio::test]
async fn a_track_a_join_in_place_was_cut_short_in_is_fetched_again() {
    let parts = tempfile::tempdir().unwrap();
    let track = parts.path().join("track0.mp4");
    std::fs::write(&track, b"half a join").unwrap();
    std::fs::write(crate::mp4mux::joining_mark(&track), b"").unwrap();
    crate::download::drop_spoiled(&track).await;
    assert!(!track.exists() && !crate::mp4mux::joining_mark(&track).exists());
    // One with no mark is what was fetched: kept.
    std::fs::write(&track, b"whole").unwrap();
    crate::download::drop_spoiled(&track).await;
    assert!(track.exists());
}
