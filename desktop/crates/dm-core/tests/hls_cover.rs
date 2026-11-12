// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The HLS paths `tests/hls.rs` leaves out: shapes dm hands on rather than
//! assembles, a part of a stream, a resume that finds the stream already
//! gathered or remuxed, servers that cut a byte range short or refuse one,
//! and a live recording that is scrambled, starts with an initialisation
//! segment, loses its playlist for a while, or is stopped before it has
//! anything. Every server is on loopback, and its playlists are written
//! here.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use cbc::cipher::{block_padding::Pkcs7, BlockModeEncrypt, InnerIvInit, KeyInit};
use dm_core::{client, download, download_reporting, Error, Options, Span, Update};

mod common;
use common::tempdir;

const KEY: [u8; 16] = [
    0x0f, 0x1e, 0x2d, 0x3c, 0x4b, 0x5a, 0x69, 0x78, 0x87, 0x96, 0xa5, 0xb4, 0xc3, 0xd2, 0xe1, 0xf0,
];

/// What one segment carries, made distinct so an out-of-order join shows.
fn segment(index: usize) -> Vec<u8> {
    (0..16 * 1024)
        .map(|i| ((index * 37 + i) % 253) as u8)
        .collect()
}

fn segments(range: std::ops::Range<usize>) -> Vec<u8> {
    range.flat_map(segment).collect()
}

/// Segment `index` scrambled with AES-128 under the playlist's sequence
/// number as its IV, as a packager does when it names no IV.
fn sealed(index: usize) -> Vec<u8> {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&(index as u64).to_be_bytes());
    let cipher = aes::Aes128::new_from_slice(&KEY).unwrap();
    cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(cipher, &iv)
        .unwrap()
        .encrypt_padded_vec::<Pkcs7>(&segment(index))
}

fn options(dir: &std::path::Path) -> Options {
    Options {
        // One try each: a failure asked for is that failure, at once.
        attempts: 1,
        ..common::options(dir)
    }
}

/// A live stream as a test wants it: a window of its latest three
/// segments, one more out at each ask, ending once all are out.
#[derive(Clone, Default)]
struct Live {
    segments: usize,
    /// An initialisation segment leads the stream.
    map: bool,
    /// Every segment is scrambled with AES-128.
    key: bool,
    /// Asks (the probe's not counted) answered with a server error.
    failing: Vec<usize>,
    /// Every ask from this one on is answered with a server error.
    fails_from: Option<usize>,
    /// An ask held this long before it is answered.
    slow: Option<(usize, Duration)>,
    /// A segment that is never served.
    missing: Option<usize>,
}

/// A site: fixed files by path, a live playlist at `/live.m3u8`, and every
/// request it was sent, with whether it asked for a byte range.
#[derive(Clone)]
struct Site {
    files: Arc<HashMap<String, Vec<u8>>>,
    live: Live,
    asks: Arc<AtomicUsize>,
    asked: Arc<Mutex<Vec<(String, bool)>>>,
    /// Segments asked for by a byte range are refused (416).
    refuse_ranges: bool,
    /// A byte range is answered with half the bytes asked for.
    short_ranges: bool,
}

impl Site {
    fn new(files: &[(&str, Vec<u8>)]) -> Self {
        Site {
            files: Arc::new(
                files
                    .iter()
                    .map(|(path, body)| (path.to_string(), body.clone()))
                    .collect(),
            ),
            live: Live::default(),
            asks: Arc::new(AtomicUsize::new(0)),
            asked: Arc::new(Mutex::new(Vec::new())),
            refuse_ranges: false,
            short_ranges: false,
        }
    }

    fn live(live: Live) -> Self {
        Site {
            live,
            ..Site::new(&[])
        }
    }

    fn asked(&self) -> Vec<(String, bool)> {
        self.asked.lock().unwrap().clone()
    }

    async fn serve(&self) -> SocketAddr {
        common::serve(
            axum::Router::new()
                .fallback(answer)
                .with_state(self.clone()),
        )
        .await
    }

    fn live_playlist(&self, ask: usize) -> String {
        let live = &self.live;
        let out = (ask + 1).min(live.segments);
        let first = out.saturating_sub(3);
        let ext = if live.map { "m4s" } else { "ts" };
        let mut text = format!(
            "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n"
        );
        if live.map {
            text.push_str("#EXT-X-MAP:URI=\"init.mp4\"\n");
        }
        if live.key {
            text.push_str("#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n");
        }
        for n in first..out {
            text.push_str(&format!("#EXTINF:1.0,\nseg{n}.{ext}\n"));
        }
        if out == live.segments {
            text.push_str("#EXT-X-ENDLIST\n");
        }
        text
    }
}

