//! What the browser extension and the desktop app say to each other.
//!
//! The two used to be written in two languages, each with its own copy of
//! the vocabulary, kept in step by a comment asking whoever changed one to
//! retype the other. Both are Rust now, so the words, the shapes of the
//! messages and the rule that decides which downloads the app takes are
//! written here once: the native host in dm-shell answers with these types
//! and the extension, compiled to WebAssembly, asks with them.
//!
//! Native messaging frames are JSON objects. Every one carries
//! `protocolVersion` and a `type`; the rest depends on the type.
//!
//! A frame's `protocolVersion` is the oldest version that reads it
//! correctly, not the newest its writer knows: a host older than a frame's
//! version does not answer it, so everything an older side reads correctly
//! goes out at the old number and only what it would misread goes out at
//! the new one. Each side reads every version from [`OLDEST_PROTOCOL`] to
//! [`PROTOCOL_VERSION`], and the host's hello says the newest it reads
//! (`protocol`), which is how the extension knows what the app can judge.
//!
//! | version | what it added                                                |
//! |---------|--------------------------------------------------------------|
//! | 1       | everything else                                              |
//! | 2       | `download.start` with `caught`: the host judges `mime`, the  |
//! |         | `cookies` and the `deadline`, refusing a sign-in page served |
//! |         | in the file's place. A version-1 app ignored those fields    |
//! |         | and took the page, and the browser's copy was erased         |
//!
//! | extension sends   | host answers                                  |
//! |-------------------|-----------------------------------------------|
//! | `engine.hello`    | `engine.hello` with `engineVersion`, `ytdlp`  |
//! | `settings.get`    | `settings.get` with `catch` ([`CatchRules`])  |
//! | `download.start`  | `download.started`, or `download.failed`      |
//! | `media.formats`   | `media.formats` with `choices`, or a `reason` |
//! | `download.batch`  | `download.started`, or `download.failed`      |
//!
//! The host is deliberately one-way past `download.started`: the extension
//! catches and the application shows what happened.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The newest version this side reads and writes. A frame of a version
/// outside [`OLDEST_PROTOCOL`]`..=PROTOCOL_VERSION` is not read.
pub const PROTOCOL_VERSION: u64 = 2;

/// The oldest version still read: what an older extension or app sends.
pub const OLDEST_PROTOCOL: u64 = 1;

/// The first version whose host judges a caught download by what the
/// browser said it was (`mime`), so it refuses a sign-in page served to dm
/// in the file's place. The extension hands a download the browser holds,
/// and will erase, only to an app at least this new.
pub const JUDGES_PAGES_SINCE: u64 = 2;

/// The native messaging host's name, which is also its manifest's file name
/// without the `.json`.
pub const NATIVE_HOST: &str = "com.dm.engine";

/// The id `manifest.firefox.json` gives the extension. Firefox host manifests
/// name extensions by this, not by an origin.
pub const FIREFOX_EXTENSION_ID: &str = "easy-download-manager@pranavo72bex.github.io";

/// The id Chrome gives the unpacked extension, fixed by the public `key` in
/// `extension/manifest.json`. A store build gets another id from the store,
/// which is registered by naming it.
pub const CHROME_EXTENSION_ID: &str = "fojpndfjodldkfbamkofdmnfedgjjejo";

/// The frame types, for code that has to name one without building it.
pub mod kind {
    pub const HELLO: &str = "engine.hello";
    pub const SETTINGS: &str = "settings.get";
    pub const START: &str = "download.start";
    pub const STARTED: &str = "download.started";
    pub const FAILED: &str = "download.failed";
    pub const SHOW: &str = "app.show";
}

/// What the extension asks.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum Request {
    /// Is an app there, which version, and can it resolve a player page.
    #[serde(rename = "engine.hello")]
    Hello,
    /// Which downloads the app wants caught. They live in the app's settings
    /// (Options › File types), not in the extension.
    #[serde(rename = "settings.get")]
    Settings,
    /// Take this download.
    #[serde(rename = "download.start")]
    Start(Start),
    /// What qualities a page's video comes in, for the "Download video"
    /// panel's list.
    #[serde(rename = "media.formats")]
    Formats(FormatsAsked),
    /// A page's links, all or the selected ones, for the app's "Download all
    /// links" dialog, as the browser's menus hand them over.
    #[serde(rename = "download.batch")]
    Batch(Batch),
    /// Bring the app's window to the front, starting the app if it is not
    /// running: what the toolbar button does. It hands over nothing.
    #[serde(rename = "app.show")]
    Show,
}

/// Links handed over together.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Batch {
    pub id: String,
    /// The page they are on, sent as Referer.
    pub page: String,
    pub urls: Vec<String>,
    /// The page's site's cookies, where the toolbar popup hands over its
    /// sign-in: they go with its links on that site only, as with a single
    /// download. Empty from an extension that sends none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
}

/// A page to list the qualities of.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct FormatsAsked {
    pub url: String,
    #[serde(default)]
    pub referrer: String,
}

