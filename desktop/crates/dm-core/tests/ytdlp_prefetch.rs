//! A first start with nothing to run videos with, neither installed nor
//! fetched, fetches yt-dlp in the background a little after the start, so
//! the first video finds it there (docs/gaps.md row 35). Its own test
//! binary: it says through the environment where the releases are, that
//! only fetched tools count, and that "a little after" is now.
#![cfg(unix)]

mod common;

use axum::routing::get;
use sha2::Digest as _;

use common::{serve, tempdir};

#[tokio::test(flavor = "multi_thread")]
async fn a_first_start_with_nothing_fetches_yt_dlp_in_the_background() {
    let asset = dm_core::tools::ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH).unwrap();
    let zip = {
        use std::io::Write as _;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut out);
        let file = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        z.start_file(asset.trim_end_matches(".zip"), file).unwrap();
        z.write_all(b"#!/bin/sh\necho prefetched\n").unwrap();
        z.finish().unwrap();
        out.into_inner()
    };
    let sums = format!("{}  {asset}\n", hex::encode(sha2::Sha256::digest(&zip)));
    let app = axum::Router::new()
        .route("/y/SHA2-256SUMS", get(move || async move { sums }))
        .route(&format!("/y/{asset}"), get(move || async move { zip }));
    let base = format!("http://{}", serve(app).await);

    let dir = tempdir();
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    dm_core::tools::set_folder(tools.clone());
    std::env::set_var("DM_ONLY_FETCHED_TOOLS", "1");
    std::env::set_var("DM_YTDLP_RELEASES", format!("{base}/y"));
    std::env::set_var("DM_PREFETCH_AFTER", "0");
    assert!(
        !dm_core::ytdlp::available(),
        "nothing to run videos with, to start"
    );

    dm_core::tools::freshen(dm_core::client()).await;
    assert!(
        dm_core::ytdlp::available(),
        "yt-dlp was not fetched at the start"
    );
}
