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
//! Where the first piece does not say how many follow, the video is fetched
//! from its other formats, as before dm read numbered pieces.
//!
//! The stand-in is named by an environment variable, which is the process's:
//! one server for every test here, each case a video id of its own.

mod common;

#[path = "common/mp4_samples.rs"]
mod mp4_samples;

use std::sync::{Mutex, OnceLock};

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
/// Without `count`, the first piece says nothing of how many follow. Returns the pieces in
/// order, and the picture's bytes each piece carries.
fn numbered(pieces: u32, count: bool) -> (Vec<Vec<u8>>, usize) {
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
    message.extend_from_slice(b"Sequence-Number: 0\r\n");
    if count {
        message.extend_from_slice(format!("Segment-Count: {pieces}\r\n").as_bytes());
    }
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

/// The stand-in's streams: the numbered pieces with their count, the same without it, the
/// picture as one file, the sound as one file.
struct Media {
    counted: Vec<Vec<u8>>,
    uncounted: Vec<Vec<u8>>,
    picture_bytes: usize,
    plain: Vec<u8>,
    sound: Vec<u8>,
}

fn media() -> &'static Media {
    static MEDIA: OnceLock<Media> = OnceLock::new();
    MEDIA.get_or_init(|| {
        let (counted, picture_bytes) = numbered(PIECES, true);
        let (uncounted, _) = numbered(PIECES, false);
        Media {
            counted,
            uncounted,
            picture_bytes,
            plain: fixture("picture.f137.mp4"),
            sound: fixture("sound.f140.m4a"),
        }
    })
}

/// Every stream request the stand-in was sent.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The stand-in's origin, started once for every test here on a runtime of its own.
fn stand_in() -> &'static str {
    static ORIGIN: OnceLock<String> = OnceLock::new();
    ORIGIN.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let origin = format!("http://{}", listener.local_addr().unwrap());
                let api = {
                    let origin = origin.clone();
                    move |body: String| player(origin.clone(), body)
                };
                let app = axum::Router::new()
                    .route("/watch", get(watch_page))
                    .route("/youtubei/v1/player", post(api))
                    .fallback(get(streams));
                tx.send(origin).unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        });
        let origin = rx.recv().unwrap();
        std::env::set_var("DM_YOUTUBE_HOST", &origin);
        origin
    })
}

/// The watch page: the player's timestamp and the visitor id; no stream needs solving.
async fn watch_page() -> Response {
    text(
        StatusCode::OK,
        "text/html",
        r#"<html><script>ytcfg.set({"PLAYER_JS_URL":"/s/player/abcd1234/player_ias.vflset/en_US/base.js","STS":12345,"VISITOR_DATA":"visitor-1"});</script></html>"#.into(),
    )
}

/// The player API's answer: the 1080p picture only as numbered pieces (no size, no ranges);
/// a 720p one as one file; the sound as one file.
async fn player(origin: String, body: String) -> Response {
    let asked: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let id = asked["videoId"].as_str().unwrap_or("").to_string();
    let m = media();
    let answer = serde_json::json!({
        "playabilityStatus": {"status": "OK"},
        "videoDetails": {"videoId": id, "title": "Pieces", "lengthSeconds": "6"},
        "streamingData": {"adaptiveFormats": [
            {"itag": 137, "mimeType": "video/mp4; codecs=\"avc1.640028\"", "width": 1920,
             "height": 1080, "fps": 30, "bitrate": 4000000, "type": "FORMAT_STREAM_TYPE_OTF",
             "url": format!("{origin}/numbered?id={id}&itag=137")},
            {"itag": 136, "mimeType": "video/mp4; codecs=\"avc1.4d401f\"", "width": 1280,
             "height": 720, "fps": 30, "bitrate": 2000000, "contentLength": m.plain.len().to_string(),
             "url": format!("{origin}/plain?id={id}&itag=136")},
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "averageBitrate": 130000,
             "contentLength": m.sound.len().to_string(), "url": format!("{origin}/sound?id={id}&itag=140")}
        ]}
    });
    text(StatusCode::OK, "application/json", answer.to_string())
}

