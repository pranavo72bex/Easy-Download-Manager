// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A saved page's markup and its stylesheets (gaps row F2): the addresses in them found, and
//! rewritten to the files saved beside them. Read byte by byte, so a page in any charset that
//! keeps ASCII as ASCII comes out with every byte but the rewritten addresses as it came.

use std::ops::Range;

use url::Url;

/// What an address in a page is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Another page: followed within the Grabber's scope.
    Page,
    /// Something the page needs to be drawn: a stylesheet, a script, a picture, a font.
    Resource,
    /// Where a form sends what is typed, or a page moves on to by itself: never fetched.
    Elsewhere,
}

/// What a piece of a page holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum What {
    /// One address, the whole value of an attribute.
    Link(Kind),
    /// A `srcset`: pictures, each with its width or density.
    Srcset,
    /// A stylesheet: a `<style>` element's text or a `style` attribute.
    Css,
    /// A `<meta http-equiv="refresh">`'s `content`: a wait, then `url=` an address.
    Refresh,
    /// A `<base href>`: honoured while reading, left out of the saved page, whose links
    /// are then relative to where it is saved.
    Base,
    /// An integrity check on the tag whose address is the piece at this index: left out
    /// when that address is saved, as a file opened from the disk fails the check.
    Integrity(usize),
}

/// A place in a page that holds an address or more.
#[derive(Debug, Clone)]
struct Piece {
    /// The bytes it spans: an attribute's value with its quotes for a link, without them
    /// for the others; a whole attribute for a base or an integrity check; a `<style>`'s text.
    range: Range<usize>,
    what: What,
    /// Whether it is in an attribute, whose text has entities and whose quotes are kept.
    attribute: bool,
}

/// What a read of a page found.
#[derive(Debug, Default)]
struct Scan {
    pieces: Vec<Piece>,
    /// The first `<base href>`'s value, its entities decoded.
    base: Option<String>,
    /// Whether the page says its own charset in a `<meta>`.
    charset_said: bool,
    /// Just past `<head ...>`, where a `<meta charset>` goes.
    after_head: Option<usize>,
    /// Just past `<!doctype ...>`, where it goes in a page with no `<head>`.
    after_doctype: Option<usize>,
}

/// An attribute of a tag.
struct Attr {
    name: String,
    /// Its value with its quotes; `None` for a bare name.
    value: Option<Range<usize>>,
    /// Its value without them.
    inner: Range<usize>,
    /// The name, the `=` and the value.
    whole: Range<usize>,
}

