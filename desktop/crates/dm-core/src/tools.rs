//! yt-dlp (and QuickJS, which it runs a site's player checks with), fetched by
//! dm itself where the computer has none, so a video site's page downloads
//! with nothing installed by hand. Nothing else is fetched: dm joins
//! picture and sound and records streams itself (`mp4mux`, `mkvmux`,
//! `tsmux`, `rtmp`, `rtsp`, `mms`).
//!
//! dm still writes no site logic of its own: it runs yt-dlp, as it always
//! has. What changes is where that yt-dlp comes from. One the person
//! installed is used first, wherever it is (`ytdlp::which`), unless it is
//! months old (`ytdlp::binary`); with none, the first video page that
//! needs it has dm fetch yt-dlp's own standalone build from yt-dlp's
//! GitHub releases, checked against the `SHA2-256SUMS` published beside
//! it, and a pinned QuickJS build checked against the digests written into
//! dm. Both go into dm's own `tools` folder, through dm's proxy like any
//! download, and yt-dlp is fetched afresh there once a week (`freshen`):
//! sites change, and an old yt-dlp stops working on them.

// Reads outside input (a checksums file): every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use futures_util::StreamExt;
use sha2::{Digest as _, Sha256};

use crate::Error;

/// The folder dm keeps the tools it fetched in, once the app or the host
/// has said where dm's home is. Unset (a library user, a test), nothing is
/// fetched.
static FOLDER: OnceLock<PathBuf> = OnceLock::new();

/// Says where fetched tools live: `<dm home>/tools`.
pub fn set_folder(folder: PathBuf) {
    let _ = FOLDER.set(folder);
}

/// Whether dm may fetch its video helper at all (Options › Downloads). Off,
/// only a yt-dlp the person installed is used.
static ALLOWED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Lets dm fetch its video helper, or not.
pub fn allow_fetching(allowed: bool) {
    ALLOWED.store(allowed, std::sync::atomic::Ordering::Relaxed);
}

fn fetching_allowed() -> bool {
    ALLOWED.load(std::sync::atomic::Ordering::Relaxed)
}

/// What a video page is told where fetching is turned off.
pub const NOT_FETCHED: &str = "dm's video helper is not fetched, as Options › Downloads says; install yt-dlp, or let dm fetch it there";

/// The fetched tools' folder, if dm has one.
pub fn folder() -> Option<&'static Path> {
    FOLDER.get().map(PathBuf::as_path)
}

/// One fetch of each tool at a time: two downloads needing yt-dlp at once
/// wait for the one fetch; yt-dlp and QuickJS come down side by side.
static FETCHING: [tokio::sync::Mutex<()>; 2] = [
    tokio::sync::Mutex::const_new(()),
    tokio::sync::Mutex::const_new(()),
];

/// How many tool fetches are under way, for the window to say why a video
/// waits the first time ("Getting ready for videos") rather than
/// "Starting…" for the half-minute the fetch takes.
static PREPARING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whether dm is fetching a tool a video needs, just now.
pub fn preparing() -> bool {
    PREPARING.load(std::sync::atomic::Ordering::Relaxed) > 0
}

/// Counts a fetch for as long as it lasts, however it ends.
struct Preparing;

impl Preparing {
    fn new() -> Self {
        PREPARING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Preparing
    }
}

impl Drop for Preparing {
    fn drop(&mut self) {
        PREPARING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn lock_of(tool: Tool) -> &'static tokio::sync::Mutex<()> {
    match tool {
        Tool::YtDlp => &FETCHING[0],
        Tool::QuickJs => &FETCHING[1],
    }
}

/// A tool dm can fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    YtDlp,
    /// The JavaScript engine yt-dlp answers a site's player challenges
    /// with; without one, YouTube gives it too little to download. QuickJS:
    /// 1–3 MB where Deno is 85, and the same formats at the same speed.
    QuickJs,
}

impl Tool {
    fn program(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::QuickJs => "qjs",
        }
    }

    /// Its file in the tools folder.
    fn file_name(self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.program())
        } else {
            self.program().to_string()
        }
    }
}

/// Where each comes from. Fixed in dm; a test's server stands in for them.
#[derive(Debug, Clone)]
pub struct Sources {
    /// yt-dlp's newest release, its files under this address.
    pub ytdlp: String,
    /// The pinned QuickJS release, its files under this address.
    pub quickjs: String,
    /// The pinned release's digest of each of its builds, by file name: it
    /// publishes no checksums file, and these are GitHub's record of them.
    pub quickjs_digests: Vec<(String, String)>,
}

