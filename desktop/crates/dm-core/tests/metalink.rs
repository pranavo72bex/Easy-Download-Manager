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

/// A file served in ranges, with an ETag of its own, as a mirror does.
fn ranged(
    body: Vec<u8>,
    etag: &'static str,
) -> impl Fn(axum::http::HeaderMap) -> std::future::Ready<axum::response::Response> + Clone {
    move |headers: axum::http::HeaderMap| {
        let total = body.len();
        let (mut from, mut to, mut status) = (0usize, total - 1, 200);
        if let Some(range) = headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
        {
            let (a, b) = range.split_once('-').unwrap_or(("0", ""));
            from = a.parse().unwrap_or(0);
            to = b.parse().map_or(total - 1, |b: usize| b.min(total - 1));
            status = 206;
        }
        let mut res = axum::response::Response::builder()
            .status(status)
            .header("accept-ranges", "bytes")
            .header("etag", etag)
            .header("content-length", to - from + 1);
        if status == 206 {
            res = res.header("content-range", format!("bytes {from}-{to}/{total}"));
        }
        std::future::ready(
            res.body(axum::body::Body::from(body[from..=to].to_vec()))
                .unwrap(),
        )
    }
}

/// The first mirror serves another file of the same length. The document's
/// digest says which is the file: the download goes on to the next mirror
/// rather than failing, and fails only when none of them matches.
#[tokio::test(flavor = "multi_thread")]
async fn a_mirror_serving_other_bytes_is_passed_over_for_one_that_matches_the_digest() {
    use sha2::{Digest, Sha256};
    let size = 6 * 1024 * 1024;
    let good: Vec<u8> = (0..size).map(|i| (i * 7 % 251) as u8).collect();
    let wrong: Vec<u8> = (0..size).map(|i| (i * 11 % 241) as u8).collect();
    let digest = hex::encode(Sha256::digest(&good));
    let app = Router::new()
        .route("/m1/f.bin", get(ranged(wrong, "\"wrong\"")))
        .route("/m2/f.bin", get(ranged(good.clone(), "\"good\"")));
    let mirrors = serve(app).await;
    let listing = |hash: String, urls: String| {
        let doc = format!(
            r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink"><file name="f.bin"><size>{size}</size>
            <hash type="sha-256">{hash}</hash>{urls}</file></metalink>"#
        );
        serve(Router::new().route(
            "/f.meta4",
            get(move || {
                let doc = doc.clone();
                async move { ([("content-type", "application/metalink4+xml")], doc) }
            }),
        ))
    };
    let both = format!(
        r#"<url priority="1">http://{mirrors}/m1/f.bin</url><url priority="2">http://{mirrors}/m2/f.bin</url>"#
    );
    let listing_addr = listing(digest.clone(), both).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 6,
        ..options(dir.path())
    };
    let done = dm_core::download::download(
        &dm_core::client(),
        &format!("http://{listing_addr}/f.meta4"),
        &opts,
    )
    .await
    .expect("the good mirror was not used after the first one's bytes were refused");
    assert!(
        std::fs::read(&done.path).unwrap() == good,
        "the file is not the one the digest names"
    );

    // With only the wrong one, the digest refuses it and nothing is kept.
    let only_wrong = format!(r#"<url>http://{mirrors}/m1/f.bin</url>"#);
    let listing_addr = listing(digest, only_wrong).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 6,
        ..options(dir.path())
    };
    let refused = dm_core::download::download(
        &dm_core::client(),
        &format!("http://{listing_addr}/f.meta4"),
        &opts,
    )
    .await;
    assert!(refused.is_err(), "bytes that fail the digest were kept");
}