fn status(code: StatusCode) -> Response {
    Response::builder()
        .status(code)
        .body(Body::empty())
        .unwrap()
}

fn kind_of(path: &str) -> &'static str {
    if path.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if path.ends_with(".ts") {
        "video/mp2t"
    } else {
        "application/octet-stream"
    }
}

fn byte_range(headers: &HeaderMap) -> Option<(usize, Option<usize>)> {
    let range = headers.get("range")?.to_str().ok()?;
    let (first, last) = range.strip_prefix("bytes=")?.split_once('-')?;
    Some((first.parse().ok()?, last.parse().ok()))
}

async fn answer(State(site): State<Site>, uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().to_string();
    let range = byte_range(&headers);
    site.asked
        .lock()
        .unwrap()
        .push((path.clone(), range.is_some()));

    if path == "/live.m3u8" {
        // The probe asks with a range; only the playlist's own asks count.
        let ask = if range.is_some() {
            site.asks.load(Ordering::SeqCst)
        } else {
            site.asks.fetch_add(1, Ordering::SeqCst)
        };
        if range.is_none() {
            let live = &site.live;
            if live.failing.contains(&ask) || live.fails_from.is_some_and(|from| ask >= from) {
                return status(StatusCode::INTERNAL_SERVER_ERROR);
            }
            if let Some((slow, held)) = live.slow {
                if ask == slow {
                    tokio::time::sleep(held).await;
                }
            }
        }
        return Response::builder()
            .header("content-type", kind_of(&path))
            .body(Body::from(site.live_playlist(ask)))
            .unwrap();
    }
    if site.live.segments > 0 {
        if path == "/init.mp4" {
            return Response::new(Body::from(b"INIT".to_vec()));
        }
        if path == "/key.bin" {
            return Response::new(Body::from(KEY.to_vec()));
        }
        if let Some(n) = path
            .strip_prefix("/seg")
            .and_then(|rest| rest.split('.').next())
            .and_then(|n| n.parse::<usize>().ok())
        {
            if site.live.missing == Some(n) {
                return status(StatusCode::NOT_FOUND);
            }
            let body = if site.live.key { sealed(n) } else { segment(n) };
            return Response::new(Body::from(body));
        }
    }

    let Some(body) = site.files.get(&path) else {
        return status(StatusCode::NOT_FOUND);
    };
    let playlist = path.ends_with(".m3u8");
    match range {
        Some(_) if !playlist && site.refuse_ranges => status(StatusCode::RANGE_NOT_SATISFIABLE),
        Some((first, last)) if !playlist => {
            let last = last.unwrap_or(body.len() - 1).min(body.len() - 1);
            let mut piece = body[first..=last].to_vec();
            if site.short_ranges {
                piece.truncate(piece.len() / 2);
            }
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("content-type", kind_of(&path))
                .header(
                    "content-range",
                    format!("bytes {first}-{last}/{}", body.len()),
                )
                .body(Body::from(piece))
                .unwrap()
        }
        _ => Response::builder()
            .header("content-type", kind_of(&path))
            .body(Body::from(body.clone()))
            .unwrap(),
    }
}

/// A finished media playlist of `count` four-second `.ts` segments.
fn vod(count: usize, extra: &str) -> Vec<u8> {
    let body: String = (0..count)
        .map(|i| format!("#EXTINF:4.0,\nseg{i}.ts\n"))
        .collect();
    format!(
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n{extra}{body}#EXT-X-ENDLIST\n"
    )
    .into_bytes()
}

fn vod_site(count: usize) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![("/v/show.m3u8".to_string(), vod(count, ""))];
    for i in 0..count {
        files.push((format!("/v/seg{i}.ts"), segment(i)));
    }
    files
}

fn site_of(files: &[(String, Vec<u8>)]) -> Site {
    let borrowed: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.clone()))
        .collect();
    Site::new(&borrowed)
}

fn internal(result: Result<dm_core::Finished, Error>) -> String {
    match result {
        Err(Error::Internal(why)) => why,
        other => panic!("not refused in words: {other:?}"),
    }
}

const TWO_KEYS: &str = "#EXT-X-KEY:METHOD=AES-128,URI=\"a.key\"\n#EXTINF:4.0,\nseg0.ts\n\
                        #EXT-X-KEY:METHOD=AES-128,URI=\"b.key\"\n";

