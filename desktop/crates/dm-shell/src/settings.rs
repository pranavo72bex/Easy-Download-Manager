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
use dm_core::{Categories, Category, Limiter, Options, Proxy, Schedule};
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
    /// for a link, and it is what the extension's setting has always used.
    pub speed_limit: u64,
    /// The limit the speed limiter last had on, in KB/s, so turning it back
    /// on — in this run or the next — restores it. Zero for never set.
    #[serde(default)]
    pub last_speed_limit: u64,

    /// Addresses typed into Add URL, newest first, without any login in
    /// them, for picking again; cleared from the dialog.
    #[serde(default)]
    pub recent_urls: Vec<String>,

    /// The download limit: no more than this many megabytes
    /// every `limit_hours` hours. Zero is none.
    pub limit_mb: u64,
    pub limit_hours: u64,

    /// Sort finished files into folders by kind.
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
    /// a height. Kept as the extension's vocabulary so the two settings pages
    /// offer the same choices.
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

    /// Start with the computer, so a browser hand-off has an
    /// app to land in without anyone opening it first. Off by default: a
    /// program that adds itself to login items uninvited is one people remove.
    #[serde(default)]
    pub start_on_login: bool,
    /// Look for a newer dm about once a week. Off unless turned on: it asks GitHub, and dm otherwise
    /// contacts nothing but what is downloaded.
    pub check_updates: bool,
    /// The window's language, by code ("es", "pt-BR"); empty follows the
    /// system's where dm has it.
    pub language: String,
    /// When that was last done, in seconds since 1970.
    pub update_checked: u64,

    /// Open a progress window for a download as it starts.
    #[serde(default = "yes")]
    pub show_progress: bool,
    /// Say so in a window when a download finishes, with Open and Open
    /// folder.
    #[serde(default = "yes")]
    pub show_complete: bool,
    /// Play the system's short sound when a download finishes.
    pub sound_on_complete: bool,
    /// A video from the browser's panel: "Download details" opens for it, to name it and choose where it goes, before it starts.
    pub ask_panel_downloads: bool,
    /// The window's look: "system" (the modern look, light or dark as the computer
    /// is), "light", "dark", or "classic" (a classic Windows look).
    pub theme: String,
    /// What a double-click on a row does: "properties", "open" (a finished
    /// file; Properties for one not finished) or "folder" (its folder).
    pub double_click: String,
    /// A finished file takes the date its server says it last changed,
    /// rather than when it was fetched. Off by default.
    pub server_dates: bool,
    /// Seconds to wait between attempts after a failure; 0 lets dm choose
    /// (quick at first, then half a minute apart).
    pub retry_wait: u64,
    /// The toolbar styles: "both" (icons and text), "icons" (small, icons
    /// only) or "text" (text only). Anything else is "both".
    pub toolbar_style: String,
    /// Toolbar buttons in the order a person put them, by command id; any
    /// not named follow in the default order.
    pub toolbar_order: Vec<String>,
    /// The download list's columns in the order chosen, and those turned
    /// off, by their keys; empty is the default order, all shown.
    pub column_order: Vec<String>,
    pub columns_hidden: Vec<String>,
    /// The column the list is sorted by, by its key, and which way; empty
    /// for the order downloads were added in. Kept so the list is sorted
    /// the same way at the next launch.
    pub sort_column: String,
    #[serde(default = "yes")]
    pub sort_ascending: bool,
    /// Toolbar buttons taken off, by command id.
    pub toolbar_hidden: Vec<String>,
    /// Sign-ins: a user and password used for downloads from a
    /// host, so a protected file need not be typed for each time.
    #[serde(default, deserialize_with = "lenient::site_logins")]
    pub site_logins: Vec<SiteLogin>,

    /// Which downloads the browser extension takes from the browser —
    /// Options › File types. Kept here, in the app, and sent to the extension
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
    /// Sites that take fewer connections at once than `connections`.
    #[serde(default, deserialize_with = "lenient::host_limits")]
    pub host_limits: Vec<dm_core::HostLimit>,
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
    /// Have yt-dlp fetch a video's subtitles and embed them, where an
    /// ffmpeg is there to do it.
    #[serde(default)]
    pub subtitles: bool,
    /// "Files on a site" searches saved to run again, by hand or on a timer.
    #[serde(default)]
    pub site_searches: Vec<crate::lists::Search>,
    /// Queues that run fewer files at once than `concurrency`.
    #[serde(default, deserialize_with = "lenient::queue_limits")]
    pub queue_limits: Vec<QueueLimit>,
    /// Put a `.ts` stream into an MP4 where an ffmpeg is there to do it.
    #[serde(default = "yes")]
    pub remux_ts: bool,
    /// Where unfinished downloads are staged; empty for dm's own folder.
    /// A download already begun keeps the folder it began in.
    #[serde(default)]
    pub parts_dir: String,
}

