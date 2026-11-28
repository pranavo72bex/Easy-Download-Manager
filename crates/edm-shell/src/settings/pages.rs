// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The settings each page of Options edits, a struct a page (gaps row A8): what
//! `edm_desktop_base::options::tab_settings` names for the page is the page's struct here.
//! [`Settings`](super::Settings) holds each with `#[serde(flatten)]`, so `settings.json` keeps
//! every key where it was, beside the settings no page edits: a file an older EDM wrote reads
//! here, and one written here reads in an older EDM.

use std::path::PathBuf;

use edm_core::{Category, Proxy};
use serde::{Deserialize, Serialize};

use super::{QueueLimit, QueueStart, SiteRule};

/// Options › General: the language, the look, the start, the sounds and the windows EDM
/// opens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Watch the clipboard for links and offer to download them. Off by
    /// default: people copy URLs constantly, so this asks rather than grabs.
    #[serde(default)]
    pub watch_clipboard: bool,
    /// Start with the computer, so a browser hand-off has an app to land in
    /// without anyone opening it first. On for a new install, in the tray
    /// with no window; Options › General turns it off.
    #[serde(default)]
    pub start_on_login: bool,
    /// Open EDM's control port (loopback only, a token only this account can
    /// read) at every start, so `edm-desktop list`, `pause`, `resume` and
    /// `cancel` find the running EDM. Off unless turned on: otherwise EDM
    /// listens on nothing, and the port exists only under `--control`.
    #[serde(default)]
    pub allow_command_line: bool,
    /// Let an AI assistant use EDM over MCP (`edm-desktop mcp`): its requests
    /// are answered only while this is on, and it opens the control port at
    /// start as `allow_command_line` does. Off in a new install, by EDM's own
    /// rule that nothing listens unless turned on (gaps row F7); a settings
    /// file that already says either keeps it.
    #[serde(default)]
    pub allow_mcp: bool,
    /// Look for a newer EDM about once a week. Off unless turned on: it asks
    /// GitHub, and EDM otherwise contacts nothing but what is downloaded.
    pub check_updates: bool,
    /// The window's language, by code ("es", "pt-BR"); empty follows the
    /// system's where EDM has it.
    pub language: String,
    /// Open a progress window for a download as it starts, as reference A
    /// does (on by default since edition 2).
    #[serde(default)]
    pub show_progress: bool,
    /// Say so in a window when a download finishes, with Open and Open
    /// folder, as reference A does (on by default since edition 2).
    #[serde(default)]
    pub show_complete: bool,
    /// Play the system's short sound when a download finishes.
    pub sound_on_complete: bool,
    /// A video from the browser's panel: "Download details" opens for it,
    /// to name it and choose where it goes, before it starts.
    pub ask_panel_downloads: bool,
    /// A download caught from the browser: "Download details" opens for it
    /// too, rather than it starting at once, as reference A's File Info
    /// does (on by default since edition 2).
    #[serde(default)]
    pub ask_caught_downloads: bool,
    /// The window's look: "system" (the modern look, light or dark as the
    /// computer is), "light", "dark", or "classic" (a classic Windows look).
    pub theme: String,
    /// What a double-click on a row does: "properties", "open" (a finished
    /// file; Properties for one not finished) or "folder" (its folder).
    pub double_click: String,
    /// What the window's close button does: "ask" (the first time, and the
    /// answer is kept here), "tray" (EDM goes on running in the tray) or
    /// "exit".
    pub on_close: String,
    /// A finished file takes the date its server says it last changed,
    /// rather than when it was fetched. Off by default.
    pub server_dates: bool,
    /// Play the system's short failure sound when a download fails.
    #[serde(default)]
    pub sound_on_failure: bool,
    /// Windows 11's Mica behind the window's frame, from the next start.
    /// Off unless chosen: it needs a see-through window, which an old GPU
    /// driver can draw black.
    #[serde(default)]
    pub mica: bool,
    /// Open "Update download link" by itself when a download's signed link
    /// has run out part-way. Off, the row says it failed and its menu has
    /// Update download link.
    #[serde(default = "super::yes")]
    pub offer_new_link: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        GeneralSettings {
            watch_clipboard: false,
            start_on_login: true,
            allow_command_line: false,
            allow_mcp: false,
            check_updates: false,
            language: String::new(),
            show_progress: true,
            show_complete: true,
            sound_on_complete: false,
            ask_panel_downloads: true,
            ask_caught_downloads: true,
            theme: "system".into(),
            double_click: "open".into(),
            on_close: "ask".into(),
            server_dates: false,
            sound_on_failure: false,
            mica: false,
            offer_new_link: true,
        }
    }
}

/// Options › Browser: which downloads the browser's extension takes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserSettings {
    /// Which downloads the browser extension takes from the browser —
    /// Options › Browser. Kept here, in the app, and sent to the extension
    /// when it asks (`settings.get`), so there is one list and it is the
    /// app's. Spelled as the protocol spells it: `minSizeKb`, `skipHosts`.
    #[serde(default)]
    pub catch: edm_protocol::CatchRules,
}

