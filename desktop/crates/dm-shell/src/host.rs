// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Taking a download from the browser.
//!
//! The browser launches this as a native messaging host: a short-lived
//! process it owns and kills when the port closes. So it holds nothing worth
//! losing — it writes the request into a directory the running app watches,
//! says it did, and exits.
//!
//! Deliberately one-way. The browser extension does not draw progress bars;
//! it catches the click and the application shows what happened. Relaying
//! progress back through a process the browser can kill at any moment buys a
//! second copy of a queue that is already on screen.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dm_protocol::{
    Batch, CatchRules, Choice, FormatsAsked, Hello, Reply, Request as Asked, SettingsReply, Start,
};

/// A request the app will pick up.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, PartialEq)]
pub struct Request {
    /// The address to download.
    pub url: String,
    /// The page it came from. Kept because a media URL alone is often not
    /// something anything can fetch twice.
    #[serde(default)]
    pub referrer: String,
    /// `--save-to`: the folder to save in, from the command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<String>,
    /// `--name`: the name to save as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_name: Option<String>,
    /// `--later`: into the main queue, not started.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub later: bool,
    /// The quality picked in the browser's video panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    /// A separate sound track for `url`'s picture, fetched with it and joined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    /// What the panel's list called that quality.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Open "Download details" for it rather than starting it, as with a
    /// video from the browser's panel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ask: bool,
    /// The part of a video the panel's "Part" line asked for: its start and
    /// end as typed, either empty. "Download details" opens with them filled.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    /// Where that part ends, as typed; see `from`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    /// Caught by the browser extension, rather than asked for from its
    /// panel or menu: which of Options' two "ask" settings governs it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub caught: bool,
    /// A page's links, for the app's "Download all links" dialog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    /// The form the download is the answer to, where the page sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<dm_protocol::Posted>,
    /// Each of those links' words on the page, by address, where it has any.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub texts: std::collections::BTreeMap<String, String>,
    /// The file names those links' `download` attributes give, by address.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub names: std::collections::BTreeMap<String, String>,
    /// The browser's cookies for the site, as a `Cookie` header, where the
    /// person switched that on for the site in the toolbar popup.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
    /// Those cookies are a video page's session, for this download alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cookies_once: bool,
    /// The request headers the browser sent for it (`Start::headers`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    /// The title of the page it came from, to name a file whose own name says nothing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// The browser it came from ("Microsoft Edge"), as the host's parent
    /// process is named, for the dialog to say; `None` where that cannot
    /// be told.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    /// Handed over by the browser's extension, not given on a command line:
    /// it may take the place of a row waiting for a fresh address for the
    /// same file (`Queue::adopt_fresh_link`) rather than be added again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_browser: bool,
    /// Only a newer address for a row waiting for one (`Start::renews`):
    /// dropped where no row takes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub renews: bool,
}

mod parent;
pub mod served;
/// The browser this host was started by, by its parent process's program
/// name (past the `cmd` Chrome and Edge start it through on Windows), in the
/// words a person knows it by. `None` where it cannot be told.
pub use parent::parent_browser;

/// Frames one message the way native messaging wants it.
pub fn frame(value: &serde_json::Value) -> Vec<u8> {
    let body = value.to_string().into_bytes();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&body);
    out
}

/// Reads one message, or `None` when the browser has gone.
///
/// A length the browser would never send means the stream has lost its place;
/// reading it would allocate until the process died, so it stops instead.
///
/// A whole frame that is not JSON is `Null`, not the end: the frame was read
/// in full, so the stream has not lost its place, and ending there left the
/// browser's every later message on that port unanswered.
pub fn read_frame(input: &mut impl Read) -> Option<serde_json::Value> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header).ok()?;
    let len = u32::from_le_bytes(header) as usize;
    if len > 64 * 1024 * 1024 {
        return None;
    }

    let mut body = vec![0u8; len];
    input.read_exact(&mut body).ok()?;
    Some(serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null))
}

/// Where the app looks for work handed over by a browser.
pub fn inbox(home: &Path) -> PathBuf {
    home.join("inbox")
}

/// The file in the inbox where a running dm writes the loopback port its
/// doorbell listens on. No extension, so the watcher never reads it as a
/// request.
pub const BELL: &str = "bell";

/// Tells a running dm there is something in its inbox, now.
///
/// The inbox is polled every half second, which every browser hand-off to a
/// running dm waited out: a quarter of a second on average between the click
/// and the first byte, half of one at worst. One empty datagram to the port
/// dm wrote down wakes its watcher at once. It carries nothing and asks for
/// nothing: a datagram from anyone else only makes dm look at its inbox
/// early, which it does twice a second anyway. Best effort: a dm that wrote
/// no bell, or has gone, is found by the poll as before.
pub fn ring(home: &Path) {
    let Some(port) = std::fs::read_to_string(inbox(home).join(BELL))
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|&port| port != 0)
    else {
        return;
    };
    if let Ok(socket) = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)) {
        let _ = socket.send_to(&[], (std::net::Ipv4Addr::LOCALHOST, port));
    }
}

