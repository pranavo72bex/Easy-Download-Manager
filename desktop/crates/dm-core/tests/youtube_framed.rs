// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube video whose formats carry neither an address nor a cipher, served only by the
//! framed stream its player's answer names (gaps row Y2), against a stand-in for YouTube: its
//! watch page, its player API and the framed stream are a local server's. The stream answers
//! each POST with a framed answer of the shape YouTube's has (UMP parts: a format's first word,
//! a media header, its media in two parts, its end, the policy for the next request, a
//! redirect), built here, a few pieces at a time; the picture's and the sound's bytes land in
//! their files and are joined. Where the stream wants a proof-of-origin token, or refuses, the
//! download fails in words.
//!
//! The parts' kinds and the messages' field numbers are those dm writes and reads
//! (`youtube::framed::messages`); the stand-in shows dm keeps to its own word, not that YouTube
//! does.

mod common;

#[path = "common/mp4_samples.rs"]
mod mp4_samples;

use std::sync::{Mutex, OnceLock};

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use base64::Engine as _;
use dm_core::youtube::framed::messages::kind;
use dm_core::youtube::framed::ump::put_part;
use dm_core::youtube::framed::wire::{Message, Writer};
use dm_core::{client, videopage, youtube, Control, Error, Quality};

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

/// A box of `kind` inside `data` (the boxes a piece nests searched): where its body starts.
fn find_box(data: &[u8], kind: &[u8; 4]) -> Option<usize> {
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        if size < 8 || at + size > data.len() {
            return None;
        }
        if &data[at + 4..at + 8] == kind {
            return Some(at + 8);
        }
        if matches!(&data[at + 4..at + 8], b"moof" | b"traf") {
            if let Some(found) = find_box(&data[at + 8..at + size], kind) {
                return Some(at + 8 + found);
            }
        }
        at += size;
    }
    None
}

/// Where a top-level box of `kind` starts, and its size.
fn top_box(data: &[u8], kind: &[u8; 4]) -> (usize, usize) {
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        if &data[at + 4..at + 8] == kind {
            return (at, size);
        }
        at += size;
    }
    panic!("no {kind:?} box");
}

/// A fixture as the framed stream serves it: its first boxes, then `pieces` copies of its one
/// piece, each numbered and timed after the one before. Returns them, and the media bytes each
/// piece carries.
fn pieces_of(name: &str, pieces: u32) -> (Vec<u8>, Vec<Vec<u8>>, usize) {
    let whole = fixture(name);
    let (moov_at, moov_size) = top_box(&whole, b"moov");
    let (sidx_at, sidx_size) = top_box(&whole, b"sidx");
    let (moof_at, _) = top_box(&whole, b"moof");
    let (mdat_at, mdat_size) = top_box(&whole, b"mdat");
    let sidx = &whole[sidx_at..sidx_at + sidx_size];
    let entries = if sidx[8] == 1 { 40 } else { 32 };
    let duration = u32::from_be_bytes(sidx[entries + 4..entries + 8].try_into().unwrap());
    let init = whole[..moov_at + moov_size].to_vec();
    let mut out = Vec::new();
    for n in 0..pieces {
        let mut piece = whole[moof_at..mdat_at + mdat_size].to_vec();
        let seq_at = find_box(&piece, b"mfhd").unwrap();
        piece[seq_at + 4..seq_at + 8].copy_from_slice(&(n + 1).to_be_bytes());
        let tfdt_at = find_box(&piece, b"tfdt").unwrap();
        let time = u64::from(n) * u64::from(duration);
        if piece[tfdt_at] == 1 {
            piece[tfdt_at + 4..tfdt_at + 12].copy_from_slice(&time.to_be_bytes());
        } else {
            piece[tfdt_at + 4..tfdt_at + 8].copy_from_slice(&(time as u32).to_be_bytes());
        }
        out.push(piece);
    }
    (init, out, mdat_size - 8)
}

const PICTURE_PIECES: u32 = 3;
const PIECE_MS: u64 = 2_000;
const CONFIG: &[u8] = b"the stand-in's ustreamer config";

