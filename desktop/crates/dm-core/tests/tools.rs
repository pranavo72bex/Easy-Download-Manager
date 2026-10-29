//! The tools dm hands work to: found where people install them, and
//! fetched by dm itself where the computer has none.

mod common;

use dm_core::tools::{fetch_into, Sources, Tool};
use futures_util::StreamExt as _;
use sha2::Digest as _;

/// An installed yt-dlp was reported missing on Windows. Run by
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

/// yt-dlp's unpacked build as its release gives it: the program beside an
/// `_internal` folder, in a zip.
fn build_zip(program: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let file = |mode| zip::write::SimpleFileOptions::default().unix_permissions(mode);
    zip.start_file("yt-dlp_linux", file(0o755)).unwrap();
    zip.write_all(program).unwrap();
    zip.start_file("_internal/lib.so", file(0o644)).unwrap();
    zip.write_all(b"library").unwrap();
    zip.finish().unwrap();
    out.into_inner()
}

/// A server standing in for yt-dlp's and QuickJS's releases: yt-dlp's Linux
/// and Windows builds, the same bytes, the checksums given, and one QuickJS
/// build.
async fn releases(ytdlp: Vec<u8>, sums: String) -> String {
    use axum::routing::get;
    let exe = ytdlp.clone();
    let app = axum::Router::new()
        .route("/y/SHA2-256SUMS", get(move || async move { sums }))
        .route("/y/yt-dlp_linux.zip", get(move || async move { ytdlp }))
        .route("/y/yt-dlp_win.zip", get(move || async move { exe }))
        .route("/q/qjs-linux-x86_64", get(|| async { &b"a quickjs"[..] }));
    format!("http://{}", common::serve(app).await)
}

/// What dm fetches is kept only when it matches its published checksum, and
/// is placed whole and runnable; a copy that does not match is refused and
/// nothing is left.
#[tokio::test]
async fn fetched_tools_are_kept_only_when_they_match() {
    let program = b"#!/bin/sh\necho fetched\n".to_vec();
    let zip = build_zip(&program);
    let digest = hex::encode(sha2::Sha256::digest(&zip));
    let base = releases(
        zip,
        format!("{digest}  yt-dlp_linux.zip\n0000000000000000000000000000000000000000000000000000000000000000  yt-dlp_win.zip\n"),
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
    // The single file an earlier dm fetched: gone once the unpacked build is.
    // Named as dm names it on this system (`.exe` on Windows).
    let one_file = dir.path().join(if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    });
    std::fs::write(&one_file, b"old one-file build").unwrap();

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
    assert!(ytdlp.ends_with("yt-dlp-linux/yt-dlp_linux"), "{ytdlp:?}");
    assert_eq!(
        std::fs::read(ytdlp.parent().unwrap().join("_internal/lib.so")).unwrap(),
        b"library"
    );
    assert!(!one_file.exists(), "the old file stayed");
    assert!(
        !ytdlp.parent().unwrap().with_extension("partial").exists(),
        "a staging folder was left"
    );
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
    // Said as "no build" where the computer has no Python either, and as
    // "no portable build" where it has one and the release lists none.
    assert!(
        said.contains("no build") || said.contains("no portable build"),
        "{said}"
    );
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

/// While a tool is fetched, the bytes so far and the length are told, for
/// the row's status to say (docs/gaps.md row 35).
#[tokio::test(flavor = "multi_thread")]
async fn the_bytes_of_a_fetch_are_told_while_it_runs() {
    use axum::routing::get;
    let program = b"#!/bin/sh\necho slow\n".to_vec();
    let zip = build_zip(&program);
    let digest = hex::encode(sha2::Sha256::digest(&zip));
    let sums = format!("{digest}  yt-dlp_linux.zip\n");
    let served = zip.clone();
    let app = axum::Router::new()
        .route("/y/SHA2-256SUMS", get(move || async move { sums }))
        .route(
            "/y/yt-dlp_linux.zip",
            get(move || async move {
                // In four pieces, a moment apart, as a large file comes.
                let pieces: Vec<Vec<u8>> = served
                    .chunks(served.len() / 4 + 1)
                    .map(<[u8]>::to_vec)
                    .collect();
                let stream = futures_util::stream::iter(pieces).then(|piece| async move {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    Ok::<_, std::io::Error>(bytes::Bytes::from(piece))
                });
                axum::response::Response::builder()
                    .header("Content-Length", served.len())
                    .body(axum::body::Body::from_stream(stream))
                    .unwrap()
            }),
        );
    let base = format!("http://{}", common::serve(app).await);
    let sources = Sources {
        ytdlp: format!("{base}/y"),
        quickjs: format!("{base}/q"),
        quickjs_digests: vec![],
    };
    let dir = common::tempdir();
    let folder = dir.path().to_path_buf();
    let total = zip.len() as u64;
    let fetching = tokio::spawn(async move {
        fetch_into(
            &dm_core::client(),
            Tool::YtDlp,
            &folder,
            &sources,
            "linux",
            "x86_64",
        )
        .await
    });
    let mut seen = None;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Some((got, said_total)) = dm_core::tools::preparing_progress() {
            if got > 0 {
                seen = Some((got, said_total));
                break;
            }
        }
    }
    fetching.await.unwrap().expect("the fetch");
    let (got, said_total) = seen.expect("the bytes so far were never told");
    assert!(got > 0 && got <= total, "{got} of {total}");
    assert_eq!(said_total, Some(total));
    assert_eq!(
        dm_core::tools::preparing_progress(),
        None,
        "told as fetching after the fetch"
    );
}