/// yt-dlp's own newest release, and QuickJS-ng 0.17.0. `DM_YTDLP_RELEASES`
/// names another address for yt-dlp's files: a test's server standing in
/// for GitHub.
pub fn official() -> Sources {
    Sources {
        ytdlp: std::env::var("DM_YTDLP_RELEASES")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "https://github.com/yt-dlp/yt-dlp/releases/latest/download".into()),
        quickjs: "https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0".into(),
        quickjs_digests: [
            (
                "qjs-darwin-arm64",
                "8be3ddfe3397d2e692e4e1e8972ee9d032a0a580505d2f8b4ea528cf1b651c11",
            ),
            (
                "qjs-darwin-x86_64",
                "9e5e101b4fd13cda3204222ca9f8be35412c41dcdef3745829633b7a67245412",
            ),
            (
                "qjs-linux-aarch64",
                "3372133484edf50a69f3c67903af41206d22a061e930e3cfb63269272ef56d2e",
            ),
            (
                "qjs-linux-x86_64",
                "0bfc02511a9f549c28b53880d988fc7cd5d361e90c5e8afdfcd7dc6774ceace5",
            ),
            (
                "qjs-windows-x86_64.exe",
                "2aeabf0092c3262d6b2609824418f7dd7ed1f1df939f73b2b15645230cac0d77",
            ),
        ]
        .iter()
        .map(|(n, d)| ((*n).to_string(), (*d).to_string()))
        .collect(),
    }
}

/// yt-dlp's standalone build for an OS and processor, by its release's
/// file name.
pub fn ytdlp_asset(os: &str, arch: &str) -> Option<&'static str> {
    ytdlp_asset_for(os, arch, false)
}

/// [`ytdlp_asset`] for a Linux whose C library is musl where `musl` (Alpine
/// and the like), which the glibc build does not run on.
///
/// The unpacked builds, every one: the single-file build unpacks itself at
/// every start, 8 to 14 seconds on macOS where the unpacked one takes 0.3.
pub fn ytdlp_asset_for(os: &str, arch: &str, musl: bool) -> Option<&'static str> {
    Some(match (os, arch, musl) {
        ("windows", "x86_64", _) => "yt-dlp_win.zip",
        ("windows", "aarch64", _) => "yt-dlp_win_arm64.zip",
        ("windows", "x86", _) => "yt-dlp_win_x86.zip",
        ("macos", _, _) => "yt-dlp_macos.zip",
        ("linux", "x86_64", false) => "yt-dlp_linux.zip",
        ("linux", "aarch64", false) => "yt-dlp_linux_aarch64.zip",
        ("linux", "x86_64", true) => "yt-dlp_musllinux.zip",
        ("linux", "aarch64", true) => "yt-dlp_musllinux_aarch64.zip",
        _ => return None,
    })
}

/// Whether this Linux runs on musl: its loader is in `/lib` under a musl
/// name, which glibc's is not.
fn on_musl() -> bool {
    cfg!(target_os = "linux")
        && std::fs::read_dir("/lib")
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("ld-musl-"))
}

/// The program inside an unpacked build's folder: named as its archive is,
/// except Windows's, which is `yt-dlp.exe`.
pub(crate) fn unpacked_program(asset: &str) -> String {
    if asset.starts_with("yt-dlp_win") {
        "yt-dlp.exe".into()
    } else {
        asset.trim_end_matches(".zip").to_string()
    }
}

/// QuickJS's build for an OS and processor, by its release's file name.
/// Windows on Arm runs the x86-64 build, as it runs any.
pub fn quickjs_asset(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("macos", "aarch64") => "qjs-darwin-arm64",
        ("macos", "x86_64") => "qjs-darwin-x86_64",
        ("linux", "x86_64") => "qjs-linux-x86_64",
        ("linux", "aarch64") => "qjs-linux-aarch64",
        ("windows", _) => "qjs-windows-x86_64.exe",
        _ => return None,
    })
}

/// The digest a `SHA2-256SUMS` file gives `name`: lines of a hex digest,
/// space(s), then the file's name (a `*` before it marks binary).
pub fn digest_in(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| digest.to_ascii_lowercase())
    })
}

/// The most a fetched file may be: yt-dlp's builds are under 50 MB,
/// QuickJS's under 5.
const MOST: usize = 160 * 1024 * 1024;

