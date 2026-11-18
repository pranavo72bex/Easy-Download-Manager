// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube video whose best picture is served only as numbered pieces
//! (`FORMAT_STREAM_TYPE_OTF`: `&sq=0` its first boxes, saying how many pieces
//! follow, then `&sq=1` on), read by dm itself and fetched by the DASH piece
//! fetcher (gaps row Y14), against a stand-in for YouTube: its watch page, its
//! player API and the streams are all a local server's. That picture was left
//! out, and a video served that way lost its higher qualities.
//!
//! One test in this binary: the stand-in is named by an environment variable,
//! which is the process's.

mod common;

#[path = "common/mp4_samples.rs"]
mod mp4_samples;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use dm_core::{client, videopage, youtube, Quality};

use common::{options, tempdir};
use mp4_samples::tracks;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mp4")
            .join(name),
    )
    .unwrap()
}

/// The top-level boxes of `data`: each one's kind, where it starts and its size.
fn top_boxes(data: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let mut at = 0;
    let mut out = Vec::new();
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().unwrap();
        let size = if size == 0 { data.len() - at } else { size };
        out.push((kind, at, size));
        at += size;
    }
    out
}

/// A box of `kind` inside `data` (nested boxes searched depth first): where its body starts,
/// and the body's length.
fn find_box(data: &[u8], kind: &[u8; 4]) -> Option<(usize, usize)> {
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        let k = &data[at + 4..at + 8];
        if size < 8 || at + size > data.len() {
            return None;
        }
        if k == kind {
            return Some((at + 8, size - 8));
        }
        if matches!(k, b"moov" | b"trak" | b"moof" | b"traf" | b"mdia" | b"minf") {
            if let Some(found) = find_box(&data[at + 8..at + size], kind) {
                return Some((at + 8 + found.0, found.1));
            }
        }
        at += size;
    }
    None
}

/// The picture fixture (one piece of a second or so) as numbered pieces: the first (`sq=0`)
/// its first boxes and the words on how many pieces follow, inside an `emsg` box; then
/// `pieces` copies of its one piece, each with its own sequence number and decode time.
/// Returns the pieces in order, and the picture's bytes each piece carries.
fn numbered(pieces: u32) -> (Vec<Vec<u8>>, usize) {
    let whole = fixture("picture.f137.mp4");
    let boxes = top_boxes(&whole);
    let at = |kind: &[u8; 4]| {
        boxes
            .iter()
            .find(|(k, _, _)| k == kind)
            .map(|(_, a, s)| (*a, *s))
            .unwrap()
    };
    let (moov_at, moov_size) = at(b"moov");
    let (sidx_at, sidx_size) = at(b"sidx");
    let (moof_at, _) = at(b"moof");
    let (mdat_at, mdat_size) = at(b"mdat");
    let sidx = &whole[sidx_at..sidx_at + sidx_size];
    let entries = if sidx[8] == 1 { 40 } else { 32 };
    let duration = u32::from_be_bytes(sidx[entries + 4..entries + 8].try_into().unwrap());

    let mut message = Vec::new();
    message.extend_from_slice(&[0, 0, 0, 0]);
    message.extend_from_slice(b"urn:stand-in:pieces\0\0");
    message.extend_from_slice(&[0; 16]);
    message.extend_from_slice(
        format!("Sequence-Number: 0\r\nSegment-Count: {pieces}\r\n").as_bytes(),
    );
    let mut first = whole[..moov_at + moov_size].to_vec();
    first.extend_from_slice(&(8 + message.len() as u32).to_be_bytes());
    first.extend_from_slice(b"emsg");
    first.extend_from_slice(&message);

    let mut out = vec![first];
    for n in 0..pieces {
        let mut piece = whole[moof_at..mdat_at + mdat_size].to_vec();
        let (seq_at, _) = find_box(&piece, b"mfhd").unwrap();
        piece[seq_at + 4..seq_at + 8].copy_from_slice(&(n + 1).to_be_bytes());
        let (tfdt_at, _) = find_box(&piece, b"tfdt").unwrap();
        let time = u64::from(n) * u64::from(duration);
        if piece[tfdt_at] == 1 {
            piece[tfdt_at + 4..tfdt_at + 12].copy_from_slice(&time.to_be_bytes());
        } else {
            piece[tfdt_at + 4..tfdt_at + 8].copy_from_slice(&(time as u32).to_be_bytes());
        }
        out.push(piece);
    }
    (out, mdat_size - 8)
}

/// A piece of `data` as a range request asks for it.
fn ranged(data: &[u8], headers: &HeaderMap) -> Response {
    let size = data.len();
    let (a, b) = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap(),
                b.parse::<usize>().unwrap_or(size - 1).min(size - 1),
            )
        })
        .unwrap_or((0, size - 1));
    Response::builder()
        .status(206)
        .header("content-range", format!("bytes {a}-{b}/{size}"))
        .header("content-length", b - a + 1)
        .body(Body::from(bytes::Bytes::copy_from_slice(&data[a..=b])))
        .unwrap()
}

fn text(status: StatusCode, kind: &str, body: String) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", kind)
        .body(Body::from(body))
        .unwrap()
}

const PIECES: u32 = 6;

