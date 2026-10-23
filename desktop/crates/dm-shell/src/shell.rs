//! What the app does around its window: how it starts, how it takes a
//! browser's hand-off, how a settings change is put into force.
//!
//! None of it depends on how the window is drawn, so it lives here rather
//! than in the egui window's crate, and a test can drive it with no window.

use std::path::{Path, PathBuf};

use dm_core::{Checksum, Id, Queue};

use crate::host;
use crate::settings::{self, Settings};

/// The name of the native messaging manifest, which is also how Firefox
/// identifies itself when it launches this binary as a host.
pub const HOST_MANIFEST: &str = "com.dm.engine.json";

/// Whether a browser started this rather than a person.
///
/// A native messaging manifest names an executable and cannot pass it
/// arguments, so a flag of our own would never arrive. Browsers do identify
/// themselves, but not in the same way, and getting that wrong is silent:
/// the binary opens a window, writes nothing to stdout, and the browser sits
/// holding a port that never answers. That is exactly what Firefox did.
///
/// - **Chrome** (and the browsers built on it) passes the calling
///   extension's origin, `chrome-extension://<id>/`, and on Windows a
///   `--parent-window=<handle>` after it.
/// - **Firefox** passes the path of the app manifest, and in some versions
///   the extension's id after it: never a link, never a flag.
///
/// Detecting both is what lets one binary be both the window and the host,
/// with no shim script in between to install, sign and keep in step.
///
/// The whole command line has to be that shape, not one argument somewhere
/// in it: a link ending in `com.dm.engine.json`, a `--name` of that, or a
/// `chrome-extension://` link passed by a person turned their launch into
/// a host that read nothing from stdin and dropped the link.
pub fn browser_launch(args: &[String]) -> bool {
    match args {
        // dm's own way in, for scripts and the installers' checks.
        [only] if only == "--native-host" => true,
        [origin, rest @ ..]
            if origin.starts_with("chrome-extension://")
                || origin.starts_with("moz-extension://") =>
        {
            rest.len() <= 1 && rest.iter().all(|arg| arg.starts_with("--parent-window="))
        }
        [manifest, rest @ ..] if is_host_manifest(manifest) => {
            rest.len() <= 1
                && rest
                    .iter()
                    .all(|id| !id.is_empty() && !id.starts_with('-') && !id.contains("://"))
        }
        _ => false,
    }
}

/// A path to dm's own app manifest, as Firefox gives it: a file path, not
/// a link that happens to end the same way.
fn is_host_manifest(arg: &str) -> bool {
    let path = Path::new(arg);
    !arg.contains("://")
        && path.has_root()
        && path.file_name().is_some_and(|name| name == HOST_MANIFEST)
}

/// This process's arguments, less its own name.
pub fn args() -> Vec<String> {
    std::env::args().skip(1).collect()
}

/// Whether this launch was the login item's, which opens nothing.
///
/// Starting with the computer is for the queue and the browser hand-off,
/// not for a window over whatever the person was about to do; it waits in
/// the tray until asked for.
pub fn launched_at_login(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--hidden")
}

/// What the command line asks for: downloads, and whether to start the
/// main queue.
#[derive(Debug, Default, PartialEq)]
pub struct Orders {
    pub downloads: Vec<host::Request>,
    /// `--start-queue`: start the main queue.
    pub start_queue: bool,
    /// `--when-done WHAT`: what the window does once the downloads are
    /// done, one of [`WHEN_DONE`].
    pub when_done: Option<String>,
}

/// The words `--when-done` takes.
pub const WHEN_DONE: [&str; 5] = ["nothing", "exit", "sleep", "hibernate", "shutdown"];

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
pub const WHEN_DONE_FILE: &str = "when-done";

fn is_link(arg: &str) -> bool {
    arg.starts_with("http://")
        || arg.starts_with("https://")
        || arg.starts_with("magnet:")
        || dm_core::ftp::is_ftp(arg)
        || dm_core::torrent::local_file(arg).is_some() && dm_core::is_torrent(arg)
}

/// A `.torrent` on this computer named by its path, as a shell gives it,
/// as the `file://` address the queue takes; a file manager gives that
/// address itself.
fn local_torrent(arg: &str) -> Option<String> {
    let path = Path::new(arg);
    if arg.contains("://") || !dm_core::is_torrent(arg) || !path.is_file() {
        return None;
    }
    let full = std::path::absolute(path).ok()?;
    dm_core::torrent::file_url(&full)
}