#[tokio::test]
async fn a_stream_that_changes_its_key_part_way_is_said_to_be_beyond_dm() {
    // Straight to the media playlist.
    let site = Site::new(&[("/v/show.m3u8", vod(2, TWO_KEYS))]);
    let addr = site.serve().await;
    let dir = tempdir();
    let why = internal(
        download(
            &client(),
            &format!("http://{addr}/v/show.m3u8"),
            &options(dir.path()),
        )
        .await,
    );
    assert!(
        why.contains("changes its encryption part-way through"),
        "{why}"
    );
    assert!(
        why.ends_with("this stream shape is not supported yet"),
        "{why}"
    );

    // Through a master playlist naming it.
    let site = Site::new(&[
        (
            "/v/master.m3u8",
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nshow.m3u8\n".to_vec(),
        ),
        ("/v/show.m3u8", vod(2, TWO_KEYS)),
    ]);
    let addr = site.serve().await;
    let why = internal(
        download(
            &client(),
            &format!("http://{addr}/v/master.m3u8"),
            &options(dir.path()),
        )
        .await,
    );
    assert!(why.contains("changes its encryption"), "{why}");
    // Nothing was fetched past the playlists.
    assert!(
        site.asked().iter().all(|(p, _)| p.ends_with(".m3u8")),
        "{:?}",
        site.asked()
    );
}

#[tokio::test]
async fn a_file_called_a_playlist_that_is_not_one_is_refused() {
    let site = Site::new(&[("/v/show.m3u8", b"<html>not here</html>".to_vec())]);
    let addr = site.serve().await;
    let dir = tempdir();
    let why = internal(
        download(
            &client(),
            &format!("http://{addr}/v/show.m3u8"),
            &options(dir.path()),
        )
        .await,
    );
    assert_eq!(why, "that is not an HLS playlist");
}

#[tokio::test]
async fn a_master_whose_rendition_is_live_is_recorded_from_that_rendition() {
    let mut site = Site::live(Live {
        segments: 2,
        ..Live::default()
    });
    site.files = Arc::new(HashMap::from([(
        "/master.m3u8".to_string(),
        b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nlive.m3u8\n".to_vec(),
    )]));
    let addr = site.serve().await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/master.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..2));
}

/// A master whose rendition keeps its sound in a playlist of its own.
fn paired(picture: Vec<u8>, sound: Vec<u8>) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "/p/master.m3u8".into(),
            b"#EXTM3U\n\
              #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
              #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"aud\"\n\
              picture.m3u8\n"
                .to_vec(),
        ),
        ("/p/picture.m3u8".into(), picture),
        ("/p/sound.m3u8".into(), sound),
    ]
}

fn media(names: &[&str], ended: bool) -> Vec<u8> {
    let mut text = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n");
    for name in names {
        text.push_str(&format!("#EXTINF:4.0,\n{name}\n"));
    }
    if ended {
        text.push_str("#EXT-X-ENDLIST\n");
    }
    text.into_bytes()
}

#[tokio::test]
async fn a_picture_and_sound_dm_cannot_join_are_said_to_be_beyond_it() {
    let dir = tempdir();
    let master = |addr: SocketAddr| format!("http://{addr}/p/master.m3u8");

    // The picture changes its key part-way.
    let site = site_of(&paired(vod(2, TWO_KEYS), media(&["a0.aac"], true)));
    let addr = site.serve().await;
    let why = internal(download(&client(), &master(addr), &options(dir.path())).await);
    assert!(why.contains("changes its encryption"), "{why}");

    // The sound is live while the picture is not.
    let site = site_of(&paired(media(&["v0.ts"], true), media(&["a0.aac"], false)));
    let addr = site.serve().await;
    let why = internal(download(&client(), &master(addr), &options(dir.path())).await);
    assert!(why.contains("carries its sound separately"), "{why}");

    // Both transport streams: not a pair dm joins.
    let site = site_of(&paired(media(&["v0.ts"], true), media(&["a0.ts"], true)));
    let addr = site.serve().await;
    let why = internal(download(&client(), &master(addr), &options(dir.path())).await);
    assert!(why.contains("carries its sound separately"), "{why}");
}

