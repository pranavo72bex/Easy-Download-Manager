// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The inbox: a launch's orders handed to the dm already running, and that dm taking what is left
//! there.

use super::*;

/// While the window waits for a fresh link from the browser, every hand-off
/// goes to it rather than straight into the queue.
static TO_WINDOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Sends every hand-off to the window (`on`), or lets the queue take them
/// as usual.
pub fn route_to_window(on: bool) {
    TO_WINDOW.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn routed_to_window() -> bool {
    TO_WINDOW.load(std::sync::atomic::Ordering::Relaxed)
}

/// The file in the inbox that carries `--when-done` to the window.
pub const WHEN_DONE_FILE: &str = dm_core::paths::INBOX_WHEN_DONE;

/// The file in the inbox that carries `--new-download` to the window.
pub const NEW_DOWNLOAD_FILE: &str = dm_core::paths::INBOX_NEW_DOWNLOAD;

/// Gives this launch's orders to the dm already running, and asks it to
/// show itself, rather than opening a second window over a second queue
/// writing the same file.
pub fn hand_to_running(home: &Path, orders: &Orders) {
    for (n, request) in orders.downloads.iter().enumerate() {
        if let Err(e) = host::deposit(home, &format!("argv-{}-{n}", std::process::id()), request) {
            eprintln!("dm: could not hand {} to the running dm: {e}", request.url);
        }
    }
    if orders.start_queue {
        let _ = leave(home, dm_core::paths::INBOX_START_QUEUE, b"");
    }
    leave_when_done(home, orders);
    leave_new_download(home, orders);
    // Last, and rung: the requests filed above are found with it.
    let _ = leave(home, dm_core::paths::INBOX_SHOW, b"");
}

/// Leaves `name` in the running dm's inbox, saying `said`, and rings its bell (`host::ring`):
/// the watcher looks at once, where it would otherwise find it at its next look, seconds away
/// (gaps row S18). Every one of dm's own writers goes through here.
pub fn leave(home: &Path, name: &str, said: &[u8]) -> std::io::Result<()> {
    let dir = host::inbox(home);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(name), said)?;
    host::ring(home);
    Ok(())
}

/// What the system opened with this dm (macOS: a `magnet:` link clicked
/// anywhere, a `.torrent` file opened from Finder), each a link or a file's
/// path, handed to the window through the inbox as a command-line link is.
/// Anything that is neither a link nor a `.torrent` is passed by. Returns
/// how many downloads were handed.
pub fn opened_by_system(home: &Path, opened: &[String]) -> usize {
    let handed = orders(opened);
    if !handed.downloads.is_empty() {
        hand_to_running(home, &handed);
    }
    handed.downloads.len()
}

/// The start of the inbox file that asks the running dm to try a failed
/// download again: its row's id follows.
pub const RETRY_PREFIX: &str = dm_core::paths::INBOX_RETRY_PREFIX;

/// Asks the running dm to try row `id` again, through its inbox: a
/// notification's Retry, whose click is heard on a thread that has no queue
/// to ask (gaps row U15).
pub fn ask_running_to_retry(home: &Path, id: u64) {
    let _ = leave(home, &format!("{RETRY_PREFIX}{id}"), b"");
}

/// The row a file in the inbox asks to be tried again, by its name.
pub fn retry_asked(name: &str) -> Option<u64> {
    name.strip_prefix(RETRY_PREFIX)?.parse().ok()
}

/// `--new-download`, left in the inbox for the window to take up: the one
/// running, or the one this launch starts.
pub fn leave_new_download(home: &Path, orders: &Orders) {
    if orders.new_download {
        let _ = leave(home, NEW_DOWNLOAD_FILE, b"");
    }
}

/// `--when-done`, left in the inbox for the window to take up: the one
/// running, or the one this launch starts.
pub fn leave_when_done(home: &Path, orders: &Orders) {
    if let Some(word) = &orders.when_done {
        let _ = leave(home, WHEN_DONE_FILE, word.as_bytes());
    }
}

/// A launch that finds dm already running: registers this copy with any
/// browser installed since — before handing off, since the hand-off ends
/// this launch, and a person following "open dm once" after installing a
/// browser while dm ran got a dm that never told it — then gives
/// the orders to the running dm. `register` is
/// [`crate::register::ensure_if_claimed`] outside tests. Returns what the
/// registration could not do, for this launch to say.
pub fn hand_off(
    home: &Path,
    orders: &Orders,
    register: impl FnOnce() -> Result<Option<Vec<std::path::PathBuf>>, crate::Error>,
) -> Option<String> {
    let failed = register().err().map(String::from);
    hand_to_running(home, orders);
    failed
}

