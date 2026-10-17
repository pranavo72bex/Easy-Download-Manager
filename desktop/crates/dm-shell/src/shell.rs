//! What the app does around its window: how it starts, how it takes a
//! browser's hand-off, how a settings change is put into force.
//!
//! Each of these was a function in the Tauri shell's `main.rs`, and each is
//! as true of any other window. Moved here so the egui window and the Tauri
//! one cannot disagree about any of it.

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
/// - **Chrome** passes the calling extension's origin, `chrome-extension://…`.
/// - **Firefox** passes the path of the app manifest and nothing else — no
///   extension id, whatever the shape of the Chrome case suggests.
///
/// Detecting both is what lets one binary be both the window and the host,
/// with no shim script in between to install, sign and keep in step.
pub fn browser_launch(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg != "--desktop"
            && (arg == "--native-host"
            || arg.starts_with("chrome-extension://")
            || arg.starts_with("moz-extension://")
            // Firefox, which says nothing else about itself.
            || arg.ends_with(HOST_MANIFEST))
    })
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
}

fn is_link(arg: &str) -> bool {
    arg.starts_with("http://")
        || arg.starts_with("https://")
        || arg.starts_with("magnet:")
        || dm_core::ftp::is_ftp(arg)
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
            _ if is_link(arg) => {
                orders.downloads.push(host::Request {
                    url: arg.clone(),
                    ..std::mem::take(&mut next)
                });
            }
            _ => {}
        }
        i += 1;
    }
    orders
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
    let _ = std::fs::write(host::inbox(home).join("show"), b"");
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
    let (mut saved, unreadable) = Settings::load();
    let mut notices: Vec<String> = unreadable.into_iter().collect();

    let client = match dm_core::proxy::client_with_timeout(&saved.proxy, saved.read_timeout()) {
        Ok(client) => client,
        Err(why) => {
            notices.push(format!(
                "the saved proxy could not be used ({why}), so it is turned off and downloads \
                 go out directly. Fix it under Options › Proxy and tick it again."
            ));
            saved.proxy.enabled = false;
            dm_core::client()
        }
    };
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
    watch_rules(&queue);
    // Read back whatever the last run left before anything can add a row:
    // the command line below, the inbox watcher and the window all number
    // new rows, and a row numbered before the store was read collided with
    // a restored one.
    queue.persist_to(settings::home().join("queue.json")).await;
    for request in &orders.downloads {
        // With a site's saved sign-in, as the window adds a link.
        queue
            .add_with(&saved.with_login(&request.url), add_options(request))
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
pub fn unusable(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if !["http://", "https://", "ftp://", "ftps://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return None;
    }
    match url::Url::parse(url) {
        Ok(parsed) if parsed.host_str().is_some_and(|h| !h.is_empty()) => None,
        Ok(_) | Err(url::ParseError::EmptyHost) => {
            Some(format!("{url} names no site to download from"))
        }
        Err(e) => Some(format!("{url} is not a link dm can read ({e})")),
    }
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
        skipped.push(format!(
            "{} of the links are not ones dm can download",
            urls.len() - ids.len()
        ));
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
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(p.parent().unwrap_or(p));
        c
    };
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
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
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(not(target_os = "windows"))]
    command.arg(p);
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
) {
    let dir = host::inbox(&settings::home());
    loop {
        // A second dm, launched by hand while this one was in the tray,
        // leaves this instead of opening a second window.
        if tokio::fs::remove_file(dir.join("show")).await.is_ok() {
            show();
        }
        // A second launch with `--start-queue`.
        if tokio::fs::remove_file(dir.join("start-queue"))
            .await
            .is_ok()
        {
            queue.start_queue(dm_core::MAIN_QUEUE).await;
        }

        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(text) = tokio::fs::read_to_string(&path).await else {
                    continue;
                };
                // Removed before it is acted on, so a request that upsets the
                // app is not retried on every restart for ever. One that
                // cannot be removed is left alone rather than queued again
                // twice a second for the life of the process.
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    eprintln!(
                        "dm: a hand-off in {} could not be consumed: {e}",
                        path.display()
                    );
                    continue;
                }

                match serde_json::from_str::<host::Request>(&text) {
                    // add_from, not add: the engine needs the page as Referer
                    // or a hotlink-protected file 403s in the app where the
                    // browser's own download would have worked.
                    // A video from the browser's panel: the window asks first.
                    Ok(request) if request.ask => ask(request),
                    Ok(request) => {
                        let url = host::saved(&settings::home()).with_login(&request.url);
                        queue.add_with(&url, add_options(&request)).await;
                    }
                    // The browser was told this started. It is kept, under a
                    // name the watcher skips, rather than vanishing.
                    Err(e) => {
                        let kept = path.with_extension("unreadable");
                        let _ = tokio::fs::write(&kept, &text).await;
                        eprintln!(
                            "dm: a hand-off could not be read ({e}); kept as {}",
                            kept.display()
                        );
                    }
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
mod tests {

    #[test]
    fn a_relative_save_folder_is_the_callers_and_never_kept_relative() {
        let args: Vec<String> = ["--save-to", "myout", "https://files.example/a.zip"]
            .map(String::from)
            .to_vec();
        let orders = super::orders(&args);
        let here = std::env::current_dir().unwrap();
        assert_eq!(
            orders.downloads[0].save_dir.as_deref(),
            Some(here.join("myout").display().to_string().as_str())
        );
    }

    #[tokio::test]
    async fn a_pasted_list_skips_the_link_it_cannot_use_and_adds_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::settings::Settings {
            download_dir: dir.path().join("out"),
            ..Default::default()
        };
        let queue = dm_core::Queue::new(
            dm_core::client(),
            settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
            1,
        );
        let added = super::add(
            &queue,
            "http:// http://127.0.0.1:9/a.bin http://127.0.0.1:9/b.bin",
        )
        .await
        .expect("one bad link refused the whole paste");
        assert_eq!(added.ids.len(), 2);
        assert_eq!(added.skipped.len(), 1, "{:?}", added.skipped);
        assert!(added.skipped[0].contains("names no site"));
        assert!(super::add(&queue, "http://").await.is_err());
    }

    #[test]
    fn an_address_with_no_site_is_refused_with_a_plain_reason() {
        assert!(super::unusable("http://")
            .unwrap()
            .contains("names no site"));
        assert_eq!(super::unusable("https://files.example/a.zip"), None);
        assert_eq!(super::unusable("magnet:?xt=urn:btih:abc"), None);
    }

    #[test]
    fn a_command_line_is_split_as_a_shell_splits_it() {
        assert_eq!(super::words("clamscan --quiet"), ["clamscan", "--quiet"]);
        assert_eq!(
            super::words(r#""/Applications/My Scanner.app/x" -a 'b c' d\ e"#),
            ["/Applications/My Scanner.app/x", "-a", "b c", "d e"]
        );
        assert!(super::words("   ").is_empty());
        assert_eq!(super::words(r#"x """#), ["x", ""]);
    }

    #[test]
    fn open_with_names_the_bundle_on_macos_and_runs_the_program_elsewhere() {
        let file = Path::new("/d/file.zip");
        let (program, args) =
            open_with_command(file, Path::new("/Applications/Unarchiver.app"), true);
        assert_eq!(program, PathBuf::from("open"));
        assert_eq!(args, ["-a", "/Applications/Unarchiver.app", "/d/file.zip"]);
        let (program, args) = open_with_command(file, Path::new("/usr/bin/file-roller"), false);
        assert_eq!(program, PathBuf::from("/usr/bin/file-roller"));
        assert_eq!(args, ["/d/file.zip"]);
        assert!(open_with("/no/such/file", Path::new("/bin/sh")).is_err());
    }

    use super::*;

    /* One binary is both the window and the host, and which one it is comes
     * down to reading the arguments a browser passes. Firefox passes the
     * manifest path and nothing else; getting that wrong opened a window,
     * wrote nothing to stdout, and left the browser holding a port that
     * never answered — with no error anywhere. */
    #[test]
    fn each_browser_is_recognised_by_what_it_actually_passes() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // Firefox: the path of the app manifest, and that is all.
        assert!(browser_launch(&args(&[
            "/Users/x/Library/Application Support/Mozilla/NativeMessagingHosts/com.dm.engine.json"
        ])));
        // Linux puts it somewhere else entirely.
        assert!(browser_launch(&args(&[
            "/home/x/.mozilla/native-messaging-hosts/com.dm.engine.json"
        ])));
        // Chrome: the calling extension's origin.
        assert!(browser_launch(&args(&[
            "chrome-extension://abcdefghijklmnop/",
            "--parent-window=0"
        ])));

        // And a person, who passes nothing, or opens a file.
        assert!(!browser_launch(&args(&[])));
        assert!(!browser_launch(&args(&["--help"])));
        assert!(!browser_launch(&args(&["--desktop"])));
        assert!(!browser_launch(&args(&["/Users/x/notes.json"])));
    }

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_links_are_taken_from_the_command_line() {
        let args = argv(&["--hidden", "https://e.com/a", "notes.txt", "magnet:?xt=x"]);
        let urls: Vec<String> = orders(&args).downloads.into_iter().map(|r| r.url).collect();
        assert_eq!(urls, ["https://e.com/a", "magnet:?xt=x"]);
        assert!(launched_at_login(&args));
    }

    #[test]
    fn options_apply_to_the_link_after_them() {
        let got = orders(&argv(&[
            "--save-to",
            "/tmp/in",
            "--name",
            "b.zip",
            "--later",
            "https://e.com/a.zip",
            "https://e.com/c.iso",
            "--start-queue",
            "/data",
        ]));
        assert_eq!(got.downloads.len(), 2);
        let first = &got.downloads[0];
        assert_eq!(first.url, "https://e.com/a.zip");
        assert_eq!(first.save_dir.as_deref(), Some("/tmp/in"));
        assert_eq!(first.save_name.as_deref(), Some("b.zip"));
        assert!(first.later);
        let second = &got.downloads[1];
        assert_eq!(
            (second.save_dir.as_deref(), second.later),
            (None, false),
            "not carried over"
        );
        assert!(got.start_queue);
    }

    #[test]
    fn options_with_no_link_after_them_do_nothing() {
        assert_eq!(
            orders(&argv(&["--name", "x", "--later"])),
            Orders::default()
        );
        assert_eq!(
            orders(&argv(&["--save-to"])),
            Orders::default(),
            "a flag at the end"
        );
        assert_eq!(orders(&argv(&["notes.txt"])), Orders::default());
    }

    #[test]
    fn an_empty_checksum_clears_and_nonsense_is_refused() {
        assert_eq!(checksum("  ").unwrap(), None);
        assert!(checksum("not a digest").is_err());
        assert!(checksum(&"a".repeat(64)).unwrap().is_some());
    }
}
