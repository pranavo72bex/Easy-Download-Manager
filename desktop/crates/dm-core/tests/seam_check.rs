// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The seams between ranges fetched on several connections are fetched a
//! second time over a fresh request and compared, as reference A does; bytes
//! that differ restart the download on one connection.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use dm_core::{client, download};

mod common;
use common::{options, tempdir};

const MB: usize = 1024 * 1024;

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

#[derive(Clone)]
struct Site {
    body: Arc<Vec<u8>>,
    /// A small range (a re-check of a seam) is answered with other bytes.
    corrupt_checks: bool,
    /// How many small ranges were asked for.
    checks: Arc<AtomicUsize>,
}

async fn answer(State(site): State<Site>, headers: HeaderMap) -> Response {
    let total = site.body.len();
    let (a, b) = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap_or(0),
                b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
            )
        })
        .unwrap_or((0, total - 1));
    let mut bytes = site.body[a..=b].to_vec();
    // A check of a seam is a few kilobytes from the middle of the file;
    // the pieces themselves are whole megabytes.
    if a > 0 && b - a < 8192 {
        site.checks.fetch_add(1, Ordering::SeqCst);
        if site.corrupt_checks {
            for byte in bytes.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }
    }
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("accept-ranges", "bytes")
        .header("content-range", format!("bytes {a}-{b}/{total}"))
        .header("content-length", b - a + 1)
        .header("etag", "\"s\"")
        .body(Body::from(bytes))
        .unwrap()
}

async fn serve(corrupt_checks: bool) -> (String, Arc<AtomicUsize>) {
    let checks = Arc::new(AtomicUsize::new(0));
    let site = Site {
        body: Arc::new(pattern(8 * MB)),
        corrupt_checks,
        checks: checks.clone(),
    };
    let app = axum::Router::new().fallback(answer).with_state(site);
    let addr = common::serve(app).await;
    (format!("http://{addr}/f.bin"), checks)
}

/// A server that, for any range request that starts at least 64 bytes into
/// the file, corrupts bytes at positions 10–60 of the returned slice but
/// leaves byte 0 intact.  The lead window fetched by a range-N request starts
/// at `seam - 64`; position 0 of the response is `seam - 64` (unchanged), but
/// positions 10–60 are corrupted.  The previous range wrote the honest bytes
/// at those same file offsets, so the assembly-time comparison catches the
/// disagreement.  A 1-byte check (position 0 only) would pass; the 64-byte
/// check catches the interior mismatch.
async fn serve_partial_corrupt() -> (String, Arc<AtomicUsize>) {
    let checks = Arc::new(AtomicUsize::new(0));
    let body = Arc::new(pattern(8 * MB));
    let checks_c = checks.clone();
    let body_c = body.clone();

    let app = axum::Router::new().fallback(move |headers: axum::http::HeaderMap| {
        let checks = checks_c.clone();
        let body = body_c.clone();
        async move {
            let total = body.len();
            let (a, b) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    (
                        a.parse::<usize>().unwrap_or(0),
                        b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
                    )
                })
                .unwrap_or((0, total - 1));
            let mut bytes = body[a..=b].to_vec();
            // Any range that starts 64+ bytes in is a lead-bearing request;
            // corrupt positions 10–60 (not 0, so a 1-byte check passes).
            if a >= 64 {
                checks.fetch_add(1, Ordering::SeqCst);
                for i in 10..60usize {
                    if let Some(byte) = bytes.get_mut(i) {
                        *byte = byte.wrapping_add(1);
                    }
                }
            }
            axum::response::Response::builder()
                .status(axum::http::StatusCode::PARTIAL_CONTENT)
                .header("accept-ranges", "bytes")
                .header("content-range", format!("bytes {a}-{b}/{total}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"s\"")
                .body(axum::body::Body::from(bytes))
                .unwrap()
        }
    });
    let addr = common::serve(app).await;
    (format!("http://{addr}/f.bin"), checks)
}

#[tokio::test]
async fn honest_ranges_pass_the_second_look_and_stay_on_several_connections() {
    let (url, checks) = serve(false).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts).await.expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(8 * MB));
    assert!(done.connections > 1, "{} connection", done.connections);
    // Every small range counts: the seams, and the last bytes of each finished
    // piece asked for again, of which a split makes more.
    let looked = checks.load(Ordering::SeqCst);
    assert!(
        (1..=24).contains(&looked),
        "{looked} seams were looked at again"
    );
}