/// Reads the command line: links, each taking the options given before it,
/// and `--start-queue`:
///
/// ```text
/// dm-desktop [--save-to FOLDER] [--name NAME] [--later] LINK … [--start-queue]
/// ```
///
/// `--save-to` and `--name` say where and as what the next link is saved,
/// `--later` adds it to the main queue without starting it, and
/// `--start-queue` starts the main queue. A bare link (a `.desktop` entry's
/// `%u`, a shell) is a download with no options. Anything else is left to
/// the flags read elsewhere (`--hidden`, `--control`) or ignored.
pub fn orders(args: &[String]) -> Orders {
    let mut orders = Orders::default();
    let mut next = host::Request::default();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "--save-to" => {
                // Made full here, against this command's own folder: the
                // dm it is handed to runs elsewhere, and a row keeps it.
                let here = std::env::current_dir().ok();
                next.save_dir = args.get(i + 1).and_then(|typed| {
                    let full = settings::full_folder(typed, here.as_deref());
                    if full.is_none() {
                        eprintln!(
                            "dm: {typed} is not a folder dm can use; saving to the usual one"
                        );
                    }
                    full.map(|p| p.display().to_string())
                });
                i += 1;
            }
            "--name" => {
                next.save_name = args.get(i + 1).cloned();
                i += 1;
            }
            "--later" => next.later = true,
            "--start-queue" => orders.start_queue = true,
            "--when-done" => {
                let word = args.get(i + 1).map(|w| w.trim().to_ascii_lowercase());
                match word {
                    Some(w) if WHEN_DONE.contains(&w.as_str()) => orders.when_done = Some(w),
                    _ => eprintln!(
                        "dm: --when-done takes one of {}; nothing changed",
                        WHEN_DONE.join(", ")
                    ),
                }
                i += 1;
            }
            _ if is_link(arg) => {
                orders.downloads.push(host::Request {
                    url: arg.clone(),
                    ..std::mem::take(&mut next)
                });
            }
            _ => {
                if let Some(url) = local_torrent(arg) {
                    orders.downloads.push(host::Request {
                        url,
                        ..std::mem::take(&mut next)
                    });
                }
            }
        }
        i += 1;
    }
    orders
}

/// Bare words on the command line that are no link, no file on this
/// computer and no option's value: a mistyped command (`dm lst`), which
/// `orders` ignores and which would otherwise just open the window.
pub fn unrecognised(args: &[String]) -> Vec<String> {
    let mut words = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "--save-to" | "--name" | "--when-done" | "--control" => i += 1,
            a if a.starts_with('-') || a.contains(':') || is_link(a) || Path::new(a).exists() => {}
            a => words.push(a.to_string()),
        }
        i += 1;
    }
    words
}

/// What a request from the command line or a browser asks the queue for.
pub fn add_options(request: &host::Request) -> dm_core::AddOptions {
    dm_core::AddOptions {
        referrer: request.referrer.clone(),
        save_dir: request.save_dir.clone(),
        save_name: request.save_name.clone(),
        later: request.later,
        quality: request.quality.clone(),
        cookies: request.cookies.clone(),
        cookies_once: request.cookies_once,
        queue: if request.later {
            dm_core::MAIN_QUEUE.to_string()
        } else {
            String::new()
        },
        ..dm_core::AddOptions::default()
    }
}

/// Gives this launch's orders to the dm already running, and asks it to
/// show itself, rather than opening a second window over a second queue
/// writing the same file.
pub fn hand_to_running(home: &Path, orders: &Orders) {
    for (n, request) in orders.downloads.iter().enumerate() {
        if let Err(e) = host::deposit(home, &format!("argv-{}-{n}", std::process::id()), request) {
            eprintln!("dm: could not hand {} to the running dm: {e}", request.url);
        }
    }
    let _ = std::fs::create_dir_all(host::inbox(home));
    if orders.start_queue {
        let _ = std::fs::write(host::inbox(home).join("start-queue"), b"");
    }
    leave_when_done(home, orders);
    let _ = std::fs::write(host::inbox(home).join("show"), b"");
}

