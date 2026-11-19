// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Lists of downloads, the ways dm takes them in and gives them out: a
//! numbered series made from one address with a wildcard, dm's own
//! `.dmlist` file for export and import, and the links on a page for
//! "Site files".

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
/// Why a numbered series cannot be made: a kind, so the window can say
/// it in the language in force. `Display` is the English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesError {
    /// The address has no `*`, or more than one.
    Star,
    /// From is after To.
    Backwards,
    /// Ten thousand addresses or more.
    TooMany,
    /// From and To are not both numbers, or both letters of one case.
    Ends,
}

impl SeriesError {
    /// The English sentence, which is also the key it is translated by.
    pub fn english(self) -> &'static str {
        match self {
            SeriesError::Star => "The address needs exactly one * to fill in.",
            SeriesError::Backwards => "The range runs backwards.",
            SeriesError::TooMany => "Ten thousand addresses is more than a batch.",
            SeriesError::Ends => "From and To must both be numbers, or both letters of one case.",
        }
    }
}

impl std::fmt::Display for SeriesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.english())
    }
}

/// "Add numbered series": one address with a `*` standing for a number
/// or a letter, filled in from `from` to `to`.
///
/// Numbers can be padded to `width` digits (`file*.jpg`, 1 to 12, width 2 →
/// `file01.jpg` … `file12.jpg`); letters run `a` to `z` or `A` to `Z`.
pub fn series(
    template: &str,
    from: &str,
    to: &str,
    width: usize,
) -> Result<Vec<String>, SeriesError> {
    let template = template.trim();
    if template.matches('*').count() != 1 {
        return Err(SeriesError::Star);
    }
    let fill = |part: String| template.replacen('*', &part, 1);
    let (from, to) = (from.trim(), to.trim());

    if let (Ok(a), Ok(b)) = (from.parse::<u64>(), to.parse::<u64>()) {
        if a > b {
            return Err(SeriesError::Backwards);
        }
        if b - a >= 10_000 {
            return Err(SeriesError::TooMany);
        }
        // A From typed with its zeros ("09") is padded as typed: the zero
        // was dropped and "file9" made.
        let width = if from.len() > 1 && from.starts_with('0') {
            width.max(from.len())
        } else {
            width
        };
        return Ok((a..=b).map(|n| fill(format!("{n:0width$}"))).collect());
    }

    let letter = |s: &str| {
        let mut chars = s.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphabetic() => Some(c),
            _ => None,
        }
    };
    match (letter(from), letter(to)) {
        (Some(a), Some(b)) if a.is_ascii_lowercase() == b.is_ascii_lowercase() => {
            // z to a runs backwards, as 9 to 1 does: it was said to be
            // neither numbers nor letters.
            if a > b {
                return Err(SeriesError::Backwards);
            }
            Ok((a..=b).map(|c| fill(c.to_string())).collect())
        }
        _ => Err(SeriesError::Ends),
    }
}

/// One download in a saved list: an address and the page it came from,
/// and what a person gave the row — its queue, note, name and digest — so
/// a list moved to another computer is the same list there.
///
/// Each beyond the address may be missing, as it is from a list written
/// before they were kept, and is then nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Listed {
    /// The address to download, without a login (Options › Sign-ins puts it
    /// back on import).
    pub url: String,
    /// The page it was found on, sent as the referrer; empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub referrer: String,
    /// The named queue it waits in; empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub queue: String,
    /// The Note column.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The name a person chose over the server's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The digest it is held to, as `sha256:…`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checksum: String,
}

/// dm's own list file (`.dmlist`): JSON, saying what it is, so a file of
/// something else is not read as one.
#[derive(serde::Serialize, serde::Deserialize)]
struct ListFile {
    format: String,
    version: u32,
    downloads: Vec<Listed>,
}

const LIST_FORMAT: &str = "dm download list";