/// A server of `pattern(size)` that records every closed range it is asked for but the probe's,
/// which asks with no `If-Range` (it is how dm learns the validator).
async fn serve_recording(size: usize) -> (String, Arc<std::sync::Mutex<Vec<(usize, usize)>>>) {
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (body, seen) = (Arc::new(pattern(size)), asked.clone());
    let app = axum::Router::new().fallback(move |headers: HeaderMap| {
        let (body, seen) = (body.clone(), seen.clone());
        async move {
            let total = body.len();
            let (a, b) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    let a = a.parse::<usize>().unwrap_or(0);
                    match b.parse::<usize>() {
                        Ok(b) => {
                            if headers.contains_key("if-range") {
                                seen.lock().unwrap().push((a, b));
                            }
                            (a, b.min(total - 1))
                        }
                        Err(_) => (a, total - 1),
                    }
                })
                .unwrap_or((0, total - 1));
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("accept-ranges", "bytes")
                .header("content-range", format!("bytes {a}-{b}/{total}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"s\"")
                .body(Body::from(body[a..=b].to_vec()))
                .unwrap()
        }
    });
    let addr = common::serve(app).await;
    (format!("http://{addr}/f.bin"), asked)
}

/// Each seam is asked for again once, in one request across it, and the file's end once: the
/// end of each range is looked at in the same request as the start of the next, where it was
/// asked for apart as well, and a seam was asked for again at the end besides (gaps row 177).
#[tokio::test]
async fn each_seam_is_asked_for_once_across_it_and_the_files_end_once() {
    let size = 8 * MB;
    let (url, asked) = serve_recording(size).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts).await.expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(size));
    let asked = asked.lock().unwrap().clone();
    let (checks, ranges): (Vec<_>, Vec<_>) = asked.iter().copied().partition(|(a, b)| b - a < 2048);
    // Where each range begins: the probe's at 0, each other's 64 bytes after its request does.
    let mut starts: Vec<usize> = ranges.iter().map(|(a, _)| a + 64).collect();
    starts.sort_unstable();
    assert_eq!(
        checks.len(),
        starts.len() + 1,
        "not one check a seam and one for the end: {checks:?} for ranges starting at {starts:?}"
    );
    let mut seen = std::collections::BTreeSet::new();
    for &(a, b) in &checks {
        assert!(b - a < 1024, "a check of {} bytes", b - a + 1);
        let at = starts.iter().find(|&&seam| a < seam && seam <= b);
        let looked_at = match at {
            Some(&seam) => seam,
            None => {
                assert_eq!(b, size - 1, "a check across no seam: {a}-{b}");
                size
            }
        };
        assert!(
            seen.insert(looked_at),
            "{looked_at} asked for twice: {checks:?}"
        );
    }
}

/// A server of `pattern(size)` that answers each check of a seam (a closed range of under 2 KiB)
/// `wait` after it is asked, and every other range at once; with when each check came.
async fn serve_slow_checks(
    size: usize,
    wait: std::time::Duration,
) -> (String, Arc<std::sync::Mutex<Vec<std::time::Instant>>>) {
    let came = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (body, seen) = (Arc::new(pattern(size)), came.clone());
    let app = axum::Router::new().fallback(move |headers: HeaderMap| {
        let (body, seen) = (body.clone(), seen.clone());
        async move {
            let total = body.len();
            let (a, b) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .map(|(a, b)| {
                    (
                        a.parse::<usize>().unwrap_or(0),
                        b.parse::<usize>().unwrap_or(total - 1).min(total - 1),
                    )
                })
                .unwrap_or((0, total - 1));
            if b - a < 2048 {
                seen.lock().unwrap().push(std::time::Instant::now());
                tokio::time::sleep(wait).await;
            }
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("accept-ranges", "bytes")
                .header("content-range", format!("bytes {a}-{b}/{total}"))
                .header("content-length", b - a + 1)
                .header("etag", "\"s\"")
                .body(Body::from(body[a..=b].to_vec()))
                .unwrap()
        }
    });
    let addr = common::serve(app).await;
    (format!("http://{addr}/f.bin"), came)
}

/// A range's own checks are asked at once, not each once the one before it is answered: the
/// range whole last nearly always has two to ask (a seam either side, or a seam and the file's
/// end), and the download waits for them, so asked one after the other its finish took a round
/// trip more than it needed (gaps rows 171 and 177). Here every check is answered a second after
/// it is asked, and the ranges of a local file are whole within milliseconds of each other: asked
/// at once, every check comes within those milliseconds; one after the other, a range's second
/// check came a second after its first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ranges_own_checks_are_asked_at_once() {
    let (size, wait) = (8 * MB, std::time::Duration::from_secs(1));
    let (url, came) = serve_slow_checks(size, wait).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts).await.expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(size));
    let came = came.lock().unwrap().clone();
    // Four ranges: three seams and the file's end.
    assert!(came.len() >= 2, "{} checks", came.len());
    let (first, last) = (came.iter().min().unwrap(), came.iter().max().unwrap());
    let spread = last.duration_since(*first);
    assert!(
        spread < wait / 2,
        "a check was asked only once the one before it was answered: the checks came over {spread:?}"
    );
}

