// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::tests::sidx;
use super::*;
use crate::{Categories, Control, Limiter, Quality};
use axum::body::Body;
use axum::extract::Path as UrlPath;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Four pieces of a second each, 100 000 bytes apiece.
const PIECE: usize = 100_000;
const PIECES: usize = 4;

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut whole = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    whole.extend_from_slice(kind);
    whole.extend_from_slice(body);
    whole
}

/// A stream laid out as a DASH MP4 is: `ftyp` and `moov`, its index,
/// then its pieces, whose bytes differ from place to place so a slice
/// written in the wrong order or from the wrong offset shows. Returns
/// the stream, where its init ends, and where its index ends.
fn stream() -> (Vec<u8>, usize, usize) {
    let init = [
        boxed(b"ftyp", b"iso6\0\0\0\0iso6dash"),
        boxed(b"moov", &[7; 200]),
    ]
    .concat();
    let index = sidx(1000, 0, &[(PIECE as u32, 1000); PIECES]);
    let mut data = [init.clone(), index.clone()].concat();
    let pieces_at = data.len();
    data.extend((0..PIECE * PIECES).map(|i| ((i * 7 + i / 977) % 251) as u8));
    assert_eq!(data.len(), pieces_at + PIECE * PIECES);
    (data, init.len(), pieces_at)
}

/// What each request carried: its range, user agent and referrer.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    /// The case the request was made in (the path's first part).
    how: String,
    range: String,
    agent: String,
    referer: String,
}

#[derive(Default)]
struct Served {
    asked: Mutex<Vec<Asked>>,
    /// How often each range was asked, for a server that fails a
    /// range the first time.
    times: Mutex<HashMap<String, u32>>,
}