/// The stand-in's two formats: each one's first boxes and pieces.
struct Media {
    picture: (Vec<u8>, Vec<Vec<u8>>),
    picture_bytes: usize,
    sound: (Vec<u8>, Vec<Vec<u8>>),
}

fn media() -> &'static Media {
    static MEDIA: OnceLock<Media> = OnceLock::new();
    MEDIA.get_or_init(|| {
        let (init, pieces, picture_bytes) = pieces_of("picture.f137.mp4", PICTURE_PIECES);
        let (sound_init, sound, _) = pieces_of("sound.f140.m4a", 1);
        Media {
            picture: (init, pieces),
            picture_bytes,
            sound: (sound_init, sound),
        }
    })
}

/// What each framed request said: its path and query, whether it carried the config and the
/// cookie, the formats it asked for, those it said had begun, what it said it held of each
/// (format, milliseconds, first and last piece) and where it said the player was.
#[derive(Debug, Clone)]
struct Asked {
    uri: String,
    config: bool,
    cookie: bool,
    audio: Option<u64>,
    video: Option<u64>,
    lmt: Option<u64>,
    started: Vec<u64>,
    held: Vec<(u64, u64, u64, u64)>,
    player_ms: Option<u64>,
}

static SEEN: Mutex<Vec<Asked>> = Mutex::new(Vec::new());

/// The control of a fetch the stream pauses, once, when it is asked for the picture's piece
/// after [`PAUSED_AFTER`] ("framedResumed").
static PAUSING: Mutex<Option<Control>> = Mutex::new(None);

/// The picture's pieces a paused fetch holds.
const PAUSED_AFTER: u64 = 2;

/// How long the stream holds back its answer to the request it paused at: longer than a fetch
/// takes to see a pause (`STOP_LOOK`, a quarter of a second).
const PAUSED_ANSWER_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

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
                let stream = {
                    let origin = origin.clone();
                    move |uri: Uri, body: Bytes| framed(origin.clone(), uri, body)
                };
                let app = axum::Router::new()
                    .route("/watch", get(watch_page))
                    .route("/youtubei/v1/player", post(api))
                    .route("/videoplayback", post(stream.clone()))
                    .route("/moved", post(stream))
                    .route("/plain", get(plain));
                tx.send(origin).unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        });
        let origin = rx.recv().unwrap();
        std::env::set_var("DM_YOUTUBE_HOST", &origin);
        origin
    })
}

fn answer(status: StatusCode, kind: &str, body: impl Into<Body>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", kind)
        .body(body.into())
        .unwrap()
}

/// The watch page: the player's timestamp and the visitor id; nothing needs solving.
async fn watch_page() -> Response {
    answer(
        StatusCode::OK,
        "text/html",
        r#"<html><script>ytcfg.set({"PLAYER_JS_URL":"/s/player/abcd1234/player_ias.vflset/en_US/base.js","STS":12345,"VISITOR_DATA":"visitor-1"});</script></html>"#,
    )
}

/// The player API's answer: a 1080p picture and a sound with neither address nor cipher, and
/// the framed stream that serves them. "framedMixed" and "framedFalls" also list a 720p picture
/// and the sound with addresses.
async fn player(origin: String, body: String) -> Response {
    let asked: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let id = asked["videoId"].as_str().unwrap_or("").to_string();
    let config = base64::engine::general_purpose::URL_SAFE.encode(CONFIG);
    let mut formats = vec![
        serde_json::json!({"itag": 137, "mimeType": "video/mp4; codecs=\"avc1.640028\"",
            "width": 1920, "height": 1080, "fps": 30, "bitrate": 4000000,
            "lastModified": "1700000000000137"}),
        serde_json::json!({"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
            "averageBitrate": 130000, "lastModified": "1700000000000140"}),
    ];
    if id == "framedMixed" || id == "framedFalls" {
        let picture = fixture("picture.f137.mp4").len();
        let sound = fixture("sound.f140.m4a").len();
        formats.push(
            serde_json::json!({"itag": 136, "mimeType": "video/mp4; codecs=\"avc1.4d401f\"",
            "width": 1280, "height": 720, "fps": 30, "bitrate": 2000000,
            "contentLength": picture.to_string(), "url": format!("{origin}/plain?id={id}&itag=136")}),
        );
        formats.push(
            serde_json::json!({"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
            "averageBitrate": 130000, "contentLength": sound.to_string(),
            "url": format!("{origin}/plain?id={id}&itag=140")}),
        );
    }
    let reply = serde_json::json!({
        "playabilityStatus": {"status": "OK"},
        "videoDetails": {"videoId": id, "title": "Framed", "lengthSeconds": "6"},
        "streamingData": {
            "adaptiveFormats": formats,
            "serverAbrStreamingUrl": format!("{origin}/videoplayback?id={id}&expire=9999999999"),
        },
        "playerConfig": {"mediaCommonConfig": {"mediaUstreamerRequestConfig": {
            "videoPlaybackUstreamerConfig": config,
        }}},
    });
    answer(StatusCode::OK, "application/json", reply.to_string())
}

