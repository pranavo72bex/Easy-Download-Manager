// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Watch pages, kept a moment (gaps row Y13): the page the panel's list was read from is the
//! page a download of the line picked reads first, asked the same way (`reading::watch_page`),
//! so it is not fetched a second time. Kept by the address asked and the cookies sent with it,
//! the cookies as a digest alone: a page fetched with one session is never read for another.

use std::time::{Duration, Instant};

/// How many pages are kept: a page is a megabyte or two.
const PAGES_KEPT: usize = 4;

/// How long a page is kept: the list, a line picked and the download's start.
const KEPT_FOR: Duration = Duration::from_secs(120);

/// The largest page kept; a larger one is read once and let go.
const PAGE_KEPT_MOST: usize = 4 << 20;

/// One page, by the address it was asked at and the digest of the cookies sent.
struct Kept {
    address: String,
    cookie: Vec<u8>,
    at: Instant,
    page: String,
}

/// The pages kept, newest last.
struct Pages(Vec<Kept>);

impl Pages {
    /// Lets go of the pages older than [`KEPT_FOR`].
    fn expire(&mut self, now: Instant) {
        self.0
            .retain(|k| now.saturating_duration_since(k.at) < KEPT_FOR);
    }

    fn find(&mut self, address: &str, cookie: &[u8], now: Instant) -> Option<String> {
        self.expire(now);
        self.0
            .iter()
            .find(|k| k.address == address && k.cookie == cookie)
            .map(|k| k.page.clone())
    }

    fn keep(&mut self, address: &str, cookie: Vec<u8>, page: &str, now: Instant) {
        self.expire(now);
        self.0
            .retain(|k| !(k.address == address && k.cookie == cookie));
        self.0.push(Kept {
            address: address.to_string(),
            cookie,
            at: now,
            page: page.to_string(),
        });
        let over = self.0.len().saturating_sub(PAGES_KEPT);
        self.0.drain(..over);
    }

    fn forget(&mut self, address: &str) {
        self.0.retain(|k| k.address != address);
    }
}

static PAGES: std::sync::Mutex<Pages> = std::sync::Mutex::new(Pages(Vec::new()));

/// The cookies' digest a page is kept by: the session itself is not held a second time.
fn key_of(cookie: &str) -> Vec<u8> {
    use sha2::Digest as _;
    sha2::Sha256::digest(cookie.as_bytes()).to_vec()
}

/// The page asked at `address` with `cookie` a moment ago, where one was.
pub(super) fn kept_page(address: &str, cookie: &str) -> Option<String> {
    crate::lock(&PAGES).find(address, &key_of(cookie), Instant::now())
}

/// `page`, asked at `address` with `cookie`, kept for the next ask of the same.
pub(super) fn keep_page(address: &str, cookie: &str, page: &str) {
    if page.len() > PAGE_KEPT_MOST {
        return;
    }
    crate::lock(&PAGES).keep(address, key_of(cookie), page, Instant::now());
    // Let go when its time is up, not at the next video's read: EDM at rest holds no page.
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async {
            tokio::time::sleep(KEPT_FOR).await;
            crate::lock(&PAGES).expire(Instant::now());
        });
    }
}

/// The pages asked at `address` let go of, whatever cookies they were asked with: a reading of
/// one failed, and the next asks the page afresh.
pub(super) fn forget_pages(address: &str) {
    crate::lock(&PAGES).forget(address);
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "https://www.youtube.com/watch?v=aaaaaaaaaaa";
    const PLAIN: &str = "PREF=hl=en&tz=UTC; SOCS=CAI";

    fn pages() -> Pages {
        Pages(Vec::new())
    }

    #[test]
    fn a_page_is_read_again_by_the_same_ask_within_its_time() {
        let mut pages = pages();
        let now = Instant::now();
        pages.keep(AT, key_of(PLAIN), "page", now);
        let soon = now + Duration::from_secs(60);
        assert_eq!(
            pages.find(AT, &key_of(PLAIN), soon).as_deref(),
            Some("page")
        );
        // Another video's page is not this one's.
        let other = AT.replace("aaaaaaaaaaa", "bbbbbbbbbbb");
        assert_eq!(pages.find(&other, &key_of(PLAIN), soon), None);
        // Past its time it is let go of.
        assert_eq!(pages.find(AT, &key_of(PLAIN), now + KEPT_FOR), None);
        assert!(pages.0.is_empty());
    }

    /// A page asked without a session is not read for a signed-in ask, nor one asked with
    /// one session for another (gaps row Y4).
    #[test]
    fn a_page_asked_with_other_cookies_is_not_read() {
        let mut pages = pages();
        let now = Instant::now();
        let signed = format!("SAPISID=one; {PLAIN}");
        pages.keep(AT, key_of(PLAIN), "anyone's", now);
        assert_eq!(pages.find(AT, &key_of(&signed), now), None);
        pages.keep(AT, key_of(&signed), "theirs", now);
        let another = format!("SAPISID=two; {PLAIN}");
        assert_eq!(pages.find(AT, &key_of(&another), now), None);
        assert_eq!(
            pages.find(AT, &key_of(&signed), now).as_deref(),
            Some("theirs")
        );
        assert_eq!(
            pages.find(AT, &key_of(PLAIN), now).as_deref(),
            Some("anyone's")
        );
    }

    #[test]
    fn only_the_newest_few_pages_are_kept() {
        let mut pages = pages();
        let now = Instant::now();
        let address = |n: usize| format!("{AT}&n={n}");
        for n in 0..PAGES_KEPT + 2 {
            pages.keep(&address(n), key_of(PLAIN), &n.to_string(), now);
        }
        assert_eq!(pages.0.len(), PAGES_KEPT);
        assert_eq!(pages.find(&address(1), &key_of(PLAIN), now), None);
        let newest = address(PAGES_KEPT + 1);
        let found = pages.find(&newest, &key_of(PLAIN), now);
        assert_eq!(found, Some((PAGES_KEPT + 1).to_string()));
        // The same ask again replaces its page rather than keeping two.
        pages.keep(&newest, key_of(PLAIN), "again", now);
        assert_eq!(pages.0.len(), PAGES_KEPT);
        assert_eq!(
            pages.find(&newest, &key_of(PLAIN), now).as_deref(),
            Some("again")
        );
    }

    #[test]
    fn a_page_forgotten_or_too_large_is_asked_again() {
        let mut pages = pages();
        let now = Instant::now();
        pages.keep(AT, key_of(PLAIN), "page", now);
        pages.forget(AT);
        assert_eq!(pages.find(AT, &key_of(PLAIN), now), None);

        let large = format!("{AT}&large=1");
        keep_page(&large, PLAIN, &"a".repeat(PAGE_KEPT_MOST + 1));
        assert_eq!(kept_page(&large, PLAIN), None);
        let usual = format!("{AT}&usual=1");
        keep_page(&usual, PLAIN, "usual");
        assert_eq!(kept_page(&usual, PLAIN).as_deref(), Some("usual"));
        forget_pages(&usual);
        assert_eq!(kept_page(&usual, PLAIN), None);
    }
}
