// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Links in text and in files: what the clipboard offer, a drop and the
//! watched folder read as addresses to download. Pure text work, the same
//! whichever presentation hands the text or the file over.

/// What the window offers to queue on its own: the first word of some text,
/// if it is a link. The queue is the authority on what it accepts.
pub fn looks_downloadable(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?;
    let lower = first.to_ascii_lowercase();
    SCHEMES
        .iter()
        .any(|s| lower.starts_with(s))
        .then(|| first.to_string())
}

/// The starts of the addresses dm takes: the web and FTP, magnets, and the
/// stream servers' protocols dm records (`dm_core::live`).
pub const SCHEMES: [&str; 13] = [
    "http://", "https://", "ftp://", "magnet:?", "rtmp://", "rtmps://", "rtmpe://", "rtmpt://",
    "rtsp://", "rtsps://", "mms://", "mmsh://", "mmst://",
];

/// Whether `text` holds a second address after its first: two links
/// pasted into one field, run together ("…/a.ziphttps://…") or with a
/// space between them. A scheme after `=`, `/`, `?`, `&`, `#` or
/// `%` is part of the first address — a redirect's parameter, or an
/// archive's copy of a page — and is not counted.
pub fn two_addresses(text: &str) -> bool {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let schemes = SCHEMES;
    let starts = |at: usize| {
        lower
            .get(at..)
            .is_some_and(|rest| schemes.iter().any(|s| rest.starts_with(s)))
    };
    if text.split_whitespace().count() > 1
        && text
            .split_whitespace()
            .skip(1)
            .any(|w| looks_downloadable(w).is_some())
    {
        return true;
    }
    lower.char_indices().skip(1).any(|(at, _)| {
        starts(at)
            && lower[..at]
                .chars()
                .next_back()
                .is_some_and(|before| !"=/?&#%".contains(before))
    })
}

/// Links inside some text: a dropped `.webloc` or `.url` file, a list, the
/// clipboard.
/// Words are split on white space, quotes and angle brackets only — never
/// on `=`, which every query string holds: splitting there queued
/// `download.php?id=5` as `download.php?id`. A word that is not
/// an address itself is looked into for one that starts after `=`, `;`,
/// `(` or `[`: a `.url` file's `URL=` or `BASEURL=`, a page's unquoted
/// `href=` or `data-src=`, a refresh's `0;url=`, a forum's `[url=…]`. And
/// `&amp;` — how a page or a `.webloc` plist writes `&` — is read as `&`.
pub fn links_in(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || "<>\"'".contains(c)) {
        let word = sentence_trimmed(&address_in(word).replace("&amp;", "&")).to_string();
        if looks_downloadable(&word).is_some() && !found.contains(&word) {
            found.push(word);
        }
    }
    found
}

/// An address without the sentence around it: a full stop, comma or
/// other mark after it, and a `)` or `]` that closes a bracket opened
/// before it. "Get http://h/e.zip." fetched `e.zip.`. A
/// bracket the address opened itself — an IPv6 host's — stays.
pub fn sentence_trimmed(address: &str) -> &str {
    let mut address = address;
    loop {
        let before = address.len();
        address = address.trim_end_matches(['.', ',', ';', ':', '!', '?']);
        for (open, close) in [('(', ')'), ('[', ']')] {
            if address.ends_with(close)
                && address.matches(open).count() < address.matches(close).count()
            {
                address = &address[..address.len() - 1];
            }
        }
        if address.len() == before {
            return address;
        }
    }
}

/// The address in `word`: all of it where it starts with one, else from
/// the first scheme after `=`, `;`, `(` or `[` — up to the `]` or `)`
/// that closes that bracket. The word itself where there is none.
pub fn address_in(word: &str) -> &str {
    if looks_downloadable(word).is_some() {
        return word;
    }
    let lower = word.to_ascii_lowercase();
    let schemes = SCHEMES;
    let start = word.char_indices().find_map(|(at, c)| {
        let next = at + c.len_utf8();
        ("=;([".contains(c)
            && lower
                .get(next..)
                .is_some_and(|rest| schemes.iter().any(|s| rest.starts_with(s))))
        .then_some((next, c))
    });
    let Some((at, opener)) = start else {
        return word;
    };
    let address = &word[at..];
    let closer = match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        // `[url=…]`: the bracket opened before the `=`.
        _ if word[..at].contains('[') => Some(']'),
        _ => None,
    };
    let Some(close) = closer else {
        return address;
    };
    // Up to the bracket that closes the one before the address; brackets
    // opened inside it (an IPv6 host's) are its own.
    let open = if close == ')' { '(' } else { '[' };
    let mut depth = 0usize;
    for (at, c) in address.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return &address[..at];
            }
            depth -= 1;
        }
    }
    address
}

