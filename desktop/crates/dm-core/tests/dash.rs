//! DASH, which the client does not fetch itself: it either hands the manifest
//! to yt-dlp or says why it cannot.
//!
//! Pinned rather than left to the machine. The earlier test asserted "refused
//! or handed over", which on a developer's Mac exercised only the hand-off
//! and on CI only the refusal — either branch could be deleted and the test
//! would still pass somewhere. `DM_YTDLP` names the tool or, empty, says
//! there is none, so each route is driven on purpose: the refusal with the
//! tool declared absent, the hand-off with a stand-in that records what it
//! was asked and produces a file, the way the real one would.
//!
//! Unix only: the stand-in is a shell script.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::header;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use dm_core::{client, download, Checksum, Options};

mod common;
use common::{options, serve, tempdir};

/// The tests set process-wide environment, so they take turns. tokio's
/// mutex, because the turn spans a download's awaits.
static ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const MPD: &str = "<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" \
    type=\"static\"><Period><AdaptationSet mimeType=\"video/mp4\">\
    <Representation id=\"v\" bandwidth=\"400000\" height=\"360\"/>\
    </AdaptationSet></Period></MPD>";

/// A manifest that is DASH by content type alone: the URL does not end in
/// `.mpd`, which is how a CDN's `/manifest?format=dash` looks.
async fn serve_manifest() -> String {
    let app = Router::new().route(
        "/v/manifest",
        get(|| async {
            Response::builder()
                .header(header::CONTENT_TYPE, "application/dash+xml")
                .body(Body::from(MPD))
                .unwrap()
        }),
    );
    let addr = serve(app).await;
    format!("http://{addr}/v/manifest?format=dash")
}

/// A video site's watch page: HTML, which is never the download anyone
/// meant. The second route into yt-dlp, and the one a browser hand-off
/// takes — it had no test that reached it at all.
async fn serve_page() -> String {
    let app = Router::new().route(
        "/watch",
        get(|| async {
            Response::builder()
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from("<html><video src=\"blob:x\"></video></html>"))
                .unwrap()
        }),
    );
    let addr = serve(app).await;
    format!("http://{addr}/watch?v=abc")
}

