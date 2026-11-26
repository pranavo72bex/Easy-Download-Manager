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
// dm's own media code is dm-media's (gaps row A6); the engine's paths to it stay.
pub use dm_media::aac;

// The network, sign-ins, the stop flag and the proxy script's interpreter are dm-net's (gaps
// row A6); the engine's paths to them stay.
pub use dm_net::auth;

mod category;

// The clock and calendar (gaps row 215): the time now, this computer's offset, the dates
// servers and people write; dm-net's, so every crate above it reads them the same.
pub use dm_net::clock;

mod checksum;

mod clip;

pub(crate) use dm_net::connect_first;

pub(crate) use dm_net::only_through;

pub(crate) use dm_net::control;

pub use dm_net::cookies;

/// What a row says when the browser's panel handed over a page whose video dm cannot read
/// itself (any site but YouTube) and no file of the video had been seen (gaps row 155).
pub const PAGE_NOT_READ: &str = "dm cannot read this page's video itself (it reads YouTube's \
     pages), and the browser had not seen the video's own files: play the video for a few \
     seconds, then press Download video again";

pub mod dash;

pub(crate) use dm_net::decode;

mod download;
pub mod lookalike;

// What the engine is told for the whole program, put in force in one place (gaps row A11).
mod engine_config;

// Every `DM_` switch of the environment, in one table, and the one way to read one (gaps row A11).
pub mod env;

pub mod extract;

// A file-storage service's share address, turned into the file's (gaps row W7).
mod filehost;

// The video a web page carries, read from the page, for any site (the maintainer, 2026-11-21).
pub mod pagemedia;

pub use dm_media::fmp4;

pub use dm_media::frames;

pub mod finish_trace;
// A feed (RSS, Atom) read as a playlist, to be followed (gaps row W10).
pub mod feed;
// Where a person's files and the system's folders are (gaps row X1).
pub mod folders;
pub mod ftp;

pub mod gif;

pub mod heavy;

pub use dm_net::hex;

pub use dm_net::javascript;

pub(crate) use dm_net::limit;

// dm's one way to log, `note!(warn, "…")` (gaps row A19): dm-net's, used as `dm_core::note!`.
pub use dm_net::note;

pub mod live;

pub mod heap;
#[cfg(target_os = "macos")]
pub mod memory;

mod metalink;

pub mod mkvmux;

pub mod mms;

pub mod disguise;
pub mod motw;

pub mod mp3;

pub mod mp4mux;

pub mod naming;

pub use dm_net::ntlm;

pub use dm_net::pac;

// The name of every file dm keeps in its home (gaps row A8).
pub mod paths;

mod plan;

mod probe;

mod progress;

pub(crate) use dm_net::proxy;

mod queue;

/// Each network's own allowance (gaps row K5c).
pub mod network_allowance;
pub mod quota;

pub use dm_net::redirects;

pub mod retry;

pub mod rtmp;

pub mod rtsp;

// The format number in each file dm saves (gaps row A8).
pub mod saved;

mod schedule;

mod platform;

pub(crate) use dm_net::second_path;

pub mod secretstore;

pub use dm_media::shape;

pub mod sound_language;

mod stream;

pub use dm_media::subtitles;

/// The tests' logins and secrets, never written in the code.
#[cfg(test)]
pub(crate) use dm_test_support::secrets as test_secrets;

pub use second_path::{answer_time, interfaces, is_off, on_second, system_name, Interface};

// A test's own second network alone; the program's is put in force by `configure`.
#[doc(hidden)]
pub use second_path::{set_extra_paths, set_second_client, set_second_path, set_third_path};

mod torrent;

pub mod tsmux;

pub mod videopage;

pub mod youtube;

pub(crate) use dm_net::{agents, error, http_client, processes};
mod files;
mod zipinfo;

/// The HTTP client crate, for callers that take or build a client of the
/// engine's kind without a dependency of their own.
pub use reqwest;

// The crate root's own names, in modules by what they are about; every name is
// still at the root (`crate::lock`, `dm_core::Error`, `dm_core::agent_for`).
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

pub use connect_first::{ConnectFirst, Redial};

// A test's own hook alone; the program's is put in force by `configure`.
#[doc(hidden)]
pub use connect_first::set_connect_first;

// The one network downloads are held to (gaps row W12): a test's own adapter and list alone;
// the program's is put in force by `configure`.
#[doc(hidden)]
pub use only_through::{set_lister as set_adapter_lister, set_only_through};

// Changes each time the network downloads are held to, or its address, changes: a client built
// at another is built again (`DownloadList::client_built_with`).
pub use only_through::generation as network_generation;

// The computer's network changed (gaps row N8): said by whoever drives dm where its system
// tells it first (a phone's), and looked for by running torrents, which announce themselves
// again on the new network.
pub use only_through::{moves as network_moves, network_changed, woke};

pub use control::{Control, Wanted};

pub use download::{
    download, download_reporting, free_name, free_space, names_a_document, nth_free_name,
    nth_free_name_with, read_not_written, slow_part_writes_to, Finished, HostLimit, Options, Room,
    Update, CHECKED_BESIDE, LIVE_PIECES_GONE, NOTES, NOT_UNPACKED_PASSWORD, NOT_UNPACKED_REFUSED,
    PARTS_LEFT_OUT, PREMIERE_IN_A_MOMENT, PREMIERE_IN_HOURS, PREMIERE_IN_MINUTES, PREMIERE_NOTES,
    PREMIERE_NOT_BEGUN, PROTECTED_ABOVE, UNPACKED, UNPACKED_AND_TRASHED, UNPACKED_NOT_TRASHED,
    UNPACK_NOTES,
};

pub use download::same::{SameFile, SameFiles, SAME_FILE};

pub use engine_config::{configure, trust_bench_ca, use_bench_dns, EngineConfig};

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
    accepted, queue_names, saved_rows, shown_url, AddOptions, Connection, DownloadList, Entry,
    Hold, Id, SharedRead, State, LINK_EXPIRED, MAIN_QUEUE, RECHECK_QUEUE, UNFINISHED,
};

pub use schedule::{
    date_now, is_time, minutes as minutes_of, minutes_until_next_start, now_local, weekday_now,
    Schedule,
};

pub use stream::{
    kind_of, media_playlist, pick as pick_quality, titled as stream_titled,
    variants as stream_variants, Kind as StreamKind, Quality, Stream, Variant,
};

#[doc(hidden)]
pub use sound_language::set_sound_language;

pub use torrent::{
    download_torrent, file_url, is_torrent, local_file, merged_files, only_files, sharing,
    sharing_any, stop_sharing, tiers, Priority as TorrentPriority, Share, Sharing, Swarm,
    TorrentFile, TorrentOptions,
};

pub use zipinfo::{
    zip_contents, zip_contents_of_file, zip_take_one, ZipEntry, MOST_TAKEN as ZIP_MOST_TAKEN,
};

/// A lock that is taken even if a thread that held it panicked (dm-net's).
pub use dm_net::lock;

#[cfg(test)]
mod tests;
