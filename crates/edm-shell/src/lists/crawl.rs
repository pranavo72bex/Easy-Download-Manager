// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Site files" at work: reading a site's pages, or an FTP server's folders, for the
//! files a [`Search`] wants, and keeping those within its sizes.

use crate::Error;

use super::{
    has_extension, links_on_page, CrawlWatch, Crawled, Search, MAX_DEPTH, MAX_PAGES, MOST_MINUTES,
};

/// A link's path, for the filters: lower-cased, decoded.
pub(super) fn path_of(link: &str) -> String {
    url::Url::parse(link)
        .map(|u| edm_core::decode_percent(u.path()).to_ascii_lowercase())
        .unwrap_or_default()
}

pub(super) fn has_any(path: &str, words: &[String]) -> bool {
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

/// `page` with the login the start page was given, for a page on that same host: a
/// site that wants a sign-in wants it on every page of it, and never on another host.
pub(super) fn with_start_login(page: &str, start: &url::Url) -> String {
    if start.username().is_empty() {
        return page.to_string();
    }
    let Ok(mut parsed) = url::Url::parse(page) else {
        return page.to_string();
    };
    let same_place = parsed.host_str().map(str::to_ascii_lowercase)
        == start.host_str().map(str::to_ascii_lowercase)
        && parsed.port_or_known_default() == start.port_or_known_default()
        && parsed.scheme() == start.scheme();
    if !same_place || !parsed.username().is_empty() {
        return page.to_string();
    }
    let _ = parsed.set_username(start.username());
    let _ = parsed.set_password(start.password());
    parsed.to_string()
}

/// `address` without a user and password, for saying it in a message.
pub(super) fn bare_of_login(address: &str) -> String {
    match url::Url::parse(address) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        Err(_) => address.to_string(),
    }
}

pub(super) fn same_site(a: &url::Url, link: &str) -> bool {
    url::Url::parse(link).ok().is_some_and(|b| {
        a.host_str().map(str::to_ascii_lowercase) == b.host_str().map(str::to_ascii_lowercase)
    })
}

