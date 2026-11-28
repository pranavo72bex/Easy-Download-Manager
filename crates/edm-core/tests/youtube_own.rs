// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube watch page read by EDM itself (`edm_core::youtube`), against a
//! stand-in for YouTube: its watch page, its player API, its player script
//! and the solver's releases are all a local server's, and the streams are
//! served only once their `n` values and signature have been solved. No
//! nothing else is available to fall back on, so the download is the reader's.
//!
//! One test in this binary: the stand-in is named by environment variables,
//! which are the process's. The stand-in also serves the video's captions, which go into the
//! finished MP4 as a text track with Options' subtitles on (gaps row Y6).

mod common;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use edm_core::{client, videopage};

use common::{options, tempdir};

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

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

/// A box of `kind` inside `data` (nested boxes searched depth first):
/// where its body starts, and the body's length.
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
        // Only the boxes that hold others are looked inside.
        if matches!(
            k,
            b"moov" | b"trak" | b"edts" | b"moof" | b"traf" | b"mdia" | b"minf"
        ) {
            if let Some(found) = find_box(&data[at + 8..at + size], kind) {
                return Some((at + 8 + found.0, found.1));
            }
        }
        at += size;
    }
    None
}

/// A fixture (one piece of a second or so) as a stream of `pieces` pieces,
/// each a copy of the one with its own sequence number and decode time,
/// under a new index: the shape YouTube's DASH streams come in. Returns the
/// file, where its first boxes end (inclusive), and its index's range.
fn pieced(name: &str, pieces: u32) -> (Vec<u8>, (u64, u64), (u64, u64)) {
    let whole = fixture(name);
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
    let (moof_at, moof_size) = at(b"moof");
    let (mdat_at, mdat_size) = at(b"mdat");
    let init = whole[..moov_at + moov_size].to_vec();
    let sidx = &whole[sidx_at..sidx_at + sidx_size];
    let timescale = u32::from_be_bytes(sidx[16..20].try_into().unwrap());
    // The first entry's duration: after the 64-bit times of a version 1 index.
    let entries = if sidx[8] == 1 { 40 } else { 32 };
    let duration = u32::from_be_bytes(sidx[entries + 4..entries + 8].try_into().unwrap());
    let piece_len = (moof_size + mdat_size) as u32;
    // The new index: version 0, `pieces` references, the first piece right after it.
    let mut index = Vec::new();
    index.extend_from_slice(&(32 + 12 * pieces).to_be_bytes());
    index.extend_from_slice(b"sidx");
    index.extend_from_slice(&[0, 0, 0, 0]);
    index.extend_from_slice(&1u32.to_be_bytes());
    index.extend_from_slice(&timescale.to_be_bytes());
    index.extend_from_slice(&0u32.to_be_bytes());
    index.extend_from_slice(&0u32.to_be_bytes());
    index.extend_from_slice(&0u16.to_be_bytes());
    index.extend_from_slice(&(pieces as u16).to_be_bytes());
    for _ in 0..pieces {
        index.extend_from_slice(&piece_len.to_be_bytes());
        index.extend_from_slice(&duration.to_be_bytes());
        index.extend_from_slice(&0x9000_0000u32.to_be_bytes());
    }
    let mut file = init.clone();
    file.extend_from_slice(&index);
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
        file.extend_from_slice(&piece);
    }
    let init_range = (0, init.len() as u64 - 1);
    let index_range = (init.len() as u64, (init.len() + index.len()) as u64 - 1);
    (file, init_range, index_range)
}

/// A piece of `data` as a range request asks for it.
fn ranged(data: &[u8], headers: &HeaderMap) -> Response {
    let size = data.len();
    let (a, b) = range_asked(headers, size);
    Response::builder()
        .status(206)
        .header("content-range", format!("bytes {a}-{b}/{size}"))
        .header("content-length", b - a + 1)
        .header("etag", "\"v\"")
        .body(Body::from(bytes::Bytes::copy_from_slice(&data[a..=b])))
        .unwrap()
}

