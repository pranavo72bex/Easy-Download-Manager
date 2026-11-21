// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the browsers' extensions said of themselves, and which browsers dm has heard from.

use super::*;

/// The extension file's format number (`dm_core::saved`): 1, the hello's fields beside
/// `format`. The formats cache carries none: it stays a bare list, read in either shape
/// (`dm_core::saved::rows_from_slice`).
pub(super) const EXTENSION_FORMAT: u32 = 1;

/// The answer to one frame, or `None` for a frame that gets none.
/// The file that keeps what the last extension to say hello said of itself.
pub(super) fn extension_file(home: &Path) -> std::path::PathBuf {
    home.join(dm_core::paths::EXTENSION)
}

/// Keeps what an extension said of itself in its hello, written only when it changed. Best
/// effort: a home that cannot be written to costs nothing but the note. A bare hello, from an
/// extension of 3.4.0 or older, is kept as it is, so an older extension is known for one.
pub(super) fn note_extension(home: &Path, greeting: &dm_protocol::Greeting) {
    if extension_heard(home).as_ref() == Some(greeting) {
        return;
    }
    let Ok(text) = serde_json::to_string(&dm_core::saved::Stamped {
        format: EXTENSION_FORMAT,
        body: greeting,
    }) else {
        return;
    };
    let _ = std::fs::create_dir_all(home);
    let file = extension_file(home);
    let staged = file.with_extension("tmp");
    if std::fs::write(&staged, text).is_ok() {
        let _ = std::fs::rename(&staged, &file);
    }
}

/// What the last extension to say hello said of itself: its version and what it can do
/// (both empty for an extension of 3.4.0 or older), or `None` before any has.
pub fn extension_heard(home: &Path) -> Option<dm_protocol::Greeting> {
    let text = std::fs::read_to_string(extension_file(home)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The file whose being there says a browser's extension has talked to dm.
fn browser_seen_file(home: &Path) -> std::path::PathBuf {
    home.join(dm_core::paths::BROWSER_SEEN)
}

/// Notes that an extension has said hello, and which browser it is in
/// where that can be told (a line each). Best effort: a home that cannot
/// be written to only means the window goes on offering the extension.
pub(super) fn mark_browser_seen(home: &Path) {
    note_browser(home, parent_browser().as_deref());
}

/// [`mark_browser_seen`] for a browser already named (or none), heard now.
pub(super) fn note_browser(home: &Path, browser: Option<&str>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    note_browser_at(home, browser, now);
}

/// [`note_browser`] at `now`, in seconds since 1970: a line each, the browser's name and when
/// it was last heard from (gaps row U24), written again only when that is a minute old or more.
/// One that cannot be named is kept under no name.
pub(super) fn note_browser_at(home: &Path, browser: Option<&str>, now: u64) {
    let file = browser_seen_file(home);
    let mut heard = browsers_heard_all(home);
    let name = browser.unwrap_or("");
    match heard.iter_mut().find(|(n, _)| n == name) {
        Some((_, at)) if at.is_some_and(|at| now.saturating_sub(at) < 60) && file.exists() => {
            return;
        }
        Some((_, at)) => *at = Some(now),
        None => heard.push((name.to_string(), Some(now))),
    }
    let _ = std::fs::create_dir_all(home);
    let text: String = heard
        .iter()
        .map(|(name, at)| match at {
            Some(at) => format!("{name}\t{at}\n"),
            None => format!("{name}\n"),
        })
        .collect();
    let _ = std::fs::write(file, text);
}

/// Every line of the file: a browser's name ("" for one not named) and when it was last heard
/// from, where the file says (a file from before keeps names alone).
fn browsers_heard_all(home: &Path) -> Vec<(String, Option<u64>)> {
    std::fs::read_to_string(browser_seen_file(home))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| match line.split_once('\t') {
            Some((name, at)) => (name.trim().to_string(), at.trim().parse().ok()),
            None => (line.trim().to_string(), None),
        })
        .collect()
}

/// The browsers whose extension has talked to this dm, by the names
/// [`parent_browser`] gives ("Microsoft Edge", "Firefox", "Brave").
pub fn browsers_seen(home: &Path) -> Vec<String> {
    browsers_heard(home)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// [`browsers_seen`], each with when it was last heard from (seconds since 1970) where that
/// is known: what Options says beside "Connected" (gaps row U24).
pub fn browsers_heard(home: &Path) -> Vec<(String, Option<u64>)> {
    browsers_heard_all(home)
        .into_iter()
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

/// Whether any browser's extension has ever talked to this dm: until one
/// has, the window says how to add it.
pub fn browser_seen(home: &Path) -> bool {
    browser_seen_file(home).exists()
}