/// One page, as text: its final address and its HTML. A file is refused as
/// a page, whatever it is called.
async fn fetch_page(
    client: &edm_core::reqwest::Client,
    page: &str,
    ask: &str,
    start: bool,
) -> Result<(String, String), Error> {
    // `ask` may carry a login; `page` is what is said in a message, and it may too.
    let shown = bare_of_login(page);
    let res = edm_core::auth::send(client, client.get(ask), ask)
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("the page answered {}", res.status()).into());
    }
    let kind = res
        .headers()
        .get(edm_core::reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| edm_core::mime_essence(Some(v)))
        .unwrap_or_default();
    // Past the start page, an answer that does not say it is a page is not
    // read as one: a bare `/download/123` is usually the file itself.
    let html = kind == "text/html" || kind == "application/xhtml+xml";
    if !(html || (start && kind.is_empty())) {
        return Err(format!("{shown} is a file ({kind}), not a page of links").into());
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
pub async fn crawl(client: &edm_core::reqwest::Client, search: &Search) -> Result<Crawled, Error> {
    crawl_with(client, search, None).await
}

/// [`crawl`], with the engine's settings for an FTP server's folders: its
/// proxy, which a listing goes through as a download does. An FTP search
/// without them is refused, never sent around a proxy.
pub async fn crawl_with(
    client: &edm_core::reqwest::Client,
    search: &Search,
    options: Option<&edm_core::Options>,
) -> Result<Crawled, Error> {
    crawl_watched(client, search, options, &CrawlWatch::default()).await
}

/// [`crawl_with`], telling `watch` how far it has come and stopped by it (gaps row U25).
pub async fn crawl_watched(
    client: &edm_core::reqwest::Client,
    search: &Search,
    options: Option<&edm_core::Options>,
    watch: &CrawlWatch,
) -> Result<Crawled, Error> {
    let minutes = search.stop_after_minutes.min(MOST_MINUTES);
    let deadline = (minutes > 0)
        .then(|| tokio::time::Instant::now() + std::time::Duration::from_secs(minutes * 60));
    if edm_core::ftp::is_ftp(search.page.trim()) {
        let Some(options) = options else {
            return Err("an FTP server's folders are searched with EDM's settings".into());
        };
        return crawl_ftp(search, options, deadline).await;
    }
    let mut found = crawl_until_watched(client, search, deadline, watch).await?;
    found.files = within_sizes(client, found.files, search.min_kb, search.max_kb).await;
    Ok(found)
}

/// How long a server is given to answer for one file's size (`within_sizes`).
const SIZE_WITHIN: std::time::Duration = std::time::Duration::from_secs(15);

/// The files whose size is within `min_kb` and `max_kb` (0 is no limit at that end), each
/// asked for its size once, eight at a time; one whose size the server does not say is kept,
/// as reference A keeps it.
pub async fn within_sizes(
    client: &edm_core::reqwest::Client,
    files: Vec<String>,
    min_kb: u64,
    max_kb: u64,
) -> Vec<String> {
    use futures_util::StreamExt;
    if min_kb == 0 && max_kb == 0 {
        return files;
    }
    let sizes: Vec<(String, Option<u64>)> = futures_util::stream::iter(files)
        .map(|file| async move {
            let size = client
                .head(&file)
                .timeout(SIZE_WITHIN)
                .send()
                .await
                .ok()
                .filter(|r| r.status().is_success())
                .and_then(|r| {
                    r.headers()
                        .get(edm_core::reqwest::header::CONTENT_LENGTH)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                });
            (file, size)
        })
        .buffered(8)
        .collect()
        .await;
    sizes
        .into_iter()
        .filter(|(_, size)| {
            size.is_none_or(|bytes| {
                (min_kb == 0 || bytes >= min_kb.saturating_mul(edm_core::KB))
                    && (max_kb == 0 || bytes <= max_kb.saturating_mul(edm_core::KB))
            })
        })
        .map(|(file, _)| file)
        .collect()
}

/// An FTP server's folders, from the start folder down `depth` levels:
/// each folder is a page, its files the links. Kept or skipped by the same
/// filters as a site's pages, and stopped at the same limits.
pub async fn crawl_ftp(
    search: &Search,
    options: &edm_core::Options,
    deadline: Option<tokio::time::Instant>,
) -> Result<Crawled, Error> {
    walk_folders(search, deadline, |folder| async move {
        edm_core::ftp::list(&folder, options).await
    })
    .await
}

/// The walk itself, with what lists a folder given: EDM's FTP client, or a
/// test's made-up server.
pub(super) async fn walk_folders<F, Fut>(
    search: &Search,
    deadline: Option<tokio::time::Instant>,
    list: F,
) -> Result<Crawled, Error>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<edm_core::ftp::Listed>, edm_core::Error>>,
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
            Err(e) if found.pages == 0 => return Err(e.to_string().into()),
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
    client: &edm_core::reqwest::Client,
    search: &Search,
    deadline: Option<tokio::time::Instant>,
) -> Result<Crawled, Error> {
    crawl_until_watched(client, search, deadline, &CrawlWatch::default()).await
}

/// [`crawl_until`], telling `watch` how far it has come and stopped by it.
async fn crawl_until_watched(
    client: &edm_core::reqwest::Client,
    search: &Search,
    deadline: Option<tokio::time::Instant>,
    watch: &CrawlWatch,
) -> Result<Crawled, Error> {
    use std::collections::{HashSet, VecDeque};
    use std::sync::atomic::Ordering;
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
        // Stop pressed: what was found so far is the answer, as at the time limit.
        if found.pages > 0 && watch.stop.load(Ordering::Relaxed) {
            found.stopped = true;
            break;
        }
        // Past the time it was given: what was found so far is the answer.
        // The start page is always read, however slow.
        let ask = with_start_login(&page, &start);
        let fetch = fetch_page(client, &page, &ask, found.pages == 0);
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
        watch.pages.store(found.pages, Ordering::Relaxed);
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
                    watch.files.store(found.files.len(), Ordering::Relaxed);
                }
                continue;
            }
            if level < depth
                && looks_like_page(&link)
                && (search.other_sites || sites.iter().any(|site| same_site(site, &link)))
                && seen.insert(link.clone())
            {
                frontier.push_back((link, level + 1));
            }
        }
    }
    Ok(found)
}
