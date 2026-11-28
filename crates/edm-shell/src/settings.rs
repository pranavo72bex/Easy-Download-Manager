// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the person using EDM has chosen, and where it is kept.
//!
//! Written on every change rather than on exit. A preferences file that only
//! survives a clean shutdown is one that loses your settings the first time
//! something crashes, which is the moment you were most likely changing them.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use edm_core::Schedule;
use serde::{Deserialize, Serialize};

// What the tests name through `super::*` and this file no longer uses itself.
#[cfg(test)]
use edm_core::{AddOptions, Category, Limiter, Quality};
#[cfg(test)]
use std::path::PathBuf;

pub use pages::{
    AdvancedSettings, BrowserSettings, DownloadSettings, FolderSettings, GeneralSettings,
};
// What downloading does on a metered connection (gaps row W11).
pub use pages::{METERED_ASK, METERED_GO, METERED_WAIT};
pub use paths::{enter_portable, full_folder, full_path, home, home_short, portable_home_in};
pub use paths::{PORTABLE_DATA, PORTABLE_MARKER};
pub use scale::{PanelWidths, TextSizes, SIDEBAR_WIDTH, TEXT_SIZE};
pub use sites::{SiteLogin, SiteRule};

/// Everything a person sets in EDM, as `settings.json` keeps it: the window's
/// Options, the list's columns and sort, the side panel, and what the
/// browser's extension is told. A field missing from the file takes its
/// default, so an older file still reads.
///
/// What a page of Options edits is that page's struct (`pages.rs`), flattened: the file keeps
/// one object with every key at its top, as before the pages had structs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Options › General.
    #[serde(flatten)]
    pub general: GeneralSettings,
    /// Options › Browser.
    #[serde(flatten)]
    pub browser: BrowserSettings,
    /// Options › Folders.
    #[serde(flatten)]
    pub folders: FolderSettings,
    /// Options › Downloads.
    #[serde(flatten)]
    pub downloads: DownloadSettings,
    /// Options › Advanced.
    #[serde(flatten)]
    pub advanced: AdvancedSettings,
    /// The limit the speed limiter last had on, in KB/s, so turning it back
    /// on — in this run or the next — restores it. Zero for never set.
    #[serde(default)]
    pub last_speed_limit: u64,
    /// A lower speed limit for set hours of every day (when others share
    /// the link), beside the one for the rest of the day.
    #[serde(default)]
    pub slow_hours: SlowHours,
    /// Send EDM's own name to servers first, not the browser-like one
    /// (Options › Advanced › Identify as EDM only). A site that refuses the
    /// first name is still asked again with the others.
    #[serde(default)]
    pub identify_as_dm: bool,
    /// What to do when an address typed in or caught is already in the list,
    /// remembered from the question's tick: `""` asks, or `"show"`,
    /// `"again"`, `"replace"`. Options › General undoes it.
    #[serde(default)]
    pub duplicate_choice: String,
    /// What to do when a download is the same file as one already here (its name, size and
    /// entity tag those of a finished download, or a file of its name and size in the folder),
    /// remembered from the question's tick: `""` asks, or `"show"`, `"again"` (gaps row W18).
    /// Options › General undoes it. Left out of the file while it asks.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub same_file_choice: String,
    /// Addresses typed into Add URL, newest first, without any login in
    /// them, for picking again; cleared from the dialog.
    #[serde(default)]
    pub recent_urls: Vec<String>,
    /// The folder last chosen in Download details for each kind of file,
    /// by its folder name (`Category::folder`), offered first the next
    /// time a file of that kind is added there.
    #[serde(default)]
    pub last_folders: std::collections::BTreeMap<String, String>,
    /// The download limit: no more than this many megabytes
    /// every `limit_hours` hours. Zero is none.
    pub limit_mb: u64,
    /// The span the download limit counts over, in hours.
    pub limit_hours: u64,
    /// Categories a person made: a name that is also a folder, file types,
    /// and optionally the only sites they come from. Looked at before the
    /// built-in kinds.
    #[serde(deserialize_with = "lenient::custom_categories")]
    pub custom_categories: Vec<edm_core::Custom>,
    /// When the queue may run.
    #[serde(deserialize_with = "lenient::schedule")]
    pub schedule: Schedule,
    /// A schedule of its own for a named queue, by the queue's name; a queue
    /// with none uses `schedule`.
    pub queue_schedules: std::collections::BTreeMap<String, Schedule>,
    /// Set when this run could not read its passwords back (the store refused, or the file
    /// was read only in part or not at all, so its defaults stand in): an empty password then
    /// says nothing about what the person wants, and the one in the store is not forgotten on
    /// the next save (security review). Never written to the file.
    #[serde(skip)]
    pub keep_stored_passwords: bool,
    /// A link copied while the clipboard is watched is taken at once, its
    /// Download details opened (several, the list of them), rather than
    /// offered in a bar first. Off by default, as reference A's.
    #[serde(default)]
    pub start_copied: bool,
    /// A download shown in Download details starts fetching at once, while the person still
    /// chooses where it goes; Start now places it, Cancel removes it. Off by default, as
    /// reference A's "Start downloading immediately while displaying" (gaps row 85).
    #[serde(default)]
    pub start_while_asking: bool,
    /// Where the list of peers a torrent never connects to is: an address on
    /// the web or a file on this computer, in the P2P format, plain or
    /// gzipped. Empty for none.
    #[serde(default)]
    pub peer_blocklist: String,
    /// Keep a finished torrent shared with other peers (gaps row 147). Off unless
    /// turned on: it sends from the person's connection.
    #[serde(default)]
    pub torrent_share: bool,
    /// Shared until it has sent this many times its size.
    #[serde(default = "default_share_ratio")]
    pub torrent_share_ratio: f64,
    /// Shared for at most this many minutes.
    #[serde(default = "default_share_minutes")]
    pub torrent_share_minutes: u64,
    /// The most a shared torrent sends, in KB a second; 0 for no limit of its own.
    #[serde(default)]
    pub torrent_upload_limit: u64,
    /// When EDM last looked for a newer version, in seconds since 1970.
    pub update_checked: u64,
    /// Which edition of the pop-up defaults the four switches below were
    /// last brought up to. A file from an older edition is brought up to
    /// this one's defaults once (`clamped`); from then on a person's choice
    /// stands.
    #[serde(default)]
    pub popups_version: u32,
    /// How large the GPUI window draws everything, as a multiple of its
    /// design (Options › General › Appearance's slider, View › Text size):
    /// one of `TEXT_SIZE.stops`, 0.6 to 1.2, 1.0 the default; Ctrl (Cmd)
    /// with + and − steps a stop. Kept here so it holds across launches and
    /// either window can read it.
    #[serde(default = "default_text_size", deserialize_with = "lenient::text_size")]
    pub text_size: f32,
    /// The hint about the Scheduler, said once when a very big download
    /// starts, has been said.
    pub hinted_big: bool,
    /// The hint that a row's Details button opens a download's progress,
    /// said once when a download starts with the progress window off, has
    /// been said.
    pub hinted_details: bool,
    /// The toolbar styles: "both" (icons and text), "icons" (small, icons
    /// only) or "text" (text only). Anything else is "both".
    pub toolbar_style: String,
    /// Toolbar buttons in the order a person put them, by command id; any
    /// not named follow in the default order.
    pub toolbar_order: Vec<String>,
    /// The download list's columns in the order chosen, by their keys; empty is the default
    /// order.
    pub column_order: Vec<String>,
    /// The columns turned off, by key: at first Description, Referrer and Folder.
    pub columns_hidden: Vec<String>,
    /// The widths a person gave columns by dragging a heading's edge, in points at 100%, by key;
    /// a column not named has its own (gaps row U4).
    pub column_widths: std::collections::BTreeMap<String, f32>,
    /// How wide the GPUI window's side panel is, in points at 100%: its edge dragged, kept
    /// here so it holds across launches; `SIDEBAR_WIDTH.default` for a file from before it.
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    /// The side panel's groups shut from their heading ("All downloads", "Queues", "Grabber
    /// projects"), by their English heading, kept for the next launch (gaps row U42).
    #[serde(default)]
    pub shut_groups: Vec<String>,
    /// The column the list is sorted by, by its key, and which way; empty
    /// for the order downloads were added in. Kept so the list is sorted
    /// the same way at the next launch.
    pub sort_column: String,
    /// Whether `sort_column` runs from the smallest up.
    #[serde(default = "yes")]
    pub sort_ascending: bool,
    /// View › Categories: the side panel shown, kept for the next launch (gaps row U29).
    #[serde(default = "yes")]
    pub show_categories: bool,
    /// View › Toolbar: the toolbar shown, kept for the next launch.
    #[serde(default = "yes")]
    pub show_toolbar: bool,
    /// View › Details: the chosen download's detail under the list (gaps row E8), off at first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_details: bool,
    /// Toolbar buttons taken off, by command id: at first Delete completed and the Grabber,
    /// which Downloads and Tasks hold, so the toolbar keeps the everyday ones.
    pub toolbar_hidden: Vec<String>,
    /// Sign-ins: a user and password used for downloads from a
    /// host, so a protected file need not be typed for each time.
    #[serde(default, deserialize_with = "lenient::site_logins")]
    pub site_logins: Vec<SiteLogin>,
    /// A second network (an interface's name, such as `en5` for an iPhone
    /// over USB) that every other connection of a download goes out on;
    /// empty for none. Not used while a proxy is on: the proxy is the path.
    #[serde(default)]
    pub second_path: String,
    /// A third network beside the second, by name; empty for none (gaps row K3). Its
    /// connections are a share of every download's as the second's are.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub third_path: String,
    /// The second network's own speed limit in KB/s, shared by its connections; 0 for none
    /// (gaps row K5): a phone's metered data held to a rate whatever the line beside it does.
    #[serde(default, skip_serializing_if = "none_set")]
    pub second_limit_kb: u64,
    /// The third network's own speed limit in KB/s, as `second_limit_kb` is the second's; 0
    /// for none (gaps row K5b).
    #[serde(default, skip_serializing_if = "none_set")]
    pub third_limit_kb: u64,
    /// A name a person gave a network, by its interface (gaps row K4): said where EDM names it,
    /// in place of the system's ("Phone" for "iPhone USB").
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub network_names: std::collections::BTreeMap<String, String>,
    /// The second network's own allowance: at most this many MB every `second_limit_hours`
    /// hours over it; 0 for none (gaps row K5d).
    #[serde(default, skip_serializing_if = "none_set")]
    pub second_limit_mb: u64,
    /// The length of the second network's allowance period, in hours; 0 is one.
    #[serde(default, skip_serializing_if = "none_set")]
    pub second_limit_hours: u64,
    /// The third network's own allowance, as the second's.
    #[serde(default, skip_serializing_if = "none_set")]
    pub third_limit_mb: u64,
    /// The length of the third network's allowance period, in hours; 0 is one.
    #[serde(default, skip_serializing_if = "none_set")]
    pub third_limit_hours: u64,
    /// A sound file of the person's own for an event (`complete`, `failed`, `queue_started`,
    /// `queue_done`); an event with no entry, or an empty one, keeps the system's sound. Only `.wav`
    /// is promised. The switches above still decide whether the finish and failure sounds play.
    #[serde(default)]
    pub sounds: std::collections::BTreeMap<String, String>,
    /// "Site files" searches saved to run again, by hand or on a timer.
    #[serde(default)]
    pub site_searches: Vec<crate::lists::Search>,
    /// Playlists, channels' uploads and feeds followed: read again every so
    /// many hours, their new items added to a queue (gaps row W10).
    #[serde(default)]
    pub follows: Vec<crate::app::follow::Follow>,
    /// The floating drop target: a small window over every other, which
    /// takes a link dragged onto it. Off unless chosen (View › Drop target).
    #[serde(default)]
    pub drop_target: bool,
}