/// The streams: each numbered piece by its `sq`, and the one files by range.
async fn streams(uri: Uri, headers: HeaderMap) -> Response {
    SEEN.lock().unwrap().push(uri.to_string());
    let m = media();
    let query = uri.query().unwrap_or("");
    let sq = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("sq="))
        .and_then(|n| n.parse::<usize>().ok());
    let pieces = if query.contains("id=nocount0001") {
        &m.uncounted
    } else {
        &m.counted
    };
    match (uri.path(), sq) {
        ("/numbered", Some(n)) if n < pieces.len() => Response::builder()
            .status(200)
            .header("content-length", pieces[n].len())
            .body(Body::from(pieces[n].clone()))
            .unwrap(),
        ("/plain", None) => ranged(&m.plain, &headers),
        ("/sound", None) => ranged(&m.sound, &headers),
        _ => text(StatusCode::NOT_FOUND, "text/plain", format!("no {uri}")),
    }
}

/// The stream requests for `id` so far.
fn seen_for(id: &str) -> Vec<String> {
    SEEN.lock()
        .unwrap()
        .iter()
        .filter(|u| u.contains(&format!("id={id}&")))
        .cloned()
        .collect()
}

fn page(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// The joined file holds a picture and a sound, AVC and AAC, the picture's samples in order:
/// the picture's bytes.
fn picture_and_sound(path: &std::path::Path) -> usize {
    let joined = std::fs::read(path).unwrap();
    let all = tracks(&joined);
    assert_eq!(all.len(), 2, "picture and sound are both in it");
    let picture = all.iter().find(|t| &t.handler == b"vide").unwrap();
    let sound = all.iter().find(|t| &t.handler == b"soun").unwrap();
    assert_eq!(&picture.codec, b"avc1");
    assert_eq!(&sound.codec, b"mp4a");
    assert!(picture.samples.windows(2).all(|w| w[0].dts < w[1].dts));
    assert!(!sound.samples.is_empty());
    picture.samples.iter().map(|s| s.data.len()).sum()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_best_picture_served_as_numbered_pieces_is_fetched_whole_with_its_sound() {
    stand_in();
    let id = "numbered001";

    // The reading picks the numbered picture, and says it is numbered pieces.
    let staging = tempdir();
    let read = youtube::read(
        &client(),
        &page(id),
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
        &page(id),
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
    let done = videopage::download_page(&client(), &page(id), &options(dir.path()), None)
        .await
        .expect("the video served as numbered pieces did not download");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Pieces [numbered001].mp4"
    );
    let seen = seen_for(id);
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
    // Every piece's picture is in it, in order.
    assert_eq!(
        picture_and_sound(&done.path),
        PIECES as usize * media().picture_bytes
    );
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");
}

/// The first piece does not say how many follow: the video is fetched as before dm read
/// numbered pieces, the 720p picture as one file, whole, with its sound, and no numbered piece
/// after the first is asked for.
#[tokio::test(flavor = "multi_thread")]
async fn numbered_pieces_that_do_not_say_how_many_give_way_to_the_other_formats() {
    stand_in();
    let id = "nocount0001";
    let dir = tempdir();
    let done = videopage::download_page(&client(), &page(id), &options(dir.path()), None)
        .await
        .expect("the video did not fall back to its other formats");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Pieces [nocount0001].mp4"
    );
    let seen = seen_for(id);
    assert!(
        seen.iter()
            .any(|u| u.starts_with("/numbered?") && u.ends_with("&sq=0")),
        "the first piece was asked for: {seen:?}"
    );
    assert!(
        !seen
            .iter()
            .any(|u| u.starts_with("/numbered?") && !u.ends_with("&sq=0")),
        "a piece after the first was asked for: {seen:?}"
    );
    assert!(seen.iter().any(|u| u.starts_with("/plain")));
    // The one file's picture, whole.
    assert_eq!(picture_and_sound(&done.path), media().picture_bytes);
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");
}
