//! yt-dlp and ffmpeg, fetched by dm itself where the computer has none, so
//! a video site's page downloads with nothing installed by hand.
//!
//! dm still writes no site logic of its own: it runs yt-dlp, as it always
//! has. What changes is where that yt-dlp comes from. One the person
//! installed is used first, wherever it is (`ytdlp::which`); with none,
//! the first video page that needs it has dm fetch yt-dlp's own standalone
//! build from yt-dlp's GitHub releases, checked against the `SHA2-256SUMS`
//! published beside it, and a single-file ffmpeg build from one pinned
//! release, checked against digests written into dm. Both go into dm's
//! own `tools` folder, through dm's proxy like any download, and yt-dlp is
//! kept current there with its own `-U`: sites change, and an old yt-dlp
//! stops working on them.

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

/// The fetched tools' folder, if dm has one.
pub fn folder() -> Option<&'static Path> {
    FOLDER.get().map(PathBuf::as_path)
}

/// One fetch of each tool at a time: two downloads needing yt-dlp at once
/// wait for the one fetch; yt-dlp, ffmpeg and Deno come down side by side.
static FETCHING: [tokio::sync::Mutex<()>; 3] = [
    tokio::sync::Mutex::const_new(()),
    tokio::sync::Mutex::const_new(()),
    tokio::sync::Mutex::const_new(()),
];

fn lock_of(tool: Tool) -> &'static tokio::sync::Mutex<()> {
    match tool {
        Tool::YtDlp => &FETCHING[0],
        Tool::Ffmpeg => &FETCHING[1],
        Tool::Deno => &FETCHING[2],
    }
}

/// A tool dm can fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
    /// The JavaScript runtime yt-dlp answers a site's player challenges
    /// with; without one, YouTube gives it too little to download.
    Deno,
}

impl Tool {
    fn program(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "ffmpeg",
            Tool::Deno => "deno",
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
    /// The pinned ffmpeg release, its files under this address.
    pub ffmpeg: String,
    /// The pinned release's digest of each `ffmpeg-<platform>.gz`.
    pub ffmpeg_digests: Vec<(String, String)>,
    /// Deno's newest release, each zip with its `.sha256sum` beside it.
    pub deno: String,
}

/// yt-dlp's own releases, and ffmpeg-static's b6.1.1 (an FFmpeg 6 build,
/// one file per platform), whose files publish no digests of their own:
/// these are theirs, taken when it was chosen.
pub fn official() -> Sources {
    Sources {
        ytdlp: "https://github.com/yt-dlp/yt-dlp/releases/latest/download".into(),
        ffmpeg: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1".into(),
        deno: "https://github.com/denoland/deno/releases/latest/download".into(),
        ffmpeg_digests: [
            (
                "darwin-arm64",
                "8923876afa8db5585022d7860ec7e589af192f441c56793971276d450ed3bbfa",
            ),
            (
                "darwin-x64",
                "929b375c1182d956c51f7ac25e0b2b0411fb01f6f407aa15c9758efeb4242106",
            ),
            (
                "linux-x64",
                "bfe8a8fc511530457b528c48d77b5737527b504a3797a9bc4866aeca69c2dffa",
            ),
            (
                "linux-arm64",
                "754a678672298bc68156adff58aa7385a592c2b30b1d0ae8750c45c915c4bac0",
            ),
            (
                "win32-x64",
                "8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77",
            ),
        ]
        .iter()
        .map(|(p, d)| ((*p).to_string(), (*d).to_string()))
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

/// Deno's build for an OS and processor, by its release's file name.
pub fn deno_asset(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("macos", "aarch64") => "deno-aarch64-apple-darwin.zip",
        ("macos", "x86_64") => "deno-x86_64-apple-darwin.zip",
        ("linux", "x86_64") => "deno-x86_64-unknown-linux-gnu.zip",
        ("linux", "aarch64") => "deno-aarch64-unknown-linux-gnu.zip",
        ("windows", "x86_64") => "deno-x86_64-pc-windows-msvc.zip",
        ("windows", "aarch64") => "deno-aarch64-pc-windows-msvc.zip",
        _ => return None,
    })
}

/// The one SHA-256 a checksum file holds, however it is written: `<hex>
/// <name>` (macOS and Linux) or a PowerShell listing with `Hash : <HEX>`
/// (Windows).
pub fn only_digest(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_hexdigit())
        .find(|word| word.len() == 64)
        .map(str::to_ascii_lowercase)
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(
        b.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?)).ok()
}

