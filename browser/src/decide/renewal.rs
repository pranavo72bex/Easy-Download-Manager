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
    /// Remembers an address the app took. A web address only; a second hand-off of the same one
    /// starts its count again.
    pub fn keep(&mut self, url: &str, referrer: &str) {
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
            if let Some(new) = seen.renewed(&entry.url) {
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
mod tests {
    use super::*;
    use crate::decide::{Header, Sent};

    fn heard(seen: &mut RequestHeaders, url: &str) {
        seen.keep(&Sent {
            url: url.into(),
            kind: "media".into(),
            request_headers: vec![Header {
                name: "sec-ch-ua".into(),
                value: Some("Edge".into()),
            }],
        });
    }

    const OLD: &str = "https://cdn.example/videoplayback?itag=22&sig=old&expire=1";
    const NEW: &str = "https://cdn.example/videoplayback?itag=22&sig=new&expire=2";

    #[test]
    fn a_handed_stream_asked_for_anew_is_offered_once_in_each_interval() {
        let mut handed = Handed::default();
        let mut seen = RequestHeaders::default();
        handed.keep(OLD, "https://site.example/watch");
        heard(&mut seen, OLD);
        assert!(handed.due(&seen, 0.0).is_empty(), "nothing newer yet");
        heard(&mut seen, NEW);
        let due = handed.due(&seen, 1_000.0);
        assert_eq!(
            due,
            vec![Renewal {
                old: OLD.into(),
                new: NEW.into(),
                referrer: "https://site.example/watch".into(),
            }]
        );
        assert!(handed.due(&seen, 1_000.0 + RENEW_EVERY_MS - 1.0).is_empty());
        assert_eq!(handed.due(&seen, 1_000.0 + RENEW_EVERY_MS).len(), 1);
    }

    #[test]
    fn another_stream_or_a_piece_of_one_is_not_a_renewal() {
        let mut handed = Handed::default();
        let mut seen = RequestHeaders::default();
        handed.keep(OLD, "");
        heard(
            &mut seen,
            "https://cdn.example/videoplayback?itag=140&sig=new&expire=2",
        );
        heard(
            &mut seen,
            "https://cdn.example/videoplayback?itag=22&sig=new&expire=2&range=0-99",
        );
        heard(
            &mut seen,
            "https://other.example/videoplayback?itag=22&sig=new&expire=2",
        );
        assert!(handed.due(&seen, 0.0).is_empty());
    }

    #[test]
    fn offers_stop_after_the_most_for_one_hand_off() {
        let mut handed = Handed::default();
        let mut seen = RequestHeaders::default();
        handed.keep(OLD, "");
        heard(&mut seen, NEW);
        let offered: usize = (0..MOST_OFFERS + 5)
            .map(|n| handed.due(&seen, f64::from(n) * RENEW_EVERY_MS).len())
            .sum();
        assert_eq!(offered, MOST_OFFERS as usize);
        // Handed over again, it is counted afresh.
        handed.keep(OLD, "");
        assert_eq!(handed.due(&seen, 1e12).len(), 1);
    }

    #[test]
    fn only_web_addresses_are_kept_and_the_list_is_bounded() {
        let mut handed = Handed::default();
        handed.keep("magnet:?xt=urn:btih:abc", "");
        handed.keep("blob:https://site.example/1", "");
        assert!(handed.kept.is_empty());
        for n in 0..MOST_HANDED + 10 {
            handed.keep(&format!("https://cdn.example/{n}.mp4?sig=a"), "");
        }
        assert_eq!(handed.kept.len(), MOST_HANDED);
        assert!(handed.kept.front().unwrap().url.contains("/10.mp4"));
    }

    #[test]
    fn what_a_worker_kept_survives_its_nap_and_newer_wins() {
        let mut first = Handed::default();
        first.keep(OLD, "https://site.example/a");
        let saved = first.export();
        let mut next = Handed::default();
        next.keep(OLD, "https://site.example/b");
        next.merge(&saved);
        next.merge("not json");
        assert_eq!(next.kept.len(), 1);
        assert_eq!(next.kept[0].referrer, "https://site.example/b");
        let mut fresh = Handed::default();
        fresh.merge(&saved);
        assert_eq!(fresh.kept.len(), 1);
    }
}