/// A format with an address, whole or by the range asked.
async fn plain(uri: Uri, headers: HeaderMap) -> Response {
    let name = if uri.query().unwrap_or("").ends_with("itag=136") {
        "picture.f137.mp4"
    } else {
        "sound.f140.m4a"
    };
    let data = fixture(name);
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
        .body(Body::from(data[a..=b].to_vec()))
        .unwrap()
}

/// What the request holds of format `itag`: the last piece's number.
fn held_of(request: Message<'_>, itag: u64) -> Option<u64> {
    request
        .all_bytes(3)
        .map(Message)
        .filter(|range| range.message(1).and_then(|f| f.uint(1)) == Some(itag))
        .find_map(|range| range.uint(5))
}

/// A piece of format `itag` written as the stream writes one: its header, its media in two
/// parts, its end.
fn put_piece(out: &mut Vec<u8>, id: u64, itag: u64, sequence: Option<u64>, bytes: &[u8]) {
    let mut header = Writer::new();
    header.uint(1, id).uint(3, itag);
    match sequence {
        Some(n) => {
            header
                .uint(9, n)
                .uint(11, (n - 1) * PIECE_MS)
                .uint(12, PIECE_MS);
        }
        None => {
            header.flag(8, true);
        }
    }
    put_part(out, kind::MEDIA_HEADER, header.as_bytes());
    let half = bytes.len() / 2;
    for part in [&bytes[..half], &bytes[half..]] {
        let mut media = vec![id as u8];
        media.extend_from_slice(part);
        put_part(out, kind::MEDIA, &media);
    }
    put_part(out, kind::MEDIA_END, &[id as u8]);
}

