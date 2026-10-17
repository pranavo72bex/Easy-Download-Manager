//! The cases where a download could finish with the wrong bytes, or not
//! finish at all, that an audit against the download-manager checklist found.
//!
//! Each serves its own file from a loopback server and compares what was
//! saved with what was served, byte for byte.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download};
use futures_util::StreamExt;

use common::{options, serve, tempdir};

fn body(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// A server that honours ranges and `If-Range` against one ETag.
fn ranged(data: &[u8], etag: &str, headers: &HeaderMap) -> Response {
    let total = data.len();
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .map(|v| {
            let (a, b) = v.split_once('-').unwrap();
            let a: usize = a.parse().unwrap();
            let b = b.parse::<usize>().unwrap_or(total - 1).min(total - 1);
            (a, b)
        });
    let if_range = headers.get("if-range").and_then(|v| v.to_str().ok());
    match range {
        Some((a, b)) if if_range.is_none_or(|v| v == etag) => Response::builder()
            .status(206)
            .header("content-range", format!("bytes {a}-{b}/{total}"))
            .header("etag", etag)
            .body(Body::from(data[a..=b].to_vec()))
            .unwrap(),
        _ => Response::builder()
            .status(200)
            .header("etag", etag)
            .header("content-length", total)
            .body(Body::from(data.to_vec()))
            .unwrap(),
    }
}

/// Bytes on disk under `dir`'s part files.
fn staged(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir.join("parts"))
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("part-"))
                .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0)
}

#[tokio::test]
async fn a_file_republished_between_runs_is_not_spliced_into_the_old_parts() {
    /* If-Range guards one run. Across a pause, the second run probed the new
     * file, sent its new ETag as the condition, and the server — rightly —
     * served the new bytes as a range, appended to parts of the old file.
     * The result matched neither version and was reported Completed. */
    let old = Arc::new(body(4 * 1024 * 1024, 1));
    let new = Arc::new(body(4 * 1024 * 1024, 99));
    let dir = tempdir();

    let served = old.clone();
    let slow = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let data = served.clone();
            async move {
                let resp = ranged(&data, "\"v1\"", &h);
                if resp.status() != 206 {
                    return resp;
                }
                let (parts, b) = resp.into_parts();
                let bytes = axum::body::to_bytes(b, usize::MAX).await.unwrap();
                let chunks: Vec<_> = bytes
                    .chunks(16 * 1024)
                    .map(bytes::Bytes::copy_from_slice)
                    .collect();
                let s = futures_util::stream::iter(chunks).then(|c| async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok::<_, std::io::Error>(c)
                });
                Response::from_parts(parts, Body::from_stream(s))
            }
        }),
    );
    let first = serve(slow).await;
    let o = options(dir.path());
    let control = o.control.clone();
    let parts = dir.path().to_path_buf();
    tokio::spawn(async move {
        while staged(&parts) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        control.pause();
    });
    let paused = download(&client(), &format!("http://{first}/f.bin"), &o).await;
    assert!(paused.is_err(), "the first run was meant to be paused");
    assert!(staged(dir.path()) > 0, "nothing was staged to splice into");

    let served = new.clone();
    let republished = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let data = served.clone();
            async move { ranged(&data, "\"v2\"", &h) }
        }),
    );
    let second = serve(republished).await;
    let done = download(
        &client(),
        &format!("http://{second}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the resumed download failed");
    let saved = std::fs::read(&done.path).unwrap();
    assert!(
        saved == *new,
        "the saved file is not the file that was served"
    );
}

