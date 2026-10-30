//! A YouTube watch page read by dm itself (`dm_core::youtube`), against a
//! stand-in for YouTube: its watch page, its player API, its player script
//! and the solver's releases are all a local server's, and the streams are
//! served only once their `n` values and signature have been solved. No
//! yt-dlp is available to fall back on, so the download is the reader's.
//!
//! One test in this binary: the stand-in is named by environment variables,
//! which are the process's.

mod common;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use dm_core::{client, ytdlp};

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

/// Whether a JavaScript engine the reader can run the solver in is here.
fn an_engine_is_here() -> bool {
    ["deno", "qjs"].iter().any(|program| {
        std::process::Command::new(program)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_youtube_page_is_read_and_fetched_by_dm_with_no_video_helper() {
    if !an_engine_is_here() {
        eprintln!("no deno or qjs here: skipped");
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
            get(|| async { text(StatusCode::OK, "text/javascript", "var player = 1;".into()) }),
        )
        .route(
            "/ejs/yt.solver.lib.min.js",
            get(|| async { text(StatusCode::OK, "text/javascript", "var lib = { from: 'the lib' };".into()) }),
        )
        .route(
            "/ejs/yt.solver.core.min.js",
            get(|| async {
                // The stand-in solver: each challenge answered in capitals,
                // in the real solver's shape.
                text(
                    StatusCode::OK,
                    "text/javascript",
                    "var jsc = function (input) { if (!globalThis.from || input.type !== 'player' || typeof input.player !== 'string') { return { type: 'error', error: 'bad input' }; } return { type: 'result', responses: input.requests.map(function (r) { return { type: 'result', data: Object.fromEntries(r.challenges.map(function (c) { return [c, c.toUpperCase()]; })) }; }) }; };".into(),
                )
            }),
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
                        _ => text(StatusCode::FORBIDDEN, "text/plain", format!("not solved: {uri}")),
                    }
                }
            }
        }))
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempdir();
    // The stand-in, and no yt-dlp at all.
    std::env::set_var("DM_YOUTUBE_HOST", &origin);
    std::env::set_var("DM_EJS_RELEASES", format!("{origin}/ejs"));
    std::env::set_var("DM_YTDLP", dir.path().join("no-such-yt-dlp"));
    dm_core::tools::set_folder(dir.path().join("tools"));

    let page = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
    let done = ytdlp::download_page(&client(), page, &options(dir.path()), None)
        .await
        .expect("the page did not download through dm's own reader");

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
    // The solver's scripts were kept, for the next reading.
    assert!(dir
        .path()
        .join("tools/ejs-0.8.0/yt.solver.core.min.js")
        .is_file());

    // A part of the same video, from 2 to 4 seconds. Each piece of the
    // picture plays a second, so its pieces 2 and 3 are asked for, by
    // their bytes alone, after its first boxes and its index; the sound's
    // pieces are 1.046 s each, so its 1 to 3. The file is named for the
    // span and cut to it.
    requests.0.lock().unwrap().clear();
    let mut part = options(dir.path());
    part.span = dm_core::Span::new(2.0, 4.0);
    let done = ytdlp::download_page(&client(), page, &part, None)
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
}

/// `text` percent-encoded as a query value.
fn urlencoding(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}
