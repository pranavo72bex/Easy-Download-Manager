//! What a context menu click or the toolbar button hands over, and the page's links.

use super::*;

/// The context menu items: id, title, contexts.
pub const MENUS: &[(&str, &str, &[&str])] = &[
    (MENU_LINK, "Download link with dm", &["link"]),
    (MENU_MEDIA, "Download media with dm", &["video", "audio"]),
    // "Download all links" and "Download selected links".
    (MENU_ALL_LINKS, "Download all links with dm", &["page"]),
    (
        MENU_SELECTED_LINKS,
        "Download selected links with dm",
        &["selection"],
    ),
];
pub const MENU_LINK: &str = "dm-link";
pub const MENU_MEDIA: &str = "dm-media";
pub const MENU_ALL_LINKS: &str = "dm-all-links";
pub const MENU_SELECTED_LINKS: &str = "dm-selected-links";

/// What the background asks the page script for: the page's links, or only
/// those in the selection.
pub const LINKS_MESSAGE: &str = "dm.links";

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
    pub url: String,
    pub referrer: String,
    /// A page rather than a file: the app hands it to yt-dlp if it turns out
    /// to be a player page.
    pub page: bool,
}

/// A `contextMenus.OnClickData`.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct MenuClick {
    pub menu_item_id: Value,
    pub link_url: Option<String>,
    pub src_url: Option<String>,
    pub page_url: Option<String>,
    pub frame_url: Option<String>,
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

/// What a menu click hands over, or why nothing. `private` is whether the
/// tab it was in is in a private window.
pub fn menu_target(click: &MenuClick, private: bool) -> Result<Target, String> {
    not_private(private)?;
    let page = text(&click.page_url);
    let frame = text(&click.frame_url);
    let item = click.menu_item_id.as_str().unwrap_or("");
    let target = match item {
        MENU_LINK => Target {
            url: text(&click.link_url),
            referrer: page.clone(),
            page: false,
        },
        MENU_MEDIA => {
            let src = text(&click.src_url);
            if handable(&src) {
                Target {
                    url: src,
                    referrer: page.clone(),
                    page: false,
                }
            } else {
                // A `blob:` source is a Media Source player assembling the
                // video from pieces; the file does not exist anywhere to be
                // fetched. The page does, and the app's yt-dlp reads it.
                Target {
                    url: if frame.is_empty() {
                        page.clone()
                    } else {
                        frame
                    },
                    referrer: String::new(),
                    page: true,
                }
            }
        }
        other => return Err(format!("{other} is not one of dm's menu items")),
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
