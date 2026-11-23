// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A feed (RSS 2.0 or Atom): a podcast's episodes, a channel's uploads, a
//! site's new files, read as a playlist is (gaps row W10). Each item is a
//! download: its enclosure (the episode's file) where it has one, else its
//! link. The Playlist dialog lists them, and a feed followed is read again
//! every so many hours for the items it did not list before. A torrent feed's
//! items are kept too (gaps row C10): a `.torrent` file's address, a magnet
//! link in the item's link or enclosure, or its `torrent:magnetURI` (else
//! `torrent:infoHash`), each added as any torrent is.

// Reads outside input: an index out of range here is a crash a server could
// cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::youtube::{Playlist, PlaylistEntry};
use crate::Error;

/// The most items read of one feed.
const ITEMS_MOST: usize = 1000;

/// The most of a feed that is read: a podcast's whole history is a few
/// megabytes; a body that goes on past this is not a feed.
const FEED_MOST: usize = 16 << 20;

/// The longest a feed is waited for.
const WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

/// Whether an address names a feed by its look: a file ending `.rss` or
/// `.atom`, an `.xml` one named for a feed (`feed.xml`, `rss.xml`,
/// `podcast.xml`, YouTube's `feeds/videos.xml`), or a path whose last part
/// is `feed` or `rss`. A feed named otherwise is read as a page.
pub fn looks_like_feed(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let path = parsed.path().to_ascii_lowercase();
    let path = path.trim_end_matches('/');
    let last = path.rsplit('/').next().unwrap_or_default();
    if last.ends_with(".rss") || last.ends_with(".atom") || matches!(last, "feed" | "rss") {
        return true;
    }
    last.ends_with(".xml")
        && (path.contains("feed") || path.contains("rss") || path.contains("podcast"))
}

/// One item as the feed gives it.
#[derive(Default)]
struct Item {
    title: String,
    id: String,
    link: String,
    enclosure: String,
    /// A torrent feed's `<torrent:magnetURI>`, or one made of its
    /// `<torrent:infoHash>`.
    magnet: String,
}

/// Whether `address` is a magnet link (its scheme in any case).
fn is_magnet(address: &str) -> bool {
    address
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("magnet:"))
}

/// The magnet link of a torrent's info hash (40 hex digits, or 32 in base
/// 32); `None` for anything else.
fn magnet_of_hash(hash: &str) -> Option<String> {
    let hash = hash.trim();
    let fits = match hash.len() {
        40 => hash.bytes().all(|b| b.is_ascii_hexdigit()),
        32 => hash.bytes().all(|b| b.is_ascii_alphanumeric()),
        _ => false,
    };
    fits.then(|| format!("magnet:?xt=urn:btih:{hash}"))
}

impl Item {
    /// The item as a playlist's entry: its file's address, made whole
    /// against the feed's, where it has a web one.
    /// The enclosure first, then a torrent feed's magnet, then the link; a
    /// magnet link is kept as the feed says it.
    fn entry(self, base: Option<&url::Url>) -> Option<PlaylistEntry> {
        let wanted = [&self.enclosure, &self.magnet, &self.link]
            .into_iter()
            .map(|a| a.trim())
            .find(|a| !a.is_empty())?;
        let address = if is_magnet(wanted) {
            wanted.to_string()
        } else {
            let address = match base {
                Some(base) => base.join(wanted).ok()?,
                None => url::Url::parse(wanted).ok()?,
            };
            if !matches!(address.scheme(), "http" | "https") {
                return None;
            }
            address.to_string()
        };
        let title = self.title.trim().to_string();
        Some(PlaylistEntry {
            id: Some(self.id.trim().to_string())
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| address.clone()),
            title: if title.is_empty() {
                address.clone()
            } else {
                title
            },
            seconds: None,
            page: Some(address),
        })
    }
}

/// A tag's name without its namespace, in lower case (`atom:link` is a link).
fn local(raw: &str) -> String {
    let name = raw.to_ascii_lowercase();
    name.rsplit(':').next().unwrap_or_default().to_string()
}

/// A tag's attribute, its references undone.
fn attribute(tag: &BytesStart, name: &str) -> Option<String> {
    let found = tag.try_get_attribute(name).ok().flatten()?;
    let value = found
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .map(|v| v.into_owned())
        .unwrap_or_else(|_| found.value.to_string());
    Some(value.trim().to_string())
}

