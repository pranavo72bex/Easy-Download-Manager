// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! BitTorrent, the one kind of download the browser side cannot do at all.
//!
//! An extension has no sockets, so a magnet link is the clearest case of
//! something only the desktop engine can take. This wraps `librqbit` (through `dm-torrent`,
//! the one crate that names it) in the
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

use dm_torrent::{Session, Setup, Source, State};
use futures_util::StreamExt;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::download::{Finished, Options, Update};
use crate::{category, Error, Progress};

mod blocklist;
mod follow;
mod placing;
mod run;
mod trackers;
use blocklist::blocklist_file;
pub(crate) use follow::hold_sharing;
use follow::{sharing_held, Follow};
#[cfg(all(test, unix))]
use placing::copy_across;
#[cfg(test)]
use placing::folder_name;
use placing::place;
use run::*;
use trackers::{tcp_trackers_of, tcp_trackers_of_magnet};

/// A torrent's swarm as its engine last saw it: shown beside the bytes, as
/// a torrent has peers where a file has connections.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Swarm {
    /// Peers connected now.
    pub connected: u32,
    /// Peers known: every address found for it this run, connected or not.
    pub known: u32,
    /// Of those connected, the seeders: peers with the whole torrent. The rest connected are
    /// leechers, still fetching it themselves.
    pub seeders: u32,
    /// Bytes sent to peers this run.
    pub uploaded: u64,
    /// Bytes a second sent to peers, over the last sample.
    pub upload_speed: f64,
}

impl Swarm {
    /// Peers connected that are still fetching the torrent themselves (leechers).
    pub fn leechers(&self) -> u32 {
        self.connected.saturating_sub(self.seeders)
    }