/// One quality the panel offers: what the list says, the quality to fetch it
/// at ("audio", a height such as "720", or "best"), and its size in bytes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Choice {
    pub label: String,
    pub quality: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// One download handed over.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Start {
    /// The extension's name for this hand-off, echoed in the answer. The host
    /// files the request under it, so it holds it to a plain name.
    pub id: String,
    pub url: String,
    /// The page the download came from. A media URL alone is often not
    /// something anything can fetch twice; the page it was on usually is.
    #[serde(default)]
    pub referrer: String,
    /// A download the browser had started and is holding: the host checks
    /// that dm can fetch it before taking it, because a refusal now leaves
    /// it with the browser and a failure later loses it. A file asked for
    /// from the panel or a menu is checked too: refused, the extension has
    /// the browser download it instead. A page for yt-dlp is not checked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub caught: bool,
    /// The quality picked in the panel's list; absent is the app's setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    /// What the list called it, for the app's dialog to show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A video from the panel: the app opens "Download details" for it, to name it and choose where it goes, before starting.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ask: bool,
    /// The browser's cookies for the download's site, as the `Cookie`
    /// header it would send, where the person switched "hand over this
    /// site's sign-in" on in the toolbar popup. Empty otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
    /// What the browser said the download was (its `DownloadItem.mime`), for
    /// a download it had started. A server that sends the browser a zip and
    /// dm, without the browser's sign-in, a login page is not one dm can
    /// fetch; this is how the host tells. Empty where the browser did not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mime: String,
    /// When the extension stops waiting for an answer, in milliseconds since
    /// the Unix epoch on this computer's clock, which both sides read. Past
    /// it the extension has given the download back to the browser, so a
    /// host that would answer later refuses instead of filing a second copy.
    /// Absent from an older extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<u64>,
    /// The browser holds this download, paused, and erases its copy once
    /// the app has taken it: a sign-in page taken for the file loses it.
    /// Only such a hand-off needs a host that judges `mime` (version 2); a
    /// file a person asked for from a menu or the panel, which the browser
    /// never started, is checked (`caught`) at version 1 as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
}

/// The longest the host spends asking a server about a caught download.
/// Well inside the extension's wait for an answer (20 seconds), which also
/// has to cover the host's own start.
pub const PROBE_WITHIN_MS: u64 = 10_000;

/// Kept back from the extension's deadline for filing the download and
/// sending the answer back.
pub const ANSWER_MARGIN_MS: u64 = 2_000;

/// How long the host may still spend on a hand-off whose answer is due by
/// `deadline`, or `None` when it is too late to take it at all: the extension
/// will have given it back to the browser before an answer could arrive, and
/// taking it then has the file downloaded twice.
pub fn time_left(now_ms: u64, deadline: Option<u64>) -> Option<u64> {
    let Some(by) = deadline else {
        return Some(PROBE_WITHIN_MS);
    };
    by.checked_sub(now_ms)?
        .checked_sub(ANSWER_MARGIN_MS)
        .filter(|left| *left > 0)
        .map(|left| left.min(PROBE_WITHIN_MS))
}

/// Whether what dm was served is a page standing in for the file the browser
/// is downloading: typically a sign-in page that a site redirects a request
/// without its cookies to. `served` is the type dm's request ended at, after
/// redirects; `browser_mime` what the browser was given; `url` the download.
///
/// Only an HTML answer is suspect, and only when the browser was not getting
/// HTML itself. Where the browser did not say, the address's own file type
/// decides: `report.pdf` answered with a page is not the report.
pub fn page_instead_of_file(browser_mime: &str, url: &str, served: &str) -> bool {
    let is_page = |mime: &str| {
        let essence = mime
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        essence == "text/html" || essence == "application/xhtml+xml"
    };
    if !is_page(served) {
        return false;
    }
    if !browser_mime.trim().is_empty() {
        return !is_page(browser_mime);
    }
    // Names that are pages, or scripts that usually answer with one.
    const PAGES: &[&str] = &[
        "html", "htm", "xhtml", "shtml", "php", "asp", "aspx", "jsp", "cgi", "pl",
    ];
    extension_of(base_name(path_of(url))).is_some_and(|ext| !PAGES.contains(&ext.as_str()))
}

