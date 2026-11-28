// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Save pages for offline reading" (gaps row F2): the Grabber's pages, read within its own
//! scope (the depth, the start page's site unless other sites are asked for, the paths left
//! out, a hundred pages, the time given and Stop), saved with what they need to be drawn
//! (stylesheets and what they import, scripts, pictures, fonts) in folders that mirror the
//! site's addresses. Each saved page's links to another saved page or file are made relative
//! paths to it, and every other web address it names is made absolute, so nothing in the
//! folder points to a file that is not there. Only the pages within scope are read, and only
//! what they need is asked for: a page outside scope is never fetched, nor anything it names.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;

use url::Url;

use crate::Error;

use super::{
    has_any, looks_like_page, path_of, same_site, with_start_login, CrawlWatch, Search, MAX_DEPTH,
    MAX_PAGES, MOST_MINUTES,
};

pub mod markup;
mod names;

#[cfg(test)]
mod tests;

/// The most files one save fetches beside its pages.
pub const MAX_RESOURCES: usize = 2000;

/// The largest file a page needs that is saved; a larger one keeps its web address.
pub const RESOURCE_CAP: usize = 32 * 1024 * 1024;

/// The most one save writes, in bytes; past it, what is left keeps its web address.
pub const MOST_BYTES: usize = 1024 * 1024 * 1024;

/// The most of a page read, as the Grabber reads one.
const PAGE_CAP: usize = 8 * 1024 * 1024;

/// How many of a page's files are asked for at once.
const AT_ONCE: usize = 6;

/// What a save of pages for offline reading did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Saved {
    /// How many pages were saved.
    pub pages: usize,
    /// How many files the pages need were saved beside them.
    pub resources: usize,
    /// The start page's saved file, to open first.
    pub index: PathBuf,
    /// Whether the time given ran out, or Stop was pressed, with more to read.
    pub stopped: bool,
}

/// One page or file read.
struct Got {
    /// The address it was asked for, without a login.
    address: Url,
    /// Where it was read from in the end, which its links are relative to.
    landed: Url,
    /// Its type, as the server said (`text/html`).
    mime: String,
    /// The charset the server said, for a page.
    charset: Option<String>,
    bytes: Vec<u8>,
    page: bool,
}

/// An address as a saved file is looked up by: without its fragment and any login.
fn key(address: &Url) -> String {
    let mut address = address.clone();
    address.set_fragment(None);
    let _ = address.set_username("");
    let _ = address.set_password(None);
    address.to_string()
}

/// The charset a `Content-Type` names.
fn charset_of(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches(['"', '\'']).to_string())
            .filter(|v| !v.is_empty())
    })
}

/// What was read: where it landed, its type, its charset and its bytes.
type Read = (Url, String, Option<String>, Vec<u8>);

/// `ask` read: a page (the start page when `start`), held to [`PAGE_CAP`] and refused when it
/// is not HTML, as the Grabber's search reads one; or a file a page needs, refused past `cap`.
async fn fetch(
    client: &edm_core::reqwest::Client,
    ask: &str,
    page: Option<bool>,
    cap: usize,
) -> Result<Read, Error> {
    use futures_util::StreamExt as _;
    let res = edm_core::auth::send(client, client.get(ask), ask)
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("it answered {}", res.status()).into());
    }
    let content_type = res
        .headers()
        .get(edm_core::reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mime = edm_core::mime_essence(Some(&content_type));
    if let Some(start) = page {
        let html = mime == "text/html" || mime == "application/xhtml+xml";
        if !(html || (start && mime.is_empty())) {
            return Err(format!("a file ({mime}), not a page").into());
        }
    } else if res.content_length().is_some_and(|n| n > cap as u64) {
        return Err("larger than a page's file is saved".into());
    }
    let landed = res.url().clone();
    let mut bytes = Vec::new();
    let mut body = res.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if bytes.len() + chunk.len() > cap {
            if page.is_none() {
                return Err("larger than a page's file is saved".into());
            }
            let room = cap - bytes.len();
            bytes.extend_from_slice(chunk.get(..room).unwrap_or_default());
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((landed, mime, charset_of(&content_type), bytes))
}

/// `future`, unless `deadline` comes first: `None` then.
async fn before<T>(
    deadline: Option<tokio::time::Instant>,
    future: impl std::future::Future<Output = T>,
) -> Option<T> {
    match deadline {
        Some(at) => tokio::time::timeout_at(at, future).await.ok(),
        None => Some(future.await),
    }
}

/// The site's pages within the search's scope, read as the Grabber's search reads them, and
/// the files they need, not yet read.
struct Reading<'a> {
    client: &'a edm_core::reqwest::Client,
    search: &'a Search,
    start: Url,
    deadline: Option<tokio::time::Instant>,
    watch: &'a CrawlWatch,
    got: Vec<Got>,
    needed: Vec<Url>,
    asked: HashSet<String>,
    stopped: bool,
    bytes: usize,
}