/// The inbox file that asks a running dm to end.
pub const QUIT: &str = dm_core::paths::INBOX_QUIT;

/// Asks a running dm to end, through its inbox; nothing when none runs, so
/// that a later launch does not read the request and end at once.
pub fn ask_running_to_quit(home: &Path) -> std::io::Result<bool> {
    if !crate::instance::running(home)? {
        return Ok(false);
    }
    leave(home, QUIT, b"")?;
    Ok(true)
}

/// Takes downloads the browser left for us, and a second launch's request
/// to be shown, for as long as the app runs.
///
/// Polled rather than watched: a filesystem notifier is another dependency
/// and another set of platform differences, for a directory that sees a
/// handful of files a day.
pub async fn watch_inbox(
    queue: DownloadList,
    show: impl Fn() + Send + 'static,
    ask: impl Fn(host::Request) + Send + 'static,
    quit: impl Fn() + Send + 'static,
) {
    let dir = host::inbox(&settings::home());
    /* The file calls here are std's, made on the task's own thread: a few
     * tiny ones twice a second. Through tokio's each was a trip to the
     * blocking pool, which handed them round its parked threads in turn so
     * that none was ever idle long enough to end: after ten downloads,
     * seventy threads and their memory stayed for the life of dm.
     * */
    let bell = open_bell(&dir);
    loop {
        // `--unregister`, uninstalling dm: this dm ends, as a stop signal
        // ends it, rather than stay in the tray and write back what the
        // uninstall removed.
        if std::fs::remove_file(dir.join(QUIT)).is_ok() {
            quit();
        }
        // Left by a second launch instead of opening a second window, and
        // by the browser's toolbar button.
        if std::fs::remove_file(dir.join(dm_core::paths::INBOX_SHOW)).is_ok() {
            show();
        }
        // A second launch with `--start-queue`.
        if std::fs::remove_file(dir.join(dm_core::paths::INBOX_START_QUEUE)).is_ok() {
            queue.start_queue(dm_core::MAIN_QUEUE).await;
        }

        let waiting: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        for path in waiting {
            // A notification's Retry (`ask_running_to_retry`), taken once.
            if let Some(id) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(retry_asked)
            {
                if std::fs::remove_file(&path).is_ok() {
                    queue.retry(id).await;
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // Removed before it is acted on, so a request that upsets the
            // app is not retried on every restart for ever. One that
            // cannot be removed is left alone rather than queued again
            // twice a second for the life of the process.
            if let Err(e) = std::fs::remove_file(&path) {
                dm_core::note!(
                    warn,
                    "dm: a hand-off in {} could not be consumed: {e}",
                    path.display()
                );
                continue;
            }

            match serde_json::from_str::<host::Request>(&text) {
                // The browser's newer address for a file whose row waits for
                // one (its address ran out): that row takes it and goes on,
                // and a renewal is nothing more (`take_from_browser`).
                Ok(request) if request.from_browser || request.renews => {
                    let saved = host::saved(&settings::home());
                    let asks = request.ask || routed_to_window();
                    if let Taken::Ask(request) =
                        take_from_browser(&queue, &saved, request, asks).await
                    {
                        ask(*request);
                    }
                }
                // A video from the browser's panel, or any hand-off while the
                // window waits for one: the window asks first.
                Ok(request) if request.ask || routed_to_window() => ask(request),
                // With the page as Referer (`add_options`): without it a
                // hotlink-protected file 403s in the app where the browser's
                // own download would have worked.
                Ok(request) => {
                    let saved = host::saved(&settings::home());
                    let url = saved.with_login(&request.url);
                    let with = saved.with_site_rule(&request.url, add_options(&request));
                    queue.add_with(&url, with).await;
                }
                // The browser was told this started. It is kept, under a
                // name the watcher skips, rather than vanishing.
                Err(e) => {
                    let kept = path.with_extension("unreadable");
                    let _ = std::fs::write(&kept, &text);
                    dm_core::note!(
                        warn,
                        "dm: a hand-off could not be read ({e}); kept as {}",
                        kept.display()
                    );
                }
            }
        }
        /* Spawned, never awaited: `run` returns only once the queue has
         * drained, so awaiting it stopped this loop reading the inbox for as
         * long as anything was downloading — which is exactly when a browser
         * hand-off arrives. Started only when nobody is serving the queue. */
        if !queue.scheduling() {
            wake(&queue);
        }
        wait_for_inbox(bell.as_ref()).await;
    }
}

/// What became of a browser's hand-off ([`take_from_browser`]).
#[derive(Debug, PartialEq)]
pub enum Taken {
    /// A row waiting for a fresh address for the same file took it and goes on.
    Adopted(dm_core::Id),
    /// A renewal no row waits for: nothing is done with it.
    Dropped,
    /// For the window to ask about first.
    Ask(Box<host::Request>),
    /// Added to the queue as a download of its own.
    Added(Option<dm_core::Id>),
}

/// A hand-off from the browser's extension. One the window asks about
/// (`asks`) is asked about first, as contract row B63 has it. Otherwise it
/// is first offered to a row whose signed address ran out and that waits for
/// the browser's next one for the same file (`DownloadList::adopt_fresh_link`), and
/// added where none takes it; a renewal (`Request::renews`) never asks and is
/// nothing more.
pub async fn take_from_browser(
    queue: &DownloadList,
    saved: &Settings,
    request: host::Request,
    asks: bool,
) -> Taken {
    if asks && !request.renews {
        return Taken::Ask(Box::new(request));
    }
    let with = saved.with_site_rule(&request.url, add_options(&request));
    if let Some(id) = queue.adopt_fresh_link(&request.url, &with).await {
        return Taken::Adopted(id);
    }
    if request.renews {
        return Taken::Dropped;
    }
    let url = saved.with_login(&request.url);
    Taken::Added(queue.add_with(&url, with).await)
}

/// How often the inbox is looked at where dm could open no bell.
const INBOX_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// How often it is looked at anyway with a bell: every writer rings it (`leave`, `host::deposit`'s
/// callers), so this is for a file left by hand or by an older dm (gaps row S18: twice a second
/// for the life of the process before).
const INBOX_POLL_RUNG: std::time::Duration = std::time::Duration::from_secs(5);

/// The doorbell a browser hand-off rings (`host::ring`): a loopback socket
/// whose port is written down in the inbox for the host to find. `None`
/// where it cannot be had, and the inbox is polled as before.
pub(super) fn open_bell(dir: &std::path::Path) -> Option<tokio::net::UdpSocket> {
    let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).ok()?;
    socket.set_nonblocking(true).ok()?;
    let port = socket.local_addr().ok()?.port();
    std::fs::create_dir_all(dir).ok()?;
    // Whole, then moved: a host reading a half-written port would ring
    // somebody else's.
    let temp = dir.join(format!(".{}.{}.tmp", host::BELL, std::process::id()));
    std::fs::write(&temp, port.to_string()).ok()?;
    if std::fs::rename(&temp, dir.join(host::BELL)).is_err() {
        let _ = std::fs::remove_file(&temp);
        return None;
    }
    tokio::net::UdpSocket::from_std(socket).ok()
}

/// Until the next look at the inbox: the poll's interval, or sooner when the
/// bell rings. Every datagram already waiting is taken with the first, so a
/// burst of rings is one look; and a look follows the last by a few
/// milliseconds at least, so a program sending datagrams as fast as it can
/// makes dm read a directory a few hundred times a second, no more. The
/// floor comes before the wait, not after a ring: a ring meanwhile waits on
/// the socket, and the first after a quiet spell is answered at once.
pub(super) async fn wait_for_inbox(bell: Option<&tokio::net::UdpSocket>) {
    let Some(bell) = bell else {
        tokio::time::sleep(INBOX_POLL).await;
        return;
    };
    tokio::time::sleep(BELL_FLOOR).await;
    let mut buf = [0u8; 16];
    tokio::select! {
        _ = tokio::time::sleep(INBOX_POLL_RUNG - BELL_FLOOR) => {}
        _ = bell.recv(&mut buf) => {
            while bell.try_recv(&mut buf).is_ok() {}
        }
    }
}

/// The least time between two looks at the inbox when the bell rings.
const BELL_FLOOR: std::time::Duration = std::time::Duration::from_millis(3);
