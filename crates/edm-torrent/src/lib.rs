// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! BitTorrent's engine, behind EDM's own words (gaps rows A6 and P3).
//!
//! The one crate that names `librqbit`, a library compiled in (the recorded exception, row P3).
//! edm-core's `torrent.rs` drives a download and a finished torrent's sharing through what is
//! here: a [`Session`] set up by [`Setup`], a [`Torrent`] added to it from a [`Source`], and
//! that torrent's [`Stats`] and files. What a download means to EDM (its options, its stop, the
//! folder it lands in) stays in edm-core; what the swarm is stays here, so the library can be
//! replaced piece by piece behind these types.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use std::collections::HashSet;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use librqbit::{AddTorrent, AddTorrentOptions, TorrentStatsState};
use tokio_util::sync::CancellationToken;

/// One message a peer sent, read from the start of `bytes` as the torrent library reads every
/// peer's: how many bytes it took, or why it could not be read (cut short, unknown, malformed).
/// Never panics, whatever the bytes: the fuzz target `peer_messages` holds it to that (gaps rows
/// H11a, H11b).
pub fn read_peer_message(bytes: &[u8]) -> Result<usize, String> {
    librqbit_peer_protocol::Message::deserialize(bytes, &[])
        .map(|(_, used)| used)
        .map_err(|e| e.to_string())
}

mod error;
pub use error::Error;

/// Public trackers every public torrent is announced to beside its own, as phone torrent
/// apps do: a magnet's trackers are often dead years on (an old film's), and the DHT alone can
/// take a minute to find the first peer. UDP, long-running and open to all; never for a private
/// torrent (librqbit leaves them out of one) nor a private session (behind a proxy, or kept to
/// the trackers), whose UDP would leave without the proxy.
pub const PUBLIC_TRACKERS: [&str; 6] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://explodie.org:6969/announce",
];

/// The trackers a session adds to each of its public torrents: [`PUBLIC_TRACKERS`], or none
/// for a private session or one behind a proxy.
fn session_trackers(setup: &Setup) -> std::collections::HashSet<url::Url> {
    if setup.private || setup.socks5.is_some() {
        return std::collections::HashSet::new();
    }
    PUBLIC_TRACKERS
        .iter()
        .filter_map(|t| url::Url::parse(t).ok())
        .collect()
}

/// The DHT's routers, which any search can start from: more than librqbit's two
/// (`dht.transmissionbt.com`, `dht.libtorrent.org`), as torrent apps use, so one slow or
/// blocked on a network does not leave a magnet with dead trackers waiting.
pub const DHT_ROUTERS: [&str; 5] = [
    "router.bittorrent.com:6881",
    "router.utorrent.com:6881",
    "dht.transmissionbt.com:6881",
    "dht.libtorrent.org:25401",
    "dht.aelitis.com:6881",
];

/// The most nodes kept between sessions, and the most a session starts from beside the
/// routers: enough to reach the network at once, few enough to ask in a moment.
const KEPT_NODES: usize = 200;

/// How often a running session writes the nodes it knows ([`Setup::dht_nodes`]), beside
/// once when it ends.
const KEEP_NODES_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// The addresses to start a DHT search from: the nodes kept in `text` (one `address:port` a
/// line; anything else is passed over), at most [`KEPT_NODES`], then the routers.
fn bootstrap_from(text: &str) -> Vec<String> {
    let mut addrs: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().parse::<SocketAddr>().ok())
        .take(KEPT_NODES)
        .map(|addr| addr.to_string())
        .collect();
    addrs.extend(DHT_ROUTERS.iter().map(|r| (*r).to_string()));
    addrs
}

