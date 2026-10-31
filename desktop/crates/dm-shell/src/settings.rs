//! What the person using dm has chosen, and where it is kept.
//!
//! Written on every change rather than on exit. A preferences file that only
//! survives a clean shutdown is one that loses your settings the first time
//! something crashes, which is the moment you were most likely changing them.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::path::PathBuf;

use dm_core::stream::Quality;
use dm_core::{AddOptions, Categories, Category, Limiter, Options, Proxy, Schedule};
use serde::{Deserialize, Serialize};

/// Bounds the engine will honour, applied here so a value typed into the
/// window and a value read from a hand-edited file are held to the same rule.
const CONNECTIONS: (usize, usize) = (1, 32);
const CONCURRENCY: (usize, usize) = (1, 10);
/// A gigabyte a second is past any link this would be throttling.
const SPEED_LIMIT_KB: (u64, u64) = (0, 1024 * 1024);
/// A petabyte, and a month: past any allowance a link has.
const LIMIT_MB: u64 = 1024 * 1024 * 1024;
const LIMIT_HOURS: u64 = 24 * 31;

/// Everything a person sets in dm, as `settings.json` keeps it: the window's
/// Options, the list's columns and sort, the side panel, and what the
/// browser's extension is told. A field missing from the file takes its
/// default, so an older file still reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Connections per file.
    pub connections: usize,
    /// Files transferring at once.
    pub concurrency: usize,
    /// Where finished files land.
    pub download_dir: PathBuf,

    /// Kilobytes a second across everything. Zero is no limit.
    ///
    /// Kilobytes rather than bytes because that is the unit people think in
    /// for a link.
    pub speed_limit: u64,
    /// The limit the speed limiter last had on, in KB/s, so turning it back
    /// on — in this run or the next — restores it. Zero for never set.
    #[serde(default)]
    pub last_speed_limit: u64,

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

    /// Sort finished files into folders by kind. On for a new install.
    pub categorize: bool,
    /// What each of those folders is called, in `Category::ALL` order. An
    /// empty name leaves that kind unsorted.
    pub category_folders: Vec<String>,
    /// Categories a person made: a name that is also a folder, file types,
    /// and optionally the only sites they come from. Looked at before the
    /// built-in kinds.
    #[serde(deserialize_with = "lenient::custom_categories")]
    pub custom_categories: Vec<dm_core::Custom>,

    /// Which rendition to take from an adaptive stream: `best`, `audio`, or
    /// a height.
    pub stream_quality: String,

    /// When the queue may run.
    #[serde(deserialize_with = "lenient::schedule")]
    pub schedule: Schedule,

    /// Where downloads connect through.
    #[serde(deserialize_with = "lenient::proxy")]
    pub proxy: Proxy,

    /// Watch the clipboard for links and offer to download them. Off by
    /// default: people copy URLs constantly, so this asks rather than grabs.
    #[serde(default)]
    pub watch_clipboard: bool,

    /// Start with the computer, so a browser hand-off has an app to land in
    /// without anyone opening it first. On for a new install, in the tray
    /// with no window; Options › General turns it off.
    #[serde(default)]
    pub start_on_login: bool,
    /// Open dm's control port (loopback only, a token only this account can
    /// read) at every start, so `dm-desktop list`, `pause`, `resume` and
    /// `cancel` find the running dm. Off unless turned on: otherwise dm
    /// listens on nothing, and the port exists only under `--control`.
    #[serde(default)]
    pub allow_command_line: bool,
    /// Let an AI assistant use dm over MCP (`dm-desktop mcp`): its requests
    /// are answered only while this is on, and it opens the control port at
    /// start as `allow_command_line` does. On unless turned off, also for a
    /// settings file written before it existed.
    #[serde(default = "default_true")]
    pub allow_mcp: bool,
    /// A folder dm reads every few seconds for links saved there: a `.url`,
    /// a `.webloc`, a text file of links or a `.torrent` is added, then
    /// moved into a `done` folder inside it. Empty is off.
    #[serde(default)]
    pub watch_folder: String,
    /// Look for a newer dm about once a week. Off unless turned on: it asks
    /// GitHub, and dm otherwise contacts nothing but what is downloaded.
    pub check_updates: bool,
    /// The window's language, by code ("es", "pt-BR"); empty follows the
    /// system's where dm has it.
    pub language: String,
    /// When dm last looked for a newer version, in seconds since 1970.
    pub update_checked: u64,

    /// Which edition of the pop-up defaults the two switches below were
    /// last brought up to. Zero is a file from before they were off by
    /// default, whose `true` was the default, not a choice: it is read as
    /// off once (`clamped`), and from then on a person's choice stands.
    #[serde(default)]
    pub popups_version: u32,
    /// Open a progress window for a download as it starts. Off unless turned
    /// on: the list already shows progress, and a window opening over the
    /// work on every download was in the way.
    #[serde(default)]
    pub show_progress: bool,
    /// Say so in a window when a download finishes, with Open and Open
    /// folder. Off unless turned on: a download just starts and finishes
    /// in the list.
    #[serde(default)]
    pub show_complete: bool,
    /// Play the system's short sound when a download finishes.
    pub sound_on_complete: bool,
    /// A video from the browser's panel: "Download details" opens for it,
    /// to name it and choose where it goes, before it starts.
    pub ask_panel_downloads: bool,
    /// A download caught from the browser: "Download details" opens for it
    /// too, rather than it starting at once. Off by default, since 2026-10-26.
    #[serde(default)]
    pub ask_caught_downloads: bool,
    /// The window's look: "system" (the modern look, light or dark as the
    /// computer is), "light", "dark", or "classic" (a classic Windows look).
    pub theme: String,
    /// What a double-click on a row does: "properties", "open" (a finished
    /// file; Properties for one not finished) or "folder" (its folder).
    pub double_click: String,
    /// What the window's close button does: "ask" (the first time, and the
    /// answer is kept here), "tray" (dm goes on running in the tray) or
    /// "exit".
    pub on_close: String,
    /// The hint about the Scheduler, said once when a very big download
    /// starts, has been said.
    pub hinted_big: bool,
    /// The hint that a row's Details button opens a download's progress,
    /// said once when a download starts with the progress window off, has
    /// been said.
    pub hinted_details: bool,
    /// A finished file takes the date its server says it last changed,
    /// rather than when it was fetched. Off by default.
    pub server_dates: bool,
    /// Seconds to wait between attempts after a failure; 0 lets dm choose
    /// (quick at first, then half a minute apart).
    pub retry_wait: u64,
    /// Minutes a live stream is recorded for before dm ends it and keeps
    /// what it has; 0 records until the stream ends or Stop.
    #[serde(default)]
    pub record_minutes: u64,
    /// The toolbar styles: "both" (icons and text), "icons" (small, icons
    /// only) or "text" (text only). Anything else is "both".
    pub toolbar_style: String,
    /// Toolbar buttons in the order a person put them, by command id; any
    /// not named follow in the default order.
    pub toolbar_order: Vec<String>,
    /// The download list's columns in the order chosen, and those turned
    /// off, by their keys; empty is the default order, all shown.
    pub column_order: Vec<String>,
    /// The columns turned off, by key.
    pub columns_hidden: Vec<String>,
    /// The column the list is sorted by, by its key, and which way; empty
    /// for the order downloads were added in. Kept so the list is sorted
    /// the same way at the next launch.
    pub sort_column: String,
    /// Whether `sort_column` runs from the smallest up.
    #[serde(default = "yes")]
    pub sort_ascending: bool,
    /// Toolbar buttons taken off, by command id.
    pub toolbar_hidden: Vec<String>,
    /// Sign-ins: a user and password used for downloads from a
    /// host, so a protected file need not be typed for each time.
    #[serde(default, deserialize_with = "lenient::site_logins")]
    pub site_logins: Vec<SiteLogin>,

    /// Which downloads the browser extension takes from the browser —
    /// Options › Browser. Kept here, in the app, and sent to the extension
    /// when it asks (`settings.get`), so there is one list and it is the
    /// app's. Spelled as the protocol spells it: `minSizeKb`, `skipHosts`.
    #[serde(default)]
    pub catch: dm_protocol::CatchRules,

    /// A `cookies.txt` (Netscape format) whose cookies dm sends with its own
    /// requests and hands to yt-dlp, for downloads that need a signed-in
    /// session. Empty for none.
    #[serde(default)]
    pub cookies_file: String,

    /// A command run on each finished file, its path added as the last
    /// argument: a virus checker, or a script of the person's own. Empty
    /// for none.
    #[serde(default)]
    pub on_complete: String,

    /// How many times one connection tries before the download fails, for
    /// a failure waiting can fix. Zero keeps trying until it gets through
    /// or is paused.
    #[serde(default = "default_attempts")]
    pub attempts: u32,
    /// Seconds a connection may send nothing before dm gives up on it and
    /// tries again.
    #[serde(default = "default_read_timeout")]
    pub read_timeout: u64,
    /// A second network (an interface's name, such as `en5` for an iPhone
    /// over USB) that every other connection of a download goes out on;
    /// empty for none. Not used while a proxy is on: the proxy is the path.
    #[serde(default)]
    pub second_path: String,
    /// Sites that take fewer connections at once than `connections`.
    #[serde(default, deserialize_with = "lenient::host_limits")]
    pub host_limits: Vec<dm_core::HostLimit>,
    /// A folder and a speed limit for a site's downloads (Options › Network).
    #[serde(default, deserialize_with = "lenient::site_rules")]
    pub site_rules: Vec<SiteRule>,
    /// Hours between the recheck queue asking its servers again on its
    /// own. Zero asks only when "Start queue" is clicked on it.
    #[serde(default)]
    pub recheck_hours: u64,
    /// Play the system's short failure sound when a download fails.
    #[serde(default)]
    pub sound_on_failure: bool,
    /// Start the Main queue as dm opens, so what was added for later goes
    /// at the next launch without a click.
    #[serde(default)]
    pub start_queue_on_launch: bool,
    /// Have yt-dlp fetch a video's subtitles, carried inside the video.
    #[serde(default)]
    pub subtitles: bool,
    /// "Site files" searches saved to run again, by hand or on a timer.
    #[serde(default)]
    pub site_searches: Vec<crate::lists::Search>,
    /// Queues that run fewer files at once than `concurrency`.
    #[serde(default, deserialize_with = "lenient::queue_limits")]
    pub queue_limits: Vec<QueueLimit>,
    /// The floating drop target: a small window over every other, which
    /// takes a link dragged onto it. Off unless chosen (View › Drop target).
    #[serde(default)]
    pub drop_target: bool,
    /// Windows 11's Mica behind the window's frame, from the next start.
    /// Off unless chosen: it needs a see-through window, which an old GPU
    /// driver can draw black.
    #[serde(default)]
    pub mica: bool,
    /// Fetch yt-dlp and QuickJS from their GitHub releases the first time a
    /// video page needs them (and keep the fetched yt-dlp current). Off,
    /// only a yt-dlp the person installed is used.
    #[serde(default = "yes")]
    pub fetch_tools: bool,
    /// Queues that start on their own each day at a time of their own.
    #[serde(default)]
    pub queue_starts: Vec<QueueStart>,
    /// Put a `.ts` stream into an MP4 (dm's own remux).
    #[serde(default = "yes")]
    pub remux_ts: bool,
    /// Open "Update download link" by itself when a download's signed link
    /// has run out part-way. Off, the row says it failed and its menu has
    /// Update download link.
    #[serde(default = "yes")]
    pub offer_new_link: bool,
    /// Where unfinished downloads are staged; empty for dm's own folder.
    /// A download already begun keeps the folder it began in.
    #[serde(default)]
    pub parts_dir: String,
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

