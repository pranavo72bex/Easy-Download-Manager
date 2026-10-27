//! yt-dlp's process trees: each run is ended with everything under it.

#[cfg(windows)]
use std::process::Stdio;

/// yt-dlp and everything it started, ended together.
/// On unix yt-dlp leads a process group of its own and the group is sent
/// SIGKILL by `killpg`; on Windows `taskkill /T` ends the tree under it.
/// `None` once the run has been reaped, when the number may belong to
/// someone else.
/// No `kill` program on unix: looked up on `PATH`, it was not found under
/// the minimal one a browser's native host or the Dock starts dm with, the
/// error went unread, and the worker under yt-dlp ran on. On unix nothing
/// here waits. On Windows `taskkill` is waited for, at most
/// [`TREE_KILL_WAIT`], before yt-dlp itself is killed: the one-file yt-dlp
/// is a launcher with the real yt-dlp as its child, and `/T` finds that
/// child through the launcher, so a launcher killed first (dm used to start
/// taskkill and kill yt-dlp at once) left the worker running.
/// The async path waits off the runtime; `Drop`, the rare path of
/// a run abandoned mid-way, waits where it is.
pub(super) struct Tree(Option<u32>);

/// How long `taskkill` is given to end a tree.
#[cfg_attr(not(windows), allow(dead_code))]
const TREE_KILL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Every yt-dlp tree still running, for `end_all`.
static LIVE: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// Ends every yt-dlp tree still running. For a dm leaving at once (a second
/// interrupt) without the unwinding that would end each on its own: yt-dlp
/// runs in a process group of its own, which a terminal's Ctrl+C no longer
/// reaches, so nothing else would stop it. Each tree is
/// waited for (on Windows, briefly) before dm exits and takes yt-dlp's
/// launcher down with it.
pub fn end_all() {
    let pids = std::mem::take(&mut *crate::lock(&LIVE));
    for pid in pids {
        end_tree(pid);
    }
}

/// Ends the tree under `pid`, and on Windows returns once `taskkill` has.
pub(super) fn end_tree(pid: u32) {
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(pid) {
        // SAFETY: `killpg` takes two integers and touches no memory of
        // ours. The group is yt-dlp's own (`process_group(0)` at spawn)
        // and is not yet reaped, so the number is still its.
        let _ = unsafe { libc::killpg(group, libc::SIGKILL) };
    }
    #[cfg(windows)]
    if let Ok(mut taskkill) = crate::no_window(&mut std::process::Command::new("taskkill"))
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        wait_at_most(&mut taskkill, TREE_KILL_WAIT);
    }
}

/// Waits for `child` up to `limit`, then kills it rather than wait on.
/// Whether it ended on its own in time.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn wait_at_most(child: &mut std::process::Child, limit: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if start.elapsed() < limit => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

impl Tree {
    pub(super) fn of(pid: Option<u32>) -> Self {
        if let Some(pid) = pid {
            crate::lock(&LIVE).push(pid);
        }
        Tree(pid)
    }

    /// yt-dlp has been reaped: its number may be another process's now.
    pub(super) fn disarm(&mut self) {
        let _ = self.take();
    }

    /// The number to end, handed over once and forgotten here.
    pub(super) fn take(&mut self) -> Option<u32> {
        let pid = self.0.take()?;
        crate::lock(&LIVE).retain(|p| *p != pid);
        Some(pid)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Some(pid) = self.take() {
            end_tree(pid);
        }
    }
}
