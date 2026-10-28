//! A video page read by yt-dlp and fetched by dm (`ytdlp::download_page`).
//!
//! yt-dlp here is a stand-in (`tests/fixtures/fake-ytdlp.py`) that answers
//! a reading with what a test wrote for it, and downloads only when dm hands
//! the page back to it: so each test says exactly which streams yt-dlp
//! "found", and can tell whether dm fetched them or yt-dlp did. Unix only,
//! as the stand-in is a script; the real yt-dlp's route is `tests/ytdlp.rs`.
#![cfg(unix)]

mod common;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, ytdlp};

use common::{options, tempdir};

/// What the server saw: each request's path, agent and client port.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(String, String, u16)>>>);

#[derive(Clone)]
struct Served {
    files: Arc<Vec<(&'static str, Vec<u8>)>>,
    refuse: Option<&'static str>,
    seen: Seen,
    /// Bytes a second each answer is held to, where it is held.
    pace: Option<u64>,
}

async fn serve_files(served: Served) -> SocketAddr {
    let app = axum::Router::new()
        .fallback(get(
            |State(s): State<Served>,
             ConnectInfo(from): ConnectInfo<SocketAddr>,
             uri: Uri,
             headers: HeaderMap| async move {
                let path = uri.path().to_string();
                let agent = headers
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                s.seen
                    .0
                    .lock()
                    .unwrap()
                    .push((path.clone(), agent, from.port()));
                if s.refuse == Some(path.as_str()) {
                    return Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(Body::empty())
                        .unwrap();
                }
                let Some((_, data)) = s.files.iter().find(|(p, _)| *p == path) else {
                    return Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::empty())
                        .unwrap();
                };
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
                let piece = bytes::Bytes::copy_from_slice(&data[a..=b]);
                let body = match s.pace {
                    None => Body::from(piece),
                    Some(rate) => {
                        const CHUNK: usize = 64 * 1024;
                        let wait = std::time::Duration::from_secs_f64(CHUNK as f64 / rate as f64);
                        let chunks: Vec<bytes::Bytes> = (0..piece.len())
                            .step_by(CHUNK)
                            .map(|at| piece.slice(at..(at + CHUNK).min(piece.len())))
                            .collect();
                        Body::from_stream(futures_util::StreamExt::then(
                            futures_util::stream::iter(chunks),
                            move |chunk| async move {
                                tokio::time::sleep(wait).await;
                                Ok::<_, std::io::Error>(chunk)
                            },
                        ))
                    }
                };
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{size}"))
                    .header("content-length", b - a + 1)
                    .header("etag", "\"v\"")
                    .body(body)
                    .unwrap()
            },
        ))
        .with_state(served);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    addr
}

/// The stand-in yt-dlp, named for every test in this binary.
fn use_fake_ytdlp() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-ytdlp.py");
        // Made runnable here: git keeps the mode, a zip of the tree may not.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        // SAFETY: runs once (`Once`), and every test in this binary calls this
        // before it reads the environment.
        unsafe {
            std::env::set_var("DM_YTDLP", &fake);
            // No JavaScript runtime is looked for or passed.
            std::env::set_var("DM_DENO", "");
            std::env::set_var("DM_QJS", "");
        }
    });
}

/// A case folder for the stand-in: the reading it prints (`__STAGE__` is
/// where yt-dlp was told to save) and what it saves if asked to download.
fn case(dir: &Path, reading: serde_json::Value, fallback: &[u8]) -> (PathBuf, String) {
    let case = dir.join("case");
    std::fs::create_dir_all(&case).unwrap();
    let mut reading = reading;
    reading["fake_case"] = serde_json::Value::String(case.display().to_string());
    std::fs::write(case.join("info.json"), reading.to_string()).unwrap();
    std::fs::write(case.join("fallback.bin"), fallback).unwrap();
    let page = format!("http://127.0.0.1:9/watch?case={}", case.display());
    (case, page)
}

fn calls(case: &Path) -> Vec<String> {
    std::fs::read_to_string(case.join("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mp4")
            .join(name),
    )
    .unwrap()
}