/// `--when-done`, left in the inbox for the window to take up: the one
/// running, or the one this launch starts.
pub fn leave_when_done(home: &Path, orders: &Orders) {
    if let Some(word) = &orders.when_done {
        let _ = std::fs::create_dir_all(host::inbox(home));
        let _ = std::fs::write(host::inbox(home).join(WHEN_DONE_FILE), word);
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
    register: impl FnOnce() -> Result<Option<Vec<std::path::PathBuf>>, String>,
) -> Option<String> {
    let failed = register().err();
    hand_to_running(home, orders);
    failed
}

/// The queue the saved settings describe, with what the last run left in it
/// read back, and anything worth saying about how that went.
///
/// Built from the saved proxy rather than a plain client and then corrected:
/// the first thing a queue does on startup is resume what was in flight, and
/// that must not go out directly. A proxy that cannot be built is said out
/// loud rather than quietly bypassed, and turned off for everything — the
/// tools dm runs are told the proxy from the same settings, and a client
/// going direct beside a yt-dlp handed a proxy that does not parse is two
/// routes disagreeing about one setting.
pub struct Started {
    pub queue: Queue,
    pub settings: Settings,
    pub notices: Vec<String>,
}

pub async fn start(orders: &Orders) -> Started {
    // yt-dlp and QuickJS, fetched into dm's own folder when a video page
    // needs them; a yt-dlp fetched before is kept current.
    dm_core::tools::set_folder(settings::home().join("tools"));
    let (mut saved, unreadable) = Settings::load();
    dm_core::tools::allow_fetching(saved.fetch_tools);
    let mut notices: Vec<String> = unreadable.into_iter().collect();

    let client = match dm_core::proxy::client_with_timeout(&saved.proxy, saved.read_timeout()) {
        Ok(client) => client,
        Err(why) => {
            notices.push(format!(
                "the saved proxy could not be used ({why}), so it is turned off and downloads \
                 go out directly. Fix it under Options › Proxy and tick it again."
            ));
            saved.proxy.enabled = false;
            // Saved turned off, as the notice says: the browser's native
            // host reads the settings file, and still finding the proxy on
            // there it left every caught download with the browser while
            // the app went direct.
            if let Err(e) = saved.save() {
                notices.push(format!("the proxy could not be saved as turned off: {e}"));
            }
            dm_core::client()
        }
    };
    // A yt-dlp dm fetched itself, a week old, is fetched afresh.
    tokio::spawn(dm_core::tools::freshen(client.clone()));
    // The cookies file, read now rather than at the first change of a
    // setting: before, a download that needed it failed until then.
    notices.extend(use_cookies(&saved));

    let queue = Queue::new(
        client,
        // Part files go under dm's own directory unless a folder was chosen:
        // they are not files anyone wants to find in their downloads folder.
        saved.options(
            saved.parts_dir(),
            dm_core::Limiter::new(saved.speed_limit_bytes()),
        ),
        saved.concurrency,
    );
    // Before anything can serve the queue: applying it from a spawned task
    // raced the scheduler, and a scheduled queue ran at launch.
    queue.set_schedule(saved.schedule.clone());
    queue.set_allowance(saved.allowance());
    queue.set_recheck_every(saved.recheck_every());
    queue.set_queue_limits(saved.queue_limit_map());
    queue.set_queue_starts(saved.queue_start_list());
    watch_rules(&queue);
    // Read back whatever the last run left before anything can add a row:
    // the command line below, the inbox watcher and the window all number
    // new rows, and a row numbered before the store was read collided with
    // a restored one.
    queue.persist_to(settings::home().join("queue.json")).await;
    for request in &orders.downloads {
        // With a site's saved sign-in, as the window adds a link.
        queue
            .add_with(
                &saved.with_login(&request.url),
                saved.with_site_rule(&request.url, add_options(request)),
            )
            .await;
    }
    if orders.start_queue || saved.start_queue_on_launch {
        queue.start_queue(dm_core::MAIN_QUEUE).await;
    }

    Started {
        queue,
        settings: saved,
        notices,
    }
}

/// Whether the client must be built again for a change from `previous`.
fn new_client(settings: &Settings, previous: Option<&Settings>) -> bool {
    previous.is_none_or(|held| {
        held.proxy != settings.proxy || held.read_timeout != settings.read_timeout
    })
}

/// Why a change cannot be put into force, found before any of it is: a
/// value it refuses, or a proxy that cannot be built. `apply` after this
/// changes everything or nothing, where before a proxy refused halfway left
/// the half before it live.
pub fn check(settings: &Settings, previous: Option<&Settings>) -> Result<(), String> {
    if let Some(why) = previous.and_then(|held| settings.refusal(held)) {
        return Err(why);
    }
    if new_client(settings, previous) {
        dm_core::proxy::client_with_timeout(&settings.proxy, settings.read_timeout())?;
    }
    Ok(())
}

/// Puts the cookies file in force: read again each time, so one exported
/// again since is in force without choosing it again. One that can no
/// longer be read is a problem to show, not a reason to refuse the change
/// it came with — a cookies file moved away used to fail every change of
/// every setting after it, the language and a saved search's last run
/// among them. Said once, until it reads again or another problem replaces
/// it, since this runs on every change.
pub fn use_cookies(settings: &Settings) -> Option<String> {
    static SAID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let outcome = dm_core::cookies::use_file(Path::new(settings.cookies_file.trim()));
    let mut said = dm_core::lock(&SAID);
    match outcome {
        Ok(_) => {
            *said = None;
            None
        }
        Err(why) if said.as_deref() == Some(why.as_str()) => None,
        Err(why) => {
            *said = Some(why.clone());
            Some(format!(
                "{why}; downloads go without its cookies until it is back or another is \
                 chosen in Options › Network"
            ))
        }
    }
}

/// Puts settings into effect on a running queue — everything but the login
/// item, which belongs to whichever window registers it. What can fail is
/// tried first (`check`), so a change is in force whole or not at all; the
/// problems it found that do not stop it come back to be shown.
///
/// The client is only rebuilt when the proxy actually changed. A new client
/// means a new TLS backend, a fresh root store, and every keep-alive
/// connection dropped — for ticking a box about folders.
pub async fn apply(
    settings: &Settings,
    previous: Option<&Settings>,
    queue: &Queue,
) -> Result<Vec<String>, String> {
    check(settings, previous)?;
    if new_client(settings, previous) {
        queue
            .set_client(&settings.proxy, settings.read_timeout())
            .await?;
    }
    let problems: Vec<String> = use_cookies(settings).into_iter().collect();

    let current = queue.options().await;
    // The limiter belongs to the running queue. The parts folder is the
    // settings' now: a row records the folder it began in, so a change
    // here reaches rows not yet started and strands nothing.
    queue
        .set_options(settings.options(settings.parts_dir(), current.limiter))
        .await;

    queue.set_concurrency(settings.concurrency);
    queue.set_speed_limit(settings.speed_limit_bytes()).await;
    queue.set_schedule(settings.schedule.clone());
    queue.set_allowance(settings.allowance());
    queue.set_recheck_every(settings.recheck_every());
    queue.set_queue_limits(settings.queue_limit_map());
    queue.set_queue_starts(settings.queue_start_list());
    dm_core::tools::allow_fetching(settings.fetch_tools);
    Ok(problems)
}

/// Starts the scheduler again when the schedule's window opens or the
/// download limit's period rolls over, for the rows they held.
///
/// The scheduler runs only while something is queued or running, and a held
/// row is neither: without this, a window opening at one in the morning
/// started nothing until a person touched the queue.
pub fn watch_rules(queue: &Queue) {
    let queue = queue.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if queue.may_run() && queue.held().await {
                queue.run().await;
            }
            // The recheck queue's own timer, where one is set.
            if queue.recheck_if_due().await.is_some() {
                queue.run().await;
            }
            // Queues that start on their own at a time of day.
            let (day, minute) = dm_core::schedule::now_local();
            for name in queue.starts_due(day, minute) {
                if queue.start_queue(&name).await > 0 {
                    wake(&queue);
                }
            }
        }
    });
}

