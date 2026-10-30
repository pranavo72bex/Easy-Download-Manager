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

// Every public item says what it means: the types are the wire format the
// extension and the app read (docs/gaps.md row 27).
#![warn(missing_docs)]
// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
// From Google's Rust style guide, checked in CI: an `unsafe` block says why it
// is sound (`// SAFETY:`; not on Windows, whose blocks are not all written up
// yet), and a string is built with `format!` rather than `+`, and a loop is a
// loop rather than `for_each`.
#![cfg_attr(not(windows), warn(clippy::undocumented_unsafe_blocks))]
#![warn(clippy::string_add, clippy::needless_for_each)]
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

/// The id Microsoft Edge Add-ons gave the extension's listing, which every
/// registration allows beside the unpacked build's.
pub const EDGE_EXTENSION_ID: &str = "ooafpfjpliofcdlnbdkjhonaoebcpnej";

/// The frame types, for code that has to name one without building it.
pub mod kind {
    /// [`Request::Hello`] and its answer, [`Reply::Hello`].
    pub const HELLO: &str = "engine.hello";
    /// [`Request::Settings`] and its answer, [`Reply::Settings`].
    pub const SETTINGS: &str = "settings.get";
    /// [`Request::Start`]: a download handed over.
    pub const START: &str = "download.start";
    /// [`Reply::Started`]: the app has the download.
    pub const STARTED: &str = "download.started";
    /// [`Reply::Failed`]: the app did not take it; the browser keeps it.
    pub const FAILED: &str = "download.failed";
    /// [`Request::Show`]: bring the app's window to the front.
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
    /// The extension's name for this hand-off, echoed in the answer.
    pub id: String,
    /// The page they are on, sent as Referer.
    pub page: String,
    /// The links, in the page's order; the app's dialog lets the person choose.
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
    /// The page whose video is listed.
    pub url: String,
    /// The page the video page was reached from, if any, sent as Referer.
    #[serde(default)]
    pub referrer: String,
    /// The page's site's cookies, as the `Cookie` header the browser would
    /// send: a video site checks them before it lists anything ("confirm
    /// you're not a bot"). Used for this one query and kept nowhere.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
}

/// One quality the panel offers: what the list says, the quality to fetch it
/// at ("audio", a height such as "720", or "best"), and its size in bytes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Choice {
    /// What the panel's list says, such as "1080p MP4".
    pub label: String,
    /// The quality to fetch it at: "audio", a height such as "720", or "best".
    pub quality: String,
    /// Its size in bytes, where yt-dlp's reading of the page says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// One download handed over.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Start {
    /// The extension's name for this hand-off, echoed in the answer. The host
    /// files the request under it, so it holds it to a plain name.
    pub id: String,
    /// The download's address, or the page's for a video page.
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
    /// A separate sound track for the picture at `url`, from a player that
    /// fetched them apart; dm fetches both and joins them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
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
    /// The cookies are for this download alone: a video page's session,
    /// handed over with every video so the site lets yt-dlp in, rather than
    /// a sign-in the person asked dm to keep. The row drops them once it
    /// finishes. False from an extension that sends only kept ones.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cookies_once: bool,
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
/// Inside the extension's wait for an answer (20 seconds), which also has
/// to cover the host's own start (measured at 50 ms) and the answer's
/// margin below. It was 10 s; a server slow to answer dm's first request
/// (12 s, in a check) left its download with the browser, every time.
pub const PROBE_WITHIN_MS: u64 = 16_000;

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
    /// The app is there: its version, and what it can do.
    #[serde(rename = "engine.hello")]
    Hello(Hello),
    /// The app's catch rules.
    #[serde(rename = "settings.get")]
    Settings(SettingsReply),
    /// The app has the download; the browser's copy can go.
    #[serde(rename = "download.started")]
    Started {
        /// The hand-off's [`Start::id`].
        id: String,
    },
    /// The app did not take the download; the browser keeps it.
    #[serde(rename = "download.failed")]
    Failed {
        /// The hand-off's [`Start::id`].
        id: String,
        /// Why, where the host can say.
        #[serde(default)]
        failure: Option<Failure>,
    },
    /// The qualities on offer, best first; empty with a `reason` when the
    /// page could not be read (no yt-dlp, an unsupported site).
    #[serde(rename = "media.formats")]
    Formats {
        /// The qualities, best first.
        #[serde(default)]
        choices: Vec<Choice>,
        /// Why there are none, in words for the panel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The window was asked to show itself; a `reason` when it could not be.
    #[serde(rename = "app.shown")]
    Shown {
        /// Why the window could not be shown, in words for the popup.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// The app's answer to [`Request::Hello`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Hello {
    /// The app's version, such as "0.8.4".
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

/// The app's answer to [`Request::Settings`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SettingsReply {
    /// Which downloads the app wants caught (Options › File types).
    #[serde(rename = "catch", default)]
    pub rules: CatchRules,
}

/// Why the app did not take a download.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Failure {
    /// The reason, in words a person reads in the extension's log.
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

    /// Reads a request frame, refusing one of a version this side does not read.
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

    /// Reads a reply frame, refusing one of a version this side does not read.
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

/// The file types dm used to catch out of the box: archives, installers and
/// disk images, video, audio and documents — and `.torrent`. dm now catches
/// every type unless a list is chosen; this is what an unchosen list from an
/// earlier edition holds, which is how one is told from a chosen one.
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    // archives
    "zip", "rar", "7z", "gz", "gzip", "tgz", "bz2", "xz", "zst", "tar", "z", "lzh", "arj", "cab",
    "lz", "lzma", "tbz2", "txz", // programs and disk images
    "exe", "msi", "msu", "dmg", "pkg", "deb", "rpm", "apk", "appimage", "iso", "img", "bin",
    "msix", "jar", "whl", "ipa", "vmdk", "vdi", "vhd", "vhdx", "ova", // video
    "mp4", "m4v", "mkv", "avi", "mov", "wmv", "flv", "webm", "mpg", "mpeg", "3gp", "ogv", "ts",
    "rmvb", "vob", "m2ts", "mts", "asf", // audio
    "mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "wma", "aif", "aiff",
    // documents
    "pdf", "epub", "mobi", "djvu", "ppt", "pptx", "pps", "doc", "docx", "xls", "xlsx", "odt", "ods",
    "odp", // the one kind only dm has
    "torrent",
];

