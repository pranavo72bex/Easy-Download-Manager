//! "MP3 audio" picked for a video's sound, through the queue as a hand-off from
//! the browser's panel arrives: the M4A is fetched and the row ends as an MP3
//! of the same length, with nothing installed beyond dm (on Linux the AAC is
//! decoded by the decoder built into dm; on macOS and Windows by the system's).

mod common;

use axum::body::Body;
use axum::response::Response;
use axum::routing::get;
use dm_core::client;

use common::{options, serve, tempdir};

fn sound() -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/tone.stereo.m4a"),
    )
    .unwrap()
}

#[tokio::test]
async fn a_sound_asked_for_as_mp3_lands_as_an_mp3_of_its_length() {
    if !dm_core::mp3::possible() {
        eprintln!("no AAC decoder here (a Windows N edition): nothing to check");
        return;
    }
    let data = sound();
    let len = data.len();
    let addr = serve(axum::Router::new().route(
        "/videoplayback.m4a",
        get(move || {
            let data = data.clone();
            async move {
                Response::builder()
                    .status(200)
                    .header("content-type", "audio/mp4")
                    .header("content-length", len)
                    .body(Body::from(data))
                    .unwrap()
            }
        }),
    ))
    .await;
    let dir = tempdir();
    let q = dm_core::Queue::new(client(), options(dir.path()), 1);
    let id = q
        .add_with(
            &format!("http://{addr}/videoplayback.m4a"),
            dm_core::AddOptions {
                quality: Some("audio/mp3".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(
        std::path::Path::new(&row.path)
            .extension()
            .and_then(|e| e.to_str()),
        Some("mp3"),
        "{:?}",
        row.path
    );
    let path = std::path::Path::new(&row.path);
    let bytes = std::fs::read(path).unwrap();
    // Whole MPEG-1 Layer III frames at 44.1 kHz, stereo 192 kbit/s, 2 s of them.
    let (mut at, mut frames) = (0usize, 0u64);
    while at + 4 <= bytes.len() {
        assert_eq!(
            (bytes[at], bytes[at + 1] & 0xFE),
            (0xFF, 0xFA),
            "no MPEG-1 Layer III frame at {at}"
        );
        at += 144 * 192_000 / 44100 + usize::from((bytes[at + 2] >> 1) & 1);
        frames += 1;
    }
    assert_eq!(at, bytes.len(), "frames do not tile the file");
    let seconds = frames as f64 * 1152.0 / 44100.0;
    assert!((seconds - 2.0).abs() < 0.1, "{seconds} s");
    // The M4A it came from is not left beside it.
    let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
}