fn at(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

fn starts_ci(b: &[u8], i: usize, word: &[u8]) -> bool {
    b.get(i..i + word.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(word))
}

/// Where `word` is next, from `from`, in any case.
fn find_ci(b: &[u8], from: usize, word: &[u8]) -> Option<usize> {
    let last = b.len().checked_sub(word.len())?;
    (from..=last).find(|&i| starts_ci(b, i, word))
}

fn find_byte(b: &[u8], from: usize, c: u8) -> Option<usize> {
    b.get(from..)?
        .iter()
        .position(|&x| x == c)
        .map(|p| p + from)
}

/// An attribute's value without the quotes around it.
fn unquote(raw: &[u8]) -> &[u8] {
    match raw {
        [q @ (b'"' | b'\''), inner @ .., last] if last == q => inner,
        [b'"' | b'\'', rest @ ..] => rest,
        _ => raw,
    }
}

fn text(b: &[u8], range: Range<usize>) -> String {
    String::from_utf8_lossy(b.get(range).unwrap_or_default()).into_owned()
}

/// An attribute's text with its character references read, for resolving the address in it.
pub fn decode_entities(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp).unwrap_or_default());
        let after = rest.get(amp + 1..).unwrap_or_default();
        let end = after.find(';').filter(|&e| e <= 10);
        let named = end.and_then(|e| {
            let name = after.get(..e)?;
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, e))
        });
        match named {
            Some((c, e)) => {
                out.push(c);
                rest = after.get(e + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A tag's attributes from `i`, just past its name, and where the tag ends.
fn attributes(b: &[u8], mut i: usize) -> (Vec<Attr>, usize) {
    let mut attrs = Vec::new();
    loop {
        while at(b, i).is_ascii_whitespace() || at(b, i) == b'/' {
            i += 1;
        }
        if i >= b.len() {
            return (attrs, b.len());
        }
        if at(b, i) == b'>' {
            return (attrs, i + 1);
        }
        let start = i;
        // The first character is the name's even when it is `=`, so a stray one moves on.
        i += 1;
        while i < b.len()
            && !matches!(at(b, i), b'=' | b'>' | b'/')
            && !at(b, i).is_ascii_whitespace()
        {
            i += 1;
        }
        let name = text(b, start..i).to_ascii_lowercase();
        let mut j = i;
        while at(b, j).is_ascii_whitespace() {
            j += 1;
        }
        if at(b, j) != b'=' {
            attrs.push(Attr {
                name,
                value: None,
                inner: i..i,
                whole: start..i,
            });
            continue;
        }
        j += 1;
        while at(b, j).is_ascii_whitespace() {
            j += 1;
        }
        let (value, inner) = match at(b, j) {
            q @ (b'"' | b'\'') => {
                let close = find_byte(b, j + 1, q).unwrap_or(b.len());
                (j..(close + 1).min(b.len()), j + 1..close)
            }
            _ => {
                let mut k = j;
                while k < b.len() && at(b, k) != b'>' && !at(b, k).is_ascii_whitespace() {
                    k += 1;
                }
                (j..k, j..k)
            }
        };
        i = value.end;
        attrs.push(Attr {
            name,
            whole: start..value.end,
            value: Some(value),
            inner,
        });
    }
}

/// What each address-holding attribute of a tag holds.
fn link_kind(tag: &str, attr: &str, rel: &str) -> Option<What> {
    let resource = Some(What::Link(Kind::Resource));
    match (tag, attr) {
        (_, "style") => Some(What::Css),
        ("a" | "area" | "iframe" | "frame", "href" | "src") => Some(What::Link(Kind::Page)),
        ("link", "href") => {
            let needed = rel.split_ascii_whitespace().any(|r| {
                matches!(
                    r,
                    "stylesheet"
                        | "icon"
                        | "apple-touch-icon"
                        | "preload"
                        | "modulepreload"
                        | "manifest"
                )
            });
            Some(What::Link(if needed { Kind::Resource } else { Kind::Page }))
        }
        ("link", "imagesrcset") | ("img" | "source", "srcset") => Some(What::Srcset),
        ("img" | "script" | "source" | "video" | "audio" | "track" | "embed" | "input", "src")
        | ("video", "poster")
        | ("object", "data")
        | ("body" | "table" | "td" | "th", "background") => resource,
        ("form", "action") | ("button" | "input", "formaction") => {
            Some(What::Link(Kind::Elsewhere))
        }
        _ => None,
    }
}

impl Scan {
    /// What one tag holds.
    fn tag(&mut self, b: &[u8], name: &str, attrs: &[Attr], end: usize) {
        let value_of = |wanted: &str| {
            attrs
                .iter()
                .find(|a| a.name == wanted)
                .map(|a| decode_entities(&text(b, a.inner.clone())).to_ascii_lowercase())
                .unwrap_or_default()
        };
        match name {
            "head" if self.after_head.is_none() => self.after_head = Some(end),
            "base" => {
                if let Some(href) = attrs.iter().find(|a| a.name == "href") {
                    if self.base.is_none() {
                        self.base = Some(decode_entities(&text(b, href.inner.clone())));
                    }
                    self.pieces.push(Piece {
                        range: href.whole.clone(),
                        what: What::Base,
                        attribute: true,
                    });
                }
                return;
            }
            "meta" => {
                let equiv = value_of("http-equiv");
                if attrs.iter().any(|a| a.name == "charset")
                    || (equiv == "content-type" && value_of("content").contains("charset"))
                {
                    self.charset_said = true;
                }
                if equiv == "refresh" {
                    if let Some(content) = attrs.iter().find(|a| a.name == "content") {
                        self.pieces.push(Piece {
                            range: content.inner.clone(),
                            what: What::Refresh,
                            attribute: true,
                        });
                    }
                }
                return;
            }
            _ => {}
        }
        let rel = value_of("rel");
        let mut link = None;
        for attr in attrs {
            let Some(value) = attr.value.clone() else {
                continue;
            };
            let Some(what) = link_kind(name, &attr.name, &rel) else {
                continue;
            };
            let range = if matches!(what, What::Link(_)) {
                value
            } else {
                attr.inner.clone()
            };
            if matches!(what, What::Link(Kind::Resource)) {
                link = Some(self.pieces.len());
            }
            self.pieces.push(Piece {
                range,
                what,
                attribute: true,
            });
        }
        if let (Some(link), Some(check)) = (link, attrs.iter().find(|a| a.name == "integrity")) {
            self.pieces.push(Piece {
                range: check.whole.clone(),
                what: What::Integrity(link),
                attribute: true,
            });
        }
    }
}

/// Every piece of a page that holds an address.
fn scan(b: &[u8]) -> Scan {
    let mut scan = Scan::default();
    let mut i = 0;
    while let Some(lt) = find_byte(b, i, b'<') {
        i = lt + 1;
        if starts_ci(b, i, b"!--") {
            i = find_ci(b, i + 3, b"-->").map_or(b.len(), |p| p + 3);
            continue;
        }
        let c = at(b, i);
        if matches!(c, b'!' | b'?' | b'/') {
            let end = find_byte(b, i, b'>').map_or(b.len(), |p| p + 1);
            if starts_ci(b, i, b"!doctype") && scan.after_doctype.is_none() {
                scan.after_doctype = Some(end);
            }
            i = end;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            continue;
        }
        let start = i;
        while at(b, i).is_ascii_alphanumeric() || matches!(at(b, i), b'-' | b':' | b'_') {
            i += 1;
        }
        let name = text(b, start..i).to_ascii_lowercase();
        let (attrs, end) = attributes(b, i);
        i = end;
        scan.tag(b, &name, &attrs, end);
        if matches!(
            name.as_str(),
            "script" | "style" | "textarea" | "title" | "xmp"
        ) {
            let close = format!("</{name}");
            let stop = find_ci(b, i, close.as_bytes()).unwrap_or(b.len());
            if name == "style" {
                scan.pieces.push(Piece {
                    range: i..stop,
                    what: What::Css,
                    attribute: false,
                });
            }
            i = stop;
        }
    }
    scan
}

/// The address pages on `page` are relative to: its `<base href>` when it has a web one.
fn base_of(scan: &Scan, page: &Url) -> Url {
    scan.base
        .as_deref()
        .and_then(|href| page.join(href.trim()).ok())
        .filter(|base| matches!(base.scheme(), "http" | "https"))
        .unwrap_or_else(|| page.clone())
}

/// `value`, an address as written, resolved: a web address without its fragment.
fn resolve(value: &str, base: &Url) -> Option<Url> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('#') {
        return None;
    }
    let mut link = base.join(value).ok()?;
    if !matches!(link.scheme(), "http" | "https") {
        return None;
    }
    link.set_fragment(None);
    Some(link)
}

/// The addresses in a `srcset`: the bytes each spans in it and its text.
fn srcset_spots(b: &[u8]) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while at(b, i).is_ascii_whitespace() || at(b, i) == b',' {
            i += 1;
        }
        let start = i;
        while i < b.len() && !at(b, i).is_ascii_whitespace() {
            i += 1;
        }
        let mut end = i;
        // A comma that ends an address ends its candidate, as one with no descriptor.
        let ended = end > start && at(b, end - 1) == b',';
        if ended {
            end -= 1;
        }
        if end > start {
            out.push((start..end, text(b, start..end)));
        }
        if !ended {
            // Past the descriptors, which run to the next comma.
            i = find_byte(b, i, b',').map_or(b.len(), |p| p + 1);
        }
    }
    out
}