/// Fewer files at once for one named queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueLimit {
    pub queue: String,
    pub files: usize,
}

/// Ten: the last five half a minute apart, so a download rides out a
/// couple of minutes without a network (`retry::backoff`).
/// How many addresses Add URL remembers.
pub const RECENT_URLS: usize = 20;

fn default_attempts() -> u32 {
    10
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
        serde_json::to_string_pretty(&out).unwrap_or_default()
    }

    /// Settings read from such a file, in place of these: the passwords
    /// this computer already has for the same site and name, and for the
    /// same proxy user, are kept, as is its history.
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
        Ok(read.clamped())
    }
}

/// One entry in Options › Sign-ins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteLogin {
    /// The host, or a suffix of it: `example.com` covers `dl.example.com`.
    pub host: String,
    pub username: String,
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
            host_limits: Vec::new(),
            recheck_hours: 0,
            sound_on_failure: false,
            start_queue_on_launch: false,
            subtitles: false,
            site_searches: Vec::new(),
            queue_limits: Vec::new(),
            remux_ts: true,
            parts_dir: String::new(),
            on_complete: String::new(),
            cookies_file: String::new(),
            connections: 8,
            concurrency: 3,
            // The directory the operating system already nominates. A
            // download manager that invents its own is one people have to go
            // looking in.
            download_dir: dirs::download_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
                .join("dm"),
            speed_limit: 0,
            last_speed_limit: 0,
            recent_urls: Vec::new(),
            limit_mb: 0,
            limit_hours: 1,
            categorize: false,
            category_folders: default_folders(),
            custom_categories: Vec::new(),
            stream_quality: default_quality(),
            schedule: Schedule::default(),
            proxy: Proxy::default(),
            watch_clipboard: false,
            start_on_login: false,
            check_updates: false,
            language: String::new(),
            update_checked: 0,
            show_progress: true,
            show_complete: true,
            sound_on_complete: false,
            ask_panel_downloads: true,
            theme: "system".into(),
            double_click: "properties".into(),
            server_dates: false,
            retry_wait: 0,
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
        if !["properties", "open", "folder"].contains(&self.double_click.as_str()) {
            self.double_click = "properties".into();
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
        // A limit names a site, or it limits nothing; and it is a number of
        // connections the engine will open.
        // A folder for unfinished downloads is a full path: `~` is the home
        // folder, and a relative one is no folder — a Dock launch starts in
        // `/`, where nothing can be written, and every download would fail.
        self.parts_dir = full_path(&self.parts_dir)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
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
        // The download folder is held to the same rule. Options refuses a
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
        // settings travel; one already saved is not (gap 77).
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
                return Some(format!("the cookies file {cookies} could not be read: {e}"));
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
            sign_in: None,
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

    /// The speed ceiling in bytes a second, which is what the engine counts.
    /// The download limit, in the engine's units.
    pub fn allowance(&self) -> dm_core::quota::Allowance {
        dm_core::quota::Allowance {
            bytes: self.limit_mb.saturating_mul(1024 * 1024),
            period: std::time::Duration::from_secs(self.limit_hours.max(1) * 3600),
        }
    }

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
        // folder itself; said, rather than done without a word (gap 77).
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

    pub fn save(&self) -> std::io::Result<()> {
        dm_core::write_atomic(
            &Self::path(),
            serde_json::to_string_pretty(self)?.as_bytes(),
        )
    }
}