    /// The swarm as the engine's sample counts it, with the upload speed since
    /// the sample before (`before`, `dt` seconds earlier).
    fn of(stats: &dm_torrent::Stats, before: Option<u64>, dt: f64) -> Swarm {
        let connected = stats.peers_connected.unwrap_or(0);
        let uploaded = stats.uploaded_bytes;
        Swarm {
            connected,
            known: stats.peers_seen.unwrap_or(0).max(connected),
            seeders: stats.seeders_connected.unwrap_or(0).min(connected),
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
    if crate::env::get_os("DM_NO_SWARM").is_some_and(|v| !v.is_empty()) {
        return Err(Error::Internal(
            "torrents are held off the network in this run (DM_NO_SWARM)".into(),
        ));
    }
    if let Some(why) = options.no_route_through_system_proxy(url, "a torrent") {
        return Err(Error::Internal(why));
    }
    // The swarm's sockets are librqbit's, which dm cannot bind to one network: held to one
    // (gaps row W12), a torrent is not started rather than sent around it.
    if let Some(name) = crate::only_through::only_through() {
        return Err(Error::Internal(crate::only_through::refused_words(
            "a torrent",
            &name,
        )));
    }
    let connect = swarm_proxy(options.proxy_for(url).as_deref())?;
    let proxied = connect.is_some();
    // Behind a proxy, or asked to keep to the trackers: no DHT, no local discovery.
    let private = proxied || options.torrent.no_public_discovery;

    /* Staged like every other download, under its own parts directory, and
     * moved into the download folder only when whole. Written straight into
     * the download folder, a paused torrent's half-fetched files would sit
     * there looking finished, and removing the row would leave them: the
     * queue clears a row's parts directory, and a torrent's would be empty. */
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
    /* No state file of the engine's: dm's queue is the record of what to
     * resume. The DHT on, for magnets with no tracker; off behind a proxy, and
     * local discovery with it: both are UDP, which a SOCKS5 connection cannot
     * carry, so they went out directly with the info-hash and the real
     * address. A magnet then needs a tracker. */
    let session = Session::start(Setup {
        output: output.clone(),
        private,
        listen: false,
        blocklist_url,
        cancel: token.clone(),
        download_bps: download_cap(options).await,
        upload_bps: None,
        socks5: connect,
    })
    .await
    .map_err(|e| Error::Internal(format!("the torrent engine would not start: {e}")))?;

    // Dropped on every exit, so a paused or failed torrent does not leave a
    // session listening on a port for the life of the process.
    let _guard = CancelOnDrop(token.clone());

    let add = source_of(client, url, options, proxied).await?;

    // What it was added from, to add it again over the finished files when it is
    // kept shared (gaps row 147).
    let again: Option<Source> = options.torrent.share.map(|_| add.clone());

    /* Raced against the row's control: a magnet's metadata comes from its
     * peers, and with no peer answering the add waits with no end. Awaited
     * on its own, a pause or a cancel of such a magnet never took effect.
     * Stopped here, the guard below cancels the token, which shuts the
     * session and its half-made lookup down with it. */
    let added = crate::download::or_stop(&options.control, async {
        // Over the files already on disk (`Session::add`); the files a person left ticked,
        // where they left some out.
        session
            .add(add, options.torrent.only.clone(), None)
            .await
            .map_err(|e| Error::Internal(format!("the torrent could not be added: {e}")))
    })
    .await?;
    let handle = added.ok_or_else(|| Error::Internal("the torrent produced no handle".into()))?;

    check_files(&session, &handle, &token, options, report.as_ref()).await?;

    /* Tier by tier where the files' priorities differ: the library counts
     * only the files asked for, so the row's total is every wanted file's,
     * and its "finished" ends a tier rather than the torrent. */
    let tiers = options.torrent.tiers.clone();
    let tiered_total = tiered_total(&handle, &tiers);

    transfer(
        &session,
        &handle,
        &token,
        options,
        report.as_ref(),
        &tiers,
        tiered_total,
    )
    .await?;

    let (name, bytes, staged) = finished_files(&handle, options)?;

    // Forgotten by the session, files kept, so nothing holds them open
    // while they are moved into place.
    let id = handle.id();
    drop(handle);
    session
        .forget(id, false)
        .await
        .map_err(|e| Error::Internal(format!("the finished torrent could not be released: {e}")))?;
    drop(session);

    let (path, category, verified, left_behind) = place(&staged, &name, options).await?;

    share_finished(options, url, again, &staged, &path, bytes);

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
    again: Source,
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
    again: Source,
    only: Option<Vec<usize>>,
    size: u64,
    share: Share,
    proxy: Option<String>,
    no_public_discovery: bool,
    token: &CancellationToken,
) -> Result<(), Error> {
    let connect = swarm_proxy(proxy.as_deref())?;
    let proxied = connect.is_some();
    let private = proxied || no_public_discovery;
    let upload_bps = u32::try_from(share.upload_kbps.saturating_mul(crate::KB))
        .ok()
        .and_then(NonZeroU32::new);
    // As the download's own session (see there): no UDP behind a proxy.
    let session = Session::start(Setup {
        output: folder.clone(),
        private,
        // Reachable by peers: sharing is what the person turned on. Not behind a proxy, which
        // would show the real address.
        listen: !proxied,
        blocklist_url: None,
        cancel: token.clone(),
        // Nothing is fetched: the files are whole, or the sharing ends.
        download_bps: NonZeroU32::new(1),
        upload_bps,
        socks5: connect,
    })
    .await
    .map_err(|e| Error::Internal(format!("its session would not start: {e}")))?;
    let _guard = CancelOnDrop(token.clone());
    let added = session
        .add(again, only, Some(&folder))
        .await
        .map_err(|e| Error::Internal(format!("it could not be added again: {e}")))?;
    let handle = added.ok_or_else(|| Error::Internal("it produced no handle".into()))?;
    let listening = session.listen_addr();
    let mut deadline = Instant::now() + Duration::from_secs(share.minutes.saturating_mul(60));
    let goal = (size as f64 * share.ratio).ceil() as u64;
    let mut follow = Follow::new();
    // Since when its sharing is held with the downloads (`hold_sharing`), while it is.
    let mut held_since: Option<Instant> = None;
    loop {
        if token.is_cancelled() {
            break;
        }
        let stats = handle.stats();
        match stats.state {
            State::Initializing => {}
            State::Error => {
                break;
            }
            _ if !stats.finished => {
                let _ = session.forget(handle.id(), false).await;
                return Err(Error::Internal("its files are no longer whole".into()));
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
        if sent >= goal || (held_since.is_none() && Instant::now() >= deadline) {
            break;
        }
        if sharing_held() {
            // Paused while live, each tick: one still checking its files is paused once live.
            let _ = session.pause(&handle).await;
            held_since.get_or_insert_with(Instant::now);
        } else if let Some(since) = held_since.take() {
            // The time held is not counted, and started again it announces itself afresh.
            deadline += since.elapsed();
            let _ = session.resume(&handle).await;
            follow.settle();
        } else {
            follow.follow(&session, &handle).await;
        }
        tokio::select! {
            () = token.cancelled() => break,
            () = tokio::time::sleep(TICK) => {}
        }
    }
    let _ = session.forget(handle.id(), false).await;
    Ok(())
}

/// What a torrent reports as downloaded: the engine's count, held to the
/// total and never below what was reported before.
fn high_water(before: Option<u64>, progress: u64, total: Option<u64>) -> u64 {
    progress
        .min(total.unwrap_or(u64::MAX))
        .max(before.unwrap_or(0))
}

/// How the swarm is reached when a proxy is configured.
///
/// The engine can carry trackers and peers over SOCKS5 and nothing else. An
/// HTTP proxy cannot carry a peer connection, so a torrent on a machine with
/// one configured is refused with the reason rather than sent out around it.
fn swarm_proxy(proxy: Option<&str>) -> Result<Option<String>, Error> {
    let Some(proxy) = proxy else {
        return Ok(None);
    };
    /* The engine names SOCKS5 `socks5://` and refuses `socks5h://`, which is
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
    Ok(Some(proxy))
}

/// The shared download ceiling, in the shape the engine wants it.
///
/// This torrent's part of the limiter every HTTP download shares, re-read
/// every tick, so one ceiling governs files, torrents and video runs
/// together as they start and stop. Zero
/// — no limit — becomes `None`, which is the engine's word for the same thing.
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
mod tests;

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
