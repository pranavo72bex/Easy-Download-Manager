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
    pub url: String,
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
                .filter(|row| dm_core::queue::accepted(&row.url))
                .collect();
        }
    }
    text.split_whitespace()
        .map(link_in)
        .filter(|word| dm_core::queue::accepted(word))
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
                if dm_core::queue::accepted(&link) && !found.contains(&link) {
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
    pub page: String,
    /// File types wanted, without dots; empty takes every file.
    pub types: Vec<String>,
    /// Pages past the start page to read, 0 to [`MAX_DEPTH`]. Only pages on
    /// the start page's site are read.
    pub depth: u8,
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
            include: Vec::new(),
            exclude: Vec::new(),
            every_hours: 0,
            stop_after_minutes: 0,
            last_run: 0,
            seen: Vec::new(),
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
    pub files: Vec<String>,
    /// How many pages were read.
    pub pages: usize,
    /// Whether the run stopped at its time limit with pages left to read.
    pub stopped: bool,
}

/// A link's path, for the filters: lower-cased, decoded.
fn path_of(link: &str) -> String {
    url::Url::parse(link)
        .map(|u| dm_core::probe::decode(u.path()).to_ascii_lowercase())
        .unwrap_or_default()
}

fn has_any(path: &str, words: &[String]) -> bool {
    words
        .iter()
        .map(|w| w.trim().to_ascii_lowercase())
        .filter(|w| !w.is_empty())
        .any(|w| path.contains(&w))
}

/// Whether a link is one more page of the site rather than a file: a
/// folder, a bare name, or one of the extensions pages are served with.
pub fn looks_like_page(link: &str) -> bool {
    let path = path_of(link);
    if path.is_empty() || path.ends_with('/') {
        return true;
    }
    let last = path.rsplit('/').next().unwrap_or_default();
    match last.rsplit_once('.') {
        None => true,
        Some((_, ext)) => matches!(
            ext,
            "html" | "htm" | "xhtml" | "shtml" | "php" | "asp" | "aspx" | "jsp" | "cgi" | "do"
        ),
    }
}

fn same_site(a: &url::Url, link: &str) -> bool {
    url::Url::parse(link).ok().is_some_and(|b| {
        a.host_str().map(str::to_ascii_lowercase) == b.host_str().map(str::to_ascii_lowercase)
    })
}

/// One page, as text: its final address and its HTML. A file is refused as
/// a page, whatever it is called.
async fn fetch_page(
    client: &dm_core::reqwest::Client,
    page: &str,
    start: bool,
) -> Result<(String, String), String> {
    let res = dm_core::redirects::send(client.get(page))
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("the page answered {}", res.status()));
    }
    let kind = res
        .headers()
        .get(dm_core::reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| dm_core::probe::mime_essence(Some(v)))
        .unwrap_or_default();
    // Past the start page, an answer that does not say it is a page is not
    // read as one: a bare `/download/123` is usually the file itself.
    let html = kind == "text/html" || kind == "application/xhtml+xml";
    if !(html || (start && kind.is_empty())) {
        return Err(format!("{page} is a file ({kind}), not a page of links"));
    }
    let base = res.url().to_string();
    // A page, not a file: past a few megabytes this is not HTML to read,
    // and it is not read past them — not buffered whole and then cut.
    const CAP: usize = 8 * 1024 * 1024;
    let mut bytes = Vec::new();
    let mut body = res.bytes_stream();
    while let Some(chunk) = futures_util::StreamExt::next(&mut body).await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        let room = CAP - bytes.len();
        bytes.extend_from_slice(chunk.get(..room.min(chunk.len())).unwrap_or_default());
        if bytes.len() >= CAP {
            break;
        }
    }
    Ok((base, String::from_utf8_lossy(&bytes).into_owned()))
}

/// Reads the start page and, `depth` pages deep on the same site, the
/// pages it links to, and gathers the files wanted. Each page once, at most
/// [`MAX_PAGES`] of them. A start page that cannot be read is the error;
/// a page further in that cannot is skipped.
pub async fn crawl(client: &dm_core::reqwest::Client, search: &Search) -> Result<Crawled, String> {
    crawl_with(client, search, None).await
}

/// [`crawl`], with the engine's settings for an FTP server's folders: its
/// proxy, which a listing goes through as a download does. An FTP search
/// without them is refused, never sent around a proxy.
pub async fn crawl_with(
    client: &dm_core::reqwest::Client,
    search: &Search,
    options: Option<&dm_core::Options>,
) -> Result<Crawled, String> {
    let minutes = search.stop_after_minutes.min(MOST_MINUTES);
    let deadline = (minutes > 0)
        .then(|| tokio::time::Instant::now() + std::time::Duration::from_secs(minutes * 60));
    if dm_core::ftp::is_ftp(search.page.trim()) {
        let Some(options) = options else {
            return Err("an FTP server's folders are searched with dm's settings".into());
        };
        return crawl_ftp(search, options, deadline).await;
    }
    crawl_until(client, search, deadline).await
}