/// A named queue that starts on its own each day at a time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueStart {
    /// The queue's name.
    pub queue: String,
    /// `HH:MM`, local time.
    pub at: String,
}

/// Fewer files at once for one named queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueLimit {
    /// The queue's name.
    pub queue: String,
    /// How many of its downloads run at once.
    pub files: usize,
}

/// How many addresses Add URL remembers.
pub const RECENT_URLS: usize = 20;

/// Ten: the last five half a minute apart, so a download rides out a
/// couple of minutes without a network (`retry::backoff`).
fn default_text_size() -> f32 {
    TEXT_SIZE.default
}

fn default_sidebar_width() -> f32 {
    SIDEBAR_WIDTH.default
}

fn default_share_ratio() -> f64 {
    1.0
}

fn default_share_minutes() -> u64 {
    120
}

fn yes() -> bool {
    true
}

/// The edition of the pop-up defaults now. 1: the progress and finished
/// windows off. 2: the progress and finished windows on, and Download
/// details before a caught or a panel download, as reference A does them
/// (`docs/decisions.md`, "EDM works exactly like reference A").
pub const POPUPS_VERSION: u32 = 2;

/// The settings file's format number (`edm_core::saved`), written beside its fields. 0, before
/// the numbers: the same fields with none. A file of a higher number reads as far as this EDM
/// knows its fields; the rest are passed over, as any field it does not know.
pub const SETTINGS_FORMAT: u32 = 1;