/// What the host answers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum Reply {
    #[serde(rename = "engine.hello")]
    Hello(Hello),
    #[serde(rename = "settings.get")]
    Settings(SettingsReply),
    #[serde(rename = "download.started")]
    Started { id: String },
    #[serde(rename = "download.failed")]
    Failed {
        id: String,
        #[serde(default)]
        failure: Option<Failure>,
    },
    /// The qualities on offer, best first; empty with a `reason` when the
    /// page could not be read (no yt-dlp, an unsupported site).
    #[serde(rename = "media.formats")]
    Formats {
        #[serde(default)]
        choices: Vec<Choice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The window was asked to show itself; a `reason` when it could not be.
    #[serde(rename = "app.shown")]
    Shown {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Hello {
    #[serde(rename = "engineVersion", default)]
    pub engine_version: String,
    /// Whether the app has a yt-dlp to hand a player page to. Absent from a
    /// host too old to say, which is not the same as `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ytdlp: Option<bool>,
    /// The newest protocol version the app reads. Absent from an app of
    /// version 1, which said nothing of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u64>,
}

impl Hello {
    /// The newest protocol version the app reads.
    pub fn protocol(&self) -> u64 {
        self.protocol.unwrap_or(OLDEST_PROTOCOL)
    }

    /// Whether the app refuses a sign-in page served to it in place of a
    /// file the browser is downloading — without which a download the
    /// browser holds must not be handed to it, since the browser's copy is
    /// erased once the app has taken it.
    pub fn judges_pages(&self) -> bool {
        self.protocol() >= JUDGES_PAGES_SINCE
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SettingsReply {
    #[serde(rename = "catch", default)]
    pub rules: CatchRules,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Failure {
    #[serde(default)]
    pub message: String,
}

/// Why a frame was not read.
#[derive(Debug, Clone, PartialEq)]
pub enum Unreadable {
    /// Another protocol version, or none.
    Version(Option<u64>),
    /// The right version, but not a message this side knows.
    Shape(String),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::Version(Some(v)) => write!(
                f,
                "protocol version {v} is not one of {OLDEST_PROTOCOL} to {PROTOCOL_VERSION}"
            ),
            Unreadable::Version(None) => write!(f, "a frame with no protocol version"),
            Unreadable::Shape(why) => write!(f, "a frame that could not be read: {why}"),
        }
    }
}

fn envelope<T: Serialize>(message: &T, version: u64) -> Value {
    let mut value = serde_json::to_value(message).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.insert("protocolVersion".into(), version.into());
    }
    value
}

fn open<T: serde::de::DeserializeOwned>(frame: &Value) -> Result<T, Unreadable> {
    match frame.get("protocolVersion").and_then(Value::as_u64) {
        Some(v) if (OLDEST_PROTOCOL..=PROTOCOL_VERSION).contains(&v) => {}
        other => return Err(Unreadable::Version(other)),
    }
    // Keys a message does not use — `protocolVersion`, or the `filename` an
    // older extension sent — are ignored rather than refused.
    serde_json::from_value(frame.clone()).map_err(|e| Unreadable::Shape(e.to_string()))
}

impl Request {
    /// The frame, at the oldest version that reads it correctly.
    pub fn to_frame(&self) -> Value {
        envelope(self, self.needs())
    }

    /// The oldest protocol version that reads this request correctly. A
    /// download the browser holds (`held`) needs a host that judges what it
    /// is served (version 2): a version-1 host would take a sign-in page for
    /// the file, so it is sent at 2, which such a host does not answer, and
    /// the extension gives the download back when no answer comes. Anything
    /// else, a file asked for from a menu included, a version-1 host reads.
    pub fn needs(&self) -> u64 {
        match self {
            Request::Start(start) if start.held => JUDGES_PAGES_SINCE,
            _ => OLDEST_PROTOCOL,
        }
    }

    pub fn from_frame(frame: &Value) -> Result<Self, Unreadable> {
        open(frame)
    }
}

impl Reply {
    /// The frame, at version 1: every reply is one a version-1 extension
    /// reads correctly (the hello's `protocol` is a key it ignores).
    pub fn to_frame(&self) -> Value {
        envelope(self, OLDEST_PROTOCOL)
    }

    pub fn from_frame(frame: &Value) -> Result<Self, Unreadable> {
        open(frame)
    }

    /// A refusal, in the one shape every refusal takes.
    pub fn failed(id: &str, message: impl Into<String>) -> Self {
        Reply::Failed {
            id: id.into(),
            failure: Some(Failure {
                message: message.into(),
            }),
        }
    }
}

/* ------------------------------------------------------------ catching */

/// The file types caught out of the box: archives, installers and disk
/// images, video, audio and documents — and `.torrent`.
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    // archives
    "zip", "rar", "7z", "gz", "gzip", "tgz", "bz2", "xz", "zst", "tar", "z", "lzh", "arj", "cab",
    // programs and disk images
    "exe", "msi", "msu", "dmg", "pkg", "deb", "rpm", "apk", "appimage", "iso", "img", "bin",
    // video
    "mp4", "m4v", "mkv", "avi", "mov", "wmv", "flv", "webm", "mpg", "mpeg", "3gp", "ogv", "ts",
    // audio
    "mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "wma", "aif", "aiff",
    // documents
    "pdf", "epub", "mobi", "djvu", "ppt", "pptx", "pps", // the one kind only dm has
    "torrent",
];

/// Which downloads the app takes from the browser. Kept in the app's settings
/// file as `catch`, and sent to the extension when it asks.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CatchRules {
    /// Off means the browser keeps every download it starts; the context menu
    /// and the toolbar button still hand things over on request.
    pub enabled: bool,
    /// File types, without the dot. Empty means every type.
    pub extensions: Vec<String>,
    /// Smaller than this stays with the browser. Zero is no floor. An unknown
    /// size is not a reason to leave a download.
    #[serde(alias = "min_size_kb")]
    pub min_size_kb: u64,
    /// Hosts whose downloads stay with the browser, each with its subdomains.
    #[serde(alias = "skip_hosts")]
    pub skip_hosts: Vec<String>,
    /// The key that, held while a link is clicked, leaves its download with
    /// the browser: "alt", "shift" or "ctrl" (Cmd on a Mac too).
    #[serde(alias = "keep_key")]
    pub keep_key: String,
    /// The key that, held while a link is clicked, hands its download to dm
    /// whatever the types and size floor say; the switch and the sites to
    /// skip still hold. Empty for none.
    #[serde(alias = "take_key")]
    pub take_key: String,
    /// Hosts whose links work once: handed to dm without dm asking the
    /// server first, which spent such a link before dm's own download could
    /// use it (gap 49). Each covers its subdomains.
    #[serde(alias = "once_hosts")]
    pub once_hosts: Vec<String>,
}

