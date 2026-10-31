//! Downloading from sites that serve video through their own player.
//!
//! dm fetches files and HLS itself; what it cannot do is turn a page like a
//! video site's watch URL into a file, because the address of the media is
//! not in the page — it is minted by the site's own player at play time.
//! `yt-dlp` is the tool that knows how, for a thousand sites, and it is the
//! one dm hands those to. dm does not reimplement any of that: it runs
//! yt-dlp — the one the user installed, or the official build dm fetches
//! (`tools`) — the way a person would at a shell, and places what it
//! produces like any other download.
//!
//! So this is deliberately a thin wrapper around a child process — no site
//! logic and no token minting of dm's own — and it is used only when the
//! URL turned out to be a web page rather than a file. The one site dm
//! reads itself is YouTube (`crate::youtube`, since 2026-10-30): the same
//! few requests yt-dlp makes, and the public-domain solver scripts yt-dlp
//! itself runs for the player's `n` values; yt-dlp stays behind it.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::download::{Finished, Options, Update};
use crate::stream::Quality;
use crate::{category, Error};

mod choices;
mod fetch;
mod page_data;
mod playlist;
mod process;
mod readings;
mod refusal;
mod sandbox;

pub use choices::{choices, list_choices, list_choices_fast, read_choices, with_mp3, Choice};
pub use fetch::download_page;
pub use playlist::{playlist_of, read_playlist};
pub use process::end_all;
pub use readings::{keep_readings_in, kept_choices, mark_reading, warm_reading};
/// Whether `url` is a YouTube page whose own data lists its qualities
/// (`list_choices_fast`).
pub fn is_watch_page(url: &str) -> bool {
    page_data::is_watch_page(url)
}

pub use refusal::{refusal, said_by, UNKNOWN_SITE};

#[cfg(test)]
use process::wait_at_most;
use process::{end_tree, Tree};
#[cfg(test)]
use readings::reading_path;
use readings::{fresh_reading, remember_reading, wait_for_reading};

/// The line prefix yt-dlp is told to stamp on its progress, so dm can pick
/// its own numbers out of everything else the tool prints.
const MARK: &str = "@dm@";

/// Whether a yt-dlp is on the machine to hand pages to.
///
/// Checked rather than assumed: dm works without one, and a page it cannot
/// hand anywhere is reported as such rather than failing to launch a program
/// that is not there.
pub fn available() -> bool {
    which("yt-dlp").is_some()
}

/// Whether a yt-dlp dm would run is there: `available`, and not one too old
/// to be passed over (Debian's own package, say) with nothing fetched to
/// replace it. What a test that needs a real yt-dlp asks, to skip rather
/// than fail on a machine with only a stale one.
pub fn usable() -> bool {
    binary().is_some()
}

/// Why yt-dlp at `path` would not start, in words a person can act on:
/// the system's own words kept in brackets.
fn would_not_start(path: &Path, error: &std::io::Error) -> String {
    let why = match error.kind() {
        std::io::ErrorKind::NotFound => {
            "its program is gone from dm's tools folder; dm fetches it again at the next video"
        }
        std::io::ErrorKind::PermissionDenied => {
            "the system will not let it run: its folder may not allow programs (a noexec temporary \
             folder), or the security software blocked it"
        }
        _ if error.raw_os_error() == Some(86) || error.raw_os_error() == Some(8) => {
            "it is built for another kind of processor"
        }
        _ => "it would not start",
    };
    format!(
        "dm's video helper (yt-dlp, at {}) {why} ({error})",
        path.display()
    )
}

/// Where yt-dlp actually is, resolved the same way `available` decides.
pub(crate) fn binary() -> Option<PathBuf> {
    let found = which("yt-dlp");
    // One named outright (`DM_YTDLP`) is used as it is.
    if std::env::var_os("DM_YTDLP").is_some() {
        return found;
    }
    // An installed yt-dlp months old fails on the sites that changed since
    // (Debian's and Ubuntu's own packages lag far behind): passed over for
    // the copy dm keeps current, or for one fetched now (`tools::ensure`),
    // or for the one shipped inside dm's own package.
    if let Some(found) = found {
        if crate::tools::is_fetched(&found) || !stale(&found) {
            return Some(found);
        }
    }
    // A shipped one is copied into the tools folder on first use and run
    // from there (`tools::adopt`); in place only where that copy fails.
    crate::tools::fetched("yt-dlp").or_else(|| {
        let shipped = bundled()?;
        let zipped = shipped.extension().is_some_and(|e| e == "zip");
        crate::tools::adopt(&shipped).or_else(|| (!zipped).then_some(shipped))
    })
}

/// The yt-dlp shipped inside dm's own package, where the packaging put one
/// (`docs/RELEASING.md`): the release's zip under `yt-dlp/` beside the
/// program, or in the macOS bundle's `Contents/Resources` (an unpacked
/// build there too, for an older package). The first video
/// then needs no fetch; a newer one dm fetches after a failure takes its
/// place (`tools::fetched` is looked at first).
fn bundled() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    bundled_in(&exe).into_iter().find(|p| p.is_file())
}

/// Where a shipped yt-dlp may be, for the program at `exe`.
fn bundled_in(exe: &Path) -> Vec<PathBuf> {
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    // The folders a package puts it in: beside the program, under its
    // resources (as cargo-packager lays them out, with or without the
    // `resources/` it was given), in a macOS bundle's Resources, and in a
    // Linux package's own folders.
    let mut folders = vec![dir.join("yt-dlp"), dir.join("resources").join("yt-dlp")];
    if cfg!(target_os = "macos") {
        let resources = dir.join("..").join("Resources");
        folders.push(resources.join("yt-dlp"));
        folders.push(resources.join("resources").join("yt-dlp"));
    } else if cfg!(target_os = "linux") {
        for lib in ["lib", "share"] {
            folders.push(dir.join("..").join(lib).join("dm").join("yt-dlp"));
        }
    }
    // The build's zip as its release names it (`yt-dlp_macos.zip`), which
    // is how a package carries it; then the program as the unpacked build
    // names it (`yt-dlp_macos`), then the plain name.
    let asset = crate::tools::ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH);
    let mut names: Vec<String> = asset.map(str::to_string).into_iter().collect();
    names.extend(asset.map(crate::tools::unpacked_program));
    names.push(
        if cfg!(windows) {
            "yt-dlp.exe"
        } else {
            "yt-dlp"
        }
        .to_string(),
    );
    folders
        .iter()
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .collect()
}

/// How old an installed yt-dlp may be before dm prefers its own: YouTube
/// changes often enough that a release four months old is often broken.
const STALE_DAYS: i64 = 120;

/// Whether the yt-dlp at `path` says it is older than [`STALE_DAYS`]. Asked
/// once a run for each copy; one that does not answer with a date is taken
/// as it is.
fn stale(path: &Path) -> bool {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, bool>>> =
        std::sync::OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some(known) = crate::lock(seen).get(path) {
        return *known;
    }
    let mut command = std::process::Command::new(path);
    crate::no_window(&mut command);
    if let Some(dir) = runnable_temp() {
        command.env("TMPDIR", dir);
    }
    let said = command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let today = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i64);
    let old = said
        .as_deref()
        .and_then(released_on)
        .is_some_and(|day| today - day > STALE_DAYS);
    crate::lock(seen).insert(path.to_path_buf(), old);
    old
}