/// What a site's downloads get when they are added: a folder of their own
/// and a speed limit of their own. The host covers its subdomains, as a
/// site's connection limit does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteRule {
    /// The site, which covers its subdomains.
    pub host: String,
    /// Where its downloads are saved, instead of the folder their kind has;
    /// empty keeps the kind's.
    pub folder: String,
    /// KB/s for each of its downloads; zero for none.
    pub speed: u64,
}

/// How many addresses Add URL remembers.
pub const RECENT_URLS: usize = 20;

/// Ten: the last five half a minute apart, so a download rides out a
/// couple of minutes without a network (`retry::backoff`).
fn default_true() -> bool {
    true
}

fn default_attempts() -> u32 {
    15
}

fn default_read_timeout() -> u64 {
    30
}

/// Bounds for the two above: a hundred tries is already an hour of asking,
/// and a connection silent for ten minutes is not slow.
const ATTEMPTS: (u32, u32) = (0, 100);
const READ_TIMEOUT_SECS: (u64, u64) = (5, 600);

fn yes() -> bool {
    true
}

/// The edition of the pop-up defaults now (1: progress and finished windows
/// off until turned on).
pub const POPUPS_VERSION: u32 = 1;

impl Settings {
    /// The settings as a file to carry to another computer: every choice,
    /// but no password — Sign-ins keep their sites and names, the proxy its
    /// address and user — and none of this computer's own history.
    pub fn exported(&self) -> String {
        let mut out = self.clone();
        for login in &mut out.site_logins {
            login.password.clear();
        }
        out.proxy.password.clear();
        out.recent_urls.clear();
        out.last_folders.clear();
        serde_json::to_string_pretty(&out).unwrap_or_default()
    }