/// Whether any of `urls` is served from a host on `list`, each entry
/// covering its subdomains, with or without a leading `*.`.
fn listed(list: &[String], urls: &[&str]) -> bool {
    let hosts: Vec<String> = urls
        .iter()
        .map(|url| host_of(url))
        .filter(|host| !host.is_empty())
        .collect();
    list.iter().any(|entry| {
        let entry = entry.trim().trim_start_matches("*.").to_ascii_lowercase();
        !entry.is_empty()
            && hosts.iter().any(|host| {
                *host == entry
                    || host
                        .strip_suffix(entry.as_str())
                        .is_some_and(|rest| rest.ends_with('.'))
            })
    })
}

/// The keys a click can be held with, as the rules name them.
pub const CLICK_KEYS: [&str; 3] = ["alt", "shift", "ctrl"];

impl Default for CatchRules {
    fn default() -> Self {
        CatchRules {
            enabled: true,
            extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            min_size_kb: 0,
            skip_hosts: Vec::new(),
            keep_key: "alt".into(),
            take_key: "shift".into(),
            once_hosts: Vec::new(),
        }
    }
}

/// A download the browser is starting, as far as catching cares.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Candidate<'a> {
    /// Where the bytes come from, after redirects when the browser knows.
    pub url: &'a str,
    /// The address the download was asked for, before any redirect. A
    /// site's downloads are often served from another host
    /// (`github.com` to `objects.githubusercontent.com`), and a site on the
    /// skip list means its downloads, wherever they are served from.
    pub asked_url: &'a str,
    /// The page the download was started from.
    pub referrer: &'a str,
    /// The name the browser chose, if it has chosen; a full path is fine.
    pub filename: &'a str,
    pub mime: &'a str,
    pub size: Option<u64>,
}

/// Why a download was left with the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    Off,
    SkippedHost,
    TooSmall,
    NotAFileType,
}

impl Left {
    pub fn reason(self) -> &'static str {
        match self {
            Left::Off => "catching is turned off in dm's settings",
            Left::SkippedHost => "that site is on dm's list of sites to leave alone",
            Left::TooSmall => "smaller than dm's minimum size to catch",
            Left::NotAFileType => "not one of the file types dm catches",
        }
    }
}

impl CatchRules {
    /// Whether a link from `url`, met on `referrer`, is one of a site whose
    /// links work once, so dm takes it without asking the server first.
    pub fn works_once(&self, url: &str, referrer: &str) -> bool {
        listed(&self.once_hosts, &[url, referrer])
    }

    /// Whether the app wants this download.
    pub fn catches(&self, download: &Candidate<'_>) -> Result<(), Left> {
        self.takes(download, false)
    }

    /// Whether the app wants this download, where `every_type` says the
    /// toolbar popup takes every download from its site. That switch is
    /// about type and size only: the master switch and the sites to leave
    /// alone, both the app's, still hold.
    pub fn takes(&self, download: &Candidate<'_>, every_type: bool) -> Result<(), Left> {
        if !self.enabled {
            return Err(Left::Off);
        }

        // Where it is served from, where it was asked for, and the page it
        // came from: any of them on the list leaves it with the browser.
        let skipped = listed(
            &self.skip_hosts,
            &[download.url, download.asked_url, download.referrer],
        );
        if skipped {
            return Err(Left::SkippedHost);
        }
        if every_type {
            return Ok(());
        }

        if self.min_size_kb > 0
            && download
                .size
                .is_some_and(|size| size < self.min_size_kb.saturating_mul(1024))
        {
            return Err(Left::TooSmall);
        }

        let wanted: Vec<String> = self
            .extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        // The name the browser chose, then the URL's, then what the server
        // said it was sending: a `download?id=5` that is a PDF is still a PDF.
        let kinds = [
            extension_of(base_name(download.filename)),
            extension_of(base_name(path_of(download.url))),
            extension_for_mime(download.mime).map(str::to_string),
        ];
        if kinds.iter().flatten().any(|kind| wanted.contains(kind)) {
            Ok(())
        } else {
            Err(Left::NotAFileType)
        }
    }
}

