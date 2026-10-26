//! Handing a page to yt-dlp, end to end, against a local server.
//!
//! The wrapper's pure parts — quality to format, progress parsing — are unit
//! tested; this is the part that cannot be faked, that dm actually runs the
//! installed yt-dlp, reads its progress and its final path, and places what
//! it produced. yt-dlp's generic extractor downloads a direct URL as a single
//! format, so a plain file on a loopback server is a real run of the whole
//! path without reaching the public network.
//!
//! Skipped, not failed, where no yt-dlp is installed: dm works without one,
//! and so does its suite.

use std::sync::Arc;

use axum::body::Body;
use axum::http::header;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use dm_core::ytdlp;

mod common;
use common::{options, serve, tempdir};

#[tokio::test(flavor = "multi_thread")]
async fn a_page_is_handed_to_ytdlp_and_the_file_is_placed() {
    if !ytdlp::usable() {
        eprintln!("no usable yt-dlp installed — skipping the yt-dlp end-to-end test");
        return;
    }

    let body: Vec<u8> = (0..640 * 1024).map(|i| (i * 13 % 251) as u8).collect();
    let served = Arc::new(body.clone());

    // Served as a plain file, which yt-dlp's generic extractor takes as a
    // single direct format — the same shape a page's resolved media URL is.
    let app = Router::new().route(
        "/clip.mp4",
        get({
            let served = Arc::clone(&served);
            move || {
                let served = Arc::clone(&served);
                async move {
                    Response::builder()
                        .header(header::CONTENT_TYPE, "video/mp4")
                        .body(Body::from(served.to_vec()))
                        .unwrap()
                }
            }
        }),
    );
    let addr = serve(app).await;
    let url = format!("http://{addr}/clip.mp4");

    let dir = tempdir();
    let finished = ytdlp::download_ytdlp(&url, &options(dir.path()), None)
        .await
        .expect("yt-dlp download failed");

    let saved = std::fs::read(&finished.path).expect("the placed file is where it was reported");
    assert_eq!(
        saved, body,
        "the file yt-dlp produced is not what was served"
    );
    assert_eq!(finished.bytes, body.len() as u64);
    // Placed under the download folder, not left in the staging directory.
    assert!(
        finished.path.starts_with(dir.path().join("out")),
        "the file was not moved into the download folder: {:?}",
        finished.path
    );
}

/// A DASH manifest whose picture and sound are separate, as a video site
/// serves its better qualities, fetched by the real yt-dlp: one MP4
/// arrives, joined by dm where yt-dlp leaves the two halves apart.
#[tokio::test(flavor = "multi_thread")]
async fn a_picture_and_sound_served_apart_arrive_as_one_mp4() {
    if !ytdlp::usable() {
        eprintln!("no usable yt-dlp installed — skipping the yt-dlp end-to-end test");
        return;
    }
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4");
    let picture = std::fs::read(fixtures.join("picture.f137.mp4")).unwrap();
    let sound = std::fs::read(fixtures.join("sound.f140.m4a")).unwrap();
    let manifest = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT1S" minBufferTime="PT1S" profiles="urn:mpeg:dash:profile:isoff-on-demand:2011">
 <Period>
  <AdaptationSet mimeType="video/mp4">
   <Representation id="137" codecs="avc1.64000a" width="64" height="36" bandwidth="20000"><BaseURL>picture.mp4</BaseURL></Representation>
  </AdaptationSet>
  <AdaptationSet mimeType="audio/mp4">
   <Representation id="140" codecs="mp4a.40.2" audioSamplingRate="22050" bandwidth="32000"><BaseURL>sound.m4a</BaseURL></Representation>
  </AdaptationSet>
 </Period>
</MPD>"#;
    let file = |bytes: Vec<u8>, kind: &'static str| {
        get(move || {
            let bytes = bytes.clone();
            async move {
                Response::builder()
                    .header(header::CONTENT_TYPE, kind)
                    .body(Body::from(bytes))
                    .unwrap()
            }
        })
    };
    let app = Router::new()
        .route(
            "/clip.mpd",
            file(manifest.as_bytes().to_vec(), "application/dash+xml"),
        )
        .route("/picture.mp4", file(picture, "video/mp4"))
        .route("/sound.m4a", file(sound, "audio/mp4"));
    let addr = serve(app).await;
    let url = format!("http://{addr}/clip.mpd");

    let dir = tempdir();
    let finished = ytdlp::download_ytdlp(&url, &options(dir.path()), None).await;
    let finished = finished.expect("yt-dlp and dm's join failed");

    assert_eq!(
        finished.path.extension().and_then(|e| e.to_str()),
        Some("mp4")
    );
    let joined = std::fs::read(&finished.path).unwrap();
    let has = |kind: &[u8]| joined.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 2, "picture and sound are both in it");
    assert!(has(b"avc1") >= 1 && has(b"mp4a") >= 1);
    // Nothing of the halves is left beside it.
    let left: Vec<_> = std::fs::read_dir(finished.path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
}