/// The day a yt-dlp version names, as days since 1970: its versions are
/// dates, `2026.08.19` (a nightly adds `.123456`).
fn released_on(version: &str) -> Option<i64> {
    let mut parts = version.trim().split('.');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(2000..=2200).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// A Deno yt-dlp can run a site's player challenges in, if any: one the
/// person installed (or one an earlier dm fetched).
pub(crate) fn deno() -> Option<PathBuf> {
    which("deno")
}

/// The QuickJS yt-dlp can run a site's player challenges in, if any: the
/// one dm fetched, or one installed.
pub(crate) fn quickjs() -> Option<PathBuf> {
    which("qjs")
}

/// Tells yt-dlp which JavaScript engine to use, by its path: it looks only
/// on the PATH, which the one dm fetched, or one a Dock launch cannot see,
/// is not on. A Deno the person installed first, which reads a page a
/// little faster; else QuickJS, with Deno's default turned off so the one
/// given is the one used.
fn js_runtime(command: &mut Command) {
    if let Some(deno) = deno() {
        command
            .arg("--js-runtimes")
            .arg(format!("deno:{}", deno.display()));
    } else if let Some(quickjs) = quickjs() {
        command
            .arg("--no-js-runtimes")
            .arg("--js-runtimes")
            .arg(format!("quickjs:{}", quickjs.display()));
    }
}

/// Whether `path` is on a file system mounted `noexec`, by the mount table
/// (`/proc/self/mounts`): the mount whose point is the longest parent of
/// `path` decides. A point is compared by whole folders, so `/tmp2` is not
/// `/tmp`.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn on_noexec_mount(mounts: &str, path: &Path) -> bool {
    // The table writes a space, a tab, a new line and a backslash in a name as
    // \040, \011, \012 and \134.
    fn unescaped(field: &str) -> String {
        let mut out = String::new();
        let mut chars = field.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                let octal: String = chars.clone().take(3).collect();
                if let Ok(byte) = u8::from_str_radix(&octal, 8) {
                    out.push(byte as char);
                    chars.nth(2);
                    continue;
                }
            }
            out.push(c);
        }
        out
    }
    let mut deciding: Option<(usize, bool)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_device), Some(point), Some(_kind), Some(options)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let point = unescaped(point);
        if path.starts_with(&point) {
            let depth = Path::new(&point).components().count();
            if deciding.is_none_or(|(deepest, _)| depth >= deepest) {
                deciding = Some((depth, options.split(',').any(|o| o == "noexec")));
            }
        }
    }
    deciding.is_some_and(|(_, noexec)| noexec)
}

/// Where yt-dlp may unpack itself, where the system's temporary folder does
/// not let a program run from it (mounted `noexec`, as hardened and tuned
/// Linux systems have `/tmp`): a folder of dm's own beside the tools, which
/// has to allow it for the tools to run at all. `None` where the temporary
/// folder is fine, or there is no tools folder, which leaves yt-dlp as it
/// was. A single-file yt-dlp (what dm fetched before it fetched the unpacked
/// one, and what a person installs) unpacks into the temporary folder at every
/// start, and could not start under `noexec`: "error while loading shared
/// libraries: libz.so.1: failed to map segment from shared object".
fn runnable_temp() -> Option<&'static Path> {
    static CHOSEN: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CHOSEN
        .get_or_init(|| {
            if !cfg!(target_os = "linux") {
                return None;
            }
            let temp = std::env::temp_dir().canonicalize().ok()?;
            let mounts = std::fs::read_to_string("/proc/self/mounts").ok()?;
            if !on_noexec_mount(&mounts, &temp) {
                return None;
            }
            let dir = crate::tools::folder()?.join("tmp");
            std::fs::create_dir_all(&dir).ok()?;
            forget_old_unpackings(&dir);
            Some(dir)
        })
        .as_deref()
}

/// What a yt-dlp that was killed left of its unpacking (`_MEI…`), a day old or
/// more, removed from `dir`.
fn forget_old_unpackings(dir: &Path) {
    let day = std::time::Duration::from_secs(86_400);
    let Ok(listing) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in listing.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > day);
        if old && entry.file_name().to_string_lossy().starts_with("_MEI") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// yt-dlp run where it can unpack itself (`runnable_temp`).
/// yt-dlp at `binary`, in the sandbox where Linux allows one, writing to
/// `writable` and the temporary folder dm points it at. A fetched portable
/// build is started through the computer's Python (`tools::python_for`).
pub(crate) fn writable_to(binary: &Path, writable: &[&Path]) -> Command {
    let mut open = writable.to_vec();
    if let Some(dir) = runnable_temp() {
        open.push(dir);
    }
    match crate::tools::python_for(binary) {
        Some(python) => {
            let mut command = sandbox::command(&python, &open);
            command.arg(binary);
            command
        }
        None => sandbox::command(binary, &open),
    }
}

/// A Python 3.10 or newer on this computer, for yt-dlp's portable build
/// where the release has no standalone one: `DM_PYTHON` outright, else
/// `python3` on `PATH` or in the usual places, asked its version once.
pub(crate) fn python3() -> Option<PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            if let Some(named) = std::env::var_os("DM_PYTHON") {
                return (!named.is_empty())
                    .then(|| PathBuf::from(named))
                    .filter(|p| p.is_file());
            }
            let on_path = std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_default();
            let usual = [
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/opt/homebrew/bin"),
            ];
            on_path
                .iter()
                .chain(usual.iter())
                .map(|dir| dir.join("python3"))
                .filter(|p| p.is_file())
                .find(|p| python_is_new_enough(p))
        })
        .clone()
}

/// Whether `python` says it is 3.10 or newer, which yt-dlp needs.
fn python_is_new_enough(python: &Path) -> bool {
    let mut command = std::process::Command::new(python);
    crate::no_window(&mut command);
    let Ok(out) = command.arg("--version").output() else {
        return false;
    };
    let said = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    let mut parts = said
        .trim()
        .strip_prefix("Python ")
        .unwrap_or("")
        .split('.')
        .filter_map(|n| n.parse::<u32>().ok());
    matches!((parts.next(), parts.next()), (Some(3), Some(minor)) if minor >= 10)
}

fn unpack_where_programs_run(command: &mut Command) {
    if let Some(dir) = runnable_temp() {
        command.env("TMPDIR", dir);
    }
}

/// The first `yt-dlp` on `PATH`, or in the places package managers put it.
///
/// `PATH` alone is not enough, and this is the bug that made "YouTube does
/// not download" true on a machine with yt-dlp installed. A program launched
/// from the Dock, a desktop entry, or by a browser's native messaging host
/// inherits the session's minimal `PATH` — `/usr/bin:/bin` and little else —
/// not the shell's, so `/opt/homebrew/bin` and `~/.local/bin` are invisible to
/// it even though `which yt-dlp` at a terminal finds it fine. The usual homes
/// are searched after `PATH`, so the tool a person installed is the tool the
/// app finds.
///
/// `DM_` and the program's name (`DM_YTDLP`, `DM_DENO`, `DM_QJS`) name the
/// binary outright, for an install in none of those places; set to nothing,
/// they say there is no such tool,
/// which is how a test pins down what dm does without one on a machine that
/// has one.
fn which(program: &str) -> Option<PathBuf> {
    let override_var = format!("DM_{}", program.replace('-', "").to_ascii_uppercase());
    if let Some(named) = std::env::var_os(&override_var) {
        return (!named.is_empty())
            .then(|| PathBuf::from(named))
            .filter(|p| p.is_file());
    }
    // As a computer with nothing installed: only what dm fetched itself.
    // For checking the out-of-the-box route where tools are installed.
    if std::env::var_os("DM_ONLY_FETCHED_TOOLS").is_some_and(|v| !v.is_empty()) {
        return crate::tools::fetched(program);
    }
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    let home = dirs::home_dir().unwrap_or_default();
    let usual = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        home.join(".local/bin"),
        home.join("bin"),
        PathBuf::from("/snap/bin"),
        home.join("Library/Python/3.13/bin"),
        home.join("Library/Python/3.12/bin"),
    ];
    let windows = if cfg!(windows) {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        windows_homes(&Homes {
            local: var("LOCALAPPDATA"),
            roaming: var("APPDATA"),
            home: home.clone(),
            program_data: var("ProgramData"),
            program_files: var("ProgramFiles"),
            system_drive: var("SystemDrive"),
        })
    } else {
        Vec::new()
    };
    // Last, the copy dm fetched itself (`tools`): one the person installed
    // is theirs to choose.
    find_in(
        on_path.into_iter().chain(usual).chain(windows),
        &names(program, cfg!(windows)),
    )
    .or_else(|| crate::tools::fetched(program))
}

/// The file names a program goes by: on Windows its `.exe` (and `.com`),
/// which is what is on disk. The bare name was looked for there and never
/// found, so an installed yt-dlp was reported missing (issue 10).
fn names(program: &str, windows: bool) -> Vec<String> {
    if windows {
        vec![format!("{program}.exe"), format!("{program}.com")]
    } else {
        vec![program.to_string()]
    }
}