impl Settings {
    /// The settings as a file to carry to another computer: every choice,
    /// but no password — Sign-ins keep their sites and names, the proxy its
    /// address and user — and none of this computer's own history.
    pub fn exported(&self) -> String {
        let mut out = self.clone();
        for login in &mut out.site_logins {
            login.password.clear();
        }
        out.advanced.proxy.password.clear();
        out.recent_urls.clear();
        out.last_folders.clear();
        serde_json::to_string_pretty(&edm_core::saved::Stamped {
            format: SETTINGS_FORMAT,
            body: &out,
        })
        .unwrap_or_default()
    }

    /// Settings read from such a file, in place of these: the passwords
    /// this computer already has for the same site and name, and for the
    /// same proxy (host, port and user), are kept, as are its history and the command it
    /// runs after a download, which a file never sets.
    pub fn imported(&self, text: &str) -> Result<Settings, String> {
        let mut read: Settings = serde_json::from_str(text)
            .map_err(|e| format!("that is not an EDM settings file ({e})"))?;
        for login in &mut read.site_logins {
            if let Some(known) = self
                .site_logins
                .iter()
                .find(|k| k.host == login.host && k.username == login.username)
            {
                login.password = known.password.clone();
            }
        }
        // For the same proxy: a user name alone is guessable, and a file naming
        // another proxy must not be handed this computer's password for it.
        if read.advanced.proxy.username == self.advanced.proxy.username
            && read.advanced.proxy.host == self.advanced.proxy.host
            && read.advanced.proxy.port == self.advanced.proxy.port
        {
            read.advanced
                .proxy
                .password
                .clone_from(&self.advanced.proxy.password);
        }
        read.recent_urls.clone_from(&self.recent_urls);
        read.last_folders = self.last_folders.clone();
        // Never a program to run from someone else's file, so a settings file passed around
        // cannot make EDM start something.
        read.keep_programs_of(self);
        Ok(read.clamped())
    }

