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
        assert!(err.to_string().contains("video helper"), "{watch}: {err}");
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
    let addr = site().await;

    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/docs/notes.html"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), PAGE.as_bytes());

    // Asked for as media (the browser's panel or media menu), the page is
    // not saved as its HTML in place of the video.
    let media = dm_core::Options {
        media: true,
        ..options(&dir.path().join("media"))
    };
    let err = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/docs/notes.html"),
        &media,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("does not know"), "{err}");

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

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_page_named_as_a_document_is_fetched_by_dm_when_yt_dlp_is_refused() {
    /* yt-dlp goes first — some video sites end a watch page in `.html` —
     * but only its "unknown site" used to fall back to dm's own client: a
     * page served only to a request naming its referrer, which yt-dlp was
     * refused (a 403), failed the row. Any refusal now falls back. */
    use axum::http::{HeaderMap, StatusCode};
    use std::os::unix::fs::PermissionsExt;
    let _turn = ONE_AT_A_TIME.lock().await;
    let dir = tempdir();
    let ran = dir.path().join("ran");
    let script = dir.path().join("yt-dlp");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ntouch '{}'\necho 'ERROR: HTTP Error 403: Forbidden' >&2\nexit 1\n",
            ran.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("DM_YTDLP", &script);
    const PAGE_URL: &str = "http://example.test/reports";
    let gated = |headers: HeaderMap| async move {
        if headers.get("referer").and_then(|r| r.to_str().ok()) != Some(PAGE_URL) {
            return (StatusCode::FORBIDDEN, [("content-type", "text/html")], "no").into_response();
        }
        ([("content-type", "text/html; charset=utf-8")], PAGE).into_response()
    };
    use axum::response::IntoResponse;
    let addr = serve(Router::new().route("/docs/report.html", get(gated))).await;

    let downloads = dir.path().join("downloads");
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/docs/report.html"),
        &dm_core::Options {
            referrer: Some(PAGE_URL.into()),
            ..options(&downloads)
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), PAGE.as_bytes());
    assert!(ran.exists(), "yt-dlp was not asked first");
}
