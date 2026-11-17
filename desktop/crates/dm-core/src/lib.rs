// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's download engine.
//!
//! Deliberately free of any window. Everything a download manager is judged
//! on — several connections at once, resume, throughput, not corrupting the
//! file — lives here, where it can be tested against a real server in
//! milliseconds instead of driven through a user interface.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
pub mod aac;

pub mod auth;

mod category;

mod checksum;

mod clip;

mod connect_first;

mod control;

pub mod cookies;

/// What a row says when the browser's panel handed over a page whose video dm cannot read
/// itself (any site but YouTube) and no file of the video had been seen (gaps row 155).
pub const PAGE_NOT_READ: &str = "dm cannot read this page's video itself (it reads YouTube's \
     pages), and the browser had not seen the video's own files: play the video for a few \
     seconds, then press Download video again";

pub mod dash;

mod decode;

mod download;

pub mod extract;

pub mod fmp4;

pub mod frames;

mod dns;
pub mod finish_trace;
pub mod ftp;

pub mod gif;

pub mod heavy;

pub mod hex;

pub mod javascript;

mod limit;

pub mod live;

pub mod heap;
#[cfg(target_os = "macos")]
pub mod memory;

mod metalink;

#[cfg(windows)]
pub(crate) mod mf;

pub mod mkvmux;

pub mod mms;

pub mod motw;

pub mod mp3;

pub mod mp4mux;

pub mod naming;

pub mod ntlm;

pub mod pac;

mod plan;

mod probe;

mod progress;

mod proxy;

mod queue;

pub mod quota;

pub mod redirects;

pub mod retry;

pub mod rtmp;

pub mod rtsp;

mod schedule;

mod second_path;

pub mod secretstore;

pub mod shape;

mod stream;

pub mod subtitles;

/// The tests' logins and secrets, never written in the code.
#[cfg(test)]
#[path = "../../../test-support/secrets.rs"]
pub(crate) mod test_secrets;

pub use second_path::{
    interfaces, on_second, set_second_client, set_second_path, system_name, Interface,
};

mod torrent;

pub mod tsmux;

pub mod videopage;

pub mod youtube;

mod agents;
mod error;
mod files;
mod http_client;
mod processes;
mod zipinfo;

/// The HTTP client crate, for callers that take or build a client of the
/// engine's kind without a dependency of their own.
pub use reqwest;

// What lib.rs held itself until 2026-11-04, by what it is about; every name is
// where it was (`crate::lock`, `dm_core::Error`, `dm_core::agent_for`).
pub use agents::*;
pub use error::*;
pub use files::*;
pub use http_client::*;

/// A kilobyte as this computer's own file manager counts one (gaps row U34): 1,000 bytes on macOS
/// and Linux (Finder, GNOME Files, Dolphin), 1,024 on Windows (Explorer). Sizes are shown in it
/// and speed limits typed in KB/s mean it, so a limit and the speed shown beside it agree.
pub const KB: u64 = if cfg!(windows) { 1024 } else { 1000 };
pub use processes::*;

pub use category::{
    by_name, classify, host_of, one_folder, portable_folder, Categories, Category, Custom,
};

pub use checksum::{digest, Algorithm, Checksum};

pub use clip::Span;

pub use connect_first::{set_connect_first, ConnectFirst, Redial};

pub use control::{Control, Wanted};

pub use download::{
    download, download_reporting, free_name, free_space, names_a_document, nth_free_name,
    nth_free_name_with, slow_part_writes_to, Finished, HostLimit, Options, Room, Update,
    CHECKED_BESIDE, NOTES,
};

pub use limit::Limiter;

pub use metalink::{is_metalink, parse as parse_metalink, Metalink};

pub use plan::{plan, record as record_plan, Segment, MAX_CONNECTIONS};

pub use probe::{
    decode as decode_percent, filename_for, mime_essence, probe, sanitize as sanitize_name, Probe,
};

pub use progress::Progress;

pub use proxy::{
    client_with, client_with_timeout, route_for as proxy_route_for, system_proxy_in_force,
    Kind as ProxyKind, Proxy, SchemeProxy,
};

pub use queue::{
    accepted, queue_names, shown_url, AddOptions, Connection, Entry, Id, Queue, State,
    LINK_EXPIRED, MAIN_QUEUE, RECHECK_QUEUE, UNFINISHED,
};

pub use schedule::{
    is_time, minutes as minutes_of, minutes_until_next_start, now_local, weekday_now, Schedule,
};

pub use stream::{
    kind_of, media_playlist, pick as pick_quality, variants as stream_variants, Kind as StreamKind,
    Quality, Stream, Variant,
};

pub use torrent::{
    download_torrent, file_url, is_torrent, local_file, merged_files, only_files, sharing,
    sharing_any, stop_sharing, tiers, Priority as TorrentPriority, Share, Sharing, Swarm,
    TorrentFile, TorrentOptions,
};

pub use zipinfo::{
    zip_contents, zip_contents_of_file, zip_take_one, ZipEntry, MOST_TAKEN as ZIP_MOST_TAKEN,
};

/// A lock that is taken even if a thread that held it panicked.
///
/// The data under dm's plain mutexes — the problem list, the schedule, the
/// window's settings — stays meaningful after a panic elsewhere, and a
/// second panic over a poisoned lock would turn one failure into a crash.
pub fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
