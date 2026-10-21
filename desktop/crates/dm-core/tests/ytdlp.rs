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
    if !ytdlp::available() {
        eprintln!("no yt-dlp installed — skipping the yt-dlp end-to-end test");
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
/// serves its better qualities, fetched by the real yt-dlp with no ffmpeg
/// anywhere: dm joins the two halves itself into one MP4.
#[tokio::test(flavor = "multi_thread")]
async fn a_picture_and_sound_served_apart_arrive_as_one_mp4_with_no_ffmpeg() {
    if !ytdlp::available() {
        eprintln!("no yt-dlp installed — skipping the yt-dlp end-to-end test");
        return;
    }
    std::env::set_var("DM_FFMPEG", "");
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
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
    std::env::remove_var("DM_FFMPEG");
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

/// An HLS stream fetched by the real yt-dlp with no ffmpeg: yt-dlp saves the
/// transport stream under an `.mp4` name, and dm makes it the MP4 it says
/// it is. (An ffmpeg makes the stream; dm is told there is none.)
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_hls_stream_yt_dlp_saved_as_ts_is_made_a_real_mp4() {
    if !ytdlp::available() {
        eprintln!("no yt-dlp installed — skipping the yt-dlp end-to-end test");
        return;
    }
    let made = tempdir();
    let ok = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=3:size=128x72:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=3",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-f",
            "hls",
            "-hls_time",
            "1",
            "-hls_list_size",
            "0",
        ])
        .arg(made.path().join("show.m3u8"))
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("no ffmpeg here to make a stream with; skipped");
        return;
    }
    let folder = made.path().to_path_buf();
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
    std::env::set_var("DM_FFMPEG", "");
    let dir = tempdir();
    let finished = ytdlp::download_ytdlp(
        &format!("http://{addr}/show.m3u8"),
        &options(dir.path()),
        None,
    )
    .await;
    std::env::remove_var("DM_FFMPEG");
    let finished = finished.expect("yt-dlp failed");
    let saved = std::fs::read(&finished.path).unwrap();
    assert_eq!(saved.get(4..8), Some(&b"ftyp"[..]), "{:?}", finished.path);
    assert_eq!(finished.bytes, saved.len() as u64);
}
