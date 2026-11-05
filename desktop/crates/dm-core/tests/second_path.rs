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

/// The address the second path goes out from: this computer's own on a network, which the
/// loopback server answers as it answers 127.0.0.1. Windows refuses a connection to the
/// loopback from another interface's address, and its loopback answers the whole of 127/8, so
/// there the second path is 127.0.0.2.
fn second_address() -> Option<IpAddr> {
    if cfg!(windows) {
        return "127.0.0.2".parse().ok();
    }
    dm_core::interfaces()
        .into_iter()
        .map(|i| i.address)
        .find(IpAddr::is_ipv4)
}

/// One test at a time: the second path is the process's.
static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn a_download_goes_out_on_both_paths_and_is_whole() {
    let _one = ONE.lock().await;
    // This computer's own address on a network, for the second path: the
    // loopback server answers it as it answers 127.0.0.1.
    let Some(other) = second_address() else {
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

    // Bound to that address alone: bound to its interface too, as Options
    // sets it by name, a Linux socket cannot reach the loopback server at
    // all (macOS's can), and the split between two paths is what is tested.
    dm_core::set_second_client(Some(
        dm_core::builder().local_address(other).build().unwrap(),
    ));
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
    dm_core::set_second_client(None);
    let done = done.unwrap();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        body,
        "the file is not whole"
    );
    // What the progress window counts each path's share from.
    assert!(
        !dm_core::on_second(&parts).is_empty(),
        "no range recorded as fetched on the second path"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.contains(&"127.0.0.1".parse::<IpAddr>().unwrap()) && seen.contains(&other),
        "both paths were not used: {seen:?}"
    );
}

/// An HLS stream's segments alternate onto the second path (`second_path::client_for_piece`,
/// gaps row 47): the server sees both source addresses, and the stream is whole.
#[tokio::test]
async fn a_streams_pieces_alternate_onto_the_second_path() {
    let _one = ONE.lock().await;
    let Some(other) = second_address() else {
        eprintln!("no network address here; skipped");
        return;
    };
    const SEGMENTS: usize = 6;
    // Transport-stream packets in shape (a sync byte every 188), each segment its own bytes.
    let segment = |n: usize| -> Vec<u8> {
        (0..348)
            .flat_map(|_| {
                let mut packet = vec![(n as u8).wrapping_add(1); 188];
                packet[0] = 0x47;
                packet
            })
            .collect()
    };
    let seen: Arc<Mutex<HashSet<IpAddr>>> = Arc::default();
    let from_ips = seen.clone();
    let app = axum::Router::new()
        .route(
            "/p.m3u8",
            get(|| async {
                let mut list = String::from(
                    "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n",
                );
                for n in 0..SEGMENTS {
                    list.push_str(&format!("#EXTINF:4,\n/s{n}.ts\n"));
                }
                list.push_str("#EXT-X-ENDLIST\n");
                Response::builder()
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from(list))
                    .unwrap()
            }),
        )
        .route(
            "/{name}",
            get(
                move |axum::extract::ConnectInfo(from): axum::extract::ConnectInfo<SocketAddr>,
                      axum::extract::Path(name): axum::extract::Path<String>| {
                    let seen = from_ips.clone();
                    async move {
                        seen.lock().unwrap().insert(from.ip());
                        let n: usize = name
                            .trim_start_matches('s')
                            .trim_end_matches(".ts")
                            .parse()
                            .unwrap_or(0);
                        Response::builder()
                            .header("content-type", "video/mp2t")
                            .body(Body::from(segment(n)))
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
    dm_core::set_second_client(Some(
        dm_core::builder().local_address(other).build().unwrap(),
    ));
    let dir = tempdir();
    let opts = options(dir.path());
    let parts = opts.parts_dir.clone();
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://127.0.0.1:{port}/p.m3u8"),
        &opts,
    )
    .await;
    dm_core::set_second_client(None);
    let done = done.unwrap();
    let whole: Vec<u8> = (0..SEGMENTS).flat_map(segment).collect();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        whole,
        "the stream is not whole"
    );
    assert!(
        !dm_core::on_second(&parts).is_empty(),
        "no piece recorded as fetched on the second path"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.contains(&"127.0.0.1".parse::<IpAddr>().unwrap()) && seen.contains(&other),
        "both paths were not used: {seen:?}"
    );
}
