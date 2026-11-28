// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The quality list and the download disagree (gaps row Y9): the list is read from the watch
//! page's own player data, the download from a client's answer, which lacks the page's 1080p
//! picture and holds a second 720p one the height's rules would take. Against a stand-in for
//! YouTube (its watch page, its player API and the streams are a local server's):
//!
//! * a line picked is fetched by the format number it was listed from;
//! * a line whose picture the answer lacks is fetched at the nearest, not refused;
//! * a page with no player data is listed from EDM's own reading;
//! * the watch page the list is read from is fetched once, for the download too (gaps row Y13).
//!
//! The stand-in is named by an environment variable, which is the process's: one server for
//! every test here, each case a video id of its own.

mod common;

#[path = "common/mp4_samples.rs"]
mod mp4_samples;

use std::sync::{Mutex, OnceLock};

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use edm_core::{client, videopage, Options, Quality};

use common::{options, tempdir};
use mp4_samples::tracks;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mp4")
            .join(name),
    )
    .unwrap()
}

fn picture() -> &'static [u8] {
    static PICTURE: OnceLock<Vec<u8>> = OnceLock::new();
    PICTURE.get_or_init(|| fixture("picture.f137.mp4"))
}

fn sound() -> &'static [u8] {
    static SOUND: OnceLock<Vec<u8>> = OnceLock::new();
    SOUND.get_or_init(|| fixture("sound.f140.m4a"))
}

/// A piece of `data` as a range request asks for it.
fn ranged(data: &[u8], headers: &HeaderMap) -> Response {
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
        .body(Body::from(bytes::Bytes::copy_from_slice(&data[a..=b])))
        .unwrap()
}

fn text(status: StatusCode, kind: &str, body: String) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", kind)
        .body(Body::from(body))
        .unwrap()
}

/// The video whose watch page carries no player data.
const NO_PAGE_DATA: &str = "nopagedata1";

/// Every stream request the stand-in was sent.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The query of every watch page the stand-in was asked for.
static WATCHED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The stand-in's origin, started once for every test here on a runtime of its own.
fn stand_in() -> &'static str {
    static ORIGIN: OnceLock<String> = OnceLock::new();
    ORIGIN.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let origin = format!("http://{}", listener.local_addr().unwrap());
                let api = {
                    let origin = origin.clone();
                    move |body: String| player(origin.clone(), body)
                };
                let app = axum::Router::new()
                    .route("/watch", get(watch_page))
                    .route("/youtubei/v1/player", post(api))
                    .fallback(get(streams));
                tx.send(origin).unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        });
        let origin = rx.recv().unwrap();
        std::env::set_var("EDM_YOUTUBE_HOST", &origin);
        origin
    })
}

/// The watch page: the player's timestamp and the visitor id (no stream needs solving), and
/// the page's own player data, listing a 1080p picture (137), a 720p one (136) and the sound;
/// none for [`NO_PAGE_DATA`].
async fn watch_page(uri: Uri) -> Response {
    WATCHED
        .lock()
        .unwrap()
        .push(uri.query().unwrap_or("").to_string());
    let id = uri
        .query()
        .unwrap_or("")
        .split('&')
        .find_map(|pair| pair.strip_prefix("v="))
        .unwrap_or("")
        .to_string();
    let config = r#"<script>ytcfg.set({"PLAYER_JS_URL":"/s/player/abcd1234/player_ias.vflset/en_US/base.js","STS":12345,"VISITOR_DATA":"visitor-1"});</script>"#;
    let data = if id == NO_PAGE_DATA {
        String::new()
    } else {
        let player = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {"videoId": id, "isLive": false},
            "streamingData": {"adaptiveFormats": [
                {"itag": 137, "mimeType": "video/mp4; codecs=\"avc1.640028\"", "width": 1920,
                 "height": 1080, "fps": 30, "contentLength": "8000"},
                {"itag": 136, "mimeType": "video/mp4; codecs=\"avc1.4d401f\"", "width": 1280,
                 "height": 720, "fps": 30, "contentLength": "2000"},
                {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                 "averageBitrate": 130000, "contentLength": "500"}
            ]}
        });
        format!("<script>var ytInitialPlayerResponse = {player};var meta = 1;</script>")
    };
    text(
        StatusCode::OK,
        "text/html",
        format!("<html>{config}{data}</html>"),
    )
}

/// The player API's answer (the client a download reads): no 1080p picture; at 720p the
/// page's 136 and another, 398, at a higher bitrate, which the height's rules alone take; the
/// sound.
async fn player(origin: String, body: String) -> Response {
    let asked: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let id = asked["videoId"].as_str().unwrap_or("").to_string();
    let answer = serde_json::json!({
        "playabilityStatus": {"status": "OK"},
        "videoDetails": {"videoId": id, "title": "Match", "lengthSeconds": "1"},
        "streamingData": {"adaptiveFormats": [
            {"itag": 136, "mimeType": "video/mp4; codecs=\"avc1.4d401f\"", "width": 1280,
             "height": 720, "fps": 30, "bitrate": 1000000,
             "contentLength": picture().len().to_string(),
             "url": format!("{origin}/picture?id={id}&itag=136")},
            {"itag": 398, "mimeType": "video/mp4; codecs=\"av01.0.05M.08\"", "width": 1280,
             "height": 720, "fps": 30, "bitrate": 3000000,
             "contentLength": picture().len().to_string(),
             "url": format!("{origin}/picture?id={id}&itag=398")},
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "averageBitrate": 130000,
             "contentLength": sound().len().to_string(),
             "url": format!("{origin}/sound?id={id}&itag=140")}
        ]}
    });
    text(StatusCode::OK, "application/json", answer.to_string())
}