/// What yt-dlp does, from dm's side: takes the arguments, leaves a file
/// where `-o` said, names it where `--print-to-file` said. Records its
/// arguments so the test can see what it was told. `DM_FAKE_FAIL` makes it
/// fail the way a real one does with two streams and no ffmpeg.
fn fake_ytdlp(dir: &Path) -> PathBuf {
    let script = dir.join("yt-dlp");
    std::fs::write(
        &script,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$DM_FAKE_RECORD"
prev=""
for a in "$@"; do
  [ "$prev" = "--config-locations" ] && cat "$a" > "$DM_FAKE_RECORD.config"
  prev="$a"
done
if [ -n "$DM_FAKE_FAIL" ]; then
  echo "ERROR: [generic] manifest: Requested format is not available" >&2
  exit 1
fi
prev=""
for a in "$@"; do
  [ "$prev" = "after_move:filepath" ] && result="$a"
  [ "$prev" = "-o" ] && template="$a"
  prev="$a"
done
out="$(dirname "$template")/joined [id].mp4"
if [ -n "$DM_FAKE_HALVES" ]; then
  # As yt-dlp leaves a pair it has no ffmpeg to join: side by side under
  # the name it would have joined them as, which it names all the same.
  if [ -n "$DM_FAKE_SUBS" ]; then
    printf 'WEBVTT\n\n00:00:00.200 --> 00:00:00.800\nHello\n' > "$(dirname "$template")/joined [id].en.vtt"
  fi
  for half in "$DM_FAKE_HALVES"/*; do
    name=$(basename "$half")
    cp "$half" "$(dirname "$template")/joined [id].${name#*.}"
  done
else
  printf 'hello dash' > "$out"
fi
echo "[download] Destination: $out"
printf '%s\n' "$out" > "$result"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    // Categories are off in the test options, so nothing nests.
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn recorded(record: &Path) -> Vec<String> {
    std::fs::read_to_string(record)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn without_ytdlp_a_dash_manifest_is_refused_and_names_the_fix() {
    let _turn = ENV.lock().await;
    std::env::set_var("DM_YTDLP", "");

    let url = serve_manifest().await;
    let dir = tempdir();
    let err = download(&client(), &url, &options(dir.path()))
        .await
        .expect_err("a DASH manifest was fetched with nothing to join it");

    let said = err.to_string();
    assert!(
        said.contains("video helper"),
        "the refusal should say what fixes it: {said}"
    );
    // Not the video half, not the manifest's own XML, nothing.
    assert_eq!(files_under(&dir.path().join("out")), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn with_ytdlp_a_dash_manifest_is_handed_over_with_the_proxy_and_the_page() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    let record = dir.path().join("argv");
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", &record);
    std::env::remove_var("DM_FAKE_FAIL");

    let proxy_password = common::secrets::secret("proxy");
    let url = serve_manifest().await;
    let finished = download(
        &client(),
        &url,
        &Options {
            proxy: Some(format!("socks5://me:{proxy_password}@127.0.0.1:1080")),
            referrer: Some("https://page.example/watch".into()),
            ..options(dir.path())
        },
    )
    .await
    .expect("the hand-off failed");

    // Placed like any other download, with what the tool produced.
    assert!(finished.path.starts_with(dir.path().join("out")));
    assert_eq!(std::fs::read(&finished.path).unwrap(), b"hello dash");
    assert_eq!(finished.bytes, 10);

    /* Told the same things every request of dm's own carries. A proxy the
     * client honours and yt-dlp does not is a direct connection from a
     * machine configured to make none. */
    let argv = recorded(&record);
    let after = |flag: &str| {
        argv.iter()
            .position(|a| a == flag)
            .map(|i| argv[i + 1].as_str())
    };
    /* The proxy, with its password, in a file of the run's own rather
     * than on the command line, where any local user's `ps` reads it. */
    assert!(
        !argv.iter().any(|a| a.contains(&proxy_password)),
        "the proxy's password was on yt-dlp's command line: {argv:?}"
    );
    let config = std::fs::read_to_string(record.with_extension("config")).unwrap_or_default();
    assert_eq!(
        config.trim(),
        format!("--proxy 'socks5://me:{proxy_password}@127.0.0.1:1080'")
    );
    assert!(
        !Path::new(after("--config-locations").unwrap()).exists(),
        "the proxy's file outlived the run"
    );
    assert_eq!(after("--referer"), Some("https://page.example/watch"));
    // No ffmpeg: what ffmpeg was asked for, a picture-only stream and its
    // sound, which dm joins itself.
    assert_eq!(after("-f"), Some("bv+ba/b"));
    assert_eq!(argv.last().map(String::as_str), Some(url.as_str()));
    assert!(
        after("--retries").is_some(),
        "the tool's retries were unbounded"
    );
}

#[tokio::test]
async fn without_ytdlp_a_web_page_is_refused_rather_than_saved_as_html() {
    let _turn = ENV.lock().await;
    std::env::set_var("DM_YTDLP", "");

    let url = serve_page().await;
    let dir = tempdir();
    let err = download(&client(), &url, &options(dir.path()))
        .await
        .expect_err("a web page was saved as a download");

    let said = err.to_string();
    assert!(said.contains("video helper"), "{said}");
    // Not the page's own HTML under the name `watch`, which is what this
    // route replaced.
    assert_eq!(files_under(&dir.path().join("out")), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn with_ytdlp_a_web_page_is_handed_over() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    let record = dir.path().join("argv");
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", &record);
    std::env::remove_var("DM_FAKE_FAIL");

    let url = serve_page().await;
    let finished = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the page was not handed to yt-dlp");

    assert_eq!(std::fs::read(&finished.path).unwrap(), b"hello dash");
    let argv = recorded(&record);
    assert_eq!(argv.last().map(String::as_str), Some(url.as_str()));
}

#[tokio::test]
async fn a_digest_that_does_not_match_fails_the_hand_off_and_keeps_nothing() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", dir.path().join("argv"));
    std::env::remove_var("DM_FAKE_FAIL");

    let url = serve_manifest().await;
    let err = download(
        &client(),
        &url,
        &Options {
            checksum: Checksum::parse(&"0".repeat(64)),
            ..options(dir.path())
        },
    )
    .await
    .expect_err("a file that failed its digest was reported as a download");

    assert!(err.to_string().contains("sha256"), "{err}");
    assert_eq!(files_under(&dir.path().join("out")), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn without_ffmpeg_dm_joins_the_picture_and_sound_yt_dlp_left() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", dir.path().join("argv"));
    std::env::remove_var("DM_FAKE_FAIL");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4");
    std::env::set_var("DM_FAKE_HALVES", &fixtures);

    let url = serve_manifest().await;
    let finished = download(&client(), &url, &options(dir.path())).await;
    std::env::remove_var("DM_FAKE_HALVES");
    let finished = finished.expect("the two halves were not joined");

    // One MP4 under the joined name, holding both, and no halves left.
    assert_eq!(
        finished.path.file_name().unwrap().to_string_lossy(),
        "joined [id].mp4"
    );
    let joined = std::fs::read(&finished.path).unwrap();
    let picture = std::fs::read(fixtures.join("picture.f137.mp4")).unwrap();
    let sound = std::fs::read(fixtures.join("sound.f140.m4a")).unwrap();
    assert!(joined.len() < picture.len() + sound.len());
    assert!(joined.len() > picture.len().max(sound.len()));
    let has = |kind: &[u8]| joined.windows(4).filter(|w| *w == kind).count();
    assert_eq!(has(b"trak"), 2, "picture and sound are both in it");
    assert_eq!(has(b"moof"), 0, "written plain, not fragmented");
    assert_eq!(
        files_under(&dir.path().join("out")),
        vec![finished.path.clone()]
    );
}

/// A WebM line picked in the panel, with no ffmpeg: yt-dlp is asked for a
/// WebM picture and sound, and dm joins them into one WebM.
#[tokio::test]
async fn a_webm_asked_for_is_joined_by_dm_into_one_webm() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    let record = dir.path().join("argv");
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", &record);
    std::env::remove_var("DM_FAKE_FAIL");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/webm");
    std::env::set_var("DM_FAKE_HALVES", &fixtures);

    let url = serve_manifest().await;
    let finished = download(
        &client(),
        &url,
        &Options {
            container: Some("webm".into()),
            ..options(dir.path())
        },
    )
    .await;
    std::env::remove_var("DM_FAKE_HALVES");
    let finished = finished.expect("the two halves were not joined");

    assert_eq!(
        finished.path.file_name().unwrap().to_string_lossy(),
        "joined [id].webm"
    );
    let argv = recorded(&record);
    let f = argv
        .iter()
        .position(|a| a == "-f")
        .map(|i| argv[i + 1].clone());
    assert_eq!(
        f.as_deref(),
        Some("bv[ext=webm]+ba[ext=webm]/b[ext=webm]/b")
    );
    let joined = std::fs::read(&finished.path).unwrap();
    assert_eq!(
        joined.get(..4),
        Some(&[0x1A, 0x45, 0xDF, 0xA3][..]),
        "an EBML file"
    );
    let has = |kind: &[u8]| joined.windows(kind.len()).filter(|w| *w == kind).count();
    assert!(
        has(b"V_VP9") == 1 && has(b"A_OPUS") == 1,
        "both tracks in it"
    );
    assert_eq!(
        files_under(&dir.path().join("out")),
        vec![finished.path.clone()]
    );
}

#[tokio::test]
async fn subtitles_are_fetched_and_carried_in_the_video_with_or_without_ffmpeg() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    let record = dir.path().join("argv");
    let tool = fake_ytdlp(dir.path());
    std::env::set_var("DM_YTDLP", &tool);
    // The stand-in is a file, which is all `which` asks of an ffmpeg.
    std::env::set_var("DM_FFMPEG", &tool);
    std::env::set_var("DM_FAKE_RECORD", &record);
    std::env::remove_var("DM_FAKE_FAIL");

    let url = serve_page().await;
    let with = Options {
        subtitles: true,
        ..options(dir.path())
    };
    download(&client(), &url, &with)
        .await
        .expect("the hand-off failed");
    let args = recorded(&record);
    for wanted in ["--write-subs", "--embed-subs", "--sub-langs"] {
        assert!(
            args.iter().any(|a| a == wanted),
            "{wanted} missing: {args:?}"
        );
    }

    // Without an ffmpeg they are fetched as WebVTT and dm puts them inside
    // the video it joins; no subtitle file is left beside it.
    let dir = tempdir();
    let record = dir.path().join("argv");
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", &record);
    std::env::set_var(
        "DM_FAKE_HALVES",
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4"),
    );
    std::env::set_var("DM_FAKE_SUBS", "1");
    let with = Options {
        subtitles: true,
        ..options(dir.path())
    };
    let done = download(&client(), &url, &with).await;
    std::env::remove_var("DM_FAKE_HALVES");
    std::env::remove_var("DM_FAKE_SUBS");
    let done = done.expect("the hand-off failed");
    let args = recorded(&record);
    assert!(!args.iter().any(|a| a == "--embed-subs"), "{args:?}");
    assert!(args.iter().any(|a| a == "--write-subs"), "{args:?}");
    let joined = std::fs::read(&done.path).unwrap();
    let has = |kind: &[u8]| joined.windows(kind.len()).filter(|w| *w == kind).count();
    assert_eq!(has(b"tx3g"), 1, "no subtitle track");
    assert_eq!(has(b"Hello"), 1, "the cue's text is not in it");
    assert_eq!(
        files_under(&dir.path().join("out")),
        vec![done.path.clone()]
    );

    // And off, nothing about subtitles at all.
    let dir = tempdir();
    let record = dir.path().join("argv");
    let tool = fake_ytdlp(dir.path());
    std::env::set_var("DM_YTDLP", &tool);
    std::env::set_var("DM_FFMPEG", &tool);
    std::env::set_var("DM_FAKE_RECORD", &record);
    download(&client(), &url, &options(dir.path()))
        .await
        .expect("the hand-off failed");
    assert!(!recorded(&record).iter().any(|a| a.contains("subs")));
}

