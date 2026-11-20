// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube premiere waited for (gaps row Y5), against a stand-in for YouTube: its player
//! first answers that the video is offline, set to start a second later, then that it is live
//! with an HLS playlist of it, which the stand-in serves as a live stream that ends after a
//! few asks. The download waits, its row saying so, reads the video again past the start, and
//! records the stream. One set more than a week ahead is refused saying when; a cancel ends
//! the wait at once.
//!
//! One test in this binary: the stand-in is named by environment variables, which are the
//! process's.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use dm_core::{client, videopage, Error, Update};
use serde_json::{json, Value};

use common::{options, tempdir};

/// The premiere that starts a second after it is first asked about.
const SOON: &str = "premiere001";
/// One set to start more than a week away.
const FAR: &str = "farAway0001";
/// One an hour away, cancelled while it waits.
const LATER: &str = "cancelled01";

fn text(kind: &str, body: impl Into<Body>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", kind)
        .body(body.into())
        .unwrap()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// What segment `n` of the live stream carries, distinct for each.
fn segment(n: usize) -> Vec<u8> {
    (0..16 * 1024).map(|i| ((n * 41 + i) % 251) as u8).collect()
}

/// The player's answer: offline, set to start `starts_in` seconds from now, as an upcoming
/// stream's offline slate says it.
fn offline(id: &str, starts_in: u64) -> Value {
    json!({
        "playabilityStatus": {
            "status": "LIVE_STREAM_OFFLINE",
            "reason": "Premieres in a moment",
            "liveStreamability": {"liveStreamabilityRenderer": {"videoId": id,
                "offlineSlate": {"liveStreamOfflineSlateRenderer": {
                    "scheduledStartTime": (unix_now() + starts_in).to_string()}}}},
        },
        "videoDetails": {"videoId": id, "title": "Launch", "isUpcoming": true,
            "isLiveContent": true},
    })
}

/// The live playlist at its `ask`th ask (the probe's, with a range, not counted): one segment,
/// then two, then the two with its end.
fn live_playlist(ask: usize) -> String {
    let out = (ask + 1).min(2);
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n");
    text.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
    for n in 0..out {
        text.push_str(&format!("#EXTINF:1.0,\nseg{n}.ts\n"));
    }
    if ask >= 2 {
        text.push_str("#EXT-X-ENDLIST\n");
    }
    text
}

#[tokio::test(flavor = "multi_thread")]
async fn a_premiere_is_waited_for_then_recorded_as_a_live_stream() {
    let asks: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
    let playlist_asks: Arc<Mutex<usize>> = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());

    let player_api = {
        let (asks, origin) = (asks.clone(), origin.clone());
        move |body: String| {
            let (asks, origin) = (asks.clone(), origin.clone());
            async move {
                let body: Value = serde_json::from_str(&body).unwrap_or_default();
                let id = body["videoId"].as_str().unwrap_or("").to_string();
                let ask = {
                    let mut asks = asks.lock().unwrap();
                    let n = asks.entry(id.clone()).or_default();
                    *n += 1;
                    *n
                };
                let answer = match id.as_str() {
                    SOON if ask == 1 => offline(&id, 1),
                    SOON => json!({
                        "playabilityStatus": {"status": "OK"},
                        "videoDetails": {"videoId": id, "title": "Launch", "isLive": true,
                            "isLiveContent": true},
                        "streamingData": {"hlsManifestUrl": format!("{origin}/live/live.m3u8")},
                    }),
                    FAR => offline(&id, 8 * 24 * 3_600),
                    _ => offline(&id, 3_600),
                };
                text("application/json", answer.to_string())
            }
        }
    };
    let playlist = {
        let playlist_asks = playlist_asks.clone();
        move |headers: HeaderMap| {
            let playlist_asks = playlist_asks.clone();
            async move {
                let ask = {
                    let mut asked = playlist_asks.lock().unwrap();
                    let ask = *asked;
                    if headers.get("range").is_none() {
                        *asked += 1;
                    }
                    ask
                };
                text("application/vnd.apple.mpegurl", live_playlist(ask))
            }
        }
    };
    let app = axum::Router::new()
        .route(
            "/watch",
            get(|| async {
                text(
                    "text/html",
                    r#"<html><script>ytcfg.set({"STS":12345,"VISITOR_DATA":"visitor-1"});</script></html>"#,
                )
            }),
        )
        .route("/youtubei/v1/player", post(player_api))
        .route("/live/live.m3u8", get(playlist))
        .fallback(get(|uri: Uri| async move {
            let n = uri
                .path()
                .strip_prefix("/live/seg")
                .and_then(|rest| rest.strip_suffix(".ts"))
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(99);
            text("video/mp2t", segment(n))
        }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempdir();
    std::env::set_var("DM_HOME", dir.path().join("home"));
    std::env::set_var("DM_YOUTUBE_HOST", &origin);
    let watch = |id: &str| format!("https://www.youtube.com/watch?v={id}");

    // Set to start a second from now: waited for, its row saying so, then recorded.
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let heard = tokio::spawn(async move {
        let mut notes = Vec::new();
        while let Some(update) = rx.recv().await {
            if let Update::Note(said) = update {
                notes.push(said);
            }
        }
        notes
    });
    let began = Instant::now();
    let done = videopage::download_page(&client(), &watch(SOON), &options(dir.path()), Some(tx))
        .await
        .expect("the premiere was not recorded once it began");
    assert!(
        began.elapsed() >= Duration::from_secs(2),
        "the premiere was not waited for: {:?}",
        began.elapsed()
    );
    let mut whole = segment(0);
    whole.extend(segment(1));
    assert_eq!(std::fs::read(&done.path).unwrap(), whole);
    assert_eq!(asks.lock().unwrap().get(SOON), Some(&2));
    assert!(*playlist_asks.lock().unwrap() >= 3, "not recorded as live");
    let notes = heard.await.unwrap();
    let waited = notes
        .iter()
        .position(|n| videopage::waits_for_premiere(n))
        .unwrap_or_else(|| panic!("the row never said it waits: {notes:?}"));
    // The wait's word is taken off the row before the recording.
    assert!(notes[waited..].iter().any(String::is_empty), "{notes:?}");

    // More than a week ahead: refused, saying when it starts.
    let why = videopage::download_page(&client(), &watch(FAR), &options(dir.path()), None)
        .await
        .expect_err("a premiere a week away was waited for");
    let why = why.to_string();
    assert!(why.starts_with("the premiere starts at "), "{why}");
    assert!(why.ends_with(", more than a week from now"), "{why}");

    // An hour away, cancelled while it waits: over at once.
    let waiting = options(dir.path());
    let control = waiting.control.clone();
    let began = Instant::now();
    let (outcome, ()) = tokio::join!(
        videopage::download_page(&client(), &watch(LATER), &waiting, None),
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            control.cancel();
        }
    );
    let why = outcome.err();
    assert!(matches!(why, Some(Error::Cancelled)), "{why:?}");
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(asks.lock().unwrap().get(LATER), Some(&1));
}
