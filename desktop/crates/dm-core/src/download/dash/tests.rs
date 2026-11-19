// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The DASH download's tests, and the helpers the other streams' tests share.

use super::*;

mod hls_pairs;
mod listed;

/// A manifest's size: exact from every piece's bytes, else about, from the bitrate and the
/// playing time (gaps row Y22).
#[test]
fn a_manifests_size_is_exact_from_its_ranges_else_about() {
    use crate::dash::{Medium, Piece, Track};
    let piece = |range, millis| Piece {
        url: "https://cdn.example/v".into(),
        range,
        lock: None,
        millis,
    };
    let track = |pieces| Track {
        medium: Medium::Video,
        id: "v".into(),
        bandwidth: 8_000_000,
        height: 720,
        webm: false,
        pieces,
        indexed: None,
        seams: Vec::new(),
    };
    let ranged = track(vec![
        piece(Some((0, 999)), 0),
        piece(Some((1000, 2999)), 2000),
    ]);
    assert_eq!(size_of_tracks(&[&ranged]), (Some(3000), None));
    let timed = track(vec![piece(None, 2000), piece(None, 2000)]);
    assert_eq!(size_of_tracks(&[&timed]), (None, Some(4_000_000)));
    let silent = track(vec![piece(None, 0)]);
    assert_eq!(size_of_tracks(&[&silent]), (None, None));
}

/// A stream of numbered pieces: each asked for by its number, the rest of the address as
/// it came, and how many there are read from its first (gaps row Y14).
#[test]
fn numbered_pieces_are_asked_by_number_and_counted_from_the_first() {
    assert_eq!(
        numbered_address("https://r1.example/v?itag=137&n=A%3D", 0),
        "https://r1.example/v?itag=137&n=A%3D&sq=0"
    );
    assert_eq!(
        numbered_address("https://r1.example/v", 12),
        "https://r1.example/v?sq=12"
    );
    let first = |words: &str| {
        let mut bytes = b"\0\0\0\x10ftypdash".to_vec();
        bytes.extend_from_slice(words.as_bytes());
        bytes
    };
    assert_eq!(
        segment_count(&first("Sequence-Number: 0\r\nSegment-Count: 1199\r\n")),
        Some(1199)
    );
    assert_eq!(segment_count(&first("Segment-Count:42")), Some(42));
    assert_eq!(segment_count(&first("Sequence-Number: 0\r\n")), None);
    assert_eq!(segment_count(&first("Segment-Count: 0")), None);
    assert_eq!(segment_count(&first("Segment-Count: lots")), None);
    assert_eq!(segment_count(&first("Segment-Count: 99999999999999")), None);
    assert_eq!(segment_count(&first("Segment-Count: 200001")), None);
    assert_eq!(
        segment_count(&first("Segment-Count: 200000")),
        Some(200_000)
    );
}
use crate::mp4mux::testing::{fragmented, plain};
use axum::body::Body;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;

