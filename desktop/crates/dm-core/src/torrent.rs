// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! BitTorrent, the one kind of download the browser side cannot do at all.
//!
//! An extension has no sockets, so a magnet link is the clearest case of
//! something only the desktop engine can take. This wraps `librqbit` in the
//! same shape as the HTTP downloader: given a URL and the shared `Options`,
//! it reports progress and returns a `Finished`, so the queue drives a
//! torrent exactly as it drives a file and nothing above this line has to
//! know the difference.
//!
//! One session per torrent, torn down by a cancellation token when the row is
//! paused or cancelled. That is heavier than a single shared session, but it
//! keeps a torrent's whole lifetime inside one call the queue already knows
//! how to start, pause and resume — a paused torrent's files stay on disk and
//! a fresh run resumes from them.

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions, TorrentStatsState};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::download::{Finished, Options, Update};
use crate::{category, Error, Progress};

/// A torrent's swarm as its engine last saw it: shown beside the bytes, as
/// a torrent has peers where a file has connections.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Swarm {
    /// Peers connected now.
    pub connected: u32,
    /// Peers known: every address found for it this run, connected or not.
    pub known: u32,
    /// Bytes sent to peers this run.
    pub uploaded: u64,
    /// Bytes a second sent to peers, over the last sample.
    pub upload_speed: f64,
}

impl Swarm {
    /// The swarm as librqbit's sample counts it, with the upload speed since
    /// the sample before (`before`, `dt` seconds earlier).
    fn of(stats: &librqbit::TorrentStats, before: Option<u64>, dt: f64) -> Swarm {
        let peers = stats.live.as_ref().map(|l| &l.snapshot.peer_stats);
        let connected = peers.map_or(0, |p| p.live);
        let uploaded = stats.uploaded_bytes;
        Swarm {
            connected,
            known: peers.map_or(0, |p| p.seen).max(connected),
            uploaded,
            upload_speed: match before {
                Some(before) if dt > 0.0 => uploaded.saturating_sub(before) as f64 / dt,
                _ => 0.0,
            },
        }
    }
}

/// What only a torrent's download reads, beside the file download's options.
#[derive(Debug, Clone, Default)]
pub struct TorrentOptions {
    /// The files to fetch, by their place in the torrent; `None` is every file.
    pub only: Option<Vec<usize>>,
    /// The wanted files tier by tier where their priorities differ ([`tiers`]):
    /// the first is asked for, the next added once it is whole. Empty where
    /// they are fetched together.
    pub tiers: Vec<Vec<usize>>,
    /// Where the list of peers never connected to is: an address on the web,
    /// or a file on this computer. `None` for no list.
    pub blocklist: Option<String>,
    /// Keep a finished torrent shared, and how long; `None` stops it once its
    /// files are whole (gaps row 147).
    pub share: Option<Share>,
    /// Peers found only through the torrent's trackers: no DHT and no local
    /// discovery, which announce this computer to the public network. Set by
    /// the tests, which run their swarms on loopback.
    pub no_public_discovery: bool,
}

impl TorrentOptions {
    /// Every file wanted, by its place in the torrent: all the tiers where
    /// there are several, else `only`. `None` is every file.
    pub fn wanted(&self) -> Option<Vec<usize>> {
        if self.tiers.is_empty() {
            return self.only.clone();
        }
        let mut all: Vec<usize> = self.tiers.iter().flatten().copied().collect();
        all.sort_unstable();
        Some(all)
    }
}

/// How long a finished torrent is shared: until it has sent `ratio` times its
/// size or for `minutes`, whichever comes first, sending at most `upload_kbps`
/// (0: no limit of its own).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Share {
    /// How many times its size it sends before it stops.
    pub ratio: f64,
    /// The longest it is shared, in minutes.
    pub minutes: u64,
    /// The most it sends, in KiB a second; 0 for no limit of its own.
    pub upload_kbps: u64,
}

/// A finished torrent being shared: what it has sent, of a size of `size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sharing {
    /// What it has sent since it finished, in bytes.
    pub sent: u64,
    /// Its size, in bytes.
    pub size: u64,
    /// Where its session listens for peers, where it does.
    pub listening: Option<std::net::SocketAddr>,
}

type Shared = std::collections::HashMap<PathBuf, (Sharing, CancellationToken)>;

/// The finished torrents being shared, by where their files are.
fn shared() -> std::sync::MutexGuard<'static, Shared> {
    static SHARED: std::sync::OnceLock<std::sync::Mutex<Shared>> = std::sync::OnceLock::new();
    crate::lock(SHARED.get_or_init(Default::default))
}

/// Whether the finished torrent at `path` is being shared, and how far.
pub fn sharing(path: &Path) -> Option<Sharing> {
    shared().get(path).map(|(sharing, _)| *sharing)
}

/// Whether any finished torrent is being shared.
pub fn sharing_any() -> bool {
    !shared().is_empty()
}

/// Stops sharing the finished torrent at `path`, as its row is removed or its
/// files deleted: its session lets go of the files.
pub fn stop_sharing(path: &Path) {
    if let Some((_, token)) = shared().remove(path) {
        token.cancel();
    }
}

/// How long a blocklist fetched from the web is used before it is fetched again.
const BLOCKLIST_FRESH: Duration = Duration::from_secs(24 * 3600);
/// The largest blocklist read: the lists in use are a few megabytes, gzipped.
const BLOCKLIST_MOST: u64 = 64 * 1024 * 1024;

/// The blocklist as librqbit reads it, a `file:` address: one on this
/// computer as it is; one on the web fetched with dm's own client (so
/// through the proxy, which librqbit's own fetch would go around) and kept
/// a day beside the parts. One that cannot be read is an error: a torrent
/// does not go on as if the person had asked for no list.
async fn blocklist_file(
    client: &reqwest::Client,
    address: &str,
    parts_dir: &Path,
) -> Result<String, Error> {
    let address = address.trim();
    let refused = |why: String| {
        Error::Internal(format!(
            "the peer blocklist at {address} could not be read: {why}"
        ))
    };
    if address.starts_with("file:") {
        return Ok(address.to_string());
    }
    if !(address.starts_with("http://") || address.starts_with("https://")) {
        let path = Path::new(address);
        if !path.is_file() {
            return Err(refused("there is no such file".into()));
        }
        return file_url(path).ok_or_else(|| refused("its path cannot be made an address".into()));
    }
    let folder = parts_dir.parent().unwrap_or(parts_dir);
    let kept = folder.join(format!("peer-blocklist-{:016x}", fnv(address)));
    let fresh = tokio::fs::metadata(&kept)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age < BLOCKLIST_FRESH);
    if !fresh {
        let mut response = client
            .get(address)
            .send()
            .await
            .map_err(|e| refused(e.to_string()))?;
        if !response.status().is_success() {
            return Err(refused(format!(
                "the server answered {}",
                response.status()
            )));
        }
        let too_big = || refused("it is larger than 64 MB".into());
        if response
            .content_length()
            .is_some_and(|n| n > BLOCKLIST_MOST)
        {
            return Err(too_big());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| refused(e.to_string()))? {
            body.extend_from_slice(&chunk);
            if body.len() as u64 > BLOCKLIST_MOST {
                return Err(too_big());
            }
        }
        tokio::fs::create_dir_all(folder)
            .await
            .map_err(Error::Disk)?;
        // Whole or not at all: a torrent started meanwhile reads the last one.
        let partial = kept.with_extension("part");
        tokio::fs::write(&partial, &body)
            .await
            .map_err(Error::Disk)?;
        tokio::fs::rename(&partial, &kept)
            .await
            .map_err(Error::Disk)?;
    }
    file_url(&kept).ok_or_else(|| refused("its copy's path cannot be made an address".into()))
}