/// A server that leaves the seam boundary byte correct but corrupts bytes
/// 10–60 of the lead window. A 1-byte overlap would miss this; 64 bytes catch
/// it. This test verifies G96: dm fetches and compares LEAD_BYTES = 64 bytes.
#[tokio::test]
async fn a_corrupt_seam_interior_is_caught_by_the_64_byte_overlap() {
    let (url, checks) = serve_partial_corrupt().await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let done = dm_core::download(&dm_core::client(), &url, &opts)
        .await
        .expect("the download failed instead of going on with one connection");
    // Fell back to one connection because the seam bytes disagreed.
    assert_eq!(
        done.connections, 1,
        "it did not fall back to one connection when seam interior differed"
    );
    assert!(checks.load(Ordering::SeqCst) >= 1, "no seam check was made");
}

#[tokio::test]
async fn seams_that_differ_when_asked_for_again_restart_the_file_on_one_connection() {
    let (url, checks) = serve(true).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 4,
        ..options(dir.path())
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let seen = tokio::spawn(async move {
        let mut said = Vec::new();
        while let Some(update) = rx.recv().await {
            if let dm_core::Update::Retrying { why, .. } = update {
                said.push(why);
            }
        }
        said
    });
    let done = dm_core::download_reporting(&client(), &url, &opts, Some(tx))
        .await
        .expect("the download failed instead of going on with one connection");
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        pattern(8 * MB),
        "the file is not the one served"
    );
    assert_eq!(
        done.connections, 1,
        "it did not fall back to one connection"
    );
    assert!(checks.load(Ordering::SeqCst) >= 1);
    let said = seen.await.unwrap();
    assert!(
        said.iter().any(|why| why.contains("did not agree")),
        "the row was not told why: {said:?}"
    );
}

/// One request to [`Paced`]: when it came, the range it asked for (`None` for an open end), whether
/// it was a check, and when its answer was sent whole.
#[derive(Debug, Clone)]
struct PacedAsk {
    came: std::time::Duration,
    from: usize,
    to: Option<usize>,
    check: bool,
    sent: Arc<std::sync::Mutex<Option<std::time::Duration>>>,
}

/// A server of `pattern(size)` that answers each check (a closed range of under 2 KiB asked under
/// the validator) `check_late` after it comes, the answer from the file's start (the probe's, which
/// is range 0's) 16 KiB every 12 ms, and every other range 64 KiB every 10 ms; with when each
/// request came and when its answer was sent whole or let go.
struct Paced {
    data: Vec<u8>,
    check_late: std::time::Duration,
    started: std::time::Instant,
    log: std::sync::Mutex<Vec<PacedAsk>>,
}

/// A place among the requests [`Paced`] serves, held until the answer is sent or let go.
struct Serving {
    paced: Arc<Paced>,
    sent: Arc<std::sync::Mutex<Option<std::time::Duration>>>,
}

impl Drop for Serving {
    fn drop(&mut self) {
        *self.sent.lock().unwrap() = Some(self.paced.started.elapsed());
    }
}

async fn paced(State(p): State<Arc<Paced>>, headers: HeaderMap) -> Response {
    use futures_util::StreamExt as _;
    let total = p.data.len();
    let (from, to) = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| (a.parse::<usize>().unwrap_or(0), b.parse::<usize>().ok()))
        .unwrap_or((0, None));
    let check = headers.contains_key("if-range") && to.is_some_and(|to| to - from < 2048);
    let sent = Arc::new(std::sync::Mutex::new(None));
    p.log.lock().unwrap().push(PacedAsk {
        came: p.started.elapsed(),
        from,
        to,
        check,
        sent: sent.clone(),
    });
    let place = Arc::new(Serving {
        paced: p.clone(),
        sent,
    });
    if check {
        tokio::time::sleep(p.check_late).await;
    }
    let b = to.unwrap_or(total - 1).min(total - 1);
    let (piece, every) = match (check, from) {
        (true, _) => (b - from + 1, 0),
        (false, 0) => (16 * 1024, 12),
        (false, _) => (64 * 1024, 10),
    };
    let chunks: Vec<Vec<u8>> = p.data[from..=b].chunks(piece).map(<[u8]>::to_vec).collect();
    let body = futures_util::stream::iter(chunks).then(move |chunk| {
        let _held = place.clone();
        async move {
            tokio::time::sleep(std::time::Duration::from_millis(every)).await;
            Ok::<_, std::io::Error>(chunk)
        }
    });
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("accept-ranges", "bytes")
        .header("content-range", format!("bytes {from}-{b}/{total}"))
        .header("content-length", b - from + 1)
        .header("etag", "\"s\"")
        .body(Body::from_stream(body))
        .unwrap()
}