/// `url`'s body, tried three times: a dropped connection or a server's
/// passing trouble should not cost a person their first video.
async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, Error> {
    let mut tries = 0u64;
    loop {
        tries += 1;
        match fetch_once(client, url).await {
            Ok(body) => return Ok(body),
            Err(e) if tries < 3 && crate::retry::transient(&e) => {
                tokio::time::sleep(std::time::Duration::from_secs(tries)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// What is being fetched right now: each file's bytes so far and its length
/// where the server said it, for the row's status to say (`preparing_progress`).
static PROGRESS: std::sync::Mutex<Vec<(String, u64, Option<u64>)>> =
    std::sync::Mutex::new(Vec::new());

/// The bytes so far of the tools being fetched, and their length together
/// where every server said it; `None` while nothing is being fetched.
pub fn preparing_progress() -> Option<(u64, Option<u64>)> {
    let fetching = crate::lock(&PROGRESS);
    if fetching.is_empty() {
        return None;
    }
    let got = fetching.iter().map(|(_, got, _)| got).sum();
    let total = fetching
        .iter()
        .map(|(_, _, total)| *total)
        .sum::<Option<u64>>();
    Some((got, total))
}

/// One file's place in [`PROGRESS`] while it is fetched, gone when it is
/// done or failed.
struct Counting(String);

impl Counting {
    fn start(url: &str, total: Option<u64>) -> Counting {
        crate::lock(&PROGRESS).push((url.to_string(), 0, total));
        Counting(url.to_string())
    }

    fn got(&self, bytes: u64) {
        if let Some(entry) = crate::lock(&PROGRESS)
            .iter_mut()
            .find(|(u, ..)| *u == self.0)
        {
            entry.1 = bytes;
        }
    }
}

impl Drop for Counting {
    fn drop(&mut self) {
        crate::lock(&PROGRESS).retain(|(u, ..)| *u != self.0);
    }
}

async fn fetch_once(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, Error> {
    let res = crate::redirects::send(client.get(url)).await?;
    if !res.status().is_success() {
        return Err(Error::Refused {
            status: res.status().as_u16(),
            url: url.to_string(),
            body: String::new(),
            retry_after: None,
        });
    }
    let counting = Counting::start(url, res.content_length());
    let mut body = Vec::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > MOST {
            return Err(Error::Internal(format!(
                "{url} is larger than any build of it"
            )));
        }
        body.extend_from_slice(&chunk);
        counting.got(body.len() as u64);
    }
    Ok(body)
}

fn hex_of(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The SHA-256 digest of `bytes`, in hex: what a fetched file is held to.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex_of(bytes)
}

/// Fetches a small file of a tool's, with a tool's retries.
pub(crate) async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, Error> {
    fetch(client, url).await
}

/// Writes `bytes` as an executable at `path`, whole or not at all.
async fn place(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let partial = path.with_extension("partial");
    tokio::fs::write(&partial, bytes).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755)).await?;
    }
    tokio::fs::rename(&partial, path).await?;
    Ok(())
}

/// The folder an unpacked yt-dlp is kept in, under the tools folder: one for
/// each system, the macOS one named as it was when macOS was the only one.
fn unpacked_dir(os: &str) -> &'static str {
    match os {
        "macos" => "yt-dlp-macos",
        "windows" => "yt-dlp-windows",
        _ => "yt-dlp-linux",
    }
}

/// The yt-dlp shipped inside dm's own package (`bundled`, the program in
/// its unpacked folder) copied into dm's tools folder, where a fetched one
/// would be, and that copy's program returned: whole or not at all, once.
///
/// The copy is dm's own writing, so it carries no quarantine mark however
/// dm itself arrived, and it is where dm's updates replace it later; the
/// shipped folder is never written to (an installed app's is read-only, or
/// is translocated). `None` where there is no tools folder or the copy
/// fails, and the caller runs the shipped one in place.
pub(crate) fn adopt(bundled: &Path) -> Option<PathBuf> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = crate::lock(&ONE);
    if let Some(done) = fetched("yt-dlp") {
        return Some(done);
    }
    let folder = folder()?;
    let into = folder.join(unpacked_dir(std::env::consts::OS));
    // A staging folder of this process's own: another dm adopting at the
    // same moment (a page reading started by the browser) cannot mix into it.
    let staging = into.with_extension(format!("adopting-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(folder).ok()?;
    if bundled.extension().is_some_and(|e| e == "zip") {
        // The build as its release ships it, unpacked here.
        let program =
            ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH).map(unpacked_program)?;
        let unpacked = if cfg!(target_os = "macos") {
            std::process::Command::new("/usr/bin/ditto")
                .args(["-x", "-k"])
                .arg(bundled)
                .arg(&staging)
                .status()
                .is_ok_and(|s| s.success())
                && unquarantined(&staging)
        } else {
            std::fs::read(bundled)
                .map_err(|e| e.to_string())
                .and_then(|zip| unpack_portable(&zip, &staging))
                .is_ok()
        };
        if !unpacked || !staging.join(&program).is_file() {
            let _ = std::fs::remove_dir_all(&staging);
            return None;
        }
        let _ = std::fs::remove_dir_all(&into);
        if std::fs::rename(&staging, &into).is_err() {
            let _ = std::fs::remove_dir_all(&staging);
            return None;
        }
        return fetched("yt-dlp");
    }
    let source = bundled.parent()?;
    let copied = if cfg!(target_os = "macos") {
        // `ditto` with no extended attributes: the signatures are in the
        // files themselves, and the quarantine mark a browser put on a
        // downloaded dm (which `--noqtn` alone kept, seen 2026-10-30) is
        // left behind, so macOS never stops the copy to ask.
        std::process::Command::new("/usr/bin/ditto")
            .args(["--noextattr", "--noqtn"])
            .arg(source)
            .arg(&staging)
            .status()
            .is_ok_and(|s| s.success())
            && unquarantined(&staging)
    } else {
        copy_tree(source, &staging).is_ok()
    };
    let name = bundled.file_name()?;
    if !copied || !staging.join(name).is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return None;
    }
    let _ = std::fs::remove_dir_all(&into);
    if std::fs::rename(&staging, &into).is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        return None;
    }
    fetched("yt-dlp")
}

