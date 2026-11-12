// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The world a window test runs in: a server on the loopback with the files the tests fetch,
//! a runtime for the queue, a fake system and a folder of its own. As the egui window's
//! tests' (`dm-desktop/tests/window/support.rs`), from which it moved.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::response::Response;
use dm_shell::Settings;

use super::{home, Fake, Harness};

/// A server, a runtime, a fake system and a folder, for one test.
pub(crate) struct World {
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) fake: Arc<Fake>,
    pub(crate) dir: tempfile::TempDir,
    pub(crate) addr: SocketAddr,
}

pub(crate) const PAYLOAD: usize = 256 * 1024;

/// How many times `/secret.zip`, which always wants a sign-in, has been asked for: a download
/// tried again reaches the server again, however soon it fails again.
pub(crate) static SECRET_ASKED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub(crate) fn payload() -> Vec<u8> {
    (0..PAYLOAD).map(|i| (i * 7 % 251) as u8).collect()
}

/// A small ZIP, stored: a folder, a 1,000-byte file in it and a 5,000-byte one.
pub(crate) fn tiny_zip() -> Vec<u8> {
    use std::io::Write;
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.add_directory("docs/", stored).unwrap();
        zip.start_file("docs/readme.txt", stored).unwrap();
        zip.write_all(&[b'a'; 1000]).unwrap();
        zip.start_file("setup.exe", stored).unwrap();
        zip.write_all(&[b'b'; 5000]).unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}

/// `data` as a server that serves ranges answers for it.
fn ranged(headers: &axum::http::HeaderMap, data: Vec<u8>) -> Response {
    let total = data.len();
    let asked = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'));
    match asked {
        Some((from, to)) => {
            let from: usize = from.parse().unwrap_or(0);
            let to: usize = to.parse().unwrap_or(total - 1).min(total - 1);
            Response::builder()
                .status(206)
                .header("content-range", format!("bytes {from}-{to}/{total}"))
                .header("accept-ranges", "bytes")
                .header("content-length", to - from + 1)
                .body(Body::from(data[from..=to].to_vec()))
                .unwrap()
        }
        None => Response::builder()
            .header("accept-ranges", "bytes")
            .header("content-length", total)
            .body(Body::from(data))
            .unwrap(),
    }
}