/// Starts the scheduler, if it is not already running.
///
/// It runs until the queue empties, so a new arrival while it is idle needs
/// it started again. Starting a second one is harmless: the first returns
/// as soon as it finds nothing to do. Must be called inside the runtime.
pub fn wake(queue: &Queue) {
    let queue = queue.clone();
    tokio::spawn(async move { queue.run().await });
}

/// Why `url` cannot be added, where that can be told before fetching: a
/// web or FTP address with no site in it (`http://`) was accepted and only
/// failed later with "builder error: empty host".
///
/// Read from the text as typed, not only as parsed: the parser turns
/// `http:///x` into a site called `x`, and that row failed sixteen seconds
/// later with a name lookup error. A magnet with no torrent hash in it is
/// refused here too, rather than failing once the engine has it.
pub fn unusable(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("magnet:") {
        return (!magnet_names_a_torrent(url))
            .then(|| format!("{url} names no torrent to download (no valid xt=urn:btih: hash)"));
    }
    let rest = ["http://", "https://", "ftp://", "ftps://"]
        .iter()
        .find_map(|scheme| lower.strip_prefix(scheme))?;
    // The authority as typed: up to the first `/`, `?` or `#`, less any
    // `user:password@`.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let site = authority.rsplit('@').next().unwrap_or("");
    if site.is_empty() || site.starts_with(':') {
        return Some(format!("{url} names no site to download from"));
    }
    match url::Url::parse(url) {
        Ok(parsed) if parsed.host_str().is_some_and(|h| !h.is_empty()) => None,
        Ok(_) | Err(url::ParseError::EmptyHost) => {
            Some(format!("{url} names no site to download from"))
        }
        Err(e) => Some(format!("{url} is not a link dm can read ({e})")),
    }
}

