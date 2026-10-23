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

/// A server standing in for yt-dlp's and QuickJS's releases: yt-dlp's Linux
/// and Windows builds, the same bytes, the checksums given, and one QuickJS
/// build.
async fn releases(ytdlp: Vec<u8>, sums: String) -> String {
    use axum::routing::get;
    let exe = ytdlp.clone();
    let app = axum::Router::new()
        .route("/y/SHA2-256SUMS", get(move || async move { sums }))
        .route("/y/yt-dlp_linux", get(move || async move { ytdlp }))
        .route("/y/yt-dlp.exe", get(move || async move { exe }))
        .route("/q/qjs-linux-x86_64", get(|| async { &b"a quickjs"[..] }));
    format!("http://{}", common::serve(app).await)
}

/// What dm fetches is kept only when it matches its published checksum, and
/// is placed whole and runnable; a copy that does not match is refused and
/// nothing is left.
#[tokio::test]
async fn fetched_tools_are_kept_only_when_they_match() {
    let program = b"#!/bin/sh\necho fetched\n".to_vec();
    let digest = hex::encode(sha2::Sha256::digest(&program));
    let base = releases(
        program.clone(),
        format!("{digest}  yt-dlp_linux\n0000000000000000000000000000000000000000000000000000000000000000  yt-dlp.exe\n"),
    )
    .await;
    let sources = Sources {
        ytdlp: format!("{base}/y"),
        quickjs: format!("{base}/q"),
        quickjs_digests: vec![(
            "qjs-linux-x86_64".into(),
            hex::encode(sha2::Sha256::digest(b"a quickjs")),
        )],
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
    // A copy that does not match its checksum (the Windows build, listed
    // with another's): refused, and nothing written.
    let other = common::tempdir();
    let said = fetch_into(
        &client,
        Tool::YtDlp,
        other.path(),
        &sources,
        "windows",
        "x86_64",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("does not match"), "{said}");
    assert!(std::fs::read_dir(other.path()).unwrap().next().is_none());
    // QuickJS, kept when it is the build dm knows, refused when not.
    let quickjs = fetch_into(
        &client,
        Tool::QuickJs,
        dir.path(),
        &sources,
        "linux",
        "x86_64",
    )
    .await
    .expect("the matching QuickJS");
    assert_eq!(std::fs::read(&quickjs).unwrap(), b"a quickjs");
    let lying = Sources {
        quickjs_digests: vec![("qjs-linux-x86_64".into(), "ab".repeat(32))],
        ..sources.clone()
    };
    let elsewhere = common::tempdir();
    let said = fetch_into(
        &client,
        Tool::QuickJs,
        elsewhere.path(),
        &lying,
        "linux",
        "x86_64",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(said.contains("does not match"), "{said}");
    assert!(std::fs::read_dir(elsewhere.path())
        .unwrap()
        .next()
        .is_none());

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

/// The real thing, from GitHub: yt-dlp's and QuickJS's builds for this
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
    for tool in [Tool::YtDlp, Tool::QuickJs] {
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
        let flag = "--version";
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