async fn serve_paced(size: usize, check_late: std::time::Duration) -> (String, Arc<Paced>) {
    let p = Arc::new(Paced {
        data: pattern(size),
        check_late,
        started: std::time::Instant::now(),
        log: std::sync::Mutex::new(Vec::new()),
    });
    let app = axum::Router::new().fallback(paced).with_state(p.clone());
    let addr = common::serve(app).await;
    (format!("http://127.0.0.1:{}/f.bin", addr.port()), p)
}

/// The range whole first takes its next work at once, not after its checks are answered (gaps
/// row S4). Two connections: the last range, served fast, is whole long before range 0, served
/// slowly from the probe, and its connection halves range 0. The check of the file's end that
/// range asks is answered 1.5 s late here; asked on its own connection before the range's took
/// more work, the halving came only once it was answered, when range 0 was nearly whole and no
/// longer worth halving.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_range_whole_takes_its_next_work_without_waiting_for_its_checks() {
    let (size, late) = (4 * MB, std::time::Duration::from_millis(1500));
    let (url, served) = serve_paced(size, late).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 2,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts).await.expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(size));
    let log = served.log.lock().unwrap().clone();
    let last = log
        .iter()
        .find(|a| !a.check && a.from > 0 && a.to == Some(size - 1))
        .unwrap_or_else(|| panic!("no request for the last range: {log:?}"));
    let whole = last.sent.lock().unwrap().expect("the last range was sent");
    let next = log
        .iter()
        .filter(|a| !a.check && a.came >= last.came && a.from != last.from)
        .map(|a| a.came)
        .min()
        .unwrap_or_else(|| panic!("nothing was asked after the last range: {log:?}"));
    assert!(
        next < whole + late / 3,
        "the next range was asked {:?} after the last range was whole, with its check {late:?} late: {log:?}",
        next.saturating_sub(whole)
    );
    // Every seam was still asked for again, and the file's end.
    let checks = log.iter().filter(|a| a.check).count();
    // Every range is asked for as a closed range; what has no range at all looks for a checksum
    // file beside the download, once it is whole.
    let ranges = log.iter().filter(|a| !a.check && a.to.is_some()).count();
    assert_eq!(
        checks, ranges,
        "{checks} checks for {ranges} ranges: {log:?}"
    );
}

/// The most requests [`Paced`] served at once, each from `LET_GO` after it came until its answer
/// was sent or let go. An answer the client lets go part-way (a range halved under its
/// connection, or the probe's at range 0's end) is seen gone only at the server's next write or
/// the one after, 10 to 24 ms later here, while the connection has already asked for its next
/// range: counted from when they came, the two looked like one request too many.
fn most_at_once(log: &[PacedAsk]) -> usize {
    const LET_GO: std::time::Duration = std::time::Duration::from_millis(40);
    let mut edges = Vec::new();
    for asked in log {
        let from = asked.came + LET_GO;
        let to = asked.sent.lock().unwrap().expect("every answer ended");
        if to > from {
            edges.push((from, 1i32));
            edges.push((to, -1i32));
        }
    }
    // An answer that ends as another begins is not served beside it.
    edges.sort();
    let (mut now, mut most) = (0i32, 0i32);
    for (_, step) in edges {
        now += step;
        most = most.max(now);
    }
    usize::try_from(most).unwrap_or(0)
}

/// The checks go out beside the ranges on the connections the download has, never on more: a
/// site limited to three connections never serves four requests at once, with every check
/// answered late so that it overlaps the ranges still being fetched (gaps row S4).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checks_beside_the_ranges_never_ask_more_than_a_sites_limit_of_connections() {
    let (size, late) = (6 * MB, std::time::Duration::from_millis(300));
    let (url, served) = serve_paced(size, late).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 8,
        host_limits: vec![dm_core::HostLimit {
            host: "127.0.0.1".into(),
            connections: 3,
        }],
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts).await.expect("download");
    assert_eq!(std::fs::read(&done.path).unwrap(), pattern(size));
    let log = served.log.lock().unwrap().clone();
    let most = most_at_once(&log);
    assert!(
        most <= 3,
        "{most} requests at once to a site limited to 3: {log:?}"
    );
    let checks = log.iter().filter(|a| a.check).count();
    // Every range is asked for as a closed range; what has no range at all looks for a checksum
    // file beside the download, once it is whole.
    let ranges = log.iter().filter(|a| !a.check && a.to.is_some()).count();
    assert_eq!(
        checks, ranges,
        "{checks} checks for {ranges} ranges: {log:?}"
    );
}
