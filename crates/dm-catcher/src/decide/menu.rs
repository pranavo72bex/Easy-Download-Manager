// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a context menu click or the toolbar button hands over, and the page's links.

use super::*;
use crate::words::{self, Word};

/// The context menu items: id, title, contexts. The titles are made in the
/// browser's language ([`crate::words`]).
pub const MENUS: &[(&str, Word, &[&str])] = &[
    (MENU_LINK, words::MENU_LINK, &["link"]),
    (MENU_MEDIA, words::MENU_MEDIA, &["video", "audio"]),
    // "Download all links" and "Download selected links".
    (MENU_ALL_LINKS, words::MENU_ALL_LINKS, &["page"]),
    (
        MENU_SELECTED_LINKS,
        words::MENU_SELECTED_LINKS,
        &["selection"],
    ),
];
/// The id of the context menu entry for a link.
pub const MENU_LINK: &str = "dm-link";
/// The id of the context menu entry for a video or audio element.
pub const MENU_MEDIA: &str = "dm-media";
/// The id of the context menu entry that downloads all links on a page.
pub const MENU_ALL_LINKS: &str = "dm-all-links";
/// The id of the context menu entry that downloads the selected links.
pub const MENU_SELECTED_LINKS: &str = "dm-selected-links";

/// What the background asks the page script for: the page's links, or only
/// those in the selection.
pub const LINKS_MESSAGE: &str = "dm.links";

/// What the background asks the page script for when a file's server refused the background's
/// own read of its first bytes: the same bytes, read from the page, with the page as referrer
/// (issue #56: Bilibili's `bilivideo.com` mirrors answer 403 to a request no page of theirs sent).
pub const TRACK_MESSAGE: &str = "dm.track";

/// How many links one page hands over at most: a page of ten thousand is a
/// listing nobody meant to download whole, and the dialog has to draw them.
pub const MOST_LINKS: usize = 2000;

/// The links worth listing from those a page holds: web addresses only,
/// each once, without in-page jumps to the page itself, in page order.
pub fn links_from(hrefs: &[String], page: &str) -> Vec<String> {
    let here = page.split('#').next().unwrap_or(page);
    let mut seen = std::collections::HashSet::new();
    hrefs
        .iter()
        .map(|h| h.trim())
        .filter(|h| h.starts_with("https://") || h.starts_with("http://"))
        .map(|h| h.split('#').next().unwrap_or(h).to_string())
        .filter(|h| h != here)
        .filter(|h| seen.insert(h.clone()))
        .take(MOST_LINKS)
        .collect()
}

/// The forms pages sent lately, by address: the body of the last form sent
/// to each, for a download that turns out to be its answer. Kept two minutes,
/// and twenty at most.
#[derive(Debug, Default)]
pub struct Forms {
    sent: Vec<(String, dm_protocol::Posted, f64)>,
}

/// How long a form is kept for the download it may start.
pub const FORM_KEPT_MS: f64 = 120_000.0;

/// The most of a sent form that is kept, its fields encoded or its raw body.
pub const MOST_FORM: usize = 64 * 1024;

/// What of a request's body is kept for a download that is its answer: a POST's form fields,
/// encoded as the browser encodes them, or else its raw body in base64, each at most
/// [`MOST_FORM`]; nothing for another method, a body that is empty or too large. The browser
/// hands the background the fields as lists of values under each name, in the page's order.
/// Decided here, not in the loader (gaps row 169).
pub fn posted_body(
    method: &str,
    fields: Option<&[(String, String)]>,
    raw: Option<&[Vec<u8>]>,
) -> Option<dm_protocol::Posted> {
    if method != "POST" {
        return None;
    }
    let (form, raw) = match (fields, raw) {
        (Some(fields), _) => (form_encoded(fields), String::new()),
        (None, Some(parts)) => {
            let size: usize = parts.iter().map(Vec::len).sum();
            if size == 0 || size > MOST_FORM {
                return None;
            }
            (String::new(), base64(&parts.concat()))
        }
        (None, None) => return None,
    };
    if (form.is_empty() && raw.is_empty()) || form.len() > MOST_FORM {
        return None;
    }
    Some(dm_protocol::Posted {
        form,
        raw,
        content_type: String::new(),
    })
}