/// Serves `data` at `/{how}/s.mp4`, where `how` says how the server
/// answers a range from `from` on (the stream's own first bytes are
/// always served honestly): `honest`, `ignores` (the whole file),
/// `shifted` (another range than asked), `longer` (a byte more than
/// stated, the length said), `streams-more` and `streams-less` (a
/// body of no stated length with a byte more or less), `flaky` (a 503
/// the first time each range is asked), `gone` (404).
async fn serve(data: Vec<u8>, from: usize) -> (String, Arc<Served>) {
    let data = Arc::new(data);
    let served = Arc::new(Served::default());
    let state = served.clone();
    let app = axum::Router::new().route(
        "/{how}/s.mp4",
        axum::routing::get(move |UrlPath(how): UrlPath<String>, headers: HeaderMap| {
            let (data, state) = (data.clone(), state.clone());
            async move {
                let header = |name: &str| {
                    headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string()
                };
                let range = header("range");
                state.asked.lock().unwrap().push(Asked {
                    how: how.clone(),
                    range: range.clone(),
                    agent: header("user-agent"),
                    referer: header("referer"),
                });
                let times = {
                    let mut times = state.times.lock().unwrap();
                    let n = times.entry(range.clone()).or_default();
                    *n += 1;
                    *n
                };
                let n = data.len();
                let (a, z) = range
                    .strip_prefix("bytes=")
                    .and_then(|r| r.split_once('-'))
                    .and_then(|(a, z)| Some((a.parse::<usize>().ok()?, z.parse::<usize>().ok()?)))
                    .map(|(a, z)| (a, z.min(n - 1)))
                    .unwrap();
                let how = if a < from { "honest" } else { how.as_str() };
                let partial = |a: usize, z: usize, body: Body| {
                    Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-range", format!("bytes {a}-{z}/{n}"))
                        .body(body)
                        .unwrap()
                };
                let streamed = |bytes: Vec<u8>| {
                    Body::from_stream(futures_util::stream::iter([Ok::<_, std::io::Error>(
                        bytes::Bytes::from(bytes),
                    )]))
                };
                match how {
                    "ignores" => Response::new(Body::from(data.to_vec())),
                    "shifted" => partial(a + 1, z, Body::from(data[a + 1..=z].to_vec())),
                    "longer" => partial(a, z, Body::from(data[a..=z + 1].to_vec())),
                    "streams-more" => partial(a, z, streamed(data[a..=z + 1].to_vec())),
                    "streams-less" => partial(a, z, streamed(data[a..z].to_vec())),
                    "flaky" if times == 1 => Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(Body::empty())
                        .unwrap(),
                    "gone" | "gone-again" => Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::empty())
                        .unwrap(),
                    _ => partial(a, z, Body::from(data[a..=z].to_vec())),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), served)
}

fn options(dir: &std::path::Path, attempts: u32) -> Options {
    Options {
        connections: 3,
        control: Control::new(),
        dest_dir: dir.join("out"),
        parts_dir: dir.join("parts"),
        limiter: Limiter::unlimited(),
        categories: Categories::default(),
        checksum: None,
        quality: Quality::Best,
        sound: None,
        span: None,
        shape: None,
        mp3: false,
        gif: false,
        media: false,
        referrer: None,
        proxy: None,
        container: None,
        attempts,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        room: Default::default(),
        remux: false,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
        same_files: None,
        frame_rate: None,
        caption: None,
        format: None,
        sign_in: None,
        save_name: None,
        accept_page: false,
        torrent: Default::default(),
        system_proxy: false,
        title_hint: None,
    }
}

/// The stream at `{base}/{how}/s.mp4`, its init and index stated where
/// `stated` holds them, written to `target`.
fn source(base: &str, how: &str, stated: Option<(usize, usize)>, target: PathBuf) -> Source {
    Source {
        url: format!("{base}/{how}/s.mp4"),
        init: stated.map(|(init, _)| (0, init as u64 - 1)),
        index: stated.map(|(init, index)| (init as u64, index as u64 - 1)),
        agent: Some("edm-test-agent".into()),
        headers: Headers::of([("referer", "http://example.test/page")]),
        chunk: Some(64 << 10),
        target,
    }
}

fn span(from: f64, to: f64) -> Span {
    Span::new(from, to).unwrap()
}

/// A part with its init and index stated: the init, then the bytes of
/// the pieces that overlap the span (the second and third), fetched in
/// slices no larger than the cap, each with the stream's user agent and
/// headers; progress said up to the whole; and a second fetch of the
/// same part, now whole on disk, asks for nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_part_is_its_init_and_the_pieces_over_its_span() {
    let (data, init, index) = stream();
    let (base, served) = serve(data.clone(), index).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("nested/part.mp4");
    let source = source(&base, "honest", Some((init, index)), target.clone());
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let part = fetch_part(
        &crate::client(),
        &source,
        span(1.2, 2.5),
        &options(dir.path(), 3),
        Some(tx),
    )
    .await
    .unwrap();
    let (begin, end) = (index + PIECE, index + 3 * PIECE - 1);
    let expected = [&data[..init], &data[begin..=end]].concat();
    assert_eq!(
        part,
        Part {
            bytes: expected.len() as u64,
            starts: 1.0,
            ends: 3.0
        }
    );
    assert!(
        std::fs::read(&target).unwrap() == expected,
        "the part's bytes differ"
    );
    assert!(!dir.path().join("nested/part.mp4.part").exists());

    let asked = served.asked.lock().unwrap().clone();
    let ranges: Vec<&str> = asked.iter().map(|a| a.range.as_str()).collect();
    assert_eq!(ranges[0], format!("bytes=0-{}", init - 1));
    assert_eq!(ranges[1], format!("bytes={init}-{}", index - 1));
    let slice = 64 << 10;
    let mut slices: Vec<String> = (0..)
        .map(|n| begin + n * slice)
        .take_while(|a| *a <= end)
        .map(|a| format!("bytes={a}-{}", (a + slice - 1).min(end)))
        .collect();
    let mut got: Vec<String> = ranges[2..].iter().map(|r| r.to_string()).collect();
    got.sort();
    slices.sort();
    assert_eq!(got, slices, "the slices asked for");
    assert!(asked
        .iter()
        .all(|a| a.agent == "edm-test-agent" && a.referer == "http://example.test/page"));

    let mut last = None;
    while let Ok(update) = rx.try_recv() {
        if let Update::Progress(p) = update {
            last = Some(p);
        }
    }
    let last = last.expect("no progress was said");
    assert_eq!(last.downloaded, part.bytes);
    assert_eq!(last.total, Some(part.bytes));

    let again = fetch_part(
        &crate::client(),
        &source,
        span(1.2, 2.5),
        &options(dir.path(), 3),
        None,
    )
    .await
    .unwrap();
    assert_eq!(again, part);
    assert_eq!(
        served.asked.lock().unwrap().len(),
        asked.len() + 2,
        "a part whole on disk was fetched again"
    );
}

/// With no index stated, the stream's first 256 KiB are read and the
/// index found in them; the part is the same as with it stated.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_index_is_found_in_the_first_bytes_where_none_is_stated() {
    let (data, init, index) = stream();
    let (base, served) = serve(data.clone(), index).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("part.mp4");
    let source = source(&base, "honest", None, target.clone());
    let part = fetch_part(
        &crate::client(),
        &source,
        span(0.0, 0.5),
        &options(dir.path(), 3),
        None,
    )
    .await
    .unwrap();
    assert_eq!((part.starts, part.ends), (0.0, 1.0));
    let expected = [&data[..init], &data[index..index + PIECE]].concat();
    assert!(std::fs::read(&target).unwrap() == expected);
    let asked = served.asked.lock().unwrap();
    assert_eq!(asked[0].range, format!("bytes=0-{}", PROBE - 1));
}