    /// Settings read from such a file, in place of these: the passwords
    /// this computer already has for the same site and name, and for the
    /// same proxy user, are kept, as are its history and the command it
    /// runs after a download, which a file never sets.
    pub fn imported(&self, text: &str) -> Result<Settings, String> {
        let mut read: Settings = serde_json::from_str(text)
            .map_err(|e| format!("that is not a dm settings file ({e})"))?;
        for login in &mut read.site_logins {
            if let Some(known) = self
                .site_logins
                .iter()
                .find(|k| k.host == login.host && k.username == login.username)
            {
                login.password = known.password.clone();
            }
        }
        if read.proxy.username == self.proxy.username {
            read.proxy.password = self.proxy.password.clone();
        }
        read.recent_urls = self.recent_urls.clone();
        read.last_folders = self.last_folders.clone();
        // Never a program to run from someone else's file: the command run
        // after every download stays this computer's own, so a settings file
        // passed around cannot make dm start something.
        read.on_complete = self.on_complete.clone();
        Ok(read.clamped())
    }
}

/// One entry in Options › Sign-ins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteLogin {
    /// The host, exactly: `example.com` does not cover `dl.example.com`.
    /// A leading dot (`.example.com`) also covers its subdomains.
    pub host: String,
    /// The user name sent to that host.
    pub username: String,
    /// The password sent with it, kept in this account's own file (0600).
    pub password: String,
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

impl Default for Settings {
    fn default() -> Self {
        Settings {
            attempts: default_attempts(),
            read_timeout: default_read_timeout(),
            second_path: String::new(),
            host_limits: Vec::new(),
            site_rules: Vec::new(),
            recheck_hours: 0,
            sound_on_failure: false,
            start_queue_on_launch: false,
            subtitles: false,
            site_searches: Vec::new(),
            queue_limits: Vec::new(),
            queue_starts: Vec::new(),
            drop_target: false,
            mica: false,
            offer_new_link: true,
            fetch_tools: true,
            remux_ts: true,
            parts_dir: String::new(),
            on_complete: String::new(),
            cookies_file: String::new(),
            connections: 8,
            concurrency: 3,
            // A `dm` folder inside the one the operating system already
            // nominates. A download manager that puts files anywhere else is
            // one people have to go looking in.
            download_dir: dirs::download_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
                .join("dm"),
            speed_limit: 0,
            last_speed_limit: 0,
            recent_urls: Vec::new(),
            last_folders: Default::default(),
            limit_mb: 0,
            limit_hours: 1,
            categorize: true,
            category_folders: default_folders(),
            custom_categories: Vec::new(),
            stream_quality: default_quality(),
            schedule: Schedule::default(),
            proxy: Proxy::default(),
            watch_clipboard: false,
            start_on_login: true,
            allow_command_line: false,
            allow_mcp: true,
            watch_folder: String::new(),
            check_updates: false,
            language: String::new(),
            update_checked: 0,
            popups_version: POPUPS_VERSION,
            show_progress: false,
            show_complete: false,
            sound_on_complete: false,
            ask_panel_downloads: false,
            ask_caught_downloads: false,
            theme: "system".into(),
            double_click: "open".into(),
            on_close: "ask".into(),
            hinted_big: false,
            hinted_details: false,
            server_dates: false,
            retry_wait: 0,
            record_minutes: 0,
            toolbar_style: "both".into(),
            toolbar_order: Vec::new(),
            column_order: Vec::new(),
            columns_hidden: Vec::new(),
            sort_column: String::new(),
            sort_ascending: true,
            toolbar_hidden: Vec::new(),
            site_logins: Vec::new(),
            catch: dm_protocol::CatchRules::default(),
        }
    }
}