/// A live picture whose sound is a pair dm cannot join (both transport
/// streams) is recorded as the picture alone, and the sound's segments are
/// not fetched.
#[tokio::test]
async fn a_live_picture_with_sound_apart_dm_cannot_join_is_recorded_as_the_picture() {
    let mut site = Site::live(Live {
        segments: 2,
        ..Live::default()
    });
    site.files = Arc::new(HashMap::from([
        (
            "/master.m3u8".to_string(),
            b"#EXTM3U\n\
              #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
              #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"aud\"\n\
              live.m3u8\n"
                .to_vec(),
        ),
        ("/sound.m3u8".to_string(), media(&["a0.ts"], true)),
    ]));
    let addr = site.serve().await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/master.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..2));
    assert!(
        !site.asked().iter().any(|(p, _)| p.starts_with("/a0")),
        "the sound was fetched: {:?}",
        site.asked()
    );
}

/// A live picture and sound that read as a pair dm joins but whose bytes
/// cannot be joined (the picture is not a real transport stream): the
/// picture is kept alone, as recorded, and the sound's file is gone.
#[tokio::test]
async fn a_live_pair_that_cannot_be_joined_keeps_the_picture() {
    let mut site = Site::live(Live {
        segments: 2,
        ..Live::default()
    });
    site.files = Arc::new(HashMap::from([
        (
            "/master.m3u8".to_string(),
            b"#EXTM3U\n\
              #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
              #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"aud\"\n\
              live.m3u8\n"
                .to_vec(),
        ),
        ("/sound.m3u8".to_string(), media(&["a0.aac"], true)),
        ("/a0.aac".to_string(), packed_sound(0)),
    ]));
    let addr = site.serve().await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/master.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..2));
    assert!(
        site.asked().iter().any(|(p, _)| p == "/a0.aac"),
        "the sound was not recorded: {:?}",
        site.asked()
    );
    let left: Vec<String> = std::fs::read_dir(dir.path().join("parts"))
        .map(|listing| {
            listing
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("live."))
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "left behind: {left:?}");
}

/// Real H.264 picture segments in a transport stream, for a live stand-in.
const PICTURES: [&[u8]; 3] = [
    include_bytes!("fixtures/media/hls/h264/seg0.ts"),
    include_bytes!("fixtures/media/hls/h264/seg1.ts"),
    include_bytes!("fixtures/media/hls/h264/seg2.ts"),
];

/// One second of packed AAC sound for segment `index`: an ID3 tag stating
/// when it starts in the 90 kHz clock, then 44.1 kHz stereo AAC-LC frames
/// of 300 bytes.
fn packed_sound(index: usize) -> Vec<u8> {
    let mut sound = b"ID3\x04\x00\x00\x00\x00\x00\x4f".to_vec();
    sound.extend_from_slice(b"PRIV\x00\x00\x00\x35\x00\x00");
    sound.extend_from_slice(b"com.apple.streaming.transportStreamTimestamp\0");
    sound.extend_from_slice(&(index as u64 * 90_000).to_be_bytes());
    for _ in 0..43 {
        let mut frame = vec![0xff, 0xf1, 0x50, 0x80, 0x25, 0x80 | 0x1f, 0xfc];
        frame.resize(300, 7);
        sound.extend(frame);
    }
    sound
}

/// A live playlist of `count` one-second segments named `name{n}.{ext}`,
/// at ask `ask`: one more segment out at each ask, the latest three
/// listed, and the end tag once all are out.
fn live_media(ask: usize, count: usize, name: &str, ext: &str) -> String {
    let out = (ask + 1).min(count);
    let first = out.saturating_sub(3);
    let mut text = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n"
    );
    for n in first..out {
        text.push_str(&format!("#EXTINF:1.0,\n{name}{n}.{ext}\n"));
    }
    if out == count {
        text.push_str("#EXT-X-ENDLIST\n");
    }
    text
}

