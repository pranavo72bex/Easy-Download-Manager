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
    pub custom_categories: Vec<dm_core::Custom>,

    /// Which rendition to take from an adaptive stream: `best`, `audio`, or
    /// a height. Kept as the extension's vocabulary so the two settings pages
    /// offer the same choices.
    pub stream_quality: String,

    /// When the queue may run.
    pub schedule: Schedule,

    /// Where downloads connect through.
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
    /// Toolbar buttons taken off, by command id.
    pub toolbar_hidden: Vec<String>,
    /// Sign-ins: a user and password used for downloads from a
    /// host, so a protected file need not be typed for each time.
    #[serde(default)]
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
    #[serde(default)]
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
    #[serde(default)]
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
pub struct QueueLimit {
    pub queue: String,
    pub files: usize,
}

fn default_attempts() -> u32 {
    5
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

/// One entry in Options › Sign-ins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
            toolbar_style: "both".into(),
            toolbar_order: Vec::new(),
            column_order: Vec::new(),
            columns_hidden: Vec::new(),
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
        let parts = self.parts_dir.trim();
        self.parts_dir = match parts.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => dirs::home_dir()
                .map(|home| {
                    home.join(rest.trim_start_matches('/'))
                        .display()
                        .to_string()
                })
                .unwrap_or_default(),
            _ if std::path::Path::new(parts).is_absolute() => parts.to_string(),
            _ => String::new(),
        };
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
        if self.download_dir.as_os_str().is_empty() {
            self.download_dir = Settings::default().download_dir;
        }
        // One name per category, always: the engine indexes this list by
        // category, and a short one read from a hand-edited file would leave
        // the kinds past its end unsorted for no stated reason.
        self.category_folders
            .resize(Category::ALL.len(), String::new());
        self
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
            remux: self.remux_ts,
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
        match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => (settings.clamped(), None),
            Err(e) => {
                let aside = path.with_extension("json.unreadable");
                let notice = match std::fs::rename(path, &aside) {
                    Ok(()) => format!(
                        "the saved settings could not be read ({e}); they were kept as {} and \
                         the defaults are in use",
                        aside.display()
                    ),
                    Err(io) => format!(
                        "the saved settings could not be read ({e}) or moved aside ({io}); the \
                         defaults are in use and the next change will replace them"
                    ),
                };
                (Settings::default().clamped(), Some(notice))
            }
        }
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
            "https://me:p%40ss@dl.example.com/f.zip"
        );
        assert_eq!(
            settings.with_login("https://notexample.com/f.zip"),
            "https://notexample.com/f.zip"
        );
        // An address with its own login keeps it.
        assert_eq!(
            settings.with_login("https://you:x@example.com/f"),
            "https://you:x@example.com/f"
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
        assert_eq!((d.attempts, d.read_timeout), (5, 30));
        assert_eq!(d.options(PathBuf::new(), Limiter::unlimited()).attempts, 5);
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