/// YouTube's shape: a picture and a sound over https with an agent they
/// must be asked with. dm fetches both itself, with that agent, and joins
/// them; yt-dlp only read the page.
#[tokio::test(flavor = "multi_thread")]
async fn a_picture_and_sound_ytdlp_found_are_fetched_by_dm_and_joined() {
    use_fake_ytdlp();
    let seen = Seen::default();
    let addr = serve_files(Served {
        files: Arc::new(vec![
            ("/picture", fixture("picture.f137.mp4")),
            ("/sound", fixture("sound.f140.m4a")),
        ]),
        refuse: None,
        seen: seen.clone(),
        pace: None,
    })
    .await;
    let dir = tempdir();
    let headers = serde_json::json!({"User-Agent": "FakePlayer/1.0", "Accept": "*/*"});
    let (case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "abc", "title": "Talk", "ext": "mp4",
            "filename": "__STAGE__/Talk [abc].mp4",
            "requested_formats": [
                {"format_id": "137", "ext": "mp4", "protocol": "https",
                 "url": format!("http://{addr}/picture"), "http_headers": headers},
                {"format_id": "140", "ext": "m4a", "protocol": "https",
                 "url": format!("http://{addr}/sound"), "http_headers": headers},
            ]
        }),
        b"not this",
    );

    let done = ytdlp::download_page(&client(), &page, &options(dir.path()), None)
        .await
        .expect("the page did not download");

    assert_eq!(done.path.file_name().unwrap(), "Talk [abc].mp4");
    let joined = std::fs::read(&done.path).unwrap();
    let has = |kind: &[u8]| joined.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 2, "picture and sound are both in it");
    assert!(has(b"avc1") >= 1 && has(b"mp4a") >= 1);
    let seen = seen.0.lock().unwrap().clone();
    assert!(seen.iter().any(|(p, ..)| p == "/picture"));
    assert!(seen.iter().any(|(p, ..)| p == "/sound"));
    assert!(
        seen.iter().all(|(_, agent, _)| agent == "FakePlayer/1.0"),
        "a stream was asked for without the agent yt-dlp gave: {seen:?}"
    );
    let calls = calls(&case);
    assert_eq!(calls.len(), 1, "yt-dlp was asked to download: {calls:?}");
    assert!(calls[0].contains("-O"), "{calls:?}");
}

/// One large stream is fetched on many connections, which is the point.
#[tokio::test(flavor = "multi_thread")]
async fn one_large_stream_is_fetched_on_many_connections() {
    use_fake_ytdlp();
    let data: Vec<u8> = (0..6 * 1024 * 1024).map(|i| (i * 7 % 251) as u8).collect();
    let seen = Seen::default();
    let addr = serve_files(Served {
        files: Arc::new(vec![("/video", data.clone())]),
        refuse: None,
        seen: seen.clone(),
        pace: None,
    })
    .await;
    let dir = tempdir();
    let (_, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "big", "title": "Big", "ext": "mp4", "format_id": "18",
            "protocol": "https", "url": format!("http://{addr}/video"),
            "filename": "__STAGE__/Big [big].mp4",
            "http_headers": {"User-Agent": "FakePlayer/1.0"}
        }),
        b"not this",
    );

    let done = ytdlp::download_page(&client(), &page, &options(dir.path()), None)
        .await
        .expect("the page did not download");

    assert_eq!(std::fs::read(&done.path).unwrap(), data, "not the file");
    let ports: HashSet<u16> = seen.0.lock().unwrap().iter().map(|s| s.2).collect();
    assert!(
        ports.len() > 1,
        "fetched on one connection, as yt-dlp would have"
    );
}

