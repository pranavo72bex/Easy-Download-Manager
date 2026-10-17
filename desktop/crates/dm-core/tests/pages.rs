//! A link to a document that is itself a page — `notes.html`, `feed.xml` —
//! is that document, whether or not yt-dlp is installed; a watch page with
//! no extension is still yt-dlp's.
//!
//! One test at a time: whether yt-dlp is there is said by an environment
//! variable, which is process-wide.

mod common;

use axum::routing::get;
use axum::Router;
use common::{options, serve, tempdir};

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const PAGE: &str = "<!doctype html><title>notes</title><p>the page itself</p>";

async fn site() -> std::net::SocketAddr {
    let html = || async { ([("content-type", "text/html; charset=utf-8")], PAGE) };
    serve(
        Router::new()
            .route("/docs/notes.html", get(html))
            .route("/watch", get(html))
            .route("/view.php", get(html)),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_named_as_a_document_is_saved_without_yt_dlp() {
    let _turn = ONE_AT_A_TIME.lock().await;
    std::env::set_var("DM_YTDLP", "");
    let addr = site().await;
    let dir = tempdir();
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/docs/notes.html"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), PAGE.as_bytes());
    assert_eq!(done.path.file_name().unwrap(), "notes.html");

    // A watch page, with no extension or a script's, is not a document:
    // without yt-dlp it is refused, not saved as its HTML.
    for watch in ["watch", "view.php?v=1"] {
        let dir = tempdir();
        let err = dm_core::download(
            &dm_core::client(),
            &format!("http://{addr}/{watch}"),
            &options(dir.path()),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("yt-dlp"), "{watch}: {err}");
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_page_named_as_a_document_is_saved_when_yt_dlp_does_not_know_the_site() {
    use std::os::unix::fs::PermissionsExt;
    let _turn = ONE_AT_A_TIME.lock().await;
    let dir = tempdir();
    let script = dir.path().join("yt-dlp");
    std::fs::write(
        &script,
        "#!/bin/sh\necho \"ERROR: Unsupported URL: $1\" >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("DM_YTDLP", &script);
    std::env::set_var("DM_FFMPEG", "");
    let addr = site().await;

    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/docs/notes.html"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), PAGE.as_bytes());

    // A watch page yt-dlp does not know is still its refusal to report.
    let err = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/watch"),
        &options(dir.path()),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("does not know"), "{err}");
}