/// The first of `names` in the first of `dirs` that has one.
fn find_in(dirs: impl Iterator<Item = PathBuf>, names: &[String]) -> Option<PathBuf> {
    for dir in dirs {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The folders a Windows account's own environment names.
struct Homes {
    local: Option<PathBuf>,
    roaming: Option<PathBuf>,
    home: PathBuf,
    program_data: Option<PathBuf>,
    program_files: Option<PathBuf>,
    system_drive: Option<PathBuf>,
}

/// The folders in `dir` whose names start with `prefix` (any case), newest
/// first where they are versioned: Python313 before Python312.
fn children(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let prefix = prefix.to_ascii_lowercase();
    let mut found: Vec<PathBuf> = listing
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(&prefix)
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found.reverse();
    found
}

/// Where Windows installs put yt-dlp, beside `PATH`: winget's links and its
/// package folders (a build that keeps its program in a `bin` one level
/// down), pip's Scripts folders for python.org's and the Store's Pythons,
/// Scoop's shims, pipx's folder, Chocolatey's bin, and the folders people
/// unpack tools into by hand. A dm started by a browser or from the
/// Start menu has a `PATH` that may name none of them.
fn windows_homes(h: &Homes) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = &h.local {
        let winget = local.join("Microsoft").join("WinGet");
        dirs.push(winget.join("Links"));
        for package in children(&winget.join("Packages"), "") {
            dirs.push(package.join("bin"));
            for inner in children(&package, "") {
                dirs.push(inner.join("bin"));
                dirs.push(inner);
            }
            dirs.push(package);
        }
        for python in children(&local.join("Programs").join("Python"), "python") {
            dirs.push(python.join("Scripts"));
        }
        for store in children(&local.join("Packages"), "pythonsoftwarefoundation.python") {
            for python in children(&store.join("LocalCache").join("local-packages"), "python") {
                dirs.push(python.join("Scripts"));
            }
        }
    }
    if let Some(roaming) = &h.roaming {
        for python in children(&roaming.join("Python"), "python") {
            dirs.push(python.join("Scripts"));
        }
    }
    dirs.push(h.home.join("scoop").join("shims"));
    dirs.push(h.home.join(".local").join("bin"));
    if let Some(data) = &h.program_data {
        dirs.push(data.join("chocolatey").join("bin"));
    }
    if let Some(files) = &h.program_files {
        dirs.push(files.join("yt-dlp"));
    }
    if let Some(drive) = &h.system_drive {
        let root = PathBuf::from(format!("{}\\", drive.display()));
        dirs.push(root.join("yt-dlp"));
    }
    dirs
}

/// Writes `text` to `path` readable by this account alone.
pub(crate) async fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    let _ = tokio::fs::remove_file(path).await;
    let mut open = tokio::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    open.mode(0o600);
    let mut file = open.open(path).await?;
    tokio::io::AsyncWriteExt::write_all(&mut file, text.as_bytes()).await?;
    Ok(())
}

/// Hands yt-dlp the proxy in a config file of the run's own under `dir`,
/// readable by this account alone and removed when the run ends. On the
/// command line, the proxy's user name and password were there for any
/// local user's `ps` to read, as the cookies were before them.
async fn proxy_by_file(
    command: &mut Command,
    dir: &Path,
    proxy: &str,
) -> Result<RemoveOnDrop, Error> {
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("dm-proxy-{}-{run}.conf", std::process::id()));
    // yt-dlp splits its config as a POSIX shell does: single quotes keep
    // everything, and a quote inside is closed, escaped and reopened.
    let quoted = proxy.replace('\'', r"'\''");
    write_private(&path, &format!("--proxy '{quoted}'\n")).await?;
    command.arg("--config-locations").arg(&path);
    Ok(RemoveOnDrop(path))
}

/// Hands yt-dlp the site's sign-in in a cookies file of the run's own at
/// `path`, readable by this account alone and removed when the run ends —
/// never on the command line, where any local user's `ps` reads it — and
/// scoped to the site's own server, so yt-dlp sends it there and nowhere
/// else. A cookies file a person chose is copied in beside it, since yt-dlp
/// takes one; with no sign-in for `url`, that file is handed over as it is.
async fn cookies_by_file(
    command: &mut Command,
    path: &Path,
    sign_in: Option<&crate::cookies::SignIn>,
    url: &str,
) -> Result<Option<RemoveOnDrop>, Error> {
    let Some(sign_in) = sign_in.filter(|s| s.for_url(url).is_some()) else {
        if let Some(cookies) = crate::cookies::file() {
            command.arg("--cookies").arg(cookies);
        }
        return Ok(None);
    };
    let mut text = String::from("# Netscape HTTP Cookie File\n");
    if let Some(chosen) = crate::cookies::file() {
        if let Ok(theirs) = tokio::fs::read_to_string(&chosen).await {
            text.push_str(&theirs);
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
    }
    text.push_str(&sign_in.netscape_lines());
    // A file left by a run that ended uncleanly is not one to add to.
    let _ = tokio::fs::remove_file(path).await;
    write_private(path, &text).await?;
    command.arg("--cookies").arg(path);
    Ok(Some(RemoveOnDrop(path.to_path_buf())))
}

/// Removes a file when it goes out of scope: a run's private cookies file,
/// whichever way the run ends.
pub(crate) struct RemoveOnDrop(pub(crate) PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Whether a response is a web page rather than a file dm fetches itself.
///
/// A file has a body dm can range over; a web page does not, and saving the
/// page's HTML as though it were the download is the wrong answer whether or
/// not a yt-dlp is there to do better. So this is decided on the content type
/// alone, and the caller decides between handing it to yt-dlp and refusing.
pub fn is_page(content_type: Option<&str>) -> bool {
    crate::probe::mime_essence(content_type) == "text/html"
}

/// The `-f` selector for a wanted quality, in yt-dlp's language.
///
/// The best picture and the best sound are usually separate streams, which
/// dm joins itself (`mp4mux` into an MP4, `mkvmux` into a WebM): the
/// biggest picture, MP4 preferred at a size, whatever its container, and
/// a WebM picture and sound where WebM is asked for. A picture-only stream
/// (`bv`), so one that carries its own sound is not joined with a second.
///
/// A quality names the picture's shorter side, as the sites name it: 720p
/// is 1280×720 for a landscape video and 720×1280 for a portrait one (a
/// Short, a phone's clip), so the portrait video is asked for by its width
/// first (`aspect_ratio<1`, which a format with no width fails and so falls
/// to the height clause), then any video by its height.
fn format_for(quality: Quality, container: Option<&str>) -> String {
    let webm = container == Some("webm");
    let chosen = match quality {
        Quality::Best if webm => "bv[ext=webm]+ba[ext=webm]/b[ext=webm]/b".to_string(),
        Quality::UpTo(size) if webm => format!(
            "bv[ext=webm][aspect_ratio<1][width<={size}]+ba[ext=webm]/bv[ext=webm][height<={size}]+ba[ext=webm]/b[ext=webm][aspect_ratio<1][width<={size}]/b[ext=webm][height<={size}]/b[aspect_ratio<1][width<={size}]/b[height<={size}]/b"
        ),
        Quality::Best => "bv+ba/b".to_string(),
        Quality::UpTo(size) => format!(
            "bv[aspect_ratio<1][width<={size}]+ba/bv[height<={size}]+ba/b[aspect_ratio<1][width<={size}]/b[height<={size}]/b"
        ),
        // The best audio-only stream — what "audio" means for a video site.
        Quality::Audio => return "ba/b".to_string(),
    };
    /* Another container (a site's FLV, 3GP, MOV) is one dm does not join
     * into: asked for whole, picture and sound in the one file, which
     * needs no joining; else what dm joins. */
    match container {
        Some(c) if !joined_by_dm(c) => format!("b[ext={c}]/{chosen}"),
        _ => chosen,
    }
}

/// Whether dm joins picture and sound into this container itself.
fn joined_by_dm(container: &str) -> bool {
    container.is_empty() || ["mp4", "m4a", "webm"].contains(&container)
}

/// What yt-dlp leaves where it does not join them itself: the picture and
/// the sound beside the name it would have joined them under,
/// `name.f137.mp4` and `name.f140.m4a`, picture first.
async fn parts_of(joined: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (joined.parent(), joined.file_stem()) else {
        return Vec::new();
    };
    let prefix = format!("{}.f", stem.to_string_lossy());
    let Ok(mut listing) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    while let Ok(Some(entry)) = listing.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let media = [".mp4", ".m4a", ".m4v", ".mov", ".webm"]
            .iter()
            .any(|ext| name.to_ascii_lowercase().ends_with(ext));
        if name.starts_with(&prefix) && media {
            parts.push(entry.path());
        }
    }
    // Picture before sound, as players expect the first track to be.
    parts.sort_by_key(|p| {
        let audio = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("m4a"));
        (audio, p.clone())
    });
    parts
}

