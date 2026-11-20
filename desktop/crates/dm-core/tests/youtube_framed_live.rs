// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube live broadcast whose player names no HLS playlist, recorded through its framed
//! stream (gaps row Y2), against a stand-in for YouTube: its watch page, its player API and the
//! framed stream are a local server's. The stand-in's broadcast moves its head one piece on
//! with each request; each answer says where the head is (`LiveMetadata`) before its pieces,
//! then, for each format, the piece after the last one the request holds, or, where it holds
//! none, the one at the player's time, never one past the head. Its player calls the video live
//! until the head is at its last piece, and a video after.
//!
//! dm starts at the head, not at the stream's start; goes on piece by piece as the head moves;
//! where an answer is cut short in a piece, asks again from its last whole piece; tells the row
//! of pieces the stream moved past before it had them; keeps what it recorded when stopped; and
//! ends once the broadcast has stood still and its player no longer calls it live, its picture
//! and sound joined.
//!
//! The parts' kinds and the messages' field numbers are those dm writes and reads
//! (`youtube::framed::messages`); the stand-in shows dm keeps to its own word, not that YouTube
//! does.

mod common;

#[path = "common/mp4_samples.rs"]
mod mp4_samples;

#[path = "common/framed_media.rs"]
mod framed_media;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};

use axum::body::{Body, Bytes};
use axum::http::{StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use base64::Engine as _;
use dm_core::youtube::framed::messages::kind;
use dm_core::youtube::framed::ump::put_part;
use dm_core::youtube::framed::wire::{Message, Writer};
use dm_core::{client, videopage, Control, Update};

use common::{options, tempdir};
use framed_media::Split;
use mp4_samples::tracks;

/// How long each piece plays, as the stream's headers say.
const PIECE_MS: u64 = 250;
/// The broadcast's head when it is first asked for: the pieces before it are its past.
const FIRST_HEAD: u64 = 8;
/// The last piece of a broadcast that ends: its head stops there, and its player says so.
const LAST: u64 = 14;
/// [`DROPPED`]'s: the picture's last piece held when one answer is cut short.
const DROP_AFTER: u64 = 10;
/// [`GAPPED`]'s: the last piece held when the stream moves [`GONE`] pieces past it before dm
/// has them.
const GAP_AFTER: u64 = 10;
const GONE: u64 = 2;
/// [`STOPPED`]'s: the picture's last piece held when the stream stops the recording, as a
/// person would; its broadcast does not end while the test runs.
const STOP_AFTER: u64 = FIRST_HEAD + 2;
const ENDLESS: u64 = 1_000_000;
const CONFIG: &[u8] = b"the stand-in's ustreamer config";

/// A broadcast one of whose answers is cut short.
const DROPPED: &str = "liveDropped";
/// A broadcast that moves past pieces before dm has them.
const GAPPED: &str = "liveGapNote";
/// A broadcast whose recording is stopped.
const STOPPED: &str = "liveStopped";

/// The stand-in's picture and sound.
struct Media {
    picture: Split,
    sound: Split,
}

fn media() -> &'static Media {
    static MEDIA: OnceLock<Media> = OnceLock::new();
    MEDIA.get_or_init(|| Media {
        picture: Split::of("picture.f137.mp4"),
        sound: Split::of("sound.f140.m4a"),
    })
}

/// One broadcast as the stand-in plays it.
#[derive(Default)]
struct Broadcast {
    /// Framed requests answered so far.
    asks: u64,
    /// Whether its one answer cut short has come.
    dropped: bool,
    /// Whether its one gap has come.
    gapped: bool,
}

static BROADCASTS: LazyLock<Mutex<HashMap<String, Broadcast>>> = LazyLock::new(Default::default);

/// The last piece of broadcast `id`.
fn last_of(id: &str) -> u64 {
    if id == STOPPED {
        ENDLESS
    } else {
        LAST
    }
}

/// Where broadcast `id`'s head is, as it stands.
fn head_of(id: &str, broadcast: &Broadcast) -> u64 {
    let gone = if broadcast.gapped { GONE } else { 0 };
    (FIRST_HEAD + broadcast.asks + gone).min(last_of(id))
}