/// The framed stream: the first request sent on elsewhere; then, for each format, its first
/// word and its first boxes where it holds nothing, and the one piece after what it holds.
async fn framed(origin: String, uri: Uri, body: Bytes) -> Response {
    let request = Message(&body);
    let context = request.message(19);
    SEEN.lock().unwrap().push(Asked {
        uri: uri.to_string(),
        config: request.bytes(5) == Some(CONFIG),
        cookie: context.and_then(|c| c.bytes(3)) == Some(&b"stand-in cookie"[..]),
        audio: request.message(16).and_then(|f| f.uint(1)),
        video: request.message(17).and_then(|f| f.uint(1)),
        lmt: request.message(17).and_then(|f| f.uint(2)),
        started: request
            .all_bytes(2)
            .filter_map(|f| Message(f).uint(1))
            .collect(),
        held: request
            .all_bytes(3)
            .map(Message)
            .filter_map(|r| {
                let itag = r.message(1)?.uint(1)?;
                Some((itag, r.uint(3)?, r.uint(4)?, r.uint(5)?))
            })
            .collect(),
        player_ms: request.uint(4),
    });
    let query = uri.query().unwrap_or("");
    // A fetch paused part-way: the pause, then the answer held back until it is seen.
    if query.contains("id=framedResumed") && held_of(request, 137) == Some(PAUSED_AFTER) {
        let pausing = PAUSING.lock().unwrap().take();
        if let Some(control) = pausing {
            control.pause();
            tokio::time::sleep(PAUSED_ANSWER_WAIT).await;
        }
    }
    let mut out = Vec::new();
    if query.contains("id=framedToken") {
        let mut status = Writer::new();
        status.uint(1, 3);
        put_part(&mut out, kind::STREAM_PROTECTION_STATUS, status.as_bytes());
        return answer(StatusCode::OK, "application/vnd.yt-ump", out);
    }
    if query.contains("id=framedError") || query.contains("id=framedFalls") {
        let mut error = Writer::new();
        error.text(1, "sabr.config_expired").uint(2, 2);
        put_part(&mut out, kind::ERROR, error.as_bytes());
        return answer(StatusCode::OK, "application/vnd.yt-ump", out);
    }
    let mut policy = Writer::new();
    policy.uint(4, 5).bytes(7, b"stand-in cookie");
    put_part(&mut out, kind::NEXT_REQUEST_POLICY, policy.as_bytes());
    if uri.path() == "/videoplayback" {
        let mut to = Writer::new();
        to.text(1, &format!("{origin}/moved?{}", query.replace("&rn=1", "")));
        put_part(&mut out, kind::REDIRECT, to.as_bytes());
        return answer(StatusCode::OK, "application/vnd.yt-ump", out);
    }
    let m = media();
    let mut id = 0;
    for (itag, (init, pieces)) in [(137u64, &m.picture), (140, &m.sound)] {
        let held = held_of(request, itag);
        if held.is_none() {
            let mut start = Writer::new();
            start
                .message(2, |f| {
                    f.uint(1, itag);
                })
                .uint(3, pieces.len() as u64 * PIECE_MS)
                .uint(4, pieces.len() as u64);
            put_part(
                &mut out,
                kind::FORMAT_INITIALIZATION_METADATA,
                start.as_bytes(),
            );
            put_piece(&mut out, id, itag, None, init);
            id += 1;
        }
        let next = held.unwrap_or(0) + 1;
        if let Some(piece) = pieces.get(next as usize - 1) {
            put_piece(&mut out, id, itag, Some(next), piece);
            id += 1;
        }
    }
    answer(StatusCode::OK, "application/vnd.yt-ump", out)
}