/// What a link or an enclosure tag says of the item: RSS's `<enclosure
/// url>`, Atom's `<link rel="enclosure" href>` and its plain or
/// `alternate` link.
fn take_tag(name: &str, tag: &BytesStart, item: &mut Item) {
    match name {
        "enclosure" if item.enclosure.is_empty() => {
            item.enclosure = attribute(tag, "url").unwrap_or_default();
        }
        "link" => {
            let Some(href) = attribute(tag, "href") else {
                return;
            };
            match attribute(tag, "rel").as_deref() {
                Some("enclosure") if item.enclosure.is_empty() => item.enclosure = href,
                None | Some("alternate") if item.link.is_empty() => item.link = href,
                _ => {}
            }
        }
        _ => {}
    }
}

/// A feed's text read as a playlist: its title and its items in its order,
/// each its file's address (relative ones made whole against `address`).
/// `None` when it is not an RSS or Atom feed.
pub fn parse(xml: &str, address: &str) -> Option<Playlist> {
    let base = url::Url::parse(address).ok();
    // Not trimmed: `Talk &amp; More` comes as three pieces, and trimming each
    // ran them together. What is kept is trimmed whole.
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = false;
    let mut is_feed = false;
    let mut title = String::new();
    let mut item: Option<Item> = None;
    let mut entries = Vec::new();
    let mut cut_short = false;
    // The element whose text arrives next, and that text.
    let mut collecting: Option<String> = None;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(tag)) => {
                let name = local(tag.name().as_ref());
                match name.as_str() {
                    "rss" | "feed" | "rdf" => is_feed = true,
                    "item" | "entry" => item = Some(Item::default()),
                    _ => {}
                }
                if let Some(item) = item.as_mut() {
                    take_tag(&name, &tag, item);
                }
                collecting = matches!(
                    name.as_str(),
                    "title" | "guid" | "id" | "link" | "magneturi" | "infohash"
                )
                .then_some(name);
                text.clear();
            }
            Ok(Event::Empty(tag)) => {
                let name = local(tag.name().as_ref());
                if let Some(item) = item.as_mut() {
                    take_tag(&name, &tag, item);
                }
            }
            Ok(Event::Text(chunk)) => {
                if collecting.is_some() {
                    let raw = chunk.xml10_content();
                    match quick_xml::escape::unescape(&raw) {
                        Ok(decoded) => text.push_str(&decoded),
                        Err(_) => text.push_str(&raw),
                    }
                }
            }
            Ok(Event::CData(chunk)) => {
                if collecting.is_some() {
                    text.push_str(&chunk.into_inner());
                }
            }
            Ok(Event::GeneralRef(entity)) => {
                if collecting.is_some() {
                    text.push_str(&crate::dash::entity_text(&entity));
                }
            }
            Ok(Event::End(tag)) => {
                let name = local(tag.name().as_ref());
                if collecting.as_deref() == Some(name.as_str()) {
                    let said = std::mem::take(&mut text);
                    match (item.as_mut(), name.as_str()) {
                        (Some(item), "title") => item.title = said,
                        (Some(item), "guid" | "id") => item.id = said,
                        (Some(item), "link") if item.link.is_empty() => item.link = said,
                        (Some(item), "magneturi") if is_magnet(said.trim()) => {
                            item.magnet = said.trim().to_string();
                        }
                        (Some(item), "infohash") if item.magnet.is_empty() => {
                            item.magnet = magnet_of_hash(&said).unwrap_or_default();
                        }
                        (None, "title") if title.is_empty() => title = said.trim().to_string(),
                        _ => {}
                    }
                }
                collecting = None;
                if matches!(name.as_str(), "item" | "entry") {
                    if let Some(entry) = item.take().and_then(|i| i.entry(base.as_ref())) {
                        if entries.len() >= ITEMS_MOST {
                            cut_short = true;
                            break;
                        }
                        entries.push(entry);
                    }
                }
            }
            _ => {}
        }
    }
    is_feed.then(|| Playlist {
        id: address.to_string(),
        title: if title.is_empty() {
            address.to_string()
        } else {
            title
        },
        entries,
        cut_short,
    })
}

/// The feed at `url`, read: its items as a playlist's videos.
pub async fn read_feed(client: &reqwest::Client, url: &str) -> Result<Playlist, Error> {
    let read = async {
        let reply = crate::redirects::send(client.get(url)).await?;
        let mut reply = Error::refused_or(reply).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = reply.chunk().await? {
            if bytes.len() + chunk.len() > FEED_MOST {
                return Err(Error::Internal("the feed is larger than any feed".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        parse(&String::from_utf8_lossy(&bytes), url)
            .ok_or_else(|| Error::Internal("that address is not an RSS or Atom feed".into()))
    };
    tokio::time::timeout(WITHIN, read)
        .await
        .map_err(|_| Error::Internal("the feed took too long to read".into()))?
}

#[cfg(test)]
mod tests;
