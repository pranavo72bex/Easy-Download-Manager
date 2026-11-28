// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The name of every file and folder EDM keeps in its home (gaps row A8), each written once.
//! Where the home is comes from `edm_shell::settings::home` (or `EDM_HOME`); what is in it is
//! named here, so a file is renamed in one place and a reader and its writer cannot disagree.
//! These are names on disk: changing one strands what an EDM before it wrote.
//!
//! Most are joined onto the home (`home.join(paths::SETTINGS)`). The queue's companions are
//! kept beside the queue file wherever it is (a test gives it a folder of its own), so they come
//! with a function from that file.

use std::path::{Path, PathBuf};

/// The folder in the system's place for a program's data (`%LOCALAPPDATA%`,
/// `~/Library/Application Support`, `~/.local/share`) that is EDM's home when nothing says
/// otherwise.
pub const HOME_FOLDER: &str = "edm-engine";

/// The folder beside the program that is a portable EDM's home.
pub const PORTABLE_HOME_FOLDER: &str = "edm-data";

/// [`HOME_FOLDER`]'s name before the app was renamed EDM (it was dm).
pub const OLD_HOME_FOLDER: &str = "dm-engine";

/// [`PORTABLE_HOME_FOLDER`]'s name before the rename.
pub const OLD_PORTABLE_HOME_FOLDER: &str = "dm-data";

/// `home`, after moving the folder named `old` beside it into its place when `home` is not
/// there yet: what an app from before the rename kept (its settings, list and window) is
/// this one's. Where the move fails the old folder is used where it is, so nothing is lost.
pub fn adopt_old_home(home: PathBuf, old: &str) -> PathBuf {
    let Some(was) = home.parent().map(|p| p.join(old)) else {
        return home;
    };
    if home.exists() || !was.is_dir() {
        return home;
    }
    match std::fs::rename(&was, &home) {
        Ok(()) => home,
        Err(_) => was,
    }
}

/// Everything a person sets in EDM (`Settings`).
pub const SETTINGS: &str = "settings.json";

/// The list of downloads.
pub const QUEUE: &str = "queue.json";

/// What changed in a long list since its file was last written whole: the queue file's own
/// name with this extension (`queue.journal`).
pub const JOURNAL_EXTENSION: &str = "journal";

/// The rows' logins, cookies and headers, kept out of the queue file and beside it.
pub const SECRETS: &str = "secrets.json";

/// What the download limit has used in its period, beside the queue file.
pub const QUOTA: &str = "quota";

/// When the recheck queue last ran, beside the queue file.
pub const RECHECK_AT: &str = "recheck-at";

/// The extension a JSON file that could not be read is kept under, beside it
/// (`settings.json.unreadable`, `queue.json.unreadable`).
pub const UNREADABLE_EXTENSION: &str = "json.unreadable";

/// The extension a file of secrets that could not be opened is kept under
/// (`secrets.json.unopened`).
pub const UNOPENED_EXTENSION: &str = "json.unopened";

/// What the window is fetching, for the browser's toolbar pop-up.
pub const GOING: &str = "going.json";

/// A newer document of site rules than the one EDM ships, from the update check.
pub const RULES: &str = "rules.json";

/// The DHT nodes a torrent session last knew, one `address:port` a line: the next session
/// starts its search from them as well as the routers, rather than from nothing (gaps row N33).
pub const DHT_NODES: &str = "dht-nodes.txt";

/// The passwords tried on a finished archive that has one, sealed as the sign-ins are (gaps row
/// C2): one a line, the one that last opened an archive first.
pub const ARCHIVE_PASSWORDS: &str = "archive-passwords";

/// A page's video formats, kept a while so a second look asks the site nothing.
pub const FORMATS_CACHE: &str = "formats.cache.json";

/// What the last browser extension to say hello said of itself.
pub const EXTENSION: &str = "extension.json";

/// The browsers whose extension has talked to EDM, a line each.
pub const BROWSER_SEEN: &str = "browser.seen";

/// The folder work is handed to a running EDM in: by the browser's host, a second launch and
/// the command line.
pub const INBOX: &str = "inbox";

/// In the inbox: asks a running EDM to bring its window forward.
pub const INBOX_SHOW: &str = "show";

/// In the inbox: asks a running EDM to start the main queue (`--start-queue`).
pub const INBOX_START_QUEUE: &str = "start-queue";

/// In the inbox: the loopback port a running EDM's doorbell listens on. No extension, so the
/// watcher never reads it as a request.
pub const INBOX_BELL: &str = "bell";

/// In the inbox: asks a running EDM to end.
pub const INBOX_QUIT: &str = "quit";

/// In the inbox: carries `--when-done` to the window.
pub const INBOX_WHEN_DONE: &str = "when-done";

