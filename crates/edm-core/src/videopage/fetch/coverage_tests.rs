// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::{Categories, Control, Limiter};

fn options(dir: &Path) -> Options {
    Options {
        connections: 4,
        control: Control::new(),
        dest_dir: dir.join("out"),
        parts_dir: dir.join("parts"),
        limiter: Limiter::unlimited(),
        room: Default::default(),
        categories: Categories::default(),
        checksum: None,
        quality: crate::stream::Quality::Best,
        sound: None,
        span: None,
        shape: None,
        mp3: false,
        gif: false,
        media: false,
        referrer: None,
        proxy: None,
        container: None,
        attempts: 1,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        sign_in: None,
        save_name: None,
        accept_page: false,
        torrent: Default::default(),
        system_proxy: false,
        title_hint: None,
        remux: false,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
        same_files: None,
        frame_rate: None,
        caption: None,
        format: None,
    }
}

fn read(json: &str) -> Read {
    Read::new(serde_json::from_str(json).unwrap())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A reading served from a moment past is fetched at once; one served a
/// second from now is waited for; a cancel ends the wait.
#[tokio::test]
async fn the_wait_for_a_stream_to_be_served_ends_on_time_or_on_a_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path());
    let past = read(&format!(r#"{{"available_at": {}}}"#, now() - 5));
    let at = std::time::Instant::now();
    wait_until_available(&past, &opts).await.unwrap();
    assert!(at.elapsed() < Duration::from_millis(100));

    let soon = read(&format!(r#"{{"available_at": {}}}"#, now() + 1));
    wait_until_available(&soon, &opts).await.unwrap();
    assert!(at.elapsed() >= Duration::from_millis(400), "it waited");

    let later = read(&format!(r#"{{"available_at": {}}}"#, now() + 60));
    let cancelled = options(dir.path());
    cancelled.control.cancel();
    let at = std::time::Instant::now();
    assert!(matches!(
        wait_until_available(&later, &cancelled).await,
        Err(Error::Cancelled)
    ));
    assert!(at.elapsed() < Duration::from_secs(2), "the wait was cut");
}

/// Bytes on disk are a ledger of the engine's with more than nothing in
/// it, in a stream's own parts folder; anything else is not.
#[tokio::test]
async fn bytes_fetched_are_told_by_the_engines_ledgers() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("parts");
    assert!(!fetched_any(&staging).await, "no staging at all");
    std::fs::create_dir_all(staging.join("elsewhere/parts")).unwrap();
    std::fs::write(staging.join("elsewhere/parts/done-0"), "99").unwrap();
    std::fs::create_dir_all(staging.join(".edm-f140")).unwrap();
    std::fs::create_dir_all(staging.join(".edm-f137/parts")).unwrap();
    std::fs::write(staging.join(".edm-f137/parts/piece-0"), "99").unwrap();
    std::fs::write(staging.join(".edm-f137/parts/done-0"), "0").unwrap();
    std::fs::write(staging.join(".edm-f137/parts/done-1"), "many").unwrap();
    assert!(!fetched_any(&staging).await);
    std::fs::write(staging.join(".edm-f137/parts/done-2"), " 4096\n").unwrap();
    assert!(fetched_any(&staging).await);
}

/// A refused address that neither ran out nor served bytes is not read
/// again: the page is not asked at all.
#[tokio::test]
async fn a_refusal_at_the_first_ask_is_not_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let r = read(
        r#"{"filename": "/s/a.mp4", "protocol": "https",
            "url": "https://x.example/v?expire=99999999999"}"#,
    );
    let plan = r.plan(Path::new("/s")).unwrap();
    assert!(!plan.streams[0].expired());
    let renewed = renewed(
        "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        &plan,
        &options(dir.path()),
        false,
    )
    .await;
    assert!(matches!(renewed, Ok(None)));
    let odd = Stream {
        url: "not an address".into(),
        ..plan.streams[0].clone()
    };
    assert!(!odd.expired());
}

/// Streams are the same read again where their ids, files and stated
/// sizes agree; a size one reading did not state does not count.
#[test]
fn the_same_streams_are_told_by_id_file_and_size() {
    let r = read(
        r#"{"filename": "/s/a.mp4", "requested_formats": [
            {"format_id": "1", "protocol": "https", "url": "https://x/1", "filesize": 10},
            {"format_id": "2", "protocol": "https", "url": "https://x/2"}]}"#,
    );
    let a = r.plan(Path::new("/s")).unwrap();
    let b = r.plan(Path::new("/s")).unwrap();
    assert!(a.same_streams(&b));
    let mut no_size = r.plan(Path::new("/s")).unwrap();
    no_size.streams[0].size = None;
    assert!(a.same_streams(&no_size));
    let mut bigger = r.plan(Path::new("/s")).unwrap();
    bigger.streams[0].size = Some(11);
    assert!(!a.same_streams(&bigger));
    let mut other = r.plan(Path::new("/s")).unwrap();
    other.streams[1].id = "3".into();
    assert!(!a.same_streams(&other));
    let mut fewer = r.plan(Path::new("/s")).unwrap();
    fewer.streams.pop();
    assert!(!a.same_streams(&fewer));
    // A range that is not one, or too large to be a stream's first boxes.
    assert_eq!(range_of(Some("10-5")), None);
    assert_eq!(range_of(Some("0-99999999")), None);
    assert_eq!(range_of(Some("a-b")), None);
    assert_eq!(range_of(Some(" 0-740 ")), Some((0, 740)));
}