/// A live stream whose master keeps the sound in a playlist of its own:
/// both playlists grow a segment at each ask and then end. Both are
/// recorded, each segment fetched once, and joined into one MP4 with a
/// picture track and a sound track.
#[tokio::test]
async fn a_live_stream_with_its_sound_apart_is_recorded_with_its_sound() {
    #[derive(Clone, Default)]
    struct Paired {
        picture_asks: Arc<AtomicUsize>,
        sound_asks: Arc<AtomicUsize>,
        asked: Arc<Mutex<Vec<String>>>,
    }
    async fn answer(State(site): State<Paired>, uri: Uri, headers: HeaderMap) -> Response {
        let path = uri.path().to_string();
        site.asked.lock().unwrap().push(path.clone());
        let probe = headers.contains_key("range");
        let playlist = |text: String| {
            Response::builder()
                .header("content-type", "application/vnd.apple.mpegurl")
                .body(Body::from(text))
                .unwrap()
        };
        // A probe asks with a range; only the playlist's own asks count.
        let ask = |asks: &AtomicUsize| {
            if probe {
                asks.load(Ordering::SeqCst)
            } else {
                asks.fetch_add(1, Ordering::SeqCst)
            }
        };
        match path.as_str() {
            "/lp/master.m3u8" => playlist(
                "#EXTM3U\n\
                 #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"en\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO=\"aud\"\n\
                 picture.m3u8\n"
                    .to_string(),
            ),
            "/lp/picture.m3u8" => playlist(live_media(ask(&site.picture_asks), 3, "v", "ts")),
            "/lp/sound.m3u8" => playlist(live_media(ask(&site.sound_asks), 3, "a", "aac")),
            _ => {
                let n = |prefix: &str, ext: &str| {
                    path.strip_prefix(prefix)
                        .and_then(|rest| rest.strip_suffix(ext))
                        .and_then(|n| n.parse::<usize>().ok())
                };
                if let Some(picture) = n("/lp/v", ".ts").and_then(|n| PICTURES.get(n)) {
                    return Response::builder()
                        .header("content-type", "video/mp2t")
                        .body(Body::from(picture.to_vec()))
                        .unwrap();
                }
                if let Some(n) = n("/lp/a", ".aac").filter(|n| *n < 3) {
                    return Response::new(Body::from(packed_sound(n)));
                }
                status(StatusCode::NOT_FOUND)
            }
        }
    }

    let site = Paired::default();
    let addr = common::serve(
        axum::Router::new()
            .fallback(answer)
            .with_state(site.clone()),
    )
    .await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/lp/master.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();

    assert_eq!(done.path.file_name().unwrap(), "lp.mp4");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(&data[4..8], b"ftyp");
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
    assert!(data.windows(4).any(|w| w == b"avc1"), "no picture track");
    assert!(data.windows(4).any(|w| w == b"mp4a"), "no sound track");
    assert!(data.windows(4).any(|w| w == b"soun"), "no sound handler");
    // Recorded as it went: each playlist asked for again as it grew, and
    // every segment of both fetched once.
    let asked = site.asked.lock().unwrap().clone();
    for playlist in ["/lp/picture.m3u8", "/lp/sound.m3u8"] {
        let asks = asked.iter().filter(|p| *p == playlist).count();
        assert!(asks >= 3, "{playlist} asked {asks} times: {asked:?}");
    }
    for n in 0..3 {
        for segment in [format!("/lp/v{n}.ts"), format!("/lp/a{n}.aac")] {
            let times = asked.iter().filter(|p| **p == segment).count();
            assert_eq!(times, 1, "{segment} fetched {times} times: {asked:?}");
        }
    }
    // Nothing left in the parts folder but what the engine keeps there.
    let parts = std::fs::read_dir(dir.path().join("parts"))
        .map(|listing| {
            listing
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("live."))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(parts.is_empty(), "left behind: {parts:?}");
}

#[tokio::test]
async fn a_part_outside_the_stream_is_refused_before_anything_is_fetched() {
    let site = site_of(&vod_site(3));
    let addr = site.serve().await;
    let dir = tempdir();
    let asked = Options {
        span: Span::new(100.0, 110.0),
        ..options(dir.path())
    };
    let why = internal(download(&client(), &format!("http://{addr}/v/show.m3u8"), &asked).await);
    assert_eq!(why, "that part is outside the stream's length");
    assert!(!site.asked().iter().any(|(p, _)| p.contains("/seg")));
}

/// A part of a transport stream is joined whole at the segments around it,
/// named for the part, and the name is told as soon as it is known.
#[tokio::test]
async fn a_part_of_a_transport_stream_is_its_segments_named_for_the_part() {
    let site = site_of(&vod_site(5));
    let addr = site.serve().await;
    let dir = tempdir();
    let asked = Options {
        span: Span::new(4.5, 9.0),
        ..options(dir.path())
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let done = download_reporting(
        &client(),
        &format!("http://{addr}/v/show.m3u8"),
        &asked,
        Some(tx),
    )
    .await
    .unwrap();
    let name = done.path.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        name.starts_with("show (") && name.ends_with(").ts"),
        "{name}"
    );
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(1..3));
    let mut named = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let Update::Named { name, .. } = update {
            named.push(name);
        }
    }
    assert_eq!(named.last(), Some(&name), "{named:?}");
    // Only the segments the part needs.
    let fetched: Vec<String> = site
        .asked()
        .into_iter()
        .filter(|(p, range)| p.contains("/seg") && !range)
        .map(|(p, _)| p)
        .collect();
    assert_eq!(fetched.len(), 2, "{fetched:?}");
}