    /// Puts back `mine`'s value of every setting that makes EDM run something: the command
    /// after each download, the program at the end of the shared schedule's hours and of each
    /// queue's own (a queue `mine` does not have runs none), and the proxy script, which is a
    /// program EDM runs (in a sandbox) and decides where every request goes. One list, for a
    /// settings file imported and for a script driving the window (security reviews). Says
    /// which of them were changed back.
    pub fn keep_programs_of(&mut self, mine: &Settings) -> KeptPrograms {
        let mut kept = KeptPrograms::default();
        if self.downloads.on_complete != mine.downloads.on_complete {
            self.downloads
                .on_complete
                .clone_from(&mine.downloads.on_complete);
            kept.command = true;
        }
        if self.advanced.proxy.script != mine.advanced.proxy.script {
            self.advanced
                .proxy
                .script
                .clone_from(&mine.advanced.proxy.script);
            kept.proxy_script = true;
        }
        if self.schedule.run_when_done != mine.schedule.run_when_done {
            self.schedule
                .run_when_done
                .clone_from(&mine.schedule.run_when_done);
            kept.schedule = true;
        }
        for (name, schedule) in &mut self.queue_schedules {
            let theirs = mine
                .queue_schedules
                .get(name)
                .map(|s| s.run_when_done.as_str())
                .unwrap_or_default();
            if schedule.run_when_done != theirs {
                schedule.run_when_done = theirs.to_string();
                kept.schedule = true;
            }
        }
        kept
    }