/// The nodes `nodes` names, one a line, at most [`KEPT_NODES`], written to `file` by way of a
/// file of this process's and this call's own, so two sessions writing at once leave one whole
/// list rather than a mix.
fn keep_nodes(file: &Path, nodes: &[SocketAddr]) -> std::io::Result<()> {
    static EACH: AtomicU64 = AtomicU64::new(0);
    if nodes.is_empty() {
        return Ok(());
    }
    let text: String = nodes
        .iter()
        .take(KEPT_NODES)
        .map(|n| format!("{n}\n"))
        .collect();
    let mut temp = file.as_os_str().to_owned();
    temp.push(format!(
        ".{}.{}",
        std::process::id(),
        EACH.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// How a session is set up.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    /// Where its torrents' files are written.
    pub output: PathBuf,
    /// No DHT and no local discovery: behind a proxy (both are UDP, which SOCKS5 cannot carry)
    /// or where a person asked to keep to the trackers.
    pub private: bool,
    /// Reachable by peers on a port of the system's choosing.
    pub listen: bool,
    /// The router asked to forward the listening port (UPnP), where it listens.
    pub upnp: bool,
    /// The file of DHT nodes a session starts from and leaves for the next ([`DHT_ROUTERS`]
    /// beside them); `None` starts from the routers alone and keeps nothing.
    pub dht_nodes: Option<PathBuf>,
    /// The peers never to connect to: a list's address as the library reads it (`file:`).
    pub blocklist_url: Option<String>,
    /// Shuts the whole session down when cancelled.
    pub cancel: CancellationToken,
    /// The most bytes a second fetched; `None` is no limit.
    pub download_bps: Option<NonZeroU32>,
    /// The most bytes a second sent; `None` is no limit.
    pub upload_bps: Option<NonZeroU32>,
    /// The SOCKS5 proxy (`socks5://...`) trackers and peers are reached through, where one is.
    pub socks5: Option<String>,
    /// The network adapter every socket of the session is bound to (peers, the DHT, trackers,
    /// local discovery, the listening port), by its system name: Options › Only through (gaps
    /// row H23). The library binds by name on macOS, Linux and Android, not on Windows.
    pub bind_device: Option<String>,
}

/// What a torrent is added from.
#[derive(Debug, Clone)]
pub enum Source {
    /// A magnet link, spelled `magnet:` as the library looks for it.
    Magnet(String),
    /// A `.torrent` file's bytes.
    File(bytes::Bytes),
}

/// One session of the engine: its sockets, its peers, the torrents added to it.
pub struct Session(Arc<librqbit::Session>);

/// Where in its session a torrent is.
pub type Id = usize;

impl Session {
    /// A session as `setup` asks. The session's state is not kept in a file (EDM's queue is the
    /// record of what to resume), nor is librqbit's own DHT file used: it is one per user, and a
    /// second session (two torrents at once, or a resume while the paused one is still closing)
    /// failed to start with it ("error initializing persistent DHT"). Each session's DHT has a
    /// port of its own and starts from the nodes [`Setup::dht_nodes`] keeps and the routers.
    pub async fn start(setup: Setup) -> Result<Session, Error> {
        let trackers = session_trackers(&setup);
        let kept = setup
            .dht_nodes
            .as_deref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .unwrap_or_default();
        let cancel = setup.cancel.clone();
        let session = librqbit::Session::new_with_opts(
            setup.output,
            librqbit::SessionOptions {
                persistence: None,
                bind_device_name: setup.bind_device,
                dht: (!setup.private).then(|| librqbit::DhtSessionConfig {
                    persistence: None,
                    bootstrap_addrs: Some(bootstrap_from(&kept)),
                    ..Default::default()
                }),
                disable_local_service_discovery: setup.private,
                trackers,
                listen: setup.listen.then(|| librqbit::ListenerOptions {
                    enable_upnp_port_forwarding: setup.upnp,
                    ..Default::default()
                }),
                blocklist_url: setup.blocklist_url,
                cancellation_token: Some(setup.cancel),
                ratelimits: librqbit::limits::LimitsConfig {
                    download_bps: setup.download_bps,
                    upload_bps: setup.upload_bps,
                },
                connect: setup.socks5.map(|proxy| librqbit::ConnectionOptions {
                    proxy_url: Some(proxy),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        if let (Some(file), Some(dht)) = (setup.dht_nodes, session.get_dht().cloned()) {
            // The nodes it knows written now and then, and once more as it ends, for the next
            // session to start from (gaps row N33).
            tokio::spawn(async move {
                loop {
                    let ended = tokio::select! {
                        () = tokio::time::sleep(KEEP_NODES_EVERY) => false,
                        () = cancel.cancelled() => true,
                    };
                    let nodes: Vec<SocketAddr> = dht.with_routing_tables(|v4, v6| {
                        v4.iter().chain(v6.iter()).map(|n| n.addr()).collect()
                    });
                    let file = file.clone();
                    let _ = tokio::task::spawn_blocking(move || keep_nodes(&file, &nodes)).await;
                    if ended {
                        break;
                    }
                }
            });
        }
        Ok(Session(session))
    }

    /// Adds the torrent `source` names, resuming from and seeding the files already on disk
    /// (a paused torrent that could not overwrite its own parts would start from nothing every
    /// time it resumed). `only` is the files to fetch by their place in it (`None`, every one);
    /// `folder` the folder its files are in, where not the session's. `Ok(None)` where the
    /// engine added it but gave no handle to it.
    pub async fn add(
        &self,
        source: Source,
        only: Option<Vec<usize>>,
        folder: Option<&Path>,
    ) -> Result<Option<Torrent>, Error> {
        let add = match source {
            Source::Magnet(link) => AddTorrent::Url(link.into()),
            Source::File(bytes) => AddTorrent::TorrentFileBytes(bytes),
        };
        let added = self
            .0
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    overwrite: true,
                    output_folder: folder.map(|f| f.to_string_lossy().into_owned()),
                    only_files: only,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(added.into_handle().map(|inner| Torrent {
            inner,
            sent_before: AtomicU64::new(0),
        }))
    }

    /// The session's download ceiling now, in bytes a second; `None` is no limit.
    pub fn download_bps(&self) -> Option<NonZeroU32> {
        self.0.ratelimits.get_download_bps()
    }

    /// Sets the session's download ceiling. Each call makes a fresh limiter with a full
    /// second's allowance: set it only when it changed.
    pub fn set_download_bps(&self, bps: Option<NonZeroU32>) {
        self.0.ratelimits.set_download_bps(bps);
    }

    /// Asks for the files of `torrent` at the places in `files`, and no others.
    pub async fn ask_for(&self, torrent: &Torrent, files: &HashSet<usize>) -> Result<(), Error> {
        self.0
            .update_only_files(&torrent.inner, files)
            .await
            .map_err(|e| Error::from(e.to_string()))
    }

    /// Forgets the torrent `id`, deleting its files where `delete_files` says.
    pub async fn forget(&self, id: Id, delete_files: bool) -> Result<(), Error> {
        self.0
            .delete(id.into(), delete_files)
            .await
            .map_err(|e| Error::from(e.to_string()))
    }

    /// Stops `torrent`'s transfer, its peers let go and its trackers no longer asked, its files
    /// and what it has kept; [`Session::resume`] starts it again. Only a live one is paused:
    /// one still checking its files, already paused or failed is left as it is (`Ok(false)`).
    pub async fn pause(&self, torrent: &Torrent) -> Result<bool, Error> {
        if torrent.stats().state != State::Live {
            return Ok(false);
        }
        // The engine counts what it sent from each start: what this run sent is kept.
        let sent = torrent.inner.stats().uploaded_bytes;
        self.0
            .pause(&torrent.inner)
            .await
            .map_err(|e| Error::from(e.to_string()))?;
        torrent.sent_before.fetch_add(sent, Ordering::SeqCst);
        Ok(true)
    }

    /// Starts a paused `torrent` again: it announces itself afresh to its trackers, the DHT and
    /// local discovery, from the computer's address now. One not paused is left as it is
    /// (`Ok(false)`).
    pub async fn resume(&self, torrent: &Torrent) -> Result<bool, Error> {
        if torrent.stats().state != State::Paused {
            return Ok(false);
        }
        self.0
            .unpause(&torrent.inner)
            .await
            .map_err(|e| Error::from(e.to_string()))?;
        Ok(true)
    }

    /// Announces a live `torrent` again where the computer's network changed (gaps row N8): its
    /// peers, reached from the address the computer had, are let go, and its trackers, the DHT
    /// and local discovery are told the address it has now, so peers find it there. The
    /// listening socket is bound to every address (`[::]`), so it takes peers on the new one
    /// as it is. Its files and pieces are kept, so nothing is checked or fetched again.
    /// `Ok(false)` where it is not live (checking its files, paused or failed).
    pub async fn reannounce(&self, torrent: &Torrent) -> Result<bool, Error> {
        if !self.pause(torrent).await? {
            return Ok(false);
        }
        self.resume(torrent).await
    }

    /// Where the session listens for peers, where it does.
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.0.listen_addr()
    }
}

/// A torrent added to a [`Session`].
pub struct Torrent {
    inner: Arc<librqbit::ManagedTorrent>,
    /// Bytes sent in its runs before this one (each [`Session::pause`] ends one).
    sent_before: AtomicU64,
}

/// One file of a torrent, as its metadata names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    /// Its path inside the torrent, as the swarm wrote it: not to be trusted as a path.
    pub relative: PathBuf,
    /// Its length in bytes.
    pub len: u64,
}

/// What a torrent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Checking what is on disk; its count of bytes is how much it has checked so far.
    Initializing,
    /// Fetching or sharing.
    Live,
    /// Paused.
    Paused,
    /// Failed; [`Stats::error`] says why.
    Error,
}

/// A torrent's state and counts as the engine last saw them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// What it is doing.
    pub state: State,
    /// Why it failed, where it did.
    pub error: Option<String>,
    /// Bytes of the files asked for that it has.
    pub progress_bytes: u64,
    /// Bytes sent to peers since it was added, across its pauses.
    pub uploaded_bytes: u64,
    /// Bytes of the files asked for.
    pub total_bytes: u64,
    /// Whether every file asked for is whole.
    pub finished: bool,
    /// Peers connected now, while it is live.
    pub peers_connected: Option<u32>,
    /// Peers known this run, connected or not, while it is live.
    pub peers_seen: Option<u32>,
    /// Of the peers connected, those that have the whole torrent (seeders), while it is live.
    pub seeders_connected: Option<u32>,
}

impl Torrent {
    /// Where it is in its session.
    pub fn id(&self) -> Id {
        self.inner.id()
    }

    /// Its name, once its metadata is known (for a magnet, after the first peers answer).
    pub fn name(&self) -> Option<String> {
        self.inner.name()
    }

    /// Its info-hash, in hexadecimal.
    pub fn info_hash(&self) -> String {
        self.inner.info_hash().as_string()
    }

    /// The folder its files are written in.
    pub fn output_folder(&self) -> &Path {
        self.inner.output_folder()
    }

    /// Its files, in its own order; an error where its metadata is not known.
    pub fn files(&self) -> Result<Vec<FileInfo>, Error> {
        self.inner
            .with_metadata(|metadata| {
                metadata
                    .file_infos
                    .iter()
                    .map(|file| FileInfo {
                        relative: file.relative_filename.clone(),
                        len: file.len,
                    })
                    .collect()
            })
            .map_err(|e| Error::from(e.to_string()))
    }

    /// Its state and counts now.
    pub fn stats(&self) -> Stats {
        let stats = self.inner.stats();
        let peers = stats.live.as_ref().map(|l| &l.snapshot.peer_stats);
        Stats {
            state: match stats.state {
                TorrentStatsState::Initializing { .. } => State::Initializing,
                TorrentStatsState::Live => State::Live,
                TorrentStatsState::Paused => State::Paused,
                TorrentStatsState::Error => State::Error,
            },
            error: stats.error,
            progress_bytes: stats.progress_bytes,
            uploaded_bytes: self
                .sent_before
                .load(Ordering::SeqCst)
                .saturating_add(stats.uploaded_bytes),
            total_bytes: stats.total_bytes,
            finished: stats.finished,
            peers_connected: peers.map(|p| p.live),
            peers_seen: peers.map(|p| p.seen),
            seeders_connected: peers.map(|p| p.live_seeders),
        }
    }
}

/// What a `.torrent` file says that a copy with fewer trackers keeps: its trackers, in its
/// order, and its info dictionary byte for byte (so the copy is still the same torrent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metainfo {
    /// Every tracker it announces to.
    pub trackers: Vec<String>,
    /// Its info dictionary, as it is in the file.
    pub info: Vec<u8>,
}

/// The [`Metainfo`] of the `.torrent` file `bytes`; an error where it is not one.
pub fn metainfo(bytes: &[u8]) -> Result<Metainfo, Error> {
    let parsed = librqbit::torrent_from_bytes(bytes).map_err(|e| e.to_string())?;
    Ok(Metainfo {
        trackers: parsed
            .iter_announce()
            .map(|t| String::from_utf8_lossy(t.as_ref()).into_owned())
            .collect(),
        info: parsed.info.raw_bytes.as_ref().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    /// A keep-alive is read whole; garbage and a message cut short are errors, not panics.
    #[test]
    fn a_peers_message_is_read_or_refused_never_a_panic() {
        assert_eq!(super::read_peer_message(&[0, 0, 0, 0]), Ok(4));
        assert!(super::read_peer_message(&[0, 0, 0, 5, 4]).is_err());
        assert!(super::read_peer_message(&[0, 0, 0, 2, 250, 1]).is_err());
        for n in 0..300u32 {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + n) as u8).collect();
            let _ = super::read_peer_message(&bytes);
        }
    }

    use super::*;

    /// A number in a `.torrent` past its type's end is refused, not multiplied past it: the
    /// nightly fuzz run's input, which panicked where overflow is checked (vendor/README.md,
    /// `numbers`).
    #[test]
    fn a_number_too_big_for_its_type_is_refused_not_overflowed() {
        assert!(metainfo(b"d1::166666666666661666666666613").is_err());
        assert!(metainfo(b"d4:infod12:piece lengthi99999999999999999999999eee").is_err());
        // The nightly fuzzer's find (issue #72, crash-47a28fb2): a string's length past usize's
        // end, which crates.io's reader overflowed on; the vendored, checked one refuses it.
        assert!(metainfo(b"d77777777777777777777777770\x91\x91\x91d770\x91\x91\x91d0").is_err());
    }

    #[test]
    fn a_public_session_announces_to_every_public_tracker() {
        let trackers = session_trackers(&Setup::default());
        assert_eq!(trackers.len(), PUBLIC_TRACKERS.len());
        assert!(trackers.iter().all(|t| t.scheme() == "udp"));
    }

    #[test]
    fn a_private_session_or_one_behind_a_proxy_adds_none() {
        let private = Setup {
            private: true,
            ..Setup::default()
        };
        assert!(session_trackers(&private).is_empty());
        let proxied = Setup {
            socks5: Some("socks5://127.0.0.1:1080".into()),
            ..Setup::default()
        };
        assert!(session_trackers(&proxied).is_empty());
    }

    /// A session held to one adapter binds every socket to it (gaps row H23): one that names
    /// no adapter of this computer does not start at all, rather than going out another way;
    /// the loopback adapter, which every computer has, starts.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_held_to_an_adapter_binds_to_it_or_does_not_start() {
        let dir = tempfile::tempdir().unwrap();
        let held = |name: &str| Setup {
            output: dir.path().to_path_buf(),
            private: true,
            listen: true,
            bind_device: Some(name.into()),
            ..Setup::default()
        };
        assert!(Session::start(held("edm-no-such-adapter0")).await.is_err());
        let lo = if cfg!(target_os = "macos") {
            "lo0"
        } else {
            "lo"
        };
        let setup = held(lo);
        let cancel = setup.cancel.clone();
        let started = Session::start(setup).await;
        cancel.cancel();
        assert!(started.is_ok(), "{:?}", started.err());
    }
}

