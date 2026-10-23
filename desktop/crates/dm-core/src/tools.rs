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

/// yt-dlp's own newest release, and QuickJS-ng 0.17.0.
pub fn official() -> Sources {
    Sources {
        ytdlp: "https://github.com/yt-dlp/yt-dlp/releases/latest/download".into(),
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
    Some(match (os, arch) {
        ("windows", "x86_64") => "yt-dlp.exe",
        ("windows", "aarch64") => "yt-dlp_arm64.exe",
        // Unpacked: the single-file build unpacks itself at every start,
        // 8 to 14 seconds on macOS where this takes 0.3.
        ("macos", _) => "yt-dlp_macos.zip",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        _ => return None,
    })
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

async fn fetch_once(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, Error> {
    let res = client.get(url).send().await?;
    if !res.status().is_success() {
        return Err(Error::Refused {
            status: res.status().as_u16(),
            url: url.to_string(),
            body: String::new(),
            retry_after: None,
        });
    }
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
    }
    Ok(body)
}

fn hex_of(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
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

/// The folder the unpacked macOS yt-dlp is kept in, under the tools folder,
/// and its program within.
const MACOS_DIR: &str = "yt-dlp-macos";
const MACOS_PROGRAM: &str = "yt-dlp_macos";

/// Whether `path` is a copy dm fetched (under its tools folder).
pub(crate) fn is_fetched(path: &Path) -> bool {
    folder().is_some_and(|folder| path.starts_with(folder))
}

/// The copy of `program` dm fetched, if there is one.
pub(crate) fn fetched(program: &str) -> Option<PathBuf> {
    let folder = folder()?;
    let candidates = [
        folder.join(MACOS_DIR).join(MACOS_PROGRAM),
        folder.join(if cfg!(windows) {
            format!("{program}.exe")
        } else {
            program.to_string()
        }),
    ];
    candidates
        .into_iter()
        .filter(|c| program == "yt-dlp" || !c.starts_with(folder.join(MACOS_DIR)))
        .find(|c| c.is_file())
}

/// Unpacks a zip into `into` with macOS's own `ditto`, whole or not at all.
async fn unpack(zip: &[u8], into: &Path) -> Result<(), Error> {
    let parent = into.parent().unwrap_or(into);
    tokio::fs::create_dir_all(parent).await?;
    let archive = into.with_extension("zip.partial");
    let staging = into.with_extension("partial");
    tokio::fs::write(&archive, zip).await?;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    let status = tokio::process::Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&archive)
        .arg(&staging)
        .status()
        .await;
    let _ = tokio::fs::remove_file(&archive).await;
    if !status.is_ok_and(|s| s.success()) {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(Error::Internal(
            "yt-dlp's macOS build could not be unpacked".into(),
        ));
    }
    let _ = tokio::fs::remove_dir_all(into).await;
    tokio::fs::rename(&staging, into).await?;
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
            let asset = ytdlp_asset(os, arch).ok_or_else(|| {
                Error::Internal(format!("yt-dlp publishes no build for {os} on {arch}"))
            })?;
            let sums = fetch(client, &format!("{}/SHA2-256SUMS", sources.ytdlp)).await?;
            let expected = digest_in(&String::from_utf8_lossy(&sums), asset)
                .ok_or_else(|| Error::Internal(format!("yt-dlp's checksums name no {asset}")))?;
            let bytes = fetch(client, &format!("{}/{asset}", sources.ytdlp)).await?;
            if hex_of(&bytes) != expected {
                return Err(Error::Internal(
                    "the yt-dlp fetched does not match its published checksum, so it was not kept"
                        .into(),
                ));
            }
            if asset.ends_with(".zip") {
                let dir = folder.join(MACOS_DIR);
                unpack(&bytes, &dir).await?;
                return Ok(dir.join(MACOS_PROGRAM));
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
    ytdlp
        .map(|_| ())
        .map_err(|e| format!("dm's video helper could not be got ready ({e})"))
}

/// Whether yt-dlp is there, or dm would fetch it when a page needs it.
pub fn video_possible() -> bool {
    crate::ytdlp::available()
        || (folder().is_some()
            && fetching_allowed()
            && std::env::var_os("DM_YTDLP").is_none()
            && ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH).is_some())
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
        return;
    };
    let week = std::time::Duration::from_secs(7 * 24 * 3600);
    let stale = std::fs::metadata(&current)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age > week);
    if !stale {
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

    #[test]
    fn each_system_gets_its_own_builds() {
        assert_eq!(ytdlp_asset("windows", "x86_64"), Some("yt-dlp.exe"));
        assert_eq!(ytdlp_asset("macos", "aarch64"), Some("yt-dlp_macos.zip"));
        assert_eq!(
            ytdlp_asset("linux", "aarch64"),
            Some("yt-dlp_linux_aarch64")
        );
        assert_eq!(ytdlp_asset("freebsd", "x86_64"), None);
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
}