impl Settings {
    /// Forces every value into a range the engine will accept.
    ///
    /// Applied on read as well as on write: the file is plain JSON on a disk
    /// anyone can edit, and a hand-typed 500 connections should be clamped
    /// rather than handed to the engine or rejected with an error nobody sees.
    pub fn clamped(mut self) -> Self {
        self.connections = self.connections.clamp(CONNECTIONS.0, CONNECTIONS.1);
        self.concurrency = self.concurrency.clamp(CONCURRENCY.0, CONCURRENCY.1);
        self.speed_limit = self.speed_limit.clamp(SPEED_LIMIT_KB.0, SPEED_LIMIT_KB.1);
        self.recent_urls.truncate(RECENT_URLS);
        self.retry_wait = self.retry_wait.min(3600);
        // A day at most: a stream meant to run longer is stopped by hand.
        self.record_minutes = self.record_minutes.min(24 * 60);
        if !["ask", "tray", "exit"].contains(&self.on_close.as_str()) {
            self.on_close = "ask".into();
        }
        if !["properties", "open", "folder"].contains(&self.double_click.as_str()) {
            self.double_click = "properties".into();
        }
        self.catch.bring_up_to_date();
        // Both pop-ups were on for everyone before they were off by default:
        // a saved `true` from then is the old default, so it is let go once.
        if self.popups_version < POPUPS_VERSION {
            self.show_progress = false;
            self.show_complete = false;
            self.popups_version = POPUPS_VERSION;
        }
        // The click keys are one of those the extension knows, or (to take)
        // none; anything else is what they ship as.
        if !dm_protocol::CLICK_KEYS.contains(&self.catch.keep_key.as_str()) {
            self.catch.keep_key = "alt".into();
        }
        if !self.catch.take_key.is_empty()
            && !dm_protocol::CLICK_KEYS.contains(&self.catch.take_key.as_str())
        {
            self.catch.take_key = "shift".into();
        }
        self.last_speed_limit = self
            .last_speed_limit
            .clamp(SPEED_LIMIT_KB.0, SPEED_LIMIT_KB.1);
        self.limit_mb = self.limit_mb.min(LIMIT_MB);
        self.limit_hours = self.limit_hours.clamp(1, LIMIT_HOURS);
        self.attempts = self.attempts.clamp(ATTEMPTS.0, ATTEMPTS.1);
        self.recheck_hours = self.recheck_hours.min(LIMIT_HOURS);
        self.read_timeout = self
            .read_timeout
            .clamp(READ_TIMEOUT_SECS.0, READ_TIMEOUT_SECS.1);
        // A folder for unfinished downloads is a full path: `~` is the home
        // folder, and a relative one is no folder — a Dock launch starts in
        // `/`, where nothing can be written, and every download would fail.
        self.parts_dir = full_path(&self.parts_dir)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        // A limit names a queue or a site, or it limits nothing; and it is a
        // number the engine will take.
        self.queue_limits
            .retain(|limit| !limit.queue.trim().is_empty());
        for limit in &mut self.queue_limits {
            limit.queue = limit.queue.trim().to_string();
            limit.files = limit.files.clamp(CONCURRENCY.0, CONCURRENCY.1);
        }
        self.host_limits
            .retain(|limit| !limit.host.trim().is_empty());
        for limit in &mut self.host_limits {
            limit.host = limit.host.trim().to_ascii_lowercase();
            limit.connections = limit.connections.clamp(CONNECTIONS.0, CONNECTIONS.1);
        }
        // A rule names a site, and its folder is a whole path or none.
        self.site_rules.retain(|rule| !rule.host.trim().is_empty());
        for rule in &mut self.site_rules {
            rule.host = rule
                .host
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            rule.folder = full_path(&rule.folder)
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            rule.speed = rule.speed.min(SPEED_LIMIT_KB.1);
        }
        // The download folder is held to the parts folder's rule. Options refuses a
        // relative one as it is typed (`refusal`); one in a file edited by
        // hand falls back to the default rather than to wherever dm was
        // started from.
        self.download_dir = full_path(&self.download_dir.display().to_string())
            .unwrap_or_else(|| Settings::default().download_dir);
        // One name per category, always: the engine indexes this list by
        // category, and a short one read from a hand-edited file would leave
        // the kinds past its end unsorted for no stated reason.
        self.category_folders
            .resize(Category::ALL.len(), String::new());
        self
    }

