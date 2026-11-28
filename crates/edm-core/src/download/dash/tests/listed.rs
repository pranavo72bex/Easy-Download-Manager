// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Listed streams: a one-file track stopped part way, a part of a stream, a failed sound resumed.

use super::*;

/// A picture of four fragments of a tenth of a second each, listed by byte range in an
/// MPD of its own (with a sound in a file of its own, when `sound` is given) on 127.0.0.1.
/// The sound answers 500 while `failing` is set; what is given back counts the picture's requests.
async fn serve_listed(
    sound: bool,
    audio_only: bool,
    failing: Arc<std::sync::atomic::AtomicBool>,
) -> (String, tempfile::TempDir, Arc<AtomicU64>) {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 4);
    let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
    let video = std::fs::read(video).unwrap();
    let audio = std::fs::read(audio).unwrap();
    let starts = box_starts(&video);
    let mut list = format!(r#"<Initialization range="0-{}"/>"#, starts[2] - 1);
    for f in 0..4 {
        let (a, b) = (starts[2 + f * 2], starts[4 + f * 2] - 1);
        list.push_str(&format!(r#"<SegmentURL mediaRange="{a}-{b}"/>"#));
    }
    let picture = format!(
        r#"<AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="9" height="360">
              <BaseURL>v.mp4</BaseURL><SegmentList timescale="10" duration="1">{list}</SegmentList></Representation></AdaptationSet>"#
    );
    let sound_set = r#"<AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1">
              <BaseURL>a.m4a</BaseURL></Representation></AdaptationSet>"#;
    let sets = match (audio_only, sound) {
        (true, _) => sound_set.to_string(),
        (false, true) => format!("{picture}{sound_set}"),
        (false, false) => picture,
    };
    let mpd = format!(
        r#"<MPD type="static" mediaPresentationDuration="PT0.4S"><Period>{sets}</Period></MPD>"#
    );
    let hits = Arc::new(AtomicU64::new(0));
    let counted = hits.clone();
    let picture_route = ranged(video, true);
    let sound_route = ranged(audio, true);
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
        .route("/show/v.mp4", picture_route)
        .route("/show/a.m4a", sound_route)
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let (failing, counted) = (failing.clone(), counted.clone());
                async move {
                    let path = request.uri().path().to_string();
                    if path.ends_with("v.mp4") {
                        counted.fetch_add(1, Ordering::SeqCst);
                    }
                    if path.ends_with("a.m4a") && failing.load(Ordering::SeqCst) {
                        return Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::empty())
                            .unwrap();
                    }
                    next.run(request).await
                }
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/show/video.mpd"), dir, hits)
}

#[tokio::test]
async fn a_stream_being_joined_names_its_row_and_reports_its_bytes() {
    let (url, dir, _) = serve(true).await;
    let options = options(dir.path());
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let done =
        crate::download::download_reporting(&crate::reqwest_defaults(), &url, &options, Some(tx))
            .await
            .unwrap();
    let mut said = Vec::new();
    while let Ok(update) = rx.try_recv() {
        said.push(update);
    }
    assert!(
        said.iter()
            .any(|u| matches!(u, Update::Named { name, .. } if name == "show.mp4")),
        "{said:?}"
    );
    assert_eq!(done.path.file_name().unwrap(), "show.mp4");
}

/// A track that is one whole file of 96 MiB is fetched in ranged pieces of 8 MiB, each a
/// part file of its own: stopped part-way, the next run fetches only the pieces it has not
/// got, not the file from its start (gaps row Y17). Its mdat holds the sound's samples, then
/// bytes nothing refers to, so the file is a real sound file of that size.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_one_file_track_stopped_part_way_goes_on_from_its_pieces() {
    let dir = tempfile::tempdir().unwrap();
    let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
    let mut audio = std::fs::read(audio).unwrap();
    // The mdat is last: grown by 96 MiB its header says so.
    const MORE: usize = 96 * 1024 * 1024;
    let mdat = audio.windows(4).rposition(|w| w == b"mdat").unwrap() - 4;
    let size = u32::from_be_bytes(audio[mdat..mdat + 4].try_into().unwrap()) as usize + MORE;
    audio[mdat..mdat + 4].copy_from_slice(&(size as u32).to_be_bytes());
    audio.resize(audio.len() + MORE, 0);
    let total = audio.len() as u64;
    let served = Arc::new(AtomicU64::new(0));
    let mpd = r#"<MPD type="static" mediaPresentationDuration="PT0.2S"><Period>
            <AdaptationSet mimeType="audio/mp4"><Representation id="a" bandwidth="1">
            <BaseURL>a.m4a</BaseURL></Representation></AdaptationSet></Period></MPD>"#;
    let audio = Arc::new(audio);
    let counted = Arc::clone(&served);
    let app = axum::Router::new()
        .route("/show/sound.mpd", get(move || async move { mpd }))
        .route(
            "/show/a.m4a",
            get(move |headers: HeaderMap| {
                let (audio, counted) = (Arc::clone(&audio), Arc::clone(&counted));
                async move {
                    let (a, b) = headers
                        .get(header::RANGE)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("bytes="))
                        .and_then(|v| v.split_once('-'))
                        .and_then(|(a, b)| {
                            Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                        })
                        .unwrap_or((0, audio.len() - 1));
                    let b = b.min(audio.len() - 1);
                    counted.fetch_add((b + 1 - a) as u64, Ordering::SeqCst);
                    // Slow enough to be stopped part-way: 2 ms a MiB.
                    tokio::time::sleep(std::time::Duration::from_millis(
                        ((b + 1 - a) >> 20) as u64 * 2,
                    ))
                    .await;
                    Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(
                            header::CONTENT_RANGE,
                            format!("bytes {a}-{b}/{}", audio.len()),
                        )
                        .body(Body::from(audio[a..=b].to_vec()))
                        .unwrap()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let url = format!("http://{addr}/show/sound.mpd");

    let first = options(dir.path());
    let (control, parts) = (first.control.clone(), first.parts_dir.clone());
    let pieces = |parts: &Path| {
        std::fs::read_dir(parts).map_or(0, |d| {
            d.flatten()
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.starts_with("part-") && !name.ends_with(".partial")
                })
                .count()
        })
    };
    let watching = {
        let parts = parts.clone();
        tokio::spawn(async move {
            while pieces(&parts) < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            control.pause();
        })
    };
    let stopped = crate::download::download(&crate::reqwest_defaults(), &url, &first).await;
    watching.abort();
    assert!(stopped.is_err(), "the first run was meant to be stopped");
    let kept = pieces(&parts);
    assert!((3..12).contains(&kept), "{kept} pieces kept of 12");
    let before = served.load(Ordering::SeqCst);

    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options(dir.path()))
        .await
        .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "sound.m4a");
    let again = served.load(Ordering::SeqCst) - before;
    // What was kept is not asked for again: at most what was missing, and a piece or two
    // under way when it stopped.
    let missing = total - kept as u64 * ONE_FILE_PIECE;
    assert!(
        again <= missing + 4 * ONE_FILE_PIECE,
        "the second run fetched {again} bytes where {missing} were missing"
    );
}