#[cfg(test)]
mod kept_nodes_tests {
    use super::*;

    #[test]
    fn a_search_starts_from_the_kept_nodes_then_the_routers() {
        let from = bootstrap_from("1.2.3.4:6881\nnot a node\n[2001:db8::1]:51413\n");
        assert_eq!(from[0], "1.2.3.4:6881");
        assert_eq!(from[1], "[2001:db8::1]:51413");
        assert_eq!(&from[2..], DHT_ROUTERS.map(String::from).as_slice());
        assert_eq!(bootstrap_from("").len(), DHT_ROUTERS.len());
    }

    #[test]
    fn the_nodes_kept_are_read_back_and_no_more_than_the_most() {
        let dir = std::env::temp_dir().join(format!("edm-torrent-nodes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dht-nodes.txt");
        let many: Vec<SocketAddr> = (0..KEPT_NODES as u16 + 50)
            .map(|p| SocketAddr::from(([10, 0, 0, 1], 1000 + p)))
            .collect();
        keep_nodes(&file, &many).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text.lines().count(), KEPT_NODES);
        assert_eq!(bootstrap_from(&text).len(), KEPT_NODES + DHT_ROUTERS.len());
        // Nothing known leaves the last list as it was.
        keep_nodes(&file, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 1, "a temporary file was left beside the list");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