/// The streams, by range.
async fn streams(uri: Uri, headers: HeaderMap) -> Response {
    SEEN.lock().unwrap().push(uri.to_string());
    match uri.path() {
        "/picture" => ranged(picture(), &headers),
        "/sound" => ranged(sound(), &headers),
        _ => text(StatusCode::NOT_FOUND, "text/plain", format!("no {uri}")),
    }
}

/// The picture's format numbers asked for `id` so far.
fn pictures_for(id: &str) -> Vec<String> {
    SEEN.lock()
        .unwrap()
        .iter()
        .filter(|u| u.starts_with("/picture?") && u.contains(&format!("id={id}&")))
        .filter_map(|u| u.split("itag=").nth(1).map(str::to_string))
        .collect()
}

fn page(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// The options a line picked in the panel downloads with, as the queue reads its word.
fn picked(dir: &std::path::Path, word: &str) -> Options {
    let mut options = options(dir);
    options.quality = Quality::named(word);
    options.container = Quality::container_of(word);
    options.frame_rate = Quality::frame_rate_of(word);
    options.format = Quality::format_of(word);
    options.media = true;
    options
}

/// The list's lines, the sound as MP3 left out (it is there where EDM can make one).
fn lines(list: &[videopage::Choice]) -> Vec<(&str, &str, Option<u64>)> {
    list.iter()
        .filter(|c| c.quality != "audio/mp3")
        .map(|c| (c.label.as_str(), c.quality.as_str(), c.size))
        .collect()
}

/// The joined file holds a picture and a sound.
fn picture_and_sound(path: &std::path::Path) {
    let joined = std::fs::read(path).unwrap();
    let all = tracks(&joined);
    assert_eq!(all.len(), 2, "picture and sound are both in it");
    assert!(all.iter().any(|t| &t.handler == b"vide"));
    assert!(all.iter().any(|t| &t.handler == b"soun"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_line_picked_is_fetched_by_its_format_number_else_the_nearest() {
    stand_in();
    let id = "formatmatch";

    // The list is the page's: its 1080p line (137) is one the download's reading lacks.
    let list = videopage::list_choices(&page(id), None, None, None)
        .await
        .unwrap();
    assert_eq!(
        lines(&list),
        [
            ("1080p MP4", "1080/mp4/f137", Some(8500)),
            ("720p MP4", "720/mp4/f136", Some(2500)),
            ("M4A audio", "audio/m4a", Some(500)),
        ]
    );

    // The 720p line: its own picture, 136, though the height's rules alone take 398.
    let dir = tempdir();
    let done = videopage::download_page(
        &client(),
        &page(id),
        &picked(dir.path(), &list[1].quality),
        None,
    )
    .await
    .expect("the 720p line did not download");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Match [formatmatch].mp4"
    );
    picture_and_sound(&done.path);
    let asked = pictures_for(id);
    assert!(
        !asked.is_empty() && asked.iter().all(|i| i == "136"),
        "{asked:?}"
    );

    // The 1080p line, whose picture the reading lacks: the nearest, not "no stream at that
    // quality". (Another video of the same page data, so the requests are its own.)
    let near = "formatnear1";
    let dir = tempdir();
    let done = videopage::download_page(
        &client(),
        &page(near),
        &picked(dir.path(), &list[0].quality),
        None,
    )
    .await
    .expect("the 1080p line the reading lacks was refused");
    picture_and_sound(&done.path);
    let asked = pictures_for(near);
    assert!(
        !asked.is_empty() && asked.iter().all(|i| i == "398"),
        "{asked:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_without_player_data_is_listed_from_dms_own_reading() {
    stand_in();
    let list = videopage::list_choices(&page(NO_PAGE_DATA), None, None, None)
        .await
        .unwrap();
    // The reading's 720p pictures are one line, by its first MP4 the size of the largest;
    // its sizes are the reading's, the picture's joined with the sound's.
    let alone = sound().len() as u64;
    let whole = picture().len() as u64 + alone;
    assert_eq!(
        lines(&list),
        [
            ("720p MP4", "720/mp4/f136", Some(whole)),
            ("M4A audio", "audio/m4a", Some(alone)),
        ]
    );
    // Picking it fetches that picture.
    let dir = tempdir();
    let done = videopage::download_page(
        &client(),
        &page(NO_PAGE_DATA),
        &picked(dir.path(), &list[0].quality),
        None,
    )
    .await
    .expect("the line listed from the reading did not download");
    picture_and_sound(&done.path);
    let asked = pictures_for(NO_PAGE_DATA);
    assert!(
        !asked.is_empty() && asked.iter().all(|i| i == "136"),
        "{asked:?}"
    );
}

/// The page the list was read from is the page the download of a line picked reads, asked the
/// same way (its address past the check some pages ask first): fetched once for both, not once
/// for each (gaps row Y13).
#[tokio::test(flavor = "multi_thread")]
async fn the_list_and_the_download_of_a_line_fetch_the_watch_page_once() {
    stand_in();
    let id = "onefetch001";
    let list = videopage::list_choices(&page(id), None, None, None)
        .await
        .unwrap();
    assert_eq!(list[1].quality, "720/mp4/f136", "{list:?}");
    let dir = tempdir();
    let done = videopage::download_page(
        &client(),
        &page(id),
        &picked(dir.path(), &list[1].quality),
        None,
    )
    .await
    .expect("the line picked did not download");
    picture_and_sound(&done.path);
    let watched: Vec<String> = WATCHED
        .lock()
        .unwrap()
        .iter()
        .filter(|query| query.starts_with(&format!("v={id}&")))
        .cloned()
        .collect();
    assert_eq!(watched, [format!("v={id}&bpctr=9999999999&has_verified=1")]);
}
