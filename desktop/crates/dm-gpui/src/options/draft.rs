// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What is typed in Options' fields put into the draft (`Options::put`), a
//! part of the settings in each helper; each says whether the field was its.

use super::Options;

impl Options {
    /// What was typed in field `key` put into the draft. A number out of
    /// its range is held to it; one that is not a number is left as it was.
    pub(crate) fn put(&mut self, key: &str, text: &str) {
        let parts: Vec<&str> = key.split('.').collect();
        let parts = parts.as_slice();
        let put = self.put_general(parts, text)
            || self.put_downloads(parts, text)
            || self.put_connection(parts, text)
            || self.put_proxy(parts, text);
        if !put {
            self.put_rest(parts, text);
        }
    }

    /// The lists typed as words, the folders and the smallest file the browser hands over:
    /// whether `parts` named one of them.
    fn put_general(&mut self, parts: &[&str], text: &str) -> bool {
        let n = |lo: u64, hi: u64| text.trim().parse::<u64>().ok().map(|n| n.clamp(lo, hi));
        let s = &mut self.draft;
        let at = |i: &str| i.parse::<usize>().ok();
        match parts {
            ["types"] => self.typed.types = text.into(),
            ["skip"] => self.typed.skip = text.into(),
            ["once"] => self.typed.once = text.into(),
            ["bypass"] => self.typed.bypass = text.into(),
            ["custom", i, j] => {
                if let (Some(i), Some(j)) = (at(i), at(j)) {
                    if let Some(cell) = self.typed.custom.get_mut(i).and_then(|r| r.get_mut(j)) {
                        *cell = text.into();
                    }
                }
            }
            ["download_dir"] => s.folders.download_dir = text.into(),
            ["parts_dir"] => s.folders.parts_dir = text.into(),
            ["category_folders", i] => {
                if let Some(folder) = at(i).and_then(|i| s.folders.category_folders.get_mut(i)) {
                    *folder = text.into();
                }
            }
            ["min_size_kb"] => {
                if let Some(n) = n(0, 1024 * 1024) {
                    s.browser.catch.min_size_kb = n;
                }
            }
            _ => return false,
        }
        true
    }

    /// Downloads' connections, queues, watched folder and times: whether `parts` named one.
    fn put_downloads(&mut self, parts: &[&str], text: &str) -> bool {
        let n = |lo: u64, hi: u64| text.trim().parse::<u64>().ok().map(|n| n.clamp(lo, hi));
        let s = &mut self.draft;
        let at = |i: &str| i.parse::<usize>().ok();
        match parts {
            ["connections"] => {
                if let Some(n) = n(1, 32) {
                    s.downloads.connections = n as usize;
                }
            }
            ["concurrency"] => {
                if let Some(n) = n(1, 10) {
                    s.downloads.concurrency = n as usize;
                }
            }
            ["queue_limits", i, "files"] => {
                let limit = at(i).and_then(|i| s.downloads.queue_limits.get_mut(i));
                if let (Some(limit), Some(n)) = (limit, n(1, 10)) {
                    limit.files = n as usize;
                }
            }
            ["queue_starts", i, "at"] => {
                if let Some(start) = at(i).and_then(|i| s.downloads.queue_starts.get_mut(i)) {
                    start.at = text.into();
                }
            }
            ["watch_folder"] => s.downloads.watch_folder = text.into(),
            ["record_minutes"] => {
                if let Some(n) = n(0, 1440) {
                    s.downloads.record_minutes = n;
                }
            }
            ["recheck_hours"] => {
                if let Some(n) = n(0, 24 * 31) {
                    s.downloads.recheck_hours = n;
                }
            }
            ["on_complete"] => s.downloads.on_complete = text.into(),
            _ => return false,
        }
        true
    }