/// Whether a magnet carries a torrent's hash: an `xt` of `urn:btih:` with
/// 40 hex digits or 32 base32 letters, or a v2 `urn:btmh:` multihash.
fn magnet_names_a_torrent(url: &str) -> bool {
    let query = url.split_once('?').map_or("", |(_, q)| q);
    url::form_urlencoded::parse(query.as_bytes())
        .filter(|(key, _)| key == "xt" || key.starts_with("xt."))
        .any(|(_, topic)| {
            let topic = topic.to_ascii_lowercase();
            if let Some(hash) = topic.strip_prefix("urn:btih:") {
                (hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()))
                    || (hash.len() == 32
                        && hash.chars().all(|c| matches!(c, 'a'..='z' | '2'..='7')))
            } else if let Some(hash) = topic.strip_prefix("urn:btmh:") {
                // sha2-256 multihash: `1220` and 64 hex digits.
                hash.len() == 68
                    && hash.starts_with("1220")
                    && hash.chars().all(|c| c.is_ascii_hexdigit())
            } else {
                false
            }
        })
}

/// Queues a URL, or several pasted together.
///
/// Whitespace-separated rather than one at a time: pasting a list is the
/// normal way people add more than one. What counts as a URL is the queue's
/// to decide, so the window and the browser hand-off cannot disagree.
///
/// A link in the list that cannot be used is skipped, not the whole paste:
/// the rest are added, and what was skipped comes back with the reason.
pub async fn add(queue: &Queue, text: &str) -> Result<Added, String> {
    let mut skipped = Vec::new();
    let urls: Vec<&str> = text
        .split_whitespace()
        .filter(|u| match unusable(u) {
            Some(why) => {
                skipped.push(why);
                false
            }
            None => true,
        })
        .collect();
    let ids = if urls.is_empty() {
        Vec::new()
    } else {
        queue.add_all(&urls).await
    };
    if ids.len() < urls.len() {
        skipped.push(match urls.len() - ids.len() {
            1 => "one of the links is not one dm can download".to_string(),
            n => format!("{n} of the links are not ones dm can download"),
        });
    }
    if ids.is_empty() {
        return Err(skipped
            .into_iter()
            .next()
            .unwrap_or_else(|| "that is not a URL dm can download".into()));
    }
    wake(queue);
    Ok(Added { ids, skipped })
}

/// What `add` added, and why any link was skipped.
#[derive(Debug, Default)]
pub struct Added {
    pub ids: Vec<Id>,
    pub skipped: Vec<String>,
}

/// A digest typed against a row: empty clears it, which is how one pasted by
/// mistake is undone.
pub fn checksum(text: &str) -> Result<Option<Checksum>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    Checksum::parse(text)
        .map(Some)
        .ok_or_else(|| "that is not a checksum dm recognises".into())
}

/// Opens the finished file's folder, with the file selected where the OS can.
///
/// A finished download the user cannot get to is a download half-delivered;
/// this is the "show in folder" every file manager has.
pub fn reveal_in_folder(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("that file is no longer where dm saved it".into());
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(p);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        // Passed as it is, quoted by hand: Explorer reads `/select,` and
        // the path as one argument, and Rust's own quoting of a path with
        // spaces wraps the whole of it, which Explorer does not undo.
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("explorer");
        c.raw_arg(format!("/select,\"{path}\""));
        c
    };
    // No portable "select the file", so the containing folder is opened.
    #[cfg(all(unix, not(target_os = "macos")))]
    return linux_open(p.parent().unwrap_or(p));
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// The programs a Linux desktop opens things with, in the order tried:
/// the freedesktop one, then GNOME's, KDE's and Xfce's own, which a
/// minimal install can have without `xdg-utils`.
#[cfg(all(unix, not(target_os = "macos")))]
const LINUX_OPENERS: [&[&str]; 5] = [
    &["xdg-open"],
    &["gio", "open"],
    &["kde-open"],
    &["kde-open5"],
    &["exo-open"],
];