pub(in crate::download) fn options(dir: &Path) -> Options {
    Options {
        connections: 4,
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
        attempts: 2,
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

/// Offsets where each top-level box starts, and the end.
fn box_starts(data: &[u8]) -> Vec<usize> {
    let mut at = 0;
    let mut starts = vec![];
    while at + 8 <= data.len() {
        starts.push(at);
        at += u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    }
    starts.push(data.len());
    starts
}

/// A fragmented file (`mp4mux::testing::fragmented`) cut as a DASH server serves it: its
/// first boxes, then each fragment (`moof` and `mdat`) as a piece of its own.
pub(in crate::download) fn cut_into_pieces(file: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let starts = box_starts(file);
    let init = file[..starts[2]].to_vec();
    // ftyp, moov, then a moof and an mdat for each fragment, then the end.
    let count = (starts.len() - 3) / 2;
    let pieces = (0..count)
        .map(|f| file[starts[2 + 2 * f]..starts[4 + 2 * f]].to_vec())
        .collect();
    (init, pieces)
}

/// Serves each of `files` (an address's path and its bytes) on 127.0.0.1, counting the
/// requests for each path; gives back the server's address (`http://127.0.0.1:port`).
pub(in crate::download) async fn serve_files(
    files: Vec<(String, Vec<u8>)>,
    asked: Arc<std::sync::Mutex<Vec<String>>>,
) -> String {
    let mut app = axum::Router::new();
    for (path, body) in files {
        app = app.route(
            &path,
            get(move || async move { Response::new(Body::from(body)) }),
        );
    }
    let app = app.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let asked = asked.clone();
            async move {
                asked.lock().unwrap().push(request.uri().path().to_string());
                next.run(request).await
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Serves `body`, answering a byte range the way a CDN does.
fn ranged(body: Vec<u8>, honours: bool) -> axum::routing::MethodRouter {
    get(move |headers: HeaderMap| {
        let body = body.clone();
        async move {
            let asked = headers
                .get(header::RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)));
            match asked {
                Some((a, b)) if honours => Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {a}-{}/{}", b.min(body.len() - 1), body.len()),
                    )
                    .body(Body::from(body[a..=b.min(body.len() - 1)].to_vec()))
                    .unwrap(),
                _ => Response::new(Body::from(body)),
            }
        }
    })
}

async fn serve(honours: bool) -> (String, tempfile::TempDir, usize) {
    serve_with(honours, false).await
}

/// [`serve`], with an English subtitle file listed beside the picture and sound where
/// `texts` (gaps row Y20).
async fn serve_with(honours: bool, texts: bool) -> (String, tempfile::TempDir, usize) {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 4);
    let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
    let video = std::fs::read(video).unwrap();
    let audio = std::fs::read(audio).unwrap();
    let starts = box_starts(&video);
    // ftyp, moov, then a moof and an mdat for each of four fragments.
    let init_end = starts[2] - 1;
    let mut list = format!(r#"<Initialization range="0-{init_end}"/>"#);
    for f in 0..4 {
        let (a, b) = (starts[2 + f * 2], starts[4 + f * 2] - 1);
        list.push_str(&format!(r#"<SegmentURL mediaRange="{a}-{b}"/>"#));
    }
    let mpd = format!(
        r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period>
            <AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="9" height="360">
              <BaseURL>v.mp4</BaseURL><SegmentList>{list}</SegmentList></Representation></AdaptationSet>
            <AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1">
              <BaseURL>a.m4a</BaseURL><SegmentBase indexRange="0-1"/></Representation></AdaptationSet>
            {listed}</Period></MPD>"#,
        listed = if texts {
            r#"<AdaptationSet mimeType="text/vtt" lang="en"><Representation id="t" bandwidth="1">
              <BaseURL>en.vtt</BaseURL></Representation></AdaptationSet>"#
        } else {
            ""
        }
    );
    let app = axum::Router::new()
        .route(
            "/show/video.mpd",
            get(move || {
                let mpd = mpd.clone();
                async move {
                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/dash+xml")
                        .body(Body::from(mpd))
                        .unwrap()
                }
            }),
        )
        .route("/show/v.mp4", ranged(video, honours))
        .route("/show/a.m4a", ranged(audio, honours))
        .route(
            "/show/en.vtt",
            get(|| async { "WEBVTT\n\n00:00.500 --> 00:01.500\nHello\n" }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/show/video.mpd"), dir, 2)
}

#[tokio::test]
async fn a_picture_by_byte_ranges_and_a_sound_are_joined_into_one_mp4() {
    let (url, dir, tracks) = serve(true).await;
    let options = options(dir.path());
    let client = crate::reqwest_defaults();
    let done = crate::download::download(&client, &url, &options)
        .await
        .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(&data[4..8], b"ftyp");
    let traks = data.windows(4).filter(|w| *w == b"trak").count();
    assert_eq!(traks, tracks, "a picture and a sound");
}

#[tokio::test]
async fn a_manifests_subtitle_file_is_carried_inside_where_options_says_so() {
    let client = crate::reqwest_defaults();
    for subtitles in [true, false] {
        let (url, dir, tracks) = serve_with(true, true).await;
        let options = Options {
            subtitles,
            ..options(dir.path())
        };
        let done = crate::download::download(&client, &url, &options)
            .await
            .unwrap();
        let data = std::fs::read(&done.path).unwrap();
        let traks = data.windows(4).filter(|w| *w == b"trak").count();
        let text = data.windows(4).any(|w| w == b"tx3g");
        if subtitles {
            assert_eq!((traks, text), (tracks + 1, true), "and the English text");
        } else {
            assert_eq!((traks, text), (tracks, false), "Options says none");
        }
    }
}

#[tokio::test]
async fn a_join_the_disk_cannot_hold_is_refused_before_it_starts_and_its_tracks_kept() {
    let (url, dir, tracks) = serve(true).await;
    let client = crate::reqwest_defaults();
    let full = Options {
        room: crate::Room::with_free_space(|_| Some(100)),
        ..options(dir.path())
    };
    let err = crate::download::download(&client, &url, &full)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("not enough room: the file needs "), "{err}");
    assert!(!full.dest_dir.join("show.mp4").exists());
    let kept = std::fs::read_dir(&full.parts_dir)
        .unwrap()
        .filter(|e| e.as_ref().is_ok_and(|e| e.path().is_file()))
        .count();
    assert!(kept >= tracks, "the fetched tracks are kept: {kept}");
    // Room made: the tracks kept are joined.
    let done = crate::download::download(&client, &url, &options(dir.path()))
        .await
        .unwrap();
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), tracks);
}

#[tokio::test]
async fn a_server_that_ignores_the_range_is_not_written_in_as_the_piece() {
    let (url, dir, _) = serve(false).await;
    let options = options(dir.path());
    let client = crate::reqwest_defaults();
    let err = crate::download::download(&client, &url, &options)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("byte range"), "{err}");
    assert!(!options.dest_dir.join("show.mp4").exists());
}

#[test]
fn a_file_is_cut_into_whole_ranges_with_no_gap_or_overlap() {
    let pieces = pieces_of("http://x/f", 25, 10);
    let ranges: Vec<_> = pieces.iter().map(|p| p.range).collect();
    assert_eq!(ranges, [Some((0, 9)), Some((10, 19)), Some((20, 24))]);
    assert_eq!(pieces_of("http://x/f", 10, 10)[0].range, Some((0, 9)));
    assert_eq!(pieces_of("http://x/f", 0, 10)[0].range, None);
}

#[test]
fn a_query_parameter_is_read_by_name() {
    let url = "https://r1.example/videoplayback?itag=137&mime=video%2Fmp4&clen=99#x";
    assert_eq!(query_of(url, "mime").as_deref(), Some("video/mp4"));
    assert_eq!(query_of(url, "itag").as_deref(), Some("137"));
    assert_eq!(query_of(url, "nope"), None);
}

#[tokio::test]
async fn a_picture_and_a_sound_fetched_apart_are_joined_into_one_mp4() {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 3);
    let (audio, _) = plain(dir.path(), "a.m4a", 5, 120);
    let app = axum::Router::new()
        .route("/p/picture", ranged(std::fs::read(video).unwrap(), true))
        .route("/p/sound", ranged(std::fs::read(audio).unwrap(), true));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut options = options(dir.path());
    options.sound = Some(format!("http://{addr}/p/sound"));
    options.save_name = Some("clip".into());
    let url = format!("http://{addr}/p/picture");
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "clip.mp4");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
}