impl Reading<'_> {
    /// Whether Stop was pressed, once something was read.
    fn stop_pressed(&self) -> bool {
        !self.got.is_empty() && self.watch.stop.load(Ordering::Relaxed)
    }

    /// A file a page needs, to be read unless a path left out names it.
    fn need(&mut self, link: Url) {
        if !has_any(&path_of(link.as_str()), &self.search.exclude) && self.asked.insert(key(&link))
        {
            self.needed.push(link);
        }
    }

    /// The pages, from the start page, `depth` deep, at most [`MAX_PAGES`].
    async fn pages(&mut self) -> Result<(), Error> {
        let depth = self.search.depth.min(MAX_DEPTH);
        let mut seen: HashSet<String> = HashSet::from([key(&self.start)]);
        let mut frontier: VecDeque<(Url, u8)> = VecDeque::from([(self.start.clone(), 0)]);
        // The site is where the start page was typed and where it really is, as in a search.
        let mut sites = vec![self.start.clone()];
        while let Some((page, level)) = frontier.pop_front() {
            if self.got.len() >= MAX_PAGES {
                break;
            }
            if self.stop_pressed() {
                self.stopped = true;
                break;
            }
            let first = self.got.is_empty();
            let ask = with_start_login(page.as_str(), &self.start);
            let fetching = fetch(self.client, &ask, Some(first), PAGE_CAP);
            // The start page is always read, however slow.
            let read = if first {
                Some(fetching.await)
            } else {
                before(self.deadline, fetching).await
            };
            let Some(read) = read else {
                self.stopped = true;
                break;
            };
            let (landed, mime, charset, bytes) = match read {
                Ok(read) => read,
                Err(e) if first => return Err(format!("{}: {e}", key(&page)).into()),
                Err(_) => continue,
            };
            if first {
                seen.insert(key(&landed));
                sites.push(landed.clone());
            }
            for (link, kind) in markup::addresses(&bytes, &landed) {
                match kind {
                    markup::Kind::Resource => self.need(link),
                    markup::Kind::Page => {
                        let wanted = level < depth
                            && !has_any(&path_of(link.as_str()), &self.search.exclude)
                            && looks_like_page(link.as_str())
                            && (self.search.other_sites
                                || sites.iter().any(|site| same_site(site, link.as_str())));
                        if wanted && seen.insert(key(&link)) {
                            frontier.push_back((link, level + 1));
                        }
                    }
                    markup::Kind::Elsewhere => {}
                }
            }
            self.bytes += bytes.len();
            self.got.push(Got {
                address: page,
                landed,
                mime,
                charset,
                bytes,
                page: true,
            });
            self.watch.pages.store(self.got.len(), Ordering::Relaxed);
        }
        Ok(())
    }

    /// The files the pages need, and what their stylesheets import and name in turn.
    async fn resources(&mut self) {
        use futures_util::StreamExt as _;
        let mut saved = 0;
        while !self.needed.is_empty()
            && !self.stopped
            && saved < MAX_RESOURCES
            && self.bytes < MOST_BYTES
        {
            if self.stop_pressed() {
                self.stopped = true;
                break;
            }
            let room = (MAX_RESOURCES - saved).min(self.needed.len());
            let wave: Vec<Url> = self.needed.drain(..room).collect();
            let (client, start, deadline) = (self.client, &self.start, self.deadline);
            let read: Vec<(Url, Option<Result<Read, Error>>)> = futures_util::stream::iter(wave)
                .map(|link| async move {
                    let ask = with_start_login(link.as_str(), start);
                    let read = before(deadline, fetch(client, &ask, None, RESOURCE_CAP)).await;
                    (link, read)
                })
                .buffered(AT_ONCE)
                .collect()
                .await;
            for (link, read) in read {
                let Some(read) = read else {
                    self.stopped = true;
                    continue;
                };
                let Ok((landed, mime, _, bytes)) = read else {
                    continue;
                };
                if self.bytes + bytes.len() > MOST_BYTES {
                    continue;
                }
                if is_stylesheet(&landed, &mime) {
                    for named in markup::css_addresses(&bytes, &landed) {
                        self.need(named);
                    }
                }
                self.bytes += bytes.len();
                self.got.push(Got {
                    address: link,
                    landed,
                    mime,
                    charset: None,
                    bytes,
                    page: false,
                });
                saved += 1;
                self.watch.files.store(saved, Ordering::Relaxed);
            }
        }
    }
}