/// Opens a file or folder on Linux with the first opener that is there.
///
/// `xdg-open` comes with `xdg-utils`, which the packages now require, but
/// a desktop that lacks it (a minimal install, a container) still has its
/// own. Started from an AppImage, the AppImage's private library path is
/// not passed on: an opener loading its libraries from there fails in ways
/// nobody would trace to dm.
#[cfg(all(unix, not(target_os = "macos")))]
fn linux_open(target: &Path) -> Result<(), String> {
    linux_open_in(target, None)
}

/// How long an opener is given to fail before it is taken to be opening.
///
/// `xdg-open` with nothing to hand the file to ("no method available")
/// exits at once with 3 or 4; one that found a program has handed over or
/// is running it within a few hundred milliseconds. Waited for in place,
/// since the answer is the button's: only an opener still running after
/// this holds the window this long.
#[cfg(all(unix, not(target_os = "macos")))]
const OPENER_VERDICT: std::time::Duration = std::time::Duration::from_millis(1500);

/// The same, looking for the openers along `path` where one is given.
///
/// Each opener's exit is waited for (`OPENER_VERDICT`): `xdg-open` that
/// starts and then fails was taken as success, and the file simply did not
/// open. A failing opener passes to the next; when every one there failed,
/// what each said is the error.
#[cfg(all(unix, not(target_os = "macos")))]
fn linux_open_in(target: &Path, path: Option<&std::ffi::OsStr>) -> Result<(), String> {
    let in_appimage = std::env::var_os("APPIMAGE").is_some();
    let mut failures: Vec<String> = Vec::new();
    for opener in LINUX_OPENERS {
        let Some((program, args)) = opener.split_first() else {
            continue;
        };
        let mut command = std::process::Command::new(program);
        command.args(args.iter()).arg(target);
        if let Some(path) = path {
            command.env("PATH", path);
        }
        if in_appimage {
            command
                .env_remove("LD_LIBRARY_PATH")
                .env_remove("LD_PRELOAD");
        }
        // Its error goes to a file, not a pipe: an opener that runs the
        // program in the foreground passes its stderr on, and a pipe whose
        // reader has gone would kill that program at its first warning.
        let said = OpenerSays::new();
        command.stdin(std::process::Stdio::null());
        if let Some(file) = said.as_ref().and_then(|s| s.stdio()) {
            command.stderr(file);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        let deadline = std::time::Instant::now() + OPENER_VERDICT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                // Still running: it found something to open the file with.
                // Reaped when it ends, so it leaves no zombie behind.
                Ok(None) | Err(_) => {
                    std::thread::spawn(move || child.wait());
                    break None;
                }
            }
        };
        match status {
            None => return Ok(()),
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let words = said.map(|s| s.read()).unwrap_or_default();
                let name = opener.join(" ");
                failures.push(if words.is_empty() {
                    match status.code() {
                        Some(code) => format!("{name} failed (exit {code})"),
                        None => format!("{name} failed"),
                    }
                } else {
                    format!("{name}: {words}")
                });
            }
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "this computer could not open {}: {}",
            target.display(),
            failures.join("; ")
        ));
    }
    Err("nothing on this computer opens files: install xdg-utils \
         (`sudo apt install xdg-utils`, `sudo dnf install xdg-utils` or \
         `sudo pacman -S xdg-utils`)"
        .into())
}

/// A file an opener's stderr goes to, removed when read or dropped.
#[cfg(all(unix, not(target_os = "macos")))]
struct OpenerSays(PathBuf);