/// A server that refuses dm's own fetch: yt-dlp downloads the same reading
/// (`--load-info-json`), without reading the page again.
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_refuses_dm_is_downloaded_by_ytdlp_from_its_reading() {
    use_fake_ytdlp();
    let addr = serve_files(Served {
        files: Arc::new(vec![("/sound", fixture("sound.f140.m4a"))]),
        refuse: Some("/picture"),
        seen: Seen::default(),
        pace: None,
    })
    .await;
    let dir = tempdir();
    let (case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "abc", "title": "Talk", "ext": "mp4",
            "filename": "__STAGE__/Talk [abc].mp4",
            "requested_formats": [
                {"format_id": "137", "ext": "mp4", "protocol": "https",
                 "url": format!("http://{addr}/picture")},
                {"format_id": "140", "ext": "m4a", "protocol": "https",
                 "url": format!("http://{addr}/sound")},
            ]
        }),
        b"downloaded by yt-dlp",
    );
    let opts = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };

    let done = ytdlp::download_page(&client(), &page, &opts, None)
        .await
        .expect("the fallback did not download");

    assert_eq!(std::fs::read(&done.path).unwrap(), b"downloaded by yt-dlp");
    let calls = calls(&case);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(
        calls[1].contains("--load-info-json") && !calls[1].contains("watch?case="),
        "yt-dlp read the page again rather than downloading its reading: {calls:?}"
    );
    // Asked in pieces and for fragments together; never `--throttled-rate`,
    // which a link slower than it turns into a download that never ends.
    assert!(calls[1].contains("--http-chunk-size 10M"), "{calls:?}");
    assert!(calls[1].contains("--concurrent-fragments"), "{calls:?}");
    assert!(!calls[1].contains("--throttled-rate"), "{calls:?}");
}

/// Streams yt-dlp would fetch in pieces (HLS here) are left to it whole.
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_in_pieces_is_left_to_ytdlp() {
    use_fake_ytdlp();
    let dir = tempdir();
    let (case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "s", "title": "S", "ext": "mp4", "format_id": "hls-1",
            "protocol": "m3u8_native", "url": "http://127.0.0.1:9/s.m3u8",
            "filename": "__STAGE__/S [s].mp4"
        }),
        b"downloaded by yt-dlp",
    );

    let done = ytdlp::download_page(&client(), &page, &options(dir.path()), None)
        .await
        .expect("the page did not download");

    assert_eq!(std::fs::read(&done.path).unwrap(), b"downloaded by yt-dlp");
    let calls = calls(&case);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[1].contains("--load-info-json"), "{calls:?}");
}

/// A pause while dm fetches the streams, then a resume: the page is read
/// again (its addresses may have expired), and the bytes already fetched
/// are kept rather than fetched again.
#[tokio::test(flavor = "multi_thread")]
async fn a_paused_video_resumes_from_what_dm_fetched() {
    use_fake_ytdlp();
    let data: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i * 11 % 241) as u8).collect();
    let seen = Seen::default();
    let addr = serve_files(Served {
        files: Arc::new(vec![("/video", data.clone())]),
        refuse: None,
        seen: seen.clone(),
        pace: Some(1024 * 1024),
    })
    .await;
    let dir = tempdir();
    let (case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "p", "title": "P", "ext": "mp4", "format_id": "18",
            "protocol": "https", "url": format!("http://{addr}/video"),
            "filename": "__STAGE__/P [p].mp4"
        }),
        b"not this",
    );
    let opts = options(dir.path());
    let running = {
        let (page, opts) = (page.clone(), opts.clone());
        tokio::spawn(async move { ytdlp::download_page(&client(), &page, &opts, None).await })
    };
    // Paused once the streams are being fetched, not after a guess at how
    // long that takes to begin.
    for _ in 0..400 {
        if seen.0.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    opts.control.pause();
    let first = running.await.unwrap();
    assert!(
        matches!(first, Err(dm_core::Error::Paused)),
        "got {first:?}"
    );
    let asked_before = seen.0.lock().unwrap().len();

    let again = dm_core::Options {
        control: dm_core::Control::new(),
        ..opts
    };
    let done = ytdlp::download_page(&client(), &page, &again, None)
        .await
        .expect("the resume failed");
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        data,
        "the resume spliced wrong"
    );
    assert_eq!(
        calls(&case).len(),
        2,
        "read once a run, downloaded by dm both times"
    );
    assert!(seen.0.lock().unwrap().len() > asked_before);
}

