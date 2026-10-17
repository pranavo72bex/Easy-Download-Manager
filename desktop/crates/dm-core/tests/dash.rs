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
printf 'hello dash' > "$out"
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
        said.contains("install yt-dlp"),
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

    let url = serve_manifest().await;
    let finished = download(
        &client(),
        &url,
        &Options {
            proxy: Some("socks5://127.0.0.1:1080".into()),
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
        !argv.iter().any(|a| a.contains("secret")),
        "the proxy's password was on yt-dlp's command line: {argv:?}"
    );
    let config = std::fs::read_to_string(record.with_extension("config")).unwrap_or_default();
    assert_eq!(config.trim(), "--proxy 'socks5://127.0.0.1:1080'");
    assert!(
        !Path::new(after("--config-locations").unwrap()).exists(),
        "the proxy's file outlived the run"
    );
    assert_eq!(after("--referer"), Some("https://page.example/watch"));
    // No ffmpeg, so one stream that carries both — never a pair.
    assert_eq!(after("-f"), Some("b"));
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
    assert!(said.contains("install yt-dlp"), "{said}");
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
async fn without_ffmpeg_a_stream_in_two_halves_says_to_install_it() {
    let _turn = ENV.lock().await;
    let dir = tempdir();
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", dir.path().join("argv"));
    std::env::set_var("DM_FAKE_FAIL", "1");

    let url = serve_manifest().await;
    let err = download(&client(), &url, &options(dir.path()))
        .await
        .expect_err("nothing was produced, yet it succeeded");
    std::env::remove_var("DM_FAKE_FAIL");

    // yt-dlp's own words name neither the cause nor the fix.
    let said = err.to_string();
    assert!(said.contains("install ffmpeg"), "{said}");
    assert!(!said.contains("Requested format"), "{said}");
}

#[tokio::test]
async fn subtitles_are_fetched_and_embedded_only_where_an_ffmpeg_can_embed_them() {
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

    // Without an ffmpeg nothing can embed them, so they are not asked for:
    // files beside the video are files dm would not place.
    let dir = tempdir();
    let record = dir.path().join("argv");
    std::env::set_var("DM_YTDLP", fake_ytdlp(dir.path()));
    std::env::set_var("DM_FFMPEG", "");
    std::env::set_var("DM_FAKE_RECORD", &record);
    let with = Options {
        subtitles: true,
        ..options(dir.path())
    };
    download(&client(), &url, &with)
        .await
        .expect("the hand-off failed");
    let args = recorded(&record);
    assert!(!args.iter().any(|a| a == "--embed-subs"), "{args:?}");
    assert!(!args.iter().any(|a| a == "--write-subs"), "{args:?}");

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
