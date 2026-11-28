// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The command line: how a browser's launch is told from a person's, and the downloads and orders a
//! launch gives.

use super::*;

/// The name of the native messaging manifest, which is also how Firefox
/// identifies itself when it launches this binary as a host.
pub const HOST_MANIFEST: &str = "com.edm.engine.json";

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
/// in it: a link ending in `com.edm.engine.json`, a `--name` of that, or a
/// `chrome-extension://` link passed by a person turned their launch into
/// a host that read nothing from stdin and dropped the link.
pub fn browser_launch(args: &[String]) -> bool {
    match args {
        // EDM's own way in, for scripts and the installers' checks.
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

/// Whether the browser that started EDM's host did so for an extension EDM
/// knows: the ids built into this version, and any the manifests EDM wrote
/// allow (`register::allowed_ids`). The browser itself lets only the
/// extensions its manifest names start the host, so this refuses only what
/// never went through a browser: a program on this computer starting EDM
/// with an origin of its own and speaking the extension's protocol to it.
/// Firefox passes its manifest's path, with the extension's id after it in
/// some versions and nothing in others; a `moz-extension://` origin is a
/// profile's own name for an extension, not its id, and is not checked.
pub fn launcher_known(args: &[String]) -> bool {
    launcher_known_in(args, &edm_core::folders::home().unwrap_or_default())
}

/// [`launcher_known`], with the manifests under `home`.
pub fn launcher_known_in(args: &[String], home: &Path) -> bool {
    let id = match args {
        [origin, ..] if origin.starts_with("chrome-extension://") => origin
            .trim_start_matches("chrome-extension://")
            .trim_end_matches('/'),
        [manifest, id] if is_host_manifest(manifest) => id.as_str(),
        _ => return true,
    };
    crate::register::allowed_ids(home)
        .iter()
        .any(|known| known == id)
}

/// A path to EDM's own app manifest, as Firefox gives it: a file path, not
/// a link that happens to end the same way.
fn is_host_manifest(arg: &str) -> bool {
    let path = Path::new(arg);
    !arg.contains("://")
        && path.has_root()
        && path.file_name().is_some_and(|name| name == HOST_MANIFEST)
}

/// This process's arguments, less its own name, as a protocol launch is taken
/// ([`one_link_launch`]).
pub fn args() -> Vec<String> {
    one_link_launch(std::env::args().skip(1).collect())
}

/// A launch whose first word is a `magnet:` link is the system handing EDM that
/// one link (the `magnet:` handler is registered as `"<EDM>" "%1"`): every word
/// after it is part of the link, never an option. On Windows a link holding a
/// `"` and spaces, passed on unescaped by the program that opened it, is split
/// into words that EDM would read as `--quiet`, `--save-to FOLDER` and another
/// link, so a file of a page's choosing would be saved into a folder of its
/// choosing with nothing asked (security audit).
pub fn one_link_launch(args: Vec<String>) -> Vec<String> {
    match args.first() {
        Some(first)
            if first
                .get(..7)
                .is_some_and(|s| s.eq_ignore_ascii_case("magnet:")) =>
        {
            vec![args.join(" ")]
        }
        _ => args,
    }
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
    /// The addresses given, each as a request the inbox would hold.
    pub downloads: Vec<host::Request>,
    /// `--start-queue`: start the main queue.
    pub start_queue: bool,
    /// `--when-done WHAT`: what the window does once the downloads are
    /// done, one of [`WHEN_DONE`].
    pub when_done: Option<String>,
    /// `--new-download`: the window opens Add URL (the jump list's "New
    /// download").
    pub new_download: bool,
}

/// The words `--when-done` takes.
pub const WHEN_DONE: [&str; 5] = ["nothing", "exit", "sleep", "hibernate", "shutdown"];

fn is_link(arg: &str) -> bool {
    arg.starts_with("http://")
        || arg.starts_with("https://")
        || arg.starts_with("magnet:")
        || edm_core::ftp::is_ftp(arg)
        || edm_core::local_file(arg).is_some() && edm_core::is_torrent(arg)
}

/// A `.torrent` on this computer named by its path, as a shell gives it,
/// as the `file://` address the queue takes; a file manager gives that
/// address itself.
fn local_torrent(arg: &str) -> Option<String> {
    let path = Path::new(arg);
    if arg.contains("://") || !edm_core::is_torrent(arg) || !path.is_file() {
        return None;
    }
    let full = std::path::absolute(path).ok()?;
    edm_core::file_url(&full)
}

/// Reads the command line: links, each taking the options given before it,
/// and `--start-queue`:
///
/// ```text
/// EDM [--quiet] [--save-to FOLDER] [--name NAME] [--later | --queue NAME] LINK … [--start-queue]
/// ```
///
/// `--save-to` and `--name` say where and as what the next link is saved,
/// `--later` adds it to the main queue without starting it, `--queue NAME` to the queue of
/// that name (made where there is none) without starting it, `--quiet` has
/// every link after it added with nothing asked (no Download details), and
/// `--start-queue` starts the main queue. A bare link (a `.desktop` entry's
/// `%u`, a shell) is a download with no options. Anything else is left to
/// the flags read elsewhere (`--hidden`, `--control`) or ignored.
///
/// Reference A's switches, which a person's scripts already use, are read too (gaps row C7):
/// `/d LINK` a download, `/p FOLDER` and `/f NAME` where and as what (said before `/d` or after
/// it, for that download), `/n` nothing asked, `/a` added for later, `/s` the main queue
/// started. Only these words exactly: `/data` is a path.
pub fn orders(args: &[String]) -> Orders {
    let mut orders = Orders::default();
    let mut next = host::Request::default();
    let mut quiet = false;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        next.quiet = quiet;
        match arg.as_str() {
            // Reference A's `/d LINK`, with what `/p`, `/f`, `/n` and `/a` said around it.
            "/d" => {
                if let Some(link) = args.get(i + 1).filter(|l| is_link(l)) {
                    let (dir, name, quiet_too, later_too, until) = switches_after(args, i + 2);
                    let mut request = std::mem::take(&mut next);
                    request.url = link.clone();
                    request.save_dir = full_folder(dir.as_deref()).or(request.save_dir);
                    request.save_name = name.or(request.save_name);
                    request.quiet = request.quiet || quiet_too;
                    request.later = request.later || later_too;
                    orders.downloads.push(request);
                    // What was said for it is not said again for the next.
                    i = until;
                    continue;
                }
                i += 1;
            }
            "/p" => {
                next.save_dir = full_folder(args.get(i + 1).map(String::as_str));
                i += 1;
            }
            "/f" => {
                next.save_name = args.get(i + 1).cloned();
                i += 1;
            }
            "/n" => quiet = true,
            "/a" => next.later = true,
            "/s" => orders.start_queue = true,
            "--save-to" => {
                // Made full here, against this command's own folder: the
                // EDM it is handed to runs elsewhere, and a row keeps it.
                let here = std::env::current_dir().ok();
                next.save_dir = args.get(i + 1).and_then(|typed| {
                    let full = settings::full_folder(typed, here.as_deref());
                    if full.is_none() {
                        eprintln!(
                            "EDM: {typed} is not a folder EDM can use; saving to the usual one"
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
            // The next link into the queue of that name, not started (gaps row C7a).
            "--queue" => {
                next.queue = args
                    .get(i + 1)
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty());
                i += 1;
            }
            // Every link after it: no dialog, whatever Options says.
            "--quiet" => quiet = true,
            "--start-queue" => orders.start_queue = true,
            "--new-download" => orders.new_download = true,
            "--when-done" => {
                let word = args.get(i + 1).map(|w| w.trim().to_ascii_lowercase());
                match word {
                    Some(w) if WHEN_DONE.contains(&w.as_str()) => orders.when_done = Some(w),
                    _ => eprintln!(
                        "EDM: --when-done takes one of {}; nothing changed",
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

/// What reference A's `/p`, `/f`, `/n` and `/a` say after its `/d LINK`, from `from` to the next
/// `/d` or `/s`: a folder, a name, nothing asked, added for later; and where that ends.
type After = (Option<String>, Option<String>, bool, bool, usize);
fn switches_after(args: &[String], from: usize) -> After {
    let (mut dir, mut name, mut quiet, mut later) = (None, None, false, false);
    let mut i = from;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "/d" | "/s" => break,
            "/p" => {
                dir = args.get(i + 1).cloned();
                i += 1;
            }
            "/f" => {
                name = args.get(i + 1).cloned();
                i += 1;
            }
            "/n" => quiet = true,
            "/a" => later = true,
            _ => {}
        }
        i += 1;
    }
    (dir, name, quiet, later, i)
}

/// `typed` as a full folder, against this command's own folder, or none (said) where EDM cannot
/// use it.
fn full_folder(typed: Option<&str>) -> Option<String> {
    let typed = typed?;
    let here = std::env::current_dir().ok();
    let full = settings::full_folder(typed, here.as_deref());
    if full.is_none() {
        eprintln!("EDM: {typed} is not a folder EDM can use; saving to the usual one");
    }
    full.map(|p| p.display().to_string())
}

/// Bare words on the command line that are no link, no file on this
/// computer and no option's value: a mistyped command (`EDM lst`), which
/// `orders` ignores and which would otherwise just open the window.
pub fn unrecognised(args: &[String]) -> Vec<String> {
    let mut words = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "--save-to" | "--name" | "--queue" | "--when-done" | "--control" | "/d" | "/p"
            | "/f" => i += 1,
            a if a.starts_with('-') || a.contains(':') || is_link(a) || Path::new(a).exists() => {}
            a => words.push(a.to_string()),
        }
        i += 1;
    }
    words
}
