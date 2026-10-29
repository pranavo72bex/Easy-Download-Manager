//! A yt-dlp that says nothing is ended, not waited on for ever. Its own test
//! binary: it names the program and the limit through the environment.

mod common;
use common::{options, tempdir};

/// A yt-dlp hung before its first word kept the row "running" at nothing for
/// as long as dm ran (BUGLOG.md, BUG-004, found by qa/tests
/// `ytdlp_crash_and_hang`).
#[cfg(unix)]
#[tokio::test]
async fn a_yt_dlp_that_says_nothing_is_ended() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir();
    let fake = dir.path().join("yt-dlp");
    std::fs::write(&fake, "#!/bin/sh\nexec sleep 1000\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("DM_YTDLP", &fake);
    std::env::set_var("DM_YTDLP_QUIET", "2");
    let started = std::time::Instant::now();
    let said = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        dm_core::ytdlp::download_ytdlp("http://127.0.0.1:9/watch?v=x", &options(dir.path()), None),
    )
    .await
    .expect("still waiting on a silent yt-dlp after a minute")
    .unwrap_err()
    .to_string();
    assert!(said.contains("stopped answering"), "{said}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}
