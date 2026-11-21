// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Streams handed to dm whose signed address the page's player has since asked for anew (gaps
//! row 121). A download whose address runs out (403 or 410) waits in the app for the browser's
//! next address of the same file; the page's player keeps asking with fresh signatures while the
//! tab is open, so the newest is offered to the app, marked as a renewal only: the app gives it
//! to a row that waits and drops it otherwise. Offered at most every [`RENEW_EVERY_MS`] and
//! [`MOST_OFFERS`] times for one hand-off, so an open tab does not start the host for ever.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::RequestHeaders;

/// The most hand-offs remembered.
const MOST_HANDED: usize = 50;
/// The least time between two offers for one hand-off.
pub const RENEW_EVERY_MS: f64 = 30_000.0;
/// The most offers for one hand-off: ten minutes of them at the least spacing.
pub const MOST_OFFERS: u32 = 20;

/// One address handed to dm, and what has been offered for it since.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Entry {
    url: String,
    referrer: String,
    offered_at: f64,
    offers: u32,
    /// The tab it was handed from; only that tab's requests renew it. None from no tab (a caught
    /// download), or kept by an extension older than this field.
    #[serde(default)]
    tab: Option<i64>,
}

/// A newer address for a stream handed to dm: what the extension sends with `renews`.
#[derive(Debug, Clone, PartialEq)]
pub struct Renewal {
    /// The address that was handed over.
    pub old: String,
    /// The newest one the page asked for.
    pub new: String,
    /// The page it was handed from.
    pub referrer: String,
}

/// The addresses handed to dm, oldest forgotten first.
#[derive(Debug, Default)]
pub struct Handed {
    kept: VecDeque<Entry>,
}

impl Handed {
    /// Remembers an address the app took from `tab`. A web address only; a second hand-off of the
    /// same one starts its count again.
    pub fn keep(&mut self, url: &str, referrer: &str, tab: Option<i64>) {
        let url = url.split('#').next().unwrap_or(url);
        if !url.starts_with("http") {
            return;
        }
        self.kept.retain(|e| e.url != url);
        self.kept.push_back(Entry {
            url: url.to_string(),
            referrer: referrer.to_string(),
            offered_at: 0.0,
            offers: 0,
            tab,
        });
        while self.kept.len() > MOST_HANDED {
            self.kept.pop_front();
        }
    }

    /// The renewals due at `now`: each handed address the page has since asked for anew
    /// (`RequestHeaders::renewed`), not offered within [`RENEW_EVERY_MS`], and offered fewer than
    /// [`MOST_OFFERS`] times. Each returned is counted as offered.
    pub fn due(&mut self, seen: &RequestHeaders, now: f64) -> Vec<Renewal> {
        let mut out = Vec::new();
        for entry in self.kept.iter_mut() {
            // `offered_at` counts only once something was offered.
            if entry.offers >= MOST_OFFERS
                || (entry.offers > 0 && now - entry.offered_at < RENEW_EVERY_MS)
            {
                continue;
            }
            if let Some(new) = seen.renewed(&entry.url, entry.tab) {
                entry.offered_at = now;
                entry.offers += 1;
                out.push(Renewal {
                    old: entry.url.clone(),
                    new,
                    referrer: entry.referrer.clone(),
                });
            }
        }
        out
    }

    /// Everything kept, for the session's storage, which outlives the worker.
    pub fn export(&self) -> String {
        serde_json::to_string(&self.kept).unwrap_or_default()
    }

    /// Takes in what an earlier worker kept, older than anything kept since.
    pub fn merge(&mut self, older: &str) {
        let Ok(older) = serde_json::from_str::<Vec<Entry>>(older) else {
            return;
        };
        let mut merged: VecDeque<Entry> = older
            .into_iter()
            .filter(|e| e.url.starts_with("http") && !self.kept.iter().any(|k| k.url == e.url))
            .collect();
        merged.extend(std::mem::take(&mut self.kept));
        while merged.len() > MOST_HANDED {
            merged.pop_front();
        }
        self.kept = merged;
    }
}

#[cfg(test)]
mod tests;