/// Writes downloads as a `.dmlist` file.
pub fn to_list(rows: &[Listed]) -> String {
    let file = ListFile {
        format: LIST_FORMAT.into(),
        version: 1,
        downloads: rows.to_vec(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Reads a `.dmlist` file, or any text with links in it, one or more a
/// line — what people more often have. Anything that is not a link dm can
/// fetch is skipped.
pub fn from_list(text: &str) -> Vec<Listed> {
    if let Ok(file) = serde_json::from_str::<ListFile>(text) {
        if file.format == LIST_FORMAT {
            return file
                .downloads
                .into_iter()
                .filter(|row| dm_core::accepted(&row.url))
                .collect();
        }
    }
    text.split_whitespace()
        .map(link_in)
        .filter(|word| dm_core::accepted(word))
        .map(|word| Listed {
            url: word.to_string(),
            ..Listed::default()
        })
        .collect()
}

/// A link as it sits in prose, without what the prose put around it: the
/// `<…>` it was wrapped in, the quotes or brackets before it, and the
/// comma, full stop or closing bracket after it. A closing bracket the link
/// opened is its own and kept — `…/Rust_(programming_language)`.
fn link_in(word: &str) -> &str {
    let mut link = word.trim_start_matches(['<', '(', '[', '"', '\'']);
    loop {
        let Some(last) = link.chars().last() else {
            return link;
        };
        let opened = |open: char| link.matches(open).count() >= link.matches(last).count();
        let stray = match last {
            ',' | ';' | '.' | '>' | '\'' | '"' | ':' | '!' | '?' => true,
            ')' => !opened('('),
            ']' => !opened('['),
            _ => false,
        };
        if !stray {
            return link;
        }
        link = link.strip_suffix(last).unwrap_or_default();
    }
}

/// Every link and embedded file on a page, resolved against its address,
/// each once, every `href` in page order and then every `src`, and only
/// what dm can fetch.
pub fn links_on_page(base: &str, html: &str) -> Vec<String> {
    let Ok(base) = url::Url::parse(base) else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    let lower = html.to_ascii_lowercase();
    for attribute in ["href", "src"] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(attribute) {
            let start = from + at + attribute.len();
            from = start;
            let rest = html[start..].trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            let rest = rest.trim_start();
            let value = match rest.chars().next() {
                Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
                _ => rest
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or(""),
            };
            let value = value.replace("&amp;", "&");
            if let Ok(link) = base.join(value.trim()) {
                let mut link = link;
                link.set_fragment(None);
                let link = link.to_string();
                if dm_core::accepted(&link) && !found.contains(&link) {
                    found.push(link);
                }
            }
        }
    }
    found
}

/// Whether a link's file has one of `extensions` (without dots, any case).
/// An empty list keeps everything.
pub fn has_extension(link: &str, extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return true;
    }
    let path = url::Url::parse(link)
        .map(|u| u.path().to_string())
        .unwrap_or_default();
    let ext = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    extensions
        .iter()
        .any(|want| want.trim_start_matches('.').eq_ignore_ascii_case(&ext))
}

/// "Site files", one page deep: the page fetched, and the links on it
/// with the wanted extensions.
pub async fn grab(page: &str, extensions: &[String]) -> Result<Vec<String>, String> {
    let search = Search {
        page: page.to_string(),
        types: extensions.to_vec(),
        depth: 0,
        ..Search::default()
    };
    crawl(&dm_core::client(), &search).await.map(|c| c.files)
}

/// What "Site files" looks for, and how far. Saved under a name,
/// it is run again by hand or every so many hours, and what it finds that
/// is not already in the list is added for later.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Search {
    /// Its name once saved; empty for one run once.
    pub name: String,
    /// The page the search starts from.
    pub page: String,
    /// File types wanted, without dots; empty takes every file.
    pub types: Vec<String>,
    /// Pages past the start page to read, 0 to [`MAX_DEPTH`]. Only pages on
    /// the start page's site are read.
    pub depth: u8,
    /// Whether pages on other sites than the start page's are read too, to
    /// the same depth. Off: only the start page's own site.
    pub other_sites: bool,
    /// Whether each file is saved in folders that mirror its address
    /// (`site/folder/subfolder`) instead of the one download folder.
    pub keep_layout: bool,
    /// Only files whose path has one of these in it; empty is any.
    pub include: Vec<String>,
    /// Files and pages whose path has one of these in it are skipped.
    pub exclude: Vec<String>,
    /// Hours between runs on its own; 0 is only by hand.
    pub every_hours: u64,
    /// Minutes a run reads pages for before it stops with what it has
    /// found; 0 is no limit beyond [`MAX_PAGES`]. A site slow to answer
    /// held a run for as long as its hundred pages took.
    pub stop_after_minutes: u64,
    /// When it last ran, in seconds since 1970; 0 is never.
    pub last_run: u64,
    /// What it has found before, without logins: added once, not again
    /// after the list is cleared or a login changed the row's address.
    pub seen: Vec<String>,
    /// Files smaller than this many KB are left out; 0 is no floor. A file
    /// whose size the server does not say is kept.
    pub min_kb: u64,
    /// Files larger than this many KB are left out; 0 is no ceiling.
    pub max_kb: u64,
}

