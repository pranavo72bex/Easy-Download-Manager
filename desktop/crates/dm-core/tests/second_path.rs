//! A second network path: a ranged download's connections go out on two
//! source addresses, and the file is whole.

mod common;

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use common::{options, tempdir};

#[tokio::test]
async fn a_download_goes_out_on_both_paths_and_is_whole() {
    // This computer's own address on a network, for the second path: the
    // loopback server answers it as it answers 127.0.0.1.
    let Some(other) = dm_core::interfaces()
        .into_iter()
        .map(|i| i.address)
        .find(IpAddr::is_ipv4)
    else {
        eprintln!("no network address here; skipped");
        return;
    };
    let body: Vec<u8> = (0..8 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    let seen: Arc<Mutex<HashSet<IpAddr>>> = Arc::default();
    let (data, from_ips) = (Arc::new(body.clone()), seen.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(
            move |axum::extract::ConnectInfo(from): axum::extract::ConnectInfo<SocketAddr>,
                  headers: HeaderMap| {
                let (data, seen) = (data.clone(), from_ips.clone());
                async move {
                    seen.lock().unwrap().insert(from.ip());
                    let size = data.len();
                    let (a, b) = headers
                        .get("range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("bytes="))
                        .and_then(|v| v.split_once('-'))
                        .map(|(a, b)| {
                            (
                                a.parse::<usize>().unwrap(),
                                b.parse::<usize>().unwrap_or(size - 1).min(size - 1),
                            )
                        })
                        .unwrap_or((0, size - 1));
                    Response::builder()
                        .status(206)
                        .header("content-range", format!("bytes {a}-{b}/{size}"))
                        .header("content-length", b - a + 1)
                        .header("etag", "\"one\"")
                        .body(Body::from(data[a..=b].to_vec()))
                        .unwrap()
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });

    // As Options sets it: by the interface's name.
    let name = dm_core::interfaces()
        .into_iter()
        .find(|i| i.address == other)
        .unwrap()
        .name;
    dm_core::set_second_path(Some(&name)).unwrap();
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let parts = opts.parts_dir.clone();
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://127.0.0.1:{port}/f.bin"),
        &opts,
    )
    .await;
    dm_core::second_path::set_second_client(None);
    let done = done.unwrap();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        body,
        "the file is not whole"
    );
    // What the progress window counts each path's share from.
    assert!(
        !dm_core::second_path::on_second(&parts).is_empty(),
        "no range recorded as fetched on the second path"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.contains(&"127.0.0.1".parse::<IpAddr>().unwrap()) && seen.contains(&other),
        "both paths were not used: {seen:?}"
    );
}