#[tokio::test]
async fn a_server_without_ranges_is_fetched_again_from_the_start() {
    /* No Range header is sent to such a server, but the writer still sought
     * to the length already staged: the whole body landed at that offset.
     * A failed first run and a Retry left a part that only grew. */
    let data = Arc::new(body(200_000, 7));
    let hits = Arc::new(AtomicUsize::new(0));
    let (served, counted) = (data.clone(), hits.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(move || {
            let d = served.clone();
            let n = counted.fetch_add(1, Ordering::SeqCst);
            async move {
                // The first fetch after the probe dies a quarter of the way in.
                if n == 1 {
                    let first = bytes::Bytes::copy_from_slice(&d[..50_000]);
                    // Spaced out, so the first half reaches the disk before
                    // the connection dies rather than being lost with it.
                    let s = futures_util::stream::iter(vec![
                        Ok::<_, std::io::Error>(first),
                        Err(std::io::Error::other("dropped")),
                    ])
                    .then(|x| async move {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        x
                    });
                    return Response::builder()
                        .status(200)
                        .header("content-length", d.len())
                        .body(Body::from_stream(s))
                        .unwrap();
                }
                Response::builder()
                    .status(200)
                    .header("content-length", d.len())
                    .body(Body::from(d.to_vec()))
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let url = format!("http://{addr}/f.bin");

    // Whether the drop is retried within the run or the row is retried,
    // what is saved must be the file.
    let mut result = download(&client(), &url, &options(dir.path())).await;
    if result.is_err() {
        result = download(&client(), &url, &options(dir.path())).await;
    }
    let done = result.expect("a retried download from a server without ranges failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
}

#[tokio::test]
async fn a_file_of_unknown_length_downloads() {
    /* An unknown length is planned as one range ending at u64::MAX, and its
     * span overflowed: a panic in a debug build, a range "finished" without
     * a byte fetched in a release one. Every chunked response failed. */
    let data = Arc::new(body(100_000, 1));
    let served = data.clone();
    let app = axum::Router::new().route(
        "/f.bin",
        get(move || {
            let d = served.clone();
            async move {
                let chunks: Vec<_> = d
                    .chunks(10_000)
                    .map(|c| Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(c)))
                    .collect();
                Response::builder()
                    .status(200)
                    .body(Body::from_stream(futures_util::stream::iter(chunks)))
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("a download with no Content-Length failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
}

#[tokio::test]
async fn a_connection_dropped_mid_range_is_retried() {
    /* A dropped body surfaces as a decode/body error, which was not counted
     * as transient: one reset on one of eight ranges failed the download. */
    let data = Arc::new(body(3 * 1024 * 1024, 3));
    let hits = Arc::new(AtomicUsize::new(0));
    let (served, counted) = (data.clone(), hits.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let d = served.clone();
            let n = counted.fetch_add(1, Ordering::SeqCst);
            async move {
                let resp = ranged(&d, "\"v1\"", &h);
                if n != 2 {
                    return resp;
                }
                let (parts, b) = resp.into_parts();
                let bytes = axum::body::to_bytes(b, usize::MAX).await.unwrap();
                let s = futures_util::stream::iter(vec![
                    Ok::<_, std::io::Error>(bytes.slice(..1000)),
                    Err(std::io::Error::other("dropped")),
                ])
                .then(|x| async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    x
                });
                Response::from_parts(parts, Body::from_stream(s))
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("one dropped connection failed the whole download");
    assert!(
        hits.load(Ordering::SeqCst) > 4,
        "the dropped range was never asked for again"
    );
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
}

#[tokio::test]
async fn an_empty_file_on_a_server_with_ranges_downloads() {
    /* `bytes=0-0` of an empty file is unsatisfiable, and the probe took the
     * 416 for a refusal, though its Content-Range gives the length as 0. */
    let app = axum::Router::new().route(
        "/empty.txt",
        get(|h: HeaderMap| async move {
            if h.contains_key("range") {
                Response::builder()
                    .status(416)
                    .header("content-range", "bytes */0")
                    .body(Body::empty())
                    .unwrap()
            } else {
                Response::builder()
                    .status(200)
                    .header("content-length", 0)
                    .body(Body::empty())
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let started = std::time::Instant::now();
    let done = download(
        &client(),
        &format!("http://{addr}/empty.txt"),
        &options(dir.path()),
    )
    .await
    .expect("an empty file failed");
    assert_eq!(std::fs::metadata(&done.path).unwrap().len(), 0);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "an empty file sat through retries: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn an_empty_file_without_ranges_does_not_wait_out_retries() {
    let app = axum::Router::new().route(
        "/empty.txt",
        get(|| async {
            Response::builder()
                .status(200)
                .header("content-length", 0)
                .body(Body::empty())
                .unwrap()
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let started = std::time::Instant::now();
    let done = download(
        &client(),
        &format!("http://{addr}/empty.txt"),
        &options(dir.path()),
    )
    .await
    .expect("an empty file failed");
    assert_eq!(std::fs::metadata(&done.path).unwrap().len(), 0);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "an empty file sat through retries: {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_download_folder_that_cannot_be_written_is_said_before_fetching() {
    /* The folder was first written to when the finished file was placed, so
     * a read-only folder cost the whole transfer and then said only
     * "disk: Permission denied" — not which folder, not what to do. */
    use std::os::unix::fs::PermissionsExt;
    let data = Arc::new(body(3 * 1024 * 1024, 4));
    let hits = Arc::new(AtomicUsize::new(0));
    let (served, counted) = (data.clone(), hits.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let d = served.clone();
            counted.fetch_add(1, Ordering::SeqCst);
            async move { ranged(&d, "\"v1\"", &h) }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let o = options(dir.path());
    std::fs::create_dir_all(&o.dest_dir).unwrap();
    std::fs::set_permissions(&o.dest_dir, std::fs::Permissions::from_mode(0o500)).unwrap();

    let result = download(&client(), &format!("http://{addr}/f.bin"), &o).await;
    std::fs::set_permissions(&o.dest_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let error = result.expect_err("a download into a read-only folder succeeded");
    let said = error.to_string();
    assert!(
        said.contains(&o.dest_dir.display().to_string()),
        "the folder is not named: {said}"
    );
    assert!(
        hits.load(Ordering::SeqCst) <= 1,
        "the file was fetched before the folder was found unwritable"
    );
}

#[tokio::test]
async fn a_server_that_stops_serving_ranges_to_many_connections_is_fetched_on_one() {
    /* Wikimedia's caches, found against the real site: several ranges at
     * once are each answered with the whole file under the same ETag, a
     * lone range is served, and a range of every byte is answered whole.
     * That was reported as "the file changed on the server" and the
     * download failed. */
    let data = Arc::new(body(8_000_000, 11));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let whole = Arc::new(AtomicUsize::new(0));
    let (served, busy, answered_whole) = (data.clone(), in_flight.clone(), whole.clone());
    let app = axum::Router::new().route(
        "/f.jpg",
        get(move |headers: HeaderMap| {
            let (d, busy, answered_whole) = (served.clone(), busy.clone(), answered_whole.clone());
            async move {
                let others = busy.fetch_add(1, Ordering::SeqCst);
                // Long enough for the ranges of one plan to overlap.
                tokio::time::sleep(Duration::from_millis(150)).await;
                let every_byte = format!("bytes=0-{}", d.len() - 1);
                let whole_asked = headers
                    .get("range")
                    .is_some_and(|r| r == every_byte.as_str());
                let response = if (others > 0 && headers.contains_key("range")) || whole_asked {
                    answered_whole.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(200)
                        .header("etag", "\"v1\"")
                        .header("content-length", d.len())
                        .body(Body::from(d.to_vec()))
                        .unwrap()
                } else {
                    ranged(&d, "\"v1\"", &headers)
                };
                busy.fetch_sub(1, Ordering::SeqCst);
                response
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let url = format!("http://{addr}/f.jpg");
    let many = dm_core::Options {
        connections: 8,
        ..options(dir.path())
    };

    let done = download(&client(), &url, &many)
        .await
        .expect("a server that stopped serving ranges failed the download");
    assert!(
        whole.load(Ordering::SeqCst) > 0,
        "the server never answered a range whole"
    );
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
    assert_eq!(
        done.connections, 1,
        "the second run was not on one connection"
    );
}

#[tokio::test]
async fn a_site_that_refuses_dms_agent_is_asked_again_by_its_name_alone() {
    /* W3Schools, found against the real site: any agent with "github" in
     * it is refused with 403, and the agent with dm's contact address has
     * it. Wikimedia refuses the plain name. So a 403 is asked once more by
     * the plain name, and the whole download goes out under it. */
    let data = Arc::new(body(3_000_000, 21));
    let refused = Arc::new(AtomicUsize::new(0));
    let (served, counted) = (data.clone(), refused.clone());
    let app = axum::Router::new().route(
        "/v.mp4",
        get(move |headers: HeaderMap| {
            let (d, counted) = (served.clone(), counted.clone());
            async move {
                let agent = headers
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if agent.to_ascii_lowercase().contains("github") {
                    counted.fetch_add(1, Ordering::SeqCst);
                    return Response::builder()
                        .status(403)
                        .body(Body::from("bots not welcome"))
                        .unwrap();
                }
                ranged(&d, "\"v\"", &headers)
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let url = format!("http://{addr}/v.mp4");
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("a site that refuses dm's full agent failed the download");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
    assert_eq!(
        refused.load(Ordering::SeqCst),
        1,
        "asked with the refused agent more than once"
    );
}

#[tokio::test]
async fn a_site_that_takes_only_browsers_is_asked_as_a_browser_last() {
    /* On one network W3Schools refused dm's full agent and its plain name
     * alike, though a browser there played the same video. After both, a
     * 403 is asked once more as a browser asks. */
    let data = Arc::new(body(2_500_000, 33));
    let served = data.clone();
    let app = axum::Router::new().route(
        "/clip.mp4",
        get(move |headers: HeaderMap| {
            let d = served.clone();
            async move {
                let agent = headers
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if !agent.starts_with("Mozilla/") {
                    return Response::builder()
                        .status(403)
                        .body(Body::from("browsers only"))
                        .unwrap();
                }
                ranged(&d, "\"c\"", &headers)
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/clip.mp4"),
        &options(dir.path()),
    )
    .await
    .expect("a site that takes only browsers failed the download");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
}