#[cfg(all(unix, not(target_os = "macos")))]
impl OpenerSays {
    fn new() -> Option<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "dm-opener-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::File::create(&path).ok()?;
        Some(Self(path))
    }

    fn stdio(&self) -> Option<std::process::Stdio> {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&self.0)
            .ok()
            .map(std::process::Stdio::from)
    }

    /// Its last few lines, which is where an opener says why.
    fn read(self) -> String {
        let text = std::fs::read_to_string(&self.0).unwrap_or_default();
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let tail = lines.len().saturating_sub(3);
        lines.get(tail..).unwrap_or_default().join(" ")
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl Drop for OpenerSays {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Opens a finished file with whatever the system opens it with.
pub fn open_file(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    // An address — Help's — is opened in the browser; a path must exist.
    let is_link = path.starts_with("https://") || path.starts_with("http://");
    if !is_link && !p.exists() {
        return Err("that file is no longer where dm saved it".into());
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    // Explorer opens a file with its program, and an address in the
    // browser. Not `cmd /C start`: cmd reads `&`, `^` and `%` in a name a
    // server chose, so a file called `a&calc` ran calc.
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    // Quoted by hand, as for `/select`: Explorer splits its command line on
    // commas, and Rust quotes only a path with a space in it.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(format!("\"{path}\""));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    return linux_open(p);
    #[cfg(target_os = "macos")]
    command.arg(p);
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// The program and arguments that open `path` with `app`: on macOS through
/// `open -a`, which takes an application bundle; elsewhere the program
/// itself, given the file.
pub fn open_with_command(
    path: &Path,
    app: &Path,
    macos: bool,
) -> (PathBuf, Vec<std::ffi::OsString>) {
    if macos {
        (
            PathBuf::from("open"),
            vec!["-a".into(), app.as_os_str().into(), path.as_os_str().into()],
        )
    } else {
        (app.to_path_buf(), vec![path.as_os_str().into()])
    }
}

/// Opens a finished file with the application the person chose: "Open with".
pub fn open_with(path: &str, app: &Path) -> Result<(), String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("that file is no longer where dm saved it".into());
    }
    if !app.exists() {
        return Err(format!("{} is not there", app.display()));
    }
    let (program, args) = open_with_command(p, app, cfg!(target_os = "macos"));
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{} could not be started: {e}", app.display()))
}

/// The inbox file that asks a running dm to end.
pub const QUIT: &str = "quit";

/// Asks a running dm to end, through its inbox; nothing when none runs, so
/// that a later launch does not read the request and end at once.
pub fn ask_running_to_quit(home: &Path) -> std::io::Result<bool> {
    if !crate::instance::running(home)? {
        return Ok(false);
    }
    let dir = host::inbox(home);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(QUIT), b"")?;
    Ok(true)
}

/// Takes downloads the browser left for us, and a second launch's request
/// to be shown, for as long as the app runs.
///
/// Polled rather than watched: a filesystem notifier is another dependency
/// and another set of platform differences, for a directory that sees a
/// handful of files a day.
pub async fn watch_inbox(
    queue: Queue,
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
    loop {
        // `--unregister`, uninstalling dm: this dm ends, as a stop signal
        // ends it, rather than stay in the tray and write back what the
        // uninstall removed.
        if std::fs::remove_file(dir.join(QUIT)).is_ok() {
            quit();
        }
        // Left by a second launch instead of opening a second window, and
        // by the browser's toolbar button.
        if std::fs::remove_file(dir.join("show")).is_ok() {
            show();
        }
        // A second launch with `--start-queue`.
        if std::fs::remove_file(dir.join("start-queue")).is_ok() {
            queue.start_queue(dm_core::MAIN_QUEUE).await;
        }

        let waiting: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        for path in waiting {
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
                eprintln!(
                    "dm: a hand-off in {} could not be consumed: {e}",
                    path.display()
                );
                continue;
            }

            match serde_json::from_str::<host::Request>(&text) {
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
                    eprintln!(
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
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// The words of a command line, as a shell would split them: spaces apart,
/// except inside single or double quotes, and a backslash escaping the next
/// character outside single quotes.
pub fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => word.push(c),
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
                started = true;
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// Runs `command` on a finished file, its path added as the last argument
/// rather than pasted into the line, so no file name can become a command.
/// Started and left running; an error says why it could not start.
pub fn run_after(command: &str, file: &Path) -> Result<(), String> {
    let mut parts = words(command);
    if parts.is_empty() {
        return Ok(());
    }
    let program = parts.remove(0);
    std::process::Command::new(&program)
        .args(parts)
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            format!(
                "\"{program}\", to run after {}, could not start: {e}",
                file.display()
            )
        })
}

#[cfg(test)]
mod tests;