/// The host of a URL, lower-cased, without credentials or port. Empty when
/// there is none.
pub fn host_of(url: &str) -> String {
    let Some((_, rest)) = url.split_once("://") else {
        return String::new();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The path of a URL, without query or fragment.
fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    rest.find('/').map_or("", |at| &rest[at..])
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or("")
}

/// A file name's type, lower-cased, if it has one that looks like a type.
pub fn extension_of(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() || ext.len() > 9 {
        return None;
    }
    ext.chars()
        .all(|c| c.is_ascii_alphanumeric())
        .then(|| ext.to_ascii_lowercase())
}

/// The usual file type for what a server says it is sending.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let mime = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match mime.as_str() {
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/vnd.rar" | "application/x-rar-compressed" => "rar",
        "application/x-7z-compressed" => "7z",
        "application/gzip" | "application/x-gzip" => "gz",
        "application/x-tar" => "tar",
        "application/x-bzip2" => "bz2",
        "application/x-xz" => "xz",
        "application/zstd" => "zst",
        "application/pdf" => "pdf",
        "application/epub+zip" => "epub",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-msi" | "application/x-ms-installer" => "msi",
        "application/x-apple-diskimage" => "dmg",
        "application/vnd.debian.binary-package" => "deb",
        "application/x-rpm" => "rpm",
        "application/vnd.android.package-archive" => "apk",
        "application/x-iso9660-image" => "iso",
        "application/x-bittorrent" => "torrent",
        "video/mp4" => "mp4",
        "video/x-matroska" => "mkv",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/x-msvideo" => "avi",
        "audio/mpeg" => "mp3",
        "audio/flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/ogg" => "ogg",
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../../../test-support/secrets.rs"]
mod test_secrets;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(url: &str) -> Candidate<'_> {
        Candidate {
            url,
            ..Default::default()
        }
    }

    #[test]
    fn every_request_carries_the_version_and_reads_back() {
        for request in [
            Request::Hello,
            Request::Settings,
            Request::Start(Start {
                id: "b7".into(),
                url: "https://e.com/f.zip".into(),
                referrer: "https://e.com/".into(),
                caught: true,
                cookies: "session=1; theme=dark".into(),
                ..Start::default()
            }),
            Request::Start(Start {
                id: "m1".into(),
                url: "https://e.com/f.zip".into(),
                ..Default::default()
            }),
        ] {
            let frame = request.to_frame();
            assert_eq!(frame["protocolVersion"], request.needs());
            assert_eq!(Request::from_frame(&frame), Ok(request));
        }
    }

    #[test]
    fn the_wire_names_are_the_ones_the_host_has_always_used() {
        // An extension and a host of different builds meet in the field; the
        // strings are the contract, not the Rust names.
        assert_eq!(Request::Hello.to_frame()["type"], kind::HELLO);
        assert_eq!(Request::Settings.to_frame()["type"], kind::SETTINGS);
        let start = Request::Start(Start::default()).to_frame();
        assert_eq!(start["type"], kind::START);
        assert_eq!(
            Reply::Started { id: "a".into() }.to_frame()["type"],
            kind::STARTED
        );
        let failed = Reply::failed("a", "no").to_frame();
        assert_eq!(failed["type"], kind::FAILED);
        assert_eq!(failed["failure"]["message"], "no");
        let hello = Reply::Hello(Hello {
            engine_version: "1.2".into(),
            ytdlp: Some(false),
            protocol: None,
        })
        .to_frame();
        assert_eq!(hello["engineVersion"], "1.2");
        assert_eq!(hello["ytdlp"], false);
    }

    #[test]
    fn a_frame_of_another_version_is_not_read() {
        let frame = json!({ "protocolVersion": 3, "type": "engine.hello" });
        assert_eq!(
            Request::from_frame(&frame),
            Err(Unreadable::Version(Some(3)))
        );
        let frame = json!({ "protocolVersion": 0, "type": "engine.hello" });
        assert_eq!(
            Request::from_frame(&frame),
            Err(Unreadable::Version(Some(0)))
        );
        let frame = json!({ "type": "engine.hello" });
        assert_eq!(Request::from_frame(&frame), Err(Unreadable::Version(None)));
    }

    #[test]
    fn an_older_extensions_extra_keys_are_ignored_and_an_unknown_type_refused() {
        let frame = json!({
            "protocolVersion": 1, "type": "download.start", "id": "x",
            "url": "https://e.com/f", "filename": "f", "connections": 8, "via": "auto",
        });
        assert!(
            matches!(Request::from_frame(&frame), Ok(Request::Start(s)) if s.url == "https://e.com/f")
        );
        let Ok(Request::Start(older)) = Request::from_frame(&frame) else {
            unreachable!()
        };
        assert!(
            !older.caught,
            "an older extension's hand-off is not checked"
        );
        let pause = json!({ "protocolVersion": 1, "type": "download.pause", "id": "x" });
        assert!(matches!(
            Request::from_frame(&pause),
            Err(Unreadable::Shape(_))
        ));
    }

    #[test]
    fn a_caught_download_goes_out_at_the_version_that_judges_pages() {
        // Version 1 hosts answer only version 1 frames, and ignore `mime`:
        // a download the browser holds is sent where one cannot take it.
        let caught = Request::Start(Start {
            id: "b1".into(),
            url: "https://e.com/f.zip".into(),
            caught: true,
            held: true,
            mime: "application/zip".into(),
            ..Start::default()
        });
        assert_eq!(caught.to_frame()["protocolVersion"], JUDGES_PAGES_SINCE);
        // A file asked for from a menu or the panel is checked too, but the
        // browser holds nothing: a version-1 host must still answer it.
        let chosen = Request::Start(Start {
            id: "m1".into(),
            url: "https://e.com/f.zip".into(),
            caught: true,
            ..Start::default()
        });
        assert_eq!(chosen.to_frame()["protocolVersion"], 1);
        // Everything an older host reads correctly still reaches it.
        for old in [
            Request::Hello,
            Request::Settings,
            Request::Show,
            Request::Start(Start {
                id: "m1".into(),
                url: "https://e.com/watch".into(),
                ..Start::default()
            }),
        ] {
            assert_eq!(old.to_frame()["protocolVersion"], 1);
        }
        // Both versions are read here, and replies go out at 1, which an
        // older extension reads.
        for v in [1, 2] {
            let frame = json!({ "protocolVersion": v, "type": "engine.hello" });
            assert_eq!(Request::from_frame(&frame), Ok(Request::Hello));
        }
        let hello = Reply::Hello(Hello {
            protocol: Some(PROTOCOL_VERSION),
            ..Hello::default()
        })
        .to_frame();
        assert_eq!(hello["protocolVersion"], 1);
        assert_eq!(hello["protocol"], PROTOCOL_VERSION);
    }

    #[test]
    fn an_app_that_does_not_say_its_protocol_does_not_judge_pages() {
        let old = json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "2.0.0" });
        let Ok(Reply::Hello(hello)) = Reply::from_frame(&old) else {
            panic!("not read")
        };
        assert_eq!(hello.protocol(), 1);
        assert!(!hello.judges_pages());
        let new = json!({ "protocolVersion": 1, "type": "engine.hello", "protocol": 2 });
        let Ok(Reply::Hello(hello)) = Reply::from_frame(&new) else {
            panic!("not read")
        };
        assert!(hello.judges_pages());
    }

    #[test]
    fn a_hello_from_a_host_too_old_to_mention_ytdlp_says_unknown() {
        let frame = json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "0.1" });
        let Ok(Reply::Hello(hello)) = Reply::from_frame(&frame) else {
            panic!("not read")
        };
        assert_eq!(hello.ytdlp, None);
    }

    #[test]
    fn the_rules_cross_as_written_and_read_from_either_spelling() {
        let reply = Reply::Settings(SettingsReply {
            rules: CatchRules {
                enabled: false,
                extensions: vec!["zip".into()],
                min_size_kb: 5,
                skip_hosts: vec!["e.com".into()],
                keep_key: "ctrl".into(),
                take_key: String::new(),
                once_hosts: Vec::new(),
            },
        });
        let frame = reply.to_frame();
        assert_eq!(frame["catch"]["minSizeKb"], 5);
        assert_eq!(frame["catch"]["keepKey"], "ctrl");
        assert_eq!(frame["catch"]["takeKey"], "");
        assert_eq!(frame["catch"]["skipHosts"][0], "e.com");
        assert_eq!(Reply::from_frame(&frame), Ok(reply));

        // A hand-edited settings file may well use the file's own spelling.
        let typed: CatchRules =
            serde_json::from_value(json!({ "min_size_kb": 3, "skip_hosts": ["a.b"] })).unwrap();
        assert_eq!(typed.min_size_kb, 3);
        assert_eq!(typed.skip_hosts, vec!["a.b".to_string()]);
        assert!(typed.enabled, "a missing field takes its default");
        assert!(!typed.extensions.is_empty());
    }

    #[test]
    fn the_default_types_catch_files_not_pages() {
        let rules = CatchRules::default();
        for caught in [
            "https://e.com/setup.exe",
            "https://e.com/a/b/archive.ZIP?token=1",
            "https://e.com/film.mkv#t=3",
            "https://e.com/book.pdf",
            "https://e.com/linux.iso",
            "https://e.com/x.torrent",
        ] {
            assert_eq!(rules.catches(&file(caught)), Ok(()), "{caught}");
        }
        for left in [
            "https://e.com/",
            "https://e.com/page.html",
            "https://e.com/notes.txt",
            "https://e.com/data.json",
            "https://e.com/download?id=5",
        ] {
            assert_eq!(
                rules.catches(&file(left)),
                Err(Left::NotAFileType),
                "{left}"
            );
        }
    }

    #[test]
    fn the_browsers_name_and_the_servers_type_count_as_well_as_the_url() {
        let rules = CatchRules::default();
        let named = Candidate {
            url: "https://e.com/download?id=5",
            filename: "/Users/x/Downloads/report.pdf",
            ..Default::default()
        };
        assert_eq!(rules.catches(&named), Ok(()));
        let typed = Candidate {
            url: "https://e.com/download?id=5",
            mime: "application/zip; charset=binary",
            ..Default::default()
        };
        assert_eq!(rules.catches(&typed), Ok(()));
        let windows = Candidate {
            url: "https://e.com/get",
            filename: "C:\\Users\\x\\Downloads\\tool.msi",
            ..Default::default()
        };
        assert_eq!(rules.catches(&windows), Ok(()));
    }

    #[test]
    fn a_list_written_by_hand_is_read_generously() {
        let rules = CatchRules {
            extensions: vec![".BIN".into(), " Zip ".into(), String::new()],
            ..Default::default()
        };
        assert_eq!(rules.catches(&file("https://e.com/f.bin")), Ok(()));
        assert_eq!(rules.catches(&file("https://e.com/f.zip")), Ok(()));
        assert_eq!(
            rules.catches(&file("https://e.com/f.exe")),
            Err(Left::NotAFileType)
        );
    }

    #[test]
    fn an_empty_list_catches_every_type() {
        let rules = CatchRules {
            extensions: vec![],
            ..Default::default()
        };
        assert_eq!(rules.catches(&file("https://e.com/anything")), Ok(()));
    }

    #[test]
    fn turned_off_catches_nothing() {
        let rules = CatchRules {
            enabled: false,
            ..Default::default()
        };
        assert_eq!(rules.catches(&file("https://e.com/f.zip")), Err(Left::Off));
    }

    #[test]
    fn a_site_whose_links_work_once_is_known_by_its_host_or_the_page() {
        let rules = CatchRules {
            once_hosts: vec!["*.files.example".into(), "Once.example".into()],
            ..CatchRules::default()
        };
        assert!(rules.works_once("https://dl.files.example/a.zip", ""));
        assert!(rules.works_once("https://cdn.other/a.zip", "https://once.example/page"));
        assert!(!rules.works_once("https://notfiles.example/a.zip", ""));
        assert!(!CatchRules::default().works_once("https://once.example/a", ""));
        // An older settings file without the list still reads.
        let old: CatchRules =
            serde_json::from_value(serde_json::json!({ "enabled": true })).unwrap();
        assert!(old.once_hosts.is_empty());
    }

    #[test]
    fn a_skipped_host_covers_its_subdomains_and_nothing_that_merely_ends_alike() {
        let rules = CatchRules {
            skip_hosts: vec!["Example.com".into(), "*.cdn.net".into()],
            ..Default::default()
        };
        let signed_in = format!(
            "https://user:{}@EXAMPLE.com:8443/f.zip",
            test_secrets::secret("password")
        );
        for left in [
            "https://example.com/f.zip",
            "https://dl.example.com/f.zip",
            signed_in.as_str(),
            "https://a.cdn.net/f.zip",
        ] {
            assert_eq!(rules.catches(&file(left)), Err(Left::SkippedHost), "{left}");
        }
        for caught in ["https://notexample.com/f.zip", "https://example.org/f.zip"] {
            assert_eq!(rules.catches(&file(caught)), Ok(()), "{caught}");
        }
    }

    #[test]
    fn a_size_floor_leaves_small_files_but_not_unknown_ones() {
        let rules = CatchRules {
            min_size_kb: 100,
            ..Default::default()
        };
        let sized = |size| Candidate {
            url: "https://e.com/f.zip",
            size,
            ..Default::default()
        };
        assert_eq!(rules.catches(&sized(Some(99 * 1024))), Err(Left::TooSmall));
        assert_eq!(rules.catches(&sized(Some(100 * 1024))), Ok(()));
        assert_eq!(rules.catches(&sized(None)), Ok(()));
    }

    #[test]
    fn hosts_are_read_out_of_every_shape_of_url() {
        assert_eq!(host_of("https://A.B.com/x"), "a.b.com");
        assert_eq!(host_of(concat!("http://u:", "p", "@h.com:80?x")), "h.com");
        assert_eq!(host_of("http://[::1]:8080/x"), "::1");
        assert_eq!(host_of("https://h.com./"), "h.com");
        assert_eq!(host_of("not a url"), "");
    }

    /* Chrome names an unpacked extension that carries a `key` by the key:
     * the first 32 hex digits of its SHA-256, each digit moved into a–p. The
     * host registration assumes that id, so a key replaced without the
     * constant would leave the unpacked extension talking to nothing. */
    #[test]
    fn the_manifest_key_yields_the_id_the_host_is_registered_for() {
        use sha2::{Digest, Sha256};

        let manifest: Value =
            serde_json::from_str(include_str!("../../../../extension/manifest.json")).unwrap();
        let key = manifest["key"].as_str().expect("the manifest has no key");

        let mut bits = 0u32;
        let mut held = 0;
        let mut der = Vec::new();
        for c in key.bytes().filter(|c| *c != b'=') {
            let six = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                other => panic!("{} is not base64", other as char),
            };
            bits = (bits << 6) | six as u32;
            held += 6;
            if held >= 8 {
                held -= 8;
                der.push((bits >> held) as u8);
            }
        }

        let id: String = Sha256::digest(&der)[..16]
            .iter()
            .flat_map(|b| [b >> 4, b & 0xf])
            .map(|n| (b'a' + n) as char)
            .collect();
        assert_eq!(id, CHROME_EXTENSION_ID);
    }

    #[test]
    fn a_site_on_the_skip_list_keeps_downloads_served_from_elsewhere_and_its_pages() {
        let rules = CatchRules {
            skip_hosts: vec!["github.com".into()],
            ..Default::default()
        };
        // Asked for on the site, redirected to its file host.
        let redirected = Candidate {
            url: "https://objects.githubusercontent.com/f.zip",
            asked_url: "https://github.com/o/r/releases/download/v1/f.zip",
            ..Default::default()
        };
        assert_eq!(rules.catches(&redirected), Err(Left::SkippedHost));
        // Started from one of the site's pages.
        let from_page = Candidate {
            url: "https://cdn.example/f.zip",
            referrer: "https://github.com/o/r/releases",
            ..Default::default()
        };
        assert_eq!(rules.catches(&from_page), Err(Left::SkippedHost));
        let elsewhere = Candidate {
            url: "https://cdn.example/f.zip",
            asked_url: "https://cdn.example/f.zip",
            referrer: "https://example.org/",
            ..Default::default()
        };
        assert_eq!(rules.catches(&elsewhere), Ok(()));
    }

    #[test]
    fn taking_every_download_from_a_site_overrides_type_and_size_only() {
        let page = Candidate {
            url: "https://e.com/notes.txt",
            size: Some(10),
            ..Default::default()
        };
        let rules = CatchRules {
            min_size_kb: 100,
            ..Default::default()
        };
        assert!(rules.catches(&page).is_err());
        assert_eq!(rules.takes(&page, true), Ok(()));
        // The app's own master switch and skip list still stand.
        let off = CatchRules {
            enabled: false,
            ..Default::default()
        };
        assert_eq!(off.takes(&page, true), Err(Left::Off));
        let skipping = CatchRules {
            skip_hosts: vec!["e.com".into()],
            ..Default::default()
        };
        assert_eq!(skipping.takes(&page, true), Err(Left::SkippedHost));
    }

    #[test]
    fn a_page_served_to_dm_for_a_file_served_to_the_browser_is_told_apart() {
        let html = "text/html; charset=utf-8";
        // The browser was getting a zip; dm, without its cookies, a login page.
        assert!(page_instead_of_file(
            "application/zip",
            "https://e.com/get?id=1",
            html
        ));
        assert!(page_instead_of_file(
            "",
            "https://e.com/files/report.pdf",
            html
        ));
        assert!(page_instead_of_file(
            "application/octet-stream",
            "https://e.com/x",
            "application/xhtml+xml"
        ));
        // A page the browser was saving as a page, the same thing to dm.
        assert!(!page_instead_of_file(
            "text/html",
            "https://e.com/f.zip",
            html
        ));
        // Nothing said and nothing in the name: no way to tell, so taken.
        assert!(!page_instead_of_file(
            "",
            "https://e.com/download?id=5",
            html
        ));
        assert!(!page_instead_of_file(
            "",
            "https://e.com/get.php?id=5",
            html
        ));
        // A file served as a file.
        assert!(!page_instead_of_file(
            "application/zip",
            "https://e.com/f.zip",
            "application/zip"
        ));
        assert!(!page_instead_of_file("", "https://e.com/f.zip", ""));
    }

    #[test]
    fn a_hand_off_is_given_only_the_time_the_extension_still_waits() {
        // An older extension says nothing: the probe's own limit.
        assert_eq!(time_left(5_000, None), Some(PROBE_WITHIN_MS));
        // Plenty left: the probe's limit, no more.
        assert_eq!(time_left(0, Some(60_000)), Some(PROBE_WITHIN_MS));
        // Little left: what remains short of the margin.
        assert_eq!(time_left(10_000, Some(15_000)), Some(3_000));
        // Within the margin or past it: too late to take.
        assert_eq!(time_left(10_000, Some(10_000 + ANSWER_MARGIN_MS)), None);
        assert_eq!(time_left(30_000, Some(20_000)), None);
    }

    #[test]
    fn a_hand_offs_type_and_deadline_cross_and_an_older_one_has_neither() {
        let start = Request::Start(Start {
            id: "b1".into(),
            url: "https://e.com/f".into(),
            mime: "application/zip".into(),
            deadline: Some(1_700_000_000_000),
            ..Start::default()
        });
        let frame = start.to_frame();
        assert_eq!(frame["mime"], "application/zip");
        assert_eq!(frame["deadline"], 1_700_000_000_000u64);
        assert_eq!(Request::from_frame(&frame), Ok(start));
        let older =
            json!({ "protocolVersion": 1, "type": "download.start", "id": "x", "url": "u" });
        let Ok(Request::Start(older)) = Request::from_frame(&older) else {
            panic!("not read")
        };
        assert_eq!((older.mime.as_str(), older.deadline), ("", None));
    }

    #[test]
    fn a_dot_in_a_directory_or_a_hidden_file_is_not_a_type() {
        assert_eq!(extension_of("archive.tar.gz"), Some("gz".into()));
        assert_eq!(extension_of(".bashrc"), None);
        assert_eq!(extension_of("v1.0"), Some("0".into()));
        assert_eq!(extension_of("noext"), None);
        assert_eq!(extension_of("weird.a-b"), None);
        assert_eq!(
            CatchRules::default().catches(&file("https://e.com/v1.2/download")),
            Err(Left::NotAFileType)
        );
    }
}