    /// Puts back `mine`'s value of every setting that says where a download is written: the
    /// download folder, the parts folder, sorting by kind and each kind's folder, and each
    /// site's own folder. For a script driving the window: an assistant that set the download
    /// folder to `~/Library/LaunchAgents` or a Startup folder in Options, then added a download,
    /// would be let through by the control port's rule (a download "into the download folder or
    /// below it") against the new folder (security audit). Not for a settings file imported: a
    /// person moving EDM to another computer brings their folders. Says whether any was put back.
    pub fn keep_places_of(&mut self, mine: &Settings) -> bool {
        let folders = |s: &Settings| -> Vec<(String, String)> {
            s.advanced
                .site_rules
                .iter()
                .map(|r| (r.host.clone(), r.folder.clone()))
                .collect()
        };
        let changed = self.folders.download_dir != mine.folders.download_dir
            || self.folders.parts_dir != mine.folders.parts_dir
            || self.folders.categorize != mine.folders.categorize
            || self.folders.category_folders != mine.folders.category_folders
            || folders(self) != folders(mine);
        if changed {
            self.folders
                .download_dir
                .clone_from(&mine.folders.download_dir);
            self.folders.parts_dir.clone_from(&mine.folders.parts_dir);
            self.folders.categorize = mine.folders.categorize;
            self.folders
                .category_folders
                .clone_from(&mine.folders.category_folders);
            // A site's speed may be the script's; its folder is put back, and a site rule the
            // person did not have writes nowhere of its own.
            for rule in &mut self.advanced.site_rules {
                rule.folder = mine
                    .advanced
                    .site_rules
                    .iter()
                    .find(|r| r.host == rule.host)
                    .map(|r| r.folder.clone())
                    .unwrap_or_default();
            }
        }
        changed
    }
}

/// Which settings that run something [`Settings::keep_programs_of`] changed back.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeptPrograms {
    /// The command run after each download.
    pub command: bool,
    /// A program run at the end of the scheduled hours, shared or a queue's own.
    pub schedule: bool,
    /// The proxy script.
    pub proxy_script: bool,
    /// Where downloads go: the download folder, the parts folder, the kinds' folders, sorting
    /// into them, a site's own folder (a script's change alone, `keep_places_of`).
    pub folders: bool,
}

/// A lower speed limit for set hours of every day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlowHours {
    /// Whether the slow hours are used.
    pub enabled: bool,
    /// `HH:MM`, local time: when they start.
    pub from: String,
    /// `HH:MM`, local time: when they end; before `from`, past midnight.
    pub to: String,
    /// The limit in those hours, in KB/s.
    pub limit: u64,
}

impl Default for SlowHours {
    fn default() -> Self {
        SlowHours {
            enabled: false,
            from: "08:00".into(),
            to: "18:00".into(),
            limit: 256,
        }
    }
}

impl SlowHours {
    /// Whether the slow hours are in force on a weekday (0 = Sunday) at a
    /// minute past midnight: every day, as the Scheduler's window reads.
    pub fn open_at(&self, weekday: u8, minute: u32) -> bool {
        self.enabled
            && self.limit > 0
            && edm_core::is_time(&self.from)
            && edm_core::is_time(&self.to)
            && Schedule {
                enabled: true,
                from: self.from.clone(),
                to: self.to.clone(),
                days: Vec::new(),
                ..Schedule::default()
            }
            .open_at(weekday, minute)
    }
}