/// How much of a dropped file is looked at for links.
pub const DROP_LOOK: u64 = 64 * 1024;

/// The kinds of file a drop is read for links: a browser's link file, a
/// list, a page (a `.torrent` is taken whole, as the download itself).
/// Anything else — a PDF, an ISO, a photo — is not a list of links, and
/// the addresses that happen to be inside one are not what the person
/// dropping it wanted queued.
pub const DROP_KINDS: [&str; 6] = ["webloc", "url", "txt", "text", "html", "htm"];

/// Links in a dropped browser item: the address itself where a drag source
/// hands one over as the path, else the first 64 KB of a text-like file.
/// Read from the path, and only that much: the whole file used to be read,
/// on the window's thread, to look at the start of it, and a dropped 4 GB
/// image held the window until it had. The file's bytes as the drop hands
/// them over are the fallback where the path cannot be opened.
pub fn links_at(path: &std::path::Path, bytes: impl FnOnce() -> Option<Vec<u8>>) -> Vec<String> {
    use std::io::Read;
    let text = path.to_string_lossy();
    if text.contains("://") || text.starts_with("magnet:?") {
        return links_in(&text);
    }
    let kind = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    // A .torrent is itself the download, read from where it lies: a drop
    // of one was ignored.
    if kind.as_deref() == Some("torrent") && path.is_file() {
        return std::path::absolute(path)
            .ok()
            .and_then(|full| dm_core::file_url(&full))
            .into_iter()
            .collect();
    }
    if kind
        .as_deref()
        .is_some_and(|kind| !DROP_KINDS.contains(&kind))
    {
        return Vec::new();
    }
    let mut head = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|f| f.take(DROP_LOOK).read_to_end(&mut head))
        .is_ok();
    if !read {
        head = bytes().unwrap_or_default();
        head.truncate(DROP_LOOK as usize);
    }
    // A binary plist (a .webloc as some Macs write it) keeps its strings
    // as plain bytes between others.
    if head.starts_with(b"bplist00") {
        let text: String = String::from_utf8_lossy(&head)
            .chars()
            .map(|c| if c.is_ascii_graphic() { c } else { ' ' })
            .collect();
        return links_in(&text);
    }
    // A file with no extension is read only if it looks like text.
    if head.contains(&0) {
        return Vec::new();
    }
    links_in_file(kind.as_deref(), &String::from_utf8_lossy(&head))
}

/// The links a file of this kind is for. A `.webloc` is the one address
/// under its `URL` key, not the address of the plist format its first lines
/// name (Apple's DTD was queued beside the link); a `.url` is its `URL=`
/// line, not its icon's address. Anything else is read for every link.
pub fn links_in_file(kind: Option<&str>, text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    if kind == Some("webloc") || lower.contains("<plist") {
        let value = lower.find("<key>url</key>").and_then(|key| {
            let rest = &text[key..];
            let open = rest.find("<string>")? + "<string>".len();
            let close = rest[open..].find("</string>")?;
            Some(&rest[open..open + close])
        });
        if let Some(value) = value {
            return links_in(value);
        }
    }
    if kind == Some("url") || lower.contains("[internetshortcut]") {
        let line = text.lines().find(|l| {
            l.trim_start()
                .get(..4)
                .is_some_and(|k| k.eq_ignore_ascii_case("url="))
        });
        if let Some(line) = line {
            return links_in(line);
        }
    }
    links_in(text)
}

/// The clipboard watch's memory: offered once, never again once dismissed,
/// and nothing offered for what was copied before it was turned on. A
/// presentation reads the system's clipboard and hands the text over; what
/// to offer is decided here.
#[derive(Debug, Default)]
pub struct Clipboard {
    /// Whether the watch is on.
    pub on: bool,
    /// The last text seen on the clipboard.
    pub last: Option<String>,
    /// The address now offered to the person, if any.
    pub offered: Option<String>,
    /// Every link offered, when the clipboard holds several and nothing
    /// else: only the first was offered and the rest dropped.
    pub offered_all: Vec<String>,
    /// The addresses the person turned away.
    pub dismissed: std::collections::HashSet<String>,
    /// When the clipboard was last read.
    pub polled: Option<std::time::Instant>,
}