/// Just past the CSS string that opens at `i`.
fn string_end(b: &[u8], i: usize) -> usize {
    let quote = at(b, i);
    let mut j = i + 1;
    while j < b.len() {
        match at(b, j) {
            b'\\' => j += 2,
            b'\n' => return j,
            c if c == quote => return j + 1,
            _ => j += 1,
        }
    }
    b.len()
}

/// The addresses in a stylesheet: each `url(...)` whole and each `@import "..."`'s string
/// with its quotes, and the address's text.
fn css_spots(b: &[u8]) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = at(b, i);
        if c == b'/' && at(b, i + 1) == b'*' {
            i = find_ci(b, i + 2, b"*/").map_or(b.len(), |p| p + 2);
            continue;
        }
        if c == b'"' || c == b'\'' {
            i = string_end(b, i);
            continue;
        }
        let word_start = i == 0 || {
            let before = at(b, i - 1);
            !(before.is_ascii_alphanumeric() || matches!(before, b'-' | b'_' | b'\\'))
        };
        if word_start && starts_ci(b, i, b"url(") {
            let mut j = i + 4;
            while at(b, j).is_ascii_whitespace() {
                j += 1;
            }
            let value = if matches!(at(b, j), b'"' | b'\'') {
                let end = string_end(b, j);
                let value = text(b, j + 1..end.saturating_sub(1).max(j + 1));
                j = end;
                while at(b, j).is_ascii_whitespace() {
                    j += 1;
                }
                value
            } else {
                let close = find_byte(b, j, b')').unwrap_or(b.len());
                let value = text(b, j..close);
                j = close;
                value.trim().to_string()
            };
            if at(b, j) == b')' {
                out.push((i..j + 1, value));
            }
            i = j + 1;
            continue;
        }
        if starts_ci(b, i, b"@import") {
            let mut j = i + 7;
            while at(b, j).is_ascii_whitespace() {
                j += 1;
            }
            if matches!(at(b, j), b'"' | b'\'') {
                let end = string_end(b, j);
                out.push((j..end, text(b, j + 1..end.saturating_sub(1).max(j + 1))));
                i = end;
                continue;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// A name for a link: every byte but letters, digits and `-._~` percent-encoded, so it is
/// safe in an attribute, a `srcset` and a CSS `url()` alike.
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The link from the saved file `from` to the saved file `to`, both given as their folders
/// and then their name under the save's folder.
pub fn relative(from: &[String], to: &[String]) -> String {
    let dirs = from.split_last().map_or(&[][..], |(_, dirs)| dirs);
    let common = dirs
        .iter()
        .zip(to)
        .take_while(|(a, b)| a == b)
        .count()
        .min(to.len().saturating_sub(1));
    let mut parts: Vec<String> =
        std::iter::repeat_n("..".to_string(), dirs.len() - common).collect();
    parts.extend(to.iter().skip(common).map(|name| encode(name)));
    parts.join("/")
}

/// What an address written as `value` becomes in a saved file: the saved file's own link (from
/// `saved`, given the address without its fragment) with the fragment kept; for one not saved,
/// its absolute address, so it never names a file that is not there. `None` leaves it as it is:
/// a fragment, a `data:` or `mailto:` address, or one already absolute and not saved.
fn new_address(value: &str, base: &Url, saved: &dyn Fn(&Url) -> Option<String>) -> Option<String> {
    let link = resolve(value, base)?;
    if let Some(local) = saved(&link) {
        let fragment = base
            .join(value.trim())
            .ok()
            .and_then(|full| full.fragment().map(|f| format!("#{f}")))
            .unwrap_or_default();
        return Some(format!("{local}{fragment}"));
    }
    if Url::parse(value.trim()).is_ok() {
        return None;
    }
    base.join(value.trim()).ok().map(|full| full.to_string())
}

/// `address` safe inside an attribute's double quotes.
fn in_attribute(address: &str) -> String {
    address.replace('&', "&amp;").replace('"', "&quot;")
}

/// `address` safe as an unquoted CSS `url()`.
fn in_css(address: &str) -> String {
    let mut out = String::with_capacity(address.len());
    for c in address.chars() {
        match c {
            '(' | ')' | '\'' | '"' | '\\' | ' ' | '\t' | '\n' | '\r' => {
                out.push_str(&format!("%{:02X}", u32::from(c)));
            }
            _ => out.push(c),
        }
    }
    out
}

/// One change to a document: the bytes it replaces and what with.
type Edit = (Range<usize>, Vec<u8>);

/// `b` with `edits` made, which do not overlap.
fn apply(b: &[u8], mut edits: Vec<Edit>) -> Vec<u8> {
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = Vec::with_capacity(b.len() + 64);
    let mut from = 0;
    for (range, with) in edits {
        if range.start < from {
            continue;
        }
        out.extend_from_slice(b.get(from..range.start).unwrap_or_default());
        out.extend_from_slice(&with);
        from = range.end;
    }
    out.extend_from_slice(b.get(from..).unwrap_or_default());
    out
}

/// A CSS address's text: in an attribute its entities read, and quotes written as entities
/// (`url(&quot;a.png&quot;)`) taken off.
fn css_text(value: &str, attribute: bool) -> String {
    if !attribute {
        return value.to_string();
    }
    decode_entities(value)
        .trim()
        .trim_matches(['"', '\''])
        .to_string()
}

/// The changes to a stylesheet's text `b` (inside an attribute when `attribute`), at `offset`.
fn css_edits(
    b: &[u8],
    offset: usize,
    attribute: bool,
    base: &Url,
    saved: &dyn Fn(&Url) -> Option<String>,
) -> Vec<Edit> {
    css_spots(b)
        .into_iter()
        .filter_map(|(range, value)| {
            let value = css_text(&value, attribute);
            let address = new_address(&value, base, saved)?;
            let mut with = format!("url({})", in_css(&address));
            if attribute {
                with = in_attribute(&with);
            }
            Some((range.start + offset..range.end + offset, with.into_bytes()))
        })
        .collect()
}

/// The addresses in a stylesheet `css` read from `base`, each once: what it needs drawn.
pub fn css_addresses(css: &[u8], base: &Url) -> Vec<Url> {
    let mut found: Vec<Url> = Vec::new();
    for (_, value) in css_spots(css) {
        if let Some(link) = resolve(&value, base) {
            if !found.contains(&link) {
                found.push(link);
            }
        }
    }
    found
}

/// A stylesheet read from `base` with each address it names that was saved (`saved` gives the
/// link to it from this stylesheet's file) made that link, and each other made absolute.
pub fn rewrite_css(css: &[u8], base: &Url, saved: &dyn Fn(&Url) -> Option<String>) -> Vec<u8> {
    apply(css, css_edits(css, 0, false, base, saved))
}

/// The addresses on a page read from `page`, each once, with what each is for; read against its
/// `<base href>` when it has one, without fragments.
pub fn addresses(html: &[u8], page: &Url) -> Vec<(Url, Kind)> {
    let scan = scan(html);
    let base = base_of(&scan, page);
    let mut found: Vec<(Url, Kind)> = Vec::new();
    let mut add = |link: Option<Url>, kind: Kind| {
        if let Some(link) = link {
            if !found.iter().any(|(l, _)| *l == link) {
                found.push((link, kind));
            }
        }
    };
    for piece in &scan.pieces {
        let raw = html.get(piece.range.clone()).unwrap_or_default();
        match piece.what {
            What::Link(kind) => {
                let value = decode_entities(&String::from_utf8_lossy(unquote(raw)));
                let value = value.as_str();
                add(resolve(value, &base), kind);
            }
            What::Srcset => {
                for (_, value) in srcset_spots(raw) {
                    add(resolve(&decode_entities(&value), &base), Kind::Resource);
                }
            }
            What::Css => {
                for (_, value) in css_spots(raw) {
                    let value = css_text(&value, piece.attribute);
                    add(resolve(&value, &base), Kind::Resource);
                }
            }
            What::Refresh => {
                if let Some((_, value)) = refresh_spot(raw) {
                    add(resolve(&decode_entities(&value), &base), Kind::Elsewhere);
                }
            }
            What::Base | What::Integrity(_) => {}
        }
    }
    found
}

/// The address in a refresh's `content` (`5; url=next.html`): its bytes and text.
fn refresh_spot(b: &[u8]) -> Option<(Range<usize>, String)> {
    let at_url = find_ci(b, 0, b"url")?;
    let mut i = at_url + 3;
    while at(b, i).is_ascii_whitespace() {
        i += 1;
    }
    if at(b, i) != b'=' {
        return None;
    }
    i += 1;
    while at(b, i).is_ascii_whitespace() || matches!(at(b, i), b'"' | b'\'') {
        i += 1;
    }
    let mut end = b.len();
    while end > i
        && (at(b, end - 1).is_ascii_whitespace() || matches!(at(b, end - 1), b'"' | b'\''))
    {
        end -= 1;
    }
    (end > i).then(|| (i..end, text(b, i..end)))
}

/// Whether `charset` is a name safe to write into a `<meta charset>`.
fn safe_charset(charset: &str) -> bool {
    !charset.is_empty()
        && charset.len() <= 40
        && charset
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':'))
}