fn page(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

fn seen_for(id: &str) -> Vec<Asked> {
    SEEN.lock()
        .unwrap()
        .iter()
        .filter(|a| a.uri.contains(&format!("id={id}&")))
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_framed_stream_s_picture_and_sound_land_and_join() {
    stand_in();
    let id = "framedVideo";
    let dir = tempdir();
    let done = videopage::download_page(&client(), &page(id), &options(dir.path()), None)
        .await
        .expect("the video served by the framed stream did not download");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Framed [framedVideo].mp4"
    );
    // The picture's three pieces and the sound's one, joined: AVC and AAC, in order.
    let joined = std::fs::read(&done.path).unwrap();
    let all = tracks(&joined);
    assert_eq!(all.len(), 2, "picture and sound are both in it");
    let picture = all.iter().find(|t| &t.handler == b"vide").unwrap();
    let sound = all.iter().find(|t| &t.handler == b"soun").unwrap();
    assert_eq!(&picture.codec, b"avc1");
    assert_eq!(&sound.codec, b"mp4a");
    assert!(picture.samples.windows(2).all(|w| w[0].dts < w[1].dts));
    let bytes: usize = picture.samples.iter().map(|s| s.data.len()).sum();
    assert_eq!(bytes, PICTURE_PIECES as usize * media().picture_bytes);
    assert!(!sound.samples.is_empty());
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");

    // The first request was sent on, and every one after it went there, numbered, with the
    // config and the formats; each after the first answer with the cookie it gave.
    let seen = seen_for(id);
    assert!(seen[0].uri.starts_with("/videoplayback?") && seen[0].uri.ends_with("&rn=1"));
    assert!(seen.len() >= 1 + PICTURE_PIECES as usize, "{seen:?}");
    for (n, asked) in seen.iter().enumerate() {
        assert!(asked.uri.ends_with(&format!("&rn={}", n + 1)), "{seen:?}");
        assert!(asked.config, "request {n} did not carry the config");
        assert_eq!((asked.audio, asked.video), (Some(140), Some(137)));
        assert_eq!(asked.lmt, Some(1_700_000_000_000_137));
        assert_eq!(asked.cookie, n > 0, "request {n}'s cookie");
        assert_eq!(asked.uri.starts_with("/moved?"), n > 0);
    }
}

/// Where a format with an address offers the quality asked for, it is taken; the framed
/// stream only for what nothing else offers.
#[tokio::test(flavor = "multi_thread")]
async fn a_format_with_an_address_is_taken_before_the_framed_stream() {
    stand_in();
    let staging = tempdir();
    let address = page("framedMixed");
    let reading = |text: String| serde_json::from_str::<serde_json::Value>(&text).unwrap();
    let at_720 = youtube::read(
        &client(),
        &address,
        Quality::UpTo(720),
        None,
        false,
        None,
        staging.path(),
    )
    .await
    .unwrap();
    let at_720 = reading(at_720);
    let pair = at_720["requested_formats"].as_array().unwrap();
    assert_eq!(pair[0]["format_id"], "136");
    assert_eq!(pair[0]["protocol"], "https");
    assert_eq!(pair[1]["protocol"], "https");
    assert!(at_720.get("dm_framed").is_none());
    // At 1080, only the framed stream has it: both formats from there.
    let best = youtube::read(
        &client(),
        &address,
        Quality::Best,
        None,
        false,
        None,
        staging.path(),
    )
    .await
    .unwrap();
    let best = reading(best);
    let pair = best["requested_formats"].as_array().unwrap();
    assert_eq!(pair[0]["format_id"], "137");
    assert!(pair
        .iter()
        .all(|f| f["protocol"] == "dm_framed" && f["url"].is_null()));
    assert_eq!(pair[0]["dm_framed"]["itag"], 137);
    assert_eq!(pair[0]["dm_framed"]["lmt"], 1_700_000_000_000_137u64);
    assert!(best["dm_framed"]["url"]
        .as_str()
        .unwrap()
        .contains("/videoplayback?id=framedMixed"));
    assert_eq!(best["dm_framed"]["client_name"], 7);
}

/// A stream that wants a proof-of-origin token, or refuses, fails the download in words.
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_wants_a_token_or_refuses_is_said_in_words() {
    stand_in();
    for (id, said) in [
        (
            "framedToken",
            "proof-of-origin token, which dm does not make",
        ),
        (
            "framedError",
            "YouTube's framed stream refused: sabr.config_expired (2)",
        ),
    ] {
        let dir = tempdir();
        let why = videopage::download_page(&client(), &page(id), &options(dir.path()), None)
            .await
            .expect_err(id)
            .to_string();
        assert!(why.contains(said), "{id}: {why}");
        // Asked once: neither is asked again.
        assert_eq!(seen_for(id).len(), 1, "{id}");
    }
}

/// The framed stream fails where the video also has formats with an address: those are fetched,
/// as before dm read the framed stream.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_framed_stream_gives_way_to_the_formats_with_an_address() {
    stand_in();
    let id = "framedFalls";
    let dir = tempdir();
    let done = videopage::download_page(&client(), &page(id), &options(dir.path()), None)
        .await
        .expect("the video did not fall back to its formats with an address");
    let joined = std::fs::read(&done.path).unwrap();
    let all = tracks(&joined);
    assert_eq!(all.len(), 2, "picture and sound are both in it");
    let picture = all.iter().find(|t| &t.handler == b"vide").unwrap();
    let bytes: usize = picture.samples.iter().map(|s| s.data.len()).sum();
    assert_eq!(bytes, media().picture_bytes, "the 720p picture, whole");
    assert_eq!(seen_for(id).len(), 1, "the framed stream asked once");
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");
}