/// Whether nothing under `dir` still carries macOS's quarantine mark,
/// after removing any that does.
fn unquarantined(dir: &Path) -> bool {
    let _ = std::process::Command::new("/usr/bin/xattr")
        .args(["-d", "-r", "-s", "com.apple.quarantine"])
        .arg(dir)
        .stderr(std::process::Stdio::null())
        .status();
    std::process::Command::new("/usr/bin/xattr")
        .args(["-r", "-l", "-s"])
        .arg(dir)
        .output()
        .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).contains("com.apple.quarantine"))
}

/// `from` copied whole into `to`, file modes kept.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Whether `path` is a copy dm fetched (under its tools folder).
pub(crate) fn is_fetched(path: &Path) -> bool {
    folder().is_some_and(|folder| path.starts_with(folder))
}

/// The copy of `program` dm fetched, if there is one: an unpacked yt-dlp
/// first, else the single file an earlier dm fetched.
pub(crate) fn fetched(program: &str) -> Option<PathBuf> {
    let folder = folder()?;
    let os = std::env::consts::OS;
    let mut candidates = Vec::new();
    if program == "yt-dlp" {
        if let Some(asset) = ytdlp_asset_for(os, std::env::consts::ARCH, on_musl()) {
            candidates.push(folder.join(unpacked_dir(os)).join(unpacked_program(asset)));
        }
    }
    candidates.push(folder.join(if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    }));
    candidates.into_iter().find(|c| c.is_file())
}

/// Unpacks a zip into `into`, whole or not at all: into a folder beside it
/// first, which takes `into`'s place only once every file is out.
///
/// macOS's own `ditto` there, which keeps what a signed program's files
/// carry; the `zip` crate elsewhere (`unpack_portable`).
async fn unpack(zip: &[u8], into: &Path) -> Result<(), Error> {
    let parent = into.parent().unwrap_or(into);
    tokio::fs::create_dir_all(parent).await?;
    let staging = into.with_extension("partial");
    let _ = tokio::fs::remove_dir_all(&staging).await;
    let unpacked = if cfg!(target_os = "macos") {
        let archive = into.with_extension("zip.partial");
        tokio::fs::write(&archive, zip).await?;
        let status = tokio::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&archive)
            .arg(&staging)
            .status()
            .await;
        let _ = tokio::fs::remove_file(&archive).await;
        status.is_ok_and(|s| s.success())
    } else {
        let (zip, staging) = (zip.to_vec(), staging.clone());
        tokio::task::spawn_blocking(move || unpack_portable(&zip, &staging))
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
            .is_ok()
    };
    if !unpacked {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(Error::Internal(
            "yt-dlp's build could not be unpacked".into(),
        ));
    }
    let _ = tokio::fs::remove_dir_all(into).await;
    tokio::fs::rename(&staging, into).await?;
    Ok(())
}

/// The most an unpacked yt-dlp may come to: the builds are under 100 MB.
const MOST_UNPACKED: u64 = 600 * 1024 * 1024;

/// A zip's files written under `into`, with the permissions it records.
/// A name that would leave `into` is refused, and so is an archive that
/// unpacks to more than any build does.
pub(crate) fn unpack_portable(zip: &[u8], into: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| e.to_string())?;
    let total: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index_raw(i).ok().map(|f| f.size()))
        .sum();
    if total > MOST_UNPACKED {
        return Err("the archive unpacks to more than any yt-dlp does".into());
    }
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file
            .enclosed_name()
            .ok_or_else(|| "a name in the archive leaves the folder".to_string())?;
        let target = into.join(name);
        if file.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o777))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Fetches `tool` into `folder` from `sources`, checked, for this OS.