/// Writes a request where the app will find it.
///
/// Whole then moved, so the app cannot read a half-written file — it polls a
/// directory, and a partial JSON there would be a download silently dropped.
///
/// The id comes from the browser and is used as a filename, so it is held to
/// being a plain name: `../../../.bashrc` is a perfectly well-formed id, and
/// anything that writes where it is told by a message is a program that
/// writes wherever the sender likes.
pub fn deposit(home: &Path, id: &str, request: &Request) -> std::io::Result<Filed> {
    let named: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(64)
        .collect();
    if named.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "that is not a name a request can be filed under",
        ));
    }

    file_new(
        &inbox(home),
        &named,
        serde_json::to_string(request)?.as_bytes(),
    )
}

/// A request in the inbox: where, and whether this hand-off put it there
/// or found the very same one already waiting.
#[derive(Debug, PartialEq)]
pub struct Filed {
    /// The request's file in the inbox.
    pub path: PathBuf,
    /// Whether this hand-off wrote it; `false` when the same request was
    /// already waiting there.
    pub new: bool,
}

/// Writes `bytes` under `<name>.json` in `dir`, or `<name>-2.json` and on
/// when that is taken: never over a request already filed.
///
/// Two ids can come to the same name once held to a plain one (`a.1` and
/// `a1`, or any two that share their first 64 characters), and the file
/// was simply replaced: the first download gone, and the browser told both
/// were taken. Written whole beside the name, then linked to it, which
/// fails rather than replaces when the name is in use.
fn file_new(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<Filed> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    std::fs::create_dir_all(dir)?;
    // Not `.json`, so the app passes it by while it is being written.
    let temp = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let filed = (|| {
        let mut open = std::fs::OpenOptions::new();
        open.write(true).create_new(true);
        // Readable by this account alone: a hand-off can carry a site's
        // sign-in.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
        let mut file = open.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        for n in 1..=1000u32 {
            let path = if n == 1 {
                dir.join(format!("{name}.json"))
            } else {
                dir.join(format!("{name}-{n}.json"))
            };
            match std::fs::hard_link(&temp, &path) {
                Ok(()) => return Ok(Filed { path, new: true }),
                // The very same request again — a hand-off resent before
                // the app read the first — is the one already filed, not a
                // second download of the same thing.
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists
                        && std::fs::read(&path).is_ok_and(|filed| filed == bytes) =>
                {
                    return Ok(Filed { path, new: false })
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                // A file system without hard links: the name is checked and
                // then taken, which only another hand-off in the same
                // instant could come between.
                Err(_) if !path.exists() => {
                    return std::fs::rename(&temp, &path).map(|()| Filed { path, new: true })
                }
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "the app has a thousand requests under that name still to read",
        ))
    })();
    let _ = std::fs::remove_file(&temp);
    filed
}

/// Starts the normal desktop process so an app that is closed can consume
/// the inbox. Native messaging launches this binary in host mode, which must
/// not open a window or write anything except framed replies to stdout.
///
/// A failure comes back: the browser is about to be told the download has
/// started, and with no app to consume the inbox it has not.
fn launch_desktop() -> std::io::Result<()> {
    keep_pipes_from_children();
    let executable = std::env::current_exe()?;
    let start = |breakaway: bool| {
        let mut command = Command::new(&executable);
        command
            .arg("--desktop")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        /* Firefox on Windows runs its native hosts in a job it ends, with
         * everything in it, when the host exits: the dm started here would
         * go with it, and the download just taken from the browser would
         * wait for someone to open dm. Started outside the job where the job
         * allows it; where it does not, as before. */
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
            command.creation_flags(if breakaway {
                DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB
            } else {
                DETACHED_PROCESS
            });
        }
        let _ = breakaway;
        command.spawn().map(|_| ())
    };
    start(true).or_else(|_| start(false))
}