/// A stream shorter than the 256 KiB read to look for its index, with
/// no index stated: a server answers the range honestly with the bytes
/// it has (RFC 9110 14.1.2: a last byte past the end means the end), and
/// the part is cut from them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_shorter_than_the_probe_is_cut_with_no_index_stated() {
    let init = [
        boxed(b"ftyp", b"iso6\0\0\0\0iso6dash"),
        boxed(b"moov", &[7; 200]),
    ]
    .concat();
    let mut data = [init.clone(), sidx(1000, 0, &[(1000, 1000); 2])].concat();
    let index = data.len();
    data.extend((0..2000).map(|i| (i % 251) as u8));
    let (base, _) = serve(data.clone(), usize::MAX).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("part.mp4");
    let part = fetch_part(
        &crate::client(),
        &source(&base, "honest", None, target.clone()),
        span(1.0, 2.0),
        &options(dir.path(), 1),
        None,
    )
    .await
    .expect("a short stream could not be cut");
    assert_eq!((part.starts, part.ends), (1.0, 2.0));
    let expected = [&data[..init.len()], &data[index + 1000..]].concat();
    assert!(std::fs::read(&target).unwrap() == expected);
}

/// A stream with no index (a plain MP4's `moov` then `mdat`), or an
/// index stated where there is none, cannot be cut, and says so; a
/// span past the stream's end says there is nothing there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_with_no_index_or_nothing_in_the_span_is_refused_in_words() {
    let mut plain = [
        boxed(b"ftyp", b"isom\0\0\0\0isom"),
        boxed(b"moov", &[1; 64]),
    ]
    .concat();
    plain.extend_from_slice(&boxed(b"mdat", &vec![2; PROBE as usize]));
    let (base, _) = serve(plain, usize::MAX).await;
    let dir = tempfile::tempdir().unwrap();
    let options = options(dir.path(), 3);
    let target = dir.path().join("part.mp4");
    let said = fetch_part(
        &crate::client(),
        &source(&base, "honest", None, target.clone()),
        span(0.0, 1.0),
        &options,
        None,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("no index of its pieces"), "{said}");
    let said = fetch_part(
        &crate::client(),
        &source(&base, "honest", Some((32, 100)), target.clone()),
        span(0.0, 1.0),
        &options,
        None,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("no index of its pieces"), "{said}");

    let (data, init, index) = stream();
    let (base, _) = serve(data, index).await;
    let said = fetch_part(
        &crate::client(),
        &source(&base, "honest", Some((init, index)), target.clone()),
        span(10.0, 20.0),
        &options,
        None,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("nothing between 0:10 and 0:20"), "{said}");
    assert!(!target.exists());
}