pub async fn fetch_into(
    client: &reqwest::Client,
    tool: Tool,
    folder: &Path,
    sources: &Sources,
    os: &str,
    arch: &str,
) -> Result<PathBuf, Error> {
    let path = folder.join(tool.file_name());
    match tool {
        Tool::YtDlp => {
            let sums = fetch(client, &format!("{}/SHA2-256SUMS", sources.ytdlp)).await?;
            let sums = String::from_utf8_lossy(&sums);
            /* The standalone build for this computer, where the release has
             * one. Where it has none (Linux on another processor, a BSD),
             * yt-dlp's portable build, one file of Python, run by the
             * computer's own Python where one is installed (gaps.md row 38). */
            let standalone =
                ytdlp_asset_for(os, arch, on_musl()).filter(|a| digest_in(&sums, a).is_some());
            let Some(asset) = standalone else {
                let Some(python) = crate::ytdlp::python3() else {
                    return Err(Error::Internal(format!(
                        "yt-dlp publishes no build for {os} on {arch}, and no Python 3.10 or \
                         newer is installed to run its portable one: install yt-dlp, or Python"
                    )));
                };
                let expected = digest_in(&sums, PORTABLE).ok_or_else(|| {
                    Error::Internal("yt-dlp's checksums name no portable build".into())
                })?;
                let bytes = fetch(client, &format!("{}/{PORTABLE}", sources.ytdlp)).await?;
                if hex_of(&bytes) != expected {
                    return Err(Error::Internal(
                        "the yt-dlp fetched does not match its published checksum, so it was not kept"
                            .into(),
                    ));
                }
                let path = folder.join(PORTABLE);
                place(&path, &bytes).await?;
                let _ = tokio::fs::write(folder.join(DIGEST_NOTE), &expected).await;
                let _ = tokio::fs::write(
                    folder.join(RUN_WITH_PYTHON),
                    python.as_os_str().as_encoded_bytes(),
                )
                .await;
                return Ok(path);
            };
            let expected = digest_in(&sums, asset)
                .ok_or_else(|| Error::Internal(format!("yt-dlp's checksums name no {asset}")))?;
            let bytes = fetch(client, &format!("{}/{asset}", sources.ytdlp)).await?;
            if hex_of(&bytes) != expected {
                return Err(Error::Internal(
                    "the yt-dlp fetched does not match its published checksum, so it was not kept"
                        .into(),
                ));
            }
            if asset.ends_with(".zip") {
                let dir = folder.join(unpacked_dir(os));
                unpack(&bytes, &dir).await?;
                let program = dir.join(unpacked_program(asset));
                if !program.is_file() {
                    return Err(Error::Internal(
                        "the yt-dlp unpacked has not the program it should".into(),
                    ));
                }
                // The single file an earlier dm fetched is no longer wanted.
                let _ = tokio::fs::remove_file(&path).await;
                // Which release this is, by its published digest: what
                // `newer_ytdlp` compares the next release's against.
                let _ = tokio::fs::write(folder.join(DIGEST_NOTE), &expected).await;
                return Ok(program);
            }
            place(&path, &bytes).await?;
        }
        Tool::QuickJs => {
            let asset = quickjs_asset(os, arch).ok_or_else(|| {
                Error::Internal(format!("QuickJS publishes no build for {os} on {arch}"))
            })?;
            let expected = sources
                .quickjs_digests
                .iter()
                .find(|(name, _)| name == asset)
                .map(|(_, digest)| digest.clone())
                .ok_or_else(|| Error::Internal(format!("no QuickJS digest for {asset}")))?;
            let bytes = fetch(client, &format!("{}/{asset}", sources.quickjs)).await?;
            if hex_of(&bytes) != expected {
                return Err(Error::Internal(
                    "the QuickJS fetched does not match the build dm knows, so it was not kept"
                        .into(),
                ));
            }
            place(&path, &bytes).await?;
        }
    }
    Ok(path)
}