/// Marks this process's standard handles as not for its children.
///
/// On Windows a child gets every handle marked inheritable, not only the
/// three it is given: the dm started here held the browser's pipes to this
/// host, so the browser (or a script) reading the host's answer waited for
/// that dm to exit before it saw the answer end (found running app-check
/// on Windows). Its own are null, and the host's own writes are unchanged.
#[cfg(windows)]
fn keep_pipes_from_children() {
    use windows_sys::Win32::Foundation::{
        SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: two Win32 calls on this process's own standard handles,
        // which it keeps; a failure leaves the handle as it was.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Elsewhere a child gets only the descriptors it is given.
#[cfg(not(windows))]
fn keep_pipes_from_children() {}

/// The catch rules in the app's settings file, read without touching it.
///
/// Not `Settings::load`, which moves an unreadable file aside: this process
/// is the browser's, starts on every download, and has no window to say it
/// did so. An unreadable or missing file means the defaults, which is what
/// the app itself would be running with.
pub fn catch_rules(home: &Path) -> CatchRules {
    let mut rules = saved(home).catch;
    // The app brings a saved list up to date as it starts; the browser asks
    // for it whether or not the app has.
    rules.bring_up_to_date();
    rules
}

pub(crate) fn saved(home: &Path) -> crate::Settings {
    // Read as the window reads it, so the two never disagree (an older
    // edition of the pop-up defaults, one odd field), and read only: this
    // process has no window to say it moved a file aside.
    crate::Settings::read_only(&home.join("settings.json"))
}

/// What answering needs from outside this process. A trait so a test can
/// answer without starting a second copy of whatever runs it, or a network.
pub trait World {
    /// Whether a dm is already there to take a download.
    fn app_running(&mut self) -> bool;
    /// Starts one.
    fn launch(&mut self) -> std::io::Result<()>;
    /// Wakes a running dm's inbox watcher, so a hand-off filed for it is
    /// read now rather than at its next poll. Nothing by default.
    fn ring(&mut self) {}
    /// Whether dm, from outside the browser, can fetch `url` at all, asked
    /// for no longer than `within_ms`; when it can, the type the server
    /// said it was sending, after redirects (empty where it said none).
    fn reachable(
        &mut self,
        url: &str,
        referrer: &str,
        cookies: &str,
        within_ms: u64,
    ) -> Result<String, String>;
    /// [`World::reachable`] with the request headers the browser sent. A
    /// world that does not use them asks without.
    fn reachable_with(
        &mut self,
        url: &str,
        referrer: &str,
        cookies: &str,
        _headers: &[(String, String)],
        within_ms: u64,
    ) -> Result<String, String> {
        self.reachable(url, referrer, cookies, within_ms)
    }
    /// The qualities a page's video comes in.
    fn formats(&mut self, url: &str, referrer: &str, cookies: &str) -> Result<Vec<Choice>, String>;
    /// Milliseconds since the Unix epoch, the clock the extension's
    /// deadline is read against.
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// The real one: the instance lock, this binary, and dm's own client.
struct Here<'a> {
    home: &'a Path,
    started: bool,
}

impl World for Here<'_> {
    /// A lock that cannot be read is not a running dm: `launch` then says
    /// why, and the browser keeps its download.
    fn app_running(&mut self) -> bool {
        self.started || crate::instance::running(self.home).unwrap_or(false)
    }

    fn launch(&mut self) -> std::io::Result<()> {
        // A dm started now would find the same lock it cannot take, and
        // stop; the browser must not be told it has the download.
        crate::instance::running(self.home)?;
        launch_desktop()?;
        self.started = true;
        Ok(())
    }

    fn ring(&mut self) {
        ring(self.home);
    }

    /// One ranged request, made the way the queue will make it: through the
    /// saved proxy, with the page as referrer. A magnet has no server to ask.
    fn reachable(
        &mut self,
        url: &str,
        referrer: &str,
        cookies: &str,
        within_ms: u64,
    ) -> Result<String, String> {
        self.reachable_with(url, referrer, cookies, &[], within_ms)
    }

    fn reachable_with(
        &mut self,
        url: &str,
        referrer: &str,
        cookies: &str,
        headers: &[(String, String)],
        within_ms: u64,
    ) -> Result<String, String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Ok(String::new());
        }
        let settings = saved(self.home);
        // The same cookies the app sends, so a download they make possible
        // is taken rather than left with the browser.
        let _ = dm_core::cookies::use_file(std::path::Path::new(settings.cookies_file.trim()));
        // Never around the proxy: a proxy that cannot be built here must
        // not turn into a direct request, which is the server seeing the
        // address the person set a proxy to hide. The browser keeps it.
        let client = dm_core::client_with(&settings.proxy).map_err(|e| {
            format!("dm's proxy setting cannot be used ({e}), so it did not ask the server")
        })?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let referrer = (!referrer.is_empty()).then_some(referrer);
        runtime.block_on(async {
            match tokio::time::timeout(
                std::time::Duration::from_millis(within_ms),
                dm_core::probe(
                    &client,
                    // A site's saved sign-in (Options › Sign-ins), as the
                    // app will use it: asked without it, such a link was
                    // refused and always left with the browser.
                    &settings.with_login(url),
                    referrer,
                    dm_core::cookies::SignIn::with_headers(url, cookies, headers).as_ref(),
                ),
            )
            .await
            {
                Ok(Ok(probe)) => Ok(probe.content_type.unwrap_or_default()),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err(format!(
                    "the server did not answer dm within {} seconds",
                    within_ms.div_ceil(1000)
                )),
            }
        })
    }

    /// The qualities a page offers: YouTube from its own player data.
    fn formats(&mut self, url: &str, referrer: &str, cookies: &str) -> Result<Vec<Choice>, String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("only a web page has qualities to list".into());
        }
        // What was read a few minutes ago is not read again.
        let key = formats_key(url, cookies);
        if let Some(listed) = recalled_formats(self.home, &key, now_secs()) {
            return Ok(listed);
        }
        let proxy = saved(self.home).proxy.url_for_tools();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let referrer = (!referrer.is_empty()).then_some(referrer);
        let sign_in = dm_core::cookies::SignIn::new(url, cookies);
        let fast = runtime.block_on(dm_core::videopage::list_choices_fast(
            url,
            proxy.as_deref(),
            sign_in.as_ref(),
        ));
        let listed = if let Some(listed) = fast {
            listed
        } else {
            runtime
                .block_on(dm_core::videopage::list_choices(
                    url,
                    referrer,
                    proxy.as_deref(),
                    sign_in.as_ref(),
                ))
                .map_err(|e| e.to_string())?
        };
        let listed: Vec<Choice> = listed
            .into_iter()
            .map(|c| Choice {
                label: c.label,
                quality: c.quality,
                size: c.size,
            })
            .collect();
        remember_formats(self.home, &key, &listed, now_secs());
        Ok(listed)
    }
}