/// The range a request asks for, inclusive, or the whole.
fn range_asked(headers: &HeaderMap, size: usize) -> (usize, usize) {
    headers
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
        .unwrap_or((0, size - 1))
}

fn text(status: StatusCode, kind: &str, body: String) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", kind)
        .body(Body::from(body))
        .unwrap()
}

/// Whether a JavaScript engine the reader can answer the player's checks in is here: edm-js,
/// always (gaps row F15).
fn an_engine_is_here() -> bool {
    true
}

#[tokio::test(flavor = "multi_thread")]
async fn a_youtube_page_is_read_and_fetched_by_edm_with_no_video_helper() {
    if !an_engine_is_here() {
        eprintln!("no JavaScript engine here: skipped");
        return;
    }
    let seen = Seen::default();
    let requests = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    // The streams as YouTube serves them: six indexed pieces each.
    let (picture, pic_init, pic_index) = pieced("picture.f137.mp4", 6);
    let (sound, snd_init, snd_index) = pieced("sound.f140.m4a", 6);
    let (picture, sound) = (Arc::new(picture), Arc::new(sound));

    let player_api = {
        let origin = origin.clone();
        let (picture_len, sound_len) = (picture.len(), sound.len());
        let edges =
            |(a, b): (u64, u64)| serde_json::json!({"start": a.to_string(), "end": b.to_string()});
        let (pic_init, pic_index, snd_init, snd_index) = (
            edges(pic_init),
            edges(pic_index),
            edges(snd_init),
            edges(snd_index),
        );
        move |headers: HeaderMap, body: String| {
            let (pic_init, pic_index, snd_init, snd_index) = (
                pic_init.clone(),
                pic_index.clone(),
                snd_init.clone(),
                snd_index.clone(),
            );
            let origin = origin.clone();
            async move {
                let client_name = headers
                    .get("x-youtube-client-name")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let visitor = headers
                    .get("x-goog-visitor-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let asked: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let as_tv = asked
                    .pointer("/context/client/clientName")
                    .and_then(|v| v.as_str())
                    == Some("TVHTML5");
                let sts = asked
                    .pointer("/playbackContext/contentPlaybackContext/signatureTimestamp")
                    .and_then(|v| v.as_u64());
                let id = asked.get("videoId").and_then(|v| v.as_str()).unwrap_or("");
                if client_name != "7"
                    || visitor != "visitor-1"
                    || !as_tv
                    || sts != Some(12345)
                    || id != "jNQXAC9IVRw"
                {
                    return text(
                        StatusCode::BAD_REQUEST,
                        "text/plain",
                        format!(
                        "not asked as the TV client: {client_name} {visitor} {as_tv} {sts:?} {id}"
                    ),
                    );
                }
                let answer = serde_json::json!({
                    "playabilityStatus": {"status": "OK"},
                    "videoDetails": {"videoId": id, "title": "Talk: a \"test\"", "lengthSeconds": "4"},
                    "captions": {"playerCaptionsTracklistRenderer": {
                        "captionTracks": [
                            {"languageCode": "en", "isTranslatable": true,
                             "baseUrl": format!("{origin}/api/timedtext?v={id}&lang=en&fmt=srv3")},
                            {"languageCode": "en", "kind": "asr",
                             "baseUrl": format!("{origin}/api/timedtext?v={id}&lang=en&kind=asr")}
                        ],
                        "translationLanguages": [{"languageCode": "fr", "languageName": {"simpleText": "French"}}]
                    }},
                    "streamingData": {"adaptiveFormats": [
                        {"itag": 137, "mimeType": "video/mp4; codecs=\"avc1.640028\"", "width": 1920, "height": 1080,
                         "bitrate": 4000000, "contentLength": picture_len.to_string(), "url": format!("{origin}/picture?itag=137&n=abc"),
                         "initRange": pic_init, "indexRange": pic_index},
                        {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "averageBitrate": 130000,
                         "contentLength": sound_len.to_string(),
                         "signatureCipher": format!("s=xyz&sp=sig&url={}", urlencoding(&format!("{origin}/sound?itag=140&n=def"))),
                         "initRange": snd_init, "indexRange": snd_index},
                        {"itag": 22, "mimeType": "video/mp4; codecs=\"avc1\"", "width": 1280, "height": 720,
                         "bitrate": 1, "drmFamilies": ["w"], "url": format!("{origin}/locked")}
                    ]}
                });
                text(StatusCode::OK, "application/json", answer.to_string())
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
                    r#"<html><script>ytcfg.set({"PLAYER_JS_URL":"/s/player/abcd1234/player_ias.vflset/en_US/base.js","STS":12345,"VISITOR_DATA":"visitor-1"});</script><script>var ytInitialPlayerResponse = {};</script></html>"#.into(),
                )
            }),
        )
        .route("/youtubei/v1/player", post(player_api))
        .route(
            "/s/player/abcd1234/player_ias.vflset/en_US/base.js",
            // A player in the real one's shape, cut and run by EDM as the real one is: its
            // address class, whose own method turns `n` into capitals, its function that builds
            // a stream's address and deciphers a signature into capitals, and a call made while
            // it loads that only a page could answer.
            get(|| async { text(StatusCode::OK, "text/javascript", PLAYER.into()) }),
        )
        .fallback(get({
            let (picture, sound) = (picture.clone(), sound.clone());
            move |State(seen): State<Seen>, uri: Uri, headers: HeaderMap| {
                let (picture, sound) = (picture.clone(), sound.clone());
                async move {
                    // Each request with the range it asked for.
                    let range = headers
                        .get("range")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("-")
                        .to_string();
                    seen.0.lock().unwrap().push(format!("{uri} {range}"));
                    let query = uri.query().unwrap_or("");
                    match uri.path() {
                        "/picture" if query.contains("n=ABC") => ranged(&picture, &headers),
                        "/sound" if query.contains("n=DEF") && query.contains("sig=XYZ") => {
                            ranged(&sound, &headers)
                        }
                        // The written English track, as WebVTT; no other.
                        "/api/timedtext"
                            if query.contains("lang=en")
                                && query.ends_with("fmt=vtt")
                                && !query.contains("kind=asr") =>
                        {
                            text(StatusCode::OK, "text/vtt", CAPTIONS.into())
                        }
                        _ => text(StatusCode::FORBIDDEN, "text/plain", format!("not solved: {uri}")),
                    }
                }
            }
        }))
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempdir();
    // The stand-in.
    std::env::set_var("EDM_YOUTUBE_HOST", &origin);

    let page = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
    let done = videopage::download_page(&client(), page, &options(dir.path()), None)
        .await
        .expect("the page did not download through EDM's own reader");

    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Talk_ a _test_ [jNQXAC9IVRw].mp4"
    );
    let joined = std::fs::read(&done.path).unwrap();
    let has = |kind: &[u8]| joined.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 2, "picture and sound are both in it");
    assert!(has(b"avc1") >= 1 && has(b"mp4a") >= 1);
    let seen = seen.0.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|u| u.starts_with("/picture?") && u.contains("n=ABC")),
        "the picture was not asked for with its solved n: {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|u| u.starts_with("/sound?") && u.contains("n=DEF") && u.contains("sig=XYZ")),
        "the sound was not asked for with its solved n and signature: {seen:?}"
    );
    assert!(
        !seen.iter().any(|u| u.starts_with("/locked")),
        "a locked stream was asked for: {seen:?}"
    );
    // Subtitles off (Options' default): no caption fetched, no text track.
    assert!(
        !seen.iter().any(|u| u.starts_with("/api/timedtext")),
        "captions were fetched with subtitles off: {seen:?}"
    );
    assert!(text_tracks(&joined).is_empty());
    // Nothing was fetched to answer the checks: no tool and no script.
    assert!(
        !dir.path().join("tools").exists(),
        "a tool was fetched where the computer has an engine of its own"
    );

    // A part of the same video, from 2 to 4 seconds. Each piece of the
    // picture plays a second, so its pieces 2 and 3 are asked for, by
    // their bytes alone, after its first boxes and its index; the sound's
    // pieces are 1.046 s each, so its 1 to 3. The file is named for the
    // span and cut to it.
    requests.0.lock().unwrap().clear();
    let mut part = options(dir.path());
    part.span = edm_core::Span::new(2.0, 4.0);
    let done = videopage::download_page(&client(), page, &part, None)
        .await
        .expect("the part did not download");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Talk_ a _test_ [jNQXAC9IVRw] (0m02s-0m04s).mp4"
    );
    let joined = std::fs::read(&done.path).unwrap();
    let has = |kind: &[u8]| joined.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 2, "picture and sound are both in the part");
    assert_eq!(has(b"elst"), 2, "each track is cut by its edit list");
    let (elst_at, _) = find_box(&joined, b"elst").unwrap();
    assert_eq!(joined[elst_at], 1, "a version 1 edit list");
    let duration = u64::from_be_bytes(joined[elst_at + 8..elst_at + 16].try_into().unwrap());
    assert_eq!(
        duration, 2000,
        "the picture plays the two seconds asked for"
    );
    let asked: Vec<String> = requests.0.lock().unwrap().clone();
    let ranges_of = |path: &str| -> Vec<(u64, u64)> {
        asked
            .iter()
            .filter(|u| u.starts_with(path))
            .filter_map(|u| {
                let (a, b) = u
                    .rsplit_once(' ')?
                    .1
                    .strip_prefix("bytes=")?
                    .split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            })
            .collect()
    };
    let picture_asked = ranges_of("/picture?");
    assert!(
        picture_asked.contains(&pic_init),
        "the picture's first boxes: {picture_asked:?}"
    );
    assert!(
        picture_asked.contains(&pic_index),
        "the picture's index: {picture_asked:?}"
    );
    let piece = (picture.len() as u64 - (pic_index.1 + 1)) / 6;
    let (from, to) = (pic_index.1 + 1 + 2 * piece, pic_index.1 + 1 + 4 * piece - 1);
    let mut fetched = 0;
    for (a, b) in picture_asked
        .iter()
        .filter(|r| **r != pic_init && **r != pic_index)
    {
        assert!(
            *a >= from && *b <= to,
            "bytes outside pieces 2 and 3 were asked for: {a}-{b}"
        );
        fetched += b - a + 1;
    }
    assert_eq!(fetched, to - from + 1, "pieces 2 and 3 whole, once each");
    let sound_asked = ranges_of("/sound?");
    let piece = (sound.len() as u64 - (snd_index.1 + 1)) / 6;
    let (from, to) = (snd_index.1 + 1 + piece, snd_index.1 + 1 + 4 * piece - 1);
    let mut fetched = 0;
    for (a, b) in sound_asked
        .iter()
        .filter(|r| **r != snd_init && **r != snd_index)
    {
        assert!(
            *a >= from && *b <= to,
            "bytes outside the sound's pieces 1 to 3 were asked for: {a}-{b}"
        );
        fetched += b - a + 1;
    }
    assert_eq!(
        fetched,
        to - from + 1,
        "the sound's pieces 1 to 3 whole, once each"
    );
    // The whole video from before is still there beside the part.
    assert!(dir
        .path()
        .join("out")
        .join("Talk_ a _test_ [jNQXAC9IVRw].mp4")
        .is_file());

    // With Options' subtitles on, the written track (not the speech-recognised one, nor a
    // translation) is fetched as WebVTT and the finished MP4 carries it as a text track, read
    // back here with its times and words.
    requests.0.lock().unwrap().clear();
    let with = tempdir();
    let mut subs = options(with.path());
    subs.subtitles = true;
    let done = videopage::download_page(&client(), page, &subs, None)
        .await
        .expect("the video with its subtitles did not download");
    let asked: Vec<String> = requests.0.lock().unwrap().clone();
    let captions: Vec<&String> = asked
        .iter()
        .filter(|u| u.starts_with("/api/timedtext"))
        .collect();
    assert_eq!(captions.len(), 1, "one caption track fetched: {asked:?}");
    assert!(
        captions[0].contains("lang=en") && captions[0].contains("fmt=vtt"),
        "{captions:?}"
    );
    assert!(!captions[0].contains("kind=asr") && !captions[0].contains("tlang"));
    let joined = std::fs::read(&done.path).unwrap();
    assert_eq!(
        text_tracks(&joined),
        [(
            "tx3g".to_string(),
            "eng".to_string(),
            vec![
                (500, 1500, "Hello there".to_string()),
                (2000, 3250, "A & B\nline two".to_string()),
            ]
        )]
    );
    // Picture and sound are still in it beside the text.
    let has = |kind: &[u8]| joined.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 3, "picture, sound and text");
    // The fetched file went inside; none is left beside the video.
    let left: Vec<String> = std::fs::read_dir(done.path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".vtt"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// The stand-in's English captions.
const CAPTIONS: &str = "WEBVTT\nKind: captions\nLanguage: en\n\n\
00:00:00.500 --> 00:00:01.500 align:start position:0%\nHello <i>there</i>\n\n\
00:00:02.000 --> 00:00:03.250\nA &amp; B\nline two\n";

/// The boxes directly inside `data`: each one's kind and body.
fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut at = 0;
    let mut out = Vec::new();
    while at + 8 <= data.len() {
        let size = be32(data, at) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().unwrap();
        let (head, size) = match size {
            0 => (8, data.len() - at),
            1 => (
                16,
                u64::from_be_bytes(data[at + 8..at + 16].try_into().unwrap()) as usize,
            ),
            _ => (8, size),
        };
        out.push((kind, &data[at + head..at + size]));
        at += size;
    }
    out
}