/// `tool`, found or fetched. Found first (one the person installed, or one
/// dm fetched before); fetched only where dm has a tools folder and no
/// test or person has named the program outright (`DM_YTDLP`, `DM_QJS`,
/// which is how a test pins down what dm does without one).
pub async fn ensure(client: &reqwest::Client, tool: Tool) -> Result<PathBuf, Error> {
    let find = || match tool {
        Tool::YtDlp => crate::ytdlp::binary(),
        Tool::QuickJs => crate::ytdlp::quickjs(),
    };
    if let Some(found) = find() {
        return Ok(found);
    }
    if !fetching_allowed() {
        return Err(Error::Internal(NOT_FETCHED.into()));
    }
    let named = format!(
        "DM_{}",
        tool.program().replace('-', "").to_ascii_uppercase()
    );
    let Some(folder) = folder().filter(|_| std::env::var_os(named).is_none()) else {
        return Err(Error::Internal(format!(
            "dm's video helper ({}) is not available here",
            tool.program()
        )));
    };
    let _preparing = Preparing::new();
    let _one = lock_of(tool).lock().await;
    if let Some(found) = find() {
        return Ok(found);
    }
    fetch_into(
        client,
        tool,
        folder,
        &official(),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .await
}

/// yt-dlp for a video page: found or fetched, and with it a JavaScript
/// engine (so yt-dlp can answer YouTube's player challenges; without one it
/// gets little or nothing): a Deno the person installed, else QuickJS,
/// found or fetched. They come down at once. Picture and sound
/// dm joins itself (`mp4mux`, `mkvmux`). Says why not, where yt-dlp cannot
/// be had.
pub async fn for_video(client: &reqwest::Client) -> Result<(), String> {
    let engine = async {
        if crate::ytdlp::deno().is_none() {
            let _ = ensure(client, Tool::QuickJs).await;
        }
    };
    let (ytdlp, ()) = tokio::join!(ensure(client, Tool::YtDlp), engine);
    ytdlp.map(|_| ()).map_err(|e| {
        format!(
            "dm's video helper could not be fetched from yt-dlp's releases on GitHub ({e}): \
                 check the connection or the proxy in Options › Proxy, or install yt-dlp yourself"
        )
    })
}

/// Whether yt-dlp is there, or dm would fetch it when a page needs it.
pub fn video_possible() -> bool {
    crate::ytdlp::available()
        || (folder().is_some()
            && fetching_allowed()
            && std::env::var_os("DM_YTDLP").is_none()
            && (ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH).is_some()
                || crate::ytdlp::python3().is_some()))
}

/// Where the published digest of the yt-dlp dm fetched is noted, beside it.
const DIGEST_NOTE: &str = "yt-dlp.digest";

/// yt-dlp's portable build in its release: one file of Python, run by any
/// Python 3.10 or newer, for the computers the standalone builds leave out.
const PORTABLE: &str = "yt-dlp";

/// Noted beside a fetched portable build: the Python it is run with. Its
/// presence is how dm knows to start that build through Python rather
/// than as a program of its own.
const RUN_WITH_PYTHON: &str = "yt-dlp.python";

/// The Python a fetched portable yt-dlp is run with, where that is what was
/// fetched (`RUN_WITH_PYTHON` beside it); `None` for a program run as itself.
pub(crate) fn python_for(binary: &Path) -> Option<PathBuf> {
    let folder = binary.parent()?;
    if binary.file_name()? != PORTABLE || !is_fetched(binary) {
        return None;
    }
    let noted = std::fs::read(folder.join(RUN_WITH_PYTHON)).ok()?;
    let python = PathBuf::from(String::from_utf8_lossy(&noted).trim());
    python
        .is_file()
        .then_some(python)
        .or_else(crate::ytdlp::python3)
}

/// How long after one try for a newer yt-dlp the next may be made: a site
/// that stays broken must not have dm asking GitHub at every download.
const HEAL_EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

static LAST_HEAL: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// A newer yt-dlp than the one dm fetched, fetched now if its release has
/// one: `true` when a different build is in place and worth trying again
/// with. For a yt-dlp run that failed in words that are not a site's
/// refusal: a site has changed and yt-dlp's fix is usually out within
/// days, which the weekly `freshen` left a person waiting for
/// (docs/gaps.md row 33). Nothing where fetching is off, where the yt-dlp
/// in use is one the person installed, or within an hour of the last try.
pub async fn newer_ytdlp(client: &reqwest::Client) -> bool {
    let Some(folder) = folder().filter(|_| fetching_allowed()) else {
        return false;
    };
    if !crate::ytdlp::binary().is_some_and(|b| is_fetched(&b)) {
        return false;
    }
    {
        let mut last = crate::lock(&LAST_HEAL);
        if last.is_some_and(|at| at.elapsed() < HEAL_EVERY) {
            return false;
        }
        *last = Some(std::time::Instant::now());
    }
    let os = std::env::consts::OS;
    // The portable build's digest where that is what this computer runs.
    let asset = ytdlp_asset_for(os, std::env::consts::ARCH, on_musl()).unwrap_or(PORTABLE);
    let sources = official();
    let Ok(sums) = fetch(client, &format!("{}/SHA2-256SUMS", sources.ytdlp)).await else {
        return false;
    };
    let published = digest_in(&String::from_utf8_lossy(&sums), asset);
    let noted = std::fs::read_to_string(folder.join(DIGEST_NOTE)).ok();
    if published.is_some() && published == noted {
        return false;
    }
    let _one = lock_of(Tool::YtDlp).lock().await;
    fetch_into(
        client,
        Tool::YtDlp,
        folder,
        &sources,
        os,
        std::env::consts::ARCH,
    )
    .await
    .is_ok()
}

