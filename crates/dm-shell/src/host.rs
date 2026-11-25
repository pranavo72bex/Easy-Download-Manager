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
    feature, Batch, CatchRules, Choice, FormatsAsked, Hello, Reply, Request as Asked,
    SettingsReply, Start,
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
    /// `--queue NAME`: into that queue (made where there is none of that name), not started
    /// (gaps row C7a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    /// `--quiet`: started with nothing asked, whatever Options says about showing Download
    /// details first (gaps row F3).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub quiet: bool,
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
    /// same file (`DownloadList::adopt_fresh_link`) rather than be added again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_browser: bool,
    /// Only a newer address for a row waiting for one (`Start::renews`):
    /// dropped where no row takes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub renews: bool,
    /// "Keep no record": from a private browser window (`Start::private_window`), the row goes
    /// once its file is whole (`Entry::keep_no_record`, gaps row W17).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_no_record: bool,
}

mod parent;
mod replies;
pub mod served;
/// The browser this host was started by, by its parent process's program
/// name (past the `cmd` Chrome and Edge start it through on Windows), in the
/// words a person knows it by. `None` where it cannot be told.
pub use parent::parent_browser;

mod answer;
mod formats;
mod seen;
pub use answer::answer;
#[cfg(test)]
use answer::menu_start;
#[cfg(all(test, unix))]
use answer::unwritable;
#[cfg(test)]
use formats::{formats_file, Remembered, FORMATS_KEPT_PAGES, FORMATS_KEPT_SECS};
use formats::{formats_key, now_secs, recalled_formats, remember_formats};
pub use seen::{browser_seen, browsers_heard, browsers_seen, extension_heard};
#[cfg(test)]
use seen::{extension_file, note_browser, note_browser_at, EXTENSION_FORMAT};
use seen::{mark_browser_seen, note_extension};

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
    home.join(dm_core::paths::INBOX)
}

/// The file in the inbox where a running dm writes the loopback port its
/// doorbell listens on. No extension, so the watcher never reads it as a
/// request.
pub const BELL: &str = dm_core::paths::INBOX_BELL;

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

/// A link a page named, as dm takes it from a page's "Download all links": a web address dm
/// can download, with no login written into it, and not one inside this computer or its
/// network where the page is out on the internet. The dialog asks each link's server for its
/// name and size as it opens, so a public page listing a router's address with a login written
/// into it would have dm send that request, signed in, from the person's computer (security
/// audit).
/// `None` for a link not taken.
pub fn page_link(page: &str, link: &str) -> Option<String> {
    let link = link.trim();
    let mut parsed = url::Url::parse(link).ok()?;
    if !matches!(parsed.scheme(), "http" | "https" | "ftp") || !dm_core::accepted(link) {
        return None;
    }
    if dm_core::redirects::named_inside(page, link) {
        return None;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        return Some(parsed.to_string());
    }
    Some(link.to_string())
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
    // Started by dm's Safari extension (crates/dm-safari), this host runs in the
    // extension's sandbox, and so would a dm it started itself: the window is opened through
    // the system instead, as a person's click opens it, outside the sandbox.
    let sandboxed = std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some();
    if let Some(app) = opened_through_system(&executable, sandboxed) {
        return crate::platform::programs::open_app(&app);
    }
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

/// The app bundle to open through the system rather than start `executable` directly: where
/// this host runs in a sandbox (Safari's extension) and is inside one.
fn opened_through_system(executable: &Path, sandboxed: bool) -> Option<PathBuf> {
    sandboxed.then(|| app_bundle(executable)).flatten()
}

/// The `.app` a program is the executable of (`dm.app/Contents/MacOS/dm-gpui` gives `dm.app`),
/// or `None` for one outside a bundle.
pub(crate) fn app_bundle(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
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
    let mut rules = saved(home).browser.catch;
    // The app brings a saved list up to date as it starts; the browser asks
    // for it whether or not the app has.
    rules.bring_up_to_date();
    rules
}

pub(crate) fn saved(home: &Path) -> crate::Settings {
    // Read as the window reads it, so the two never disagree (an older
    // edition of the pop-up defaults, one odd field), and read only: this
    // process has no window to say it moved a file aside.
    crate::Settings::read_only(&home.join(dm_core::paths::SETTINGS))
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
        let _ = dm_core::cookies::use_file(Path::new(settings.advanced.cookies_file.trim()));
        // Never around the proxy: a proxy that cannot be built here must
        // not turn into a direct request, which is the server seeing the
        // address the person set a proxy to hide. The browser keeps it.
        let client = dm_core::client_with(&settings.advanced.proxy).map_err(|e| {
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
                    dm_core::cookies::SignIn::with_headers(
                        url,
                        cookies,
                        &settings.site_headers(url, headers),
                    )
                    .as_ref(),
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
        let proxy = saved(self.home).advanced.proxy.url_for_tools();
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

/// The argument that makes dm read one page for the panel and leave
/// (`warm_main`).
pub const WARM_FLAG: &str = "--warm-reading";

/// What `--warm-reading` does: nothing, since YouTube is read by dm itself
/// and non-YouTube sites have no page reader.
pub fn warm_main(_home: &Path) {}

/// Runs as the browser's native messaging host until the browser closes it.
pub fn run(home: &Path) {
    serve_here(home, &mut std::io::stdin(), &mut std::io::stdout());
}

/// [`serve`] in the real world: a dm started, files written to `home`'s inbox.
fn serve_here(home: &Path, input: &mut impl Read, output: &mut impl Write) {
    let mut here = Here {
        home,
        started: false,
    };
    serve(home, input, output, &mut here);
}

/// Answers each frame read from `input` on `output`, in turn, until `input` ends or `output`
/// cannot be written: a frame that gets no answer is skipped, and the next one is read.
pub fn serve(home: &Path, input: &mut impl Read, output: &mut impl Write, world: &mut dyn World) {
    while let Some(message) = read_frame(input) {
        let Some(reply) = answer(home, &message, world) else {
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
mod coverage_tests;
