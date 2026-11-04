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

/// Bytes staged under `dir`: what each range's ledger says is on disk (and
/// any part file an earlier dm left).
fn staged(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir.join("parts"))
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with("done-") {
                        std::fs::read_to_string(e.path())
                            .ok()
                            .and_then(|t| t.trim().parse::<u64>().ok())
                            .unwrap_or(0)
                    } else if name.starts_with("part-") {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    } else {
                        0
                    }
                })
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

/// Ranges with no ETag and no Last-Modified: nothing names the version.
fn ranged_unnamed(data: &[u8], headers: &HeaderMap) -> Response {
    let (parts, body) = ranged(data, "", headers).into_parts();
    let mut resp = Response::from_parts(parts, body);
    resp.headers_mut().remove("etag");
    resp
}

#[tokio::test]
async fn a_file_changed_while_paused_on_a_server_naming_no_version_is_not_spliced() {
    /* Same length, no validator: the probe cannot tell the versions apart,
     * so only the bytes can. The resume check fetched the end of each part
     * again, found them different, and took that for a power cut — cut the
     * part back and carried on, finishing a file half one version and half
     * the other, reported Completed. */
    let old = Arc::new(body(4 * 1024 * 1024, 1));
    let new = Arc::new(body(4 * 1024 * 1024, 99));
    let dir = tempdir();

    let served = old.clone();
    let slow = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let data = served.clone();
            async move {
                let resp = ranged_unnamed(&data, &h);
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
        while staged(&parts) < 256 * 1024 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        control.pause();
    });
    let paused = download(&client(), &format!("http://{first}/f.bin"), &o).await;
    assert!(paused.is_err(), "the first run was meant to be paused");

    let served = new.clone();
    let changed = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let data = served.clone();
            async move { ranged_unnamed(&data, &h) }
        }),
    );
    let second = serve(changed).await;
    let url = format!("http://{second}/f.bin");
    // Found changed, it starts again from nothing, and gets the new file.
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("fetching the changed file afresh failed");
    assert!(
        std::fs::read(&done.path).unwrap() == *new,
        "a file of two versions was called finished"
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
                    // Spaced out, so that quarter reaches the disk before the
                    // connection dies rather than being lost with it.
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
    if std::fs::write(o.dest_dir.join("probe"), b"").is_ok() {
        return; // root writes past permissions (a container): nothing to test
    }

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
                if agent.to_ascii_lowercase().contains("github") || agent.contains("Trident") {
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
        2,
        "asked with each refused agent (the browser's, then dm's full one) more than once"
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
                if !agent.starts_with("Mozilla/") || agent.contains("Trident") {
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

#[tokio::test]
async fn a_file_served_as_a_web_page_is_saved_as_the_file_it_is() {
    /* A .zip with a careless text/html type went to the video reader as a page, and
     * was saved as "page [html].unknown_video". */
    let data = Arc::new(body(300 * 1024, 7));
    let served = data.clone();
    let app = axum::Router::new().route(
        "/files/page.zip",
        get(move |h: HeaderMap| {
            let data = served.clone();
            async move {
                let (mut parts, b) = ranged(&data, "\"z\"", &h).into_parts();
                parts
                    .headers
                    .insert("content-type", "text/html".parse().unwrap());
                Response::from_parts(parts, b)
            }
        }),
    );
    let at = serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{at}/files/page.zip"),
        &options(dir.path()),
    )
    .await
    .expect("the zip was not downloaded");
    assert!(done.path.ends_with("page.zip"), "{}", done.path.display());
    assert!(std::fs::read(&done.path).unwrap() == *data);
}

#[tokio::test]
async fn a_web_page_answering_for_a_file_is_said_to_be_one() {
    // A sign-in page, or an expired link's apology, where a .zip was asked.
    let app = axum::Router::new().route(
        "/files/report.zip",
        get(|| async {
            Response::builder()
                .header("content-type", "text/html; charset=utf-8")
                .body(Body::from(
                    "<!DOCTYPE html><html><head><title>Sign in</title></head></html>",
                ))
                .unwrap()
        }),
    );
    let at = serve(app).await;
    let dir = tempdir();
    let failed = download(
        &client(),
        &format!("http://{at}/files/report.zip"),
        &options(dir.path()),
    )
    .await
    .expect_err("a sign-in page was saved as the zip");
    assert!(
        failed.to_string().contains("web page rather than the file"),
        "{failed}"
    );
}

#[tokio::test]
async fn a_failed_download_leaves_no_connection_still_fetching() {
    /* One range failing for good fails the download. The other connections
     * were left running behind it — writing parts and the plan under a
     * download already over, or under the fresh attempt a changed file
     * starts. Once the failure is returned, nothing more is fetched. */
    let data = Arc::new(body(8 * 1024 * 1024, 3));
    let sent = Arc::new(AtomicUsize::new(0));
    let (served, counted) = (data.clone(), sent.clone());
    let poisoned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let poison_flag = poisoned.clone();
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let (data, counted) = (served.clone(), counted.clone());
            let poisoned = poison_flag.clone();
            async move {
                let range = h
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let is_probe = range == "bytes=0-0" || range == "bytes=0-";
                // Every non-probe data request is answered as another
                // version of the file: a failure for good, retries included.
                if !is_probe && range.starts_with("bytes=") {
                    poisoned.store(true, Ordering::SeqCst);
                    return Response::builder()
                        .status(200)
                        .header("etag", "\"other\"")
                        .header("content-length", data.len())
                        .body(Body::from(data.to_vec()))
                        .unwrap();
                }
                let resp = ranged(&data, "\"v\"", &h);
                if resp.status() != 206 || is_probe {
                    return resp;
                }
                let (parts, b) = resp.into_parts();
                let bytes = axum::body::to_bytes(b, usize::MAX).await.unwrap();
                let chunks: Vec<_> = bytes
                    .chunks(16 * 1024)
                    .map(bytes::Bytes::copy_from_slice)
                    .collect();
                let s = futures_util::stream::iter(chunks).then(move |c| {
                    let counted = counted.clone();
                    async move {
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        counted.fetch_add(c.len(), Ordering::SeqCst);
                        Ok::<_, std::io::Error>(c)
                    }
                });
                Response::from_parts(parts, Body::from_stream(s))
            }
        }),
    );
    let at = serve(app).await;
    let dir = tempdir();
    let failed = download(
        &client(),
        &format!("http://{at}/f.bin"),
        &options(dir.path()),
    )
    .await;
    assert!(failed.is_err(), "a refused range did not fail the download");
    let then = sent.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let later = sent.load(Ordering::SeqCst);
    assert!(
        later <= then + 64 * 1024,
        "connections kept fetching after the download failed: {then} then {later} bytes"
    );
}