/// A page read from `page`, made to be read from the disk: each address it names that was saved
/// (`saved` gives the link to it from this page's file, for the address without its fragment) is
/// that link with its fragment, each other web address is absolute, a `<base href>` is left out,
/// and so is an integrity check on a file now read from the disk. `charset`, the one the server
/// said, is written into the page when the page does not say its own, so it reads as it did.
/// Every other byte is as it came. A page in UTF-16, which is not ASCII underneath, is kept whole.
pub fn rewrite_html(
    html: &[u8],
    page: &Url,
    charset: Option<&str>,
    saved: &dyn Fn(&Url) -> Option<String>,
) -> Vec<u8> {
    if html.starts_with(&[0xFE, 0xFF]) || html.starts_with(&[0xFF, 0xFE]) {
        return html.to_vec();
    }
    let scan = scan(html);
    let base = base_of(&scan, page);
    let mut edits: Vec<Edit> = Vec::new();
    let mut made_local = vec![false; scan.pieces.len()];
    for (index, piece) in scan.pieces.iter().enumerate() {
        let raw = html.get(piece.range.clone()).unwrap_or_default();
        let start = piece.range.start;
        match piece.what {
            What::Link(_) => {
                let value = decode_entities(&String::from_utf8_lossy(unquote(raw)));
                let value = value.as_str();
                let local = resolve(value, &base)
                    .and_then(|link| saved(&link))
                    .is_some();
                if let Some(address) = new_address(value, &base, saved) {
                    if let Some(flag) = made_local.get_mut(index) {
                        *flag = local;
                    }
                    let with = format!("\"{}\"", in_attribute(&address));
                    edits.push((piece.range.clone(), with.into_bytes()));
                }
            }
            What::Srcset => {
                for (range, value) in srcset_spots(raw) {
                    if let Some(address) = new_address(&decode_entities(&value), &base, saved) {
                        let with = in_attribute(&address).into_bytes();
                        edits.push((range.start + start..range.end + start, with));
                    }
                }
            }
            What::Css => edits.extend(css_edits(raw, start, piece.attribute, &base, saved)),
            What::Refresh => {
                if let Some((range, value)) = refresh_spot(raw) {
                    if let Some(address) = new_address(&decode_entities(&value), &base, saved) {
                        let with = in_attribute(&address).into_bytes();
                        edits.push((range.start + start..range.end + start, with));
                    }
                }
            }
            What::Base => edits.push((piece.range.clone(), Vec::new())),
            What::Integrity(link) => {
                if made_local.get(link).copied().unwrap_or(false) {
                    edits.push((piece.range.clone(), Vec::new()));
                }
            }
        }
    }
    let has_bom = html.starts_with(&[0xEF, 0xBB, 0xBF]);
    if let Some(charset) = charset.map(str::trim).filter(|c| safe_charset(c)) {
        if !scan.charset_said && !has_bom {
            let at = scan.after_head.or(scan.after_doctype).unwrap_or(0);
            edits.push((at..at, format!("<meta charset=\"{charset}\">").into_bytes()));
        }
    }
    apply(html, edits)
}