/// In the inbox: carries `--new-download` to the window.
pub const INBOX_NEW_DOWNLOAD: &str = "new-download";

/// In the inbox: the start of a file that asks a running EDM to try a failed download again,
/// its row's id after it.
pub const INBOX_RETRY_PREFIX: &str = "retry-";

/// In the inbox: asks a running EDM to stop every download (a phone's notification's Stop
/// All, gaps row N6).
pub const INBOX_STOP_ALL: &str = "stop-all";

/// The file whose lock says EDM is running.
pub const INSTANCE_LOCK: &str = "instance.lock";

/// What another program had for magnet links, kept while EDM has them (Windows).
pub const MAGNET_BACKUP: &str = "magnet-before-EDM.reg";

/// What another program had for `.torrent` files, kept while EDM has them (Windows).
pub const TORRENT_BACKUP: &str = "torrent-before-EDM.reg";

/// The folder unfinished downloads are staged in when Options names none.
pub const PARTS: &str = "parts";

/// The video site's players EDM has fetched, kept for the next look.
pub const PLAYERS: &str = "players";

/// The port the control port listens on, for a client on this computer to find.
pub const CONTROL_PORT: &str = "control.port";

/// The control port's token, readable by this account alone.
pub const CONTROL_TOKEN: &str = "control.token";

/// The folder the control port's pictures of the window are put in.
pub const SCREENSHOTS: &str = "screenshots";

/// Where and how large the main window was left.
pub const WINDOW: &str = "window.json";

/// Its being there says the welcome has been shown.
pub const WELCOMED: &str = "welcomed";

/// Its being there says the bar offering the browser's extension was closed.
pub const BROWSER_HINT_HIDDEN: &str = "browser-hint.hidden";

/// The start of the file the system's opener writes what it says into, on Linux and macOS
/// (`.opener-<process>-<count>.txt`).
pub const OPENER_PREFIX: &str = ".opener-";

/// In a download's parts folder: the ranges its parts are being fetched under.
pub const PLAN: &str = "plan.json";

/// The queue's journal, beside `queue_file`.
pub fn journal(queue_file: &Path) -> PathBuf {
    queue_file.with_extension(JOURNAL_EXTENSION)
}

/// The secrets of the queue at `queue_file`, beside it.
pub fn secrets(queue_file: &Path) -> PathBuf {
    queue_file.with_file_name(SECRETS)
}

/// What the download limit has used, beside `queue_file`.
pub fn quota(queue_file: &Path) -> PathBuf {
    queue_file.with_file_name(QUOTA)
}

/// When the recheck queue last ran, beside `queue_file`.
pub fn recheck_at(queue_file: &Path) -> PathBuf {
    queue_file.with_file_name(RECHECK_AT)
}

/// Where a JSON file at `path` that could not be read is kept.
pub fn unreadable(path: &Path) -> PathBuf {
    path.with_extension(UNREADABLE_EXTENSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_companions_sit_beside_the_queue_file_under_their_old_names() {
        let queue = Path::new("home").join(QUEUE);
        assert_eq!(journal(&queue), Path::new("home").join("queue.journal"));
        assert_eq!(secrets(&queue), Path::new("home").join("secrets.json"));
        assert_eq!(quota(&queue), Path::new("home").join("quota"));
        assert_eq!(recheck_at(&queue), Path::new("home").join("recheck-at"));
        assert_eq!(
            unreadable(&queue),
            Path::new("home").join("queue.json.unreadable")
        );
        assert_eq!(
            unreadable(&Path::new("home").join(SETTINGS)),
            Path::new("home").join("settings.json.unreadable")
        );
    }

    #[test]
    fn the_home_an_app_from_before_the_rename_kept_is_moved_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join(OLD_HOME_FOLDER);
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join(SETTINGS), "{}").unwrap();
        let home = dir.path().join(HOME_FOLDER);
        assert_eq!(adopt_old_home(home.clone(), OLD_HOME_FOLDER), home);
        assert_eq!(std::fs::read_to_string(home.join(SETTINGS)).unwrap(), "{}");
        assert!(!old.exists());
    }

    #[test]
    fn a_home_already_there_is_kept_and_the_old_one_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join(OLD_HOME_FOLDER);
        let home = dir.path().join(HOME_FOLDER);
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&home).unwrap();
        assert_eq!(adopt_old_home(home.clone(), OLD_HOME_FOLDER), home);
        assert!(old.exists());
        // Neither there: the new one, to be made.
        let fresh = dir.path().join("nothing").join(HOME_FOLDER);
        assert_eq!(adopt_old_home(fresh.clone(), OLD_HOME_FOLDER), fresh);
    }
}
