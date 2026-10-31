//! Video tools: QuickJS, fetched by dm itself where the computer has none,
//! so YouTube's solver scripts run with nothing installed by hand.
//! Nothing else is fetched: dm reads YouTube itself (`crate::youtube`),
//! joins picture and sound and records streams itself (`mp4mux`, `mkvmux`,
//! `tsmux`, `rtmp`, `rtsp`, `mms`), and other sites' videos come from
//! the browser extension's caught player requests.

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

/// Whether dm may fetch its JavaScript engine at all (Options › Downloads).
/// Off, only one the person installed is used.
static ALLOWED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Lets dm fetch its video helper, or not.
pub fn allow_fetching(allowed: bool) {
    ALLOWED.store(allowed, std::sync::atomic::Ordering::Relaxed);
}

fn fetching_allowed() -> bool {
    ALLOWED.load(std::sync::atomic::Ordering::Relaxed)
}

/// What a video page is told where fetching is turned off.
pub const NOT_FETCHED: &str = "dm's video helper is not fetched, as Options › Downloads says; install QuickJS or Deno, or let dm fetch it there";

/// The fetched tools' folder, if dm has one.
pub fn folder() -> Option<&'static Path> {
    FOLDER.get().map(PathBuf::as_path)
}

/// One fetch of QuickJS at a time: two videos needing it at once wait for
/// the one fetch.
static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

fn lock_of(_tool: Tool) -> &'static tokio::sync::Mutex<()> {
    &FETCHING
}

/// A tool dm can fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// The JavaScript engine that solves YouTube's player challenges.
    /// QuickJS: 1–3 MB where Deno is 85, and the same formats at the same
    /// speed.
    QuickJs,
}

impl Tool {
    fn program(self) -> &'static str {
        match self {
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
    /// The pinned QuickJS release, its files under this address.
    pub quickjs: String,
    /// The pinned release's digest of each of its builds, by file name: it
    /// publishes no checksums file, and these are GitHub's record of them.
    pub quickjs_digests: Vec<(String, String)>,
}

/// QuickJS-ng 0.17.0.
pub fn official() -> Sources {
    Sources {
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

/// The most a fetched file may be (QuickJS's builds are under 5 MB).
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

/// The copy of `program` dm fetched, if there is one.
pub(crate) fn fetched(program: &str) -> Option<PathBuf> {
    let file = folder()?.join(if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    });
    file.is_file().then_some(file)
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

/// A JavaScript engine for YouTube's player challenges: a Deno the person
/// installed, else QuickJS, found or fetched.
pub async fn for_video(client: &reqwest::Client) -> Result<(), String> {
    if crate::ytdlp::deno().is_none() {
        let _ = ensure(client, Tool::QuickJs).await;
    }
    Ok(())
}

/// Whether dm can read a video page: YouTube is read by dm itself.
pub fn video_possible() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn with_fetching_off_nothing_is_fetched_and_it_says_so() {
        allow_fetching(false);
        if crate::ytdlp::quickjs().is_none() {
            let said = ensure(&crate::client(), Tool::QuickJs)
                .await
                .unwrap_err()
                .to_string();
            assert!(said.contains("Options › Downloads"), "{said}");
        }
        allow_fetching(true);
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
}