/// Each way a server can answer a range dishonestly is an error of its
/// own, and no part is left on disk under its name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dishonest_answer_to_a_range_is_refused() {
    let (data, init, index) = stream();
    let (base, served) = serve(data, index).await;
    let dir = tempfile::tempdir().unwrap();
    for (how, wanted) in [
        ("ignores", "RangesIgnored"),
        ("shifted", "DishonestRanges"),
        ("longer", "DishonestRanges"),
        ("streams-more", "DishonestRanges"),
        ("streams-less", "Incomplete"),
        ("gone", "Refused"),
    ] {
        served.asked.lock().unwrap().clear();
        let target = dir.path().join(format!("{how}.mp4"));
        let error = fetch_part(
            &crate::client(),
            &source(&base, how, Some((init, index)), target.clone()),
            span(0.0, 1.0),
            &options(dir.path(), 1),
            None,
        )
        .await
        .unwrap_err();
        assert!(format!("{error:?}").starts_with(wanted), "{how}: {error:?}");
        assert!(!target.exists(), "{how}: a part was left under its name");
        // Init, index and the one piece's slices (the second may not be
        // asked once the first has failed), none asked twice.
        assert_one_try(&served, how);
    }
    // A missing piece is not asked again however many tries are left.
    // A case of its own name: the "gone" case's last slice, sent as its
    // first one failed, can reach the server after the log is cleared,
    // and was counted as this one's (seen under a full test run's load).
    served.asked.lock().unwrap().clear();
    let error = fetch_part(
        &crate::client(),
        &source(
            &base,
            "gone-again",
            Some((init, index)),
            dir.path().join("gone.mp4"),
        ),
        span(0.0, 1.0),
        &options(dir.path(), 5),
        None,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, Error::Refused { status: 404, .. }),
        "{error:?}"
    );
    assert_one_try(&served, "gone-again, five tries");
}

fn assert_one_try(served: &Served, how: &str) {
    // This case's own requests: the slice a case before it had already
    // sent when its first one failed can reach the server after the log
    // was cleared for this one (seen under coverage instrumentation).
    let asked = served.asked.lock().unwrap();
    let case = how.split(',').next().unwrap_or(how);
    let mine: Vec<&Asked> = asked.iter().filter(|a| a.how == case).collect();
    let mut ranges: Vec<&str> = mine.iter().map(|a| a.range.as_str()).collect();
    assert!((3..=4).contains(&ranges.len()), "{how}: {ranges:?}");
    ranges.sort_unstable();
    ranges.dedup();
    assert_eq!(ranges.len(), mine.len(), "{how}: a range was asked again");
}

/// A slice that fails in passing (a 503) is asked again after a wait
/// and the part is whole; with no number of tries set, ten are allowed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slice_that_fails_in_passing_is_asked_again() {
    let (data, init, index) = stream();
    let (base, served) = serve(data.clone(), index).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("part.mp4");
    let part = fetch_part(
        &crate::client(),
        &source(&base, "flaky", Some((init, index)), target.clone()),
        span(3.0, 4.0),
        &options(dir.path(), 0),
        None,
    )
    .await
    .unwrap();
    assert_eq!((part.starts, part.ends), (3.0, 4.0));
    let expected = [&data[..init], &data[index + 3 * PIECE..]].concat();
    assert!(std::fs::read(&target).unwrap() == expected);
    let times = served.times.lock().unwrap();
    let slices: Vec<_> = times
        .iter()
        .filter(|(range, _)| {
            range
                .strip_prefix("bytes=")
                .and_then(|r| r.split('-').next())
                .and_then(|a| a.parse::<usize>().ok())
                .is_some_and(|a| a >= index)
        })
        .collect();
    assert_eq!(slices.len(), 2, "{times:?}");
    assert!(slices.iter().all(|(_, n)| **n == 2), "{times:?}");
}

/// A cancel asked for while a slice waits to be asked again is
/// answered at once, not after the wait; one asked before anything is
/// fetched stops it before a request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_stops_a_part_at_once() {
    let (data, init, index) = stream();
    let (base, served) = serve(data, index).await;
    let dir = tempfile::tempdir().unwrap();
    let options = options(dir.path(), 5);
    let control = options.control.clone();
    let source = source(
        &base,
        "flaky",
        Some((init, index)),
        dir.path().join("part.mp4"),
    );
    let started = Instant::now();
    let stop = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        control.cancel();
    });
    let error = fetch_part(&crate::client(), &source, span(0.0, 1.0), &options, None)
        .await
        .unwrap_err();
    stop.await.unwrap();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert!(
        started.elapsed() < Duration::from_millis(1000),
        "{:?}",
        started.elapsed()
    );

    served.asked.lock().unwrap().clear();
    let error = fetch_part(&crate::client(), &source, span(0.0, 1.0), &options, None)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert!(served.asked.lock().unwrap().is_empty());
}

