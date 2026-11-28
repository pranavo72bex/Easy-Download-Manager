// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where each saved page and file goes under the folder chosen (gaps row F2): in folders that
//! mirror its address, as the Grabber's "Keep the site’s folders" does, under names safe on
//! every disk, none given twice.

use std::collections::HashSet;

use sha2::{Digest as _, Sha256};
use url::Url;

/// The longest name, in bytes: with the folders above it, a path stays short of what
/// Windows refuses whole.
const MOST_NAME: usize = 80;

/// The longest path under the folder chosen, in bytes, before the file is put straight in
/// its site's folder.
const MOST_PATH: usize = 180;

/// The places given so far.
#[derive(Debug, Default)]
pub struct Names {
    /// Each file's path, in lower case: two names a disk may not tell apart are not given.
    files: HashSet<String>,
    /// Each folder's.
    folders: HashSet<String>,
}

/// `bytes`' digest, its first `n` bytes in hex: what tells two addresses with one name apart.
fn short_hash(bytes: &[u8], n: usize) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(n)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A name cut to at most `most` bytes, its extension kept.
fn shortened(name: &str, most: usize) -> String {
    if name.len() <= most {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 10 => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let mut cut = most.saturating_sub(ext.len());
    while cut > 0 && !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{ext}", stem.get(..cut).unwrap_or_default())
}

/// `name` made safe as a file or folder name; empty for one that is nothing once made safe.
fn clean(name: &str) -> String {
    let name = edm_core::sanitize_name(name);
    if name == "." || name == ".." {
        return String::new();
    }
    name
}

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// The extension a file served as `mime` is opened by, for the kinds a page needs.
fn extension_for(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "text/css" => "css",
        "text/javascript" | "application/javascript" | "application/x-javascript" => "js",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "font/woff" | "application/font-woff" => "woff",
        "font/woff2" => "woff2",
        "font/ttf" => "ttf",
        "font/otf" => "otf",
        "application/json" | "application/manifest+json" => "json",
        _ => return None,
    })
}

/// The site's own folder: its host, and its port when it is not the usual one.
fn site(address: &Url) -> String {
    let host = address.host_str().unwrap_or("site");
    let site = match address.port() {
        Some(port) => format!("{host}_{port}"),
        None => host.to_string(),
    };
    let site = clean(&site.replace(':', "_"));
    if site.is_empty() {
        "site".into()
    } else {
        shortened(&site, MOST_NAME)
    }
}

/// The file's own name: a page's ends in `.html` (`index.html` for a folder's), a stylesheet's
/// or a script's in the extension it is opened by, and an address with a query has the query's
/// digest in its name, so `list?page=2` and `list?page=3` are two files.
fn file_name(last: &str, query: Option<&str>, page: bool, mime: &str) -> String {
    let mut name = clean(last);
    if page {
        if name.is_empty() {
            name = "index.html".into();
        } else if !matches!(extension(&name).as_deref(), Some("html" | "htm")) {
            name.push_str(".html");
        }
    } else {
        if name.is_empty() {
            name = "index".into();
        }
        if let Some(wanted) = extension_for(mime) {
            let has = extension(&name);
            let needs_its_own = matches!(wanted, "css" | "js")
                && !matches!((wanted, has.as_deref()), ("js", Some("mjs")));
            if has.is_none() || (needs_its_own && has.as_deref() != Some(wanted)) {
                name = format!("{name}.{wanted}");
            }
        }
    }
    if let Some(query) = query.filter(|q| !q.is_empty()) {
        let digest = short_hash(query.as_bytes(), 4);
        name = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{digest}.{ext}"),
            _ => format!("{name}-{digest}"),
        };
    }
    shortened(&name, MOST_NAME)
}

/// `name` with `-n` before its extension.
fn numbered(name: &str, n: usize) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{n}.{ext}"),
        _ => format!("{name}-{n}"),
    }
}

impl Names {
    fn key(parts: &[String]) -> String {
        parts.join("/").to_lowercase()
    }

    /// Whether a folder in `parts` (all but the last) is a file already.
    fn folder_is_a_file(&self, parts: &[String]) -> bool {
        (1..parts.len()).any(|n| {
            parts
                .get(..n)
                .is_some_and(|folders| self.files.contains(&Self::key(folders)))
        })
    }

    /// The place for `address` (a page when `page`, served as `mime`): its site's folder, the
    /// folders of its address, and its name, each safe; none given before.
    pub fn place(&mut self, address: &Url, page: bool, mime: &str) -> Vec<String> {
        let segments: Vec<String> = address
            .path_segments()
            .map(|s| s.map(edm_core::decode_percent).collect())
            .unwrap_or_default();
        let (last, folders) = segments
            .split_last()
            .map_or(("", &[][..]), |(last, folders)| (last.as_str(), folders));
        let name = file_name(last, address.query(), page, mime);
        let mut parts = vec![site(address)];
        parts.extend(
            folders
                .iter()
                .map(|f| shortened(&clean(f), MOST_NAME))
                .filter(|f| !f.is_empty()),
        );
        parts.push(name.clone());
        if parts.iter().map(|p| p.len() + 1).sum::<usize>() > MOST_PATH
            || self.folder_is_a_file(&parts)
        {
            // Too deep, or under a name already a file's: straight in the site's folder.
            let flat = format!("{}-{name}", short_hash(address.as_str().as_bytes(), 6));
            parts = vec![site(address), shortened(&flat, MOST_NAME)];
        }
        let folders_of = parts.len() - 1;
        let chosen = parts.last().cloned().unwrap_or_default();
        let mut n = 1;
        while self.files.contains(&Self::key(&parts)) || self.folders.contains(&Self::key(&parts)) {
            n += 1;
            if let Some(last) = parts.last_mut() {
                *last = numbered(&chosen, n);
            }
            if n > 10_000 {
                break;
            }
        }
        for depth in 1..=folders_of {
            if let Some(folders) = parts.get(..depth) {
                self.folders.insert(Self::key(folders));
            }
        }
        self.files.insert(Self::key(&parts));
        parts
    }
}