    /// Why a change from `previous` to this cannot be put into force, if
    /// it cannot: a time the schedule cannot read, a download folder that
    /// is not a full path, a category name that is not one folder, a
    /// cookies file that cannot be read. Judged on what changed only, so a
    /// value already saved, however odd, never blocks an unrelated change
    /// — the language, a saved search's last run.
    pub fn refusal(&self, previous: &Settings) -> Option<String> {
        if self.schedule != previous.schedule
            && self.schedule.enabled
            && !(dm_core::schedule::is_time(&self.schedule.from)
                && dm_core::schedule::is_time(&self.schedule.to))
        {
            return Some(format!(
                "the schedule's times must be HH:MM in 24 hours, not {:?} and {:?}",
                self.schedule.from, self.schedule.to
            ));
        }
        if self.queue_starts != previous.queue_starts {
            if let Some(bad) = self
                .queue_starts
                .iter()
                .find(|s| !dm_core::schedule::is_time(&s.at))
            {
                return Some(format!(
                    "a queue's start time must be HH:MM in 24 hours, not {:?}",
                    bad.at
                ));
            }
        }
        if self.download_dir != previous.download_dir {
            let typed = self.download_dir.display().to_string();
            if full_path(&typed).is_none() {
                return Some(format!(
                    "the download folder must be a full path, such as {}, not {typed:?}",
                    Settings::default().download_dir.display()
                ));
            }
        }
        if self.parts_dir.trim() != previous.parts_dir.trim()
            && !self.parts_dir.trim().is_empty()
            && full_path(&self.parts_dir).is_none()
        {
            return Some(format!(
                "the folder for unfinished downloads must be a full path, not {:?}",
                self.parts_dir
            ));
        }
        // A name made or renamed is held to every computer's rules, since
        // settings travel; one already saved is not.
        for c in &self.custom_categories {
            if previous.custom_categories.iter().any(|p| p.name == c.name) {
                continue;
            }
            if let Err(why) = dm_core::category::portable_folder(&c.name) {
                return Some(format!(
                    "a category's name is also its folder, so it cannot be {:?}: {why}",
                    c.name
                ));
            }
        }
        for (at, folder) in self.category_folders.iter().enumerate() {
            if folder.trim().is_empty() || previous.category_folders.get(at) == Some(folder) {
                continue;
            }
            if let Err(why) = dm_core::category::portable_folder(folder) {
                return Some(format!("a folder cannot be called {folder:?}: {why}"));
            }
        }
        let cookies = self.cookies_file.trim();
        if cookies != previous.cookies_file.trim() && !cookies.is_empty() {
            if let Err(e) = std::fs::metadata(cookies).and_then(|m| {
                if m.is_file() {
                    Ok(())
                } else {
                    Err(std::io::Error::other("it is not a file"))
                }
            }) {
                return Some(format!(
                    "the cookies file {cookies} could not be read: {}",
                    dm_core::file_error(&e)
                ));
            }
        }
        None
    }

