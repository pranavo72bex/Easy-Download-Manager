// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The tests of a capped download's ranges, which need a server of their own.
mod capped;

#[test]
fn under_the_computers_proxy_only_a_proxy_in_force_with_no_address_refuses() {
    // A proxy in force (a configuration script alone) and no address: refused.
    assert!(super::refuses_unrouted(true, false, false, || true));
    // No proxy on the computer: direct.
    assert!(!super::refuses_unrouted(true, false, false, || false));
    // An address to go through, a bypassed site, or settings of dm's own:
    // the computer is not asked.
    for (system, address, bypassed) in [
        (true, true, false),
        (true, false, true),
        (false, false, false),
    ] {
        assert!(!super::refuses_unrouted(
            system,
            address,
            bypassed,
            || panic!("asked")
        ));
    }
}

/// A range of 1 MiB from `start`, `held` of it fetched, its attempt asked at `asked`.
fn live(held: u64, asked: tokio::time::Instant) -> super::Live {
    super::Live {
        start: 0,
        end: 1024 * 1024 - 1,
        reserved: held,
        taken: true,
        trusted: true,
        reach: 0,
        going: super::Going {
            since: Some(asked),
            ..Default::default()
        },
    }
}

#[test]
fn a_range_is_judged_at_the_pace_of_its_last_window() {
    use std::time::Duration;
    let t0 = tokio::time::Instant::now();
    let ms = |n: u64| t0 + Duration::from_millis(n);
    let mut l = live(0, t0);
    // Asked, nothing yet: too soon to tell.
    assert_eq!(l.time_left(ms(5)), None);
    // 64 KiB every 10 ms: the first bytes open the window, the next close it once it is
    // 20 ms long.
    for (at, held) in [(10, 64), (20, 128), (30, 192)] {
        l.reserved = held * 1024;
        let first = l.going.came(ms(at), l.reserved);
        assert_eq!(first, at == 10);
    }
    // 128 KiB over the 20 ms window: 6.5 MB/s, with 832 KiB left.
    let left = l.time_left(ms(31)).expect("a whole window was measured");
    assert!((left - 832.0 / 128.0 * 0.020).abs() < 1e-9, "{left}");
    // A connection that shared the line and then had it to itself: judged by the new pace.
    for (at, held) in [(40, 448), (50, 704)] {
        l.reserved = held * 1024;
        l.going.came(ms(at), l.reserved);
    }
    let left = l.time_left(ms(51)).expect("judged");
    assert!((left - 320.0 / 512.0 * 0.020).abs() < 1e-9, "{left}");
}

#[test]
fn a_range_that_waited_a_second_for_its_answer_or_its_next_bytes_is_stuck() {
    use std::time::Duration;
    let t0 = tokio::time::Instant::now();
    let later = |d: Duration| t0 + d;
    // No answer: too soon to tell, then stuck.
    let l = live(0, t0);
    assert_eq!(l.time_left(later(Duration::from_millis(900))), None);
    assert_eq!(l.time_left(later(super::STUCK_AFTER)), Some(f64::INFINITY));
    // Bytes, then none for a second: stuck, whatever its pace was.
    let mut l = live(0, t0);
    l.going.came(later(Duration::from_millis(10)), 1000);
    l.going.came(later(Duration::from_millis(40)), 900_000);
    assert!(l
        .time_left(later(Duration::from_millis(41)))
        .is_some_and(f64::is_finite));
    assert_eq!(
        l.time_left(later(Duration::from_millis(40) + super::STUCK_AFTER)),
        Some(f64::INFINITY)
    );
}

#[test]
fn a_ts_name_loses_its_extension_in_any_case() {
    assert_eq!(super::ts_stem("show.ts"), "show");
    assert_eq!(super::ts_stem("SHOW.TS"), "SHOW");
    assert_eq!(super::ts_stem("show.mp4"), "show.mp4");
    assert_eq!(super::ts_stem("ts"), "ts");
}

use super::*;