/// The edition of the catch defaults now. 2: every type is caught, so a list
/// that is still the old defaults becomes empty.
pub const CATCH_DEFAULTS_VERSION: u32 = 2;

fn unversioned() -> u32 {
    0
}

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
    /// Which edition of [`DEFAULT_EXTENSIONS`] `extensions` has been brought
    /// up to. A saved list does not pick up the types added to the defaults
    /// since it was saved (documents, in one edition) unless the app adds
    /// them once; zero is a list saved before this field existed.
    #[serde(alias = "defaults_version", default = "unversioned")]
    pub defaults_version: u32,
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
    /// use it. Each covers its subdomains.
    #[serde(alias = "once_hosts")]
    pub once_hosts: Vec<String>,
    /// Whether the "Download video" panel shows over videos at all; the
    /// sites it is kept off are the extension's own list.
    #[serde(alias = "video_panel")]
    pub video_panel: bool,
    /// Every download caught from the browser carries the browser's cookies
    /// for its site, for that download alone, not only the sites switched
    /// on in the toolbar popup. Off unless chosen: a sign-in is the site's.
    #[serde(alias = "sign_in_everywhere")]
    pub sign_in_everywhere: bool,
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
        // An address saved whole names its host (one pasted
        // into "sites to skip" matched nothing).
        let entry = if entry.contains("://") {
            host_of(entry)
        } else {
            entry.trim().trim_start_matches("*.").to_ascii_lowercase()
        };
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

impl CatchRules {
    /// A list saved before some types joined the defaults gets them once: a
    /// person who never touched the list otherwise kept documents in the
    /// browser for good. One that is then exactly the old defaults was never
    /// chosen, and becomes every type (an empty list), which dm now catches
    /// out of the box; a list a person changed is left as it is.
    pub fn bring_up_to_date(&mut self) {
        if self.defaults_version >= CATCH_DEFAULTS_VERSION {
            return;
        }
        if !self.extensions.is_empty() {
            if self.defaults_version < 1 {
                for kind in DEFAULT_EXTENSIONS {
                    if !self.extensions.iter().any(|have| have == kind) {
                        self.extensions.push((*kind).to_string());
                    }
                }
            }
            let mut have: Vec<&str> = self.extensions.iter().map(String::as_str).collect();
            have.sort_unstable();
            have.dedup();
            let mut old: Vec<&str> = DEFAULT_EXTENSIONS.to_vec();
            old.sort_unstable();
            old.dedup();
            if have == old {
                self.extensions.clear();
            }
        }
        self.defaults_version = CATCH_DEFAULTS_VERSION;
    }
}

impl Default for CatchRules {
    fn default() -> Self {
        CatchRules {
            enabled: true,
            extensions: Vec::new(),
            defaults_version: CATCH_DEFAULTS_VERSION,
            min_size_kb: 0,
            skip_hosts: Vec::new(),
            keep_key: "alt".into(),
            take_key: "shift".into(),
            once_hosts: Vec::new(),
            video_panel: true,
            sign_in_everywhere: false,
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
    /// The content type the server gave, as the browser reports it.
    pub mime: &'a str,
    /// Its size in bytes, where the browser knows it.
    pub size: Option<u64>,
}

/// Why a download was left with the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// Catching is turned off.
    Off,
    /// The site is on the list of sites to leave alone.
    SkippedHost,
    /// Smaller than the minimum size to catch.
    TooSmall,
    /// Not one of the file types the app catches.
    NotAFileType,
}

impl Left {
    /// The reason, in words for the extension's log.
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
mod tests;
