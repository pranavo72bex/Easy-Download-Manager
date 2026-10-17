//! A Metalink's mirrors measured against the document that lists them.

mod common;

use axum::routing::get;
use axum::Router;
use common::{options, serve, tempdir};

/// The document states the length; the best-ranked mirror serves a stale,
/// shorter copy. The document is the reference, so the stale mirror is the
/// one left out — not the two current ones, which is what happened when the
/// first mirror to answer set the length.
#[tokio::test(flavor = "multi_thread")]
async fn the_documents_length_outranks_a_stale_first_mirror() {
    let app = Router::new()
        .route("/m1/f.bin", get(|| async { b"STALE".to_vec() }))
        .route("/m2/f.bin", get(|| async { b"0123456789".to_vec() }))
        .route("/m3/f.bin", get(|| async { b"0123456789".to_vec() }));
    let mirrors = serve(app).await;
    let doc = format!(
        r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink"><file name="f.bin"><size>10</size>
        <url priority="1">http://{mirrors}/m1/f.bin</url>
        <url priority="2">http://{mirrors}/m2/f.bin</url>
        <url priority="3">http://{mirrors}/m3/f.bin</url></file></metalink>"#
    );
    let listing = serve(Router::new().route(
        "/f.meta4",
        get(move || {
            let doc = doc.clone();
            async move { ([("content-type", "application/metalink4+xml")], doc) }
        }),
    ))
    .await;

    let dir = tempdir();
    let done = dm_core::download::download(
        &dm_core::client(),
        &format!("http://{listing}/f.meta4"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(done.bytes, 10);
    assert_eq!(std::fs::read(&done.path).unwrap(), b"0123456789");
    assert_eq!(
        done.mirrors, 2,
        "the stale mirror was used, or a good one was not"
    );
}

/// Every mirror disagrees with the document: nothing is fetched, and the
/// reason says so rather than a file of the wrong length being kept.
#[tokio::test(flavor = "multi_thread")]
async fn no_mirror_agreeing_with_the_document_is_refused() {
    let app = Router::new().route("/m1/f.bin", get(|| async { b"STALE".to_vec() }));
    let mirrors = serve(app).await;
    let doc = format!(
        r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink"><file name="f.bin"><size>10</size>
        <url>http://{mirrors}/m1/f.bin</url></file></metalink>"#
    );
    let listing = serve(Router::new().route(
        "/f.meta4",
        get(move || {
            let doc = doc.clone();
            async move { ([("content-type", "application/metalink4+xml")], doc) }
        }),
    ))
    .await;
    let dir = tempdir();
    let err = dm_core::download::download(
        &dm_core::client(),
        &format!("http://{listing}/f.meta4"),
        &options(dir.path()),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("length"), "{err}");
}