    /// The engine's view of all of this.
    ///
    /// One translation, used to build the queue at startup and to update it
    /// on every change. Written out twice it had already drifted — only the
    /// startup copy knew about the parts directory, and a new setting had to
    /// be remembered in both.
    pub fn options(&self, parts_dir: PathBuf, limiter: Limiter) -> Options {
        Options {
            connections: self.connections,
            control: dm_core::Control::new(),
            dest_dir: self.download_dir.clone(),
            parts_dir,
            limiter,
            categories: self.categories(),
            // Per download, not a setting: the queue fills this in from the
            // row when it starts one.
            checksum: None,
            quality: self.quality(),
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            media: false,
            referrer: None,
            // For the tools dm runs; the client has its own copy.
            proxy: self.proxy.url_for_tools(),
            container: None,
            attempts: self.attempts,
            host_limits: self.host_limits.clone(),
            subtitles: self.subtitles,
            room: Default::default(),
            remux: self.remux_ts,
            server_dates: self.server_dates,
            retry_wait: self.retry_wait,
            record_limit: (self.record_minutes > 0)
                .then(|| std::time::Duration::from_secs(self.record_minutes * 60)),
            sign_in: None,
            save_name: None,
            system_proxy: self.proxy.enabled && self.proxy.kind == dm_core::proxy::Kind::System,
            proxy_bypass: if self.proxy.enabled {
                self.proxy.bypass.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// How often the recheck queue asks its servers again, if on its own.
    pub fn recheck_every(&self) -> Option<std::time::Duration> {
        (self.recheck_hours > 0).then(|| std::time::Duration::from_secs(self.recheck_hours * 3600))
    }

    /// Where unfinished downloads are staged: the folder chosen, or dm's own.
    pub fn parts_dir(&self) -> PathBuf {
        let chosen = self.parts_dir.trim();
        if chosen.is_empty() {
            home().join("parts")
        } else {
            PathBuf::from(chosen)
        }
    }

    /// The start times that are times, for the queue.
    pub fn queue_start_list(&self) -> Vec<(String, u32)> {
        self.queue_starts
            .iter()
            .filter_map(|s| Some((s.queue.clone(), dm_core::schedule::minutes(&s.at)?)))
            .collect()
    }

    /// The per-queue limits as the engine takes them.
    pub fn queue_limit_map(&self) -> std::collections::HashMap<String, usize> {
        self.queue_limits
            .iter()
            .map(|l| (l.queue.clone(), l.files))
            .collect()
    }

    /// How long a connection may send nothing, in the client's units.
    pub fn read_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.read_timeout.max(1))
    }

    /// The engine's view of the sorting settings.
    pub fn categories(&self) -> Categories {
        Categories {
            enabled: self.categorize,
            folders: self.category_folders.clone(),
            custom: self.custom_categories.clone(),
            ..Default::default()
        }
    }

    /// The download limit, in the engine's units.
    pub fn allowance(&self) -> dm_core::quota::Allowance {
        dm_core::quota::Allowance {
            bytes: self.limit_mb.saturating_mul(1024 * 1024),
            period: std::time::Duration::from_secs(self.limit_hours.max(1) * 3600),
        }
    }

    /// The speed ceiling in bytes a second, which is what the engine counts.
    pub fn speed_limit_bytes(&self) -> u64 {
        self.speed_limit.saturating_mul(1024)
    }

    /// The engine's view of the stream quality.
    ///
    /// Anything unrecognised means the best available — a typo in a settings
    /// file should not silently fetch the smallest rendition on offer.
    pub fn quality(&self) -> Quality {
        Quality::named(&self.stream_quality)
    }

    /// Where the settings are kept: `settings.json` in dm's home.
    pub fn path() -> PathBuf {
        home().join("settings.json")
    }

    /// Reads what is stored, falling back to the defaults, and says so when
    /// it had to.
    ///
    /// A settings file that cannot be read is not worth failing to start
    /// over: the defaults are a working configuration, and refusing to open
    /// leaves someone with no way to fix the file through the app. But it is
    /// not overwritten either: the next save used to replace a file with one
    /// stray comma — a folder, a proxy, a schedule — with the defaults, and
    /// nothing said why they had all gone. It is kept aside, as the queue
    /// file is, and the window is told.
    pub fn load() -> (Self, Option<String>) {
        Self::load_from(&Self::path())
    }

    fn load_from(path: &std::path::Path) -> (Self, Option<String>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return (Settings::default().clamped(), None);
        };
        lenient::take();
        let (read, mut dropped) = match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => (settings, Vec::new()),
            // One odd entry costs that entry: the rest is read, field by
            // field, and what was left out is named below.
            Err(e) => match lenient::settings(&text) {
                Some(read) => read,
                None => {
                    let aside = path.with_extension("json.unreadable");
                    let notice = match std::fs::rename(path, &aside) {
                        Ok(()) => format!(
                            "the saved settings could not be read ({e}); they were kept as {} \
                             and the defaults are in use",
                            aside.display()
                        ),
                        Err(io) => format!(
                            "the saved settings could not be read ({e}) or moved aside ({io}); \
                             the defaults are in use and the next change will replace them"
                        ),
                    };
                    return (Settings::default().clamped(), Some(notice));
                }
            },
        };
        // What the lenient readers inside left out: a proxy type, a field
        // of a schedule, an entry of a list or a field of one.
        dropped.extend(lenient::take());
        dropped.dedup();
        let (settings, mut notes) = read.readable();
        if !dropped.is_empty() {
            notes.insert(
                0,
                format!(
                    "some saved settings could not be read ({}), so their defaults are in use",
                    dropped.join(", ")
                ),
            );
        }
        if notes.is_empty() {
            return (settings, None);
        }
        // Whatever was left out or changed, the file as it was is kept
        // before the next save writes over it.
        let aside = path.with_extension("json.unreadable");
        notes.push(match std::fs::copy(path, &aside) {
            Ok(_) => format!("the file as it was is kept as {}", aside.display()),
            Err(io) => format!("the file as it was could not be kept aside ({io})"),
        });
        (settings, Some(notes.join("; ")))
    }

    /// What was read, held to what the engine takes, with a word about each
    /// thing that had to be changed to be used: a schedule whose times
    /// cannot be read (ignored anyway, so now off, and said to be), and a
    /// category whose name is not one folder (it sorted nothing, and would
    /// otherwise sit in Options failing every OK).
    fn readable(self) -> (Self, Vec<String>) {
        let mut settings = self.clamped();
        let mut notes = Vec::new();
        let schedule = &mut settings.schedule;
        if schedule.enabled
            && !(dm_core::schedule::is_time(&schedule.from)
                && dm_core::schedule::is_time(&schedule.to))
        {
            schedule.enabled = false;
            notes.push(format!(
                "the saved schedule's times ({:?} to {:?}) are not HH:MM, so the schedule is \
                 off; set it again in the Scheduler",
                schedule.from, schedule.to
            ));
        }
        let (kept, bad): (Vec<_>, Vec<_>) = std::mem::take(&mut settings.custom_categories)
            .into_iter()
            .partition(|c| dm_core::category::one_folder(&c.name));
        settings.custom_categories = kept;
        if !bad.is_empty() {
            let names: Vec<String> = bad.iter().map(|c| format!("{:?}", c.name)).collect();
            notes.push(format!(
                "the categories {} were left out: a category's name is also its folder, and \
                 these cannot be a folder on this computer",
                names.join(", ")
            ));
        }
        // A kind's folder that cannot be used here sorts into the download
        // folder itself; said, rather than done without a word.
        // Empty is "not nested", on purpose.
        let unusable: Vec<String> = settings
            .category_folders
            .iter()
            .filter(|f| !f.trim().is_empty() && !dm_core::category::one_folder(f))
            .map(|f| format!("{f:?}"))
            .collect();
        if !unusable.is_empty() {
            notes.push(format!(
                "the folders {} cannot be a folder on this computer, so those kinds of file \
                 land in the download folder itself; rename them in Options",
                unusable.join(", ")
            ));
        }
        (settings, notes)
    }

    /// Writes the settings to [`Settings::path`] at once, readable by this
    /// account alone; a stop mid-write leaves the file before it whole.
    pub fn save(&self) -> std::io::Result<()> {
        dm_core::write_atomic(
            &Self::path(),
            serde_json::to_string_pretty(self)?.as_bytes(),
        )
    }
}

