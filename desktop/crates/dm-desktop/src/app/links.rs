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
pub(super) const SCHEMES: [&str; 13] = [
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
pub(super) fn sentence_trimmed(address: &str) -> &str {
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
pub(super) fn address_in(word: &str) -> &str {
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
pub(super) const DROP_LOOK: u64 = 64 * 1024;

/// The kinds of file a drop is read for links: a browser's link file, a
/// list, a page (a `.torrent` is taken whole, as the download itself).
/// Anything else — a PDF, an ISO, a photo — is not a list of links, and
/// the addresses that happen to be inside one are not what the person
/// dropping it wanted queued.
pub(super) const DROP_KINDS: [&str; 6] = ["webloc", "url", "txt", "text", "html", "htm"];

/// Links in a dropped browser item: the address itself where a drag source
/// hands one over as the path, else the first 64 KB of a text-like file.
/// Read from the path, and only that much: the whole file used to be read,
/// on the window's thread, to look at the start of it, and a dropped 4 GB
/// image held the window until it had. The file's bytes as the drop hands
/// them over are the fallback where the path cannot be opened.
pub fn dropped_links(file: &dyn egui::DroppedFile) -> Vec<String> {
    use std::io::Read;
    let path = file.path();
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
            .and_then(|full| dm_core::torrent::file_url(&full))
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
        head = file.bytes().unwrap_or_default();
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

#[cfg(test)]
mod two_addresses_tests {
    use super::*;
    use crate::app::{as_shown, worded_problem};

    #[test]
    fn the_saved_proxy_turned_off_at_launch_is_said_in_the_windows_words() {
        let said = worded_problem(
            "the saved proxy could not be used (the proxy has no host), so it is turned off and downloads \
             go out directly. Fix it under Options › Proxy and tick it again.",
        );
        assert!(
            said.starts_with("The proxy can't be used (the proxy has no host)"),
            "{said}"
        );
        assert_eq!(worded_problem("something else"), "something else");
        assert_eq!(
            as_shown("the cookies file x could not be read"),
            "The cookies file x could not be read"
        );
        assert_eq!(as_shown("ünlü"), "Ünlü");
        assert_eq!(as_shown(""), "");
    }

    #[test]
    fn two_links_pasted_into_one_field_are_seen() {
        for two in [
            "https://e.com/a.ziphttps://e.com/b.zip",
            "https://e.com/a.zip https://e.com/b.zip",
            "http://e.com/a.zipftp://e.com/b",
            "https://e.com/a.zipmagnet:?xt=urn:btih:abc",
        ] {
            assert!(two_addresses(two), "{two}");
        }
        for one in [
            "https://e.com/a.zip",
            "https://e.com/go?to=https://f.com/b.zip",
            "https://archive.example/web/2020/https://e.com/a.zip",
            "https://e.com/r?u=https%3A//f.com&next=http://g.com",
            "magnet:?xt=urn:btih:abc&tr=http://tracker.example/announce",
        ] {
            assert!(!two_addresses(one), "{one}");
        }
    }
}