#[test]
fn only_a_known_file_type_makes_an_address_a_file() {
    assert!(names_a_file("https://e.com/files/report.zip"));
    assert!(names_a_file("https://e.com/a/b/Setup.EXE?x=1"));
    assert!(!names_a_file(
        "https://archive.org/details/night.of.the.living.dead"
    ));
    assert!(!names_a_file("https://video.example/@some.handle"));
    assert!(!names_a_file("https://e.com/view.php?id=4"));
    assert!(!names_a_file("https://e.com/watch"));
}

#[test]
fn a_page_is_known_by_its_opening() {
    assert!(looks_like_html(b"\xef\xbb\xbf  <!DOCTYPE html><html>"));
    assert!(looks_like_html(b"\n<script>location='/login'</script>"));
    assert!(!looks_like_html(b"PK\x03\x04 zip bytes"));
    assert!(!looks_like_html(b"%PDF-1.7"));
}

#[tokio::test]
async fn two_files_finishing_at_once_never_share_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let claims: Vec<_> = (0..16)
        .map(|_| {
            let dir = dir.path().to_path_buf();
            tokio::spawn(async move { claim_path(&dir, "f.bin").await.unwrap() })
        })
        .collect();
    let mut taken = std::collections::HashSet::new();
    for claim in claims {
        assert!(
            taken.insert(claim.await.unwrap()),
            "one name was given twice"
        );
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 16);
}

/// The name shown for a file still downloading is the one it will be
/// placed under (`big.bin` until it finished as `big (1).bin`).
#[tokio::test]
async fn the_free_name_is_the_one_a_claim_takes() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(free_name(dir.path(), "big.bin"), "big.bin");
    std::fs::write(dir.path().join("big.bin"), b"old").unwrap();
    assert_eq!(free_name(dir.path(), "big.bin"), "big (1).bin");
    let claimed = claim_path(dir.path(), "big.bin").await.unwrap();
    assert_eq!(claimed, dir.path().join("big (1).bin"));
    assert_eq!(free_name(dir.path(), "big.bin"), "big (2).bin");
    assert_eq!(free_name(dir.path(), "README"), "README");
}

#[tokio::test]
async fn a_folder_already_there_is_not_moved_onto() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("album")).unwrap();
    std::fs::write(dir.path().join("album/mine.txt"), b"keep").unwrap();
    let claimed = claim_dir(dir.path(), "album").await.unwrap();
    assert_eq!(claimed, dir.path().join("album (1)"));
    assert_eq!(
        std::fs::read(dir.path().join("album/mine.txt")).unwrap(),
        b"keep"
    );
}