/// yt-dlp's format-sort preference for the container a panel line named, so
/// what arrives is what the list said: MP4 picture with M4A sound, WebM with
/// WebM, or the named sound alone. With none named (Add URL, a page handed
/// over whole) it is MP4: left to itself
/// yt-dlp picks YouTube's WebM. An MP4 is joined into an MP4 even when the
/// best picture at that height is only in WebM, and H.264 is preferred over
/// AV1 at the same height, since it plays everywhere.
fn container_sort(container: Option<&str>) -> Vec<String> {
    let mp4 = || {
        [
            "-S",
            "res,ext:mp4:m4a,vcodec:h264",
            "--merge-output-format",
            "mp4",
        ]
        .map(String::from)
        .to_vec()
    };
    let sort = match container {
        Some("mp4") | None => return mp4(),
        Some("webm") => "ext:webm:webm",
        Some("m4a") => "ext:m4a:m4a",
        Some(other) if !other.is_empty() => return vec!["-S".into(), format!("ext:{other}")],
        _ => return mp4(),
    };
    vec!["-S".into(), sort.into()]
}

/// How long yt-dlp may say nothing before dm ends it. Before its first word,
/// as long as a page reading may take (`DM_YTDLP_QUIET` seconds where set,
/// for tests); once it has started, five times that, for a merge or a slow
/// server. A yt-dlp hung at start kept its row "running" at nothing for ever
/// (BUGLOG.md, BUG-004).
struct Heard {
    since: std::time::Instant,
    /// Milliseconds after `since` it last said something; 0 for never.
    last: std::sync::atomic::AtomicU64,
    before: std::time::Duration,
}

impl Heard {
    fn new() -> Self {
        let before = std::env::var("DM_YTDLP_QUIET")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(
                std::time::Duration::from_secs(120),
                std::time::Duration::from_secs,
            );
        Heard {
            since: std::time::Instant::now(),
            last: std::sync::atomic::AtomicU64::new(0),
            before,
        }
    }

    fn now(&self) {
        let at = u64::try_from(self.since.elapsed().as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        self.last.store(at, std::sync::atomic::Ordering::Relaxed);
    }

    /// How long it has been quiet, once that is past what it may be.
    fn too_quiet(&self) -> Option<std::time::Duration> {
        let last = self.last.load(std::sync::atomic::Ordering::Relaxed);
        let (from, limit) = if last == 0 {
            (std::time::Duration::ZERO, self.before)
        } else {
            (std::time::Duration::from_millis(last), self.before * 5)
        };
        let quiet = self.since.elapsed().saturating_sub(from);
        (quiet > limit).then_some(quiet)
    }
}

/// Runs yt-dlp for one page, reporting progress and placing the result.
///
/// Staged into the download's own directory and moved into place on success,
/// the same shape every other download follows: nothing lands in the finished
/// folder until it is finished.
pub async fn download_ytdlp(
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    download_ytdlp_from(url, None, options, report).await
}

/// [`download_ytdlp`], from a reading of the page yt-dlp already made
/// (`--load-info-json`) where there is one, so the page is not read twice.
async fn download_ytdlp_from(
    url: &str,
    reading: Option<&Path>,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let mut unknown_failure = false;
    match download_ytdlp_once(url, reading, options, report.clone(), &mut unknown_failure).await {
        /* Failed in words that are not a site's refusal: a site may have
         * changed under yt-dlp, whose fix is usually out within days. A
         * newer release is fetched now, where there is one, and the run
         * made once more with it (docs/gaps.md row 33). */
        Err(why) if unknown_failure && crate::tools::newer_ytdlp(&crate::client()).await => {
            eprintln!("dm: yt-dlp failed ({why}); a newer yt-dlp was fetched, so once more");
            let mut again = false;
            download_ytdlp_once(url, reading, options, report, &mut again).await
        }
        outcome => outcome,
    }
}

/// One run of yt-dlp's own download. `unknown_failure` is set where it
/// failed in words that are not a known refusal.
async fn download_ytdlp_once(
    url: &str,
    reading: Option<&Path>,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    unknown_failure: &mut bool,
) -> Result<Finished, Error> {
    let staging = &options.parts_dir;
    tokio::fs::create_dir_all(staging)
        .await
        .map_err(Error::Disk)?;
    // Where yt-dlp writes the final path it settled on, away from the noise
    // of its progress so dm reads it back cleanly.
    let result_file = staging.join(".dm-ytdlp-result");
    let _ = tokio::fs::remove_file(&result_file).await;

    // Spawned by its resolved path, not its bare name: the bare name goes
    // through `PATH`, which is exactly what a GUI-launched app lacks, so
    // finding it in /opt/homebrew/bin and then failing to start it would be
    // the same bug one step later.
    let binary = binary()
        .ok_or_else(|| Error::Internal("dm's video helper is not available here".into()))?;
    let mut command = writable_to(&binary, &[staging]);
    crate::no_window_async(&mut command);
    command
        .arg("--no-playlist")
        .arg("--no-color")
        .arg("--newline")
        // Resume from a part left by a previous run, which is what a pause and
        // a later start turn into.
        .arg("--continue")
        .arg("-o")
        .arg(staging.join("%(title).200B [%(id)s].%(ext)s"))
        .arg("-f")
        .arg(format_for(options.quality, options.container.as_deref()))
        .args(container_sort(options.container.as_deref()))
        .arg("--progress-template")
        .arg(format!(
            "download:{MARK}%(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s"
        ))
        .arg("--print-to-file")
        .arg("after_move:filepath")
        .arg(&result_file)
        // Bounded, where yt-dlp's defaults are ten tries per fragment with
        // no socket timeout: a segment host answering 503 would otherwise
        // keep the row "running" for minutes before dm hears it failed.
        .arg("--retries")
        .arg("3")
        .arg("--fragment-retries")
        .arg("3")
        // Fragments of an HLS or DASH stream fetched together, not one at a
        // time (yt-dlp's default): a round trip per fragment otherwise.
        .arg("--concurrent-fragments")
        .arg(options.connections.clamp(1, 8).to_string())
        /* A plain file asked for in pieces of this size, each its own request:
         * a server that slows or cuts any one response (YouTube's do, which
         * yt-dlp does for its own formats already) is then asked afresh. A
         * server that ignores ranges answers the whole file and is
         * unaffected, and `--limit-rate` and the progress lines are the same
         * with it (run against yt-dlp 2026.10.21). Not `--throttled-rate`:
         * yt-dlp reads a link slower than that as a throttled server and
         * starts the download over, for ever, where it finishes without. */
        .arg("--http-chunk-size")
        .arg("10M")
        .arg("--socket-timeout")
        .arg("15")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    /* A process group of its own, so a pause reaches what yt-dlp started
     * (the JavaScript runtime it answers a site's checks with) as well as
     * yt-dlp itself. */
    #[cfg(unix)]
    command.process_group(0);

    js_runtime(&mut command);
    unpack_where_programs_run(&mut command);
    if options.subtitles {
        // Fetched as WebVTT beside the video, and put into it by dm.
        command.args(["--write-subs", "--sub-langs", "all", "--sub-format", "vtt"]);
    }

    /* The same two things every request of dm's own carries. yt-dlp makes
     * its own connections, so the proxy the client was built with does not
     * reach it unless it is told — and a machine where everything must go
     * through the proxy is exactly where a direct connection is the one
     * outcome nobody wants. The referrer is what a hotlink-protected page
     * checks for, and the probe that routed here already needed it. */
    let _proxied = match options.proxy_for(url) {
        Some(proxy) => Some(proxy_by_file(&mut command, staging, proxy).await?),
        None => None,
    };
    if let Some(referrer) = &options.referrer {
        command.arg("--referer").arg(referrer);
    }
    // The site's sign-in, in a private file of this run's (`cookies_by_file`).
    let _signed = cookies_by_file(
        &mut command,
        &staging.join("sign-in.cookies.txt"),
        options.sign_in.as_ref(),
        url,
    )
    .await?;

    // This run's part of the shared ceiling, if one is set, held until it
    // ends. yt-dlp takes bytes per second, once: it keeps the part it was
    // given when others start or finish.
    let _share = options.limiter.share();
    let rate = options.limiter.part().await;
    if rate > 0 {
        command.arg("--limit-rate").arg(rate.to_string());
    }

    match reading {
        Some(reading) => {
            command.arg("--load-info-json").arg(reading);
        }
        // After `--`: whatever the address, yt-dlp never reads it as an option.
        None => {
            command.arg("--").arg(url);
        }
    }

    let mut child = command
        .spawn()
        .map_err(|e| Error::Internal(would_not_start(&binary, &e)))?;
    // Declared after the child, so dropped before it: the group is ended
    // while yt-dlp, its leader, is still there to name it.
    let mut tree = Tree::of(child.id());

    // Drained on its own task, not left in the pipe. A long download that
    // logged more warnings than the pipe holds would otherwise block yt-dlp
    // writing to a stderr nobody is reading. Kept to report on a failure.
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Internal("yt-dlp started without its error output".into()))?;
    // When yt-dlp last said anything, on either stream, for the watchdog.
    let heard = std::sync::Arc::new(Heard::new());
    let heard_errors = heard.clone();
    let errors = tokio::spawn(async move {
        let mut text = String::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            heard_errors.now();
            text.push_str(&line);
            text.push('\n');
        }
        text
    });

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Internal("yt-dlp started without its output".into()))?;
    let mut lines = BufReader::new(stdout).lines();
    let mut named = false;
    let mut sum = Cumulative::default();

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line.map_err(Error::Disk)? else { break };
                heard.now();

                if let Some(rest) = line.strip_prefix(MARK) {
                    if let Some(tx) = &report {
                        if let Some(progress) = parse_progress(rest) {
                            let _ = tx.send(Update::Progress(sum.add(progress))).await;
                        }
                    }
                } else if let Some(dest) = line.strip_prefix("[download] Destination: ") {
                    // Each stream of a pair announces itself here.
                    sum.next_stream();
                    // An early name, before the merge settles the final one.
                    if !named {
                        if let Some(name) = Path::new(dest.trim()).file_name() {
                            let name = name.to_string_lossy().into_owned();
                            if let Some(tx) = &report {
                                let _ = tx
                                    .send(Update::Named {
                                        category: category::classify(&name, None),
                                        name,
                                    })
                                    .await;
                            }
                            named = true;
                        }
                    }
                }
            }
            // Between lines, so a pause or cancel is honoured even while
            // yt-dlp is quiet. Killing the child leaves its part behind for
            // --continue to resume from; a cancel throws that away.
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                if let Some(quiet) = heard.too_quiet() {
                    if let Some(pid) = tree.take() {
                        let _ = tokio::task::spawn_blocking(move || end_tree(pid)).await;
                    }
                    let _ = child.kill().await;
                    return Err(Error::Internal(format!(
                        "yt-dlp stopped answering: nothing from it in {} s, so it was ended",
                        quiet.as_secs()
                    )));
                }
                if let Err(stop) = options.control.check() {
                    // The tree first, and waited for, off the runtime: on
                    // Windows `taskkill /T` finds yt-dlp's workers through
                    // yt-dlp, and a launcher killed first left them
                    // running.
                    if let Some(pid) = tree.take() {
                        let _ = tokio::task::spawn_blocking(move || end_tree(pid)).await;
                    }
                    let _ = child.kill().await;
                    if matches!(stop, Error::Cancelled) {
                        remove_staging(staging).await;
                    }
                    return Err(stop);
                }
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| Error::Internal(format!("yt-dlp did not finish: {e}")))?;
    // Reaped: its number may now be another program's, never to be killed.
    tree.disarm();

    // Checked after draining stdout, so a pause noticed on the last line wins.
    options.control.check()?;

    if !status.success() {
        let stderr = errors.await.unwrap_or_default();
        let tail = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        /* yt-dlp's own words for a refusal ("Requested format is not
         * available", a sign-in) name neither the cause nor the fix. */
        if let Some(said) = refusal(&stderr) {
            return Err(Error::Internal(said));
        }
        *unknown_failure = true;
        return Err(Error::Internal(format!(
            "yt-dlp could not download that{}",
            if tail.is_empty() {
                String::new()
            } else {
                format!(": {tail}")
            }
        )));
    }

    let produced = tokio::fs::read_to_string(&result_file)
        .await
        .ok()
        .and_then(|s| s.lines().next_back().map(str::trim).map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| Error::Internal("yt-dlp reported no output file".into()))?;

    /* Subtitles yt-dlp wrote beside the video, for `finish_dm` to carry
     * inside it; their own files are removed whatever it does. */
    let found = crate::subtitles::beside(&produced);
    let (vtt_files, texts): (Vec<PathBuf>, Vec<crate::subtitles::Subtitle>) =
        found.into_iter().unzip();
    let outcome = finish_dm(&produced, texts, options).await;
    for file in vtt_files {
        let _ = tokio::fs::remove_file(file).await;
    }
    outcome
}