/// The file called `name` in a zip, inflated: from the central directory
/// (its sizes are there whether or not the entry was streamed), stored or
/// deflated. Enough for a release that zips one program.
pub fn unzip_one(zip: &[u8], name: &str) -> Option<Vec<u8>> {
    // The end-of-central-directory record, searched for from the end.
    let end = (0..zip.len().saturating_sub(21))
        .rev()
        .take(65_557)
        .find(|&at| zip.get(at..at + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))?;
    let entries = u16_at(zip, end + 10)?;
    let mut at = u32_at(zip, end + 16)?;
    for _ in 0..entries {
        if zip.get(at..at + 4)? != [0x50, 0x4b, 0x01, 0x02] {
            return None;
        }
        let method = u16_at(zip, at + 10)?;
        let packed = u32_at(zip, at + 20)?;
        let size = u32_at(zip, at + 24)?;
        let (name_len, extra, comment) = (
            u16_at(zip, at + 28)?,
            u16_at(zip, at + 30)?,
            u16_at(zip, at + 32)?,
        );
        let local = u32_at(zip, at + 42)?;
        let entry = zip.get(at + 46..at + 46 + name_len)?;
        if entry == name.as_bytes() {
            if zip.get(local..local + 4)? != [0x50, 0x4b, 0x03, 0x04] {
                return None;
            }
            let start = local + 30 + u16_at(zip, local + 26)? + u16_at(zip, local + 28)?;
            let data = zip.get(start..start.checked_add(packed)?)?;
            return match method {
                0 => Some(data.to_vec()),
                8 => {
                    use std::io::Read as _;
                    let mut out = Vec::with_capacity(size.min(MOST));
                    flate2::read::DeflateDecoder::new(data)
                        .take(MOST as u64)
                        .read_to_end(&mut out)
                        .ok()?;
                    (out.len() == size).then_some(out)
                }
                _ => None,
            };
        }
        at += 46 + name_len + extra + comment;
    }
    None
}

/// ffmpeg-static's name for an OS and processor.
pub fn ffmpeg_platform(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "win32-x64",
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

/// The most a fetched file may be: yt-dlp's builds are under 50 MB, the
/// pinned ffmpeg's under 30 MB packed and 90 MB unpacked.
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
        Tool::Deno => {
            let asset = deno_asset(os, arch).ok_or_else(|| {
                Error::Internal(format!("Deno publishes no build for {os} on {arch}"))
            })?;
            let sums = fetch(client, &format!("{}/{asset}.sha256sum", sources.deno)).await?;
            let expected = only_digest(&String::from_utf8_lossy(&sums)).ok_or_else(|| {
                Error::Internal(format!("Deno's checksum for {asset} could not be read"))
            })?;
            let packed = fetch(client, &format!("{}/{asset}", sources.deno)).await?;
            if hex_of(&packed) != expected {
                return Err(Error::Internal(
                    "the Deno fetched does not match its published checksum, so it was not kept"
                        .into(),
                ));
            }
            let inside = if os == "windows" { "deno.exe" } else { "deno" };
            let program = unzip_one(&packed, inside).ok_or_else(|| {
                Error::Internal("Deno's zip held no program dm could read".into())
            })?;
            place(&path, &program).await?;
        }
        Tool::Ffmpeg => {
            let platform = ffmpeg_platform(os, arch).ok_or_else(|| {
                Error::Internal(format!("there is no ffmpeg build for {os} on {arch}"))
            })?;
            let expected = sources
                .ffmpeg_digests
                .iter()
                .find(|(p, _)| p == platform)
                .map(|(_, d)| d.clone())
                .ok_or_else(|| Error::Internal(format!("no ffmpeg digest for {platform}")))?;
            let packed = fetch(client, &format!("{}/ffmpeg-{platform}.gz", sources.ffmpeg)).await?;
            if hex_of(&packed) != expected {
                return Err(Error::Internal(
                    "the ffmpeg fetched does not match the build dm knows, so it was not kept"
                        .into(),
                ));
            }
            let mut bytes = Vec::new();
            {
                use std::io::Read as _;
                flate2::read::GzDecoder::new(packed.as_slice())
                    .take(MOST as u64)
                    .read_to_end(&mut bytes)?;
            }
            place(&path, &bytes).await?;
        }
    }
    Ok(path)
}