/// Fields as a form sends them (`application/x-www-form-urlencoded`, as `URLSearchParams`
/// writes it): a space as `+`, letters, digits and `*-._` as they are, every other byte of the
/// UTF-8 as `%XX`.
pub fn form_encoded(fields: &[(String, String)]) -> String {
    let encode = |text: &str| {
        let mut out = String::with_capacity(text.len());
        for byte in text.bytes() {
            match byte {
                b' ' => out.push('+'),
                b'*' | b'-' | b'.' | b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => {
                    out.push(byte as char)
                }
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    };
    fields
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Bytes in standard base64, padded, as the browser's `btoa` writes them.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                let six = (n >> (18 - 6 * i) & 63) as usize;
                out.push(ALPHABET.get(six).copied().map_or('=', char::from));
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Forms {
    /// A form sent to `url` at `at` (milliseconds).
    pub fn keep(&mut self, url: &str, post: dm_protocol::Posted, at: f64) {
        let url = url.split('#').next().unwrap_or(url).to_string();
        self.sent
            .retain(|(u, _, when)| *u != url && at - *when < FORM_KEPT_MS);
        self.sent.push((url, post, at));
        if self.sent.len() > 20 {
            self.sent.remove(0);
        }
    }

    /// The form a download from `url` is the answer to, where one was sent
    /// there lately and the download was not sent on elsewhere (`final_url`):
    /// a form answered with a redirect is fetched at the redirect's address
    /// with an ordinary request. Taken, so it goes with one download.
    pub fn take(&mut self, url: &str, final_url: &str, at: f64) -> Option<dm_protocol::Posted> {
        let url = url.split('#').next().unwrap_or(url);
        let landed = final_url.split('#').next().unwrap_or(final_url);
        if !landed.is_empty() && landed != url {
            return None;
        }
        let at_index = self
            .sent
            .iter()
            .rposition(|(u, _, when)| u == url && at - *when < FORM_KEPT_MS)?;
        Some(self.sent.remove(at_index).1)
    }
}

/// The file names the links' `download` attributes give, keyed as
/// `links_from` keys their addresses: the last part of the name only (a
/// page cannot choose the folder), with no control characters, cut to 200
/// characters; a name that is nothing, `.` or `..` is none.
pub fn link_names(
    given: &std::collections::BTreeMap<String, String>,
    urls: &[String],
) -> std::collections::BTreeMap<String, String> {
    let sent: std::collections::HashSet<&str> = urls.iter().map(String::as_str).collect();
    let mut out = std::collections::BTreeMap::new();
    for (href, name) in given {
        let href = href.trim();
        let key = href.split('#').next().unwrap_or(href);
        let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
        let name: String = last
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect::<String>()
            .trim()
            .to_string();
        if !matches!(name.as_str(), "" | "." | "..") && sent.contains(key) && !out.contains_key(key)
        {
            out.insert(key.to_string(), name);
        }
    }
    out
}

/// The words of the links sent, keyed as `links_from` keys their addresses
/// (no fragment, no spaces around), their spaces closed up and cut to 200
/// characters; the first words given an address win.
pub fn link_texts(
    texts: &std::collections::BTreeMap<String, String>,
    urls: &[String],
) -> std::collections::BTreeMap<String, String> {
    let sent: std::collections::HashSet<&str> = urls.iter().map(String::as_str).collect();
    let mut out = std::collections::BTreeMap::new();
    for (href, words) in texts {
        let href = href.trim();
        let key = href.split('#').next().unwrap_or(href);
        let words: String = words
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(200)
            .collect();
        if !words.is_empty() && sent.contains(key) && !out.contains_key(key) {
            out.insert(key.to_string(), words);
        }
    }
    out
}

/// Whether the browser lets an extension run a script in a page at `url`,
/// so a page opened before dm was installed can be given dm's links reader.
/// Its own pages, other extensions' and the stores' are closed
/// to every extension.
pub fn injectable(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let web = lower.starts_with("http://") || lower.starts_with("https://");
    if !(web || lower.starts_with("file://")) {
        return false;
    }
    let host = site_of(&lower).unwrap_or_default();
    let store = host == "chromewebstore.google.com"
        || host == "addons.mozilla.org"
        || host == "microsoftedge.microsoft.com"
        || (host == "chrome.google.com" && lower.contains("/webstore"));
    !store
}

/// Why a page's links could not be read where the browser keeps dm out.
pub const NO_LINKS_HERE: &str =
    "the browser does not let extensions read this page, so dm cannot see its links";

/// What a context menu click or the toolbar button asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The address to download.
    pub url: String,
    /// The page it was found on.
    pub referrer: String,
    /// A page rather than a file: the app reads it as a video page if it turns out
    /// to be a player page.
    pub page: bool,
}

/// A `contextMenus.OnClickData`.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct MenuClick {
    /// Which menu entry was clicked.
    pub menu_item_id: Value,
    /// The link clicked, if any.
    pub link_url: Option<String>,
    /// The media element's address, if any.
    pub src_url: Option<String>,
    /// The address of the page.
    pub page_url: Option<String>,
    /// The address of the frame the click was in.
    pub frame_url: Option<String>,
    /// The text selected, if any.
    pub selection_text: Option<String>,
    /// "image", "video" or "audio", where the click was on one.
    pub media_type: Option<String>,
}

/// What the app downloads when asked: web and FTP links, and magnets.
pub(super) fn handable(url: &str) -> bool {
    is_web(url)
        || url
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("magnet:"))
        || url
            .get(..6)
            .is_some_and(|s| s.eq_ignore_ascii_case("ftp://"))
}