/// What dm does with yt-dlp's output: joins the picture and sound it left
/// apart, puts a transport stream into an MP4, carries the subtitles
/// inside, and places the result.
async fn finish_dm(
    produced: &Path,
    texts: Vec<crate::subtitles::Subtitle>,
    options: &Options,
) -> Result<Finished, Error> {
    let produced = produced.to_path_buf();
    // A part of the video: the pieces fetched hold a little around it, and
    // the joined file's edit lists play the part alone (`clip`).
    let cut = options.span.map(|span| span.millis());
    if !produced.is_file() {
        let parts = parts_of(&produced).await;
        if !parts.is_empty() {
            // A WebM where one was asked for; else an MP4, which takes a
            // WebM picture or sound too.
            let webm = options.container.as_deref() == Some("webm")
                && parts.iter().all(|p| {
                    p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("webm"))
                });
            if webm && cut.is_some() {
                return Err(Error::Internal(
                    "a part is cut from MP4 streams only, not WebM".into(),
                ));
            }
            let joined = produced.with_extension(if webm { "webm" } else { "mp4" });
            let joining = if webm {
                crate::mkvmux::join_with_text_async(parts.clone(), texts, joined.clone()).await
            } else {
                crate::mp4mux::join_cut_async(
                    parts.clone(),
                    texts,
                    joined.clone(),
                    cut,
                    options.shape,
                )
                .await
            };
            joining.map_err(|why| {
                Error::Internal(format!("the picture and sound could not be joined: {why}"))
            })?;
            for part in parts {
                let _ = tokio::fs::remove_file(part).await;
            }
            return place(&joined, options).await;
        }
    } else if let Some(cut) = cut {
        // One stream's part alone (the sound): rewritten with the cut.
        let ext = produced
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mp4".into());
        let cut_file = produced.with_extension(format!("dm-cut.{ext}"));
        crate::mp4mux::join_cut_async(
            vec![produced.clone()],
            texts,
            cut_file.clone(),
            Some(cut),
            options.shape,
        )
        .await
        .map_err(|why| Error::Internal(format!("the part could not be cut: {why}")))?;
        tokio::fs::rename(&cut_file, &produced)
            .await
            .map_err(Error::Disk)?;
        return place(&produced, options).await;
    }
    /* An HLS site's video, fetched by yt-dlp's own downloader, is the
     * stream's transport stream under an `.mp4` name: dm puts it right. */
    if is_transport_stream(&produced).await {
        let fixed = produced.with_extension("dm-remux.mp4");
        if crate::tsmux::to_mp4_async(produced.clone(), fixed.clone())
            .await
            .is_ok()
        {
            let _ = tokio::fs::rename(&fixed, &produced).await;
        } else {
            let _ = tokio::fs::remove_file(&fixed).await;
        }
    }
    /* One file that already held picture and sound, with subtitles beside
     * it: written again with them inside. A video that will not take them
     * is kept as it is; the subtitles are not worth the video. */
    let kind = produced
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !texts.is_empty() && ["mp4", "m4v", "webm", "mkv"].contains(&kind.as_str()) {
        let with = produced.with_extension(format!("dm-subtitles.{kind}"));
        let written = if kind == "webm" || kind == "mkv" {
            crate::mkvmux::join_with_text_async(vec![produced.clone()], texts, with.clone()).await
        } else {
            crate::mp4mux::join_with_text_async(vec![produced.clone()], texts, with.clone()).await
        };
        if written.is_ok() {
            let _ = tokio::fs::rename(&with, &produced).await;
        } else {
            let _ = tokio::fs::remove_file(&with).await;
        }
    }
    place(&produced, options).await
}

