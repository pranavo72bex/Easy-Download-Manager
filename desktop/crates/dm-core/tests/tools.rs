//! The tools dm hands work to: found where people install them, and
//! fetched by dm itself where the computer has none.

mod common;

use dm_core::tools::{fetch_into, Sources, Tool};
use sha2::Digest as _;

/// Issue 10: an installed yt-dlp was reported missing on Windows. Run by
/// CI after it installs yt-dlp as a person would (on Windows, `pip install
/// --user`, whose Scripts folder is not on the PATH a dm started by a
/// browser or the Start menu has); skipped everywhere else.
#[test]
fn an_installed_yt_dlp_is_found() {
    if std::env::var_os("DM_EXPECT_YTDLP").is_none() {
        eprintln!("DM_EXPECT_YTDLP not set — skipping");
        return;
    }
    assert!(
        dm_core::ytdlp::available(),
        "yt-dlp is installed (DM_EXPECT_YTDLP) and dm did not find it; PATH is {:?}",
        std::env::var_os("PATH")
    );
}

/// A server standing in for yt-dlp's and ffmpeg's releases.
async fn releases(ytdlp: Vec<u8>, sums: String, ffmpeg_gz: Vec<u8>) -> String {
    use axum::routing::get;
    let app = axum::Router::new()
        .route("/y/SHA2-256SUMS", get(move || async move { sums }))
        .route("/y/yt-dlp_linux", get(move || async move { ytdlp }))
        .route(
            "/f/ffmpeg-linux-x64.gz",
            get(move || async move { ffmpeg_gz }),
        );
    format!("http://{}", common::serve(app).await)
}

fn gz(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    out.write_all(bytes).unwrap();
    out.finish().unwrap()
}

/// What dm fetches is kept only when it matches its published checksum
/// (yt-dlp) or the digest written into dm (ffmpeg), and is placed whole and
/// runnable; a copy that does not match is refused and nothing is left.
#[tokio::test]
async fn fetched_tools_are_kept_only_when_they_match() {
    let program = b"#!/bin/sh\necho fetched\n".to_vec();
    let digest = hex::encode(sha2::Sha256::digest(&program));
    let packed = gz(b"an ffmpeg");
    let ffmpeg_digest = hex::encode(sha2::Sha256::digest(&packed));
    let base = releases(
        program.clone(),
        format!("{digest}  yt-dlp_linux\n0000000000000000000000000000000000000000000000000000000000000000  yt-dlp.exe\n"),
        packed,
    )
    .await;
    let sources = Sources {
        ytdlp: format!("{base}/y"),
        ffmpeg: format!("{base}/f"),
        ffmpeg_digests: vec![("linux-x64".into(), ffmpeg_digest)],
    };
    let dir = common::tempdir();
    let client = dm_core::client();

    let ytdlp = fetch_into(
        &client,
        Tool::YtDlp,
        dir.path(),
        &sources,
        "linux",
        "x86_64",
    )
    .await
    .expect("the matching yt-dlp");
    assert_eq!(std::fs::read(&ytdlp).unwrap(), program);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&ytdlp).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }
    let ffmpeg = fetch_into(
        &client,
        Tool::Ffmpeg,
        dir.path(),
        &sources,
        "linux",
        "x86_64",
    )
    .await
    .expect("the matching ffmpeg");
    assert_eq!(std::fs::read(&ffmpeg).unwrap(), b"an ffmpeg");

    // A digest that does not match: refused, and nothing written.
    let other = common::tempdir();
    let lying = Sources {
        ffmpeg_digests: vec![("linux-x64".into(), "ab".repeat(32))],
        ..sources.clone()
    };
    let said = fetch_into(
        &client,
        Tool::Ffmpeg,
        other.path(),
        &lying,
        "linux",
        "x86_64",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("does not match"), "{said}");
    assert!(std::fs::read_dir(other.path()).unwrap().next().is_none());
    // A build the checksums do not list.
    let said = fetch_into(
        &client,
        Tool::YtDlp,
        other.path(),
        &sources,
        "freebsd",
        "x86_64",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("no build"), "{said}");
}

/// The real thing, from GitHub: yt-dlp's and ffmpeg's builds for this
/// computer, fetched, checked and run. Network, so only where asked
/// (DM_NETWORK_TESTS=1).
#[tokio::test]
async fn the_official_builds_are_fetched_and_run() {
    if std::env::var_os("DM_NETWORK_TESTS").is_none() {
        eprintln!("DM_NETWORK_TESTS not set — skipping");
        return;
    }
    let dir = common::tempdir();
    let client = dm_core::client();
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    for tool in [Tool::YtDlp, Tool::Ffmpeg] {
        let path = fetch_into(
            &client,
            tool,
            dir.path(),
            &dm_core::tools::official(),
            os,
            arch,
        )
        .await
        .unwrap_or_else(|e| panic!("{tool:?}: {e}"));
        let flag = if tool == Tool::YtDlp {
            "--version"
        } else {
            "-version"
        };
        let ran = std::process::Command::new(&path)
            .arg(flag)
            .output()
            .unwrap();
        assert!(
            ran.status.success(),
            "{tool:?} {flag}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        println!(
            "{tool:?}: {}",
            String::from_utf8_lossy(&ran.stdout)
                .lines()
                .next()
                .unwrap_or("")
        );
    }
}