fn options_in(dir: &Path) -> Options {
    Options {
        connections: 1,
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
        attempts: 5,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        room: Default::default(),
        remux: false,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
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

/// A mount reporting zero free of zero total is unknown, not
/// full; every download of known length was refused on one.
#[test]
fn a_file_system_that_reports_no_size_is_not_taken_as_full() {
    assert_eq!(reported_free(0, 0), None);
    assert_eq!(reported_free(0, 1 << 30), Some(0));
    assert_eq!(reported_free(5, 10), Some(5));
}

#[tokio::test]
async fn a_cancelled_download_is_not_assembled() {
    // The bytes all arrived and then the row was cancelled. The join
    // must not run: the parts are being deleted while it would, and a
    // file made of whichever survived is short and looks finished.
    let dir = tempfile::tempdir().unwrap();
    let options = options_in(dir.path());
    std::fs::create_dir_all(&options.parts_dir).unwrap();
    let parts: Vec<PathBuf> = (0..2).map(|i| part_path(&options.parts_dir, i)).collect();
    for part in &parts {
        std::fs::write(part, b"1234").unwrap();
    }
    options.control.cancel();

    let outcome = finish("f.bin", None, &parts, &options).await;
    assert!(matches!(outcome, Err(Error::Cancelled)), "got {outcome:?}");
    assert!(
        !options.dest_dir.join("f.bin").exists(),
        "a cancelled download was assembled"
    );
}

#[tokio::test]
async fn a_part_taken_away_fails_the_join_rather_than_shortening_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let options = options_in(dir.path());
    std::fs::create_dir_all(&options.parts_dir).unwrap();
    std::fs::create_dir_all(&options.dest_dir).unwrap();
    let parts: Vec<PathBuf> = (0..3).map(|i| part_path(&options.parts_dir, i)).collect();
    std::fs::write(&parts[0], b"1234").unwrap();
    // parts[1] never written: it was removed under the join.
    std::fs::write(&parts[2], b"5678").unwrap();

    let into = options.dest_dir.join("f.bin");
    let outcome = join_parts(&parts, None, &into, &options.control).await;
    assert!(outcome.is_err(), "a missing part was skipped");
    assert!(!into.exists(), "a short file was left under the name");
}

/// A Cancel stops the join of a file's parts part-way and leaves no file under the name (gaps
/// row 185). The parts are sparse: only what the join wrote before it stopped is on the disk.
/// Part-way is told by the parts the join never reached, still there: a bound in time broke
/// under the load of the whole workspace's run, where one 32 MiB piece took over a second
/// (gaps row S19).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_stops_a_join_part_way() {
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let options = options_in(dir.path());
    std::fs::create_dir_all(&options.parts_dir).unwrap();
    std::fs::create_dir_all(&options.dest_dir).unwrap();
    let parts: Vec<PathBuf> = (0..8).map(|i| part_path(&options.parts_dir, i)).collect();
    for part in &parts {
        std::fs::File::create(part)
            .unwrap()
            .set_len(256 << 20)
            .unwrap();
    }
    let into = options.dest_dir.join("f.bin");
    let joining = {
        let (parts, into, control) = (parts.clone(), into.clone(), options.control.clone());
        tokio::spawn(async move { join_parts(&parts, None, &into, &control).await })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !joining.is_finished(),
        "the join was over before it could be cancelled"
    );
    options.control.cancel();
    // Only against a join that never stops: the whole 2 GiB is far more than this takes.
    let outcome = tokio::time::timeout(Duration::from_secs(60), joining)
        .await
        .expect("the join went on for a minute after the cancel")
        .unwrap();
    assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
    assert!(!into.exists(), "a part-joined file was left under the name");
    assert!(
        parts.last().is_some_and(|last| last.exists()),
        "the join went on to the last part after the cancel"
    );
}

#[test]
fn a_streams_headers_are_printed_by_name_never_by_value() {
    let headers = Headers::of([("cookie", "sid=hidden-value"), ("x-token", "abc123")]);
    let printed = format!("{headers:?}");
    assert!(
        printed.contains("cookie") && printed.contains("x-token"),
        "{printed}"
    );
    assert!(
        !printed.contains("hidden-value") && !printed.contains("abc123"),
        "{printed}"
    );
}

/// A server on 127.0.0.1 answering every path with `body` as `kind`.
async fn serve_one(kind: &'static str, body: &'static [u8]) -> std::net::SocketAddr {
    let app = axum::Router::new().fallback(move || async move {
        axum::response::Response::builder()
            .header("content-type", kind)
            .header("content-length", body.len())
            .body(axum::body::Body::from(body))
            .unwrap()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

#[tokio::test]
async fn a_part_of_a_file_that_is_no_video_is_refused_and_nothing_is_fetched_into_place() {
    let addr = serve_one("application/zip", b"PK\x03\x04 not really").await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = options_in(dir.path());
    options.span = crate::clip::Span::new(1.0, 2.0);
    let why = download(
        &crate::reqwest_defaults(),
        &format!("http://{addr}/files/pack.zip"),
        &options,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(why.contains("a part can be cut only"), "{why}");
    assert!(!options.dest_dir.join("pack.zip").exists());
}

#[tokio::test]
async fn a_page_where_a_videos_stream_should_be_is_that_streams_failure() {
    let addr = serve_one(
        "text/html; charset=utf-8",
        b"<!DOCTYPE html><html><body>sign in</body></html>",
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let options = options_in(dir.path());
    let why = download_video_stream_with(
        &crate::reqwest_defaults(),
        &format!("http://{addr}/v/stream"),
        &options,
        None,
        StreamHints::default(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(why.contains("answered with a web page"), "{why}");
}