/// Whether a file is an MPEG transport stream, whatever its name: a sync
/// byte at the start of its first two packets.
async fn is_transport_stream(path: &Path) -> bool {
    use tokio::io::AsyncReadExt;
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut head = [0u8; 189];
    file.read_exact(&mut head).await.is_ok() && head[0] == 0x47 && head[188] == 0x47
}

/// Moves a finished yt-dlp file into its category folder under the download
/// directory, the same placement every other download gets.
async fn place(produced: &Path, options: &Options) -> Result<Finished, Error> {
    // yt-dlp names the file it meant to leave; if it is not there, the usual
    // cause is a join that never happened, and "No such file" says nothing.
    if !produced.is_file() {
        return Err(Error::Internal(
            "yt-dlp finished but the file it named is missing".into(),
        ));
    }
    let name = produced
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());

    /* Placed by the same code as every other download — the category
     * folder, a name nothing else has, a rename that falls back to a copy
     * across filesystems, the digest checked and a failing file removed.
     * This used to be a second copy of all of that, one rule at a time. */
    let (dest, bytes, category) =
        crate::download::finish(&name, None, &[produced.to_path_buf()], options).await?;
    let _ = tokio::fs::remove_dir_all(&options.parts_dir).await;

    Ok(Finished {
        left_behind: None,
        path: dest,
        bytes,
        // yt-dlp manages its own connections; the column would be guessing.
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 0,
    })
}

/// A cancelled run's staging folder, removed. Tried again for a few
/// seconds: on Windows a file still open by a process that is only now
/// ending cannot be deleted, and the first try left the folder behind.
async fn remove_staging(dir: &Path) {
    for attempt in 0..15 {
        match tokio::fs::remove_dir_all(dir).await {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) if attempt < 14 => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(_) => return,
        }
    }
}

/// Progress over every stream of a run, not each on its own.
///
/// Picture and sound are fetched one after the other, each counting from
/// zero, so the bar ran to the end of the picture and fell back to nothing
/// for the sound. Streams that have finished are carried in `before`.
#[derive(Default)]
struct Cumulative {
    before: u64,
    current: u64,
}

impl Cumulative {
    /// A stream has started: whatever the last one reached is behind us.
    fn next_stream(&mut self) {
        self.before += self.current;
        self.current = 0;
    }

    fn add(&mut self, progress: crate::Progress) -> crate::Progress {
        // A count that falls is a new stream, announced or not.
        if progress.downloaded < self.current {
            self.next_stream();
        }
        self.current = progress.downloaded;
        crate::Progress {
            downloaded: self.before + progress.downloaded,
            total: progress.total.map(|t| self.before + t),
            ..progress
        }
    }
}