/// How long a read of a page's qualities is kept, in seconds.
const FORMATS_KEPT_SECS: u64 = 600;
/// How many pages' readings are kept.
const FORMATS_KEPT_PAGES: usize = 48;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A page and the kind of session it was read with, as one name: signed in
/// or not, and with which cookies' names. Their values are left out: a
/// site restamps some on every request, and a name that changed with each
/// would never be found again. Never kept as more than a fingerprint.
fn formats_key(url: &str, cookies: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut names: Vec<&str> = cookies
        .split(';')
        .filter_map(|c| c.split_once('=').map(|(n, _)| n.trim()))
        .filter(|n| !n.is_empty())
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    names.hash(&mut hasher);
    let page = url.split('#').next().unwrap_or(url);
    format!("{page}\n{:016x}", hasher.finish())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Remembered {
    key: String,
    at: u64,
    choices: Vec<Choice>,
}

fn formats_file(home: &Path) -> std::path::PathBuf {
    home.join("formats.cache.json")
}

fn recalled_formats(home: &Path, key: &str, now: u64) -> Option<Vec<Choice>> {
    let text = std::fs::read_to_string(formats_file(home)).ok()?;
    let all: Vec<Remembered> = serde_json::from_str(&text).ok()?;
    all.into_iter()
        .find(|r| r.key == key && now.saturating_sub(r.at) < FORMATS_KEPT_SECS)
        .map(|r| r.choices)
        .filter(|choices| !choices.is_empty())
}

/// Best effort: a home that cannot be written to only means the next read
/// is made again.
fn remember_formats(home: &Path, key: &str, choices: &[Choice], now: u64) {
    if choices.is_empty() {
        return;
    }
    let mut all: Vec<Remembered> = std::fs::read_to_string(formats_file(home))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    all.retain(|r| r.key != key && now.saturating_sub(r.at) < FORMATS_KEPT_SECS);
    all.push(Remembered {
        key: key.to_string(),
        at: now,
        choices: choices.to_vec(),
    });
    let excess = all.len().saturating_sub(FORMATS_KEPT_PAGES);
    all.drain(..excess);
    let Ok(text) = serde_json::to_string(&all) else {
        return;
    };
    let _ = std::fs::create_dir_all(home);
    let file = formats_file(home);
    let staged = file.with_extension("tmp");
    if std::fs::write(&staged, text).is_ok() {
        let _ = std::fs::rename(&staged, &file);
    }
}

/// The answer to one frame, or `None` for a frame that gets none.
/// The file whose being there says a browser's extension has talked to dm.
fn browser_seen_file(home: &Path) -> std::path::PathBuf {
    home.join("browser.seen")
}

/// Notes that an extension has said hello, and which browser it is in
/// where that can be told (a line each). Best effort: a home that cannot
/// be written to only means the window goes on offering the extension.
fn mark_browser_seen(home: &Path) {
    note_browser(home, parent_browser().as_deref());
}

/// [`mark_browser_seen`] for a browser already named (or none).
fn note_browser(home: &Path, browser: Option<&str>) {
    let file = browser_seen_file(home);
    let known = std::fs::read_to_string(&file).unwrap_or_default();
    let fresh = browser.filter(|b| !known.lines().any(|line| line == *b));
    if file.exists() && fresh.is_none() {
        return;
    }
    let _ = std::fs::create_dir_all(home);
    let mut text = known;
    if let Some(name) = fresh {
        text.push_str(name);
        text.push('\n');
    }
    let _ = std::fs::write(file, text);
}

/// The browsers whose extension has talked to this dm, by the names
/// [`parent_browser`] gives ("Microsoft Edge", "Firefox", "Brave").
pub fn browsers_seen(home: &Path) -> Vec<String> {
    std::fs::read_to_string(browser_seen_file(home))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether any browser's extension has ever talked to this dm: until one
/// has, the window says how to add it.
pub fn browser_seen(home: &Path) -> bool {
    browser_seen_file(home).exists()
}

/// The native host's reply to one frame from the browser's extension, as
/// JSON: `None` for a frame it does not take (another version's, or one
/// for the window, such as a pause). What the reply needs of the app (its
/// settings, its inbox, a page's qualities) is asked of `world`.
pub fn answer(
    home: &Path,
    frame: &serde_json::Value,
    world: &mut dyn World,
) -> Option<serde_json::Value> {
    let reply = match Asked::from_frame(frame) {
        // What this app can do, said up front.
        Ok(Asked::Hello) => {
            // Remembered, so the window stops offering the extension to
            // someone who has it (`browser_seen`).
            mark_browser_seen(home);
            Reply::Hello(Hello {
                engine_version: env!("CARGO_PKG_VERSION").into(),
                // The extension hands a download the browser holds only to an
                // app that says it judges what it is served.
                protocol: Some(dm_protocol::PROTOCOL_VERSION),
                // The site rules the extension adds to its compiled tables: a fix for
                // a site reaches it with the app, not with a store release.
                rules: Some(served::served(home)),
                renews: true,
            })
        }

        // Which downloads to catch: the app's choice, read on every ask so a
        // change in the window is in force by the next download.
        Ok(Asked::Settings) => Reply::Settings(SettingsReply {
            rules: catch_rules(home),
        }),

        Ok(Asked::Batch(Batch {
            id,
            page,
            urls,
            cookies,
            texts,
            names,
        })) => {
            let links: Vec<String> = urls
                .into_iter()
                .map(|u| u.trim().to_string())
                .filter(|u| dm_core::accepted(u))
                .take(2000)
                .collect();
            if links.is_empty() {
                Reply::failed(&id, "there are no links here dm can download")
            } else {
                let filed = deposit(
                    home,
                    &id,
                    &Request {
                        url: page.clone(),
                        referrer: page,
                        ask: true,
                        // The words of the links sent, no others.
                        texts: texts
                            .into_iter()
                            .filter(|(url, _)| links.contains(url))
                            .collect(),
                        names: names
                            .into_iter()
                            .filter(|(url, _)| links.contains(url))
                            .collect(),
                        links,
                        // Scoped to the page's site by the dialog's sign-in.
                        cookies,
                        ..Request::default()
                    },
                );
                match filed {
                    Ok(_) if world.app_running() => {
                        world.ring();
                        Reply::Started { id }
                    }
                    Ok(filed) => match world.launch() {
                        Ok(()) => Reply::Started { id },
                        // Taken back out: the browser keeps it, and a dm
                        // started later must not download it a second time.
                        Err(e) => {
                            // Only one this hand-off filed: the same request
                            // already waiting was answered Started before.
                            if filed.new {
                                let _ = std::fs::remove_file(&filed.path);
                            }
                            Reply::failed(&id, format!("dm could not be started to take it: {e}"))
                        }
                    },
                    Err(e) => Reply::failed(&id, format!("dm could not take it: {e}")),
                }
            }
        }

        // The toolbar button: the window to the front, or dm started, which
        // opens it. Nothing is downloaded.
        Ok(Asked::Show) => {
            let shown = if world.app_running() {
                let shown = std::fs::create_dir_all(inbox(home))
                    .and_then(|()| std::fs::write(inbox(home).join("show"), b""));
                world.ring();
                shown
            } else {
                world.launch()
            };
            Reply::Shown {
                reason: shown
                    .err()
                    .map(|e| format!("dm could not be brought up: {e}")),
            }
        }

        Ok(Asked::Formats(FormatsAsked {
            url,
            referrer,
            cookies,
        })) => match world.formats(url.trim(), referrer.trim(), &cookies) {
            Ok(choices) => Reply::Formats {
                choices,
                reason: None,
            },
            Err(why) => Reply::Formats {
                choices: Vec::new(),
                reason: Some(why),
            },
        },

        Ok(Asked::Start(Start {
            id,
            url,
            referrer,
            caught,
            quality,
            sound,
            label,
            ask,
            from,
            to,
            cookies,
            cookies_once,
            headers,
            title,
            mime,
            deadline,
            post,
            renews,
            ..
        })) => {
            let url = url.trim().to_string();
            if url.is_empty() {
                Reply::failed(&id, "no URL was given")
            } else if renews && !world.app_running() {
                // Nothing waits for it with no app running, and dm is not
                // started for a renewal the person did not ask for.
                Reply::failed(&id, "no dm is running to take a renewed address")
            } else if !dm_core::accepted(&url)
                // A file on this computer is dm's to read only when the
                // person opens it with dm, never at a web page's word.
                || url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:"))
            {
                // Refused here by the queue's own rule, rather than accepted
                // and then dropped by the queue unseen.
                Reply::failed(
                    &id,
                    "dm downloads http(s) and ftp links, magnets and .torrent files only",
                )
            } else if let Some(why) = caught
                .then(|| unwritable(&saved(home).download_dir))
                .flatten()
            {
                // A download the browser holds is left with it when dm could
                // not save it anyway: taken, it would sit paused in dm.
                Reply::failed(&id, why)
            } else if let Some(why) = checked(
                world,
                // Not asked about first where the person said the site's
                // links work once (Options › Browser): the asking spent the
                // link before dm's own download could use it.
                // Nor a form's answer: asking would send the form again.
                caught && post.is_none() && !saved(home).catch.works_once(&url, &referrer),
                Asking {
                    url: &url,
                    referrer: &referrer,
                    cookies: &cookies,
                    headers: &headers,
                },
                &mime,
                deadline,
            ) {
                // Before anything is filed: the browser fetches this with its
                // cookies and its session, and dm has neither. A link only
                // the signed-in browser can fetch is refused here, while the
                // browser still holds it, rather than taken from the browser
                // and failed in dm — a download caught and then lost.
                Reply::failed(&id, why)
            } else {
                match deposit(
                    home,
                    &id,
                    &Request {
                        url,
                        referrer,
                        quality: quality.filter(|q| !q.trim().is_empty()),
                        sound: sound
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty()),
                        label,
                        // A caught download asks too, where Options says so;
                        // a renewal never does.
                        ask: !renews && (ask || (caught && saved(home).ask_caught_downloads)),
                        // Only a video asked for from the panel has a part.
                        from: if caught {
                            String::new()
                        } else {
                            from.trim().to_string()
                        },
                        to: if caught {
                            String::new()
                        } else {
                            to.trim().to_string()
                        },
                        caught,
                        cookies: cookies.trim().to_string(),
                        cookies_once: cookies_once
                            && (!cookies.trim().is_empty() || !headers.is_empty()),
                        headers: dm_core::cookies::forwardable(&headers),
                        title: title.trim().chars().take(300).collect(),
                        browser: parent_browser(),
                        post,
                        from_browser: true,
                        renews,
                        ..Request::default()
                    },
                ) {
                    Ok(_) if world.app_running() => {
                        world.ring();
                        Reply::Started { id }
                    }
                    Ok(filed) => match world.launch() {
                        Ok(()) => Reply::Started { id },
                        // Filed, but nothing will read it: said now, not
                        // discovered when the file never appears — and taken
                        // back out, since the browser keeps the download and
                        // a dm started later must not fetch it as well.
                        Err(e) => {
                            // Only one this hand-off filed: the same request
                            // already waiting was answered Started before.
                            if filed.new {
                                let _ = std::fs::remove_file(&filed.path);
                            }
                            Reply::failed(&id, format!("dm could not be started to take it: {e}"))
                        }
                    },
                    Err(e) => Reply::failed(&id, format!("dm could not take it: {e}")),
                }
            }
        }

        // Another version, or something this host does not do: pausing and
        // cancelling belong to the window, which is where the download is.
        Err(_) => return None,
    };
    Some(reply.to_frame())
}

/// Why dm may not save into `folder`, or `None` where it may: made where
/// it is missing, and a file written in it and taken away again.
pub(crate) fn unwritable(folder: &Path) -> Option<String> {
    // A new file of an unguessable name, never one already there: a link
    // planted at the name is not followed (`create_new`). The count keeps
    // two checks at once (two hand-offs, threads of one process) apart; a
    // name already taken is tried again under the next, and only a file
    // this check made is removed.
    static CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tried = std::fs::create_dir_all(folder).and_then(|()| {
        let mut last = None;
        for _ in 0..3 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let count = CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let probe = folder.join(format!(
                ".dm-write-check-{}-{count}-{nanos}",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)
            {
                Ok(file) => {
                    drop(file);
                    let _ = std::fs::remove_file(&probe);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no name to check with")))
    });
    tried.err().map(|e| {
        format!(
            "dm may not save into its download folder {} ({e}), so the browser keeps the \
             download; choose another folder in dm's Options › Folders",
            folder.display()
        )
    })
}

/// The request a hand-off would make: its address and what the browser sent with it.
#[derive(Clone, Copy)]
struct Asking<'a> {
    url: &'a str,
    referrer: &'a str,
    cookies: &'a str,
    headers: &'a [(String, String)],
}

/// Why a hand-off must not be taken, or `None` when it can be filed.
///
/// Too late first: past the extension's deadline it has given the download
/// back to the browser, and a download filed now is fetched twice. Then, for
/// a download the browser holds, whether dm can fetch it at all — and
/// whether what dm is served is the file and not a sign-in page the site
/// sends a request without the browser's cookies to, which a check for
/// errors alone passed, losing the browser's copy for a page of HTML. The
/// time is checked again after asking, since asking takes most of it.
fn checked(
    world: &mut dyn World,
    caught: bool,
    asking: Asking<'_>,
    mime: &str,
    deadline: Option<u64>,
) -> Option<String> {
    let Asking {
        url,
        referrer,
        cookies,
        headers,
    } = asking;
    let late = "the browser stopped waiting for dm, so it keeps the download";
    let Some(within) = dm_protocol::time_left(world.now_ms(), deadline) else {
        return Some(late.into());
    };
    if !caught {
        return None;
    }
    match world.reachable_with(url, referrer, cookies, headers, within) {
        Err(why) => Some(format!("dm cannot fetch this without the browser: {why}")),
        Ok(served) if dm_protocol::page_instead_of_file(mime, url, &served) => Some(format!(
            "dm cannot fetch this without the browser: the site answers dm with a page \
             ({}) rather than the file, usually a sign-in",
            served.trim()
        )),
        Ok(_) if dm_protocol::time_left(world.now_ms(), deadline).is_none() => Some(late.into()),
        Ok(_) => None,
    }
}

/// The argument that makes dm read one page for the panel and leave
/// (`warm_main`).
pub const WARM_FLAG: &str = "--warm-reading";

#[allow(dead_code)]
fn detach(command: &mut Command, breakaway: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let _ = breakaway;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command.creation_flags(if breakaway {
            DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB
        } else {
            DETACHED_PROCESS
        });
    }
}

/// What `--warm-reading` does: no longer needed since YouTube is read by
/// dm itself and non-YouTube sites have no page reader.
pub fn warm_main(_home: &Path) {}

/// Runs as the browser's native messaging host until the browser closes it.
pub fn run(home: &Path) {
    dm_core::tools::set_folder(home.join("tools"));
    dm_core::tools::allow_fetching(saved(home).fetch_tools);
    let mut input = std::io::stdin();
    let mut output = std::io::stdout();
    let mut here = Here {
        home,
        started: false,
    };

    while let Some(message) = read_frame(&mut input) {
        let Some(reply) = answer(home, &message, &mut here) else {
            continue;
        };
        if output.write_all(&frame(&reply)).is_err() || output.flush().is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;

/// The host as the browser runs it, through what only this module reaches:
/// the real `World` (the instance lock, the program started, a probe of a
/// server on this computer), and the hand-offs that cannot be filed or
/// whose dm cannot be started.
#[cfg(test)]
mod coverage_tests {
    use super::*;

    /// A world with no network and a dm that is or is not there.
    struct Stub {
        running: bool,
        launches: bool,
        rung: usize,
    }

    impl World for Stub {
        fn app_running(&mut self) -> bool {
            self.running
        }
        fn launch(&mut self) -> std::io::Result<()> {
            if self.launches {
                Ok(())
            } else {
                Err(std::io::Error::other("no such program"))
            }
        }
        fn ring(&mut self) {
            self.rung += 1;
        }
        fn reachable(&mut self, _: &str, _: &str, _: &str, _: u64) -> Result<String, String> {
            Ok("application/zip".into())
        }
        fn formats(&mut self, _: &str, _: &str, _: &str) -> Result<Vec<Choice>, String> {
            Ok(Vec::new())
        }
    }

    fn stub(running: bool, launches: bool) -> Stub {
        Stub {
            running,
            launches,
            rung: 0,
        }
    }

    fn reply(home: &Path, frame: serde_json::Value, world: &mut Stub) -> Reply {
        Reply::from_frame(&answer(home, &frame, world).expect("an answer")).unwrap()
    }

    fn failure(reply: Reply) -> String {
        match reply {
            Reply::Failed { failure, .. } => failure.unwrap().message,
            other => panic!("taken: {other:?}"),
        }
    }

    fn filed(home: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(inbox(home))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "json"))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn batch(id: &str) -> serde_json::Value {
        Asked::Batch(Batch {
            id: id.into(),
            page: "https://e.com/list.html".into(),
            urls: vec!["https://e.com/a.zip".into()],
            ..Batch::default()
        })
        .to_frame()
    }

    #[test]
    fn a_pages_links_start_dm_when_none_runs_and_are_taken_back_when_it_cannot_start() {
        let home = tempfile::tempdir().unwrap();
        let mut world = stub(false, true);
        assert_eq!(
            reply(home.path(), batch("links-1"), &mut world),
            Reply::Started {
                id: "links-1".into()
            }
        );
        assert_eq!(filed(home.path()).len(), 1, "left for the dm started");
        assert_eq!(world.rung, 0, "no dm was there to ring for");

        // dm cannot be started: the browser is told, and nothing is left
        // for a dm started later to download a second time.
        let home = tempfile::tempdir().unwrap();
        let said = failure(reply(
            home.path(),
            batch("links-2"),
            &mut stub(false, false),
        ));
        assert!(said.contains("could not be started"), "{said}");
        assert!(filed(home.path()).is_empty());

        // The very same links already waiting were answered before: they
        // stay where they are.
        let home = tempfile::tempdir().unwrap();
        reply(home.path(), batch("links-3"), &mut stub(true, true));
        let said = failure(reply(
            home.path(),
            batch("links-3"),
            &mut stub(false, false),
        ));
        assert!(said.contains("could not be started"), "{said}");
        assert_eq!(filed(home.path()).len(), 1);
    }

    #[test]
    fn a_hand_off_whose_name_cannot_be_a_file_is_refused_with_why() {
        let home = tempfile::tempdir().unwrap();
        let said = failure(reply(home.path(), batch("../.."), &mut stub(true, true)));
        assert!(said.starts_with("dm could not take it"), "{said}");
        let start = Asked::Start(Start {
            id: "...".into(),
            url: "https://e.com/f.zip".into(),
            ..Start::default()
        })
        .to_frame();
        let said = failure(reply(home.path(), start, &mut stub(true, true)));
        assert!(said.starts_with("dm could not take it"), "{said}");
        assert!(filed(home.path()).is_empty());
    }

    #[test]
    fn a_caught_download_stays_with_the_browser_when_the_download_folder_cannot_be_made() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("a file");
        std::fs::write(&file, b"").unwrap();
        std::fs::write(
            home.path().join("settings.json"),
            serde_json::json!({ "download_dir": file.join("downloads") }).to_string(),
        )
        .unwrap();
        let start = Asked::Start(Start {
            id: "caught-1".into(),
            url: "https://e.com/f.zip".into(),
            caught: true,
            ..Start::default()
        })
        .to_frame();
        let said = failure(reply(home.path(), start, &mut stub(true, true)));
        assert!(
            said.contains("may not save into its download folder"),
            "{said}"
        );
        assert!(filed(home.path()).is_empty());
    }

    #[test]
    fn a_thousand_requests_under_one_name_are_not_written_over() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("same.json"), b"first").unwrap();
        for n in 2..=1000 {
            std::fs::write(dir.path().join(format!("same-{n}.json")), b"other").unwrap();
        }
        let refused = file_new(dir.path(), "same", b"new").unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(dir.path().join("same.json")).unwrap(),
            b"first"
        );
        let left = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(left, 0, "the staged copy was left behind");
    }

    #[test]
    fn the_real_world_knows_whether_dm_runs_and_says_why_it_cannot_tell() {
        let home = tempfile::tempdir().unwrap();
        let mut here = Here {
            home: home.path(),
            started: false,
        };
        assert!(!here.app_running(), "no dm holds this home's lock");
        here.started = true;
        assert!(here.app_running(), "the one this host started counts");

        // A home that is a file has no lock to read: no dm is started, and
        // the browser is told why.
        let file = home.path().join("not a folder");
        std::fs::write(&file, b"").unwrap();
        let mut here = Here {
            home: &file,
            started: false,
        };
        assert!(!here.app_running());
        assert!(here.launch().is_err());
        assert!(!here.started);
    }

    #[test]
    fn a_caught_download_dm_can_fetch_is_filed_and_a_quality_list_is_passed_on() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("settings.json"),
            serde_json::json!({ "download_dir": home.path().join("out") }).to_string(),
        )
        .unwrap();
        let mut world = stub(true, true);
        let start = Asked::Start(Start {
            id: "caught-2".into(),
            url: "https://e.com/f.zip".into(),
            caught: true,
            mime: "application/zip".into(),
            ..Start::default()
        })
        .to_frame();
        assert_eq!(
            reply(home.path(), start, &mut world),
            Reply::Started {
                id: "caught-2".into()
            }
        );
        assert_eq!(filed(home.path()).len(), 1);
        assert_eq!(world.rung, 1, "the running dm was told");

        let asked = Asked::Formats(FormatsAsked {
            url: " https://e.com/watch ".into(),
            ..FormatsAsked::default()
        })
        .to_frame();
        assert_eq!(
            reply(home.path(), asked, &mut world),
            Reply::Formats {
                choices: Vec::new(),
                reason: None
            }
        );
    }

    /// The program started is this test's own binary with `--desktop`, which
    /// the test harness refuses and leaves at once: nothing is opened.
    #[test]
    fn the_real_world_starts_dm_detached_from_the_browser_and_remembers_it_did() {
        let home = tempfile::tempdir().unwrap();
        let mut here = Here {
            home: home.path(),
            started: false,
        };
        here.launch().unwrap();
        assert!(here.started);
        assert!(here.app_running());
    }

    #[test]
    fn the_real_world_rings_a_running_dms_bell_and_reads_the_clock() {
        let home = tempfile::tempdir().unwrap();
        let bell = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        bell.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        std::fs::create_dir_all(inbox(home.path())).unwrap();
        std::fs::write(
            inbox(home.path()).join(BELL),
            bell.local_addr().unwrap().port().to_string(),
        )
        .unwrap();
        let mut here = Here {
            home: home.path(),
            started: false,
        };
        here.ring();
        let mut buf = [0u8; 8];
        let (got, _) = bell.recv_from(&mut buf).expect("the bell rang");
        assert_eq!(got, 0, "a ring carries nothing");

        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let now = here.now_ms();
        assert!(now >= before && now < before + 60_000, "{now} {before}");
    }

    /// A server on this computer that answers every request with `answer`,
    /// or holds every connection open and says nothing.
    fn server(answer: Option<&'static str>) -> std::net::SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut asked = Vec::new();
                let mut buf = [0u8; 1024];
                while !asked.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => asked.extend_from_slice(&buf[..n]),
                    }
                }
                match answer {
                    Some(text) => {
                        let _ = stream.write_all(text.as_bytes());
                    }
                    None => held.push(stream),
                }
            }
        });
        addr
    }

    #[test]
    fn the_real_world_asks_the_server_what_it_sends() {
        let home = tempfile::tempdir().unwrap();
        let mut here = Here {
            home: home.path(),
            started: false,
        };
        let addr = server(Some(
            "HTTP/1.1 206 Partial Content\r\ncontent-type: video/mp4\r\n\
             content-range: bytes 0-0/100\r\ncontent-length: 1\r\nconnection: close\r\n\r\nx",
        ));
        let served = here.reachable_with(
            &format!("http://{addr}/film.mp4"),
            "http://page.example/watch",
            "sid=1",
            &[("Accept-Language".into(), "en".into())],
            5000,
        );
        assert_eq!(served, Ok("video/mp4".into()));

        // A server that never answers is given up on in the time allowed.
        let silent = server(None);
        let said = here
            .reachable(&format!("http://{silent}/f.zip"), "", "", 300)
            .unwrap_err();
        assert_eq!(said, "the server did not answer dm within 1 seconds");

        // Nothing listening: the probe's own error.
        let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = gone.local_addr().unwrap().port();
        drop(gone);
        assert!(here
            .reachable(&format!("http://127.0.0.1:{port}/f.zip"), "", "", 5000)
            .is_err());
    }

    #[test]
    fn the_real_world_lists_qualities_from_a_reading_kept_or_none_for_a_site_without_them() {
        let home = tempfile::tempdir().unwrap();
        let mut here = Here {
            home: home.path(),
            started: false,
        };
        assert_eq!(
            here.formats("magnet:?xt=urn:btih:abc", "", ""),
            Err("only a web page has qualities to list".into())
        );
        // Read a moment ago: given again without asking anyone.
        let page = "https://video.example/watch/1";
        let kept = Choice {
            label: "MP4 1280×720".into(),
            quality: "720".into(),
            size: Some(10),
        };
        remember_formats(
            home.path(),
            &formats_key(page, "sid=1"),
            std::slice::from_ref(&kept),
            now_secs(),
        );
        assert_eq!(here.formats(page, "", "sid=1"), Ok(vec![kept]));
        // A page of a site dm reads no qualities from: none, and nothing is
        // asked of the network (nothing listens on port 9 here).
        assert_eq!(
            here.formats("http://127.0.0.1:9/page", "http://127.0.0.1:9/", ""),
            Ok(Vec::new())
        );
        assert!(
            recalled_formats(
                home.path(),
                &formats_key("http://127.0.0.1:9/page", ""),
                now_secs()
            )
            .is_none(),
            "an empty reading is not kept"
        );
    }

    #[test]
    fn the_warm_reading_does_nothing() {
        let home = tempfile::tempdir().unwrap();
        warm_main(home.path());
        assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
    }

    /// A program detached this way leads a process group of its own, so the
    /// browser ending the host's group does not end it.
    #[cfg(unix)]
    #[test]
    fn a_detached_program_is_in_a_process_group_of_its_own() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo $$ $(ps -o pgid= -p $$)"]);
        detach(&mut command, true);
        let out = command.output().unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let ids: Vec<&str> = said.split_whitespace().collect();
        assert_eq!(ids.len(), 2, "{said}");
        assert_eq!(ids[0], ids[1], "not its own group's leader: {said}");
        // SAFETY: reads this process's own group, and cannot fail.
        let ours = unsafe { libc::getpgrp() }.to_string();
        assert_ne!(ids[1], ours, "left in the host's group");
    }
}