fn is_stylesheet(landed: &Url, mime: &str) -> bool {
    mime == "text/css"
        || ((mime.is_empty() || mime == "text/plain")
            && landed.path().to_ascii_lowercase().ends_with(".css"))
}

/// `parts` under `folder`, each a plain name: never a way out of the folder.
fn under(folder: &Path, parts: &[String]) -> Option<PathBuf> {
    let mut path = folder.to_path_buf();
    for part in parts {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => path.push(name),
            _ => return None,
        }
    }
    Some(path)
}

/// Saves the pages `search` reaches, within its scope, into `folder` to be read offline: see the
/// module's comment. A start page that cannot be read is the error, as is a file that cannot be
/// written; a page or file further in that cannot be read keeps its web address.
pub async fn save_pages(
    client: &edm_core::reqwest::Client,
    search: &Search,
    folder: &Path,
) -> Result<Saved, Error> {
    save_pages_watched(client, search, folder, &CrawlWatch::default()).await
}

/// [`save_pages`], telling `watch` how many pages and files it has read, and stopped by it:
/// what was read when Stop is pressed is saved.
pub async fn save_pages_watched(
    client: &edm_core::reqwest::Client,
    search: &Search,
    folder: &Path,
    watch: &CrawlWatch,
) -> Result<Saved, Error> {
    let start =
        Url::parse(search.page.trim()).map_err(|e| format!("that is not a page address: {e}"))?;
    if !matches!(start.scheme(), "http" | "https") {
        return Err("only a web site's pages are saved to read offline".into());
    }
    let minutes = search.stop_after_minutes.min(MOST_MINUTES);
    let deadline = (minutes > 0)
        .then(|| tokio::time::Instant::now() + std::time::Duration::from_secs(minutes * 60));
    let mut reading = Reading {
        client,
        search,
        start,
        deadline,
        watch,
        got: Vec::new(),
        needed: Vec::new(),
        asked: HashSet::new(),
        stopped: false,
        bytes: 0,
    };
    reading.pages().await?;
    reading.resources().await;
    write(&reading.got, folder, reading.stopped).await
}

/// What was read, written under `folder` with its links made to work there.
async fn write(got: &[Got], folder: &Path, stopped: bool) -> Result<Saved, Error> {
    let mut names = names::Names::default();
    let places: Vec<Vec<String>> = got
        .iter()
        .map(|g| names.place(&g.address, g.page, &g.mime))
        .collect();
    let mut at: HashMap<String, usize> = HashMap::new();
    for (index, g) in got.iter().enumerate() {
        at.entry(key(&g.address)).or_insert(index);
        at.entry(key(&g.landed)).or_insert(index);
    }
    let mut saved = Saved {
        stopped,
        ..Saved::default()
    };
    for (g, here) in got.iter().zip(&places) {
        let link_to = |link: &Url| {
            at.get(&key(link))
                .and_then(|&index| places.get(index))
                .map(|there| markup::relative(here, there))
        };
        let bytes = if g.page {
            markup::rewrite_html(&g.bytes, &g.landed, g.charset.as_deref(), &link_to)
        } else if is_stylesheet(&g.landed, &g.mime) {
            markup::rewrite_css(&g.bytes, &g.landed, &link_to)
        } else {
            g.bytes.clone()
        };
        let path =
            under(folder, here).ok_or_else(|| format!("{} has no safe name", key(&g.address)))?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if g.page {
            if saved.pages == 0 {
                saved.index = path;
            }
            saved.pages += 1;
        } else {
            saved.resources += 1;
        }
    }
    Ok(saved)
}
