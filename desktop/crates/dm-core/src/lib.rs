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
// From Google's Rust style guide, checked in CI: an `unsafe` block says why it
// is sound (`// SAFETY:`, on every platform, Windows included), and a string is
// built with `format!` rather than `+`, and a loop is a loop rather than
// `for_each`.
#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::string_add, clippy::needless_for_each)]
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

pub mod ftp;

pub mod gif;

pub mod hex;

pub mod javascript;

mod limit;

pub mod live;

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

pub mod tools;

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
pub use processes::*;

pub use category::{
    by_name, classify, host_of, one_folder, portable_folder, Categories, Category, Custom,
};

pub use checksum::{digest, Algorithm, Checksum};

pub use clip::Span;

pub use connect_first::{set_connect_first, ConnectFirst, Redial};

pub use control::{Control, Wanted};

pub use download::{
    download, download_reporting, free_name, free_space, names_a_document, nth_free_name, Finished,
    HostLimit, Options, Room, Update,
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

pub use zipinfo::{zip_contents, zip_contents_of_file, ZipEntry};

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
mod tests {
    // An AppImage is Linux's: its path lists are split on ':', which Windows
    // does not split on.
    #[cfg(unix)]
    #[test]
    fn a_helper_started_from_the_appimage_gets_none_of_its_folders() {
        use std::ffi::OsString;
        // What AppRun hands dm on a CachyOS desktop.
        let env = |name: &str| -> Option<OsString> {
            Some(OsString::from(match name {
                "PYTHONHOME" => "/tmp/.mount_dmX/usr/",
                "PYTHONPATH" => "/tmp/.mount_dmX/usr/share/pyshared/:",
                "PYTHONDONTWRITEBYTECODE" => "1",
                "LD_LIBRARY_PATH" => "/tmp/.mount_dmX/usr/lib/:/tmp/.mount_dmX/lib/:/opt/cuda/lib",
                "PATH" => "/tmp/.mount_dmX/usr/bin/:/usr/local/bin:/usr/bin",
                "XDG_DATA_DIRS" => "/usr/local/share:/usr/share",
                _ => return None,
            }))
        };
        let out = super::without_appdir(std::path::Path::new("/tmp/.mount_dmX"), env);
        let get = |n: &str| out.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone());
        assert_eq!(get("PYTHONHOME"), Some(None));
        assert_eq!(get("PYTHONPATH"), Some(None));
        assert_eq!(get("PYTHONDONTWRITEBYTECODE"), Some(None));
        assert_eq!(
            get("LD_LIBRARY_PATH"),
            Some(Some(OsString::from("/opt/cuda/lib")))
        );
        assert_eq!(
            get("PATH"),
            Some(Some(OsString::from("/usr/local/bin:/usr/bin")))
        );
        // Left as it is: nothing of the AppImage's in it.
        assert_eq!(get("XDG_DATA_DIRS"), None);
        assert_eq!(get("QT_PLUGIN_PATH"), None);
    }

    #[test]
    fn a_file_error_is_said_without_its_system_number() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            super::file_error(&Error::from(ErrorKind::NotFound)),
            "it is not there"
        );
        assert_eq!(
            super::file_error(&Error::from(ErrorKind::PermissionDenied)),
            "dm is not allowed to use it"
        );
        assert!(!super::file_error(&Error::from_raw_os_error(5)).contains("os error"));
    }

    use super::readable;

    #[tokio::test]
    async fn a_network_error_says_why_not_only_that_it_failed() {
        // A port nothing listens on: the refusal is the reason, and it is
        // under reqwest's "error sending request".
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let err = super::client()
            .get(format!("http://127.0.0.1:{port}/f"))
            .send()
            .await
            .unwrap_err();
        let said = super::Error::Network(err).to_string();
        assert!(said.to_ascii_lowercase().contains("refused"), "{said}");
    }

    #[test]
    fn an_error_page_is_read_down_to_its_sentence() {
        // Wikimedia's 403, abridged: a head of CSS, then the sentence.
        let page = "<!DOCTYPE html><html lang=\"en\"><meta charset=\"utf-8\">\
            <title>Wikimedia Error</title><style>* { margin: 0; } body { font: 15px }</style>\
            <div class=\"content\"><h1>Error</h1>\
            <p>Too many requests. Please contact noc@wikimedia.org &amp; say so</p></div>";
        assert_eq!(
            readable(page),
            "Error Too many requests. Please contact noc@wikimedia.org & say so"
        );
    }

    #[test]
    fn a_plain_refusal_stays_as_it_was_and_a_long_one_is_cut() {
        assert_eq!(
            readable("  rate limited,\n try later "),
            "rate limited, try later"
        );
        let long = "word ".repeat(100);
        let cut = readable(&long);
        assert!(cut.ends_with('…') && cut.chars().count() <= 201, "{cut}");
    }

    /// A failed row carried the server's error page —
    /// "Error response Error code: 404 Message: File not found. …" — where
    /// "404 Not Found" says it all. A body with a sentence of its own stays.
    #[test]
    fn an_error_page_that_only_restates_the_status_is_not_repeated() {
        let python = "<!DOCTYPE HTML><html lang=\"en\"><head><meta charset=\"utf-8\">\
            <title>Error response</title></head><body><h1>Error response</h1>\
            <p>Error code: 404</p><p>Message: File not found.</p>\
            <p>Error code explanation: 404 - Nothing matches the given URI.</p></body></html>";
        assert_eq!(super::worth_saying(&readable(python), 404), "");
        // A message of its own in the same page is kept.
        let own = python
            .replace("404", "401")
            .replace("File not found", "Token expired");
        assert_eq!(super::worth_saying(&readable(&own), 401), "Token expired");
        let nginx = "<html><head><title>404 Not Found</title></head><body>\
            <center><h1>404 Not Found</h1></center><hr><center>nginx/1.24.0</center></body></html>";
        assert_eq!(super::worth_saying(&readable(nginx), 404), "");
        assert_eq!(super::worth_saying("404 page not found\n", 404), "");
        assert_eq!(super::worth_saying("Forbidden", 403), "");
        assert_eq!(
            super::worth_saying("rate limited, try later", 429),
            "rate limited, try later"
        );
        let wikimedia = "Error Too many requests. Please contact noc@wikimedia.org";
        assert_eq!(super::worth_saying(wikimedia, 429), wikimedia);
        let refused = super::Error::Refused {
            status: 404,
            url: "http://e.com/a".into(),
            body: String::new(),
            retry_after: None,
        };
        assert_eq!(
            refused.to_string(),
            "the server answered 404 Not Found for http://e.com/a"
        );
    }

    #[test]
    fn markup_that_never_closes_ends_the_reading() {
        assert_eq!(readable("before <style> never closed"), "before");
        assert_eq!(readable("before <p never closed"), "before");
    }

    #[cfg(unix)]
    #[test]
    fn dms_files_are_readable_by_this_account_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("dm-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        super::write_atomic(&path, b"[]").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(mode, 0o600, "{mode:o}");
    }

    #[test]
    fn a_windows_long_path_reads_as_a_plain_one() {
        use std::path::PathBuf;
        assert_eq!(
            super::plain_path(PathBuf::from(r"\\?\C:\Program Files\dm\dm.exe")),
            PathBuf::from(r"C:\Program Files\dm\dm.exe")
        );
        assert_eq!(
            super::plain_path(PathBuf::from(r"\\?\UNC\server\share\dm.exe")),
            PathBuf::from(r"\\?\UNC\server\share\dm.exe"),
            "a share keeps its form"
        );
        assert_eq!(
            super::plain_path(PathBuf::from("/usr/bin/dm")),
            PathBuf::from("/usr/bin/dm")
        );
    }

    #[test]
    fn writes_to_one_file_at_once_all_succeed() {
        let dir = std::env::temp_dir().join(format!("dm-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        let writers: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        super::write_atomic(&path, format!("[{i}]").as_bytes()).unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().expect("a write failed");
        }
        let left = std::fs::read_dir(&dir).unwrap().count();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(left, 1, "temporary files were left beside the file");
    }
}