/// The subtitles a reading names are fetched beside the video as
/// `name.<language>.vtt`; one that is not WebVTT, has an odd language,
/// no address or cannot be had is passed over.
#[tokio::test]
async fn the_subtitles_named_are_left_beside_the_video() {
    let app = axum::Router::new().route(
        "/en.vtt",
        axum::routing::get(|| async { "WEBVTT\n\n00:00.000 --> 00:01.000\nhello\n" }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let at = |path: &str| format!("http://{addr}{path}");
    let r = read(
        &serde_json::json!({"requested_subtitles": {
            "en": {"url": at("/en.vtt"), "ext": "vtt"},
            "fr": {"url": at("/en.vtt"), "ext": "srv3"},
            "../x": {"url": at("/en.vtt"), "ext": "vtt"},
            "": {"url": at("/en.vtt"), "ext": "vtt"},
            "de": {"url": at("/missing.vtt"), "ext": "vtt"},
            "es": {"ext": "vtt"},
        }})
        .to_string(),
    );
    let dir = tempfile::tempdir().unwrap();
    let produced = dir.path().join("Talk [x].mp4");
    // With subtitles off, none is fetched: what is fetched is only to be put inside.
    subtitles(&crate::client(), &r, &produced, &options(dir.path())).await;
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let carrying = Options {
        subtitles: true,
        ..options(dir.path())
    };
    subtitles(&crate::client(), &r, &produced, &carrying).await;
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["Talk [x].en.vtt"]);
    // None asked, or a video with no name: nothing.
    subtitles(
        &crate::client(),
        &read("{}"),
        &produced,
        &options(dir.path()),
    )
    .await;
    subtitles(&crate::client(), &r, Path::new("/"), &options(dir.path())).await;
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// A retry a stream says goes to the row as it is; its name and
/// validator do not, and its progress is summed with the others'.
#[tokio::test]
async fn a_streams_retry_is_passed_on_and_its_name_is_not() {
    let (report, mut heard) = tokio::sync::mpsc::channel(64);
    let (senders, merging) = merge_progress(Some(report), 1);
    let tx = senders[0].clone().unwrap();
    drop(senders);
    let progress = crate::Progress {
        downloaded: 5,
        total: Some(10),
        speed: 1.0,
        eta: None,
        estimated: false,
    };
    tx.send(Update::Named {
        name: "x.f137.mp4".into(),
        category: category::classify("x.mp4", None),
    })
    .await
    .unwrap();
    tx.send(Update::Validator("\"v\"".into())).await.unwrap();
    tx.send(Update::Progress(progress)).await.unwrap();
    tx.send(Update::Retrying {
        attempt: 2,
        of: 5,
        why: "reset".into(),
        wait: Duration::from_secs(1),
    })
    .await
    .unwrap();
    drop(tx);
    merging.unwrap().await.unwrap();
    let mut got = Vec::new();
    while let Ok(update) = heard.try_recv() {
        got.push(update);
    }
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(matches!(&got[0], Update::Progress(p) if p.downloaded == 5 && p.total == Some(10)));
    assert!(matches!(&got[1], Update::Retrying { attempt: 2, .. }));
    // Without a row to tell, each stream gets no sender and nothing merges.
    let (none, merging) = merge_progress(None, 2);
    assert!(none.iter().all(Option::is_none) && none.len() == 2);
    assert!(merging.is_none());
}

/// A type that is no file name's, and names a reader numbers but that
/// are not that reader's shape, are left as they are.
#[test]
fn odd_types_and_odd_numbered_names_are_left_alone() {
    let odd = read(
        r#"{"filename": "/s/a.mp4", "ext": "m p4", "protocol": "https", "url": "https://x/1"}"#,
    );
    assert_eq!(odd.plan(Path::new("/s")), None);
    let named = |filename: &str| {
        read(
            &serde_json::json!({"filename": filename, "extractor_key": "HTML5MediaEmbed",
            "id": "watch-1", "protocol": "http", "url": "http://site/clip.mp4"})
            .to_string(),
        )
        .plan(Path::new("/s"))
        .unwrap()
        .produced
    };
    assert_eq!(
        named("/s/QA clip [watch-1].mp4"),
        PathBuf::from("/s/QA clip [watch-1].mp4")
    );
    assert_eq!(
        named("/s/QA (1) [x [watch-1].mp4"),
        PathBuf::from("/s/QA (1) [x [watch-1].mp4")
    );
    assert_eq!(
        named("/s/QA (1) [watch-2].mp4"),
        PathBuf::from("/s/QA (1) [watch-2].mp4")
    );
}

/// Past its bound the table of agents gives a browser's, rather than
/// keep every agent a reading names.
#[test]
fn the_table_of_agents_is_bounded() {
    let given: Vec<&'static str> = (0..40)
        .map(|n| interned(&format!("Bounded/{n} (coverage)")))
        .collect();
    assert!(given.contains(&crate::BROWSER_AGENT));
    assert_eq!(interned("Bounded/0 (coverage)"), "Bounded/0 (coverage)");
}

/// Numbered pieces (`http_dash_segments`) are fetched by the DASH piece fetcher where EDM's own
/// reading says so (gaps row Y14); any other reading's are not EDM's to fetch as files.
#[test]
fn numbered_pieces_are_planned_from_dms_own_reading_alone() {
    let reading = |by: &str| {
        read(
            &serde_json::json!({"filename": "/s/a.mp4", "read_by": by, "requested_formats": [
                {"format_id": "137", "ext": "mp4", "protocol": "http_dash_segments",
                 "url": "https://r1.example/v?itag=137"},
                {"format_id": "140", "ext": "m4a", "protocol": "https",
                 "url": "https://r1.example/a?itag=140"}]})
            .to_string(),
        )
    };
    let plan = reading("EDM").plan(Path::new("/s")).unwrap();
    assert!(plan.streams[0].numbered);
    assert!(!plan.streams[1].numbered);
    assert_eq!(reading("another").plan(Path::new("/s")), None);
}