/// An FTP server's folders, from the start folder down `depth` levels:
/// each folder is a page, its files the links. Kept or skipped by the same
/// filters as a site's pages, and stopped at the same limits.
pub async fn crawl_ftp(
    search: &Search,
    options: &dm_core::Options,
    deadline: Option<tokio::time::Instant>,
) -> Result<Crawled, String> {
    walk_folders(search, deadline, |folder| async move {
        dm_core::ftp::list(&folder, options).await
    })
    .await
}

/// The walk itself, with what lists a folder given: dm's FTP client, or a
/// test's made-up server.
async fn walk_folders<F, Fut>(
    search: &Search,
    deadline: Option<tokio::time::Instant>,
    list: F,
) -> Result<Crawled, String>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<dm_core::ftp::Listed>, dm_core::Error>>,
{
    use std::collections::VecDeque;
    let mut start = url::Url::parse(search.page.trim())
        .map_err(|e| format!("that is not an FTP address: {e}"))?;
    if !start.path().ends_with('/') {
        let path = format!("{}/", start.path());
        start.set_path(&path);
    }
    let depth = search.depth.min(MAX_DEPTH);
    let mut found = Crawled::default();
    let mut frontier: VecDeque<(url::Url, u8)> = VecDeque::from([(start, 0)]);
    while let Some((folder, level)) = frontier.pop_front() {
        if found.pages >= MAX_PAGES {
            break;
        }
        let listing = list(folder.to_string());
        let listed = match deadline {
            Some(at) if found.pages > 0 => match tokio::time::timeout_at(at, listing).await {
                Ok(listed) => listed,
                Err(_) => {
                    found.stopped = true;
                    break;
                }
            },
            _ => listing.await,
        };
        let entries = match listed {
            Ok(entries) => entries,
            Err(e) if found.pages == 0 => return Err(e.to_string()),
            Err(_) => continue,
        };
        found.pages += 1;
        for entry in entries {
            let mut child = folder.clone();
            if let Ok(mut segments) = child.path_segments_mut() {
                segments.pop_if_empty().push(&entry.name);
                if entry.folder {
                    segments.push("");
                }
            }
            let link = child.to_string();
            let path = path_of(&link);
            if has_any(&path, &search.exclude) {
                continue;
            }
            if entry.folder {
                if level < depth {
                    frontier.push_back((child, level + 1));
                }
                continue;
            }
            let wanted_type = search.types.is_empty() || has_extension(&link, &search.types);
            let wanted = search.include.iter().all(|w| w.trim().is_empty())
                || has_any(&path, &search.include);
            if wanted_type && wanted && !found.files.contains(&link) {
                found.files.push(link);
            }
        }
    }
    Ok(found)
}

