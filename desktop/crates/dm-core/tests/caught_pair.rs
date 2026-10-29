//! A picture and a sound a page's player fetched apart (what the extension
//! catches: `browser/src/decide/streams.rs` `track_of`) are fetched by dm
//! itself and joined into one file, with no yt-dlp on the computer at all.
//! Its own test binary: it says through the environment that there is no
//! yt-dlp, and a home of its own so nothing could be fetched into a real one.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;

use common::{options, serve, tempdir};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mp4")
            .join(name),
    )
    .unwrap()
}

/// Serves `data` with byte ranges, as a player's server does, and notes the
/// agent of each request.
fn ranged(data: Vec<u8>, seen: Arc<Mutex<Vec<String>>>) -> axum::routing::MethodRouter {
    get(move |headers: HeaderMap| {
        let data = data.clone();
        let seen = seen.clone();
        async move {
            seen.lock().unwrap().push(
                headers
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string(),
            );
            let total = data.len();
            let (start, end) = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| {
                    let (a, b) = r.split_once('-')?;
                    let start: usize = a.parse().ok()?;
                    let end = b.parse::<usize>().map_or(total - 1, |b| b.min(total - 1));
                    Some((start, end))
                })
                .unwrap_or((0, total - 1));
            let partial = headers.contains_key("range");
            Response::builder()
                .status(if partial {
                    StatusCode::PARTIAL_CONTENT
                } else {
                    StatusCode::OK
                })
                .header("Accept-Ranges", "bytes")
                .header("Content-Length", end - start + 1)
                .header("Content-Type", "video/mp4")
                .header("Content-Range", format!("bytes {start}-{end}/{total}"))
                .body(Body::from(data[start..=end].to_vec()))
                .unwrap()
        }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_the_player_fetched_apart_is_fetched_and_joined_with_no_yt_dlp_at_all() {
    let dir = tempdir();
    // No yt-dlp anywhere: not installed, none fetched, none to fetch into.
    std::env::set_var("DM_YTDLP", "");
    std::env::set_var("DM_HOME", dir.path());
    assert!(
        !dm_core::ytdlp::available(),
        "the test is not without yt-dlp"
    );

    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = axum::Router::new()
        .route(
            "/player/picture",
            ranged(fixture("picture.f137.mp4"), seen.clone()),
        )
        .route(
            "/player/sound",
            ranged(fixture("sound.f140.m4a"), seen.clone()),
        );
    let addr = serve(app).await;

    let mut opts = options(dir.path());
    opts.sound = Some(format!("http://{addr}/player/sound"));
    opts.referrer = Some(format!("http://{addr}/watch"));
    opts.save_name = Some("Talk".into());
    let done = dm_core::download(
        &dm_core::client(),
        &format!("http://{addr}/player/picture"),
        &opts,
    )
    .await
    .expect("the pair did not download");

    assert_eq!(done.path.file_name().unwrap(), "Talk.mp4");
    let joined = std::fs::read(&done.path).unwrap();
    let has = |kind: &[u8]| joined.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 2, "picture and sound are both in it");
    assert!(
        has(b"avc1") >= 1 && has(b"mp4a") >= 1,
        "a picture track and a sound track"
    );
    let seen = seen.lock().unwrap();
    assert!(seen.len() >= 2, "both streams were asked for: {seen:?}");
    assert!(
        seen.iter().all(|agent| agent.starts_with("dm/")),
        "fetched by dm itself, not a helper: {seen:?}"
    );
}
