// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::download::dash::tests::{cut_into_pieces, options, samples_in};
use crate::mp4mux::tests::fragmented;
use axum::body::Body;
use axum::response::Response;
use axum::routing::get;

/// A live manifest of a picture in pieces of a tenth of a second, named by their time, the
/// timeline given; `ended` makes it static, as a stream that has ended is.
fn manifest(timeline: &str, ended: bool) -> String {
    let kind = if ended {
        r#"type="static" mediaPresentationDuration="PT0.6S""#
    } else {
        r#"type="dynamic" minimumUpdatePeriod="PT0.1S""#
    };
    format!(
        r#"<MPD {kind}><Period id="p" start="PT0S"><AdaptationSet mimeType="video/mp4">
        <SegmentTemplate timescale="90000" initialization="init.mp4" media="seg-$Time$.m4s">
        <SegmentTimeline>{timeline}</SegmentTimeline></SegmentTemplate>
        <Representation id="v" bandwidth="9" height="360"/></AdaptationSet></Period></MPD>"#
    )
}

/// Serves a live stream of six pieces whose manifest, read again and again, says what
/// `reading` gives for its nth reading; counts every request by its path.
async fn serve_live(
    reading: fn(usize) -> String,
) -> (String, Arc<std::sync::Mutex<Vec<String>>>, Vec<Vec<u8>>) {
    let dir = tempfile::tempdir().unwrap();
    let (video, samples) = fragmented(dir.path(), "v.mp4", 6);
    let (init, pieces) = cut_into_pieces(&std::fs::read(video).unwrap());
    let readings = Arc::new(AtomicUsize::new(0));
    let mut app = axum::Router::new()
        .route(
            "/live/stream.mpd",
            get(move || {
                let n = readings.fetch_add(1, Ordering::SeqCst);
                async move { Response::new(Body::from(reading(n))) }
            }),
        )
        .route(
            "/live/init.mp4",
            get(move || async move { Response::new(Body::from(init)) }),
        );
    for (n, piece) in pieces.into_iter().enumerate() {
        app = app.route(
            &format!("/live/seg-{}.m4s", n * 9000),
            get(move || async move { Response::new(Body::from(piece)) }),
        );
    }
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let noted = asked.clone();
    let app = app.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let noted = noted.clone();
            async move {
                noted.lock().unwrap().push(request.uri().path().to_string());
                next.run(request).await
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/live/stream.mpd"), asked, samples)
}

/// A stand-in for a live DASH stream: its manifest lists two pieces, then (the first gone
/// from the server's window) three newer, then turns static listing all six. The recording
/// plays the six pieces' samples in order, each fetched once, and ends by itself when the
/// manifest says the stream has (gaps row Y18).
#[tokio::test]
async fn a_live_manifest_that_grows_then_ends_is_recorded_whole_in_order() {
    // The first reading is the address being looked at before it is known to be a manifest.
    let (url, asked, samples) = serve_live(|n| match n {
        0 | 1 => manifest(r#"<S t="0" d="9000" r="1"/>"#, false),
        2 => manifest(r#"<S t="9000" d="9000" r="2"/>"#, false),
        _ => manifest(r#"<S t="0" d="9000" r="5"/>"#, true),
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let options = options(dir.path());
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let done =
        crate::download::download_reporting(&crate::reqwest_defaults(), &url, &options, Some(tx))
            .await
            .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "live.mp4");
    let (got, lengths) = samples_in(&done.path);
    assert_eq!(got, samples, "the six pieces' samples, in order");
    assert_eq!(lengths, vec![3000; 18]);
    let asked = asked.lock().unwrap().clone();
    let readings = asked.iter().filter(|p| p.ends_with(".mpd")).count();
    assert_eq!(readings, 4, "read until it turned static: {asked:?}");
    for n in 0..6 {
        let piece = format!("/live/seg-{}.m4s", n * 9000);
        assert_eq!(
            asked.iter().filter(|p| **p == piece).count(),
            1,
            "{piece} fetched once: {asked:?}"
        );
    }
    let mut named = false;
    while let Ok(update) = rx.try_recv() {
        named |= matches!(update, Update::Named { ref name, .. } if name == "live.mp4");
    }
    assert!(named, "the row is named as the recording starts");
    assert!(!options.parts_dir.join("live-picture.recording").exists());
}

/// A stream that ends may list pieces from before dm began recording it: they are older than
/// what was recorded, and are not put after it.
#[tokio::test]
async fn pieces_from_before_the_recording_began_are_not_put_after_it() {
    let (url, _asked, samples) = serve_live(|n| match n {
        0 | 1 => manifest(r#"<S t="9000" d="9000" r="2"/>"#, false),
        _ => manifest(r#"<S t="0" d="9000" r="5"/>"#, true),
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options(dir.path()))
        .await
        .unwrap();
    let (got, _) = samples_in(&done.path);
    assert_eq!(got, samples[3..], "from the second piece on, in order");
}

/// A live stream stopped by the person keeps what was recorded, as live HLS does: a live
/// stream cannot be resumed from where it was.
#[tokio::test]
async fn a_live_recording_stopped_keeps_what_it_has() {
    let (url, _asked, samples) =
        serve_live(|_| manifest(r#"<S t="0" d="9000" r="1"/>"#, false)).await;
    let dir = tempfile::tempdir().unwrap();
    let options = options(dir.path());
    let control = options.control.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1200)).await;
        control.pause();
    });
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    let (got, _) = samples_in(&done.path);
    assert_eq!(got, samples[..6], "the two pieces listed, once each");
}