    /// The speed limit, the retries, each server's and site's limits and rules, and the
    /// cookies file: whether `parts` named one of them.
    fn put_connection(&mut self, parts: &[&str], text: &str) -> bool {
        let n = |lo: u64, hi: u64| text.trim().parse::<u64>().ok().map(|n| n.clamp(lo, hi));
        let s = &mut self.draft;
        let at = |i: &str| i.parse::<usize>().ok();
        match parts {
            ["speed_limit"] => {
                if let Some(n) = n(0, 1024 * 1024) {
                    s.advanced.speed_limit = n;
                }
            }
            ["attempts"] => {
                if let Some(n) = n(0, 100) {
                    s.advanced.attempts = n as u32;
                }
            }
            ["retry_wait"] => {
                if let Some(n) = n(0, 3600) {
                    s.advanced.retry_wait = n;
                }
            }
            ["read_timeout"] => {
                if let Some(n) = n(5, 600) {
                    s.advanced.read_timeout = n;
                }
            }
            ["host_limits", i, "host"] => {
                if let Some(limit) = at(i).and_then(|i| s.advanced.host_limits.get_mut(i)) {
                    limit.host = text.into();
                }
            }
            ["host_limits", i, "connections"] => {
                let limit = at(i).and_then(|i| s.advanced.host_limits.get_mut(i));
                if let (Some(limit), Some(n)) = (limit, n(1, 32)) {
                    limit.connections = n as usize;
                }
            }
            ["site_rules", i, field] => {
                if let Some(rule) = at(i).and_then(|i| s.advanced.site_rules.get_mut(i)) {
                    match *field {
                        "host" => rule.host = text.into(),
                        "folder" => rule.folder = text.into(),
                        "agent" => rule.agent = text.into(),
                        "headers" => rule.headers = text.into(),
                        _ => {
                            if let Some(n) = n(0, 1024 * 1024) {
                                rule.speed = n;
                            }
                        }
                    }
                }
            }
            ["cookies_file"] => {
                // What was said about the file goes with it.
                let before = std::mem::replace(&mut s.advanced.cookies_file, text.into());
                if !before.is_empty() && self.says.as_ref().is_some_and(|w| w.contains(&before)) {
                    self.says = None;
                }
            }
            _ => return false,
        }
        true
    }

    /// The proxy, each scheme's and the dial-up connection: whether `parts` named one of them.
    fn put_proxy(&mut self, parts: &[&str], text: &str) -> bool {
        let n = |lo: u64, hi: u64| text.trim().parse::<u64>().ok().map(|n| n.clamp(lo, hi));
        let s = &mut self.draft;
        match parts {
            ["proxy", field] => {
                let p = &mut s.advanced.proxy;
                match *field {
                    "host" => p.host = text.into(),
                    "username" => p.username = text.into(),
                    "password" => p.password = text.into(),
                    "script" => p.script = text.into(),
                    _ => {
                        if let Some(n) = n(1, 65535) {
                            p.port = n as u16;
                        }
                    }
                }
            }
            ["per_scheme", scheme, field] => {
                if let Some(entry) = s.advanced.proxy.per_scheme.get_mut(*scheme) {
                    if *field == "host" {
                        entry.host = text.into();
                    } else if let Some(n) = n(1, 65535) {
                        entry.port = n as u16;
                    }
                }
            }
            ["dialup", field] => {
                let d = &mut s.advanced.dialup;
                match *field {
                    "username" => d.username = text.into(),
                    "password" => d.password = text.into(),
                    "attempts" => {
                        if let Some(n) = n(0, u64::from(dm_shell::dialup::MOST_ATTEMPTS)) {
                            d.attempts = n as u32;
                        }
                    }
                    _ => {
                        if let Some(n) = n(0, dm_shell::dialup::MOST_WAIT_SECONDS) {
                            d.wait_seconds = n;
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// Torrents' settings, the sites' sign-ins and the sounds.
    fn put_rest(&mut self, parts: &[&str], text: &str) {
        let n = |lo: u64, hi: u64| text.trim().parse::<u64>().ok().map(|n| n.clamp(lo, hi));
        let s = &mut self.draft;
        let at = |i: &str| i.parse::<usize>().ok();
        match parts {
            ["peer_blocklist"] => s.peer_blocklist = text.into(),
            ["torrent_share_ratio"] => {
                if let Ok(ratio) = text.trim().parse::<f64>() {
                    s.torrent_share_ratio = ratio.clamp(0.1, 100.0);
                }
            }
            ["torrent_share_minutes"] => {
                if let Some(n) = n(1, 7 * 24 * 60) {
                    s.torrent_share_minutes = n;
                }
            }
            ["torrent_upload_limit"] => {
                if let Some(n) = n(0, 1_000_000) {
                    s.torrent_upload_limit = n;
                }
            }
            ["site_logins", i, field] => {
                if let Some(login) = at(i).and_then(|i| s.site_logins.get_mut(i)) {
                    match *field {
                        "host" => login.host = text.into(),
                        "username" => login.username = text.into(),
                        _ => login.password = text.into(),
                    }
                }
            }
            ["sounds", event] => {
                if text.trim().is_empty() {
                    s.sounds.remove(*event);
                } else {
                    s.sounds.insert(event.to_string(), text.into());
                }
            }
            _ => {}
        }
    }
}