/// A watch page that refuses dm's own request (a 429 bot check) is
/// yt-dlp's to try, and downloads through it; a page yt-dlp makes nothing
/// of keeps the refusal; a file that refuses is not handed over at all.
#[tokio::test]
async fn a_page_refused_to_dm_is_handed_to_ytdlp() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    let record = dir.path().join("argv");
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", &record);
    std::env::remove_var("DM_FAKE_FAIL");
    let refuse = |status: u16| {
        move || async move {
            axum::response::Response::builder()
                .status(status)
                .body(axum::body::Body::from("unusual traffic"))
                .unwrap()
        }
    };
    let app = axum::Router::new()
        .route("/watch", axum::routing::get(refuse(429)))
        .route("/file.zip", axum::routing::get(refuse(403)));
    let base = format!("http://{}", common::serve(app).await);
    let once = Options {
        attempts: 1,
        ..options(dir.path())
    };

    let done = download(&client(), &format!("{base}/watch"), &once)
        .await
        .expect("the refused page, through yt-dlp");
    assert_eq!(std::fs::read(&done.path).unwrap(), b"hello dash");
    assert!(recorded(&record).iter().any(|a| a.ends_with("/watch")));

    std::env::set_var("DM_FAKE_FAIL", "1");
    match download(&client(), &format!("{base}/watch"), &once).await {
        Err(dm_core::Error::Refused { status: 429, .. }) => {}
        other => panic!("expected the 429, got {other:?}"),
    }
    std::env::remove_var("DM_FAKE_FAIL");
    let _ = std::fs::remove_file(&record);
    match download(&client(), &format!("{base}/file.zip"), &once).await {
        Err(dm_core::Error::Refused { status: 403, .. }) => {}
        other => panic!("expected the 403, got {other:?}"),
    }
    assert!(!record.exists(), "a refused file was handed to yt-dlp");
    std::env::remove_var("DM_YTDLP");
    std::env::remove_var("DM_FFMPEG");
}