/// Keeps a yt-dlp dm fetched current: once it is a week old, fetched
/// afresh, checked as the first was, and swapped in whole (a fetch that
/// fails leaves the one there). One the person installed is theirs to
/// update; sites change, and an old yt-dlp stops working on them.
pub async fn freshen(client: reqwest::Client) {
    let Some(folder) = folder().filter(|_| fetching_allowed()) else {
        return;
    };
    let Some(current) = fetched("yt-dlp") else {
        /* A first start with nothing to run videos with, neither installed
         * nor fetched: fetched now, in the background, so the first video
         * finds it there rather than waiting half a minute on it (gaps.md
         * row 35). A little after the start, so a dm that runs for a moment
         * (a check, a `--register`) is not fetching 36 MB. */
        if crate::ytdlp::binary().is_none() && std::env::var_os("DM_YTDLP").is_none() {
            let after = std::env::var("DM_PREFETCH_AFTER")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20);
            tokio::time::sleep(std::time::Duration::from_secs(after)).await;
            if crate::ytdlp::binary().is_none() {
                let _ = for_video(&client).await;
            }
        }
        return;
    };
    let week = std::time::Duration::from_secs(7 * 24 * 3600);
    let stale = std::fs::metadata(&current)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age > week);
    /* The single file an earlier dm fetched is replaced at once by the
     * unpacked build where there is one, not a week on: it is the one that
     * takes seconds to start. A failed fetch leaves it as it was. */
    let one_file = current == folder.join(Tool::YtDlp.file_name())
        && ytdlp_asset_for(std::env::consts::OS, std::env::consts::ARCH, on_musl()).is_some();
    if !stale && !one_file {
        return;
    }
    let _one = lock_of(Tool::YtDlp).lock().await;
    let _ = fetch_into(
        &client,
        Tool::YtDlp,
        folder,
        &official(),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn with_fetching_off_nothing_is_fetched_and_it_says_so() {
        allow_fetching(false);
        if crate::ytdlp::binary().is_none() {
            let said = ensure(&crate::client(), Tool::YtDlp)
                .await
                .unwrap_err()
                .to_string();
            assert!(said.contains("Options › Downloads"), "{said}");
        }
        allow_fetching(true);
    }

    /// The unpacked builds yt-dlp's release 2026.10.26 publishes, each with a
    /// checksum in its `SHA2-256SUMS`: what dm names must be among them, or the
    /// fetch is a 404 on that system.
    const PUBLISHED_UNPACKED: [&str; 8] = [
        "yt-dlp_linux.zip",
        "yt-dlp_linux_aarch64.zip",
        "yt-dlp_macos.zip",
        "yt-dlp_musllinux.zip",
        "yt-dlp_musllinux_aarch64.zip",
        "yt-dlp_win.zip",
        "yt-dlp_win_arm64.zip",
        "yt-dlp_win_x86.zip",
    ];

    #[test]
    fn every_system_dm_supports_gets_a_build_yt_dlp_publishes() {
        let systems = [
            ("windows", "x86_64"),
            ("windows", "aarch64"),
            ("windows", "x86"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
        ];
        for (os, arch) in systems {
            for musl in [false, true] {
                let asset = ytdlp_asset_for(os, arch, musl)
                    .unwrap_or_else(|| panic!("no build for {os} {arch} (musl: {musl})"));
                assert!(
                    PUBLISHED_UNPACKED.contains(&asset),
                    "{asset} is not among yt-dlp's published builds"
                );
                // Unpacked, with the program named inside it as the archive holds it.
                let program = unpacked_program(asset);
                assert_eq!(
                    program.ends_with(".exe"),
                    os == "windows",
                    "{asset}: {program}"
                );
            }
        }
    }

    #[test]
    fn a_system_is_never_handed_another_systems_build() {
        // Distinct builds for distinct systems: a Windows machine must not get
        // a Linux one, nor a musl system the glibc build that cannot run there.
        let build = |os, arch, musl| ytdlp_asset_for(os, arch, musl).unwrap();
        assert_ne!(
            build("windows", "x86_64", false),
            build("linux", "x86_64", false)
        );
        assert_ne!(
            build("linux", "x86_64", false),
            build("linux", "aarch64", false)
        );
        assert_ne!(
            build("linux", "x86_64", false),
            build("linux", "x86_64", true)
        );
        assert_ne!(
            build("linux", "aarch64", false),
            build("linux", "aarch64", true)
        );
        assert_ne!(
            build("windows", "x86_64", false),
            build("windows", "aarch64", false)
        );
        // A system with no published build is told so, not handed the nearest.
        assert_eq!(ytdlp_asset("freebsd", "x86_64"), None);
        assert_eq!(ytdlp_asset("linux", "riscv64"), None);
    }

    #[test]
    fn quickjs_comes_for_each_system_with_its_digest_in_dm() {
        assert_eq!(quickjs_asset("macos", "aarch64"), Some("qjs-darwin-arm64"));
        assert_eq!(quickjs_asset("linux", "x86_64"), Some("qjs-linux-x86_64"));
        assert_eq!(
            quickjs_asset("windows", "aarch64"),
            Some("qjs-windows-x86_64.exe")
        );
        assert_eq!(quickjs_asset("freebsd", "x86_64"), None);
        let sources = official();
        for (os, arch) in [
            ("macos", "aarch64"),
            ("macos", "x86_64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("windows", "x86_64"),
        ] {
            let asset = quickjs_asset(os, arch).unwrap();
            assert!(
                sources
                    .quickjs_digests
                    .iter()
                    .any(|(n, d)| n == asset && d.len() == 64),
                "{asset}"
            );
        }
    }

    #[test]
    fn a_checksums_file_is_read_for_its_one_file() {
        let sums = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  yt-dlp\n\
                    ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789 *yt-dlp.exe\n\
                    nothex  yt-dlp_macos\n";
        assert_eq!(
            digest_in(sums, "yt-dlp.exe").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert!(digest_in(sums, "yt-dlp").is_some());
        assert_eq!(digest_in(sums, "yt-dlp_macos"), None);
        assert_eq!(digest_in(sums, "yt-dlp_linux"), None);
    }

    fn zip_of(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        use std::io::Write;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for (name, body, mode) in files {
            let options = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
            zip.start_file(*name, options).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn an_unpacked_build_keeps_its_layout_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let zip = zip_of(&[
            ("yt-dlp_linux", b"#!/bin/sh\necho 1\n", 0o755),
            ("_internal/lib/a.so", b"lib", 0o644),
        ]);
        let into = dir.path().join("out");
        unpack_portable(&zip, &into).unwrap();
        assert_eq!(
            std::fs::read(into.join("_internal/lib/a.so")).unwrap(),
            b"lib"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &str| {
                std::fs::metadata(into.join(p))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(mode("yt-dlp_linux"), 0o755);
            assert_eq!(mode("_internal/lib/a.so"), 0o644);
        }
    }

    #[test]
    fn a_name_that_leaves_the_folder_is_refused_and_nothing_is_written_outside() {
        let dir = tempfile::tempdir().unwrap();
        let zip = zip_of(&[("../escaped", b"x", 0o644)]);
        let into = dir.path().join("out");
        assert!(unpack_portable(&zip, &into).is_err());
        assert!(!dir.path().join("escaped").exists());
    }
}

#[cfg(test)]
mod adopt_tests {
    use super::*;

    #[test]
    fn a_shipped_yt_dlp_is_copied_into_the_tools_folder_whole_once() {
        let dir = tempfile::tempdir().unwrap();
        let shipped = dir.path().join("Resources/yt-dlp");
        std::fs::create_dir_all(shipped.join("_internal/lib")).unwrap();
        let name = unpacked_program(
            ytdlp_asset_for(std::env::consts::OS, std::env::consts::ARCH, on_musl())
                .unwrap_or("yt-dlp_linux.zip"),
        );
        std::fs::write(shipped.join(&name), b"program").unwrap();
        std::fs::write(shipped.join("_internal/lib/x.so"), b"library").unwrap();
        set_folder(dir.path().join("tools"));
        let adopted = adopt(&shipped.join(&name)).expect("copied");
        assert!(adopted.starts_with(dir.path().join("tools")));
        assert!(is_fetched(&adopted));
        let unpacked = adopted.parent().unwrap();
        assert_eq!(
            std::fs::read(unpacked.join("_internal/lib/x.so")).unwrap(),
            b"library"
        );
        // Asked again, the copy is used, not made twice.
        std::fs::remove_file(shipped.join(&name)).unwrap();
        assert_eq!(adopt(&shipped.join(&name)), Some(adopted.clone()));

        // As a package carries it: the release's zip, unpacked on first use.
        std::fs::remove_dir_all(adopted.parent().unwrap()).unwrap();
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            zip.start_file(name.as_str(), opts).unwrap();
            std::io::Write::write_all(&mut zip, b"program").unwrap();
            zip.start_file("_internal/lib/x.so", opts).unwrap();
            std::io::Write::write_all(&mut zip, b"library").unwrap();
            zip.finish().unwrap();
        }
        let zipped = dir.path().join("Resources/yt-dlp/yt-dlp.zip");
        std::fs::write(&zipped, &bytes).unwrap();
        let unpacked = adopt(&zipped).expect("unpacked");
        assert_eq!(std::fs::read(&unpacked).unwrap(), b"program");
        assert_eq!(
            std::fs::read(unpacked.parent().unwrap().join("_internal/lib/x.so")).unwrap(),
            b"library"
        );
    }
}