/// A short name for an address, for the file its list is kept in.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// One file of a torrent, and whether it is fetched.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TorrentFile {
    /// Its path inside the torrent, folders apart by `/`.
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
    /// Whether it is fetched: every file is, until a person leaves it out.
    #[serde(default = "fetched")]
    pub wanted: bool,
    /// When it is fetched beside the torrent's other files (gaps row 149).
    #[serde(default, skip_serializing_if = "Priority::is_normal")]
    pub priority: Priority,
}

/// When a torrent's file is fetched: the High ones first, the Low ones once
/// every other is whole. The torrent library has no priority between files,
/// only a file in or out, so dm asks for one tier at a time and adds the next
/// as the last is complete; with every file Normal nothing changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Fetched before the others.
    High,
    /// As every file is, until a person says otherwise.
    #[default]
    Normal,
    /// Fetched once every other file is whole.
    Low,
}

impl Priority {
    /// Whether it is the default, which the queue file leaves unwritten.
    pub fn is_normal(&self) -> bool {
        *self == Priority::Normal
    }
}

/// The wanted files in the order they are fetched, by their place in the
/// torrent: the High, then the Normal, then the Low, each tier that has a
/// file. One tier, or none, is the torrent fetched as a whole.
pub fn tiers(files: &[TorrentFile]) -> Vec<Vec<usize>> {
    [Priority::High, Priority::Normal, Priority::Low]
        .iter()
        .map(|p| {
            files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.wanted && f.priority == *p)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        })
        .filter(|tier| !tier.is_empty())
        .collect()
}

fn fetched() -> bool {
    true
}

/// The files to fetch, by their place in the torrent, where some are left
/// out; `None` where every file is wanted (or none is known yet).
pub fn only_files(files: &[TorrentFile]) -> Option<Vec<usize>> {
    files.iter().any(|f| !f.wanted).then(|| {
        files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.wanted)
            .map(|(i, _)| i)
            .collect()
    })
}

/// A torrent's files as now reported, with the choices already made kept
/// where the list is the same one (each name and size in its place).
pub fn merged_files(kept: &[TorrentFile], reported: Vec<TorrentFile>) -> Vec<TorrentFile> {
    let same = kept.len() == reported.len()
        && kept
            .iter()
            .zip(&reported)
            .all(|(k, r)| k.name == r.name && k.size == r.size);
    if same {
        kept.to_vec()
    } else {
        reported
    }
}

/// Whether this is a download for the torrent engine rather than the HTTP one.
///
/// A `.torrent` served over HTTP is caught here too: fetched as a file it
/// would save the few kilobytes of metainfo rather than the gigabytes it
/// describes, which is never what anyone means by downloading a torrent.
pub fn is_torrent(url: &str) -> bool {
    is_magnet(url) || ends_with_ignoring_case(strip_query(url.trim()), ".torrent")
}

/// Whether this is a magnet link. A scheme is case-insensitive, so
/// `Magnet:` is one too, and would otherwise have been fetched over HTTP.
fn is_magnet(url: &str) -> bool {
    url.trim()
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("magnet:"))
}

/// The path of a `file://` address, which is how a desktop's file manager
/// hands dm a `.torrent` it was asked to open.
pub fn local_file(url: &str) -> Option<std::path::PathBuf> {
    let url = url.trim();
    if !url.get(..7)?.eq_ignore_ascii_case("file://") {
        return None;
    }
    url::Url::parse(url).ok()?.to_file_path().ok()
}

/// The `file://` address of a path on this computer.
pub fn file_url(path: &std::path::Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(String::from)
}