impl Settings {
    /// `url` with a site login's user and password in it, where one applies
    /// and the address carries none of its own. HTTPS only (plain HTTP only
    /// to this computer), and only to the exact host unless the saved host
    /// begins with a dot.
    /// The rule of the site `url` is on, the most specific host that covers
    /// it.
    pub fn site_rule_for(&self, url: &str) -> Option<&SiteRule> {
        let host = dm_core::category::host_of(url);
        self.site_rules
            .iter()
            .filter(|rule| {
                let want = rule
                    .host
                    .trim()
                    .trim_start_matches('.')
                    .to_ascii_lowercase();
                !want.is_empty() && (host == want || host.ends_with(&format!(".{want}")))
            })
            .max_by_key(|rule| rule.host.trim().len())
    }

    /// `with`, given its site's folder and speed limit where it was not given
    /// its own: what a person chose for this one download stands.
    pub fn with_site_rule(&self, url: &str, mut with: AddOptions) -> AddOptions {
        let Some(rule) = self.site_rule_for(url) else {
            return with;
        };
        if with.save_dir.as_deref().is_none_or(|d| d.trim().is_empty()) && !rule.folder.is_empty() {
            with.save_dir = Some(rule.folder.clone());
        }
        if with.speed_limit == 0 {
            with.speed_limit = rule.speed;
        }
        with
    }

    /// `url` with the login Options › Sign-ins keeps for its host, where it
    /// has none of its own; else as it is.
    pub fn with_login(&self, url: &str) -> String {
        let Ok(mut parsed) = url::Url::parse(url) else {
            return url.to_string();
        };
        if !parsed.username().is_empty() {
            return url.to_string();
        }
        let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
        let on_this_computer = host == "localhost" || host == "127.0.0.1" || host == "[::1]";
        match parsed.scheme() {
            "https" => {}
            "http" if on_this_computer => {}
            _ => return url.to_string(),
        }
        let found = self.site_logins.iter().find(|login| {
            // A host saved as a whole address names its host (one pasted in matched nothing, and the download got 401).
            let given = login.host.trim();
            let given = if given.contains("://") {
                url::Url::parse(given)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
                    .unwrap_or_default()
            } else {
                given.split(['/', ':']).next().unwrap_or("").to_string()
            };
            let with_subdomains = given.starts_with('.');
            let want = given.trim_start_matches('.').to_ascii_lowercase();
            !want.is_empty()
                && (host == want || (with_subdomains && host.ends_with(&format!(".{want}"))))
        });
        match found {
            Some(login)
                if parsed.set_username(&login.username).is_ok()
                    && parsed.set_password(Some(&login.password)).is_ok() =>
            {
                parsed.to_string()
            }
            _ => url.to_string(),
        }
    }
}

/// A folder typed or read as a full path: `~` is the home folder, and a
/// relative one is no folder — a Dock launch starts in `/`, where nothing
/// can be written, and every download would fail. `None` for those and for
/// nothing at all.
pub fn full_path(typed: &str) -> Option<PathBuf> {
    let typed = typed.trim();
    match typed.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => {
            dirs::home_dir().map(|home| home.join(rest.trim_start_matches(['/', '\\'])))
        }
        _ if !typed.is_empty() && std::path::Path::new(typed).is_absolute() => {
            Some(PathBuf::from(typed))
        }
        _ => None,
    }
}

/// A folder for one download, as a full path: `~` is the home folder, and a
/// relative one is taken against `base` — the folder of the command that
/// asked, which a running dm handed the request does not share (a Dock
/// launch's is `/`). With no base, a relative folder is `None`: a row must
/// never hold one, since Open and Open folder read it later from wherever
/// dm happens to be.
pub fn full_folder(typed: &str, base: Option<&std::path::Path>) -> Option<PathBuf> {
    full_path(typed).or_else(|| {
        let typed = typed.trim();
        (!typed.is_empty() && !typed.starts_with('~'))
            .then_some(base?.join(typed))
            .filter(|p| p.is_absolute())
    })
}

