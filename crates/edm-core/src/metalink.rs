// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Reading a Metalink, which is a list of places one file can be fetched from.
//!
//! A single server puts a ceiling on a download that no amount of segmenting
//! can lift — its own upload rate, or a per-connection cap it applies on
//! purpose. Several mirrors of the same file do not share that ceiling. This
//! is what a Metalink describes: the same bytes, in several places, with the
//! checksums to prove they are the same bytes.
//!
//! Two versions are in use and both appear in the wild. Version 4 (RFC 5854)
//! puts urls directly under the file and ranks them by `priority`, where lower
//! is better. Version 3 nests them under `<resources>` and ranks by
//! `preference`, where higher is better. Both are read here, and both come out
//! as one list, best first.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::checksum::Checksum;

/// Rank given to a mirror that states no preference at all: worse than
/// anything explicitly preferred, better than anything explicitly deferred.
const UNRANKED: i64 = 500;

/// What a Metalink says about the one file EDM will fetch.
#[derive(Debug, Clone, PartialEq)]
pub struct Metalink {
    /// Empty when the document did not name it.
    pub filename: String,
    /// `None` when the document did not state one.
    pub size: Option<u64>,
    /// The strongest digest offered that EDM can actually check.
    pub checksum: Option<Checksum>,
    /// Every usable mirror, best first.
    pub mirrors: Vec<String>,
}

/// Whether this names a mirror list rather than a file.
pub fn is_metalink(url: &str, content_type: Option<&str>) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if path.ends_with(".meta4") || path.ends_with(".metalink") {
        return true;
    }
    content_type
        .unwrap_or("")
        .to_ascii_lowercase()
        .contains("application/metalink")
}

/// Reads a Metalink into the one file EDM will fetch.
///
/// A Metalink may describe many files; EDM takes the first, because a download
/// is one file and silently queueing a dozen from one click would be a
/// surprise. `None` when there is nothing usable in it.
pub fn parse(xml: &str) -> Option<Metalink> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_end_names = false;

    // Everything is gathered for the *first* file element only, so `depth`
    // tracks whether we are still inside it.
    let mut in_file = false;
    let mut file_depth = 0usize;
    let mut depth = 0usize;
    let mut seen_metalink = false;
    let mut finished = false;

    let mut filename = String::new();
    let mut size: Option<u64> = None;
    let mut hashes: Vec<Checksum> = Vec::new();
    let mut ranked: Vec<(i64, String)> = Vec::new();

    /// What the character data arriving next belongs to.
    ///
    /// A tuple carried a rank only `url` used and a name only `hash` used,
    /// so two of the three slots were dead in every arm.
    enum Collecting {
        /// The next text is the file's size.
        Size,
        Hash(String),
        Url(i64),
    }
    let mut collecting: Option<Collecting> = None;
    let mut text = String::new();
    /* Set while inside `<pieces>`, whose `<hash>` elements are each of one
     * piece. Taken as the whole file's, one of them was the digest a
     * finished download was held to, and a correct file failed it. */
    let mut pieces_depth: Option<usize> = None;

    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => break,

            Ok(Event::Start(tag)) => {
                depth += 1;
                let name = local_name(tag.name().as_ref());
                match name.as_str() {
                    "metalink" => seen_metalink = true,
                    "file" if !finished => {
                        in_file = true;
                        file_depth = depth;
                        filename = tag
                            .try_get_attribute("name")
                            .ok()
                            .flatten()
                            .map(|a| crate::probe::sanitize(&a.value))
                            .unwrap_or_default();
                    }
                    "size" if in_file => collecting = Some(Collecting::Size),
                    "pieces" if in_file => pieces_depth = Some(depth),
                    "hash" if in_file && pieces_depth.is_none() => {
                        collecting = Some(Collecting::Hash(
                            attribute(&tag, "type").unwrap_or_default(),
                        ));
                    }
                    "url" if in_file => collecting = Some(Collecting::Url(rank_of(&tag))),
                    _ => {}
                }
                text.clear();
            }

            Ok(Event::Text(chunk)) => {
                if collecting.is_some() {
                    // Escaped in the document; `&amp;` in a mirror URL is
                    // ordinary rather than exotic.
                    let raw = chunk.xml10_content();
                    match quick_xml::escape::unescape(&raw) {
                        Ok(decoded) => text.push_str(&decoded),
                        Err(_) => text.push_str(&raw),
                    }
                }
            }

            // `&amp;` in a mirror URL comes apart from the text, as a reference of its own.
            Ok(Event::GeneralRef(entity)) => {
                if collecting.is_some() {
                    text.push_str(&crate::dash::entity_text(&entity));
                }
            }

            Ok(Event::End(tag)) => {
                let name = local_name(tag.name().as_ref());
                if let Some(what) = collecting.take() {
                    let body = text.trim();
                    match what {
                        Collecting::Size => size = body.parse::<u64>().ok().filter(|n| *n > 0),
                        // `sha-256` and `sha256` both appear; the parser wants
                        // the latter, and a bare digest is read by its length
                        // when the type attribute says nothing usable.
                        Collecting::Hash(named) => {
                            let cleaned: String =
                                body.chars().filter(|c| !c.is_whitespace()).collect();
                            let named = named.replace('-', "");
                            if let Some(parsed) = Checksum::parse(&format!("{named}:{cleaned}"))
                                .or_else(|| Checksum::parse(&cleaned))
                            {
                                hashes.push(parsed);
                            }
                        }
                        // Mirrors are probed and fetched in HTTP ranges, so
                        // an FTP mirror is not one EDM can use.
                        Collecting::Url(rank) => {
                            let lower = body.to_ascii_lowercase();
                            if lower.starts_with("http://") || lower.starts_with("https://") {
                                ranked.push((rank, body.to_string()));
                            }
                        }
                    }
                }
                text.clear();

                if name == "pieces" && pieces_depth == Some(depth) {
                    pieces_depth = None;
                }
                if name == "file" && in_file && depth == file_depth {
                    in_file = false;
                    // The first file is the one EDM takes; the rest of the
                    // document is somebody else's download.
                    finished = true;
                }
                depth = depth.saturating_sub(1);
            }

            _ => {}
        }
    }

    if !seen_metalink || ranked.is_empty() {
        return None;
    }

    // Stable, so mirrors of equal rank keep the order the document gave them.
    ranked.sort_by_key(|(rank, _)| *rank);
    // Longest digest is the strongest: sha-512 over sha-256 over md5.
    hashes.sort_by_key(|c| std::cmp::Reverse(c.expected.len()));

    Some(Metalink {
        filename,
        size,
        checksum: hashes.into_iter().next(),
        mirrors: ranked.into_iter().map(|(_, url)| url).collect(),
    })
}

/// Namespaced documents are common, and `metalink:file` is still a file.
fn local_name(raw: &str) -> String {
    let name = raw.to_ascii_lowercase();
    name.rsplit(':').next().unwrap_or(&name).to_string()
}

fn attribute(tag: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    tag.try_get_attribute(name)
        .ok()
        .flatten()
        .map(|a| a.value.trim().to_ascii_lowercase())
}

/// Both rankings, turned into one where smaller is better.
fn rank_of(tag: &quick_xml::events::BytesStart) -> i64 {
    if let Some(priority) = attribute(tag, "priority").and_then(|v| v.parse::<i64>().ok()) {
        return priority;
    }
    if let Some(preference) = attribute(tag, "preference").and_then(|v| v.parse::<i64>().ok()) {
        return 1000 - preference;
    }
    UNRANKED
}

#[cfg(test)]
mod tests;