#[tokio::test]
async fn a_stream_of_sound_alone_is_saved_as_an_m4a() {
    let idle = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (url, dir, _) = serve_listed(true, true, idle).await;
    let done = crate::download::download(&crate::reqwest_defaults(), &url, &options(dir.path()))
        .await
        .unwrap();
    assert_eq!(done.path.file_name().unwrap(), "show.m4a");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 1);
}

#[tokio::test]
async fn a_part_of_a_listed_stream_fetches_only_the_pieces_it_needs() {
    let idle = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (url, dir, hits) = serve_listed(false, false, idle).await;
    let client = crate::reqwest_defaults();
    let whole = crate::download::download(&client, &url, &options(dir.path()))
        .await
        .unwrap();
    let whole_hits = hits.swap(0, Ordering::SeqCst);
    let mut cut = options(dir.path());
    cut.span = crate::clip::Span::new(0.1, 0.2);
    let part = crate::download::download(&client, &url, &cut)
        .await
        .unwrap();
    let name = part
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("show (") && name.ends_with(").mp4"),
        "{name}"
    );
    assert!(
        hits.load(Ordering::SeqCst) < whole_hits,
        "{} pieces for a part, {whole_hits} for the whole",
        hits.load(Ordering::SeqCst)
    );
    let data = std::fs::read(&part.path).unwrap();
    assert_eq!(&data[4..8], b"ftyp");
    assert!(data.len() < std::fs::read(&whole.path).unwrap().len());

    // A span past the end of the stream is said to be outside it.
    let mut outside = options(dir.path());
    outside.span = crate::clip::Span::new(50.0, 60.0);
    let why = crate::download::download(&client, &url, &outside)
        .await
        .unwrap_err()
        .to_string();
    assert!(why.contains("outside the stream"), "{why}");
}

#[tokio::test]
async fn a_stream_whose_sound_failed_is_resumed_without_fetching_the_picture_again() {
    let failing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (url, dir, hits) = serve_listed(true, false, failing.clone()).await;
    let client = crate::reqwest_defaults();
    let options = options(dir.path());
    assert!(crate::download::download(&client, &url, &options)
        .await
        .is_err());
    assert!(
        hits.load(Ordering::SeqCst) > 0,
        "the picture was fetched first"
    );
    hits.store(0, Ordering::SeqCst);
    failing.store(false, Ordering::SeqCst);
    let done = crate::download::download(&client, &url, &options)
        .await
        .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the picture was kept");
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
}