/// The folders a file is saved in when a search keeps the site's layout: its
/// site, then the folders of its address, each made safe as a file name is.
/// `None` for an address that is not a web or FTP one.
pub fn layout_folder(file: &str) -> Option<String> {
    let parsed = url::Url::parse(file).ok()?;
    if !matches!(parsed.scheme(), "http" | "https" | "ftp") {
        return None;
    }
    let site = match parsed.port() {
        Some(port) => format!("{}_{port}", parsed.host_str()?),
        None => parsed.host_str()?.to_string(),
    };
    let mut parts = vec![dm_core::sanitize_name(&site.replace(':', "_"))];
    let mut segments: Vec<&str> = parsed.path_segments()?.collect();
    segments.pop();
    for segment in segments {
        let name = dm_core::sanitize_name(&dm_core::decode_percent(segment));
        if !name.is_empty() && name != "." && name != ".." {
            parts.push(name);
        }
    }
    Some(parts.join("/"))
}

/// The most a saved search remembers having found.
pub const MOST_SEEN: usize = 10_000;

/// Query keys a site or a share button adds to say where a visitor came
/// from; they never change what is served.
fn is_tracking(key: &str) -> bool {
    key.starts_with("utm_") || matches!(key, "fbclid" | "gclid" | "igshid" | "si" | "feature")
}

/// An address without its user and password, nor the parameters that only
/// track a visit, for telling two apart.
pub fn bare(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            if parsed.query().is_some() {
                let kept: Vec<(String, String)> = parsed
                    .query_pairs()
                    .filter(|(k, _)| !is_tracking(k))
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                if kept.is_empty() {
                    parsed.set_query(None);
                } else {
                    parsed.query_pairs_mut().clear().extend_pairs(kept);
                }
            }
            parsed.to_string()
        }
        Err(_) => url.to_string(),
    }
}

impl Default for Search {
    fn default() -> Self {
        Search {
            name: String::new(),
            page: String::new(),
            types: DEFAULT_TYPES.split(' ').map(str::to_string).collect(),
            depth: 0,
            other_sites: false,
            keep_layout: false,
            include: Vec::new(),
            exclude: Vec::new(),
            every_hours: 0,
            stop_after_minutes: 0,
            last_run: 0,
            seen: Vec::new(),
            min_kb: 0,
            max_kb: 0,
        }
    }
}

/// The longest a run may be given to read pages: a day.
pub const MOST_MINUTES: u64 = 24 * 60;

/// The longest a saved search waits between runs on its own: a month, as
/// the dialog offers. A settings file can say anything, and an unbounded
/// `every_hours` times 3600 overflowed.
pub const MOST_HOURS: u64 = 24 * 31;

impl Search {
    /// Whether a saved search is due to run on its own at `now` (seconds
    /// since 1970). Its interval is clamped to [`MOST_HOURS`]; a last run
    /// later than now is a clock that was once wrong, and is stale rather
    /// than a reason to wait until that date comes round.
    pub fn due(&self, now: u64) -> bool {
        if self.every_hours == 0 {
            return false;
        }
        if self.last_run > now {
            return true;
        }
        now - self.last_run >= self.every_hours.min(MOST_HOURS) * 3600
    }
}

/// The file types a search starts with.
pub const DEFAULT_TYPES: &str = "zip rar 7z exe msi dmg iso mp4 mkv mp3 pdf jpg png";

/// How many pages past the start page a search can be asked to read.
pub const MAX_DEPTH: u8 = 5;

/// How many pages one search reads at most, however deep: a site is not
/// the Web, and a search that never ends is not a search.
pub const MAX_PAGES: usize = 100;

/// What a search found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Crawled {
    /// The files' addresses, each once, in the order they were found.
    pub files: Vec<String>,
    /// How many pages were read.
    pub pages: usize,
    /// Whether the run stopped at its time limit with pages left to read.
    pub stopped: bool,
}

/// A crawl as it goes, for the window to show and stop (gaps row U25): the pages read and the
/// files found so far, and whether Stop was pressed, which ends it keeping what it found.
#[derive(Debug, Default)]
pub struct CrawlWatch {
    /// Pages read so far.
    pub pages: std::sync::atomic::AtomicUsize,
    /// Files found so far.
    pub files: std::sync::atomic::AtomicUsize,
    /// Set by Stop.
    pub stop: std::sync::atomic::AtomicBool,
}

mod crawl;
pub mod offline;

#[cfg(test)]
use crawl::{bare_of_login, walk_folders};
pub use crawl::{
    crawl, crawl_ftp, crawl_until, crawl_watched, crawl_with, looks_like_page, within_sizes,
};
use crawl::{has_any, path_of, same_site, with_start_login};

/// A download's address as a window shows it: a password in it is replaced
/// by dots, the rest kept (`dm_core::shown_url`, beside `Entry::redacted`).
pub use dm_core::shown_url;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod more_tests;