/// [`crawl`], stopping at `deadline` (after the start page) rather than at
/// the search's own time limit.
pub async fn crawl_until(
    client: &dm_core::reqwest::Client,
    search: &Search,
    deadline: Option<tokio::time::Instant>,
) -> Result<Crawled, String> {
    use std::collections::{HashSet, VecDeque};
    let start = url::Url::parse(search.page.trim())
        .map_err(|e| format!("that is not a page address: {e}"))?;
    let depth = search.depth.min(MAX_DEPTH);
    let mut found = Crawled::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut frontier: VecDeque<(String, u8)> = VecDeque::new();
    seen.insert(start.to_string());
    frontier.push_back((start.to_string(), 0));
    // The site is where the start page was typed and where it really is:
    // `example.com` that sends everyone on to `www.example.com` links only
    // to the second, and held to the first nothing past the start was read.
    let mut sites = vec![start.clone()];

    while let Some((page, level)) = frontier.pop_front() {
        if found.pages >= MAX_PAGES {
            break;
        }
        // Past the time it was given: what was found so far is the answer.
        // The start page is always read, however slow.
        let fetch = fetch_page(client, &page, found.pages == 0);
        let read = match deadline {
            Some(at) if found.pages > 0 => match tokio::time::timeout_at(at, fetch).await {
                Ok(read) => read,
                Err(_) => {
                    found.stopped = true;
                    break;
                }
            },
            _ => fetch.await,
        };
        let (base, html) = match read {
            Ok(read) => read,
            Err(e) if found.pages == 0 => return Err(e),
            Err(_) => continue,
        };
        if found.pages == 0 {
            if let Ok(landed) = url::Url::parse(&base) {
                seen.insert(landed.to_string());
                sites.push(landed);
            }
        }
        found.pages += 1;
        for link in links_on_page(&base, &html) {
            let path = path_of(&link);
            if has_any(&path, &search.exclude) {
                continue;
            }
            let is_file = if search.types.is_empty() {
                !looks_like_page(&link)
            } else {
                has_extension(&link, &search.types)
            };
            if is_file {
                let wanted = search.include.iter().all(|w| w.trim().is_empty())
                    || has_any(&path, &search.include);
                if wanted && !found.files.contains(&link) {
                    found.files.push(link);
                }
                continue;
            }
            if level < depth
                && looks_like_page(&link)
                && sites.iter().any(|site| same_site(site, &link))
                && seen.insert(link.clone())
            {
                frontier.push_back((link, level + 1));
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tracking_parameters_do_not_make_an_address_new() {
        let a = super::bare("https://v.example/watch?v=1&si=abc&utm_source=x");
        assert_eq!(a, super::bare("https://v.example/watch?v=1"));
        assert_ne!(a, super::bare("https://v.example/watch?v=2"));
        assert_eq!(
            super::bare("https://a.example/f.zip?fbclid=z"),
            super::bare("https://a.example/f.zip")
        );
    }

    use super::*;

    /// An FTP server's folders walked as a site's pages are: down to the
    /// depth asked, files kept by type and path, names with spaces made
    /// into addresses.
    #[tokio::test]
    async fn ftp_folders_are_walked_to_their_depth_with_the_filters() {
        use dm_core::ftp::Listed;
        let tree = |folder: String| async move {
            let entry = |name: &str, folder: bool| Listed {
                name: name.into(),
                folder,
            };
            Ok(match folder.as_str() {
                "ftp://host/pub/" => vec![
                    entry("a.zip", false),
                    entry("notes.txt", false),
                    entry("old stuff", true),
                    entry("skip", true),
                ],
                "ftp://host/pub/old%20stuff/" => {
                    vec![entry("b c.zip", false), entry("deeper", true)]
                }
                "ftp://host/pub/old%20stuff/deeper/" => vec![entry("d.zip", false)],
                "ftp://host/pub/skip/" => vec![entry("e.zip", false)],
                other => panic!("listed {other}"),
            })
        };
        let search = Search {
            page: "ftp://host/pub".into(),
            types: vec!["zip".into()],
            depth: 1,
            exclude: vec!["skip".into()],
            ..Search::default()
        };
        let found = walk_folders(&search, None, tree).await.unwrap();
        assert_eq!(
            found.files,
            [
                "ftp://host/pub/a.zip",
                "ftp://host/pub/old%20stuff/b%20c.zip"
            ]
        );
        assert_eq!(found.pages, 2);
    }

    /// A last run in the future (a clock once set ahead)
    /// held the search until that date; an interval read from a hand-edited
    /// file overflowed when turned into seconds.
    #[test]
    fn a_saved_search_is_not_held_by_a_future_clock_or_a_huge_interval() {
        let now = 1_800_000_000;
        let every_day = Search {
            every_hours: 24,
            last_run: now - 3600,
            ..Search::default()
        };
        assert!(!every_day.due(now), "ran an hour ago");
        assert!(every_day.due(now + 23 * 3600), "a day since");
        let from_the_future = Search {
            last_run: now + 40 * 365 * 24 * 3600,
            ..every_day.clone()
        };
        assert!(from_the_future.due(now), "a future last run is stale");
        let huge = Search {
            every_hours: u64::MAX,
            last_run: now - MOST_HOURS * 3600,
            ..Search::default()
        };
        assert!(huge.due(now), "clamped to a month, not overflowed");
        assert!(!Search::default().due(now), "by hand only");
    }

    #[test]
    fn a_batch_fills_the_star_with_numbers_or_letters() {
        assert_eq!(
            series("https://e.com/img*.jpg", "8", "11", 2).unwrap(),
            [
                "https://e.com/img08.jpg",
                "https://e.com/img09.jpg",
                "https://e.com/img10.jpg",
                "https://e.com/img11.jpg"
            ]
        );
        assert_eq!(
            series("https://e.com/part-*.zip", "a", "c", 0).unwrap(),
            [
                "https://e.com/part-a.zip",
                "https://e.com/part-b.zip",
                "https://e.com/part-c.zip"
            ]
        );
        use SeriesError::*;
        assert_eq!(series("https://e.com/no-star", "1", "2", 0), Err(Star));
        assert_eq!(series("https://e.com/*/*", "1", "2", 0), Err(Star));
        assert_eq!(series("https://e.com/*", "5", "1", 0), Err(Backwards));
        assert_eq!(series("https://e.com/*", "a", "Z", 0), Err(Ends));
        // Letters backwards say so, and a typed zero pads.
        assert_eq!(series("https://e.com/*", "z", "a", 0), Err(Backwards));
        assert_eq!(
            series("https://e.com/f*", "09", "11", 0).unwrap(),
            [
                "https://e.com/f09",
                "https://e.com/f10",
                "https://e.com/f11"
            ]
        );
        assert_eq!(series("https://e.com/*", "0", "99999", 0), Err(TooMany));
    }

    #[test]
    fn a_list_reads_back_as_it_was_written() {
        let rows = vec![
            Listed {
                url: "https://e.com/a.zip".into(),
                referrer: "https://e.com/page".into(),
                queue: "Nightly".into(),
                note: "the big one".into(),
                name: "mine.zip".into(),
                checksum: format!("sha256:{}", "a".repeat(64)),
            },
            Listed {
                url: "magnet:?xt=urn:btih:abc".into(),
                ..Listed::default()
            },
        ];
        let written = to_list(&rows);
        assert!(written.contains("dm download list"));
        assert_eq!(from_list(&written), rows);
        assert_eq!(from_list(&to_list(&[])), []);
    }

    #[test]
    fn a_list_file_keeps_only_what_dm_can_fetch() {
        let file = r#"{"format":"dm download list","version":1,"downloads":[
            {"url":"https://e.com/a.zip","referrer":"https://e.com/"},
            {"url":"javascript:alert(1)"},{"url":"file:///etc/passwd"}]}"#;
        assert_eq!(
            from_list(file),
            [Listed {
                url: "https://e.com/a.zip".into(),
                referrer: "https://e.com/".into(),
                ..Listed::default()
            }]
        );
    }

    #[test]
    fn plain_text_with_links_reads_too() {
        let plain = "https://e.com/1.iso\n\nnot a link\nhttps://e.com/2.iso ftp://e.com/3.iso\n";
        assert_eq!(from_list(plain).len(), 3);
        assert!(from_list("").is_empty());
        // JSON that is not dm's list is read for its links, not as a list.
        assert!(from_list(r#"{"format":"other","downloads":[]}"#).is_empty());
    }

    #[test]
    fn a_link_in_prose_is_read_without_the_punctuation_around_it() {
        /* "see https://e.com/a.zip, then…" was queued as `a.zip,`, which
         * the server does not have. */
        let prose = "Get https://e.com/a.zip, then <https://e.com/b.zip>. \
            (\"https://e.com/c.zip\"); also https://e.com/d.zip) and \
            https://en.wikipedia.org/wiki/Rust_(programming_language). \
            https://e.com/q?x=[1]]";
        let urls: Vec<String> = from_list(prose).into_iter().map(|l| l.url).collect();
        assert_eq!(
            urls,
            [
                "https://e.com/a.zip",
                "https://e.com/b.zip",
                "https://e.com/c.zip",
                "https://e.com/d.zip",
                "https://en.wikipedia.org/wiki/Rust_(programming_language)",
                "https://e.com/q?x=[1]",
            ]
        );
    }

    #[test]
    fn a_list_written_before_rows_kept_more_than_an_address_still_reads() {
        let old = r#"{"format":"dm download list","version":1,"downloads":[
            {"url":"https://e.com/a.zip","referrer":"https://e.com/"}]}"#;
        let read = from_list(old);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].referrer, "https://e.com/");
        assert!(read[0].queue.is_empty() && read[0].name.is_empty());
    }

    #[test]
    fn the_site_file_finder_finds_links_and_files_on_a_page() {
        let page = r#"<a href="/files/a.zip">a</a> <A HREF='b.ZIP'>b</A>
            <img src="pic.png"> <a href=c.zip#x>c</a> <a href="mailto:x@e.com">m</a>
            <a href="/files/a.zip">again</a> <a href="?q=1&amp;r=2">q</a>"#;
        let found = links_on_page("https://e.com/dir/index.html", page);
        assert_eq!(
            found,
            [
                "https://e.com/files/a.zip",
                "https://e.com/dir/b.ZIP",
                "https://e.com/dir/c.zip",
                "https://e.com/dir/index.html?q=1&r=2",
                "https://e.com/dir/pic.png",
            ]
        );
        let zips: Vec<_> = found
            .iter()
            .filter(|l| has_extension(l, &["zip".into()]))
            .collect();
        assert_eq!(zips.len(), 3);
        assert!(has_extension("https://e.com/x", &[]));
    }
}
