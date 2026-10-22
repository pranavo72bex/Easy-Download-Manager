//! A yt-dlp run stood in for by a script: what it started ends with it, and
//! its progress across two streams only goes forward.
//!
//! One test at a time: the stand-in is named by an environment variable,
//! which is process-wide.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dm_core::{ytdlp, Update};

mod common;
use common::{options, tempdir};

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Reports picture then sound, each counting from zero as yt-dlp does, then
/// starts a long-running child, such as a merge, and waits on it.
fn stand_in(dir: &Path) -> PathBuf {
    let script = dir.join("yt-dlp");
    std::fs::write(
        &script,
        r#"#!/bin/sh
PATH=/bin:/usr/bin; export PATH
echo "@dm@1000 2000 NA"
echo "@dm@2000 2000 NA"
echo "@dm@0 NA 500.5"
echo "@dm@500 NA 500.5"
sleep 300 &
echo $! > "$DM_FAKE_OUT/child.pid"
echo "[Merger] Merging formats"
wait
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn alive(pid: &str) -> bool {
    // A process ended but not yet reaped is a zombie, and answers `kill -0`:
    // in a container whose first process reaps no orphans (the Arch
    // package's check in CI) it stays one. Ended is ended.
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next());
        if matches!(state, Some("Z" | "X")) {
            return false;
        }
    }
    std::process::Command::new("/bin/kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Starts the stand-in and waits for its child to be running.
async fn started(
    dir: &Path,
) -> (
    tokio::task::JoinHandle<Result<dm_core::Finished, dm_core::Error>>,
    dm_core::Control,
    tokio::sync::mpsc::Receiver<Update>,
    String,
) {
    std::env::set_var("DM_YTDLP", stand_in(dir));
    std::env::set_var("DM_FAKE_OUT", dir);
    let options = options(dir);
    let control = options.control.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let run = tokio::spawn(async move {
        ytdlp::download_ytdlp("http://127.0.0.1:9/watch", &options, Some(tx)).await
    });
    let pid_file = dir.join("child.pid");
    for _ in 0..100 {
        if let Ok(pid) = std::fs::read_to_string(&pid_file) {
            if !pid.trim().is_empty() {
                return (run, control, rx, pid.trim().to_string());
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the stand-in never started its child");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pause_ends_what_yt_dlp_started_not_only_yt_dlp() {
    let _turn = ONE_AT_A_TIME.lock().await;
    pause_ends_the_tree().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pause_ends_the_tree_under_a_path_with_no_kill_on_it() {
    /* A browser's native host or the Dock starts dm with a minimal PATH.
     * The tree was ended by running `kill`, looked up there; not found,
     * the error went unread and the child ran on. */
    let _turn = ONE_AT_A_TIME.lock().await;
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(path) = self.0.take() {
                std::env::set_var("PATH", path);
            }
        }
    }
    let empty = tempdir();
    let _restore = Restore(std::env::var_os("PATH"));
    std::env::set_var("PATH", empty.path());
    pause_ends_the_tree().await;
}

async fn pause_ends_the_tree() {
    let dir = tempdir();
    let (run, control, _rx, child) = started(dir.path()).await;
    assert!(alive(&child));

    control.pause();
    let ended = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("the pause was not honoured")
        .unwrap();
    assert!(
        matches!(ended, Err(dm_core::Error::Paused)),
        "{:?}",
        ended.err()
    );

    // Reaped by whoever inherits it; a moment for that.
    let mut gone = false;
    for _ in 0..40 {
        if !alive(&child) {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if !gone {
        let _ = std::process::Command::new("/bin/kill").arg(&child).status();
    }
    assert!(gone, "yt-dlp's child {child} ran on after the pause");
}

#[tokio::test(flavor = "multi_thread")]
async fn progress_across_picture_and_sound_only_goes_forward() {
    let _turn = ONE_AT_A_TIME.lock().await;
    let dir = tempdir();
    let (run, control, mut rx, _child) = started(dir.path()).await;
    control.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), run).await;

    let mut seen = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let Update::Progress(p) = update {
            seen.push((p.downloaded, p.total));
        }
    }
    assert_eq!(
        seen,
        [
            (1000, Some(2000)),
            (2000, Some(2000)),
            (2000, Some(2500)),
            (2500, Some(2500)),
        ]
    );
}