/// A part of a fragmented MP4 stream is cut by the muxer's edit lists: here
/// a real fragmented file served as an initialisation segment and two
/// fragments.
#[tokio::test]
async fn a_part_of_a_fragmented_stream_is_cut_by_the_muxer() {
    let whole = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/media/mp4mux/picture.f137.mp4"),
    )
    .unwrap();
    let site = Site::new(&[
        (
            "/f/show.m3u8",
            b"#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:1\n\
              #EXT-X-MAP:URI=\"init.mp4\"\n\
              #EXTINF:1.0,\nf0.m4s\n#EXTINF:1.0,\nf1.m4s\n#EXT-X-ENDLIST\n"
                .to_vec(),
        ),
        ("/f/init.mp4", whole[..765].to_vec()),
        ("/f/f0.m4s", whole[829..8072].to_vec()),
        ("/f/f1.m4s", whole[8072..15945].to_vec()),
    ]);
    let addr = site.serve().await;
    let dir = tempdir();
    let asked = Options {
        span: Span::new(0.0, 0.5),
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/f/show.m3u8"), &asked).await;
    let done = done.expect("the part");
    let name = done.path.file_name().unwrap().to_string_lossy().to_string();
    assert!(name.ends_with(".mp4") && name.contains('('), "{name}");
    let cut = std::fs::read(&done.path).unwrap();
    assert_eq!(&cut[4..8], b"ftyp", "not an MP4");
    assert!(cut.len() < whole.len(), "the part is the whole stream");
    // The muxer's working files are gone with the parts.
    assert!(!dir.path().join("parts/cut-input.mp4").exists());
}

#[tokio::test]
async fn a_resume_that_finds_the_stream_already_put_into_an_mp4_places_it() {
    let site = site_of(&vod_site(3));
    let addr = site.serve().await;
    let url = format!("http://{addr}/v/show.m3u8");
    let dir = tempdir();
    let asked = Options {
        remux: true,
        ..options(dir.path())
    };
    // What a run paused after its remux left: the MP4, and the rendition.
    std::fs::create_dir_all(&asked.parts_dir).unwrap();
    std::fs::write(asked.parts_dir.join("rendition"), &url).unwrap();
    std::fs::write(asked.parts_dir.join("joined.mp4"), b"the mp4 made before").unwrap();
    let done = download(&client(), &url, &asked).await.unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    assert_eq!(std::fs::read(&done.path).unwrap(), b"the mp4 made before");
    // At most the first segment's head was looked at, never a whole one.
    assert!(
        !site
            .asked()
            .iter()
            .any(|(p, range)| p.contains("/seg") && !range),
        "{:?}",
        site.asked()
    );
}

#[tokio::test]
async fn a_resume_that_finds_the_segments_already_gathered_places_them_unfetched() {
    let site = site_of(&vod_site(3));
    let addr = site.serve().await;
    let url = format!("http://{addr}/v/show.m3u8");
    let dir = tempdir();
    let asked = options(dir.path());
    std::fs::create_dir_all(&asked.parts_dir).unwrap();
    std::fs::write(asked.parts_dir.join("rendition"), &url).unwrap();
    std::fs::write(asked.parts_dir.join("joined.ts"), segments(0..3)).unwrap();
    let done = download(&client(), &url, &asked).await.unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.ts");
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..3));
    assert!(
        !site.asked().iter().any(|(p, _)| p.contains("/seg")),
        "a segment was asked for though all were here: {:?}",
        site.asked()
    );
}

#[tokio::test]
async fn a_server_that_refuses_a_range_still_serves_its_segments_whole() {
    let mut site = site_of(&vod_site(3));
    site.refuse_ranges = true;
    let addr = site.serve().await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/v/show.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..3));
    // The head of the first was asked for by range, and refused.
    assert!(site
        .asked()
        .iter()
        .any(|(p, range)| p.ends_with("/seg0.ts") && *range));
}

#[tokio::test]
async fn a_byte_range_answered_with_fewer_bytes_fails_the_stream() {
    let mut site = Site::new(&[
        (
            "/r/show.m3u8",
            b"#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-TARGETDURATION:4\n\
              #EXTINF:4.0,\n#EXT-X-BYTERANGE:1000@0\nall.ts\n\
              #EXTINF:4.0,\n#EXT-X-BYTERANGE:1000@1000\nall.ts\n#EXT-X-ENDLIST\n"
                .to_vec(),
        ),
        ("/r/all.ts", segments(0..1)),
    ]);
    site.short_ranges = true;
    let addr = site.serve().await;
    let dir = tempdir();
    let why = internal(
        download(
            &client(),
            &format!("http://{addr}/r/show.m3u8"),
            &options(dir.path()),
        )
        .await,
    );
    assert!(
        why.ends_with("answered a byte range with the wrong number of bytes"),
        "{why}"
    );
}