/// Why nothing is handed over from a private window's tab: the rule
/// [`first_look`] applies to a private window's downloads, applied to what
/// is asked for there from a menu or the panel. Handed over, it would sit in
/// the app's queue and history after the window had closed; and the
/// browser's cookies read for it would be the normal profile's, not the
/// private window's.
pub fn not_private(private: bool) -> Result<(), String> {
    if private {
        Err("dm does not take anything from a private window; the browser can download it".into())
    } else {
        Ok(())
    }
}

/// One item of the right-click menu to make: its id, its words (this extension's own, in the
/// browser's language, for an item it knows; the app's otherwise) and where it is offered.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuToMake {
    /// The item's id, which a click names.
    pub id: String,
    /// This extension's words for it, where it knows the item.
    pub known: Option<Word>,
    /// The app's words for it, said where this extension has none.
    pub title: String,
    /// Where the browser offers it.
    pub contexts: Vec<String>,
}

/// This extension's words for a menu item it knows by id.
fn known_menu(id: &str) -> Option<&'static (&'static str, Word, &'static [&'static str])> {
    MENUS.iter().find(|(known, _, _)| *known == id)
}

/// The right-click menu to make: the one the app served (`Hello::menus`), less any item whose
/// mechanism this extension does not know; or, from an app that serves none, this extension's
/// own, less the items `hidden` (Options) leaves out.
pub fn menus_to_make(
    served: Option<&[dm_protocol::MenuItem]>,
    hidden: &[String],
) -> Vec<MenuToMake> {
    match served {
        Some(items) => items
            .iter()
            .filter(|item| {
                !item.id.trim().is_empty()
                    && !item.contexts.is_empty()
                    && dm_protocol::menu_does::ALL.contains(&item.does.as_str())
            })
            .map(|item| MenuToMake {
                id: item.id.clone(),
                known: known_menu(&item.id).map(|(_, word, _)| *word),
                title: item.title.clone(),
                contexts: item.contexts.clone(),
            })
            .collect(),
        None => MENUS
            .iter()
            .filter(|(id, _, _)| !hidden.iter().any(|h| h == id))
            .map(|(id, word, contexts)| MenuToMake {
                id: (*id).to_string(),
                known: Some(*word),
                title: word.en.to_string(),
                contexts: contexts.iter().map(|c| (*c).to_string()).collect(),
            })
            .collect(),
    }
}