/// A server with one file that arrives at once and one that never does.
pub(crate) fn world() -> World {
    home();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let addr = runtime.block_on(async {
        let app = axum::Router::new()
            .route(
                "/file.zip",
                axum::routing::get(|| async { Response::new(Body::from(payload())) }),
            )
            // A ZIP from a server that serves ranges, one from a server that
            // does not, and a file named .zip that is none.
            .route(
                "/pack.zip",
                axum::routing::get(|h: axum::http::HeaderMap| async move { ranged(&h, tiny_zip()) }),
            )
            .route(
                "/plain.zip",
                axum::routing::get(|| async { Response::new(Body::from(tiny_zip())) }),
            )
            .route(
                "/notzip.zip",
                axum::routing::get(|h: axum::http::HeaderMap| async move {
                    ranged(&h, b"this is only text, long enough to be read from its end".to_vec())
                }),
            )
            .route(
                "/page.html",
                axum::routing::get(|| async {
                    Response::new(Body::from(
                        r#"<a href="file.zip">one</a> <a href="/other.pdf">two</a> <a href="next.html">page</a>"#,
                    ))
                }),
            )
            .route(
                "/other.pdf",
                axum::routing::get(|| async { Response::new(Body::from(payload())) }),
            )
            .route(
                "/next.html",
                axum::routing::get(|| async {
                    // Said to be a page, so "Download all links" hides it.
                    Response::builder()
                        .header("content-type", "text/html")
                        .body(Body::from(
                            r#"<a href="deep.pdf">deep</a> <a href="page.html">back</a> <a href="http://elsewhere.invalid/x.html">away</a>"#,
                        ))
                        .unwrap()
                }),
            )
            .route(
                "/deep.pdf",
                axum::routing::get(|| async { Response::new(Body::from(payload())) }),
            )
            .route(
                "/secret.zip",
                axum::routing::get(|| async {
                    SECRET_ASKED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Response::builder()
                        .status(401)
                        .body(Body::from("sign in first"))
                        .unwrap()
                }),
            )
            .route(
                "/locked.zip",
                axum::routing::get(|headers: axum::http::HeaderMap| async move {
                    // Only with a sign-in, as a server behind a login is.
                    if headers.contains_key("authorization") {
                        Response::new(Body::from(payload()))
                    } else {
                        Response::builder()
                            .status(401)
                            .header("www-authenticate", "Basic realm=\"dm\"")
                            .body(Body::from("sign in first"))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/named",
                axum::routing::get(|| async {
                    // Named by its server, not by its address.
                    Response::builder()
                        .header(
                            "content-disposition",
                            "attachment; filename=\"report-2026.pdf\"",
                        )
                        .body(Body::from(payload()))
                        .unwrap()
                }),
            )
            .route(
                "/article",
                axum::routing::get(|| async {
                    // A page typed into Add URL: neither a file nor a video site's page.
                    Response::builder()
                        .header("content-type", "text/html; charset=utf-8")
                        .body(Body::from("<html><title>An article</title><body>words</body></html>"))
                        .unwrap()
                }),
            )
            .route(
                "/pagefile.zip",
                axum::routing::get(|| async {
                    // A sign-in page where a file was asked for.
                    Response::builder()
                        .header("content-type", "text/html; charset=utf-8")
                        .body(Body::from("<html><title>Sign in</title><body>expired</body></html>"))
                        .unwrap()
                }),
            )
            .route(
                "/stalled.bin",
                axum::routing::get(|| async {
                    let never = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
                    Response::builder()
                        .header("content-length", "1000000")
                        .body(Body::from_stream(never))
                        .unwrap()
                }),
            );
        // A video that sends its first 128 KiB and then nothing.
        let app = app.route(
            "/half.mp4",
            axum::routing::get(|| async {
                let first = futures_util::stream::iter([Ok::<Vec<u8>, std::io::Error>(
                    vec![9u8; 128 * 1024],
                )]);
                let never = futures_util::stream::pending();
                Response::builder()
                    .header("content-length", "1000000")
                    .body(Body::from_stream(futures_util::StreamExt::chain(
                        first, never,
                    )))
                    .unwrap()
            }),
        );
        // 8 MiB served in ranges, each sending its first 64 KiB and then
        // nothing: a download with several connections that stays under way.
        // And the same sending nothing at all: its pieces wait for good.
        let app = app
            .route(
                "/ranges-stall.bin",
                axum::routing::get(|headers: axum::http::HeaderMap| {
                    ranges_stall(64 * 1024, headers)
                }),
            )
            .route(
                "/ranges-silent.bin",
                axum::routing::get(|headers: axum::http::HeaderMap| ranges_stall(0, headers)),
            );
        // A connection that ends after a thousand bytes of a million, every
        // time, with no ranges to resume from: retried, then failed.
        let app = app.route(
            "/drops.bin",
            axum::routing::get(|| async {
                let some = futures_util::stream::iter([
                    Ok::<Vec<u8>, std::io::Error>(vec![7u8; 1000]),
                    Err(std::io::Error::other("cut")),
                ]);
                Response::builder()
                    .header("content-length", "1000000")
                    .body(Body::from_stream(some))
                    .unwrap()
            }),
        );
        // A signed link that runs out part-way: asked from the start, it
        // sends a fifth of a million bytes and is cut (more than the 64 KiB
        // a resume fetches again to check); asked to go on from part-way,
        // it has expired.
        let app = app.route(
            "/expires.bin",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                let from_start = headers
                    .get("range")
                    .and_then(|r| r.to_str().ok())
                    .is_none_or(|r| r.starts_with("bytes=0-"));
                if !from_start {
                    return Response::builder()
                        .status(403)
                        .body(Body::from("Request has expired"))
                        .unwrap();
                }
                // The engine's first look: one byte, and the size.
                if headers.get("range").is_some_and(|r| r == "bytes=0-0" || r == "bytes=0-") {
                    return Response::builder()
                        .status(206)
                        .header("content-range", "bytes 0-0/1000000")
                        .header("accept-ranges", "bytes")
                        .body(Body::from(vec![7u8]))
                        .unwrap();
                }
                use futures_util::StreamExt;
                // The bytes first, and the cut a moment after, as a
                // connection that drops does.
                let some = futures_util::stream::iter([true, false]).then(|bytes| async move {
                    if bytes {
                        Ok::<Vec<u8>, std::io::Error>(vec![7u8; 200_000])
                    } else {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Err(std::io::Error::other("cut"))
                    }
                });
                Response::builder()
                    .status(if headers.contains_key("range") { 206 } else { 200 })
                    .header("content-length", "1000000")
                    .header("content-range", "bytes 0-999999/1000000")
                    .header("accept-ranges", "bytes")
                    .body(Body::from_stream(some))
                    .unwrap()
            }),
        );
        // The same file at a fresh link, whole and in ranges.
        let app = app.route(
            "/renewed.bin",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                const SIZE: usize = 1_000_000;
                let range = headers
                    .get("range")
                    .and_then(|r| r.to_str().ok())
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.split_once('-'))
                    .and_then(|(a, b)| {
                        let a: usize = a.parse().ok()?;
                        let b: usize = b.parse().unwrap_or(SIZE - 1).min(SIZE - 1);
                        (a <= b).then_some((a, b))
                    });
                match range {
                    Some((a, b)) => Response::builder()
                        .status(206)
                        .header("content-range", format!("bytes {a}-{b}/{SIZE}"))
                        .header("accept-ranges", "bytes")
                        .body(Body::from(vec![7u8; b - a + 1]))
                        .unwrap(),
                    None => Response::builder()
                        .header("accept-ranges", "bytes")
                        .body(Body::from(vec![7u8; SIZE]))
                        .unwrap(),
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    World {
        runtime,
        fake: Arc::default(),
        dir: tempfile::tempdir().unwrap(),
        addr,
    }
}

impl World {
    /// The address of `name` on this world's server.
    pub(crate) fn url(&self, name: &str) -> String {
        format!("http://{}/{name}", self.addr)
    }

    /// The settings the tests' window starts with.
    pub(crate) fn settings(&self) -> Settings {
        // Without the progress and complete windows unless a test asks: most tests are about
        // the list, not about windows over it.
        Settings {
            download_dir: self.dir.path().join("out"),
            connections: 1,
            show_progress: false,
            show_complete: false,
            // Nor Download details before a caught or a panel download, which a new install
            // shows as reference A does, unless a test asks for it.
            ask_caught_downloads: false,
            ask_panel_downloads: false,
            // The tests find controls by their English names, whatever the machine running
            // them is set to.
            language: "en".into(),
            // Files straight into the folder and no login item, as the tests were written.
            categorize: false,
            start_on_login: false,
            // The hint that Details shows a download's progress taken as given (gaps row 174).
            hinted_details: true,
            // The browsers' bar, which a new install shows, out of the way.
            ..Settings::default()
        }
    }

    /// The window over [`World::settings`].
    pub(crate) fn harness(&self) -> Harness {
        self.harness_with(self.settings())
    }

    /// The window over `settings`, as a launch that read them would open it.
    pub(crate) fn harness_with(&self, settings: Settings) -> Harness {
        Harness::open(
            self.runtime.handle().clone(),
            Arc::clone(&self.fake),
            self.dir.path(),
            settings,
        )
    }
}

/// 8 MiB served in ranges, each sending its first `first` bytes and then
/// nothing.
async fn ranges_stall(first: u64, headers: axum::http::HeaderMap) -> Response<Body> {
    const SIZE: u64 = 8 << 20;
    let (start, end) = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(a, b)| {
            let a = a.parse::<u64>().ok()?;
            let b = b.parse::<u64>().unwrap_or(SIZE - 1).min(SIZE - 1);
            Some((a, b))
        })
        .unwrap_or((0, SIZE - 1));
    let length = end - start + 1;
    let sent: Vec<Result<Vec<u8>, std::io::Error>> = match length.min(first) {
        0 => Vec::new(),
        n => vec![Ok(vec![5u8; n as usize])],
    };
    let never = futures_util::stream::pending();
    Response::builder()
        .status(206)
        .header("accept-ranges", "bytes")
        .header("etag", "\"ranges\"")
        .header("content-range", format!("bytes {start}-{end}/{SIZE}"))
        .header("content-length", length.to_string())
        .body(Body::from_stream(futures_util::StreamExt::chain(
            futures_util::stream::iter(sent),
            never,
        )))
        .unwrap()
}
