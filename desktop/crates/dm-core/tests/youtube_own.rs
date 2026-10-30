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
        .header("etag", "\"v\"")
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");

    let player_api = {
        let origin = origin.clone();
        move |headers: HeaderMap, body: String| async move {
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
                     "bitrate": 4000000, "contentLength": "8000", "url": format!("{origin}/picture?itag=137&n=abc")},
                    {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "averageBitrate": 130000,
                     "contentLength": "500",
                     "signatureCipher": format!("s=xyz&sp=sig&url={}", urlencoding(&format!("{origin}/sound?itag=140&n=def")))},
                    {"itag": 22, "mimeType": "video/mp4; codecs=\"avc1\"", "width": 1280, "height": 720,
                     "bitrate": 1, "drmFamilies": ["w"], "url": format!("{origin}/locked")}
                ]}
            });
            text(StatusCode::OK, "application/json", answer.to_string())
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
        .fallback(get(
            |State(seen): State<Seen>, uri: Uri, headers: HeaderMap| async move {
                seen.0.lock().unwrap().push(uri.to_string());
                let query = uri.query().unwrap_or("");
                match uri.path() {
                    "/picture" if query.contains("n=ABC") => ranged(&fixture("picture.f137.mp4"), &headers),
                    "/sound" if query.contains("n=DEF") && query.contains("sig=XYZ") => {
                        ranged(&fixture("sound.f140.m4a"), &headers)
                    }
                    _ => text(StatusCode::FORBIDDEN, "text/plain", format!("not solved: {uri}")),
                }
            },
        ))
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
}

/// `text` percent-encoded as a query value.
fn urlencoding(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}