/// What a click on menu item `id` does (one of `dm_protocol::menu_does`): as the app served it,
/// or, from an app that serves none, as this extension's own items do. `None` for an item
/// neither knows.
pub fn menu_does_for(served: Option<&[dm_protocol::MenuItem]>, id: &str) -> Option<&'static str> {
    use dm_protocol::menu_does;
    match served {
        Some(items) => {
            let does = items.iter().find(|item| item.id == id)?.does.as_str();
            menu_does::ALL.iter().copied().find(|known| *known == does)
        }
        None => match id {
            MENU_LINK => Some(menu_does::LINK),
            MENU_MEDIA => Some(menu_does::MEDIA),
            MENU_ALL_LINKS => Some(menu_does::ALL_LINKS),
            MENU_SELECTED_LINKS => Some(menu_does::SELECTED_LINKS),
            _ => None,
        },
    }
}

/// What a click hands to the app for an item whose mechanism is the app's own
/// (`menu_does::APP`): the browser's words of the click, and the tab's title.
pub fn menu_clicked(click: &MenuClick, id: &str, title: &str) -> dm_protocol::MenuClicked {
    dm_protocol::MenuClicked {
        id: id.to_string(),
        item: click.menu_item_id.as_str().unwrap_or("").to_string(),
        link_url: text(&click.link_url),
        src_url: text(&click.src_url),
        page_url: text(&click.page_url),
        frame_url: text(&click.frame_url),
        selection_text: text(&click.selection_text).chars().take(2000).collect(),
        media_type: text(&click.media_type),
        title: title.trim().chars().take(300).collect(),
        ..dm_protocol::MenuClicked::default()
    }
}

/// The address a click for the app's own item is about, for the sign-in that goes with it.
pub fn menu_clicked_address(click: &dm_protocol::MenuClicked) -> &str {
    [
        &click.link_url,
        &click.src_url,
        &click.frame_url,
        &click.page_url,
    ]
    .into_iter()
    .find(|url| is_web(url))
    .map_or("", |url| url.as_str())
}

/// What a menu click hands over, or why nothing. `private` is whether the
/// tab it was in is in a private window.
pub fn menu_target(click: &MenuClick, private: bool) -> Result<Target, String> {
    let does = menu_does_for(None, click.menu_item_id.as_str().unwrap_or(""));
    menu_target_doing(click, does.unwrap_or(""), private)
}

/// [`menu_target`] for an item that does `does` (`menu_does::LINK` or `MEDIA`), whatever its id.
pub fn menu_target_doing(click: &MenuClick, does: &str, private: bool) -> Result<Target, String> {
    not_private(private)?;
    let page = text(&click.page_url);
    let frame = text(&click.frame_url);
    let item = click.menu_item_id.as_str().unwrap_or("");
    let target = match does {
        dm_protocol::menu_does::LINK => Target {
            url: text(&click.link_url),
            referrer: page,
            page: false,
        },
        dm_protocol::menu_does::MEDIA => {
            let src = text(&click.src_url);
            if handable(&src) {
                Target {
                    url: src,
                    referrer: page,
                    page: false,
                }
            } else {
                // A `blob:` source is a Media Source player assembling the
                // video from pieces; the file does not exist anywhere to be
                // fetched. The page does, and the app reads it.
                Target {
                    url: if frame.is_empty() { page } else { frame },
                    referrer: String::new(),
                    page: true,
                }
            }
        }
        _ => return Err(format!("{item} is not one of dm's menu items")),
    };
    if handable(&target.url) {
        Ok(target)
    } else {
        Err(unhandable(&target.url))
    }
}

fn unhandable(url: &str) -> String {
    let scheme = url.split(':').next().unwrap_or("");
    if url.is_empty() {
        "there is no address here for dm to take".into()
    } else {
        format!("dm takes http(s) and ftp links and magnets, and this is a {scheme}: address")
    }
}
