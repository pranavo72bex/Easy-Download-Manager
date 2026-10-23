use super::*;

impl DmApp {
    pub fn set_clipboard_watch(&mut self, on: bool) {
        self.clipboard.on = on;
        if !on {
            self.clipboard.offered = None;
            self.clipboard.offered_all.clear();
            return;
        }
        // Seeded with whatever is there now, so turning it on does not offer
        // a link copied long ago for another reason.
        self.clipboard.last = self.backend.platform.clipboard();
        self.clipboard.polled = Some(Instant::now());
    }

    /// Looks at the clipboard once, now.
    pub fn check_clipboard(&mut self) {
        if !self.clipboard.on {
            return;
        }
        self.clipboard.polled = Some(Instant::now());
        let text = self.backend.platform.clipboard();
        if text == self.clipboard.last {
            return;
        }
        self.clipboard.last = text.clone();
        let fresh = |url: &String| {
            !self.clipboard.dismissed.contains(url) && !self.rows.iter().any(|r| &r.url == url)
        };
        // Text that is links and nothing else, one or several; a sentence
        // with a link in it is not an offer.
        let words: Vec<&str> = text.as_deref().unwrap_or("").split_whitespace().collect();
        let links: Vec<String> =
            if !words.is_empty() && words.iter().all(|w| looks_downloadable(w).is_some()) {
                let mut links: Vec<String> = Vec::new();
                for w in words {
                    if !links.iter().any(|l| l == w) {
                        links.push(w.to_string());
                    }
                }
                links.into_iter().filter(fresh).collect()
            } else {
                text.as_deref()
                    .and_then(looks_downloadable)
                    .filter(fresh)
                    .into_iter()
                    .collect()
            };
        self.clipboard.offered = links.first().cloned();
        self.clipboard.offered_all = links;
    }

    pub fn clipboard_offer(&self) -> Option<&str> {
        self.clipboard.offered.as_deref()
    }
}
