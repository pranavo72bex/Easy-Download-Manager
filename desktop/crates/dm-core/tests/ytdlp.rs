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
