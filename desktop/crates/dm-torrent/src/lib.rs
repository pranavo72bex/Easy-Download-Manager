// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! BitTorrent's engine, behind dm's own words (gaps rows A6 and P3).
//!
//! The one crate that names `librqbit`, a library compiled in (the recorded exception, row P3).
//! dm-core's `torrent.rs` drives a download and a finished torrent's sharing through what is
//! here: a [`Session`] set up by [`Setup`], a [`Torrent`] added to it from a [`Source`], and
//! that torrent's [`Stats`] and files. What a download means to dm (its options, its stop, the
//! folder it lands in) stays in dm-core; what the swarm is stays here, so the library can be
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

mod error;
pub use error::Error;

/// How a session is set up.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    /// Where its torrents' files are written.
    pub output: PathBuf,
    /// No DHT and no local discovery: behind a proxy (both are UDP, which SOCKS5 cannot carry)
    /// or where a person asked to keep to the trackers.
    pub private: bool,
    /// Reachable by peers on a port of the system's choosing (no UPnP), for sharing.
    pub listen: bool,
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
    /// A session as `setup` asks. Neither the session's state nor the DHT's is kept in a file:
    /// dm's queue is the record of what to resume, and the DHT's file is one per user, so a
    /// second session (two torrents at once, or a resume while the paused one is still closing)
    /// failed to start with it ("error initializing persistent DHT"). Each session finds peers
    /// afresh on a port of its own.
    pub async fn start(setup: Setup) -> Result<Session, Error> {
        let session = librqbit::Session::new_with_opts(
            setup.output,
            librqbit::SessionOptions {
                persistence: None,
                dht: (!setup.private).then(|| librqbit::DhtSessionConfig {
                    persistence: None,
                    ..Default::default()
                }),
                disable_local_service_discovery: setup.private,
                listen: setup.listen.then(librqbit::ListenerOptions::default),
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