/// The toolbar's buttons a new EDM leaves off (`toolbar_hidden`): Delete completed and the
/// Grabber, which Downloads and Tasks hold; and on macOS Options, which is the app menu's
/// Settings… there (⌘,), as a Mac program keeps its settings, not a toolbar button (the
/// maintainer, 2026-11-27). View › Customize toolbar brings any back.
pub const TOOLBAR_HIDDEN_AT_FIRST: &[&str] = if cfg!(target_os = "macos") {
    &[
        "downloads.clear_finished",
        "program.site_files",
        "downloads.options",
    ]
} else {
    &["downloads.clear_finished", "program.site_files"]
};

impl Default for Settings {
    fn default() -> Self {
        Settings {
            general: GeneralSettings::default(),
            browser: BrowserSettings::default(),
            folders: FolderSettings::default(),
            downloads: DownloadSettings::default(),
            advanced: AdvancedSettings::default(),
            second_path: String::new(),
            third_path: String::new(),
            second_limit_kb: 0,
            third_limit_kb: 0,
            network_names: Default::default(),
            second_limit_mb: 0,
            second_limit_hours: 0,
            third_limit_mb: 0,
            third_limit_hours: 0,
            sounds: Default::default(),
            site_searches: Vec::new(),
            follows: Vec::new(),
            drop_target: false,
            last_speed_limit: 0,
            slow_hours: SlowHours::default(),
            identify_as_dm: false,
            duplicate_choice: String::new(),
            same_file_choice: String::new(),
            recent_urls: Vec::new(),
            last_folders: Default::default(),
            limit_mb: 0,
            limit_hours: 1,
            custom_categories: Vec::new(),
            schedule: Schedule::default(),
            queue_schedules: std::collections::BTreeMap::new(),
            keep_stored_passwords: false,
            start_copied: false,
            start_while_asking: false,
            peer_blocklist: String::new(),
            torrent_share: false,
            torrent_share_ratio: default_share_ratio(),
            torrent_share_minutes: default_share_minutes(),
            torrent_upload_limit: 0,
            update_checked: 0,
            popups_version: POPUPS_VERSION,
            text_size: TEXT_SIZE.default,
            hinted_big: false,
            hinted_details: false,
            toolbar_style: "both".into(),
            toolbar_order: Vec::new(),
            column_order: Vec::new(),
            column_widths: std::collections::BTreeMap::new(),
            // Description, Referrer and Folder in the columns menu, not shown at first: shown,
            // they lay past a new window's right edge, unseen, and made the list scroll
            // sideways (the maintainer, 2026-11-26).
            // Queue too (the maintainer, 2026-11-27, a more minimal window): empty for most
            // downloads, the queues in the side panel; View › Columns shows it again.
            columns_hidden: ["queue", "description", "referrer", "folder"]
                .map(String::from)
                .to_vec(),
            sidebar_width: SIDEBAR_WIDTH.default,
            shut_groups: Vec::new(),
            sort_column: String::new(),
            sort_ascending: true,
            show_categories: true,
            show_toolbar: true,
            show_details: false,
            // A lighter toolbar at first (the maintainer, 2026-11-26): View › Customize
            // toolbar brings either back.
            toolbar_hidden: TOOLBAR_HIDDEN_AT_FIRST
                .iter()
                .map(|id| (*id).to_string())
                .collect(),
            site_logins: Vec::new(),
        }
    }
}

mod engine;
mod file;
mod lenient;
mod limits;
mod pages;
mod paths;
mod scale;
mod secrets;
mod sites;
// A backup's settings read in place of these (gaps row W16).
mod restore;

#[cfg(test)]
mod tests;

/// Whether a limit is none (0), to leave it out of the file (gaps row K5).
fn none_set(limit: &u64) -> bool {
    *limit == 0
}