/// A picture and sound the extension caught from a player, refused to dm
/// (as YouTube refuses a client without the player's token): the page they
/// came from is read by the video helper and fetched by dm instead, rather
/// than the row failing on a video the browser could play.
#[tokio::test(flavor = "multi_thread")]
async fn a_caught_pair_that_is_refused_is_fetched_from_its_page_instead() {
    use_fake_ytdlp();
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i * 5 % 239) as u8).collect();
    let addr = serve_files(Served {
        files: Arc::new(vec![("/video", data.clone())]),
        refuse: Some("/picture"),
        seen: Seen::default(),
        pace: None,
    })
    .await;
    let dir = tempdir();
    let (case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "w", "title": "Watched", "ext": "mp4", "format_id": "22",
            "protocol": "https", "url": format!("http://{addr}/video"),
            "filename": "__STAGE__/Watched [w].mp4"
        }),
        b"not this",
    );
    let opts = dm_core::Options {
        sound: Some(format!("http://{addr}/sound")),
        referrer: Some(page),
        attempts: 1,
        ..options(dir.path())
    };

    let done = dm_core::download(&client(), &format!("http://{addr}/picture"), &opts)
        .await
        .expect("the caught pair was not fetched from its page");

    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    assert_eq!(done.path.file_name().unwrap(), "Watched [w].mp4");
    assert_eq!(calls(&case).len(), 1, "read once, fetched by dm");
}

/// The panel's list read the page (`list_choices`); the download a click on
/// it starts chooses its format from that reading, rather than having
/// yt-dlp read the page again, which takes seconds on YouTube.
#[tokio::test(flavor = "multi_thread")]
async fn a_download_reuses_the_reading_the_panels_list_made() {
    use_fake_ytdlp();
    let readings = Box::leak(Box::new(tempdir()));
    ytdlp::keep_readings_in(readings.path().to_path_buf());
    let data: Vec<u8> = (0..512 * 1024).map(|i| (i * 3 % 233) as u8).collect();
    let addr = serve_files(Served {
        files: Arc::new(vec![("/video", data.clone())]),
        refuse: None,
        seen: Seen::default(),
        pace: None,
    })
    .await;
    let dir = tempdir();
    let (case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "r", "title": "Read", "ext": "mp4", "format_id": "18",
            "protocol": "https", "url": format!("http://{addr}/video"),
            "filename": "__STAGE__/Read [r].mp4", "formats": []
        }),
        b"not this",
    );

    ytdlp::list_choices(&page, None, None, None)
        .await
        .expect("the list was not read");
    let done = ytdlp::download_page(&client(), &page, &options(dir.path()), None)
        .await
        .expect("the page did not download");

    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    let calls = calls(&case);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].contains("-J"), "{calls:?}");
    assert!(
        calls[1].contains("--load-info-json") && !calls[1].contains("watch?case="),
        "the page was read again for the download: {calls:?}"
    );
}

/// A click on a quality while the page is still being read behind the list
/// (the list came from the page's own data) waits for that reading, rather
/// than having yt-dlp read the page a second time at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_download_started_during_the_warming_read_waits_for_it() {
    use_fake_ytdlp();
    let readings = Box::leak(Box::new(tempdir()));
    ytdlp::keep_readings_in(readings.path().to_path_buf());
    let data: Vec<u8> = (0..256 * 1024).map(|i| (i * 7 % 251) as u8).collect();
    let addr = serve_files(Served {
        files: Arc::new(vec![("/video", data.clone())]),
        refuse: None,
        seen: Seen::default(),
        pace: None,
    })
    .await;
    let dir = tempdir();
    let (case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "w", "title": "Warm", "ext": "mp4", "format_id": "18",
            "protocol": "https", "url": format!("http://{addr}/video"),
            "filename": "__STAGE__/Warm [w].mp4", "formats": []
        }),
        b"not this",
    );
    std::fs::write(case.join("delay"), "1.5").unwrap();

    let warming = {
        let page = page.clone();
        tokio::spawn(async move { ytdlp::warm_reading(&page, None, None, None).await })
    };
    // The helper is running by now, and says so.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let started = std::time::Instant::now();
    let done = ytdlp::download_page(&client(), &page, &options(dir.path()), None)
        .await
        .expect("the page did not download");
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(800),
        "it did not wait for the reading: {:?}",
        started.elapsed()
    );
    warming.await.unwrap();

    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    let calls = calls(&case);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].contains("-J"), "{calls:?}");
    assert!(
        calls[1].contains("--load-info-json"),
        "read twice: {calls:?}"
    );
}