/// An HLS stream fetched by the real yt-dlp: where yt-dlp saves the
/// transport stream under an `.mp4` name, dm makes it the MP4 it says it
/// is. The stream is the one kept beside the tests.
#[tokio::test(flavor = "multi_thread")]
async fn an_hls_stream_yt_dlp_saved_as_ts_is_made_a_real_mp4() {
    if !ytdlp::usable() {
        eprintln!("no usable yt-dlp installed — skipping the yt-dlp end-to-end test");
        return;
    }
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/hls/h264");
    let app = Router::new().fallback(move |uri: axum::http::Uri| {
        let folder = folder.clone();
        async move {
            let name = uri.path().trim_start_matches('/').to_string();
            let kind = if name.ends_with(".m3u8") {
                "application/vnd.apple.mpegurl"
            } else {
                "video/mp2t"
            };
            match std::fs::read(folder.join(&name)) {
                Ok(bytes) => Response::builder()
                    .header(header::CONTENT_TYPE, kind)
                    .body(Body::from(bytes))
                    .unwrap(),
                Err(_) => Response::builder().status(404).body(Body::empty()).unwrap(),
            }
        }
    });
    let addr = serve(app).await;
    let dir = tempdir();
    let finished = ytdlp::download_ytdlp(
        &format!("http://{addr}/show.m3u8"),
        &options(dir.path()),
        None,
    )
    .await;
    let finished = finished.expect("yt-dlp failed");
    let saved = std::fs::read(&finished.path).unwrap();
    assert_eq!(saved.get(4..8), Some(&b"ftyp"[..]), "{:?}", finished.path);
    assert_eq!(finished.bytes, saved.len() as u64);
    let has = |kind: &[u8]| saved.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 2, "picture and sound are both in it");
    assert!(has(b"avc1") >= 1 && has(b"mp4a") >= 1);
}

/// The real yt-dlp reads a page and dm fetches what it found
/// (`download_page`): the file arrives whole, on more than one connection.
#[tokio::test(flavor = "multi_thread")]
async fn a_page_the_real_ytdlp_reads_is_fetched_by_dm_on_many_connections() {
    if !ytdlp::usable() {
        eprintln!("no usable yt-dlp installed — skipping the yt-dlp end-to-end test");
        return;
    }
    let body: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i * 13 % 251) as u8).collect();
    let served = Arc::new(body.clone());
    let ports = Arc::new(std::sync::Mutex::new(
        std::collections::HashSet::<u16>::new(),
    ));
    let seen = Arc::clone(&ports);
    let app = Router::new().route(
        "/clip.mp4",
        get(
            move |axum::extract::ConnectInfo(from): axum::extract::ConnectInfo<
                std::net::SocketAddr,
            >,
                  headers: axum::http::HeaderMap| {
                let (served, seen) = (Arc::clone(&served), Arc::clone(&seen));
                async move {
                    seen.lock().unwrap().insert(from.port());
                    let size = served.len();
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
                        .header(header::CONTENT_TYPE, "video/mp4")
                        .header("content-range", format!("bytes {a}-{b}/{size}"))
                        .header("content-length", b - a + 1)
                        .body(Body::from(served[a..=b].to_vec()))
                        .unwrap()
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let url = format!("http://{addr}/clip.mp4");

    let dir = tempdir();
    let finished = ytdlp::download_page(&dm_core::client(), &url, &options(dir.path()), None)
        .await
        .expect("the page did not download");
    assert_eq!(std::fs::read(&finished.path).unwrap(), body);
    assert!(finished.path.starts_with(dir.path().join("out")));
    assert!(
        ports.lock().unwrap().len() > 2,
        "fetched on one connection: yt-dlp downloaded it rather than dm"
    );
}

/// With the real yt-dlp: the panel's list reads the page (`list_choices`)
/// and the download chooses its format from that reading, so the page's
/// server is asked once for the page and then only for the file's bytes.
#[tokio::test(flavor = "multi_thread")]
async fn the_real_ytdlps_list_reading_serves_the_download() {
    if !ytdlp::usable() {
        eprintln!("no usable yt-dlp installed — skipping the yt-dlp end-to-end test");
        return;
    }
    let readings = Box::leak(Box::new(tempdir()));
    ytdlp::keep_readings_in(readings.path().to_path_buf());
    let body: Vec<u8> = (0..300 * 1024).map(|i| (i * 17 % 251) as u8).collect();
    let served = Arc::new(body.clone());
    let whole = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&whole);
    let app = Router::new().route(
        "/clip.mp4",
        get(move |headers: axum::http::HeaderMap| {
            let (served, counted) = (Arc::clone(&served), Arc::clone(&counted));
            async move {
                let size = served.len();
                match headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                {
                    Some((a, b)) => {
                        let a: usize = a.parse().unwrap();
                        let b = b.parse::<usize>().unwrap_or(size - 1).min(size - 1);
                        Response::builder()
                            .status(206)
                            .header(header::CONTENT_TYPE, "video/mp4")
                            .header("content-range", format!("bytes {a}-{b}/{size}"))
                            .body(Body::from(served[a..=b].to_vec()))
                            .unwrap()
                    }
                    None => {
                        // A request with no range is yt-dlp reading the page.
                        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Response::builder()
                            .header(header::CONTENT_TYPE, "video/mp4")
                            .body(Body::from(served.to_vec()))
                            .unwrap()
                    }
                }
            }
        }),
    );
    let addr = serve(app).await;
    let url = format!("http://{addr}/clip.mp4");

    ytdlp::list_choices(&url, None, None, None)
        .await
        .expect("the list was not read");
    let read_for_list = whole.load(std::sync::atomic::Ordering::SeqCst);
    assert!(read_for_list > 0, "yt-dlp's reading never asked the server");
    let dir = tempdir();
    let finished = ytdlp::download_page(&dm_core::client(), &url, &options(dir.path()), None)
        .await
        .expect("the page did not download");

    assert_eq!(std::fs::read(&finished.path).unwrap(), body);
    assert_eq!(
        whole.load(std::sync::atomic::Ordering::SeqCst),
        read_for_list,
        "yt-dlp read the page again for the download"
    );
}