/// Options › Folders: where downloads go, sorted by kind or not, and where they are staged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderSettings {
    /// Where finished files land.
    pub download_dir: PathBuf,
    /// Sort finished files into folders by category. On for a new install.
    pub categorize: bool,
    /// What each of those folders is called, in `Category::ALL` order. An
    /// empty name leaves that kind unsorted.
    pub category_folders: Vec<String>,
    /// Where unfinished downloads are staged; empty for EDM's own folder.
    /// A download already begun keeps the folder it began in.
    #[serde(default)]
    pub parts_dir: String,
    /// "Unpack when done": the kinds whose finished archive (a ZIP, 7z or tar) is unpacked
    /// into a new folder of its name beside it (gaps row W14). None in a new install.
    #[serde(default)]
    pub unpack_kinds: Vec<Category>,
    /// Once an archive is unpacked when done, it is sent to the Trash rather than kept.
    #[serde(default)]
    pub unpack_trash: bool,
}

impl Default for FolderSettings {
    fn default() -> Self {
        FolderSettings {
            download_dir: default_download_dir(),
            categorize: true,
            category_folders: default_folders(),
            parts_dir: String::new(),
            unpack_kinds: Vec::new(),
            unpack_trash: false,
        }
    }
}

/// Where finished files land in a new install.
#[cfg(not(target_os = "android"))]
fn default_download_dir() -> PathBuf {
    // A `EDM` folder inside the one the operating system already
    // nominates. A download manager that puts files anywhere else is
    // one people have to go looking in.
    edm_core::folders::downloads()
        .unwrap_or_else(|| edm_core::folders::home().unwrap_or_default())
        .join("EDM")
}

/// Where finished files land in a new install on a phone (gaps row N2): the folder the app
/// gave for them (`edm_core::folders::set_phone_folders`), which is EDM's own already; before
/// it has said, a folder inside EDM's home, never a relative path.
#[cfg(target_os = "android")]
fn default_download_dir() -> PathBuf {
    edm_core::folders::downloads().unwrap_or_else(|| super::home().join("Downloads"))
}

/// Options › Downloads: connections, files at once, the queues' own limits and starts, streams,
/// and what runs after each download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    /// Connections per file.
    pub connections: usize,
    /// Files transferring at once.
    pub concurrency: usize,
    /// Which rendition to take from an adaptive stream: `best`, `audio`, or
    /// a height.
    pub stream_quality: String,
    /// A folder EDM reads every few seconds for links saved there: a `.url`,
    /// a `.webloc`, a text file of links or a `.torrent` is added, then
    /// moved into a `done` folder inside it. Empty is off.
    #[serde(default)]
    pub watch_folder: String,
    /// Minutes a live stream is recorded for before EDM ends it and keeps
    /// what it has; 0 records until the stream ends or Stop.
    #[serde(default)]
    pub record_minutes: u64,
    /// A command run on each finished file, its path added as the last
    /// argument: a virus checker, or a script of the person's own. Empty
    /// for none.
    #[serde(default)]
    pub on_complete: String,
    /// Hours between the recheck queue asking its servers again on its
    /// own. Zero asks only when "Start queue" is clicked on it.
    #[serde(default)]
    pub recheck_hours: u64,
    /// Start the Main queue as EDM opens, so what was added for later goes
    /// at the next launch without a click.
    #[serde(default)]
    pub start_queue_on_launch: bool,
    /// Carry a video's subtitles inside it.
    #[serde(default)]
    pub subtitles: bool,
    /// Queues that run fewer files at once than `concurrency`.
    #[serde(default, deserialize_with = "super::lenient::queue_limits")]
    pub queue_limits: Vec<QueueLimit>,
    /// Queues that start on their own each day at a time of their own.
    #[serde(default)]
    pub queue_starts: Vec<QueueStart>,
    /// Put a `.ts` stream into an MP4 (EDM's own remux).
    #[serde(default = "super::yes")]
    pub remux_ts: bool,
    /// The language a video's sound is taken in where it offers several (a
    /// dub): a language tag (`es`, `pt-BR`); empty, the default, takes each
    /// video's own (gaps row Y10). Read by YouTube, HLS and DASH alike.
    #[serde(default)]
    pub sound_language: String,
    /// Days after which a finished download leaves the list, its file left where it was saved
    /// (gaps row W17); 0, the default, keeps every one.
    #[serde(default)]
    pub forget_after_days: u32,
    /// What downloading does on a metered connection (a phone's hotspot, a capped plan), as the
    /// system says (gaps row W11): `go` on, the default; `ask` once each time the connection
    /// becomes metered; or `wait` until it is not. Any other word is read as `go`.
    #[serde(default = "go_on")]
    pub on_metered: String,
    /// On its battery below this percent, the computer's downloads wait until it charges or is
    /// above it again (gaps row W11); 0, the default, never waits.
    #[serde(default)]
    pub battery_below: u32,
    /// On a phone, every download waits off Wi-Fi and a cable: on mobile data (gaps row N10).
    /// Off by default; a computer's Options do not show it and nothing there reads it.
    #[serde(default)]
    pub wifi_only: bool,
    /// On a phone, every download waits while the phone is roaming (gaps row N10).
    #[serde(default)]
    pub not_roaming: bool,
    /// On a phone, every download waits while the system's Data Saver is on (gaps row N10).
    #[serde(default)]
    pub data_saver_waits: bool,
    /// On a phone, downloads and torrents (a torrent's sharing included) go on when EDM is
    /// closed (swiped away from the recent apps), under the ongoing notification, until they
    /// are done (the maintainer, 2026-11-21). Off by default; a computer's Options do not show
    /// it and nothing there reads it.
    #[serde(default)]
    pub keep_after_close: bool,
}