#[tokio::test]
async fn a_weak_etag_is_not_sent_as_a_range_condition() {
    /* A strict server answers `If-Range` with a weak ETag by sending the
     * whole file, as the standard says; every range came back whole and
     * the download fell to one connection. */
    let data = Arc::new(body(4 * 1024 * 1024, 5));
    let whole_answers = Arc::new(AtomicUsize::new(0));
    let (served, counted) = (data.clone(), whole_answers.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |h: HeaderMap| {
            let (data, counted) = (served.clone(), counted.clone());
            async move {
                let weak = h
                    .get("if-range")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.starts_with("W/"));
                if weak {
                    counted.fetch_add(1, Ordering::SeqCst);
                    let mut plain = HeaderMap::new();
                    plain.insert("x", "y".parse().unwrap());
                    return ranged(&data, "W/\"v\"", &plain);
                }
                ranged(&data, "W/\"v\"", &h)
            }
        }),
    );
    let at = serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{at}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the download failed");
    assert!(std::fs::read(&done.path).unwrap() == *data);
    assert_eq!(
        whole_answers.load(Ordering::SeqCst),
        0,
        "a weak ETag was sent as If-Range"
    );
    assert!(done.connections > 1, "it fell back to one connection");
}

#[tokio::test]
async fn a_web_page_that_answered_for_a_file_is_saved_as_html_when_asked_to() {
    /* Reference A asks whether to save it as an HTML page; a person who said
     * yes (`Options::accept_page`) has the page kept as `<name>.html`. */
    let page = "<html><head><title>Sign in</title></head><body>expired</body></html>";
    let app = axum::Router::new().route(
        "/files/report.zip",
        get(move || async move {
            Response::builder()
                .header("content-type", "text/html; charset=utf-8")
                .body(Body::from(page))
                .unwrap()
        }),
    );
    let at = serve(app).await;
    let dir = tempdir();
    let url = format!("http://{at}/files/report.zip");

    let refused = download(&client(), &url, &options(dir.path()))
        .await
        .expect_err("a page was saved as the zip without being asked");
    assert!(refused
        .to_string()
        .contains("web page rather than the file"));

    let asked = dm_core::Options {
        accept_page: true,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &asked)
        .await
        .expect("the page was not saved");
    assert_eq!(done.path.file_name().unwrap(), "report.html");
    assert_eq!(std::fs::read_to_string(&done.path).unwrap(), page);
}