impl Settings {
    /// `url` with a site login's user and password in it, where one applies
    /// and the address carries none of its own. HTTP and HTTPS only.
    pub fn with_login(&self, url: &str) -> String {
        let Ok(mut parsed) = url::Url::parse(url) else {
            return url.to_string();
        };
        if !matches!(parsed.scheme(), "http" | "https") || !parsed.username().is_empty() {
            return url.to_string();
        }
        let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
        let found = self.site_logins.iter().find(|login| {
            let want = login
                .host
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            !want.is_empty() && (host == want || host.ends_with(&format!(".{want}")))
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
fn full_path(typed: &str) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_login_goes_on_the_hosts_it_names_and_no_others() {
        let settings = Settings {
            site_logins: vec![SiteLogin {
                host: "example.com".into(),
                username: "me".into(),
                password: "p@ss".into(),
            }],
            ..Settings::default()
        };
        assert_eq!(
            settings.with_login("https://dl.example.com/f.zip"),
            concat!("https://me:", "p%40ss", "@dl.example.com/f.zip")
        );
        assert_eq!(
            settings.with_login("https://notexample.com/f.zip"),
            "https://notexample.com/f.zip"
        );
        // An address with its own login keeps it.
        assert_eq!(
            settings.with_login(concat!("https://you:", "x", "@example.com/f")),
            concat!("https://you:", "x", "@example.com/f")
        );
        assert_eq!(settings.with_login("magnet:?xt=1"), "magnet:?xt=1");
    }

    #[test]
    fn a_settings_file_that_cannot_be_read_is_kept_and_said() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ \"connections\": 8, }").unwrap();

        let (settings, notice) = Settings::load_from(&path);
        assert_eq!(
            settings.connections,
            Settings::default().clamped().connections
        );
        let notice = notice.expect("a file that could not be read went unmentioned");
        assert!(notice.contains("could not be read"), "{notice}");
        assert!(
            !path.exists(),
            "the unreadable file is still where a save would replace it"
        );
        let kept = dir.path().join("settings.json.unreadable");
        assert_eq!(
            std::fs::read_to_string(kept).unwrap(),
            "{ \"connections\": 8, }"
        );
    }

    /// Gap 77: a category saved on a Mac or Linux with a name Windows
    /// cannot make ("Live: 2024", "Misc.") was dropped at load, and the
    /// next save deleted it. It is kept where it works; only a new or
    /// renamed name is held to every computer's rules. A kind's folder
    /// that cannot be used here is said, not filed into the root silently.
    #[test]
    fn a_saved_category_that_works_here_is_kept_and_a_new_one_is_portable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"custom_categories": [{"name": "Live: 2024", "extensions": ["mkv"]},
                {"name": "Misc."}, {"name": "a/b"}],
                "category_folders": ["Videos/HD", "Music"]}"#,
        )
        .unwrap();
        let (read, notice) = Settings::load_from(&path);
        let names: Vec<&str> = read
            .custom_categories
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        if cfg!(windows) {
            assert!(names.is_empty(), "{names:?}");
        } else {
            assert_eq!(names, ["Live: 2024", "Misc."]);
            assert_eq!(read.custom_categories[0].extensions, ["mkv"]);
        }
        let notice = notice.expect("nothing said");
        assert!(notice.contains("\"a/b\""), "{notice}");
        assert!(notice.contains("\"Videos/HD\""), "{notice}");

        // Kept, it does not block an unrelated change; a new one is held to
        // every computer's rules, and so is a renamed kind's folder.
        let mut changed = read.clone();
        changed.connections = 3;
        assert_eq!(changed.refusal(&read), None);
        changed.custom_categories.push(dm_core::Custom {
            name: "Docs: work".into(),
            ..Default::default()
        });
        assert!(changed
            .refusal(&read)
            .is_some_and(|r| r.contains("Windows")));
        let mut renamed = read.clone();
        renamed.category_folders[1] = "CON".into();
        assert!(renamed.refusal(&read).is_some());
    }

    #[test]
    fn one_odd_entry_costs_that_entry_and_not_every_setting() {
        /* A file edited by hand, or written by a later dm: a proxy type this
         * one does not know, a schedule and limits missing fields, a login
         * with no password, and a number written as a word. Each of these
         * used to move the whole file aside and start from the defaults. */
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let text = r#"{
            "connections": 4,
            "language": "de",
            "proxy": {"enabled": true, "type": "quic", "host": "p.example", "port": 3128},
            "schedule": {"enabled": true, "from": "02:00"},
            "host_limits": [{"host": "slow.example"}],
            "queue_limits": [{"queue": "Nightly"}],
            "site_logins": [{"host": "files.example", "username": "me"}],
            "custom_categories": [{"name": "Books"}],
            "concurrency": "three"
        }"#;
        std::fs::write(&path, text).unwrap();

        let (read, notice) = Settings::load_from(&path);
        assert_eq!(read.connections, 4);
        assert_eq!(read.language, "de");
        assert!(
            !read.proxy.enabled,
            "an unknown proxy type is off, not another kind"
        );
        assert_eq!(read.proxy.host, "p.example");
        assert!(read.schedule.enabled);
        assert_eq!(read.schedule.from, "02:00");
        assert_eq!(read.schedule.to, Schedule::default().to);
        assert_eq!(read.host_limits[0].host, "slow.example");
        assert_eq!(read.host_limits[0].connections, 1);
        assert_eq!(read.queue_limits[0].queue, "Nightly");
        assert_eq!(read.queue_limits[0].files, 1, "clamped up from none");
        assert_eq!(read.site_logins[0].username, "me");
        assert_eq!(read.custom_categories[0].name, "Books");
        assert_eq!(read.concurrency, Settings::default().concurrency);
        let notice = notice.expect("the entry left out went unmentioned");
        assert!(notice.contains("concurrency"), "{notice}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json.unreadable")).unwrap(),
            text,
            "the file as it was is kept"
        );
    }

    #[test]
    fn what_a_reader_inside_left_out_is_named_and_the_file_kept() {
        /* The file reads as a whole — every odd value is inside a part read
         * leniently — so nothing failed at the top. An unknown proxy type,
         * a list entry that is not one, a bad field in an entry and a
         * category that is not one folder were each dropped without a
         * word, and the next save wrote over the only copy. */
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let text = r#"{
            "proxy": {"enabled": true, "type": "quic", "host": "p.example", "port": 3128},
            "site_logins": [7, {"host": "a.example", "username": "me"}],
            "queue_limits": [{"queue": "Nightly", "files": "two"}],
            "host_limits": [{"host": "slow.example", "connections": -1}],
            "custom_categories": [{"name": "Work/Docs", "extensions": ["pdf"]}, {"name": "Books"}]
        }"#;
        std::fs::write(&path, text).unwrap();
        let (read, notice) = Settings::load_from(&path);
        let notice = notice.expect("what was left out went unmentioned");
        for named in [
            "proxy.type",
            "site_logins[0]",
            "queue_limits[0].files",
            "host_limits[0].connections",
            "Work/Docs",
        ] {
            assert!(notice.contains(named), "{named} not named: {notice}");
        }
        assert!(!read.proxy.enabled);
        assert_eq!(read.site_logins.len(), 1);
        assert_eq!(read.queue_limits[0].queue, "Nightly");
        assert_eq!(
            read.custom_categories.len(),
            1,
            "the bad category is left out"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json.unreadable")).unwrap(),
            text,
            "the file as it was is kept before a save writes over it"
        );

        // A clean file says nothing and keeps no copy.
        let clean = tempfile::tempdir().unwrap();
        let path = clean.path().join("settings.json");
        std::fs::write(&path, serde_json::to_string(&Settings::default()).unwrap()).unwrap();
        assert_eq!(Settings::load_from(&path).1, None);
        assert!(!clean.path().join("settings.json.unreadable").exists());
    }

    #[test]
    fn a_saved_schedule_with_unreadable_times_is_turned_off_and_said() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"schedule": {"enabled": true, "from": "25:00", "to": "1am", "days": []}}"#,
        )
        .unwrap();
        let (read, notice) = Settings::load_from(&path);
        assert!(!read.schedule.enabled);
        assert!(notice.unwrap().contains("not HH:MM"));
    }

    #[test]
    fn a_change_that_cannot_work_is_refused_and_an_old_one_blocks_nothing() {
        let before = Settings::default();
        let with = |change: fn(&mut Settings)| {
            let mut s = before.clone();
            change(&mut s);
            s.refusal(&before)
        };
        assert!(with(|s| {
            s.schedule.enabled = true;
            s.schedule.from = "25:00".into();
        })
        .is_some());
        assert!(with(|s| s.download_dir = "Downloads".into()).is_some());
        assert!(with(|s| s.download_dir = "~/Downloads".into()).is_none());
        assert!(with(|s| s.parts_dir = "parts".into()).is_some());
        assert!(with(|s| s.custom_categories = vec![dm_core::Custom {
            name: "Work/Docs".into(),
            ..Default::default()
        }])
        .is_some());
        assert!(with(|s| s.cookies_file = "/no/such/cookies.txt".into()).is_some());
        assert!(with(|s| s.language = "de".into()).is_none());

        // Saved before, and so not this change's doing.
        let odd = Settings {
            cookies_file: "/no/such/cookies.txt".into(),
            ..before.clone()
        };
        let mut next = odd.clone();
        next.language = "fr".into();
        assert_eq!(next.refusal(&odd), None);
    }

    #[test]
    fn a_download_folder_from_home_is_expanded_and_a_relative_one_is_not_kept() {
        let at = |typed: &str| {
            Settings {
                download_dir: typed.into(),
                ..Default::default()
            }
            .clamped()
            .download_dir
        };
        let home = dirs::home_dir().unwrap();
        assert_eq!(at("~/Downloads"), home.join("Downloads"));
        assert_eq!(at("Downloads"), Settings::default().download_dir);
    }

    #[test]
    fn a_folder_for_one_download_is_made_full_or_refused() {
        let home = dirs::home_dir().unwrap();
        let caller = std::env::temp_dir().join("caller");
        assert_eq!(full_folder("~/x", None), Some(home.join("x")));
        assert_eq!(full_folder("out", Some(&caller)), Some(caller.join("out")));
        assert_eq!(full_folder("out", None), None);
        assert_eq!(full_folder("~someone/x", Some(&caller)), None);
        let absolute = caller.join("abs");
        assert_eq!(
            full_folder(&absolute.display().to_string(), None),
            Some(absolute)
        );
    }

    #[test]
    fn no_settings_file_is_not_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let (_, notice) = Settings::load_from(&dir.path().join("settings.json"));
        assert!(notice.is_none());
    }

    #[test]
    fn values_outside_what_the_engine_accepts_are_brought_back_in() {
        let wild = Settings {
            connections: 5000,
            concurrency: 0,
            ..Default::default()
        };
        let sane = wild.clamped();
        assert_eq!(sane.connections, 32);
        assert_eq!(sane.concurrency, 1);
    }

    #[test]
    fn a_parts_folder_is_a_full_path_or_dms_own() {
        let at = |typed: &str| {
            Settings {
                parts_dir: typed.into(),
                ..Default::default()
            }
            .clamped()
            .parts_dir
        };
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            at("~/dm-parts"),
            home.join("dm-parts").display().to_string()
        );
        assert_eq!(at("parts"), "", "relative");
        assert_eq!(at("~someone/x"), "", "another user's home is not read");
        assert_eq!(at("  "), "");
        let absolute = std::env::temp_dir().join("dm-parts").display().to_string();
        assert_eq!(at(&absolute), absolute);
    }

    /// The click keys are ones the extension knows: anything else in a
    /// hand-edited file is what they ship as, and "none" stays none.
    #[test]
    fn the_click_keys_are_ones_the_extension_knows() {
        let mut catch = dm_protocol::CatchRules {
            keep_key: "hyper".into(),
            take_key: "f13".into(),
            ..Default::default()
        };
        let odd = Settings {
            catch: catch.clone(),
            ..Settings::default()
        }
        .clamped();
        assert_eq!(
            (odd.catch.keep_key.as_str(), odd.catch.take_key.as_str()),
            ("alt", "shift")
        );
        catch.keep_key = "alt".into();
        catch.take_key = String::new();
        let none = Settings {
            catch,
            ..Settings::default()
        };
        assert_eq!(none.clamped().catch.take_key, "");
    }

    /// Settings carried to another computer leave every password behind,
    /// and the one they arrive on keeps its own.
    #[test]
    fn exported_settings_carry_no_password_and_imported_ones_keep_this_computers() {
        let mut here = Settings {
            site_logins: vec![SiteLogin {
                host: "example.com".into(),
                username: "me".into(),
                password: "kept".into(),
            }],
            recent_urls: vec!["https://e.com/a.zip".into()],
            connections: 5,
            ..Settings::default()
        };
        here.proxy.username = "proxyuser".into();
        here.proxy.password = "proxykept".into();
        let file = here.exported();
        assert!(!file.contains("kept"), "a password left in the file");
        assert!(!file.contains("e.com/a.zip"), "history left in the file");
        let mut elsewhere = Settings {
            site_logins: here.site_logins.clone(),
            ..Settings::default()
        };
        elsewhere.proxy.username = "proxyuser".into();
        elsewhere.proxy.password = "proxykept".into();
        let read = elsewhere.imported(&file).unwrap();
        assert_eq!(read.connections, 5);
        assert_eq!(read.site_logins[0].password, "kept");
        assert_eq!(read.proxy.password, "proxykept");
        assert!(elsewhere.imported("not settings").is_err());
    }

    #[test]
    fn attempts_timeouts_and_site_limits_are_held_to_what_the_engine_takes() {
        let wild = Settings {
            attempts: 5000,
            read_timeout: 1,
            host_limits: vec![
                dm_core::HostLimit {
                    host: "  Example.COM ".into(),
                    connections: 0,
                },
                dm_core::HostLimit {
                    host: "   ".into(),
                    connections: 4,
                },
            ],
            ..Default::default()
        }
        .clamped();
        assert_eq!(wild.attempts, 100);
        assert_eq!(wild.read_timeout, 5);
        assert_eq!(
            wild.host_limits,
            [dm_core::HostLimit {
                host: "example.com".into(),
                connections: 1
            }],
            "a limit naming no site is dropped"
        );
        // Zero is "keep trying", and is kept.
        assert_eq!(
            Settings {
                attempts: 0,
                ..Default::default()
            }
            .clamped()
            .attempts,
            0
        );
        let d = Settings::default();
        assert_eq!((d.attempts, d.read_timeout), (10, 30));
        assert_eq!(d.options(PathBuf::new(), Limiter::unlimited()).attempts, 10);
    }

    #[test]
    fn an_empty_folder_falls_back_rather_than_saving_to_nowhere() {
        let empty = Settings {
            download_dir: PathBuf::new(),
            ..Default::default()
        };
        assert!(!empty.clamped().download_dir.as_os_str().is_empty());
    }

    #[test]
    fn a_short_folder_list_is_filled_out_rather_than_left_ragged() {
        // The engine indexes this by category; a file edited by hand can
        // easily be short, and the kinds past its end would silently stop
        // being sorted.
        let short = Settings {
            category_folders: vec!["Videos".into()],
            ..Default::default()
        }
        .clamped();
        assert_eq!(short.category_folders.len(), Category::ALL.len());
        assert_eq!(short.category_folders[0], "Videos");
    }

    #[test]
    fn the_speed_limit_is_read_in_the_unit_the_engine_counts() {
        let settings = Settings {
            speed_limit: 500,
            ..Default::default()
        };
        assert_eq!(settings.speed_limit_bytes(), 500 * 1024);
        assert_eq!(Settings::default().speed_limit_bytes(), 0, "no limit");
    }

    #[test]
    fn a_quality_is_read_as_the_engine_means_it() {
        let at = |q: &str| {
            Settings {
                stream_quality: q.into(),
                ..Default::default()
            }
            .quality()
        };

        assert_eq!(at("best"), Quality::Best);
        assert_eq!(at("audio"), Quality::Audio);
        assert_eq!(at("720"), Quality::UpTo(720));
        // A typo takes the best on offer rather than the smallest, which is
        // what a value of zero would otherwise mean.
        assert_eq!(at("hd"), Quality::Best);
        assert_eq!(at(""), Quality::Best);
    }

    #[test]
    fn settings_written_by_an_earlier_version_still_read() {
        /* The file on disk outlives the version that wrote it, and a queue
         * that refuses to start because a field is missing is a worse
         * outcome than one that fills in a default. */
        let old = r#"{"connections":4,"concurrency":2,"download_dir":"/tmp/x"}"#;
        let settings: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.connections, 4);
        assert_eq!(settings.speed_limit, 0);
        assert!(!settings.categorize);
        assert!(!settings.schedule.enabled);
        assert!(!settings.proxy.enabled);
        assert_eq!(settings.quality(), Quality::Best);
    }

    #[test]
    fn the_extension_catches_the_default_file_types_until_told_otherwise() {
        // A settings file from before the extension asked the app has no
        // `catch`; the browser must still hand over the default file types.
        let old: Settings = serde_json::from_str(r#"{"connections":4}"#).unwrap();
        assert!(old.catch.enabled);
        for kind in ["zip", "exe", "mp4", "pdf", "iso"] {
            assert!(old.catch.extensions.iter().any(|e| e == kind), "{kind}");
        }

        let chosen: Settings = serde_json::from_str(
            r#"{"catch":{"enabled":false,"extensions":["bin"],"minSizeKb":5,"skipHosts":["e.com"]}}"#,
        )
        .unwrap();
        assert!(!chosen.catch.enabled);
        assert_eq!(chosen.catch.extensions, vec!["bin".to_string()]);
        assert_eq!(chosen.catch.min_size_kb, 5);
        let saved = serde_json::to_value(&chosen).unwrap();
        assert_eq!(saved["catch"]["skipHosts"][0], "e.com");
    }

    #[test]
    fn the_defaults_are_already_within_bounds() {
        let d = Settings::default();
        let c = d.clone().clamped();
        assert_eq!(d.connections, c.connections);
        assert_eq!(d.concurrency, c.concurrency);
    }
}