/// Every file under `dir` whose name ends with `suffix` and holds `format` (`f137`), with its
/// length.
fn staged(dir: &std::path::Path, format: &str, suffix: &str) -> Vec<u64> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            found.extend(staged(&path, format, suffix));
        } else if name.ends_with(suffix) && name.contains(&format!(".{format}.")) {
            found.push(entry.metadata().unwrap().len());
        }
    }
    found
}

/// A track's samples as they can be compared: bytes, times, keyframes.
fn samples_of(joined: &[u8], handler: &[u8; 4]) -> Vec<(Vec<u8>, u64, i64, u32, bool)> {
    let all = tracks(joined);
    let track = all.iter().find(|t| &t.handler == handler).unwrap();
    track
        .samples
        .iter()
        .map(|s| (s.data.clone(), s.dts, s.offset, s.duration, s.sync))
        .collect()
}

/// A fetch paused after some pieces goes on from them: each format's file is kept beside its
/// record, the next run's first request says what is held of each and where the player is, the
/// stream sends only the pieces after, and the video is the one a fetch never paused makes.
#[tokio::test(flavor = "multi_thread")]
async fn a_paused_framed_fetch_goes_on_from_its_last_whole_piece() {
    stand_in();
    let id = "framedResumed";
    let dir = tempdir();
    let mut opts = options(dir.path());
    *PAUSING.lock().unwrap() = Some(opts.control.clone());
    let why = videopage::download_page(&client(), &page(id), &opts, None)
        .await
        .expect_err("the paused fetch ran to its end");
    assert!(matches!(why, Error::Paused), "{why:?}");

    // Kept: the picture's first boxes and two pieces, the sound's whole, each with its record.
    let m = media();
    let parts = dir.path().join("parts");
    let picture = m.picture.0.len() + m.picture.1[..PAUSED_AFTER as usize].concat().len();
    let sound = m.sound.0.len() + m.sound.1.concat().len();
    assert_eq!(staged(&parts, "f137", ".framed"), vec![picture as u64]);
    assert_eq!(staged(&parts, "f140", ".framed"), vec![sound as u64]);
    assert_eq!(staged(&parts, "f137", ".framed.held").len(), 1);
    assert_eq!(staged(&parts, "f140", ".framed.held").len(), 1);

    let before = seen_for(id).len();
    opts.control = Control::new();
    let done = videopage::download_page(&client(), &page(id), &opts, None)
        .await
        .expect("the paused fetch did not go on");
    let again = seen_for(id)[before..].to_vec();
    // Asked from what it held: four seconds of the picture, its pieces 1 to 2; the sound's
    // one piece; the player where the picture's end is. Only the picture's third was left:
    // the request sent on, then one.
    let first = &again[0];
    let held_ms = PAUSED_AFTER * PIECE_MS;
    assert!(
        first.held.contains(&(137, held_ms, 1, PAUSED_AFTER)),
        "{again:?}"
    );
    assert!(first.held.contains(&(140, PIECE_MS, 1, 1)), "{again:?}");
    assert_eq!(first.player_ms, Some(held_ms), "{again:?}");
    let mut started = first.started.clone();
    started.sort_unstable();
    assert_eq!(started, vec![137, 140], "{again:?}");
    assert_eq!(again.len(), 2, "{again:?}");
    assert!(!parts.exists(), "the staging is cleared");

    // The same picture and sound as a fetch never paused.
    let whole_dir = tempdir();
    let whole = videopage::download_page(
        &client(),
        &page("framedWhole"),
        &options(whole_dir.path()),
        None,
    )
    .await
    .expect("the fetch never paused did not download");
    let (resumed, whole) = (
        std::fs::read(&done.path).unwrap(),
        std::fs::read(&whole.path).unwrap(),
    );
    for handler in [b"vide", b"soun"] {
        assert!(
            samples_of(&resumed, handler) == samples_of(&whole, handler),
            "the {handler:?} track differs from a fetch never paused"
        );
    }
    let bytes: usize = samples_of(&resumed, b"vide")
        .iter()
        .map(|s| s.0.len())
        .sum();
    assert_eq!(bytes, PICTURE_PIECES as usize * m.picture_bytes);
}