/// `on_metered`'s default: downloading goes on.
fn go_on() -> String {
    METERED_GO.to_string()
}

/// `on_metered`: downloading goes on over a metered connection.
pub const METERED_GO: &str = "go";
/// `on_metered`: asked once each time the connection becomes metered.
pub const METERED_ASK: &str = "ask";
/// `on_metered`: downloads wait for a connection that is not metered.
pub const METERED_WAIT: &str = "wait";

impl Default for DownloadSettings {
    fn default() -> Self {
        DownloadSettings {
            connections: 16,
            concurrency: 3,
            stream_quality: default_quality(),
            watch_folder: String::new(),
            record_minutes: 0,
            on_complete: String::new(),
            recheck_hours: 0,
            start_queue_on_launch: false,
            subtitles: false,
            queue_limits: Vec::new(),
            queue_starts: Vec::new(),
            remux_ts: true,
            sound_language: String::new(),
            forget_after_days: 0,
            on_metered: go_on(),
            battery_below: 0,
            wifi_only: false,
            not_roaming: false,
            data_saver_waits: false,
            keep_after_close: false,
        }
    }
}

/// Options › Advanced: the speed limit, retries and timeouts, the proxy, dial-up, the cookies
/// file and each site's own limits and rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    /// Kilobytes a second across everything. Zero is no limit.
    ///
    /// Kilobytes rather than bytes because that is the unit people think in
    /// for a link.
    pub speed_limit: u64,
    /// Where downloads connect through.
    #[serde(deserialize_with = "super::lenient::proxy")]
    pub proxy: Proxy,
    /// A dial-up or VPN connection every download waits for (Windows; Options › Advanced ›
    /// Dial Up / VPN). Off unless the person turns it on.
    #[serde(default)]
    pub dialup: crate::dialup::DialUp,
    /// Seconds to wait between attempts after a failure; 0 lets EDM choose
    /// (quick at first, then half a minute apart).
    pub retry_wait: u64,
    /// A `cookies.txt` (Netscape format) whose cookies EDM sends with its own
    /// requests, for downloads that need a signed-in session. Empty for none.
    #[serde(default)]
    pub cookies_file: String,
    /// How many times one connection tries before the download fails, for
    /// a failure waiting can fix. Zero keeps trying until it gets through
    /// or is paused.
    #[serde(default = "default_attempts")]
    pub attempts: u32,
    /// Seconds a connection may send nothing before EDM gives up on it and
    /// tries again.
    #[serde(default = "default_read_timeout")]
    pub read_timeout: u64,
    /// Sites that take fewer connections at once than `connections`.
    #[serde(default, deserialize_with = "super::lenient::host_limits")]
    pub host_limits: Vec<edm_core::HostLimit>,
    /// A folder and a speed limit for a site's downloads (Options › Advanced).
    #[serde(default, deserialize_with = "super::lenient::site_rules")]
    pub site_rules: Vec<SiteRule>,
    /// The one network every download is held to (Options › Advanced › Network › Only
    /// through, gaps row W12), by its adapter's name (`edm_core::interfaces`); empty for any.
    /// Chosen, the second network is not used.
    #[serde(default)]
    pub only_through: String,
    /// The speed limit, the slow hours' included, holds only while someone is at the computer
    /// (gaps row C5): away, downloads go at full speed.
    #[serde(default)]
    pub limit_only_present: bool,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        AdvancedSettings {
            speed_limit: 0,
            // The computer’s proxy settings, as a browser takes them: with none set it goes
            // direct; behind one, a new EDM works where the browser does
            // (docs/decisions.md, 2026-11-06, in place of gaps row 80).
            proxy: Proxy {
                enabled: true,
                kind: edm_core::ProxyKind::System,
                ..Proxy::default()
            },
            dialup: crate::dialup::DialUp::default(),
            retry_wait: 0,
            cookies_file: String::new(),
            attempts: default_attempts(),
            read_timeout: default_read_timeout(),
            host_limits: Vec::new(),
            site_rules: Vec::new(),
            only_through: String::new(),
            limit_only_present: false,
        }
    }
}

fn default_attempts() -> u32 {
    15
}

fn default_read_timeout() -> u64 {
    30
}

fn default_quality() -> String {
    "best".into()
}

fn default_folders() -> Vec<String> {
    Category::ALL
        .iter()
        .map(|c| c.folder().to_string())
        .collect()
}