/// What one look at the clipboard found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Looked {
    /// Nothing new: the watch is off, or the text is what it was.
    Nothing,
    /// Links to take at once, where the person asked for that ("start
    /// copied"): one link's Download details, or the list of several, with
    /// no offer first. They count as dismissed, so are not offered again.
    Take(Vec<String>),
    /// Offered (`offered`, `offered_all`), or the offer withdrawn where the
    /// new text holds no link.
    Offered,
}

impl Clipboard {
    /// Turns the watch on or off. Turned on, it is seeded with what the
    /// clipboard holds now (`now`), so a link copied long ago for another
    /// reason is not offered; turned off, nothing stays offered.
    pub fn turn(&mut self, on: bool, now: impl FnOnce() -> Option<String>) {
        self.on = on;
        if !on {
            self.offered = None;
            self.offered_all.clear();
            return;
        }
        self.last = now();
        self.polled = Some(std::time::Instant::now());
    }

    /// One look at the clipboard's `text`, with the list as it stands
    /// (`rows`: an address already a row is not offered) and whether copied
    /// links start at once (`start_copied`).
    pub fn look(
        &mut self,
        text: Option<String>,
        rows: &[std::sync::Arc<dm_core::Entry>],
        start_copied: bool,
    ) -> Looked {
        if !self.on {
            return Looked::Nothing;
        }
        self.polled = Some(std::time::Instant::now());
        if text == self.last {
            return Looked::Nothing;
        }
        self.last.clone_from(&text);
        let links = clipboard_offer(text.as_deref(), &self.dismissed, rows);
        if start_copied && !links.is_empty() {
            self.dismissed.extend(links.iter().cloned());
            self.offered = None;
            self.offered_all.clear();
            return Looked::Take(links);
        }
        self.offered = links.first().cloned();
        self.offered_all = links;
        Looked::Offered
    }

    /// The person turned the offer away (or took it): nothing is offered,
    /// and none of what was offered is offered again.
    pub fn dismiss_all(&mut self) {
        self.dismissed.extend(self.offered_all.iter().cloned());
        if let Some(url) = self.offered.take() {
            self.dismissed.insert(url);
        }
        self.offered_all.clear();
    }
}

/// What the clipboard offers to download: text that is links and nothing
/// else, one or several (each once), or else the one link a text starts with;
/// a sentence with a link in it is not an offer. A link the person dismissed,
/// or one already a row's, is not offered again.
pub fn clipboard_offer(
    text: Option<&str>,
    dismissed: &std::collections::HashSet<String>,
    rows: &[std::sync::Arc<dm_core::Entry>],
) -> Vec<String> {
    let fresh = |url: &String| !dismissed.contains(url) && !rows.iter().any(|r| &r.url == url);
    let words: Vec<&str> = text.unwrap_or("").split_whitespace().collect();
    if !words.is_empty() && words.iter().all(|w| looks_downloadable(w).is_some()) {
        let mut links: Vec<String> = Vec::new();
        for w in words {
            if !links.iter().any(|l| l == w) {
                links.push(w.to_string());
            }
        }
        links.into_iter().filter(fresh).collect()
    } else {
        text.and_then(looks_downloadable)
            .filter(fresh)
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod clipboard_tests {
    use super::*;

    #[test]
    fn the_watch_offers_what_is_new_once_and_nothing_copied_before() {
        let mut watch = Clipboard::default();
        assert_eq!(
            watch.look(Some("https://e.com/a.zip".into()), &[], false),
            Looked::Nothing
        );
        // Seeded with what was there when it was turned on.
        watch.turn(true, || Some("https://e.com/old.zip".into()));
        assert_eq!(
            watch.look(Some("https://e.com/old.zip".into()), &[], false),
            Looked::Nothing
        );
        assert_eq!(
            watch.look(Some("https://e.com/a.zip".into()), &[], false),
            Looked::Offered
        );
        assert_eq!(watch.offered.as_deref(), Some("https://e.com/a.zip"));
        watch.dismiss_all();
        assert_eq!(watch.offered, None);
        // Copied again after something else: dismissed, so not offered.
        watch.look(Some("words".into()), &[], false);
        watch.look(Some("https://e.com/a.zip".into()), &[], false);
        assert_eq!(watch.offered, None);
        // Taken at once where copied links start themselves.
        assert_eq!(
            watch.look(Some("https://e.com/b.zip".into()), &[], true),
            Looked::Take(vec!["https://e.com/b.zip".into()])
        );
        assert!(watch.dismissed.contains("https://e.com/b.zip"));
        watch.turn(false, || None);
        assert!(!watch.on && watch.offered.is_none());
    }
}