#[tokio::test]
async fn a_sound_from_a_server_that_ignores_ranges_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 2);
    let (audio, _) = plain(dir.path(), "a.m4a", 3, 50);
    let app = axum::Router::new()
        .route("/p/picture", ranged(std::fs::read(video).unwrap(), true))
        .route("/p/sound", ranged(std::fs::read(audio).unwrap(), false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut options = options(dir.path());
    options.sound = Some(format!("http://{addr}/p/sound"));
    let url = format!("http://{addr}/p/picture");
    let err = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("byte ranges"), "{err}");
}

#[tokio::test]
async fn a_sound_that_claims_a_length_past_any_real_one_is_refused() {
    /* The claimed length was cut into a request per piece before any
     * byte arrived: a one-line header could ask for tens of gigabytes. */
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 2);
    let app = axum::Router::new()
        .route("/p/picture", ranged(std::fs::read(video).unwrap(), true))
        .route(
            "/p/sound",
            axum::routing::get(|| async {
                axum::response::Response::builder()
                    .status(206)
                    .header("content-range", format!("bytes 0-0/{}", u64::MAX))
                    .body(axum::body::Body::from(vec![0u8]))
                    .unwrap()
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut options = options(dir.path());
    options.sound = Some(format!("http://{addr}/p/sound"));
    let url = format!("http://{addr}/p/picture");
    let err = crate::download::download(&crate::reqwest_defaults(), &url, &options)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("bytes long"), "{err}");
}

/// Serves a manifest of one picture and one sound, each one whole file
/// of its type, and returns its address.
async fn serve_pair(picture: (&str, Vec<u8>), sound: (&str, Vec<u8>)) -> String {
    let mpd = format!(
        r#"<MPD type="static" mediaPresentationDuration="PT2S"><Period>
            <AdaptationSet mimeType="{}"><Representation id="v" bandwidth="9" height="90">
              <BaseURL>picture</BaseURL><SegmentBase indexRange="0-1"/></Representation></AdaptationSet>
            <AdaptationSet mimeType="{}"><Representation id="a" bandwidth="1">
              <BaseURL>sound</BaseURL><SegmentBase indexRange="0-1"/></Representation></AdaptationSet>
            </Period></MPD>"#,
        picture.0, sound.0
    );
    let app = axum::Router::new()
        .route(
            "/show/video.mpd",
            get(move || {
                let mpd = mpd.clone();
                async move { Response::new(Body::from(mpd)) }
            }),
        )
        .route("/show/picture", ranged(picture.1, true))
        .route("/show/sound", ranged(sound.1, true));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/show/video.mpd")
}

fn media(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/media/mp4mux")
            .join(name),
    )
    .unwrap()
}

