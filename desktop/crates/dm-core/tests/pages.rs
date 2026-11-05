//! A link to a document that is itself a page — `notes.html`, `feed.xml` —
//! is that document; a watch page with no extension that is not YouTube is
//! refused (its video comes from the browser's caught player requests).
//!
//! One test at a time: the tests here share one server's state.

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
async fn a_page_named_as_a_document_is_saved() {
    let _turn = ONE_AT_A_TIME.lock().await;
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
    // it is refused, not saved as its HTML.
    for watch in ["watch", "view.php?v=1"] {
        let dir = tempdir();
        let err = dm_core::download(
            &dm_core::client(),
            &format!("http://{addr}/{watch}"),
            &options(dir.path()),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("web page rather than the file"),
            "{watch}: the words the window asks about (B40): {err}"
        );
        // Said yes to, it is saved as the page it is (gaps row 136).
        let dir = tempdir();
        let mut yes = options(dir.path());
        yes.accept_page = true;
        let done = dm_core::download(&dm_core::client(), &format!("http://{addr}/{watch}"), &yes)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&done.path).unwrap(), PAGE.as_bytes());
        let name = done.path.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with(".html"), "{watch}: saved as {name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_named_as_a_document_behind_a_referrer_is_fetched_by_dm() {
    /* A page served only to a request naming its referrer is fetched by
     * dm's own client with the referrer it was handed. */
    use axum::http::{HeaderMap, StatusCode};
    let _turn = ONE_AT_A_TIME.lock().await;
    let dir = tempdir();
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
}
