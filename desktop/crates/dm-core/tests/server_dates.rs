//! A finished file can carry the date its server says it last changed,
//! where asked, and keeps the time it was fetched where not.

mod common;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;

const CHANGED: &str = "Tue, 15 Nov 1994 08:12:31 GMT";

async fn dated(headers: HeaderMap) -> Response {
    let body = vec![7u8; 300_000];
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
        .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)));
    let builder = Response::builder().header("last-modified", CHANGED);
    match range {
        Some((a, b)) => {
            let b = b.min(body.len() - 1);
            builder
                .status(206)
                .header("content-range", format!("bytes {a}-{b}/{}", body.len()))
                .body(Body::from(body[a..=b].to_vec()))
                .unwrap()
        }
        None => builder.body(Body::from(body)).unwrap(),
    }
}

#[tokio::test]
async fn a_finished_file_takes_the_servers_date_only_where_asked() {
    let addr = common::serve(axum::Router::new().route("/f.bin", get(dated))).await;
    let url = format!("http://{addr}/f.bin");
    let changed = std::time::UNIX_EPOCH + std::time::Duration::from_secs(784_887_151);

    let dir = common::tempdir();
    let options = dm_core::Options {
        server_dates: true,
        ..common::options(dir.path())
    };
    let done = dm_core::download(&dm_core::client(), &url, &options)
        .await
        .unwrap();
    let when = std::fs::metadata(&done.path).unwrap().modified().unwrap();
    assert_eq!(when, changed, "the server's date was not set");

    let dir = common::tempdir();
    let done = dm_core::download(&dm_core::client(), &url, &common::options(dir.path()))
        .await
        .unwrap();
    let when = std::fs::metadata(&done.path).unwrap().modified().unwrap();
    assert!(
        when > changed,
        "the server's date was set without being asked"
    );
}