/// `tool`, found or fetched. Found first (one the person installed, or one
/// dm fetched before); fetched only where dm has a tools folder and no
/// test or person has named the program outright (`DM_YTDLP`, `DM_FFMPEG`,
/// which is how a test pins down what dm does without one).
pub async fn ensure(client: &reqwest::Client, tool: Tool) -> Result<PathBuf, Error> {
    let find = || match tool {
        Tool::YtDlp => crate::ytdlp::binary(),
        Tool::Ffmpeg => crate::ytdlp::ffmpeg(),
        Tool::Deno => crate::ytdlp::deno(),
    };
    if let Some(found) = find() {
        return Ok(found);
    }
    let named = format!(
        "DM_{}",
        tool.program().replace('-', "").to_ascii_uppercase()
    );
    let Some(folder) = folder().filter(|_| std::env::var_os(named).is_none()) else {
        return Err(Error::Internal(format!("install {}", tool.program())));
    };
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

/// yt-dlp for a video page: found or fetched, and with it, where they can
/// be had, ffmpeg (so picture and sound are joined at full quality; without
/// it yt-dlp takes the one pre-joined stream, lower in quality) and Deno
/// (so yt-dlp can answer YouTube's player challenges; without one it gets
/// little or nothing). All three come down at once. Says why not, where
/// yt-dlp cannot be had.
pub async fn for_video(client: &reqwest::Client) -> Result<(), String> {
    let (ytdlp, _, _) = tokio::join!(
        ensure(client, Tool::YtDlp),
        ensure(client, Tool::Ffmpeg),
        ensure(client, Tool::Deno),
    );
    ytdlp
        .map(|_| ())
        .map_err(|e| format!("a video page needs yt-dlp, and dm could not get it ({e})"))
}

/// Whether yt-dlp is there, or dm would fetch it when a page needs it.
pub fn video_possible() -> bool {
    crate::ytdlp::available()
        || (folder().is_some()
            && std::env::var_os("DM_YTDLP").is_none()
            && ytdlp_asset(std::env::consts::OS, std::env::consts::ARCH).is_some())
}

/// Keeps a yt-dlp dm fetched current: once it is a week old, fetched
/// afresh, checked as the first was, and swapped in whole (a fetch that
/// fails leaves the one there). One the person installed is theirs to
/// update; sites change, and an old yt-dlp stops working on them.
pub async fn freshen(client: reqwest::Client) {
    let Some(folder) = folder() else {
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

    #[test]
    fn each_system_gets_its_own_builds() {
        assert_eq!(ytdlp_asset("windows", "x86_64"), Some("yt-dlp.exe"));
        assert_eq!(ytdlp_asset("macos", "aarch64"), Some("yt-dlp_macos.zip"));
        assert_eq!(
            ytdlp_asset("linux", "aarch64"),
            Some("yt-dlp_linux_aarch64")
        );
        assert_eq!(ytdlp_asset("freebsd", "x86_64"), None);
        assert_eq!(ffmpeg_platform("windows", "x86_64"), Some("win32-x64"));
        assert_eq!(ffmpeg_platform("macos", "aarch64"), Some("darwin-arm64"));
        assert_eq!(ffmpeg_platform("windows", "aarch64"), None);
        // Every platform with an ffmpeg build has its digest in dm.
        let sources = official();
        for os_arch in [
            ("macos", "aarch64"),
            ("macos", "x86_64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("windows", "x86_64"),
        ] {
            let p = ffmpeg_platform(os_arch.0, os_arch.1).unwrap();
            assert!(
                sources
                    .ffmpeg_digests
                    .iter()
                    .any(|(n, d)| n == p && d.len() == 64),
                "{p}"
            );
        }
    }

    #[test]
    fn deno_comes_for_each_system_and_its_checksum_is_read_either_way() {
        assert_eq!(
            deno_asset("macos", "aarch64"),
            Some("deno-aarch64-apple-darwin.zip")
        );
        assert_eq!(
            deno_asset("windows", "x86_64"),
            Some("deno-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(deno_asset("freebsd", "x86_64"), None);
        let unix = "5cd46d6268f6f78f5d88bdc7159d20bd44cdaa4b3303474839f87ec6fe7ae25c  deno-aarch64-apple-darwin.zip\n";
        let windows = "\r\nAlgorithm : SHA256\r\nHash      : A0C3101B4158D1DFB7D6A78A7BF0F3DE80C96BB423C152BEEC8BEB22786F2238\r\nPath      : C:\\a\\deno-x86_64-pc-windows-msvc.zip\r\n";
        assert_eq!(
            only_digest(unix).as_deref(),
            Some("5cd46d6268f6f78f5d88bdc7159d20bd44cdaa4b3303474839f87ec6fe7ae25c")
        );
        assert_eq!(
            only_digest(windows).as_deref(),
            Some("a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238")
        );
        assert_eq!(only_digest("no digest here"), None);
    }

    /// A zip as a release makes one: a local header and its data, then the
    /// central directory and its end. Built here, entry by entry.
    fn zip_of(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        use std::io::Write as _;
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data, deflate) in entries {
            let packed = if *deflate {
                let mut e =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]);
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&packed);
            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]);
            central.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn one_file_is_read_out_of_a_zip_stored_or_deflated() {
        let program = b"a program, long enough to deflate well: ".repeat(40);
        let zip = zip_of(&[("README", b"read me", false), ("deno", &program, true)]);
        assert_eq!(unzip_one(&zip, "deno"), Some(program.clone()));
        assert_eq!(unzip_one(&zip, "README").as_deref(), Some(&b"read me"[..]));
        assert_eq!(unzip_one(&zip, "deno.exe"), None);
        // Cut short or not a zip at all: nothing, and no panic.
        assert_eq!(unzip_one(&zip[..zip.len() / 2], "deno"), None);
        assert_eq!(unzip_one(b"not a zip", "deno"), None);
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
