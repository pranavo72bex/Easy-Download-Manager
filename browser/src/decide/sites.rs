//! The per-site switches kept in the extension's storage, and the cookies that go with a download.

use super::*;

/// Whether downloads are caught at all, as the toolbar popup's switch sets
/// it in the extension's local storage. Absent means on.
pub const CATCHING_KEY: &str = "dm.catching";

/// The toolbar popup asking how the app is, and asking it to show itself.
pub const STATUS_MESSAGE: &str = "dm.status";
pub const OPEN_MESSAGE: &str = "dm.open";

/// Where the sites the panel is kept off are stored, in the extension's
/// local storage: read by the page script, written by its × menu and the
/// toolbar popup.
pub const PANEL_OFF_KEY: &str = "dm.panel.off";

/// The site a page is on, as a person names it: its host, without `www.`.
pub fn site_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host
    } else {
        host.split(':').next().unwrap_or(host)
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    (!host.is_empty()).then_some(host)
}

/// Whether the panel is kept off the page at `url`: its site is listed, or
/// a site it is part of (`m.e.com` under `e.com`).
pub fn panel_off(off: &[String], url: &str) -> bool {
    site_listed(off, url)
}

/// Where the sites whose every download is taken are kept, from the toolbar
/// popup: "Take every download from this site".
pub const CATCH_ALL_KEY: &str = "dm.catch.all";

/// Whether every download from the site at `url` is taken, whatever the
/// app's catch rules say about its type or size: its site is listed, or a
/// site it is part of. The app is still asked, and still refuses what only
/// the browser can fetch.
pub fn forced(all: &[String], url: &str) -> bool {
    site_listed(all, url)
}

/// Whether a download is from a site whose every download is taken: where
/// it was asked for, where it is served from or the page it came from. A
/// site's files are often served from another host (`github.com` sends them
/// to `objects.githubusercontent.com`), and the switch was turned on for the
/// site, not for its file host.
pub fn forced_download(all: &[String], found: &Catchable) -> bool {
    [&found.url, &found.asked_url, &found.referrer]
        .iter()
        .any(|url| forced(all, url))
}

/// Where the sites whose downloads the browser keeps are listed, from the
/// toolbar popup: "Pause capturing on this site". A click with the take key
/// held still hands its download over.
pub const PAUSED_KEY: &str = "dm.catch.paused";

/// Whether capturing is paused for the site at `url`: its site is listed, or
/// a site it is part of.
pub fn paused(list: &[String], url: &str) -> bool {
    site_listed(list, url)
}

/// Whether a download is from a site capturing is paused for: where it was
/// asked for, where it is served from or the page it came from.
pub fn paused_download(list: &[String], found: &Catchable) -> bool {
    [&found.url, &found.asked_url, &found.referrer]
        .iter()
        .any(|url| paused(list, url))
}

/// Where the sites whose sign-in (their cookies) goes to dm with a download
/// are kept, from the toolbar popup: "Hand over this site's sign-in". The
/// browser's `cookies` permission is asked for the first time one is
/// switched on, and only then.
pub const COOKIES_SITES_KEY: &str = "dm.cookies.sites";

/// Whether the site at `url` is one whose cookies go with its downloads.
pub fn cookies_wanted(sites: &[String], url: &str) -> bool {
    site_listed(sites, url)
}

/// Whether the browser's cookies go with a hand-off of `url`, and if so
/// whether they are for that download alone: `None` sends none; `Some(false)`
/// is a site the person listed, whose sign-in the row keeps;
/// `Some(true)` is a video page, whose session goes with every video —
/// a video site lets yt-dlp in only with it ("confirm you're not a bot") —
/// and is dropped once that download is done. With the app's "every site"
/// switch on (`everywhere`), any site's sign-in goes, for that download
/// alone, as a video page's does.
pub fn cookies_sent(sites: &[String], url: &str, video: bool, everywhere: bool) -> Option<bool> {
    if cookies_wanted(sites, url) {
        Some(false)
    } else {
        (video || everywhere).then_some(true)
    }
}

/// What `cookies.getAll` is asked for a download's sign-in: its address and,
/// where the browser named one, the cookie store it was made in. Firefox
/// keeps each container's cookies in a store of its own, and without the
/// store it answers with the default container's, which are another
/// identity's sign-in. Chrome names none outside a private window, from
/// which nothing is taken.
pub fn cookie_query(url: &str, store: Option<&str>) -> Value {
    match store.map(str::trim).filter(|s| !s.is_empty()) {
        Some(store) => serde_json::json!({ "url": url, "storeId": store }),
        None => serde_json::json!({ "url": url }),
    }
}

/// One cookie as the browser lists it.
#[derive(Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct BrowserCookie {
    pub name: String,
    pub value: String,
}

/// The `Cookie` header a browser would send from these, name=value pairs
/// joined by "; ". Nothing with a line break can be a header.
pub fn cookie_header(cookies: &[BrowserCookie]) -> String {
    cookies
        .iter()
        .filter(|c| !c.name.is_empty())
        .map(|c| format!("{}={}", c.name, c.value))
        .map(|pair| pair.replace(['\r', '\n'], ""))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether `url`'s site is in `list`, or is part of a site that is.
pub fn site_listed(list: &[String], url: &str) -> bool {
    let Some(site) = site_of(url) else {
        return false;
    };
    list.iter().any(|listed| {
        site == *listed
            || site
                .strip_suffix(listed.as_str())
                .is_some_and(|before| before.ends_with('.'))
    })
}

/// The entries of `list` that cover `site`: the site itself, and each site
/// it is part of (`youtube.com` for `music.youtube.com`). A switch reads as
/// on when any of them is listed, so turning it off has to take out all of
/// them, not only the site's own.
pub fn covering(list: &[String], site: &str) -> Vec<String> {
    list.iter()
        .filter(|listed| {
            site == listed.as_str()
                || site
                    .strip_suffix(listed.as_str())
                    .is_some_and(|before| before.ends_with('.'))
        })
        .cloned()
        .collect()
}

/// What a popup switch says when turning it off for `site` also turned it
/// off for a wider site it was set for, or `None` when only `site`'s own
/// entry went.
pub fn wider_note(list: &[String], site: &str) -> Option<String> {
    let wider: Vec<String> = covering(list, site)
        .into_iter()
        .filter(|s| s != site)
        .collect();
    (!wider.is_empty()).then(|| {
        format!(
            "That was set for all of {}, so it is changed there too",
            wider.join(" and ")
        )
    })
}

/// The list with `site` added (`off`) or taken out, sorted, once each.
/// Taking it out takes out every entry that covers it (see [`covering`]),
/// so the switch that read as on reads as off afterwards.
pub fn with_site(off: &[String], site: &str, keep_off: bool) -> Vec<String> {
    let wider = covering(off, site);
    let mut list: Vec<String> = off
        .iter()
        .filter(|s| s.as_str() != site && (keep_off || !wider.contains(s)))
        .cloned()
        .collect();
    if keep_off {
        list.push(site.to_string());
    }
    list.sort();
    list.dedup();
    list.truncate(1000);
    list
}