/// Whether a tracker's address is one a proxy carries: HTTP or HTTPS.
/// A `udp://` tracker would be asked directly, around any proxy.
fn tcp_tracker(address: &str) -> bool {
    let lower = address.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// The refusal when a proxy leaves a torrent no way to find peers.
fn no_tracker_behind_proxy() -> Error {
    Error::Internal(
        "behind a proxy dm finds peers only through http or https trackers, and this torrent lists none"
            .into(),
    )
}

/// A magnet with only the trackers a proxy carries: behind one, the DHT is
/// off and a `udp://` tracker would be asked directly with the info-hash
/// and the real address.
fn tcp_trackers_of_magnet(magnet: &str) -> Result<String, Error> {
    let (head, query) = magnet.split_once('?').unwrap_or((magnet, ""));
    let mut trackers = 0;
    let kept: Vec<&str> = query
        .split('&')
        .filter(|part| {
            let Some(value) = part.strip_prefix("tr=") else {
                return true;
            };
            let address = percent_encoding::percent_decode_str(value).decode_utf8_lossy();
            let keep = tcp_tracker(&address);
            trackers += usize::from(keep);
            keep
        })
        .collect();
    if trackers == 0 {
        return Err(no_tracker_behind_proxy());
    }
    Ok(format!("{head}?{}", kept.join("&")))
}

/// A `.torrent` with only the trackers a proxy carries, its info dictionary
/// kept byte for byte so it is still the same torrent.
fn tcp_trackers_of(metainfo: &[u8]) -> Result<Vec<u8>, Error> {
    let parsed = librqbit::torrent_from_bytes(metainfo)
        .map_err(|e| Error::Internal(format!("this file is not a torrent: {e}")))?;
    let trackers: Vec<String> = parsed
        .iter_announce()
        .map(|t| String::from_utf8_lossy(t.as_ref()).into_owned())
        .filter(|t| tcp_tracker(t))
        .collect();
    let Some(first) = trackers.first() else {
        return Err(no_tracker_behind_proxy());
    };
    let string = |out: &mut Vec<u8>, s: &[u8]| {
        out.extend_from_slice(s.len().to_string().as_bytes());
        out.push(b':');
        out.extend_from_slice(s);
    };
    // Its keys in order, as bencoding requires.
    let mut out = b"d".to_vec();
    string(&mut out, b"announce");
    string(&mut out, first.as_bytes());
    string(&mut out, b"announce-list");
    out.extend_from_slice(b"ll");
    for t in &trackers {
        string(&mut out, t.as_bytes());
    }
    out.extend_from_slice(b"ee");
    string(&mut out, b"info");
    out.extend_from_slice(parsed.info.raw_bytes.as_ref());
    out.push(b'e');
    Ok(out)
}

fn ends_with_ignoring_case(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len()
        && s.get(s.len() - suffix.len()..)
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
}

fn strip_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// How often the row is redrawn while a torrent transfers.
const TICK: Duration = Duration::from_millis(500);

/// Runs one torrent to completion, reporting as it goes.
///
/// `client` is dm's own, so a `.torrent` fetched over HTTP goes through
/// whatever proxy the file downloads use. The swarm is librqbit's to reach,
/// and it is told the proxy too where it can use one — SOCKS5 — and refused
/// where it cannot, rather than announcing to trackers and peers directly on
/// a machine configured to make no direct connection.
pub async fn download_torrent(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<Sender<Update>>,
) -> Result<Finished, Error> {
    // A check that makes torrents (`scripts/chrome-check.py`) holds them off the network: a
    // program at a new path listening for peers raises Windows' firewall question each run.
    if std::env::var_os("DM_NO_SWARM").is_some_and(|v| !v.is_empty()) {
        return Err(Error::Internal(
            "torrents are held off the network in this run (DM_NO_SWARM)".into(),
        ));
    }
    if let Some(why) = options.no_route_through_system_proxy(url, "a torrent") {
        return Err(Error::Internal(why));
    }
    let connect = swarm_proxy(options.proxy_for(url).as_deref())?;
    let proxied = connect.is_some();
    // Behind a proxy, or asked to keep to the trackers: no DHT, no local discovery.
    let private = proxied || options.torrent.no_public_discovery;

    /* Staged like every other download, under its own parts directory, and
     * moved into the download folder only when whole. Written straight into
     * the download folder, a paused torrent's half-fetched files sat there
     * looking finished, and removing the row left them: the queue clears a
     * row's parts directory, and a torrent had nothing in it. */
    let output = options.parts_dir.clone();
    tokio::fs::create_dir_all(&output)
        .await
        .map_err(Error::Disk)?;

    // The whole session shuts down when this is cancelled, which is how a
    // pause or a cancel stops every socket at once rather than leaving peers
    // connected to a download nobody is watching.
    let token = CancellationToken::new();
    // This torrent's claim on the shared ceiling, given back on every exit.
    let _share = options.limiter.share();
    // The peers never to connect to, where a list is set.
    let blocklist_url = match &options.torrent.blocklist {
        Some(address) => Some(
            crate::download::or_stop(
                &options.control,
                blocklist_file(client, address, &options.parts_dir),
            )
            .await?,
        ),
        None => None,
    };
    let session = Session::new_with_opts(
        output.clone(),
        SessionOptions {
            // Off: dm's queue is the record of what to resume, and a second
            // one in librqbit's own state file would be a thing to keep in
            // step for no gain.
            persistence: None,
            // The DHT on, for magnets with no tracker, but without its state
            // file: that file is one per user, so a second session (two
            // torrents at once, or a resume while the paused one is still
            // closing) failed to start with "error initializing persistent
            // DHT". Each session finds peers afresh on a port of its own.
            // Off behind a proxy, and local discovery with it: both are UDP,
            // which a SOCKS5 connection cannot carry, so they went out
            // directly with the info-hash and the real address.
            // A magnet then needs a tracker.
            dht: (!private).then(|| librqbit::DhtSessionConfig {
                persistence: None,
                ..Default::default()
            }),
            disable_local_service_discovery: private,
            blocklist_url,
            cancellation_token: Some(token.clone()),
            ratelimits: librqbit::limits::LimitsConfig {
                download_bps: download_cap(options).await,
                upload_bps: None,
            },
            connect,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| Error::Internal(format!("the torrent engine would not start: {e}")))?;

    // Dropped on every exit, so a paused or failed torrent does not leave a
    // session listening on a port for the life of the process.
    let _guard = CancelOnDrop(token.clone());

    let add = if is_magnet(url) {
        // In the spelling librqbit looks for: it matches `magnet:` exactly.
        let rest = url.trim().get(7..).unwrap_or_default();
        let magnet = format!("magnet:{rest}");
        AddTorrent::Url(
            if proxied {
                tcp_trackers_of_magnet(&magnet)?
            } else {
                magnet
            }
            .into(),
        )
    } else if let Some(path) = local_file(url) {
        // A .torrent on this computer, opened with dm from a file manager.
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            Error::Internal(format!(
                "could not read the torrent file {}: {e}",
                path.display()
            ))
        })?;
        AddTorrent::TorrentFileBytes(
            if proxied {
                tcp_trackers_of(&bytes)?
            } else {
                bytes
            }
            .into(),
        )
    } else {
        // Fetched with dm's client rather than librqbit's, so a .torrent
        // behind a proxy is reachable exactly as a file would be, and with
        // the page's referrer, so one served only to its own page is too.
        let bytes = crate::download::or_stop(
            &options.control,
            fetch_metainfo(
                client,
                url,
                options.referrer.as_deref(),
                options.sign_in.as_ref(),
            ),
        )
        .await?;
        AddTorrent::TorrentFileBytes(
            if proxied {
                tcp_trackers_of(&bytes)?
            } else {
                bytes
            }
            .into(),
        )
    };

    // What it was added from, to add it again over the finished files when it is
    // kept shared (gaps row 147).
    let again: Option<AddTorrent<'static>> = options.torrent.share.map(|_| match &add {
        AddTorrent::Url(link) => AddTorrent::Url(link.to_string().into()),
        AddTorrent::TorrentFileBytes(bytes) => AddTorrent::TorrentFileBytes(bytes.clone()),
    });

    /* Raced against the row's control: a magnet's metadata comes from its
     * peers, and with no peer answering the add waits with no end. Awaited
     * on its own, a pause or a cancel of such a magnet never took effect.
     * Stopped here, the guard below cancels the token, which shuts the
     * session and its half-made lookup down with it. */
    let added = crate::download::or_stop(&options.control, async {
        session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    // Resume from and seed the files already on disk: a
                    // paused torrent that could not overwrite its own parts
                    // would start from nothing every time it resumed.
                    overwrite: true,
                    // The files a person left ticked, where they left some out.
                    only_files: options.torrent.only.clone(),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| Error::Internal(format!("the torrent could not be added: {e}")))
    })
    .await?;
    let handle = added
        .into_handle()
        .ok_or_else(|| Error::Internal("the torrent produced no handle".into()))?;

    /* A file name inside the torrent is the swarm's to choose: one with a
     * drive, a stream suffix or a device name would land outside the folder
     * the files go in. Refused as soon as the list is known. The torrent is
     * removed without deleting files: a delete would act on that path. */
    let escaping = handle
        .with_metadata(|metadata| {
            metadata
                .file_infos
                .iter()
                .any(|file| !plain_torrent_path(&file.relative_filename))
        })
        .unwrap_or(false);
    if escaping {
        let _ = session.delete(handle.id().into(), false).await;
        token.cancel();
        return Err(Error::Internal(
            "the torrent names a file outside its own folder, so it was not downloaded".into(),
        ));
    }

    // Its files, once known, for a person to choose from.
    if let Some(tx) = &report {
        let files = handle
            .with_metadata(|metadata| {
                metadata
                    .file_infos
                    .iter()
                    .map(|file| TorrentFile {
                        name: file.relative_filename.to_string_lossy().replace('\\', "/"),
                        size: file.len,
                        wanted: true,
                        priority: Priority::Normal,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !files.is_empty() {
            let _ = tx.send(Update::Files(files)).await;
        }
    }

    // A digest is for one file: refused as soon as the metadata shows more,
    // rather than after the whole tree was fetched for nothing.
    let several = handle
        .with_metadata(|metadata| metadata.file_infos.len() > 1)
        .unwrap_or(false);
    if several && options.checksum.is_some() {
        let _ = session.delete(handle.id().into(), true).await;
        token.cancel();
        return Err(Error::Internal(
            "a checksum cannot be checked over a torrent of several files".into(),
        ));
    }

    /* Tier by tier where the files' priorities differ: the library counts
     * only the files asked for, so the row's total is every wanted file's,
     * and its "finished" ends a tier rather than the torrent. */
    let tiers = options.torrent.tiers.clone();
    let mut tier = 0;
    let tiered_total = (tiers.len() > 1)
        .then(|| {
            handle
                .with_metadata(|metadata| {
                    tiers
                        .iter()
                        .flatten()
                        .filter_map(|&i| metadata.file_infos.get(i).map(|f| f.len))
                        .sum::<u64>()
                })
                .ok()
        })
        .flatten();

    let mut named = false;
    let mut last = Instant::now();
    /* From the first sample, not from zero: a resumed torrent's first report
     * already counts the bytes on disk, and measured from zero they showed
     * as a burst of speed nobody downloaded. */
    let mut last_done: Option<u64> = None;
    let mut last_sent: Option<u64> = None;

    loop {
        // A pause or cancel is honoured here, between samples, the same place
        // the HTTP engine checks it. Cancelling deletes the partial files;
        // pausing leaves them for a resume to pick up.
        match options.control.check() {
            Ok(()) => {}
            Err(stop) => {
                token.cancel();
                if matches!(stop, Error::Cancelled) {
                    let _ = session.delete(handle.id().into(), true).await;
                }
                return Err(stop);
            }
        }

        // The shared ceiling can change while this runs — someone drags the
        // limit slider — and an HTTP download picks that up on its next chunk.
        // A torrent must too, or "one ceiling governs files and torrents
        // together" would hold only at the moment it started.
        // Only when it changed: each call makes a fresh limiter with a full
        // second's allowance, and set twice a second it let a torrent run at
        // three times its share.
        let cap = download_cap(options).await;
        if session.ratelimits.get_download_bps() != cap {
            session.ratelimits.set_download_bps(cap);
        }

        let stats = handle.stats();

        // The name is known only once the metadata is, which for a magnet
        // means after the first peers answer. Sent as soon as it is, so the
        // row stops showing the info-hash and starts showing the name.
        if !named {
            if let Some(name) = handle.name() {
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

        if let Some(tx) = &report {
            let now = Instant::now();
            let dt = (now - last).as_secs_f64();
            let total = tiered_total.or((stats.total_bytes > 0).then_some(stats.total_bytes));
            /* Never less than already reported. librqbit's count
             * drops when a piece fails its hash and is fetched again, and
             * while it checks what is on disk; a row whose bar went back
             * read as a download losing data. The high-water mark, held to
             * the total, catches up once the piece arrives again. */
            // While it initialises, librqbit's count is how much of what is
            // on disk it has checked, not what it has: a fresh torrent showed
            // a piece "downloaded" with no peer connected, now and then.
            let have = match stats.state {
                TorrentStatsState::Initializing { .. } => last_done.unwrap_or(0),
                _ => stats.progress_bytes,
            };
            let done = high_water(last_done, have, total);
            // Measured rather than read off librqbit's own estimator, so the
            // number is in the same units and computed the same way as every
            // other row.
            let speed = match last_done {
                Some(before) if dt > 0.0 => done.saturating_sub(before) as f64 / dt,
                _ => 0.0,
            };
            let eta = match total {
                Some(total) if speed > 1.0 && total > done => {
                    Some(Duration::from_secs_f64((total - done) as f64 / speed))
                }
                _ => None,
            };
            let _ = tx
                .send(Update::Progress(Progress {
                    downloaded: done,
                    total,
                    speed,
                    eta,
                }))
                .await;
            let swarm = Swarm::of(&stats, last_sent, dt);
            let _ = tx.send(Update::Swarm(swarm)).await;
            last = now;
            last_done = Some(done);
            last_sent = Some(swarm.uploaded);
        }

        if stats.finished {
            if tier + 1 >= tiers.len() {
                break;
            }
            // This tier is whole: the next one joins the files asked for.
            tier += 1;
            let asked: std::collections::HashSet<usize> =
                tiers[..=tier].iter().flatten().copied().collect();
            if let Err(e) = session.update_only_files(&handle, &asked).await {
                token.cancel();
                return Err(Error::Internal(format!(
                    "the torrent's next files could not be asked for: {e}"
                )));
            }
        }
        if let TorrentStatsState::Error = stats.state {
            token.cancel();
            return Err(Error::Internal(
                stats.error.unwrap_or_else(|| "the torrent failed".into()),
            ));
        }

        tokio::time::sleep(TICK).await;
    }

    let name = handle
        .name()
        .unwrap_or_else(|| handle.info_hash().as_string());
    let bytes = handle.stats().total_bytes;

    /* Where librqbit put it, read from the handle rather than guessed from
     * the name: one file is written straight into the output folder under
     * its own name, and only a tree gets a folder named for the torrent. */
    let staged = handle
        .with_metadata(|metadata| match metadata.file_infos.as_slice() {
            [only] => handle.output_folder().join(&only.relative_filename),
            _ => handle.output_folder().to_path_buf(),
        })
        .map_err(|e| Error::Internal(format!("the finished torrent has no metadata: {e}")))?;

    /* The files left out are not the person's: pieces they share with a
     * wanted file leave them partly written, and moved with the rest they
     * would look like files that were fetched. */
    // Every tier's files: `only` is the first tier's alone where there are several.
    if let Some(only) = &options.torrent.wanted() {
        let _ = handle.with_metadata(|metadata| {
            for (i, file) in metadata.file_infos.iter().enumerate() {
                if !only.contains(&i) {
                    let _ =
                        std::fs::remove_file(handle.output_folder().join(&file.relative_filename));
                }
            }
        });
    }

    // Forgotten by the session, files kept, so nothing holds them open
    // while they are moved into place.
    let id = handle.id();
    drop(handle);
    session
        .delete(id.into(), false)
        .await
        .map_err(|e| Error::Internal(format!("the finished torrent could not be released: {e}")))?;
    drop(session);

    let (path, category, verified, left_behind) = place(&staged, &name, options).await?;

    // Shared from where it now is, where the person keeps finished torrents shared. Not
    // where the file was renamed on the way (a name already taken): the torrent names it
    // otherwise, and its engine would start a fresh file under that name.
    if let (Some(share), Some(again)) = (options.torrent.share, again) {
        let folder = if staged.is_dir() || path.is_dir() {
            Some(path.clone())
        } else if path.file_name() == staged.file_name() {
            path.parent().map(Path::to_path_buf)
        } else {
            None
        };
        if let Some(folder) = folder {
            keep_sharing(
                path.clone(),
                folder,
                again,
                options.torrent.wanted(),
                bytes,
                share,
                options.proxy_for(url),
                options.torrent.no_public_discovery,
            );
        }
    }

    Ok(Finished {
        left_behind,
        path,
        bytes,
        // A torrent has no fixed connection count; it is however many peers
        // happen to be in the swarm. Zero rather than a number that means
        // something different from what the column says everywhere else.
        connections: 0,
        category,
        verified,
        mirrors: 0,
    })
}

/// Shares a finished torrent's files from where they are until its share is
/// done, in a session of its own beside the queue: the row is complete and its
/// files are the person's from the start. The session never fetches: files that
/// are no longer the torrent's whole end the sharing rather than being fetched
/// again.
#[allow(
    clippy::too_many_arguments,
    reason = "each is one part of what the finished download leaves to share"
)]
fn keep_sharing(
    path: PathBuf,
    folder: PathBuf,
    again: AddTorrent<'static>,
    only: Option<Vec<usize>>,
    size: u64,
    share: Share,
    proxy: Option<String>,
    no_public_discovery: bool,
) {
    if share.ratio <= 0.0 || share.minutes == 0 || size == 0 {
        return;
    }
    let token = CancellationToken::new();
    let first = Sharing {
        sent: 0,
        size,
        listening: None,
    };
    if let Some((_, older)) = shared().insert(path.clone(), (first, token.clone())) {
        older.cancel();
    }
    tokio::spawn(async move {
        // One that ends early (files moved or changed, no session) ends quietly: the row is
        // complete either way, and only its "Sharing" goes.
        let _ = share_until(
            &path,
            folder,
            again,
            only,
            size,
            share,
            proxy,
            no_public_discovery,
            &token,
        )
        .await;
        token.cancel();
        let mut all = shared();
        if all.get(&path).is_some_and(|(_, t)| t.is_cancelled()) {
            all.remove(&path);
        }
    });
}

/// The sharing itself: until `share` is done, the token is cancelled, or the
/// files stop being the torrent's whole.
#[allow(clippy::too_many_arguments)]
async fn share_until(
    path: &Path,
    folder: PathBuf,
    again: AddTorrent<'static>,
    only: Option<Vec<usize>>,
    size: u64,
    share: Share,
    proxy: Option<String>,
    no_public_discovery: bool,
    token: &CancellationToken,
) -> Result<(), String> {
    let connect = swarm_proxy(proxy.as_deref()).map_err(|e| e.to_string())?;
    let proxied = connect.is_some();
    let private = proxied || no_public_discovery;
    let upload_bps = u32::try_from(share.upload_kbps.saturating_mul(1024))
        .ok()
        .and_then(NonZeroU32::new);
    let session = Session::new_with_opts(
        folder.clone(),
        SessionOptions {
            persistence: None,
            // As the download's own session (see there): no DHT state file, and no UDP
            // behind a proxy.
            dht: (!private).then(|| librqbit::DhtSessionConfig {
                persistence: None,
                ..Default::default()
            }),
            disable_local_service_discovery: private,
            // Reachable by peers, on a port of the system's choosing (no UPnP): sharing is what
            // the person turned on. Not behind a proxy, which would show the real address.
            listen: (!proxied).then(librqbit::ListenerOptions::default),
            cancellation_token: Some(token.clone()),
            ratelimits: librqbit::limits::LimitsConfig {
                // Nothing is fetched: the files are whole, or the sharing ends.
                download_bps: NonZeroU32::new(1),
                upload_bps,
            },
            connect,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| format!("its session would not start: {e}"))?;
    let _guard = CancelOnDrop(token.clone());
    let added = session
        .add_torrent(
            again,
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(folder.to_string_lossy().into_owned()),
                only_files: only,
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| format!("it could not be added again: {e}"))?;
    let handle = added
        .into_handle()
        .ok_or_else(|| "it produced no handle".to_string())?;
    let listening = session.listen_addr();
    let deadline = Instant::now() + Duration::from_secs(share.minutes.saturating_mul(60));
    let goal = (size as f64 * share.ratio).ceil() as u64;
    loop {
        if token.is_cancelled() {
            break;
        }
        let stats = handle.stats();
        match stats.state {
            TorrentStatsState::Initializing { .. } => {}
            TorrentStatsState::Error => {
                break;
            }
            _ if !stats.finished => {
                let _ = session.delete(handle.id().into(), false).await;
                return Err("its files are no longer whole".into());
            }
            _ => {}
        }
        let sent = stats.uploaded_bytes;
        if let Some((sharing, _)) = shared().get_mut(path) {
            *sharing = Sharing {
                sent,
                size,
                listening,
            };
        }
        if sent >= goal || Instant::now() >= deadline {
            break;
        }
        tokio::select! {
            () = token.cancelled() => break,
            () = tokio::time::sleep(TICK) => {}
        }
    }
    let _ = session.delete(handle.id().into(), false).await;
    Ok(())
}

/// What a torrent reports as downloaded: librqbit's count, held to the
/// total and never below what was reported before.
fn high_water(before: Option<u64>, progress: u64, total: Option<u64>) -> u64 {
    progress
        .min(total.unwrap_or(u64::MAX))
        .max(before.unwrap_or(0))
}

/// Moves what a torrent produced into the download folder.
///
/// One file is placed exactly as any other download is — sorted into its
/// category folder and checked against the digest it was given, by the same
/// code. A tree of files keeps the structure the torrent names, under the
/// download folder itself: category sorting is for one file with one kind.
/// A digest cannot be checked over a tree, and a row that carries one is
/// failed rather than completed with the digest quietly unchecked.
async fn place(
    staged: &Path,
    name: &str,
    options: &Options,
) -> Result<(PathBuf, category::Category, bool, Option<String>), Error> {
    if staged.is_file() {
        // Under the file's own name, not the torrent's: the two differ when
        // a torrent of one file is named for its release.
        let file_name = staged
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string());
        let (path, _, category) =
            crate::download::finish(&file_name, None, &[staged.to_path_buf()], options).await?;
        return Ok((path, category, options.checksum.is_some(), None));
    }
    if options.checksum.is_some() {
        return Err(Error::Internal(
            "a checksum cannot be checked over a torrent of several files".into(),
        ));
    }
    options.control.check()?;
    tokio::fs::create_dir_all(&options.dest_dir)
        .await
        .map_err(Error::Disk)?;
    let folder = folder_name(name);
    let dest = crate::download::claim_dir(&options.dest_dir, &folder).await?;
    match move_tree(staged, &dest).await {
        Ok(left_behind) => Ok((dest, category::classify(&folder, None), false, left_behind)),
        Err(e) => {
            // Only the empty folder this claimed; never anything with files in.
            let _ = tokio::fs::remove_dir(&dest).await;
            Err(e)
        }
    }
}

/// The folder a torrent of several files lands in, from the name the
/// torrent gives itself — which is the torrent's to say, and so cleaned as
/// any remote name is: `../.config/autostart` must not leave the download
/// folder, nor arrive hidden.
fn folder_name(name: &str) -> String {
    let cleaned = crate::probe::sanitize(name);
    if cleaned.is_empty() {
        "torrent".into()
    } else {
        cleaned
    }
}

/// Moves a directory: a rename where the two are on one filesystem, a copy
/// and a removal where they are not. What it says, when it says anything,
/// is a copy left behind that could not be removed: the files are in place,
/// so that is a problem to note, not a failed download.
async fn move_tree(from: &Path, to: &Path) -> Result<Option<String>, Error> {
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(None);
    }
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_across(&from, &to))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
        .map_err(Error::Disk)
}

/// A move between disks. Copied straight into `to`, a copy that
/// failed midway left half a tree in the download folder looking finished;
/// and a copy that succeeded but whose source could not be removed failed
/// the row beside a whole copy, so Retry fetched it again as "name (1)".
/// The copy goes to a hidden name beside `to` and is renamed into place
/// only when whole; it is removed if it fails.
fn copy_across(from: &Path, to: &Path) -> std::io::Result<Option<String>> {
    let parent = to.parent().unwrap_or(Path::new("."));
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = parent.join(format!(".{name}.dm-moving"));
    // One left by a move that was interrupted is no use to anyone.
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = copy_tree(from, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    // Onto the empty folder claimed for it: Unix replaces an empty folder,
    // Windows does not, so that goes first there.
    if std::fs::rename(&staging, to).is_err() {
        let _ = std::fs::remove_dir(to);
        if let Err(e) = std::fs::rename(&staging, to) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    }
    Ok(std::fs::remove_dir_all(from).err().map(|e| {
        format!(
            "{} is complete, but the copy it was moved from could not be removed from {}: {e}",
            to.display(),
            from.display()
        )
    }))
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// How the swarm is reached when a proxy is configured.
///
/// librqbit can carry trackers and peers over SOCKS5 and nothing else. An
/// HTTP proxy cannot carry a peer connection, so a torrent on a machine with
/// one configured is refused with the reason rather than sent out around it.
fn swarm_proxy(proxy: Option<&str>) -> Result<Option<librqbit::ConnectionOptions>, Error> {
    let Some(proxy) = proxy else {
        return Ok(None);
    };
    /* librqbit names SOCKS5 `socks5://` and refuses `socks5h://`, which is
     * what dm's settings hand out so names resolve at the proxy. Peers are
     * addresses already; only a tracker's name differs between the two. */
    let proxy = match proxy.strip_prefix("socks5h://") {
        Some(rest) => format!("socks5://{rest}"),
        None => proxy.to_string(),
    };
    if !proxy.starts_with("socks5://") {
        return Err(Error::Internal(
            "a torrent can only go through a SOCKS5 proxy; the configured proxy is not one, \
             and BitTorrent will not go around it"
                .into(),
        ));
    }
    Ok(Some(librqbit::ConnectionOptions {
        proxy_url: Some(proxy),
        ..Default::default()
    }))
}

/// The shared download ceiling, in the shape librqbit wants it.
///
/// This torrent's part of the limiter every HTTP download shares, re-read
/// every tick, so one ceiling governs files, torrents and video runs
/// together as they start and stop. Zero
/// — no limit — becomes `None`, which is librqbit's word for the same thing.
async fn download_cap(options: &Options) -> Option<NonZeroU32> {
    let bps = options.limiter.part().await;
    u32::try_from(bps).ok().and_then(NonZeroU32::new)
}

/// Fetches a `.torrent` file, bounded so a link that is not one cannot ask
/// for an unbounded allocation.
async fn fetch_metainfo(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Vec<u8>, Error> {
    // A metainfo file is kilobytes; ten megabytes is far past any real one
    // and small enough that a mislabelled link cannot exhaust memory.
    const CAP: usize = 10 * 1024 * 1024;
    let too_large = || Error::Internal("that .torrent file is implausibly large".into());

    let res = crate::auth::send(
        client,
        crate::download::get(client, url, referrer, sign_in),
        url,
    )
    .await?;
    let res = Error::refused_or(res).await?;
    // A stated length past the cap is rejected before a byte is read.
    if res.content_length().is_some_and(|len| len > CAP as u64) {
        return Err(too_large());
    }

    // And enforced while reading, because Content-Length is a claim: a body
    // that keeps coming past the cap is stopped rather than buffered whole.
    let mut body = Vec::new();
    let mut chunks = res.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > CAP {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Cancels the session's token however the download ends, including a panic
/// or an early return, so no path can leave a torrent's sockets open.
struct CancelOnDrop(CancellationToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {

    fn file(name: &str, wanted: bool, priority: Priority) -> TorrentFile {
        TorrentFile {
            name: name.into(),
            size: 1,
            wanted,
            priority,
        }
    }

    #[test]
    fn files_are_fetched_high_then_normal_then_low_and_those_left_out_never() {
        let files = [
            file("a", true, Priority::Low),
            file("b", true, Priority::Normal),
            file("c", true, Priority::High),
            file("d", false, Priority::High),
            file("e", true, Priority::High),
        ];
        assert_eq!(tiers(&files), vec![vec![2, 4], vec![1], vec![0]]);
        // All Normal, or one tier however it is called: fetched together.
        let plain = [
            file("a", true, Priority::Normal),
            file("b", true, Priority::Normal),
        ];
        assert_eq!(tiers(&plain).len(), 1);
        let low = [
            file("a", true, Priority::Low),
            file("b", false, Priority::High),
        ];
        assert_eq!(tiers(&low), vec![vec![0]]);
        // Every tier's files are the wanted ones, not the first tier's alone.
        let options = TorrentOptions {
            only: Some(vec![2, 4]),
            tiers: tiers(&files),
            ..Default::default()
        };
        assert_eq!(options.wanted(), Some(vec![0, 1, 2, 4]));
        let untiered = TorrentOptions {
            only: Some(vec![1]),
            ..Default::default()
        };
        assert_eq!(untiered.wanted(), Some(vec![1]));
    }

    #[test]
    fn a_normal_priority_is_not_written_and_an_old_file_reads_as_normal() {
        let plain = serde_json::to_string(&file("a", true, Priority::Normal)).unwrap();
        assert!(!plain.contains("priority"), "{plain}");
        let high = serde_json::to_string(&file("a", true, Priority::High)).unwrap();
        assert!(high.contains(r#""priority":"high""#), "{high}");
        let old: TorrentFile = serde_json::from_str(r#"{"name":"a","size":1}"#).unwrap();
        assert_eq!((old.wanted, old.priority), (true, Priority::Normal));
    }

    #[test]
    fn behind_a_proxy_only_http_trackers_are_kept() {
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567\
            &tr=udp%3A%2F%2Ft.example%3A6969&tr=http%3A%2F%2Ft.example%2Fannounce&dn=x";
        let kept = super::tcp_trackers_of_magnet(magnet).unwrap();
        assert!(!kept.contains("udp"), "{kept}");
        assert!(kept.contains("tr=http%3A%2F%2Ft.example%2Fannounce") && kept.contains("dn=x"));
        assert!(super::tcp_trackers_of_magnet(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp%3A%2F%2Fa%3A1"
        )
        .is_err());

        // A .torrent: the udp tracker dropped, the info kept byte for byte.
        let info = b"d6:lengthi3e4:name1:x12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let b = |s: &str| format!("{}:{s}", s.len());
        let (udp, http) = ("udp://t.example:69", "http://t.example/announce");
        let mut file = format!(
            "d{}{}{}ll{}el{}ee{}",
            b("announce"),
            b(udp),
            b("announce-list"),
            b(udp),
            b(http),
            b("info")
        )
        .into_bytes();
        file.extend_from_slice(info);
        file.push(b'e');
        let before = librqbit::torrent_from_bytes(&file).unwrap().info_hash;
        let kept = super::tcp_trackers_of(&file).unwrap();
        let parsed = librqbit::torrent_from_bytes(&kept).unwrap();
        assert_eq!(parsed.info_hash, before, "another torrent");
        let trackers: Vec<String> = parsed
            .iter_announce()
            .map(|t| String::from_utf8_lossy(t.as_ref()).into_owned())
            .collect();
        assert_eq!(trackers, ["http://t.example/announce"]);
    }

    #[test]
    fn a_torrent_with_only_udp_trackers_or_none_is_refused_behind_a_proxy() {
        let info = b"d6:lengthi3e4:name1:x12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let mut udp_only = b"d8:announce18:udp://t.example:694:info".to_vec();
        udp_only.extend_from_slice(info);
        udp_only.push(b'e');
        let mut none = b"d4:info".to_vec();
        none.extend_from_slice(info);
        none.push(b'e');
        for file in [udp_only, none] {
            let why = super::tcp_trackers_of(&file).unwrap_err().to_string();
            assert!(why.contains("http or https trackers"), "{why}");
        }
        let why = super::tcp_trackers_of(b"not bencode")
            .unwrap_err()
            .to_string();
        assert!(why.contains("not a torrent"), "{why}");
    }

    /// A blocklist served on 127.0.0.1, counting how often it was fetched.
    async fn blocklist_server(
        status: u16,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = hits.clone();
        let app = axum::Router::new().route(
            "/list.p2p",
            axum::routing::get(move || {
                counted.fetch_add(1, Ordering::SeqCst);
                async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        "bad:1.2.3.4-1.2.3.5\n",
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/list.p2p"), hits)
    }

    #[tokio::test]
    async fn a_blocklist_on_the_web_is_fetched_once_a_day_and_kept_beside_the_parts() {
        use std::sync::atomic::Ordering;
        let (address, hits) = blocklist_server(200).await;
        let dir = tempfile::tempdir().unwrap();
        let parts = dir.path().join("parts");
        let client = reqwest::Client::new();
        let first = super::blocklist_file(&client, &format!(" {address} "), &parts)
            .await
            .unwrap();
        let kept = local_file(&first).unwrap();
        assert_eq!(kept.parent(), Some(dir.path()), "beside the parts folder");
        assert_eq!(std::fs::read(&kept).unwrap(), b"bad:1.2.3.4-1.2.3.5\n");
        assert!(!kept.with_extension("part").exists(), "no half copy left");
        let again = super::blocklist_file(&client, &address, &parts)
            .await
            .unwrap();
        assert_eq!(again, first);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a fresh copy is not fetched again"
        );
    }

    #[tokio::test]
    async fn a_blocklist_that_cannot_be_read_is_an_error_not_no_list() {
        let dir = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();
        let (missing, _) = blocklist_server(404).await;
        let why = super::blocklist_file(&client, &missing, dir.path())
            .await
            .unwrap_err()
            .to_string();
        assert!(
            why.contains("could not be read") && why.contains("404"),
            "{why}"
        );
        let nowhere = dir.path().join("no-such-list.p2p");
        let why = super::blocklist_file(&client, &nowhere.display().to_string(), dir.path())
            .await
            .unwrap_err()
            .to_string();
        assert!(why.contains("there is no such file"), "{why}");
    }

    #[tokio::test]
    async fn a_blocklist_on_this_computer_is_handed_over_as_a_file_address() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("list.p2p");
        std::fs::write(&list, b"x").unwrap();
        let client = reqwest::Client::new();
        let address = super::blocklist_file(&client, &list.display().to_string(), dir.path())
            .await
            .unwrap();
        assert_eq!(local_file(&address).as_deref(), Some(list.as_path()));
        // An address already in that spelling goes as it is.
        let given = super::blocklist_file(&client, "file:///x/list.p2p", dir.path())
            .await
            .unwrap();
        assert_eq!(given, "file:///x/list.p2p");
    }

    use super::*;

    /// Librqbit's count dropped (a piece failing its hash, a
    /// check of the files) and the row's progress went back.
    #[test]
    fn reported_progress_never_goes_back() {
        let mut said = None;
        let mut seen = Vec::new();
        for progress in [0, 400, 900, 512, 700, 1200, 1100, 2048, 4096] {
            let done = high_water(said, progress, Some(2048));
            seen.push(done);
            said = Some(done);
        }
        assert_eq!(seen, [0, 400, 900, 900, 900, 1200, 1200, 2048, 2048]);
    }

    /// A move across disks copied straight into the download
    /// folder, so a copy that failed midway left half a tree there looking
    /// finished; and one whose source could not be removed failed the row
    /// beside a whole copy.
    #[cfg(unix)]
    #[test]
    fn a_move_across_disks_is_whole_or_absent_and_a_leftover_is_only_noted() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let holder = tmp.path().join("parts");
        let from = holder.join("Show");
        std::fs::create_dir_all(from.join("a/b")).unwrap();
        std::fs::write(from.join("one.bin"), b"one").unwrap();
        std::fs::write(from.join("a/b/two.bin"), b"two").unwrap();
        let unreadable = from.join("a/b/locked.bin");
        std::fs::write(&unreadable, b"x").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&unreadable).is_ok() {
            // Root reads past permissions (a container): nothing to test.
            return;
        }
        let downloads = tmp.path().join("downloads");
        let to = downloads.join("Show");
        std::fs::create_dir_all(&to).unwrap();

        assert!(
            copy_across(&from, &to).is_err(),
            "an unreadable file copied"
        );
        let listed = |dir: &Path| -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(listed(&downloads), ["Show"], "a staging copy was left");
        assert!(listed(&to).is_empty(), "half a tree in the download folder");

        std::fs::remove_file(&unreadable).unwrap();
        // The source's folder cannot be changed: its removal will fail.
        std::fs::set_permissions(&holder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let moved = copy_across(&from, &to);
        std::fs::set_permissions(&holder, std::fs::Permissions::from_mode(0o755)).unwrap();
        let note = moved.expect("a whole copy is not a failure");
        assert!(note.is_some_and(|n| n.contains("could not be removed")));
        assert_eq!(std::fs::read(to.join("a/b/two.bin")).unwrap(), b"two");
        assert_eq!(listed(&downloads), ["Show"]);

        // And the ordinary case: moved, nothing left behind. (The failed
        // removal above emptied the source but could not remove it.)
        std::fs::write(from.join("one.bin"), b"one").unwrap();
        let again = downloads.join("Again");
        std::fs::create_dir_all(&again).unwrap();
        assert_eq!(copy_across(&from, &again).unwrap(), None);
        assert!(!from.exists());
        assert_eq!(std::fs::read(again.join("one.bin")).unwrap(), b"one");
    }

    #[test]
    fn magnets_and_torrent_files_are_for_the_torrent_engine() {
        assert!(is_torrent("magnet:?xt=urn:btih:0123456789abcdef"));
        assert!(is_torrent("file:///home/a/Some%20Thing.torrent"));
        assert!(is_torrent("  magnet:?xt=urn:btih:x  ")); // pasted with spaces
        assert!(is_torrent("https://example.com/ubuntu.torrent"));
        // A query string is where a signed link keeps its token; the path is
        // still what says .torrent.
        assert!(is_torrent("https://example.com/get.torrent?token=abc"));
        // A scheme and an extension in capitals are the same thing.
        assert!(is_torrent("Magnet:?xt=urn:btih:0123456789abcdef"));
        assert!(is_torrent("https://example.com/X.TORRENT"));
    }

    #[test]
    fn only_a_socks5_proxy_can_carry_a_swarm() {
        assert!(matches!(swarm_proxy(None), Ok(None)));
        let Ok(Some(socks)) = swarm_proxy(Some(concat!("socks5://u:", "p", "@h:1080"))) else {
            panic!("a SOCKS5 proxy was not handed to the swarm");
        };
        assert_eq!(
            socks.proxy_url.as_deref(),
            Some(concat!("socks5://u:", "p", "@h:1080"))
        );
        // What dm's settings hand out, in the spelling librqbit reads.
        let Ok(Some(remote)) = swarm_proxy(Some(concat!("socks5h://u:", "p", "@h:1080"))) else {
            panic!("a SOCKS5 proxy that resolves names was not handed to the swarm");
        };
        assert_eq!(
            remote.proxy_url.as_deref(),
            Some(concat!("socks5://u:", "p", "@h:1080"))
        );
        // An HTTP proxy cannot carry a peer connection, and going around it
        // is the one thing a configured proxy must never see.
        let Err(refused) = swarm_proxy(Some("http://h:8080")) else {
            panic!("an HTTP proxy was accepted for a swarm");
        };
        assert!(refused.to_string().contains("SOCKS5"), "{refused}");
    }

    #[tokio::test]
    async fn a_torrents_own_name_cannot_lead_out_of_the_download_folder() {
        assert_eq!(folder_name("../.config/autostart"), "autostart");
        assert_eq!(folder_name(".."), "torrent");
        assert_eq!(folder_name(""), "torrent");
        assert_eq!(folder_name("Album (2024)"), "Album (2024)");
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("Downloads");
        tokio::fs::create_dir_all(&downloads).await.unwrap();
        let claimed = crate::download::claim_dir(&downloads, &folder_name("../.config/autostart"))
            .await
            .unwrap();
        assert_eq!(claimed.parent(), Some(downloads.as_path()));
    }

    #[test]
    fn ordinary_files_are_not() {
        assert!(!is_torrent("https://example.com/ubuntu.iso"));
        assert!(!is_torrent("https://example.com/video.mp4"));
        // The word appears, but not as the extension: this is a page, not a
        // metainfo file, and fetching it as a torrent would be wrong.
        assert!(!is_torrent("https://example.com/torrent/list"));
        assert!(!is_torrent("https://example.com/a.torrent.html"));
    }
}

/// Whether a file's path inside a torrent stays inside the folder the torrent
/// goes in: plain parts only, and on Windows none that names a drive, a stream
/// or a device, or ends in a dot or space.
fn plain_torrent_path(path: &std::path::Path) -> bool {
    use std::path::Component;
    let mut any = false;
    for part in path.components() {
        let Component::Normal(name) = part else {
            return false;
        };
        any = true;
        let name = name.to_string_lossy();
        if name.contains('\0') {
            return false;
        }
        if cfg!(windows)
            && (name.contains(':')
                || name.ends_with('.')
                || name.ends_with(' ')
                || crate::probe::reserved_on_windows(&name))
        {
            return false;
        }
    }
    any
}

#[cfg(test)]
mod path_tests {
    use super::plain_torrent_path;
    use std::path::Path;

    #[test]
    fn a_torrent_file_path_must_stay_inside_its_folder() {
        assert!(plain_torrent_path(Path::new("album/01 track.mp3")));
        assert!(!plain_torrent_path(Path::new("")));
        assert!(!plain_torrent_path(Path::new("../x")));
        assert!(!plain_torrent_path(Path::new("/etc/x")));
        assert!(!plain_torrent_path(Path::new("album/a\0b.mp3")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_drives_streams_and_devices_are_refused() {
        assert!(!plain_torrent_path(Path::new("D:/x/y.exe")));
        assert!(!plain_torrent_path(Path::new("a/b:stream")));
        assert!(!plain_torrent_path(Path::new("a/NUL")));
        assert!(!plain_torrent_path(Path::new("a/b.")));
    }
}