/// A live recording with an initialisation segment and scrambled segments,
/// told by name as it starts and by its progress as it goes.
#[tokio::test]
async fn a_scrambled_live_stream_with_an_initialisation_segment_is_recorded_in_clear() {
    let site = Site::live(Live {
        segments: 3,
        map: true,
        key: true,
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let done = download_reporting(
        &client(),
        &format!("http://{addr}/live.m3u8"),
        &options(dir.path()),
        Some(tx),
    )
    .await
    .unwrap();
    let mut wanted = b"INIT".to_vec();
    wanted.extend(segments(0..3));
    assert_eq!(std::fs::read(&done.path).unwrap(), wanted);
    // The key once, the initialisation segment once.
    let asked = site.asked();
    assert_eq!(asked.iter().filter(|(p, _)| p == "/key.bin").count(), 1);
    assert_eq!(asked.iter().filter(|(p, _)| p == "/init.mp4").count(), 1);
    let mut named = false;
    while let Ok(update) = rx.try_recv() {
        named |= matches!(update, Update::Named { .. });
    }
    assert!(named, "the recording was never named");
}

#[tokio::test]
async fn a_live_recording_cancelled_before_anything_arrives_leaves_nothing() {
    let site = Site::live(Live {
        segments: 3,
        slow: Some((1, Duration::from_secs(5))),
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let opts = options(dir.path());
    let control = opts.control.clone();
    let stop = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        control.cancel();
    });
    let result = download(&client(), &format!("http://{addr}/live.m3u8"), &opts).await;
    stop.await.unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(!opts.parts_dir.join("live.recording").exists());
}

#[tokio::test]
async fn a_live_recording_paused_before_anything_arrives_says_it_has_nothing() {
    let site = Site::live(Live {
        segments: 3,
        slow: Some((1, Duration::from_secs(5))),
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let opts = options(dir.path());
    let control = opts.control.clone();
    let stop = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        control.pause();
    });
    let why = internal(download(&client(), &format!("http://{addr}/live.m3u8"), &opts).await);
    stop.await.unwrap();
    assert_eq!(why, "the live stream sent nothing to record");
}

/// A live playlist that moves past segments before dm fetched them: the
/// ones still listed are recorded, and the row is told how many were gone.
#[tokio::test]
async fn live_segments_gone_before_they_were_fetched_are_told_on_the_row() {
    async fn answer(
        State(asks): State<Arc<AtomicUsize>>,
        uri: Uri,
        headers: HeaderMap,
    ) -> Response {
        let path = uri.path().to_string();
        if path == "/gap.m3u8" {
            // The first asks (the one that resolves it and the recording's
            // first) list segments 0 and 1; the next lists 5 and 6, ended.
            // A probe asks with a range and is not counted.
            let ask = if headers.contains_key("range") {
                asks.load(Ordering::SeqCst)
            } else {
                asks.fetch_add(1, Ordering::SeqCst)
            };
            let text = if ask < 2 {
                "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n\
                 #EXTINF:1.0,\nseg0.ts\n#EXTINF:1.0,\nseg1.ts\n"
            } else {
                "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:5\n\
                 #EXTINF:1.0,\nseg5.ts\n#EXTINF:1.0,\nseg6.ts\n#EXT-X-ENDLIST\n"
            };
            return Response::builder()
                .header("content-type", "application/vnd.apple.mpegurl")
                .body(Body::from(text))
                .unwrap();
        }
        match path
            .strip_prefix("/seg")
            .and_then(|rest| rest.strip_suffix(".ts"))
            .and_then(|n| n.parse::<usize>().ok())
        {
            Some(n) => Response::new(Body::from(segment(n))),
            None => status(StatusCode::NOT_FOUND),
        }
    }
    let addr = common::serve(
        axum::Router::new()
            .fallback(answer)
            .with_state(Arc::new(AtomicUsize::new(0))),
    )
    .await;
    let dir = tempdir();
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let done = download_reporting(
        &client(),
        &format!("http://{addr}/gap.m3u8"),
        &options(dir.path()),
        Some(tx),
    )
    .await
    .unwrap();
    let mut recorded = segments(0..2);
    recorded.extend(segments(5..7));
    assert_eq!(std::fs::read(&done.path).unwrap(), recorded);
    let mut notes = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let Update::Note(note) = update {
            notes.push(note);
        }
    }
    let said = dm_core::NOTES[1].replace("{}", "3");
    assert_eq!(notes, vec![said]);
}

#[tokio::test]
async fn a_live_playlist_that_fails_once_is_asked_again() {
    let site = Site::live(Live {
        segments: 3,
        failing: vec![2],
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/live.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..3));
}

#[tokio::test]
async fn a_live_playlist_that_fails_before_anything_is_recorded_fails_the_recording() {
    let site = Site::live(Live {
        segments: 3,
        fails_from: Some(1),
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let result = download(
        &client(),
        &format!("http://{addr}/live.m3u8"),
        &options(dir.path()),
    )
    .await;
    let Err(e) = result else {
        panic!("recorded from a playlist that never answered: {result:?}");
    };
    assert!(!matches!(e, Error::Cancelled | Error::Paused), "{e:?}");
    assert_ne!(
        e.to_string(),
        "the live stream sent nothing to record",
        "the server's refusal was not what was said"
    );
}

/// A playlist that stops answering for good ends the recording after five
/// more tries, two seconds apart, with what it had.
#[tokio::test]
async fn a_live_playlist_that_stops_answering_keeps_what_was_recorded() {
    let site = Site::live(Live {
        segments: 100,
        fails_from: Some(2),
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/live.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..2));
}

#[tokio::test]
async fn a_live_segment_that_cannot_be_had_is_left_out() {
    let site = Site::live(Live {
        segments: 2,
        missing: Some(1),
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/live.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segment(0));
}

/// Asked to remux a recording dm cannot read as H.264 and AAC: the
/// recording is kept as the transport stream it came as.
#[tokio::test]
async fn a_live_recording_dm_cannot_remux_is_kept_as_its_stream() {
    let site = Site::live(Live {
        segments: 2,
        ..Live::default()
    });
    let addr = site.serve().await;
    let dir = tempdir();
    let asked = Options {
        remux: true,
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/live.m3u8"), &asked)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "ts");
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..2));
}

/// Gathered by an earlier run that was to remux it: the gathered stream is
/// remuxed as it is, not gathered again, and kept as it came when dm cannot
/// put it into an MP4.
#[tokio::test]
async fn a_resume_with_the_stream_gathered_for_a_remux_remuxes_it_as_it_is() {
    let site = site_of(&vod_site(3));
    let addr = site.serve().await;
    let url = format!("http://{addr}/v/show.m3u8");
    let dir = tempdir();
    let asked = Options {
        remux: true,
        ..options(dir.path())
    };
    std::fs::create_dir_all(&asked.parts_dir).unwrap();
    std::fs::write(asked.parts_dir.join("rendition"), &url).unwrap();
    std::fs::write(asked.parts_dir.join("joined.ts"), segments(0..3)).unwrap();
    let done = download(&client(), &url, &asked).await.unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.ts");
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..3));
    assert!(!site.asked().iter().any(|(p, _)| p.contains("/seg")));
}

/// A segment whose server goes quiet part-way for longer than the control
/// is looked at is waited for, not dropped: every byte arrives.
#[tokio::test]
async fn a_segment_that_pauses_part_way_is_waited_for_whole() {
    use futures_util::StreamExt;
    let app = axum::Router::new().fallback(|uri: Uri| async move {
        let path = uri.path().to_string();
        if path.ends_with(".m3u8") {
            return Response::builder()
                .header("content-type", "application/vnd.apple.mpegurl")
                .body(Body::from(vod(2, "")))
                .unwrap();
        }
        let n: usize = path
            .trim_start_matches("/v/seg")
            .trim_end_matches(".ts")
            .parse()
            .unwrap_or(0);
        let body = segment(n);
        let (head, tail) = body.split_at(body.len() / 2);
        let pieces = vec![
            bytes::Bytes::copy_from_slice(head),
            bytes::Bytes::copy_from_slice(tail),
        ];
        let quiet = futures_util::stream::iter(pieces.into_iter().enumerate()).then(
            |(i, piece)| async move {
                if i == 1 {
                    tokio::time::sleep(Duration::from_millis(600)).await;
                }
                Ok::<_, std::io::Error>(piece)
            },
        );
        Response::new(Body::from_stream(quiet))
    });
    let addr = common::serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/v/show.m3u8"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), segments(0..2));
}