/// A stream of a video page that the server refuses is that stream's
/// failure: it is not handed to yt-dlp as if it were a page (two more
/// yt-dlp runs for each of a video's streams, about 1.4 s on YouTube, before
/// the helper downloaded the video from its reading as it was going to).
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_stream_is_not_handed_to_ytdlp_as_a_page() {
    use_fake_ytdlp();
    let dir = tempdir();
    let addr = serve_files(Served {
        files: Arc::new(vec![("/video", vec![1u8; 1024])]),
        refuse: Some("/video"),
        seen: Seen::default(),
        pace: None,
    })
    .await;
    let (case, _page) = case(dir.path(), serde_json::json!({ "formats": [] }), b"unused");
    let url = format!("http://{addr}/video");
    let opts = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };
    let failed = dm_core::download::download_video_stream(&client(), &url, &opts, None).await;
    assert!(
        matches!(failed, Err(dm_core::Error::Refused { status: 403, .. })),
        "{failed:?}"
    );
    assert!(
        calls(&case).is_empty(),
        "yt-dlp was run: {:?}",
        calls(&case)
    );
}

/// An address that runs out while a stream is being fetched (YouTube's do,
/// some hours on): dm reads the page again, takes the new address and goes
/// on from what it has, rather than starting over with yt-dlp.
#[tokio::test(flavor = "multi_thread")]
async fn an_address_that_runs_out_mid_download_is_renewed_and_resumed() {
    use axum::extract::Query;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use_fake_ytdlp();
    const SIZE: usize = 6 * 1024 * 1024;
    let data: Arc<Vec<u8>> = Arc::new((0..SIZE).map(|i| (i * 7 % 251) as u8).collect());
    let expired = Arc::new(AtomicBool::new(false));
    let fresh_bytes = Arc::new(AtomicUsize::new(0));
    let (served, gone, counted) = (data.clone(), expired.clone(), fresh_bytes.clone());
    let app = axum::Router::new().route(
        "/video",
        get(
            move |Query(q): Query<HashMap<String, String>>, headers: HeaderMap| {
                let (data, gone, counted) = (served.clone(), gone.clone(), counted.clone());
                async move {
                    let token = q.get("t").map(String::as_str).unwrap_or("");
                    if token == "old" && gone.load(Ordering::SeqCst) {
                        return Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .body(Body::empty())
                            .unwrap();
                    }
                    let (a, b) = headers
                        .get("range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("bytes="))
                        .and_then(|v| v.split_once('-'))
                        .map(|(a, b)| {
                            (
                                a.parse::<usize>().unwrap(),
                                b.parse::<usize>().unwrap_or(SIZE - 1).min(SIZE - 1),
                            )
                        })
                        .unwrap_or((0, SIZE - 1));
                    if token == "new" && b > a {
                        counted.fetch_add(b - a + 1, Ordering::SeqCst);
                    }
                    // About a megabyte a second to a response.
                    const CHUNK: usize = 64 * 1024;
                    let pieces: Vec<(usize, usize)> = (a..=b)
                        .step_by(CHUNK)
                        .map(|s| (s, (s + CHUNK - 1).min(b)))
                        .collect();
                    let body = Body::from_stream(futures_util::StreamExt::then(
                        futures_util::stream::iter(pieces),
                        move |(s, e)| {
                            let data = data.clone();
                            async move {
                                tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                                Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&data[s..=e]))
                            }
                        },
                    ));
                    Response::builder()
                        .status(206)
                        .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                        .header("content-length", b - a + 1)
                        .header("etag", "\"v\"")
                        .body(body)
                        .unwrap()
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempdir();
    let reading = |token: &str| {
        serde_json::json!({
            "id": "e", "title": "Expiring", "ext": "mp4", "format_id": "18",
            "protocol": "https", "url": format!("http://{addr}/video?t={token}"),
            "filename": "__STAGE__/Expiring [e].mp4", "filesize": SIZE,
            "downloader_options": {"http_chunk_size": 1024 * 1024}
        })
    };
    let (case, page) = case(dir.path(), reading("old"), b"not this");
    std::fs::write(case.join("info.next.json"), reading("new").to_string()).unwrap();

    let flip = expired.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        flip.store(true, Ordering::SeqCst);
    });
    let opts = dm_core::Options {
        connections: 2,
        ..options(dir.path())
    };
    let done = ytdlp::download_page(&client(), &page, &opts, None)
        .await
        .expect("the renewed download failed");

    assert!(
        std::fs::read(&done.path).unwrap() == *data,
        "the renewed download is not the file"
    );
    let calls = calls(&case);
    assert_eq!(
        calls.iter().filter(|c| c.contains(" -O ")).count(),
        2,
        "the page was read again once: {calls:?}"
    );
    assert!(
        calls.iter().all(|c| c.contains(" -O ")),
        "yt-dlp was handed the download: {calls:?}"
    );
    let again = fresh_bytes.load(Ordering::SeqCst);
    assert!(
        again > 0 && again < SIZE,
        "what was fetched before was fetched again: {again} of {SIZE} bytes under the new address"
    );
}

/// A stream is asked with every header its reading names, on the probe and
/// on each range; a download of the same host started apart from it is not,
/// and the headers dm owns (`Range`, `Accept-Encoding`) stay dm's.
#[tokio::test(flavor = "multi_thread")]
async fn a_streams_headers_are_sent_for_it_alone() {
    type Asked = Arc<Mutex<Vec<(String, HeaderMap)>>>;
    use_fake_ytdlp();
    const SIZE: usize = 2 * 1024 * 1024;
    let data: Arc<Vec<u8>> = Arc::new((0..SIZE).map(|i| (i * 13 % 247) as u8).collect());
    let asked: Asked = Arc::default();
    let (served, log) = (data.clone(), asked.clone());
    let app = axum::Router::new().fallback(get(move |uri: Uri, headers: HeaderMap| {
        let (data, log) = (served.clone(), log.clone());
        async move {
            log.lock()
                .unwrap()
                .push((uri.path().to_string(), headers.clone()));
            let (a, b) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    (
                        a.parse::<usize>().unwrap(),
                        b.parse::<usize>().unwrap_or(SIZE - 1).min(SIZE - 1),
                    )
                })
                .unwrap_or((0, SIZE - 1));
            Response::builder()
                .status(206)
                .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"v\"")
                .body(Body::from(bytes::Bytes::copy_from_slice(&data[a..=b])))
                .unwrap()
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempdir();
    let (_case, page) = case(
        dir.path(),
        serde_json::json!({
            "id": "h", "title": "Headed", "ext": "mp4", "format_id": "18",
            "protocol": "https", "url": format!("http://{addr}/a.mp4"),
            "filename": "__STAGE__/Headed [h].mp4",
            "http_headers": {
                "X-Stream": "yes", "Accept-Language": "xx-YY", "Sec-Fetch-Mode": "navigate",
                "Range": "bytes=5-6", "Accept-Encoding": "gzip", "Cookie": "theirs=1"
            }
        }),
        b"not this",
    );
    let done = ytdlp::download_page(&client(), &page, &options(dir.path()), None)
        .await
        .expect("the stream was not fetched");
    assert!(std::fs::read(&done.path).unwrap() == *data, "not the file");

    let other = tempdir();
    dm_core::download(
        &client(),
        &format!("http://{addr}/b.bin"),
        &options(other.path()),
    )
    .await
    .expect("the plain download failed");

    let asked = asked.lock().unwrap();
    let (stream, plain): (Vec<_>, Vec<_>) = asked.iter().partition(|(path, _)| path == "/a.mp4");
    assert!(stream.len() >= 2, "a probe and ranges: {}", stream.len());
    for (_, h) in &stream {
        assert_eq!(h.get("x-stream").map(|v| v.as_bytes()), Some(&b"yes"[..]));
        assert_eq!(
            h.get("accept-language").map(|v| v.as_bytes()),
            Some(&b"xx-YY"[..])
        );
        assert_eq!(
            h.get("accept-encoding").map(|v| v.as_bytes()),
            Some(&b"identity"[..])
        );
        assert_ne!(
            h.get("range").map(|v| v.as_bytes()),
            Some(&b"bytes=5-6"[..])
        );
    }
    assert!(!plain.is_empty());
    for (_, h) in &plain {
        assert!(h.get("x-stream").is_none(), "leaked into another download");
        assert!(
            h.get("accept-language").is_none(),
            "leaked into another download"
        );
        assert!(
            h.get("cookie").is_none(),
            "a cookie leaked into another download"
        );
    }
}
