// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Which downloads the app takes: the catch rules, their defaults and the menu's items, the
//! browsers named, and what a link's host, name and type are.

use super::*;

/* ------------------------------------------------------------ catching */

/// The file types EDM used to catch out of the box: archives, installers and
/// disk images, video, audio and documents — and `.torrent`. EDM now catches
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
    "odp", // the one kind only EDM has
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
    /// The key that, held while a link is clicked, hands its download to EDM
    /// whatever the types and size floor say; the switch and the sites to
    /// skip still hold. Empty for none.
    #[serde(alias = "take_key")]
    pub take_key: String,
    /// Hosts whose links work once: handed to EDM without EDM asking the
    /// server first, which spent such a link before EDM's own download could
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
    /// The browsers EDM takes no downloads from, by [`BROWSERS`]' names (reference A's list of
    /// the browsers it captures from, each ticked or not). Their menus and panel still hand
    /// things over when asked.
    #[serde(alias = "off_browsers", skip_serializing_if = "Vec::is_empty")]
    pub off_browsers: Vec<String>,
    /// The items of the browser's right-click menu EDM does not add, by [`MENU_ITEMS`]' ids
    /// (reference A lets its menu items be chosen).
    #[serde(alias = "hidden_menus", skip_serializing_if = "Vec::is_empty")]
    pub hidden_menus: Vec<String>,
}

/// The items EDM adds to the browser's right-click menu: id and words, as the extension makes
/// them and Options lists them.
pub const MENU_ITEMS: [(&str, &str); 4] = [
    ("edm-link", "Download link with EDM"),
    ("edm-media", "Download media with EDM"),
    ("edm-all-links", "Download all links with EDM"),
    ("edm-selected-links", "Download selected links with EDM"),
];

/// The browsers the extension tells apart, as Options lists them: what a Chromium browser
/// names itself in `navigator.userAgentData`, Firefox, and the rest of the Chromium family.
pub const BROWSERS: [&str; 7] = [
    "Microsoft Edge",
    "Google Chrome",
    "Firefox",
    "Brave",
    "Opera",
    "Vivaldi",
    "Chromium",
];

/// Which of [`BROWSERS`] this is, from the brands a Chromium browser names in
/// `navigator.userAgentData` (or a Firefox): the first one known, else "Chromium".
pub fn browser_named(brands: &[String], firefox: bool) -> &'static str {
    if firefox {
        return "Firefox";
    }
    BROWSERS
        .iter()
        .copied()
        .filter(|b| *b != "Firefox" && *b != "Chromium")
        .find(|known| {
            brands.iter().any(|brand| {
                brand.eq_ignore_ascii_case(known)
                    // Opera names itself "Opera" or "Opera GX".
                    || (*known == "Opera" && brand.to_ascii_lowercase().starts_with("opera"))
            })
        })
        .unwrap_or("Chromium")
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
    /// chosen, and becomes every type (an empty list), which EDM now catches
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
            off_browsers: Vec::new(),
            hidden_menus: Vec::new(),
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
            Left::Off => "catching is turned off in EDM's settings",
            Left::SkippedHost => "that site is on EDM's list of sites to leave alone",
            Left::TooSmall => "smaller than EDM's minimum size to catch",
            Left::NotAFileType => "not one of the file types EDM catches",
        }
    }
}

impl CatchRules {
    /// Whether a link from `url`, met on `referrer`, is one of a site whose
    /// links work once, so EDM takes it without asking the server first.
    pub fn works_once(&self, url: &str, referrer: &str) -> bool {
        listed(&self.once_hosts, &[url, referrer])
    }

    /// Whether the browser's right-click menu has EDM's item `id` (one of [`MENU_ITEMS`]).
    pub fn shows_menu(&self, id: &str) -> bool {
        !self.hidden_menus.iter().any(|hidden| hidden.trim() == id)
    }

    /// Whether EDM takes downloads from `browser` (one of [`BROWSERS`]) at all.
    pub fn takes_from(&self, browser: &str) -> bool {
        !self
            .off_browsers
            .iter()
            .any(|off| off.trim().eq_ignore_ascii_case(browser))
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
pub(super) fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    rest.find('/').map_or("", |at| &rest[at..])
}

pub(super) fn base_name(path: &str) -> &str {
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