/// Reads one progress line, `<downloaded> <total> <estimate>`.
///
/// yt-dlp writes `NA` where it does not know, and knows the total only for
/// some formats — an estimate stands in until then, and neither is fatal.
/// The estimate is a float (`1234.5`) more often than not, and read as one.
fn parse_progress(rest: &str) -> Option<crate::Progress> {
    let bytes = |s: &str| {
        s.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64)
    };
    let mut fields = rest.split_whitespace();
    let downloaded = fields.next().and_then(bytes)?;
    let total = fields.next().and_then(bytes);
    let estimate = fields.next().and_then(bytes);

    Some(crate::Progress {
        downloaded,
        total: total.or(estimate),
        // yt-dlp's own speed line is separate; dm's watcher is not on this
        // path, so speed and ETA are left for the numbers that are certain.
        speed: 0.0,
        eta: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{refusal, said_by};

    #[cfg(unix)]
    /// A stream whose renditions name no codecs, as yt-dlp
    /// reads an HLS master playlist without CODECS, still lists its heights.
    #[test]
    fn renditions_without_codecs_are_listed_by_height() {
        let described = serde_json::json!({ "formats": [
            { "format_id": "hls-360", "ext": "mp4", "height": 360, "width": 640, "vcodec": null, "acodec": null },
            { "format_id": "hls-720", "ext": "mp4", "height": 720, "width": 1280 },
            { "format_id": "a", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2" },
        ]})
        .to_string();
        let heights: Vec<String> = choices(&described).into_iter().map(|c| c.quality).collect();
        assert_eq!(heights, ["720/mp4", "360/mp4", "audio/m4a"]);
    }

    /// The tree's killer is waited for, but never for ever.
    #[test]
    fn the_trees_killer_is_waited_for_but_not_for_ever() {
        use std::time::{Duration, Instant};
        let mut quick = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        assert!(super::wait_at_most(&mut quick, Duration::from_secs(5)));

        let mut stuck = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let at = Instant::now();
        assert!(!super::wait_at_most(&mut stuck, Duration::from_millis(200)));
        assert!(at.elapsed() < Duration::from_secs(5), "waited on");
        assert!(stuck.try_wait().unwrap().is_some(), "killed and reaped");
    }

    #[tokio::test]
    async fn a_cancelled_runs_staging_is_removed_and_a_missing_one_is_no_wait() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("parts");
        std::fs::create_dir_all(staging.join("deeper")).unwrap();
        std::fs::write(staging.join("deeper/a.part"), b"x").unwrap();
        super::remove_staging(&staging).await;
        assert!(!staging.exists());
        let at = std::time::Instant::now();
        super::remove_staging(&staging).await;
        assert!(at.elapsed() < std::time::Duration::from_millis(150));
    }

    /// A reading being made is waited for only while it says so: a marker
    /// left by a host that was ended is let go of in seconds, not waited out.
    #[tokio::test]
    async fn a_stale_reading_marker_is_not_waited_for() {
        let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        super::keep_readings_in(dir.path().to_path_buf());
        let url = "https://v.example/watch?v=stale";
        let marker = super::reading_path(url).unwrap().with_extension("busy");
        let file = std::fs::File::create(&marker).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(30))
            .unwrap();
        drop(file);
        let at = std::time::Instant::now();
        super::wait_for_reading(url, &crate::control::Control::new())
            .await
            .unwrap();
        assert!(at.elapsed() < std::time::Duration::from_millis(300));
        // One touched just now is waited for (here, until the pause).
        std::fs::OpenOptions::new()
            .write(true)
            .open(&marker)
            .unwrap()
            .set_modified(std::time::SystemTime::now())
            .unwrap();
        let control = crate::control::Control::new();
        let held = control.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            held.pause();
        });
        let result = super::wait_for_reading(url, &control).await;
        assert!(matches!(result, Err(crate::Error::Paused)), "{result:?}");
    }

    /// A list asked for while a reading of the page is being made is that
    /// reading's, not a second run's: waited for, then read from it.
    #[tokio::test]
    async fn the_list_of_a_reading_being_made_is_read_from_it() {
        let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        super::keep_readings_in(dir.path().to_path_buf());
        let url = "https://v.example/watch?v=making";
        // Nothing made, nothing marked: no list.
        assert!(super::kept_choices(url).await.is_none());
        // Marked, and then made 400 ms later by another process.
        super::mark_reading(url);
        let reading = super::reading_path(url).unwrap();
        let writer = tokio::spawn({
            let reading = reading.clone();
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                let described = serde_json::json!({ "formats": [
                    { "format_id": "a", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 128, "filesize": 1000 },
                    { "format_id": "v", "ext": "mp4", "height": 720, "width": 1280, "vcodec": "avc1", "acodec": "none", "filesize": 9000 },
                ]});
                std::fs::write(&reading, described.to_string()).unwrap();
                let _ = std::fs::remove_file(reading.with_extension("busy"));
            }
        });
        let at = std::time::Instant::now();
        let list = super::kept_choices(url)
            .await
            .expect("no list from the reading");
        assert!(
            at.elapsed() >= std::time::Duration::from_millis(300),
            "did not wait"
        );
        let said: Vec<&str> = list.iter().map(|c| c.quality.as_str()).collect();
        // The sound as MP3 after it, where dm can make one.
        let mut expected = vec!["720/mp4", "audio/m4a"];
        if crate::mp3::possible() {
            expected.push("audio/mp3");
        }
        assert_eq!(said, expected);
        writer.await.unwrap();
    }

    #[test]
    fn a_sites_refusal_is_said_in_words_a_person_can_act_on() {
        let limited = "WARNING: [youtube] got redirected to https://www.google.com/sorry/index\n\
                       ERROR: [youtube] jNQXAC9IVRw: Unable to download webpage: HTTP Error 429: Too Many Requests";
        assert!(refusal(limited).unwrap().contains("try again later"));
        assert!(
            refusal("ERROR: Sign in to confirm you're not a bot. Use --cookies")
                .unwrap()
                .contains("not a bot")
        );
        assert!(
            refusal("ERROR: [vimeo] 1: The web client only works when logged-in")
                .unwrap()
                .contains("signed in")
        );
        assert_eq!(
            refusal("ERROR: [youtube] x: Private video").as_deref(),
            Some("that video is private")
        );
        assert!(refusal("ERROR: Unsupported URL: https://e.com/")
            .unwrap()
            .contains("does not know"));
        assert!(refusal(
            "ERROR: This format is DRM protected; Try selecting another format with --format"
        )
        .is_some_and(|said| said.contains("copy-protected")));
        assert_eq!(refusal("ERROR: something else entirely"), None);
        // YouTube's player script, which a cookie does not stand in for.
        for stderr in [
            "WARNING: [youtube] x: n challenge solving failed: Some formats may be missing.\nERROR: [youtube] x: Only images are available for download.",
            "WARNING: [youtube] No supported JavaScript runtime could be found.",
        ] {
            assert!(refusal(stderr).unwrap().contains("player script"), "{stderr}");
        }
        // A sign-in stays a sign-in even where the format is missing too.
        assert!(refusal(
            "ERROR: Sign in to confirm your age\nERROR: Requested format is not available"
        )
        .unwrap()
        .contains("signed in"));
        // Unknown refusals keep yt-dlp's own words, the site named, not a
        // bare failure; the video's id and yt-dlp's advice are left out.
        assert_eq!(
            said_by(
                "WARNING: x\nERROR: [e] 1: a thing went wrong; please report this issue on https://x\n",
                "could not read"
            ),
            "could not read on E (yt-dlp: a thing went wrong)"
        );
        assert_eq!(
            said_by("nothing useful", "could not read"),
            "could not read"
        );
    }

    use super::*;

    /// An installed yt-dlp is judged by the date its version names.
    #[test]
    fn a_yt_dlp_version_is_read_as_the_day_it_came_out() {
        assert_eq!(released_on("2000.03.01"), Some(11_017));
        assert_eq!(released_on("2026.08.19\n"), Some(20_684));
        assert_eq!(released_on("2026.08.19.232509"), Some(20_684));
        assert_eq!(released_on("2023.03.04"), Some(19_420));
        assert_eq!(released_on("stable@2026.08.19"), None);
        assert_eq!(released_on("1.2.3"), None);
    }

    #[test]
    fn the_list_offers_mp4_where_there_is_one_as_youtube_lists_webm_bigger() {
        // YouTube's real shape: VP9 WebM bigger than H.264 MP4 at a height,
        // Opus sound at a higher bitrate than M4A, and 4K only in WebM.
        let described = serde_json::json!({"formats": [
            {"ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.0, "filesize": 1_000},
            {"ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 140.0, "filesize": 1_100},
            {"ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1920, "height": 1080, "filesize": 9_000},
            {"ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 1920, "height": 1080, "filesize": 12_000},
            {"ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 3840, "height": 2160, "filesize": 40_000},
        ]})
        .to_string();
        let list = choices(&described);
        let labels: Vec<&str> = list.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["WEBM 3840×2160", "MP4 1920×1080", "M4A audio"]);
        assert_eq!(list[1].quality, "1080/mp4");
        assert_eq!(list[1].size, Some(10_000), "joined with the M4A sound");
    }

    #[test]
    fn a_pages_formats_become_one_choice_per_height_and_the_sound_alone() {
        let described = serde_json::json!({"formats": [
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.5, "filesize": 1_000},
            {"format_id": "249", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 50.0, "filesize": 400},
            {"format_id": "18", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "width": 640, "height": 360, "filesize": 5_000},
            {"format_id": "136", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720, "filesize": 9_000},
            {"format_id": "247", "ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 1280, "height": 720, "filesize_approx": 7_000},
            {"format_id": "sb0", "ext": "mhtml", "vcodec": "none", "acodec": "none", "height": 90},
        ]})
        .to_string();
        let list = choices(&described);
        let labels: Vec<&str> = list.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["MP4 1280×720", "MP4 640×360", "M4A audio"]);
        assert_eq!(list[0].quality, "720/mp4");
        assert_eq!(
            list[0].size,
            Some(10_000),
            "the picture and the sound it is joined with"
        );
        assert_eq!(list[1].size, Some(5_000), "already carries its sound");
        assert_eq!(list[2].quality, "audio/m4a");
        assert_eq!(Quality::named(&list[0].quality), Quality::UpTo(720));
        assert_eq!(Quality::named(&list[2].quality), Quality::Audio);
        assert_eq!(
            Quality::container_of(&list[0].quality).as_deref(),
            Some("mp4")
        );
        assert_eq!(Quality::container_of("720"), None);
        assert_eq!(Quality::container_of("720/../x"), None, "only a plain word");
        assert_eq!(
            container_sort(Some("mp4")),
            [
                "-S",
                "res,ext:mp4:m4a,vcodec:h264",
                "--merge-output-format",
                "mp4"
            ]
        );
        assert_eq!(
            container_sort(None),
            container_sort(Some("mp4")),
            "MP4 unless a line asked otherwise"
        );
        assert_eq!(container_sort(Some("webm")), ["-S", "ext:webm:webm"]);
        assert!(choices("not json").is_empty());
        assert!(choices("{}").is_empty());
    }

    #[test]
    fn a_shipped_yt_dlp_is_looked_for_beside_the_program_and_in_the_bundle() {
        let exe = if cfg!(target_os = "macos") {
            Path::new("/Applications/dm.app/Contents/MacOS/dm-desktop")
        } else if cfg!(windows) {
            Path::new(r"C:\Program Files\dm\dm-desktop.exe")
        } else {
            Path::new("/usr/bin/dm-desktop")
        };
        let places = bundled_in(exe);
        // This computer's unpacked build's own name first, beside the program.
        let own = crate::tools::ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH)
            .map(crate::tools::unpacked_program)
            .unwrap_or_else(|| "yt-dlp".into());
        let zip = crate::tools::ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH)
            .unwrap_or("yt-dlp");
        assert_eq!(places[0], exe.parent().unwrap().join("yt-dlp").join(zip));
        assert_eq!(places[1], exe.parent().unwrap().join("yt-dlp").join(&own));
        if cfg!(target_os = "macos") {
            assert!(places
                .iter()
                .any(|p| p.ends_with("Resources/yt-dlp/yt-dlp_macos")));
        }
        if cfg!(target_os = "linux") {
            assert!(places
                .iter()
                .any(|p| p.starts_with("/usr/bin/../lib/dm/yt-dlp")));
        }
        assert!(bundled_in(Path::new("dm-desktop"))
            .iter()
            .all(|p| !p.as_os_str().is_empty()));
    }

    #[test]
    fn a_quality_becomes_a_format_selector_dm_joins() {
        // The tallest picture and the best sound, joined by dm; a
        // picture-only stream (`bv`, never `bv*`), since one that carries
        // sound too would be joined with a second sound.
        assert_eq!(format_for(Quality::Best, None), "bv+ba/b");
        // A portrait video by its width first (its shorter side), then any
        // by its height.
        assert_eq!(
            format_for(Quality::UpTo(480), None),
            "bv[aspect_ratio<1][width<=480]+ba/bv[height<=480]+ba/b[aspect_ratio<1][width<=480]/b[height<=480]/b"
        );
        assert_eq!(
            format_for(Quality::UpTo(720), Some("webm")),
            "bv[ext=webm][aspect_ratio<1][width<=720]+ba[ext=webm]/bv[ext=webm][height<=720]+ba[ext=webm]/b[ext=webm][aspect_ratio<1][width<=720]/b[ext=webm][height<=720]/b[aspect_ratio<1][width<=720]/b[height<=720]/b"
        );
        assert_eq!(format_for(Quality::Audio, None), "ba/b");
        assert_eq!(
            format_for(Quality::Best, Some("webm")),
            "bv[ext=webm]+ba[ext=webm]/b[ext=webm]/b"
        );
        // A container dm does not join into: one whole file of it first.
        assert_eq!(format_for(Quality::Best, Some("flv")), "b[ext=flv]/bv+ba/b");
        assert!(joined_by_dm("mp4") && joined_by_dm("webm") && joined_by_dm(""));
        assert!(!joined_by_dm("mkv"));
    }

    #[test]
    fn progress_reads_bytes_and_falls_back_to_the_estimate() {
        let p = parse_progress("500 1000 1000").unwrap();
        assert_eq!(p.downloaded, 500);
        assert_eq!(p.total, Some(1000));

        // Total unknown, estimate present: the estimate stands in.
        let p = parse_progress("500 NA 2000").unwrap();
        assert_eq!(p.total, Some(2000));

        // Neither known yet: a count with no total is still honest.
        let p = parse_progress("500 NA NA").unwrap();
        assert_eq!(p.downloaded, 500);
        assert_eq!(p.total, None);
    }

    #[test]
    fn an_estimate_written_as_a_float_is_read() {
        let p = parse_progress("500 NA 2048.75").unwrap();
        assert_eq!(p.total, Some(2048));
        let p = parse_progress("500.0 NA NA").unwrap();
        assert_eq!(p.downloaded, 500);
        assert!(parse_progress("NaN NA NA").is_none());
        assert!(parse_progress("-1 NA NA").is_none());
    }

    #[test]
    fn progress_over_picture_then_sound_never_goes_back() {
        let mut sum = Cumulative::default();
        let seen: Vec<u64> = ["1000 2000 NA", "2000 2000 NA", "0 500 NA", "500 500 NA"]
            .iter()
            .map(|l| sum.add(parse_progress(l).unwrap()).downloaded)
            .collect();
        assert_eq!(seen, [1000, 2000, 2000, 2500]);
        // Announced by a Destination line, the second stream counts on too.
        let mut sum = Cumulative::default();
        sum.add(parse_progress("2000 2000 NA").unwrap());
        sum.next_stream();
        let p = sum.add(parse_progress("100 500 NA").unwrap());
        assert_eq!((p.downloaded, p.total), (2100, Some(2500)));
    }

    #[test]
    fn a_line_without_the_numbers_is_ignored() {
        assert!(parse_progress("not numbers here").is_none());
    }
}