/// The body of the first box of `kind` directly inside `data`.
fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data)
        .into_iter()
        .find(|(k, _)| k == kind)
        .map(|(_, body)| body)
}

fn be32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(data[at..at + 4].try_into().unwrap())
}

/// A text track read back: its sample entry, its language and its cues.
type TextTrack = (String, String, Vec<(u64, u64, String)>);

/// An MP4's subtitle tracks, read back from its sample tables: each one's sample entry, its
/// language and its cues (start and end in milliseconds, and text); the empty samples between
/// cues left out.
fn text_tracks(mp4: &[u8]) -> Vec<TextTrack> {
    let moov = child(mp4, b"moov").expect("a finished MP4 has its moov");
    boxes(moov)
        .into_iter()
        .filter(|(kind, _)| kind == b"trak")
        .filter_map(|(_, trak)| {
            let mdia = child(trak, b"mdia")?;
            let hdlr = child(mdia, b"hdlr")?;
            if !matches!(&hdlr[8..12], b"sbtl" | b"text") {
                return None;
            }
            let mdhd = child(mdia, b"mdhd")?;
            let (timescale, language_at) = if mdhd[0] == 1 {
                (u64::from(be32(mdhd, 20)), 32)
            } else {
                (u64::from(be32(mdhd, 12)), 20)
            };
            let packed = u16::from_be_bytes([mdhd[language_at], mdhd[language_at + 1]]);
            let language: String = [10, 5, 0]
                .iter()
                .map(|shift| char::from(((packed >> shift) & 0x1f) as u8 + 0x60))
                .collect();
            let stbl = child(child(mdia, b"minf")?, b"stbl")?;
            let stsd = child(stbl, b"stsd")?;
            let entry = String::from_utf8_lossy(&stsd[12..16]).into_owned();
            let stts = child(stbl, b"stts")?;
            let durations: Vec<u64> = (0..be32(stts, 4) as usize)
                .flat_map(|i| {
                    let (count, each) = (be32(stts, 8 + 8 * i), be32(stts, 12 + 8 * i));
                    std::iter::repeat_n(u64::from(each), count as usize)
                })
                .collect();
            let stsz = child(stbl, b"stsz")?;
            let (fixed, count) = (be32(stsz, 4), be32(stsz, 8) as usize);
            let sizes: Vec<usize> = (0..count)
                .map(|i| if fixed != 0 { fixed } else { be32(stsz, 12 + 4 * i) } as usize)
                .collect();
            let chunks: Vec<usize> = match child(stbl, b"stco") {
                Some(stco) => (0..be32(stco, 4) as usize)
                    .map(|i| be32(stco, 8 + 4 * i) as usize)
                    .collect(),
                None => {
                    let co64 = child(stbl, b"co64")?;
                    (0..be32(co64, 4) as usize)
                        .map(|i| {
                            u64::from_be_bytes(co64[8 + 8 * i..16 + 8 * i].try_into().unwrap())
                                as usize
                        })
                        .collect()
                }
            };
            let stsc = child(stbl, b"stsc")?;
            let runs: Vec<(usize, usize)> = (0..be32(stsc, 4) as usize)
                .map(|i| {
                    (
                        be32(stsc, 8 + 12 * i) as usize,
                        be32(stsc, 12 + 12 * i) as usize,
                    )
                })
                .collect();
            let mut offsets = Vec::new();
            for (n, start) in chunks.iter().enumerate() {
                let per = runs
                    .iter()
                    .rev()
                    .find(|(first, _)| *first <= n + 1)
                    .map_or(0, |run| run.1);
                let mut at = *start;
                for _ in 0..per {
                    let Some(size) = sizes.get(offsets.len()) else {
                        break;
                    };
                    offsets.push(at);
                    at += size;
                }
            }
            assert_eq!(offsets.len(), sizes.len(), "every sample is in a chunk");
            let mut time = 0;
            let mut cues = Vec::new();
            for ((at, size), duration) in offsets.iter().zip(&sizes).zip(&durations) {
                let sample = &mp4[*at..at + size];
                let length = usize::from(u16::from_be_bytes([sample[0], sample[1]]));
                if length > 0 {
                    cues.push((
                        time * 1000 / timescale,
                        (time + duration) * 1000 / timescale,
                        String::from_utf8(sample[2..2 + length].to_vec()).unwrap(),
                    ));
                }
                time += duration;
            }
            Some((entry, language, cues))
        })
        .collect()
}

/// The test's player: the real one's shape, small.
const PLAYER: &str = r#"var _yt_player={};(function(g){var window=this;'use strict';var ca="set;get;n".split(";");
g.ch=function(a,b){this.url=a;this.p={}};
g.ch.prototype.set=function(a,b){this.p[a]=b};
g.ch.prototype.get=function(a){return this.p[a]};
g.ch.prototype.Zn=function(){var a=this.get(ca[2]);a&&this.set(ca[2],a.toUpperCase())};
g.ch.prototype.clone=function(){return new g.ch(this.url)};
Ws=function(a){return a.toUpperCase()};
om=function(a,b,c){b=b===void 0?"":b;c=c===void 0?"":c;a=new g.ch(a,!0);a.set("alr","yes");c&&(c=Ws(decodeURIComponent(c)),a[ca[0]](b,encodeURIComponent(c)));return a};
window.document.querySelector("video").play();
})(_yt_player);
"#;

/// `text` percent-encoded as a query value.
fn urlencoding(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}