/// A part that cannot be written where it is to go is a disk error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_part_with_nowhere_to_be_written_is_a_disk_error() {
    let (data, init, index) = stream();
    let (base, _) = serve(data, index).await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a-file");
    std::fs::write(&file, b"not a folder").unwrap();
    let error = fetch_part(
        &crate::client(),
        &source(&base, "honest", Some((init, index)), file.join("part.mp4")),
        span(0.0, 1.0),
        &options(dir.path(), 1),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, Error::Disk(_)), "{error:?}");
}

/// The index's other forms: version 1 (64-bit times and offsets), and
/// a box that says it runs to the end of what was read (size 0) or
/// gives a 64-bit size (size 1).
#[test]
fn an_index_of_version_one_and_boxes_of_every_size_form_are_read() {
    let mut body = vec![1u8, 0, 0, 0];
    body.extend_from_slice(&1u32.to_be_bytes());
    body.extend_from_slice(&90_000u32.to_be_bytes());
    body.extend_from_slice(&(1u64 << 33).to_be_bytes()); // earliest
    body.extend_from_slice(&5u64.to_be_bytes()); // first offset
    body.extend_from_slice(&[0, 0, 0, 1]);
    body.extend_from_slice(&300u32.to_be_bytes());
    body.extend_from_slice(&45_000u32.to_be_bytes());
    body.extend_from_slice(&0x9000_0000u32.to_be_bytes());
    let v1 = boxed(b"sidx", &body);
    let index = parse_sidx(&v1, 1000).unwrap();
    assert_eq!(index.timescale, 90_000);
    assert_eq!(
        index.pieces,
        [Piece {
            offset: 1000 + v1.len() as u64 + 5,
            size: 300,
            start: 1 << 33,
            duration: 45_000
        }]
    );
    // An index with a timescale of 0, or naming no pieces, is none.
    assert_eq!(parse_sidx(&sidx(0, 0, &[(10, 10)]), 0), None);
    assert_eq!(parse_sidx(&sidx(1000, 0, &[]), 0), None);

    let ftyp = boxed(b"ftyp", b"isom\0\0\0\0");
    let moov = boxed(b"moov", &[0; 16]);
    // A free box of 64-bit size between `moov` and the index.
    let mut free = 1u32.to_be_bytes().to_vec();
    free.extend_from_slice(b"free");
    free.extend_from_slice(&24u64.to_be_bytes());
    free.extend_from_slice(&[0; 8]);
    let index = sidx(1000, 0, &[(50, 1000)]);
    let head = [ftyp.clone(), moov.clone(), free.clone(), index.clone()].concat();
    let init = (ftyp.len() + moov.len()) as u64;
    let at = init + free.len() as u64;
    assert_eq!(
        locate(&head),
        Some((init, (at, at + index.len() as u64 - 1)))
    );
    // A box running to the end with no index after it, a box smaller
    // than its own header, and an index before any `moov`.
    let mut open = 0u32.to_be_bytes().to_vec();
    open.extend_from_slice(b"free");
    assert_eq!(locate(&[ftyp.clone(), moov.clone(), open].concat()), None);
    assert_eq!(
        locate(&[ftyp.clone(), vec![0, 0, 0, 4, 0, 0, 0, 0]].concat()),
        None
    );
    assert_eq!(locate(&[ftyp, index].concat()), None);
}

/// Words for seconds that are not seconds are refused, and the clock
/// and the file name agree for a span under a minute.
#[test]
fn odd_words_for_seconds_are_refused() {
    for odd in ["1:x", "1:-2", "1m2x", "m", "1..2s", "1h 2q", "inf", "NaN"] {
        assert_eq!(Span::seconds(odd), None, "{odd}");
    }
    assert_eq!(
        Span::seconds("1m 30"),
        Some(90.0),
        "a bare number after units"
    );
    assert_eq!(Span::new(f64::NAN, 1.0), None);
    assert_eq!(Span::new(-1.0, 1.0), None);
    assert_eq!(Span::new(2.0, f64::INFINITY), None);
    assert_eq!(Span::clock(-5.0), "0:00");
    assert_eq!(span(5.0, 59.6).words(), "0m05s-1m00s");
}