#[cfg(test)]
mod windows_homes_tests {
    use super::{find_in, names, windows_homes, Homes};
    use std::path::PathBuf;

    /// On Windows the program is `yt-dlp.exe`, installed where
    /// pip, winget, Scoop or Chocolatey put it, none of them on the `PATH`
    /// a dm started by a browser has. Each is found there, laid out as a
    /// Windows profile is under a folder of this test's.
    #[test]
    fn windows_installs_are_found_by_their_exe() {
        let root = std::env::temp_dir().join(format!("dm-win-homes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (local, roaming, home, data) = (
            root.join("Local"),
            root.join("Roaming"),
            root.join("me"),
            root.join("ProgramData"),
        );
        let homes = Homes {
            local: Some(local.clone()),
            roaming: Some(roaming.clone()),
            home: home.clone(),
            program_data: Some(data.clone()),
            program_files: None,
            system_drive: None,
        };
        let put = |dir: PathBuf, name: &str| {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), b"MZ").unwrap();
            dir.join(name)
        };
        let exe = names("yt-dlp", true);
        assert_eq!(exe[0], "yt-dlp.exe");
        let find = |h: &Homes| find_in(windows_homes(h).into_iter(), &exe);
        assert_eq!(find(&homes), None);

        // pip install --user, with python.org's Python 3.13.
        let pip = put(
            roaming.join("Python").join("Python313").join("Scripts"),
            "yt-dlp.exe",
        );
        assert_eq!(find(&homes), Some(pip.clone()));
        std::fs::remove_file(&pip).unwrap();
        // winget's link, then its package folder alone.
        let winget = local.join("Microsoft").join("WinGet");
        let link = put(winget.join("Links"), "yt-dlp.exe");
        assert_eq!(find(&homes), Some(link.clone()));
        std::fs::remove_file(&link).unwrap();
        let package = put(
            winget.join("Packages").join("yt-dlp.yt-dlp_8wekyb3d8bbwe"),
            "yt-dlp.exe",
        );
        assert_eq!(find(&homes), Some(package.clone()));
        std::fs::remove_file(&package).unwrap();
        // Scoop, then Chocolatey.
        let scoop = put(home.join("scoop").join("shims"), "yt-dlp.exe");
        assert_eq!(find(&homes), Some(scoop.clone()));
        std::fs::remove_file(&scoop).unwrap();
        let choco = put(data.join("chocolatey").join("bin"), "yt-dlp.exe");
        assert_eq!(find(&homes), Some(choco));

        // Elsewhere the bare name, as before.
        assert_eq!(names("yt-dlp", false), ["yt-dlp"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod noexec_tests {
    use super::*;

    const MOUNTS: &str = "\
/dev/sda2 / btrfs rw,noatime 0 0
tmpfs /tmp tmpfs rw,nosuid,nodev,noexec,relatime,size=4194304k 0 0
tmpfs /run tmpfs rw,nosuid,nodev,noexec 0 0
/dev/sda3 /home btrfs rw,noatime 0 0
/dev/sdb1 /mnt/my\\040disk ext4 rw,noexec 0 0
tmpfs /tmp2 tmpfs rw,noexec 0 0
";

    fn noexec(path: &str) -> bool {
        on_noexec_mount(MOUNTS, Path::new(path))
    }

    /// The system's temporary folder as hardened systems mount it: nothing
    /// unpacked into it can be run.
    #[test]
    fn a_folder_under_a_noexec_mount_is_noexec() {
        assert!(noexec("/tmp"));
        assert!(noexec("/tmp/_MEI123/libz.so.1"));
        assert!(noexec("/run/user/1000"));
    }

    /// Where programs may run, nothing is changed.
    #[test]
    fn a_folder_on_an_ordinary_mount_is_not() {
        assert!(!noexec("/home/me/.local/share/dm-engine/tools"));
        assert!(!noexec("/var/tmp"));
    }

    /// The deepest mount point decides, by whole folders: `/tmp2` is another
    /// mount and says nothing of `/tmp`, and a folder under `/` that is not
    /// under a mount of its own follows `/`.
    #[test]
    fn the_deepest_mount_decides_by_whole_folders() {
        let mounts = "/dev/sda2 / ext4 rw,noexec 0 0\ntmpfs /tmp tmpfs rw,exec 0 0\n";
        assert!(!on_noexec_mount(mounts, Path::new("/tmp/x")));
        assert!(on_noexec_mount(mounts, Path::new("/var/tmp/x")));
        let near = "/dev/sda2 / ext4 rw 0 0\ntmpfs /tmp2 tmpfs rw,noexec 0 0\n";
        assert!(!on_noexec_mount(near, Path::new("/tmp/x")));
    }

    /// A mount point with a space in it is written with an escape.
    #[test]
    fn an_escaped_mount_point_is_read_back() {
        assert!(noexec("/mnt/my disk/video"));
    }

    /// A table with nothing in it, or not one, is no reason to change how
    /// yt-dlp is run.
    #[test]
    fn no_table_is_no_noexec() {
        assert!(!on_noexec_mount("", Path::new("/tmp")));
        assert!(!on_noexec_mount("garbage", Path::new("/tmp")));
    }
}

#[cfg(test)]
mod start_words {
    #[test]
    fn a_helper_that_will_not_start_says_why() {
        let path = std::path::Path::new("/tools/yt-dlp");
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(super::would_not_start(path, &denied).contains("will not let it run"));
        let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(super::would_not_start(path, &gone).contains("fetches it again"));
        let arch = std::io::Error::from_raw_os_error(86);
        assert!(super::would_not_start(path, &arch).contains("another kind of processor"));
    }
}