/// Reading parts of the file one at a time, so one odd entry — a proxy type
/// a later dm wrote, a field left out by hand — costs that entry and not
/// every setting beside it.
mod lenient {
    use serde::de::{DeserializeOwned, Deserializer};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    thread_local! {
        /// What the readers below left out, by where it was in the file,
        /// for `load_from` to name. Serde gives a field's reader no way to
        /// say so, and reading happens on the caller's thread.
        static DROPPED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn note(what: String) {
        DROPPED.with(|d| {
            let mut d = d.borrow_mut();
            if !d.contains(&what) {
                d.push(what);
            }
        });
    }

    /// What was left out since the last call, which forgets it.
    pub(super) fn take() -> Vec<String> {
        DROPPED.with(|d| std::mem::take(&mut *d.borrow_mut()))
    }

    /// `merged`, saying under `at` what was left out.
    fn read_at<T: Serialize + DeserializeOwned>(base: &T, given: Value, at: &str) -> Option<T> {
        match merged(base, given) {
            Some((read, dropped)) => {
                for key in dropped {
                    note(format!("{at}.{key}"));
                }
                Some(read)
            }
            None => {
                note(at.to_string());
                None
            }
        }
    }

    /// `given` laid over `base` one field at a time: each field that reads
    /// is kept, each that does not is left at the base's. The names of
    /// those left out come back with it.
    pub(super) fn merged<T: Serialize + DeserializeOwned>(
        base: &T,
        given: Value,
    ) -> Option<(T, Vec<String>)> {
        let Value::Object(mut fields) = serde_json::to_value(base).ok()? else {
            return None;
        };
        let Value::Object(given) = given else {
            return None;
        };
        let mut dropped = Vec::new();
        for (key, value) in given {
            let before = fields.insert(key.clone(), value);
            if serde_json::from_value::<T>(Value::Object(fields.clone())).is_err() {
                match before {
                    Some(old) => fields.insert(key.clone(), old),
                    None => fields.remove(&key),
                };
                dropped.push(key);
            }
        }
        serde_json::from_value(Value::Object(fields))
            .ok()
            .map(|read| (read, dropped))
    }

    pub(super) fn schedule<'de, D: Deserializer<'de>>(d: D) -> Result<dm_core::Schedule, D::Error> {
        let value = Value::deserialize(d)?;
        Ok(read_at(&dm_core::Schedule::default(), value, "schedule").unwrap_or_default())
    }

    /// A proxy whose type this dm does not know is turned off rather than
    /// used as some other kind: its host and port mean nothing without it.
    pub(super) fn proxy<'de, D: Deserializer<'de>>(d: D) -> Result<dm_core::Proxy, D::Error> {
        let value = Value::deserialize(d)?;
        Ok(match merged(&dm_core::Proxy::default(), value) {
            Some((mut read, dropped)) => {
                if dropped.iter().any(|k| k == "type") {
                    if read.enabled {
                        read.enabled = false;
                        note("proxy.type (so the proxy is off)".into());
                    } else {
                        note("proxy.type".into());
                    }
                }
                for key in dropped.into_iter().filter(|k| k != "type") {
                    note(format!("proxy.{key}"));
                }
                read
            }
            None => {
                note("proxy".into());
                dm_core::Proxy::default()
            }
        })
    }

    /// A list called `at`, each entry read over `base` and one that is not
    /// an entry at all left out, each said.
    fn list_of<'de, D, T>(d: D, base: &T, at: &str) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Serialize + DeserializeOwned,
    {
        let Value::Array(items) = Value::deserialize(d)? else {
            note(at.to_string());
            return Ok(Vec::new());
        };
        Ok(items
            .into_iter()
            .enumerate()
            .filter_map(|(i, item)| read_at(base, item, &format!("{at}[{i}]")))
            .collect())
    }

    pub(super) fn custom_categories<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<dm_core::Custom>, D::Error> {
        list_of(d, &dm_core::Custom::default(), "custom_categories")
    }

    pub(super) fn site_logins<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<super::SiteLogin>, D::Error> {
        list_of(d, &super::SiteLogin::default(), "site_logins")
    }

    pub(super) fn site_rules<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<super::SiteRule>, D::Error> {
        list_of(d, &super::SiteRule::default(), "site_rules")
    }

    pub(super) fn queue_limits<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<super::QueueLimit>, D::Error> {
        list_of(d, &super::QueueLimit::default(), "queue_limits")
    }

    /// The same for site limits, whose type has no default of its own: no
    /// site (dropped when clamped) and one connection.
    pub(super) fn host_limits<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<dm_core::HostLimit>, D::Error> {
        let base = dm_core::HostLimit {
            host: String::new(),
            connections: 1,
        };
        list_of(d, &base, "host_limits")
    }

    /// The whole file the same way, for when it does not read as it is.
    pub(super) fn settings(text: &str) -> Option<(super::Settings, Vec<String>)> {
        let value: Value = serde_json::from_str(text).ok()?;
        let Value::Object(_) = value else {
            return None;
        };
        merged(&super::Settings::default(), value)
    }
}

/// Where dm keeps what belongs to it rather than to the person using it.
pub fn home() -> PathBuf {
    /* An override, so that driving a real build does not share a queue, a
     * settings file and an inbox with the copy somebody actually uses. There
     * is no other way to run this twice on one machine. */
    if let Some(elsewhere) = std::env::var_os("DM_HOME") {
        return PathBuf::from(elsewhere);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dm-engine")
}

/// The file that makes a copy of dm portable: put beside the program, it
/// keeps dm's settings, list and window beside it too, in [`PORTABLE_DATA`],
/// rather than in the computer's place for them.
pub const PORTABLE_MARKER: &str = "portable";
/// The folder a portable dm keeps what belongs to it in.
pub const PORTABLE_DATA: &str = "dm-data";

/// Where a portable copy of dm in `program_dir` keeps its things, or `None`
/// where there is no [`PORTABLE_MARKER`] beside it. On macOS the program is
/// inside `dm.app`, so the marker beside the app counts too.
pub fn portable_home_in(program_dir: &std::path::Path) -> Option<PathBuf> {
    let mut beside = vec![program_dir.to_path_buf()];
    // dm.app/Contents/MacOS/dm-desktop: the folder holding dm.app.
    if let Some(app) = program_dir
        .ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
    {
        beside.extend(app.parent().map(std::path::Path::to_path_buf));
    }
    beside
        .into_iter()
        .find(|dir| dir.join(PORTABLE_MARKER).is_file())
        .map(|dir| dir.join(PORTABLE_DATA))
}

/// Makes this run portable when a [`PORTABLE_MARKER`] is beside the
/// program, by giving it its own home as `DM_HOME` does — so a portable
/// copy, like a test's, leaves the browsers' registration and the login
/// items to the installed one. To be called first thing, before any other
/// thread starts. An explicit `DM_HOME` wins. Returns the home it chose.
pub fn enter_portable() -> Option<PathBuf> {
    if std::env::var_os("DM_HOME").is_some_and(|h| !h.is_empty()) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let home = portable_home_in(exe.parent()?)?;
    std::env::set_var("DM_HOME", &home);
    Some(home)
}

#[cfg(test)]
mod tests;