/// A VP8 picture beside an MP4's sound, a pair no MP4 holds, is saved
/// as an MKV rather than refused; an MP4 pair is one when MKV is asked.
#[tokio::test]
async fn a_pair_no_mp4_holds_is_saved_as_an_mkv() {
    let dir = tempfile::tempdir().unwrap();
    let vp8 = dir.path().join("vp8.webm");
    crate::mkvmux::testing::tiny_webm(&vp8, "V_VP8", 1, 5);
    let url = serve_pair(
        ("video/webm", std::fs::read(&vp8).unwrap()),
        ("audio/mp4", media("sound.m4a")),
    )
    .await;
    let plain = options(dir.path());
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &plain)
        .await
        .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.mkv");
    let data = std::fs::read(&done.path).unwrap();
    assert!(data.windows(5).any(|w| w == b"V_VP8"));
    assert!(data.windows(5).any(|w| w == b"A_AAC"));

    let dir = tempfile::tempdir().unwrap();
    let url = serve_pair(
        ("video/mp4", media("picture.mp4")),
        ("audio/mp4", media("sound.m4a")),
    )
    .await;
    let mut asked = options(dir.path());
    asked.container = Some("mkv".into());
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &asked)
        .await
        .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.mkv");
    let data = std::fs::read(&done.path).unwrap();
    assert!(data.windows(15).any(|w| w == b"V_MPEG4/ISO/AVC"));
}

/// The samples of the one track in the MP4 at `path`, as bytes, and their lengths.
pub(in crate::download) fn samples_in(path: &Path) -> (Vec<Vec<u8>>, Vec<u32>) {
    crate::mp4mux::testing::samples_in(path)
}

/// A stand-in for a stream in three periods: a programme's two halves, each with first
/// boxes of its own at an address of its own, and an advert between them in another format
/// (its sample description differs) though the manifest names the same codec. The file
/// plays both halves' samples in order, one after the other, and the advert is left out
/// with a note (gaps row Y18).
#[tokio::test]
async fn the_alike_periods_of_a_stream_play_through_in_order_in_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let (video, samples) = fragmented(dir.path(), "v.mp4", 4);
    let (init, pieces) = cut_into_pieces(&std::fs::read(video).unwrap());
    assert_eq!(pieces.len(), 4);
    let mut advert_init = init.clone();
    let at = advert_init.windows(4).position(|w| w == b"test").unwrap();
    advert_init[at..at + 4].copy_from_slice(b"tesu");
    let period = |id: &str, seconds: &str| {
        format!(
            r#"<Period id="{id}" duration="{seconds}"><AdaptationSet mimeType="video/mp4" codecs="avc1.64001f">
                <SegmentTemplate timescale="10" duration="1" startNumber="1" initialization="{id}/init.mp4" media="{id}/$Number$.m4s"/>
                <Representation id="v" bandwidth="9" height="360"/></AdaptationSet></Period>"#
        )
    };
    let mpd = format!(
        r#"<MPD type="static" mediaPresentationDuration="PT0.5S">{}{}{}</MPD>"#,
        period("one", "PT0.2S"),
        period("ad", "PT0.1S"),
        period("two", "PT0.2S")
    );
    let files = vec![
        ("/show/stream.mpd".to_string(), mpd.into_bytes()),
        ("/show/one/init.mp4".to_string(), init.clone()),
        ("/show/one/1.m4s".to_string(), pieces[0].clone()),
        ("/show/one/2.m4s".to_string(), pieces[1].clone()),
        ("/show/ad/init.mp4".to_string(), advert_init),
        ("/show/ad/1.m4s".to_string(), pieces[0].clone()),
        ("/show/two/init.mp4".to_string(), init),
        ("/show/two/1.m4s".to_string(), pieces[2].clone()),
        ("/show/two/2.m4s".to_string(), pieces[3].clone()),
    ];
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let base = serve_files(files, asked.clone()).await;
    let options = options(dir.path());
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let done = crate::download::download_reporting(
        &crate::reqwest_defaults(),
        &format!("{base}/show/stream.mpd"),
        &options,
        Some(tx),
    )
    .await
    .unwrap();
    let (got, lengths) = samples_in(&done.path);
    assert_eq!(got, samples, "both halves' samples, in order");
    assert_eq!(
        lengths,
        vec![3000; 12],
        "each period plays on from the last"
    );
    let mut notes = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let Update::Note(note) = update {
            notes.push(note);
        }
    }
    assert_eq!(notes, [crate::download::NOTES[3].replace("{}", "1")]);
    // The advert's first boxes were looked at, its pieces never fetched.
    let asked = asked.lock().unwrap().clone();
    assert!(asked.contains(&"/show/ad/init.mp4".to_string()));
    assert!(!asked.contains(&"/show/ad/1.m4s".to_string()));
}
