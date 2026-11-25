// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_stream_is_named_as_the_engine_names_it() {
    let url = "http://h/streams/hls/index.m3u8";
    assert_eq!(stream_name(url, None, true).as_deref(), Some("hls.mp4"));
    assert_eq!(stream_name(url, None, false).as_deref(), Some("hls.ts"));
    assert_eq!(stream_name("http://h/file.zip", None, true), None);
}

/// A stream is suggested under the name it will be saved as,
/// the engine's own: its folder's name, as an MP4 where it is remuxed.
#[test]
fn a_stream_is_suggested_under_the_name_it_is_saved_as() {
    let url = "http://127.0.0.1:8000/hls/index.m3u8";
    assert_eq!(stream_name(url, None, true).as_deref(), Some("hls.mp4"));
    assert_eq!(stream_name(url, None, false).as_deref(), Some("hls.ts"));
    assert_eq!(
        stream_name("http://h/live/lecture.m3u8?t=1", None, true).as_deref(),
        Some("lecture.mp4")
    );
    assert_eq!(
        stream_name("http://h/x", Some("application/vnd.apple.mpegurl"), true).as_deref(),
        Some("x.mp4")
    );
    assert_eq!(stream_name("http://h/file.zip", None, true), None);
}

/// A server that is not there is a link that failed, said in words; a
/// file's name and size come from the server that answers.
#[test]
fn links_are_asked_about_and_what_fails_is_said() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, dir.path());
    let addr = runtime.block_on(async {
        let app =
            axum::Router::new().route("/f.zip", axum::routing::get(|| async { vec![0u8; 1000] }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    let (tell, heard) = std::sync::mpsc::channel();
    let file = format!("http://{addr}/f.zip");
    probe_links(
        &backend,
        vec![file.clone(), "http://127.0.0.1:9/gone.bin".into()],
        String::new(),
        None,
        move |url, found| {
            let _ = tell.send((url, found));
        },
    );
    let mut got = std::collections::HashMap::new();
    for _ in 0..2 {
        let (url, found) = heard.recv_timeout(Duration::from_secs(25)).unwrap();
        got.insert(url, found);
    }
    match &got[&file] {
        LinkFound::Known {
            name,
            kind,
            size,
            page,
        } => {
            assert_eq!(name, "f.zip");
            assert_eq!(*kind, dm_core::Category::Archives);
            assert_eq!(*size, Some(1000));
            assert!(!page);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(&got["http://127.0.0.1:9/gone.bin"], LinkFound::Failed(why) if !why.is_empty())
    );
}

/// What Download details is told for a stream, a watch page that turned
/// the quick look away, and a file that is not there.
#[test]
fn a_stream_a_turned_away_page_and_a_missing_file_are_each_told_as_such() {
    use axum::http::{header::CONTENT_TYPE, StatusCode};
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, dir.path());
    let addr = runtime.block_on(async {
        let app = axum::Router::new()
            .route(
                "/live/index.m3u8",
                axum::routing::get(|| async {
                    (
                        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
                        "#EXTM3U\n#EXTINF:1,\nsegment.ts\n#EXT-X-ENDLIST\n",
                    )
                }),
            )
            .route(
                "/watch",
                axum::routing::get(|| async { StatusCode::FORBIDDEN }),
            )
            .route(
                "/gone.zip",
                axum::routing::get(|| async { StatusCode::NOT_FOUND }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    let ask = |path: &str| {
        let (tell, heard) = std::sync::mpsc::channel();
        probe_later(
            &backend,
            format!("http://{addr}{path}"),
            String::new(),
            None,
            true,
            move |probed| {
                let _ = tell.send(probed);
            },
        );
        heard.recv_timeout(Duration::from_secs(25)).unwrap()
    };
    assert_eq!(
        ask("/live/index.m3u8"),
        Probed {
            name: Some("live.mp4".into()),
            ..Probed::default()
        }
    );
    assert_eq!(
        ask("/watch"),
        Probed {
            page: true,
            ..Probed::default()
        },
        "a bot check is the video reader's, not a problem"
    );
    let gone = ask("/gone.zip");
    assert!(!gone.page);
    assert!(gone.problem.is_some_and(|why| why.contains("404")));
}

/// A plain `http://` address to a server on the internet has an `https://` form to try first;
/// one to this computer or the local network, or one already secure, has none (gaps row H6).
#[test]
fn a_typed_http_address_to_the_internet_is_tried_over_https_first() {
    assert_eq!(
        secure_form("http://example.com/files/a.zip?x=1").as_deref(),
        Some("https://example.com/files/a.zip?x=1")
    );
    for none in [
        "https://example.com/a.zip",
        "http://127.0.0.1:8080/a.zip",
        "http://[::1]/a.zip",
        "http://localhost/a.zip",
        "http://nas.local/a.zip",
        "http://printer/a.zip",
        "ftp://example.com/a.zip",
    ] {
        assert_eq!(secure_form(none), None, "{none}");
    }
}