#[tokio::test(flavor = "multi_thread")]
async fn the_best_picture_served_as_numbered_pieces_is_fetched_whole_with_its_sound() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (pieces, picture_bytes) = numbered(PIECES);
    let pieces = Arc::new(pieces);
    let sound = Arc::new(fixture("sound.f140.m4a"));
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();

    let player = {
        let origin = origin.clone();
        let sound_len = sound.len();
        move || {
            let origin = origin.clone();
            async move {
                // The 1080p picture only as numbered pieces (no size, no ranges); a 720p one as
                // one file; the sound as one file.
                let answer = serde_json::json!({
                    "playabilityStatus": {"status": "OK"},
                    "videoDetails": {"videoId": "numbered001", "title": "Pieces", "lengthSeconds": "6"},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 137, "mimeType": "video/mp4; codecs=\"avc1.640028\"", "width": 1920,
                         "height": 1080, "fps": 30, "bitrate": 4000000, "type": "FORMAT_STREAM_TYPE_OTF",
                         "url": format!("{origin}/numbered?itag=137")},
                        {"itag": 136, "mimeType": "video/mp4; codecs=\"avc1.4d401f\"", "width": 1280,
                         "height": 720, "fps": 30, "bitrate": 2000000, "contentLength": "2000",
                         "url": format!("{origin}/plain?itag=136")},
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "averageBitrate": 130000,
                         "contentLength": sound_len.to_string(), "url": format!("{origin}/sound?itag=140")}
                    ]}
                });
                text(StatusCode::OK, "application/json", answer.to_string())
            }
        }
    };
    let streams = {
        let (pieces, sound, seen) = (pieces.clone(), sound.clone(), seen.clone());
        move |uri: Uri, headers: HeaderMap| {
            let (pieces, sound, seen) = (pieces.clone(), sound.clone(), seen.clone());
            async move {
                seen.lock().unwrap().push(uri.to_string());
                let sq = uri
                    .query()
                    .unwrap_or("")
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("sq="))
                    .and_then(|n| n.parse::<usize>().ok());
                match (uri.path(), sq) {
                    ("/numbered", Some(n)) if n < pieces.len() => Response::builder()
                        .status(200)
                        .header("content-length", pieces[n].len())
                        .body(Body::from(pieces[n].clone()))
                        .unwrap(),
                    ("/sound", None) => ranged(&sound, &headers),
                    _ => text(StatusCode::NOT_FOUND, "text/plain", format!("no {uri}")),
                }
            }
        }
    };
    let app = axum::Router::new()
        .route(
            "/watch",
            get(|| async {
                text(
                    StatusCode::OK,
                    "text/html",
                    r#"<html><script>ytcfg.set({"PLAYER_JS_URL":"/s/player/abcd1234/player_ias.vflset/en_US/base.js","STS":12345,"VISITOR_DATA":"visitor-1"});</script></html>"#.into(),
                )
            }),
        )
        .route("/youtubei/v1/player", post(player))
        .fallback(get(streams));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    std::env::set_var("DM_YOUTUBE_HOST", &origin);
    let page = "https://www.youtube.com/watch?v=numbered001";

    // The reading picks the numbered picture, and says it is numbered pieces.
    let staging = tempdir();
    let read = youtube::read(
        &client(),
        page,
        Quality::Best,
        None,
        false,
        None,
        staging.path(),
    )
    .await
    .unwrap();
    let read: serde_json::Value = serde_json::from_str(&read).unwrap();
    let pair = read["requested_formats"].as_array().unwrap();
    assert_eq!(pair[0]["format_id"], "137");
    assert_eq!(pair[0]["protocol"], "http_dash_segments");
    assert_eq!(pair[1]["format_id"], "140");
    assert_eq!(pair[1]["protocol"], "https");
    // About 4 Mbit/s for six seconds, for the quality list.
    assert_eq!(pair[0]["filesize_approx"], 3_000_000);
    // A part is cut by an index, which numbered pieces have none of: the one file is taken.
    let part = youtube::read(
        &client(),
        page,
        Quality::Best,
        None,
        true,
        None,
        staging.path(),
    )
    .await
    .unwrap();
    let part: serde_json::Value = serde_json::from_str(&part).unwrap();
    assert_eq!(part["requested_formats"][0]["format_id"], "136");

    // The download: every numbered piece fetched once, the 720p one never, and the file whole.
    let dir = tempdir();
    let done = videopage::download_page(&client(), page, &options(dir.path()), None)
        .await
        .expect("the video served as numbered pieces did not download");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Pieces [numbered001].mp4"
    );
    let seen = seen.lock().unwrap().clone();
    for n in 0..=PIECES {
        let asked = seen
            .iter()
            .filter(|u| u.starts_with("/numbered?") && u.ends_with(&format!("&sq={n}")))
            .count();
        // The first is asked twice: once for how many follow, once as the track's first piece.
        let expected = if n == 0 { 2 } else { 1 };
        assert_eq!(asked, expected, "piece {n} asked {asked} times: {seen:?}");
    }
    assert!(
        !seen.iter().any(|u| u.starts_with("/plain")),
        "the smaller picture was fetched: {seen:?}"
    );
    assert!(seen.iter().any(|u| u.starts_with("/sound")));

    let joined = std::fs::read(&done.path).unwrap();
    let all = tracks(&joined);
    assert_eq!(all.len(), 2, "picture and sound are both in it");
    let picture = all.iter().find(|t| &t.handler == b"vide").unwrap();
    let sound_track = all.iter().find(|t| &t.handler == b"soun").unwrap();
    assert_eq!(&picture.codec, b"avc1");
    assert_eq!(&sound_track.codec, b"mp4a");
    // Every piece's picture is in it, in order.
    let carried: usize = picture.samples.iter().map(|s| s.data.len()).sum();
    assert_eq!(carried, PIECES as usize * picture_bytes);
    assert!(picture.samples.windows(2).all(|w| w[0].dts < w[1].dts));
    assert!(!sound_track.samples.is_empty());
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");
}
