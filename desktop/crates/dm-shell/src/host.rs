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
    /// What the panel's list called that quality.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Open "Download details" for it rather than starting it, as with a
    /// video from the browser's panel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ask: bool,
    /// Caught by the browser extension, rather than asked for from its
    /// panel or menu: which of Options' two "ask" settings governs it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub caught: bool,
    /// A page's links, for the app's "Download all links" dialog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    /// The browser's cookies for the site, as a `Cookie` header, where the
    /// person switched that on for the site in the toolbar popup.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
    /// Those cookies are a video page's session, for this download alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cookies_once: bool,
}

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
    pub path: PathBuf,
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
    saved(home).catch
}

pub(crate) fn saved(home: &Path) -> crate::Settings {
    std::fs::read_to_string(home.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<crate::Settings>(&text).ok())
        .unwrap_or_default()
}

/// What answering needs from outside this process. A trait so a test can
/// answer without starting a second copy of whatever runs it, or a network.
pub trait World {
    /// Whether a dm is already there to take a download.
    fn app_running(&mut self) -> bool;
    /// Starts one.
    fn launch(&mut self) -> std::io::Result<()>;
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
    /// The qualities a page's video comes in, as yt-dlp reads them.
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

    /// One ranged request, made the way the queue will make it: through the
    /// saved proxy, with the page as referrer. A magnet has no server to ask.
    fn reachable(
        &mut self,
        url: &str,
        referrer: &str,
        cookies: &str,
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
        let client = dm_core::proxy::client_with(&settings.proxy).map_err(|e| {
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
                dm_core::probe::probe(
                    &client,
                    // A site's saved sign-in (Options › Sign-ins), as the
                    // app will use it: asked without it, such a link was
                    // refused and always left with the browser.
                    &settings.with_login(url),
                    referrer,
                    dm_core::cookies::SignIn::new(url, cookies).as_ref(),
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

    /// yt-dlp's reading of the page, through the saved proxy, bounded.
    fn formats(&mut self, url: &str, referrer: &str, cookies: &str) -> Result<Vec<Choice>, String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("only a web page has qualities to list".into());
        }
        let proxy = saved(self.home).proxy.url_for_tools();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let referrer = (!referrer.is_empty()).then_some(referrer);
        // yt-dlp found, or fetched now by dm where the computer has none,
        // through the same proxy the download will take.
        let settings = saved(self.home);
        if let Ok(client) = dm_core::proxy::client_with(&settings.proxy) {
            runtime.block_on(dm_core::tools::for_video(&client))?;
        }
        let listed = runtime
            .block_on(dm_core::ytdlp::list_choices(
                url,
                referrer,
                proxy.as_deref(),
                dm_core::cookies::SignIn::new(url, cookies).as_ref(),
            ))
            .map_err(|e| e.to_string())?;
        Ok(listed
            .into_iter()
            .map(|c| Choice {
                label: c.label,
                quality: c.quality,
                size: c.size,
            })
            .collect())
    }
}

/// The answer to one frame, or `None` for a frame that gets none.
/// The file whose being there says a browser's extension has talked to dm.
fn browser_seen_file(home: &Path) -> std::path::PathBuf {
    home.join("browser.seen")
}

/// Notes that an extension has said hello. Best effort: a home that cannot
/// be written to only means the window goes on offering the extension.
fn mark_browser_seen(home: &Path) {
    let file = browser_seen_file(home);
    if !file.exists() {
        let _ = std::fs::create_dir_all(home);
        let _ = std::fs::write(file, b"");
    }
}

/// Whether any browser's extension has ever talked to this dm: until one
/// has, the window says how to add it.
pub fn browser_seen(home: &Path) -> bool {
    browser_seen_file(home).exists()
}

pub fn answer(
    home: &Path,
    frame: &serde_json::Value,
    world: &mut dyn World,
) -> Option<serde_json::Value> {
    let reply = match Asked::from_frame(frame) {
        // What this app can do, said up front: a player page handed to an
        // app with no yt-dlp fails in the app, where the browser never hears
        // of it. Told now, the extension can say so.
        Ok(Asked::Hello) => {
            // Remembered, so the window stops offering the extension to
            // someone who has it (`browser_seen`).
            mark_browser_seen(home);
            Reply::Hello(Hello {
                engine_version: env!("CARGO_PKG_VERSION").into(),
                // There, or fetched by dm when a page needs it (`tools`).
                ytdlp: Some(dm_core::tools::video_possible()),
                // The extension hands a download the browser holds only to an
                // app that says it judges what it is served.
                protocol: Some(dm_protocol::PROTOCOL_VERSION),
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
        })) => {
            let links: Vec<String> = urls
                .into_iter()
                .map(|u| u.trim().to_string())
                .filter(|u| dm_core::queue::accepted(u))
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
                        links,
                        // Scoped to the page's site by the dialog's sign-in.
                        cookies,
                        ..Request::default()
                    },
                );
                match filed {
                    Ok(_) if world.app_running() => Reply::Started { id },
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
                std::fs::create_dir_all(inbox(home))
                    .and_then(|()| std::fs::write(inbox(home).join("show"), b""))
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
            label,
            ask,
            cookies,
            cookies_once,
            mime,
            deadline,
            ..
        })) => {
            let url = url.trim().to_string();
            if url.is_empty() {
                Reply::failed(&id, "no URL was given")
            } else if !dm_core::queue::accepted(&url)
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
            } else if let Some(why) = checked(
                world,
                // Not asked about first where the person said the site's
                // links work once (Options › Browser): the asking spent the
                // link before dm's own download could use it (gap 49).
                caught && !saved(home).catch.works_once(&url, &referrer),
                &url,
                &referrer,
                &cookies,
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
                        label,
                        // A caught download asks too, where Options says so.
                        ask: ask || (caught && saved(home).ask_caught_downloads),
                        caught,
                        cookies: cookies.trim().to_string(),
                        cookies_once: cookies_once && !cookies.trim().is_empty(),
                        ..Request::default()
                    },
                ) {
                    Ok(_) if world.app_running() => Reply::Started { id },
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
    url: &str,
    referrer: &str,
    cookies: &str,
    mime: &str,
    deadline: Option<u64>,
) -> Option<String> {
    let late = "the browser stopped waiting for dm, so it keeps the download";
    let Some(within) = dm_protocol::time_left(world.now_ms(), deadline) else {
        return Some(late.into());
    };
    if !caught {
        return None;
    }
    match world.reachable(url, referrer, cookies, within) {
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

/// Runs as the browser's native messaging host until the browser closes it.
pub fn run(home: &Path) {
    dm_core::tools::set_folder(home.join("tools"));
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
mod tests {
    use super::*;

    #[test]
    fn a_message_survives_being_framed_and_read_back() {
        let value = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
        let bytes = frame(&value);

        // Four bytes of little-endian length, then the body.
        assert_eq!(&bytes[..4], &(bytes.len() as u32 - 4).to_le_bytes());
        assert_eq!(read_frame(&mut &bytes[..]), Some(value));
    }

    #[test]
    fn a_truncated_stream_ends_rather_than_blocking() {
        // The browser going away mid-message is how this normally ends.
        let bytes = frame(&serde_json::json!({ "type": "engine.hello" }));
        assert_eq!(read_frame(&mut &bytes[..6]), None);
    }

    #[test]
    fn a_frame_that_is_not_json_does_not_end_the_conversation() {
        let mut bytes = 5u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{oops");
        let next = serde_json::json!({ "protocolVersion": 1, "type": "engine.hello" });
        bytes.extend_from_slice(&frame(&next));

        let mut input = &bytes[..];
        assert_eq!(read_frame(&mut input), Some(serde_json::Value::Null));
        assert_eq!(read_frame(&mut input), Some(next));
    }

    #[test]
    fn the_host_refuses_what_the_queue_would_drop() {
        for refused in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftps://e.com/f",
        ] {
            assert!(!dm_core::queue::accepted(refused), "{refused}");
        }
        assert!(dm_core::queue::accepted("https://e.com/f.bin"));
    }

    #[test]
    fn an_implausible_length_is_refused_rather_than_allocated() {
        // A desynchronised stream otherwise asks for gigabytes.
        let mut bytes = u32::MAX.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        assert_eq!(read_frame(&mut &bytes[..]), None);
    }

    #[test]
    fn a_request_lands_where_the_app_looks() {
        let dir = tempfile::tempdir().unwrap();
        let request = Request {
            url: "https://example.com/f.bin".into(),
            referrer: "https://example.com/page".into(),
            ..Request::default()
        };
        deposit(dir.path(), "abc", &request).unwrap();

        let file = inbox(dir.path()).join("abc.json");
        let read: Request = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        assert_eq!(read, request);
    }

    #[test]
    fn a_browser_does_not_get_to_choose_where_a_request_is_written() {
        /* The id arrives in a message and becomes a filename. Anything that
         * writes where a message tells it to is a program that writes
         * wherever the sender likes — and `write_atomic` creates the parent,
         * so a traversal would make the directory as well as the file. */
        let dir = tempfile::tempdir().unwrap();
        let request = Request {
            url: "https://e.com/f.bin".into(),
            referrer: String::new(),
            ..Request::default()
        };

        for hostile in ["../../escaped", "..", "/etc/passwd", "a/b"] {
            let _ = deposit(dir.path(), hostile, &request);
            let escaped = dir.path().join("escaped.json");
            assert!(!escaped.exists(), "{hostile} wrote outside the inbox");
        }

        // Everything written stayed in the inbox, under a plain name.
        for entry in std::fs::read_dir(inbox(dir.path())).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
                "{name} is not a plain name"
            );
        }

        // An id with nothing usable in it is refused rather than guessed at.
        assert!(deposit(dir.path(), "../..", &request).is_err());
    }

    #[test]
    fn two_hand_offs_never_share_one_file() {
        /* Ids that come to one name once made plain used to be written over
         * each other: the first download lost, both reported taken. */
        let dir = tempfile::tempdir().unwrap();
        let long = "X".repeat(64);
        let ids = ["a.1", "a1", "a1", &format!("{long}A"), &format!("{long}B")];
        for (n, id) in ids.iter().enumerate() {
            let request = Request {
                url: format!("https://e.com/{n}.zip"),
                ..Request::default()
            };
            deposit(dir.path(), id, &request).unwrap();
        }
        let mut urls: Vec<String> = std::fs::read_dir(inbox(dir.path()))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .map(|p| {
                serde_json::from_str::<Request>(&std::fs::read_to_string(p).unwrap())
                    .unwrap()
                    .url
            })
            .collect();
        urls.sort();
        assert_eq!(
            urls,
            (0..ids.len())
                .map(|n| format!("https://e.com/{n}.zip"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_same_hand_off_sent_twice_is_filed_once() {
        let dir = tempfile::tempdir().unwrap();
        let request = Request {
            url: "https://e.com/f.zip".into(),
            ..Request::default()
        };
        let first = deposit(dir.path(), "a1", &request).unwrap();
        let again = deposit(dir.path(), "a1", &request).unwrap();
        assert_eq!(first.path, again.path);
        assert!(first.new && !again.new);
        let filed = std::fs::read_dir(inbox(dir.path()))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .count();
        assert_eq!(filed, 1, "one download, filed twice");
    }

    #[test]
    fn no_half_written_request_is_left_for_the_app_to_read() {
        // The app polls a directory; a partial file there is a download
        // dropped without a word.
        let dir = tempfile::tempdir().unwrap();
        deposit(
            dir.path(),
            "abc",
            &Request {
                url: "https://e.com/f".into(),
                referrer: String::new(),
                ..Request::default()
            },
        )
        .unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(inbox(dir.path()))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "a temp file was left behind");
    }

    /// A world with an app that is (or is not) running, a launcher that
    /// records whether it was asked, and servers that answer dm or do not.
    struct Pretend {
        running: bool,
        launches: Result<(), ()>,
        launched: bool,
        refuses: Option<&'static str>,
        asked: Vec<(String, String)>,
        /// The cookies each probe was asked with.
        cookies_seen: Vec<String>,
        /// The type the server sends dm.
        serves: &'static str,
        /// The time, as milliseconds since the epoch, and how far a probe
        /// moves it on.
        clock: u64,
        probe_takes_ms: u64,
        /// How long each probe was allowed.
        budgets: Vec<u64>,
    }

    impl World for Pretend {
        fn app_running(&mut self) -> bool {
            self.running
        }
        fn launch(&mut self) -> std::io::Result<()> {
            self.launched = true;
            self.launches
                .map_err(|()| std::io::Error::other("no such program"))
        }
        fn reachable(
            &mut self,
            url: &str,
            referrer: &str,
            cookies: &str,
            within_ms: u64,
        ) -> Result<String, String> {
            self.asked.push((url.into(), referrer.into()));
            self.cookies_seen.push(cookies.into());
            self.budgets.push(within_ms);
            self.clock += self.probe_takes_ms;
            self.refuses
                .map_or(Ok(self.serves.to_string()), |why| Err(why.into()))
        }
        fn now_ms(&mut self) -> u64 {
            self.clock
        }
        fn formats(
            &mut self,
            url: &str,
            _referrer: &str,
            _cookies: &str,
        ) -> Result<Vec<Choice>, String> {
            if url.contains("unreadable") {
                return Err("yt-dlp could not read that page".into());
            }
            Ok(vec![
                Choice {
                    label: "MP4 1280×720".into(),
                    quality: "720".into(),
                    size: Some(10),
                },
                Choice {
                    label: "M4A audio".into(),
                    quality: "audio".into(),
                    size: None,
                },
            ])
        }
    }

    #[test]
    fn the_toolbar_button_shows_the_app_and_downloads_nothing() {
        // Running: asked to show itself through the inbox, nothing filed.
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(dir.path(), Asked::Show.to_frame(), true, Ok(()));
        assert_eq!(reply, Some(Reply::Shown { reason: None }));
        assert!(inbox(dir.path()).join("show").exists());
        assert!(!launched);
        let filed = std::fs::read_dir(inbox(dir.path()))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .count();
        assert_eq!(filed, 0, "the toolbar button handed something over");

        // Not running: started, which opens the window.
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(dir.path(), Asked::Show.to_frame(), false, Ok(()));
        assert_eq!(reply, Some(Reply::Shown { reason: None }));
        assert!(launched);

        // Not running and cannot be started: said, for the browser to show.
        let dir = tempfile::tempdir().unwrap();
        let (reply, _) = ask(dir.path(), Asked::Show.to_frame(), false, Err(()));
        assert!(
            matches!(reply, Some(Reply::Shown { reason: Some(_) })),
            "{reply:?}"
        );
    }

    #[test]
    fn a_pages_links_reach_the_app_together_to_be_picked_from() {
        let dir = tempfile::tempdir().unwrap();
        let frame = Asked::Batch(Batch {
            id: "links-1".into(),
            page: "https://e.com/list.html".into(),
            urls: vec![
                "https://e.com/a.zip".into(),
                "mailto:a@e.com".into(),
                "https://e.com/b.iso".into(),
            ],
            cookies: "sid=1".into(),
        })
        .to_frame();
        let (reply, _) = ask(dir.path(), frame, true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Started {
                id: "links-1".into()
            })
        );
        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("links-1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.links, ["https://e.com/a.zip", "https://e.com/b.iso"]);
        assert_eq!(filed.referrer, "https://e.com/list.html");
        assert!(filed.ask);
        // The site's sign-in goes with them, for the dialog to scope.
        assert_eq!(filed.cookies, "sid=1");

        let none = Asked::Batch(Batch {
            id: "links-2".into(),
            page: "https://e.com/".into(),
            urls: vec!["mailto:a@e.com".into()],
            ..Batch::default()
        })
        .to_frame();
        let (reply, _) = ask(dir.path(), none, true, Ok(()));
        assert!(matches!(reply, Some(Reply::Failed { .. })));
    }

    #[test]
    fn the_panel_is_told_the_qualities_a_page_comes_in_or_why_not() {
        let dir = tempfile::tempdir().unwrap();
        let frame = |url: &str| {
            Asked::Formats(FormatsAsked {
                url: url.into(),
                referrer: String::new(),
                cookies: String::new(),
            })
            .to_frame()
        };
        let (reply, _) = ask(
            dir.path(),
            frame("https://video.example/watch?v=1"),
            true,
            Ok(()),
        );
        let Some(Reply::Formats { choices, reason }) = reply else {
            panic!("no formats answer");
        };
        assert_eq!(reason, None);
        assert_eq!(choices[0].quality, "720");
        assert_eq!(choices.len(), 2);

        let (reply, _) = ask(
            dir.path(),
            frame("https://unreadable.example/"),
            true,
            Ok(()),
        );
        let Some(Reply::Formats { choices, reason }) = reply else {
            panic!("no formats answer");
        };
        assert!(choices.is_empty());
        assert_eq!(reason.as_deref(), Some("yt-dlp could not read that page"));
    }

    #[test]
    fn a_sites_cookies_handed_over_reach_the_app_and_the_probe() {
        let dir = tempfile::tempdir().unwrap();
        let frame = Asked::Start(Start {
            id: "c1".into(),
            url: "https://files.example/private.zip".into(),
            referrer: "https://files.example/".into(),
            caught: true,
            cookies: "session=abc; theme=dark".into(),
            ..Start::default()
        })
        .to_frame();
        let mut world = Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: None,
            asked: Vec::new(),
            cookies_seen: Vec::new(),
            serves: "application/zip",
            clock: 0,
            probe_takes_ms: 0,
            budgets: Vec::new(),
        };
        let reply = answer(dir.path(), &frame, &mut world).map(|r| Reply::from_frame(&r).unwrap());
        assert_eq!(reply, Some(Reply::Started { id: "c1".into() }));
        // The probe before taking it asked as the signed-in browser would.
        assert_eq!(world.cookies_seen, ["session=abc; theme=dark"]);
        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("c1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.cookies, "session=abc; theme=dark");
        assert!(!filed.cookies_once, "a kept sign-in was filed as one-off");

        // A video page's session is filed as that download's alone.
        let frame = Asked::Start(Start {
            id: "c3".into(),
            url: "https://video.example/watch?v=1".into(),
            cookies: "SID=1".into(),
            cookies_once: true,
            ..Start::default()
        })
        .to_frame();
        answer(dir.path(), &frame, &mut world);
        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("c3.json")).unwrap(),
        )
        .unwrap();
        assert!(filed.cookies_once && filed.cookies == "SID=1", "{filed:?}");
        // A start without any hands none over.
        let (_, _) = ask(
            dir.path(),
            start("c2", "https://plain.example/f.zip"),
            true,
            Ok(()),
        );
        let plain: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("c2.json")).unwrap(),
        )
        .unwrap();
        assert!(plain.cookies.is_empty());
    }

    #[test]
    fn a_video_from_the_panel_reaches_the_app_with_its_quality_and_asks_first() {
        let dir = tempfile::tempdir().unwrap();
        let frame = Asked::Start(Start {
            id: "panel-1".into(),
            url: "https://video.example/watch?v=1".into(),
            quality: Some("720".into()),
            label: Some("MP4 1280×720".into()),
            ask: true,
            ..Start::default()
        })
        .to_frame();
        let (reply, _) = ask(dir.path(), frame, true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Started {
                id: "panel-1".into()
            })
        );
        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("panel-1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.quality.as_deref(), Some("720"));
        assert_eq!(filed.label.as_deref(), Some("MP4 1280×720"));
        assert!(filed.ask);
    }

    fn ask(
        home: &Path,
        frame: serde_json::Value,
        running: bool,
        launches: Result<(), ()>,
    ) -> (Option<Reply>, bool) {
        let mut world = Pretend {
            running,
            launches,
            launched: false,
            refuses: None,
            asked: Vec::new(),
            cookies_seen: Vec::new(),
            serves: "application/zip",
            clock: 0,
            probe_takes_ms: 0,
            budgets: Vec::new(),
        };
        let reply = answer(home, &frame, &mut world);
        (
            reply.map(|r| Reply::from_frame(&r).unwrap()),
            world.launched,
        )
    }

    fn start(id: &str, url: &str) -> serde_json::Value {
        dm_protocol::Request::Start(Start {
            id: id.into(),
            url: url.into(),
            referrer: "https://e.com/page".into(),
            caught: true,
            ..Start::default()
        })
        .to_frame()
    }

    #[test]
    fn a_hello_is_remembered_so_the_window_stops_offering_the_extension() {
        let home = tempfile::tempdir().unwrap();
        assert!(!browser_seen(home.path()));
        let _ = ask(
            home.path(),
            dm_protocol::Request::Hello.to_frame(),
            true,
            Ok(()),
        );
        assert!(browser_seen(home.path()));
    }

    #[test]
    fn a_hello_says_what_this_app_is() {
        let dir = tempfile::tempdir().unwrap();
        let (reply, _) = ask(
            dir.path(),
            dm_protocol::Request::Hello.to_frame(),
            true,
            Ok(()),
        );
        let Some(Reply::Hello(hello)) = reply else {
            panic!("no hello")
        };
        assert_eq!(hello.engine_version, env!("CARGO_PKG_VERSION"));
        assert!(hello.ytdlp.is_some(), "the extension is told either way");
        // Said, so the extension hands this app the downloads the browser
        // holds: it judges a sign-in page served in the file's place.
        assert!(hello.judges_pages());
        assert_eq!(hello.protocol, Some(dm_protocol::PROTOCOL_VERSION));
    }

    #[test]
    fn a_caught_hand_off_at_the_new_version_is_answered_at_the_old() {
        // The extension sends a caught download at version 2; the answer
        // goes back at 1, which an older extension also reads.
        let dir = tempfile::tempdir().unwrap();
        let frame = caught("b9-1", "https://e.com/f.zip", "application/zip", None);
        assert_eq!(frame["protocolVersion"], 2);
        // A file a person asked for, checked but not held by the browser,
        // goes at 1, which an app of version 1 also answers.
        assert_eq!(start("m9-1", "https://e.com/f.zip")["protocolVersion"], 1);
        let mut world = pretend("application/zip", 0, 0);
        let reply = answer(dir.path(), &frame, &mut world).unwrap();
        assert_eq!(reply["protocolVersion"], 1);
        assert_eq!(reply["type"], "download.started");
    }

    #[test]
    fn the_extension_is_sent_the_catch_rules_in_the_apps_settings() {
        let dir = tempfile::tempdir().unwrap();
        let asked = dm_protocol::Request::Settings.to_frame();

        // No settings file: the defaults the app would run with.
        let (reply, _) = ask(dir.path(), asked.clone(), true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Settings(SettingsReply {
                rules: CatchRules::default()
            }))
        );

        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"connections":4,"catch":{"enabled":true,"extensions":["bin"],"skipHosts":["e.com"]}}"#,
        )
        .unwrap();
        let (reply, _) = ask(dir.path(), asked.clone(), true, Ok(()));
        let Some(Reply::Settings(SettingsReply { rules })) = reply else {
            panic!("no settings")
        };
        assert_eq!(rules.extensions, vec!["bin".to_string()]);
        assert_eq!(rules.skip_hosts, vec!["e.com".to_string()]);

        // A file the app could not read is left where it is: this process
        // has no window to say it moved anything.
        std::fs::write(dir.path().join("settings.json"), "{ oops").unwrap();
        let (reply, _) = ask(dir.path(), asked, true, Ok(()));
        assert_eq!(
            reply,
            Some(Reply::Settings(SettingsReply {
                rules: CatchRules::default()
            }))
        );
        assert!(dir.path().join("settings.json").exists());
    }

    #[test]
    fn a_caught_download_asks_where_to_save_only_where_options_say() {
        let filed = |home: &Path, id: &str| -> Request {
            let (reply, _) = ask(home, start(id, "https://e.com/a.zip"), true, Ok(()));
            assert_eq!(reply, Some(Reply::Started { id: id.into() }));
            serde_json::from_str(
                &std::fs::read_to_string(inbox(home).join(format!("{id}.json"))).unwrap(),
            )
            .unwrap()
        };
        // A new install asks.
        let home = tempfile::tempdir().unwrap();
        let new = filed(home.path(), "c1");
        assert!(new.caught && new.ask);
        // A settings file from before the choice existed keeps downloads
        // starting at once, as they did.
        std::fs::write(home.path().join("settings.json"), r#"{"connections":4}"#).unwrap();
        let old = filed(home.path(), "c2");
        assert!(old.caught && !old.ask);
        std::fs::write(
            home.path().join("settings.json"),
            r#"{"ask_caught_downloads":true}"#,
        )
        .unwrap();
        assert!(filed(home.path(), "c3").ask);
    }

    #[test]
    fn a_hand_off_is_filed_and_taken_by_the_running_app() {
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(
            dir.path(),
            start("b1-2", "https://e.com/f.zip"),
            true,
            Ok(()),
        );
        assert_eq!(reply, Some(Reply::Started { id: "b1-2".into() }));
        assert!(!launched, "a running app is not started twice");

        let filed: Request = serde_json::from_str(
            &std::fs::read_to_string(inbox(dir.path()).join("b1-2.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(filed.url, "https://e.com/f.zip");
        assert_eq!(filed.referrer, "https://e.com/page");
    }

    #[test]
    fn with_no_app_running_one_is_started_and_a_failure_to_start_is_said() {
        let dir = tempfile::tempdir().unwrap();
        let (reply, launched) = ask(dir.path(), start("a", "https://e.com/f.zip"), false, Ok(()));
        assert!(launched);
        assert_eq!(reply, Some(Reply::Started { id: "a".into() }));

        let (reply, _) = ask(
            dir.path(),
            start("b", "https://e.com/f.zip"),
            false,
            Err(()),
        );
        let Some(Reply::Failed { id, failure }) = reply else {
            panic!("a launch that failed was reported as taken")
        };
        assert_eq!(id, "b");
        assert!(failure.unwrap().message.contains("could not be started"));
        // The browser keeps it, so nothing of it is left for a dm started
        // later to fetch a second time: only "a" is filed.
        let left: Vec<_> = std::fs::read_dir(inbox(dir.path()))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".json"))
            .collect();
        assert_eq!(left, ["a.json"]);
    }

    #[test]
    fn what_the_queue_would_drop_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        // A .torrent on this computer is opened with dm by the person, never
        // handed over by a page.
        for url in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "file:///home/a/x.torrent",
        ] {
            let (reply, launched) = ask(dir.path(), start("x", url), true, Ok(()));
            assert!(matches!(reply, Some(Reply::Failed { .. })), "{url}");
            assert!(!launched);
        }
        assert!(!inbox(dir.path()).join("x.json").exists());
    }

    #[test]
    fn a_link_dm_cannot_fetch_itself_is_left_with_the_browser() {
        /* The browser has the site's cookies and dm does not. Taken anyway,
         * the browser's copy is cancelled and dm's fails: the download is
         * lost. Refused, the extension gives it back to the browser. */
        let dir = tempfile::tempdir().unwrap();
        let mut world = Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: Some("server returned 403 Forbidden"),
            asked: Vec::new(),
            cookies_seen: Vec::new(),
            serves: "application/zip",
            clock: 0,
            probe_takes_ms: 0,
            budgets: Vec::new(),
        };
        let reply = answer(
            dir.path(),
            &start("p", "https://e.com/private.zip"),
            &mut world,
        )
        .map(|r| Reply::from_frame(&r).unwrap());
        let Some(Reply::Failed { failure, .. }) = reply else {
            panic!("a link dm cannot reach was taken")
        };
        let said = failure.unwrap().message;
        assert!(
            said.contains("without the browser") && said.contains("403"),
            "{said}"
        );
        assert_eq!(
            world.asked,
            vec![(
                "https://e.com/private.zip".into(),
                "https://e.com/page".into()
            )],
            "asked once, the way the queue will ask, referrer and all"
        );
        assert!(!inbox(dir.path()).join("p.json").exists(), "nothing filed");
    }

    /// Gap 49: a site the person said gives links that work once is not
    /// asked about before dm takes its download; another site still is.
    #[test]
    fn a_link_from_a_site_whose_links_work_once_is_taken_unasked() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = crate::Settings::default();
        settings.catch.once_hosts = vec!["files.e.com".into()];
        std::fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string(&settings).unwrap(),
        )
        .unwrap();
        let mut world = pretend("application/zip", 0, 0);
        world.refuses = Some("server returned 410 Gone");
        let reply = answer(
            dir.path(),
            &start("p", "https://files.e.com/once.zip"),
            &mut world,
        )
        .map(|r| Reply::from_frame(&r).unwrap());
        assert!(
            world.asked.is_empty(),
            "the server was asked: {:?}",
            world.asked
        );
        assert!(
            !matches!(reply, Some(Reply::Failed { .. })),
            "refused: {reply:?}"
        );
        assert!(inbox(dir.path()).join("p.json").exists(), "not filed");
        // Another site is still asked first.
        let mut world = pretend("application/zip", 0, 0);
        answer(
            dir.path(),
            &start("q", "https://other.org/f.zip"),
            &mut world,
        );
        assert_eq!(world.asked.len(), 1);
    }

    fn pretend(serves: &'static str, clock: u64, probe_takes_ms: u64) -> Pretend {
        Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: None,
            asked: Vec::new(),
            cookies_seen: Vec::new(),
            serves,
            clock,
            probe_takes_ms,
            budgets: Vec::new(),
        }
    }

    fn caught(id: &str, url: &str, mime: &str, deadline: Option<u64>) -> serde_json::Value {
        dm_protocol::Request::Start(Start {
            id: id.into(),
            url: url.into(),
            referrer: "https://e.com/page".into(),
            caught: true,
            held: true,
            mime: mime.into(),
            deadline,
            ..Start::default()
        })
        .to_frame()
    }

    fn refused(reply: Option<serde_json::Value>) -> String {
        match reply.map(|r| Reply::from_frame(&r).unwrap()) {
            Some(Reply::Failed { failure, .. }) => failure.unwrap().message,
            other => panic!("taken: {other:?}"),
        }
    }

    #[test]
    fn a_sign_in_page_served_in_place_of_the_file_leaves_it_with_the_browser() {
        /* The site sends dm, without the browser's cookies, a redirect to
         * its sign-in page: a 200 of HTML, which a check for errors alone
         * passed. Taken, the browser's copy was cancelled and dm saved the
         * sign-in page. */
        let html = "text/html; charset=utf-8";
        let dir = tempfile::tempdir().unwrap();
        let mut world = pretend(html, 0, 0);
        let said = refused(answer(
            dir.path(),
            &caught("s1", "https://e.com/get?id=1", "application/zip", None),
            &mut world,
        ));
        assert!(
            said.contains("without the browser") && said.contains("text/html"),
            "{said}"
        );
        assert!(!inbox(dir.path()).join("s1.json").exists(), "nothing filed");

        // The browser did not say, but the address names a file.
        let said = refused(answer(
            dir.path(),
            &caught("s2", "https://e.com/files/report.pdf", "", None),
            &mut world,
        ));
        assert!(said.contains("rather than the file"), "{said}");
        assert!(!inbox(dir.path()).join("s2.json").exists());

        // A page the browser was saving as a page is the same page to dm.
        let reply = answer(
            dir.path(),
            &caught("s3", "https://e.com/article", "text/html", None),
            &mut world,
        )
        .map(|r| Reply::from_frame(&r).unwrap());
        assert_eq!(reply, Some(Reply::Started { id: "s3".into() }));

        // And the file itself, served as one, is taken.
        let mut world = pretend("application/zip", 0, 0);
        let reply = answer(
            dir.path(),
            &caught("s4", "https://e.com/get?id=1", "application/zip", None),
            &mut world,
        )
        .map(|r| Reply::from_frame(&r).unwrap());
        assert_eq!(reply, Some(Reply::Started { id: "s4".into() }));
    }

    #[test]
    fn a_hand_off_the_extension_has_stopped_waiting_for_is_not_filed() {
        /* The extension waits so long for an answer and then gives the
         * download back to the browser. A host that took it after that had
         * the file downloaded twice: by the browser and by dm. */
        let dir = tempfile::tempdir().unwrap();
        let now = 1_700_000_000_000;

        // Already too late on arrival: not asked about, not filed.
        let mut world = pretend("application/zip", now, 0);
        let said = refused(answer(
            dir.path(),
            &caught("t1", "https://e.com/f.zip", "", Some(now + 500)),
            &mut world,
        ));
        assert!(said.contains("stopped waiting"), "{said}");
        assert!(world.asked.is_empty(), "the server was asked anyway");
        assert!(!inbox(dir.path()).join("t1.json").exists());

        // The probe is given what is left, short of the margin for answering,
        // and never more than its own limit.
        let mut world = pretend("application/zip", now, 0);
        let reply = answer(
            dir.path(),
            &caught("t2", "https://e.com/f.zip", "", Some(now + 5_000)),
            &mut world,
        )
        .map(|r| Reply::from_frame(&r).unwrap());
        assert_eq!(reply, Some(Reply::Started { id: "t2".into() }));
        assert_eq!(world.budgets, [5_000 - dm_protocol::ANSWER_MARGIN_MS]);
        let mut world = pretend("application/zip", now, 0);
        let _ = answer(
            dir.path(),
            &caught("t3", "https://e.com/f.zip", "", Some(now + 60_000)),
            &mut world,
        );
        assert_eq!(world.budgets, [dm_protocol::PROBE_WITHIN_MS]);

        // A server slow enough to use it all: too late once it answers.
        let mut world = pretend("application/zip", now, 4_000);
        let said = refused(answer(
            dir.path(),
            &caught("t4", "https://e.com/f.zip", "", Some(now + 5_000)),
            &mut world,
        ));
        assert!(said.contains("stopped waiting"), "{said}");
        assert!(!inbox(dir.path()).join("t4.json").exists());

        // A link asked for from a menu is not filed late either.
        let mut world = pretend("application/zip", now, 0);
        let late = dm_protocol::Request::Start(Start {
            id: "t5".into(),
            url: "https://e.com/watch".into(),
            deadline: Some(now - 1),
            ..Start::default()
        })
        .to_frame();
        let _ = refused(answer(dir.path(), &late, &mut world));
        assert!(!inbox(dir.path()).join("t5.json").exists());
    }

    #[test]
    fn a_link_handed_over_on_request_is_taken_without_asking_the_server() {
        // The browser holds nothing to give back, and asking the server now
        // would contact it outside a schedule the app would have kept to.
        let dir = tempfile::tempdir().unwrap();
        let mut world = Pretend {
            running: true,
            launches: Ok(()),
            launched: false,
            refuses: Some("unreachable"),
            asked: Vec::new(),
            cookies_seen: Vec::new(),
            serves: "application/zip",
            clock: 0,
            probe_takes_ms: 0,
            budgets: Vec::new(),
        };
        let asked = dm_protocol::Request::Start(Start {
            id: "m1".into(),
            url: "https://e.com/f.zip".into(),
            ..Default::default()
        })
        .to_frame();
        let reply = answer(dir.path(), &asked, &mut world).map(|r| Reply::from_frame(&r).unwrap());
        assert_eq!(reply, Some(Reply::Started { id: "m1".into() }));
        assert!(world.asked.is_empty(), "the server was asked");
        assert!(inbox(dir.path()).join("m1.json").exists());
    }

    #[test]
    fn a_proxy_that_cannot_be_used_is_not_gone_around_to_ask_the_server() {
        /* The server would see the address the person set a proxy to hide:
         * the check refuses, and the browser keeps the download. A server
         * on loopback counts every request it gets. */
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            serde_json::json!({
                "proxy": { "enabled": true, "type": "http", "host": "", "port": 8080 }
            })
            .to_string(),
        )
        .unwrap();
        let mut here = Here {
            home: dir.path(),
            started: false,
        };
        let asked = here.reachable(&format!("http://{addr}/f.zip"), "", "", 2000);
        assert!(asked.is_err(), "{asked:?}");
        let mut contacted = false;
        if let Ok((mut stream, _)) = listener.accept() {
            contacted = true;
            let mut buf = [0u8; 64];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        }
        assert!(
            !contacted,
            "the server was asked directly, around the proxy"
        );
    }

    #[test]
    fn a_magnet_is_not_probed_as_though_it_had_a_server() {
        let dir = tempfile::tempdir().unwrap();
        let mut here = Here {
            home: dir.path(),
            started: false,
        };
        assert_eq!(
            here.reachable("magnet:?xt=urn:btih:abc", "", "", 1000),
            Ok(String::new())
        );
    }

    #[test]
    fn a_frame_this_host_does_not_answer_gets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        for frame in [
            serde_json::json!({ "protocolVersion": 1, "type": "download.pause", "id": "a" }),
            serde_json::json!({ "protocolVersion": 3, "type": "engine.hello" }),
            serde_json::json!({ "protocolVersion": 0, "type": "engine.hello" }),
            serde_json::Value::Null,
        ] {
            assert_eq!(ask(dir.path(), frame, true, Ok(())).0, None);
        }
    }
}