/// A server that ignores `If-Range` and republishes the file mid-download:
/// from the third request on it serves another version, under another ETag,
/// still as 206 to every range (gaps row 98). Whatever dm does, it must not
/// finish with a file made of both.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_republished_mid_download_under_a_server_ignoring_if_range_is_not_spliced() {
    const SIZE: usize = 24 * 1024 * 1024;
    let old = Arc::new(body(SIZE, 1));
    let new = Arc::new(body(SIZE, 2));
    let asked = Arc::new(AtomicUsize::new(0));
    let (served_old, served_new, counter) = (old.clone(), new.clone(), asked.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |headers: HeaderMap| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let (data, etag) = if n < 2 {
                (served_old.clone(), "\"v1\"")
            } else {
                (served_new.clone(), "\"v2\"")
            };
            async move {
                let total = data.len();
                let (a, b) = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| v.split_once('-'))
                    .map(|(a, b)| {
                        let a: usize = a.parse().unwrap_or(0);
                        (a, b.parse::<usize>().unwrap_or(total - 1).min(total - 1))
                    })
                    .unwrap_or((0, total - 1));
                // About 8 MB/s a connection, so helpers join while it runs.
                let chunks: Vec<Vec<u8>> =
                    data[a..=b].chunks(64 * 1024).map(<[u8]>::to_vec).collect();
                let stream = futures_util::stream::iter(chunks).then(|c| async move {
                    tokio::time::sleep(Duration::from_millis(8)).await;
                    Ok::<_, std::io::Error>(c)
                });
                Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes {a}-{b}/{total}"))
                    .header("content-length", b - a + 1)
                    .header("accept-ranges", "bytes")
                    .header("etag", etag)
                    .body(Body::from_stream(stream))
                    .unwrap()
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let outcome = download(
        &client(),
        &format!("http://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await;
    if let Ok(done) = outcome {
        let saved = std::fs::read(&done.path).unwrap();
        assert!(
            saved == *old || saved == *new,
            "a file of both versions was finished ({} requests)",
            asked.load(Ordering::SeqCst)
        );
    }
}

/// Behind a redirect, every piece asks the address it landed on, as
/// reference A does (gaps row 98): the redirecting address is asked once, by
/// the first request, and never again for a range.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_a_redirect_every_piece_asks_the_address_it_landed_on() {
    let data = Arc::new(body(24 * 1024 * 1024, 7));
    let at_start = Arc::new(AtomicUsize::new(0));
    let at_end = Arc::new(AtomicUsize::new(0));
    let (start_count, end_count, served) = (at_start.clone(), at_end.clone(), data.clone());
    let app = axum::Router::new()
        .route(
            "/go/f.bin",
            get(move || {
                start_count.fetch_add(1, Ordering::SeqCst);
                async {
                    Response::builder()
                        .status(302)
                        .header("location", "/files/f.bin")
                        .body(Body::empty())
                        .unwrap()
                }
            }),
        )
        .route(
            "/files/f.bin",
            get(move |headers: HeaderMap| {
                end_count.fetch_add(1, Ordering::SeqCst);
                let served = served.clone();
                async move { ranged(&served, "\"one\"", &headers) }
            }),
        );
    let addr = serve(app).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("http://{addr}/go/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the download behind a redirect failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), *data);
    assert!(
        done.connections > 1,
        "one connection only: no pieces to check"
    );
    assert_eq!(
        at_start.load(Ordering::SeqCst),
        1,
        "a piece asked the redirecting address"
    );
    assert!(at_end.load(Ordering::SeqCst) > 1);
}