/// What each framed request said: the broadcast it asked about, what it held of each format
/// (format, milliseconds, first and last piece) and where it said the player was.
#[derive(Debug, Clone)]
struct Asked {
    id: String,
    held: Vec<(u64, u64, u64, u64)>,
    player_ms: Option<u64>,
}

impl Asked {
    /// The first piece of format `itag` it held.
    fn first_of(&self, itag: u64) -> Option<u64> {
        self.held.iter().find(|h| h.0 == itag).map(|h| h.2)
    }

    /// The last piece of format `itag` it held.
    fn last_of(&self, itag: u64) -> Option<u64> {
        self.held.iter().find(|h| h.0 == itag).map(|h| h.3)
    }
}

static SEEN: Mutex<Vec<Asked>> = Mutex::new(Vec::new());

/// The control of [`STOPPED`]'s recording, which its stream pauses once.
static PAUSING: Mutex<Option<Control>> = Mutex::new(None);

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
                    .route("/videoplayback", post(framed));
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

/// The player API's answer: a 1080p picture and a sound with neither address nor cipher, the
/// framed stream that serves them, and no HLS playlist. Live, its formats marked as a live
/// one's are, until the broadcast's head is at its last piece; a video after.
async fn player(origin: String, body: String) -> Response {
    let asked: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let id = asked["videoId"].as_str().unwrap_or("").to_string();
    let live = {
        let mut all = BROADCASTS.lock().unwrap();
        let broadcast = all.entry(id.clone()).or_default();
        head_of(&id, broadcast) < last_of(&id)
    };
    let mut formats = vec![
        serde_json::json!({"itag": 137, "mimeType": "video/mp4; codecs=\"avc1.640028\"",
            "width": 1920, "height": 1080, "fps": 30, "bitrate": 4000000,
            "lastModified": "1700000000000137"}),
        serde_json::json!({"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
            "averageBitrate": 130000, "lastModified": "1700000000000140"}),
    ];
    if live {
        for format in &mut formats {
            format["targetDurationSec"] = serde_json::json!(1);
        }
    }
    let config = base64::engine::general_purpose::URL_SAFE.encode(CONFIG);
    let reply = serde_json::json!({
        "playabilityStatus": {"status": "OK"},
        "videoDetails": {"videoId": id, "title": "Live", "lengthSeconds": "0",
            "isLive": live, "isLiveContent": true},
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

/// What the request holds of each format: format, milliseconds, first and last piece.
fn held_ranges(request: Message<'_>) -> Vec<(u64, u64, u64, u64)> {
    request
        .all_bytes(3)
        .map(Message)
        .filter_map(|range| {
            let itag = range.message(1)?.uint(1)?;
            Some((itag, range.uint(3)?, range.uint(4)?, range.uint(5)?))
        })
        .collect()
}

/// A piece of format `itag` written as the stream writes one: its header (its number and time
/// where it is media), its media in two parts, its end.
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

/// An answer whose connection drops after `sent`: a body that fails part-way.
fn cut_short(sent: Vec<u8>) -> Response {
    let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
        Ok(Bytes::from(sent)),
        Err(std::io::Error::other("the stand-in's connection drops")),
    ];
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/vnd.yt-ump")
        .body(Body::from_stream(futures_util::stream::iter(chunks)))
        .unwrap()
}

/// The framed stream of a live broadcast: its head moved on one piece with each request; then
/// the policy, the head, and for each format its first word and first boxes where the request
/// says it has not begun, and the piece after the last it holds (where it holds none, the one
/// at the player's time), where that is not past the head.
async fn framed(uri: Uri, body: Bytes) -> Response {
    let request = Message(&body);
    let id = uri
        .query()
        .unwrap_or("")
        .split('&')
        .find_map(|pair| pair.strip_prefix("id="))
        .unwrap_or("")
        .to_string();
    let held = held_ranges(request);
    let last = |itag: u64| held.iter().find(|h| h.0 == itag).map(|h| h.3);
    SEEN.lock().unwrap().push(Asked {
        id: id.clone(),
        held: held.clone(),
        player_ms: request.uint(4),
    });
    // The broadcast as it stands at this request: an answer cut short, pieces moved past.
    let (head, cut, jump) = {
        let mut all = BROADCASTS.lock().unwrap();
        let broadcast = all.entry(id.clone()).or_default();
        let cut = id == DROPPED && last(137) == Some(DROP_AFTER) && !broadcast.dropped;
        broadcast.dropped |= cut;
        let jump = id == GAPPED && last(137) == Some(GAP_AFTER) && !broadcast.gapped;
        broadcast.gapped |= jump;
        let head = head_of(&id, broadcast);
        broadcast.asks += 1;
        (head, cut, jump)
    };
    if id == STOPPED && last(137) == Some(STOP_AFTER) {
        let pausing = PAUSING.lock().unwrap().take();
        if let Some(control) = pausing {
            control.pause();
        }
    }
    let mut out = Vec::new();
    let mut policy = Writer::new();
    policy.uint(4, 5).bytes(7, b"stand-in cookie");
    put_part(&mut out, kind::NEXT_REQUEST_POLICY, policy.as_bytes());
    let mut live = Writer::new();
    live.text(1, "broadcast")
        .uint(3, head)
        .uint(4, (head - 1) * PIECE_MS);
    put_part(&mut out, kind::LIVE_METADATA, live.as_bytes());
    let started: Vec<u64> = request
        .all_bytes(2)
        .filter_map(|f| Message(f).uint(1))
        .collect();
    let player_ms = request.uint(4).unwrap_or(0);
    let m = media();
    let mut header_id = 0;
    let mut cut_at = None;
    for (itag, split) in [(137u64, &m.picture), (140, &m.sound)] {
        if !started.contains(&itag) {
            let mut start = Writer::new();
            start.message(2, |f| {
                f.uint(1, itag);
            });
            put_part(
                &mut out,
                kind::FORMAT_INITIALIZATION_METADATA,
                start.as_bytes(),
            );
            put_piece(&mut out, header_id, itag, None, &split.init);
            header_id += 1;
        }
        let next = match last(itag) {
            Some(held) if jump => held + 1 + GONE,
            Some(held) => held + 1,
            None => player_ms / PIECE_MS + 1,
        };
        if next <= head {
            let at = out.len();
            put_piece(
                &mut out,
                header_id,
                itag,
                Some(next),
                &split.piece(next as u32),
            );
            header_id += 1;
            if cut && itag == 137 {
                cut_at = Some(at + (out.len() - at) / 2);
            }
        }
    }
    match cut_at {
        Some(cut) => cut_short(out[..cut].to_vec()),
        None => answer(StatusCode::OK, "application/vnd.yt-ump", out),
    }
}

fn page(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

fn seen_for(id: &str) -> Vec<Asked> {
    SEEN.lock()
        .unwrap()
        .iter()
        .filter(|a| a.id == id)
        .cloned()
        .collect()
}

/// A row to report to, and the notes said on it, gathered until the download lets it go.
fn row() -> (
    tokio::sync::mpsc::Sender<Update>,
    tokio::task::JoinHandle<Vec<String>>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let notes = tokio::spawn(async move {
        let mut notes = Vec::new();
        while let Some(update) = rx.recv().await {
            if let Update::Note(said) = update {
                notes.push(said);
            }
        }
        notes
    });
    (tx, notes)
}

/// The recording at `path` read back: the picture's media bytes, decoded in order, and the
/// sound's samples.
fn recorded(path: &std::path::Path) -> (usize, usize) {
    let joined = std::fs::read(path).unwrap();
    let all = tracks(&joined);
    assert_eq!(all.len(), 2, "picture and sound are both in it");
    let picture = all.iter().find(|t| &t.handler == b"vide").unwrap();
    let sound = all.iter().find(|t| &t.handler == b"soun").unwrap();
    assert_eq!(&picture.codec, b"avc1");
    assert_eq!(&sound.codec, b"mp4a");
    assert!(picture.samples.windows(2).all(|w| w[0].dts < w[1].dts));
    let bytes: usize = picture.samples.iter().map(|s| s.data.len()).sum();
    (bytes, sound.samples.len())
}

/// The words of the note of pieces gone, after its number.
fn gone_words() -> &'static str {
    dm_core::LIVE_PIECES_GONE.split("{}").nth(1).unwrap()
}

/// Recorded from the broadcast's head, piece by piece as the head moves on, through an answer
/// cut short in a piece (asked again from the last whole piece), to its end: once it has stood
/// still and its player no longer calls it live, the picture and sound are joined, every piece
/// from the head once and in order.
#[tokio::test(flavor = "multi_thread")]
async fn a_live_broadcast_is_recorded_from_its_head_through_a_drop_to_its_end() {
    stand_in();
    let dir = tempdir();
    let (tx, notes) = row();
    let done = videopage::download_page(&client(), &page(DROPPED), &options(dir.path()), Some(tx))
        .await
        .expect("the live broadcast was not recorded");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        format!("Live [{DROPPED}].mp4")
    );
    let (picture, sound) = recorded(&done.path);
    let pieces = (LAST - FIRST_HEAD + 1) as usize;
    assert_eq!(picture, pieces * media().picture.media_bytes);
    assert!(sound > 0);
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");

    let seen = seen_for(DROPPED);
    // The first request knows no head and holds nothing; the next asks for the head's piece.
    assert_eq!(seen[0].player_ms, Some(0), "{seen:?}");
    assert!(seen[0].held.is_empty(), "{seen:?}");
    assert_eq!(
        seen[1].player_ms,
        Some((FIRST_HEAD - 1) * PIECE_MS),
        "{seen:?}"
    );
    // Recorded from the head's piece, not from the stream's start ...
    let first = seen.iter().find_map(|a| a.first_of(137));
    assert_eq!(first, Some(FIRST_HEAD), "{seen:?}");
    // ... each request after holding one piece more as the head moves on ...
    let lasts: Vec<u64> = seen.iter().filter_map(|a| a.last_of(137)).collect();
    let mut walked = lasts.clone();
    walked.dedup();
    assert_eq!(walked, (FIRST_HEAD..=LAST).collect::<Vec<_>>(), "{seen:?}");
    // ... but the one cut short, asked again holding what it held: its piece came once.
    let again = lasts.iter().filter(|l| **l == DROP_AFTER).count();
    assert_eq!(again, 2, "{seen:?}");
    // Nothing was gone, so the row was told of nothing.
    let notes = notes.await.unwrap();
    assert!(
        !notes.iter().any(|n| n.ends_with(gone_words())),
        "{notes:?}"
    );
}

/// Pieces the stream moved past before dm had them leave a gap, which the row is told of, as a
/// live HLS recording's; the recording goes on after it.
#[tokio::test(flavor = "multi_thread")]
async fn pieces_gone_before_dm_had_them_are_told_on_the_row() {
    stand_in();
    let dir = tempdir();
    let (tx, notes) = row();
    let done = videopage::download_page(&client(), &page(GAPPED), &options(dir.path()), Some(tx))
        .await
        .expect("the live broadcast with a gap was not recorded");
    let (picture, _) = recorded(&done.path);
    let pieces = (LAST - FIRST_HEAD + 1 - GONE) as usize;
    assert_eq!(picture, pieces * media().picture.media_bytes);
    let mut walked: Vec<u64> = seen_for(GAPPED)
        .iter()
        .filter_map(|a| a.last_of(137))
        .collect();
    walked.dedup();
    let expected: Vec<u64> = (FIRST_HEAD..=GAP_AFTER)
        .chain(GAP_AFTER + 1 + GONE..=LAST)
        .collect();
    assert_eq!(walked, expected);
    let notes = notes.await.unwrap();
    let said = dm_core::LIVE_PIECES_GONE.replace("{}", &GONE.to_string());
    assert!(notes.contains(&said), "{notes:?}");
}

/// A recording stopped as a person stops one keeps what it holds, joined and placed: whole
/// pieces only, from the head to where it was stopped.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_live_recording_keeps_what_it_holds() {
    stand_in();
    let dir = tempdir();
    let opts = options(dir.path());
    *PAUSING.lock().unwrap() = Some(opts.control.clone());
    let done = videopage::download_page(&client(), &page(STOPPED), &opts, None)
        .await
        .expect("the stopped recording was not kept");
    let (picture, sound) = recorded(&done.path);
    let piece = media().picture.media_bytes;
    assert_eq!(picture % piece, 0, "whole pieces only");
    // Those up to where it was stopped, and the one under way at most.
    let held = (STOP_AFTER - FIRST_HEAD + 1) as usize;
    let got = picture / piece;
    assert!((held..=held + 1).contains(&got), "{got} pieces");
    assert!(sound > 0);
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");
}
