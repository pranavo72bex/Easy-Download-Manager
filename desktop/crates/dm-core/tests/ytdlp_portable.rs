//! Where yt-dlp's release has no standalone build for this computer, its
//! portable build (one file of Python) is fetched and run by the computer's
//! own Python (docs/gaps.md row 38). Its own test binary: it says through
//! the environment where the releases are, which Python to use, and that
//! only fetched tools count.
#![cfg(unix)]

mod common;

use std::sync::Arc;

use axum::routing::get;
use sha2::Digest as _;

use common::{options, serve, tempdir};

/// yt-dlp's portable build, as far as this test needs it: a Python program
/// that saves a file where it is told and names it as yt-dlp does.
const PORTABLE: &str = "#!/usr/bin/env python3\nimport os, sys\nargs = sys.argv[1:]\nout = os.path.dirname(args[args.index('-o') + 1])\nos.makedirs(out, exist_ok=True)\np = os.path.join(out, 'portable [x].mp4')\nopen(p, 'w').write('portable')\nopen(args[args.index('--print-to-file') + 2], 'w').write(p + '\\n')\n";

#[tokio::test(flavor = "multi_thread")]
async fn with_no_standalone_build_the_portable_one_is_fetched_and_run_by_the_computers_python() {
    let python = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v python3"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let python = python.trim().to_string();
    assert!(!python.is_empty(), "the test needs a python3");

    let zipapp = Arc::new(PORTABLE.as_bytes().to_vec());
    let digest = hex::encode(sha2::Sha256::digest(zipapp.as_slice()));
    // The release names no build for this computer, only the portable one.
    let sums = format!("{digest}  yt-dlp\n");
    let served = zipapp.clone();
    let app = axum::Router::new()
        .route("/y/SHA2-256SUMS", get(move || async move { sums }))
        .route(
            "/y/yt-dlp",
            get(move || async move { served.as_ref().clone() }),
        );
    let base = format!("http://{}", serve(app).await);

    let dir = tempdir();
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    dm_core::tools::set_folder(tools.clone());
    std::env::set_var("DM_ONLY_FETCHED_TOOLS", "1");
    std::env::set_var("DM_YTDLP_RELEASES", format!("{base}/y"));
    std::env::set_var("DM_PYTHON", &python);

    let fetched = dm_core::tools::ensure(&dm_core::client(), dm_core::tools::Tool::YtDlp)
        .await
        .expect("the portable build");
    assert_eq!(fetched, tools.join("yt-dlp"));
    assert!(
        tools.join("yt-dlp.python").is_file(),
        "the Python to run it with is noted beside it"
    );

    let done = dm_core::ytdlp::download_ytdlp(
        "https://www.youtube.com/watch?v=abc",
        &options(dir.path()),
        None,
    )
    .await
    .expect("the download, through the portable build");
    assert_eq!(std::fs::read(&done.path).unwrap(), b"portable");
}
