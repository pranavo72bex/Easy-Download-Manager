// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A page's qualities as last read, kept a few minutes so the panel is not read again.

use super::*;

/// How long a read of a page's qualities is kept, in seconds.
pub(super) const FORMATS_KEPT_SECS: u64 = 600;
/// How many pages' readings are kept.
pub(super) const FORMATS_KEPT_PAGES: usize = 48;

pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A page and the kind of session it was read with, as one name: signed in
/// or not, and with which cookies' names. Their values are left out: a
/// site restamps some on every request, and a name that changed with each
/// would never be found again. Never kept as more than a fingerprint.
pub(super) fn formats_key(url: &str, cookies: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut names: Vec<&str> = cookies
        .split(';')
        .filter_map(|c| c.split_once('=').map(|(n, _)| n.trim()))
        .filter(|n| !n.is_empty())
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    names.hash(&mut hasher);
    let page = url.split('#').next().unwrap_or(url);
    format!("{page}\n{:016x}", hasher.finish())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Remembered {
    pub(super) key: String,
    pub(super) at: u64,
    pub(super) choices: Vec<Choice>,
}

pub(super) fn formats_file(home: &Path) -> std::path::PathBuf {
    home.join(dm_core::paths::FORMATS_CACHE)
}

pub(super) fn recalled_formats(home: &Path, key: &str, now: u64) -> Option<Vec<Choice>> {
    let bytes = std::fs::read(formats_file(home)).ok()?;
    let all: Vec<Remembered> = dm_core::saved::rows_from_slice(&bytes).ok()?;
    all.into_iter()
        .find(|r| r.key == key && now.saturating_sub(r.at) < FORMATS_KEPT_SECS)
        .map(|r| r.choices)
        .filter(|choices| !choices.is_empty())
}

/// Best effort: a home that cannot be written to only means the next read
/// is made again.
pub(super) fn remember_formats(home: &Path, key: &str, choices: &[Choice], now: u64) {
    if choices.is_empty() {
        return;
    }
    let mut all: Vec<Remembered> = std::fs::read(formats_file(home))
        .ok()
        .and_then(|bytes| dm_core::saved::rows_from_slice(&bytes).ok())
        .unwrap_or_default();
    all.retain(|r| r.key != key && now.saturating_sub(r.at) < FORMATS_KEPT_SECS);
    all.push(Remembered {
        key: key.to_string(),
        at: now,
        choices: choices.to_vec(),
    });
    let excess = all.len().saturating_sub(FORMATS_KEPT_PAGES);
    all.drain(..excess);
    let Ok(text) = serde_json::to_string(&all) else {
        return;
    };
    let _ = std::fs::create_dir_all(home);
    let file = formats_file(home);
    let staged = file.with_extension("tmp");
    if std::fs::write(&staged, text).is_ok() {
        let _ = std::fs::rename(&staged, &file);
    }
}
