//! What the extension decides, with no browser in the way.
//!
//! Everything here is plain Rust: it compiles for the host and is tested
//! there with `cargo test`. The browser side (`web.rs`) only reads events into
//! these types, asks these functions, and does what they say.

// Reads what the browser and the app send: every index is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use dm_protocol::{Candidate, CatchRules, Hello, Reply, CHROME_EXTENSION_ID, EDGE_EXTENSION_ID};
use serde::Deserialize;
use serde_json::Value;

/// How long an app that answered is believed. Its settings may change, so
/// not for ever; a minute keeps a burst of downloads from starting the host
/// once each.
pub const PRESENT_FOR_MS: f64 = 60_000.0;
/// How long "no app" is believed. Short, so an app installed or registered
/// since is found without restarting the browser.
pub const ABSENT_FOR_MS: f64 = 10_000.0;
/// How long the host has to answer the handshake. Not its reply time, which
/// is milliseconds, but its start: the browser spawns it cold, and a freshly
/// installed binary can be held by the system's first-run checks.
pub const HELLO_WITHIN_MS: f64 = 10_000.0;
/// How long a host that has said hello has to send its settings before an
/// older host that does not know the question is assumed.
pub const SETTINGS_WITHIN_MS: f64 = 3_000.0;
/// How long the host has to take a download. It files it and, if the app is
/// closed, starts it; both are quick, but a slow disk is not an error.
pub const TAKEN_WITHIN_MS: f64 = 20_000.0;
/// How long a URL given back to the browser is left alone if it comes round
/// again, which it does when the extension has to restart it.
pub const RELEASED_FOR_MS: f64 = 60_000.0;

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

/// A browser's `DownloadItem`, as much of it as matters. Chrome and Firefox
/// fill different fields, and Firefox sends `null` where Chrome sends `""`,
/// so everything is optional.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: Option<f64>,
    pub url: Option<String>,
    /// Chrome only: where the redirects ended.
    pub final_url: Option<String>,
    pub referrer: Option<String>,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub file_size: Option<f64>,
    pub total_bytes: Option<f64>,
    pub state: Option<String>,
    /// Set when an extension started the download.
    pub by_extension_id: Option<String>,
    pub incognito: Option<bool>,
    /// Firefox: the container (cookie store) the download was made in.
    pub cookie_store_id: Option<String>,
}

/// A browser download that could be handed over, if the app wants it.
#[derive(Debug, Clone, PartialEq)]
pub struct Catchable {
    pub id: i64,
    /// Where the bytes come from: after redirects, where the browser says.
    pub url: String,
    /// The address the download was asked for, before any redirect.
    pub asked_url: String,
    pub referrer: String,
    pub filename: String,
    pub mime: String,
    pub size: Option<u64>,
    /// The cookie store the download was made in (Firefox's containers),
    /// whose cookies are its sign-in; `None` is the browser's default.
    pub store: Option<String>,
}

impl Catchable {
    pub fn candidate(&self) -> Candidate<'_> {
        Candidate {
            url: &self.url,
            asked_url: &self.asked_url,
            referrer: &self.referrer,
            filename: &self.filename,
            mime: &self.mime,
            size: self.size,
        }
    }
}

fn text(field: &Option<String>) -> String {
    field.as_deref().unwrap_or("").trim().to_string()
}

fn is_web(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Whether a download is one the app could be asked about at all, before
/// anyone asks it.
pub fn first_look(item: &DownloadItem, own_id: &str) -> Result<Catchable, &'static str> {
    // The one this extension started itself, giving a download back to the
    // browser: catching it again would loop.
    if item
        .by_extension_id
        .as_deref()
        .is_some_and(|by| by == own_id)
    {
        return Err("started by dm itself");
    }
    // A finished one is still a candidate. Chromium completes a small file
    // served fast, a PDF above all, before it can be held: three of four
    // 300 KB PDFs from a local server were complete before or despite the
    // pause, where a `.bin` of the same size waits for its danger check
    // (gap 103). A cache hit looks no different. Handed over, dm fetches it
    // again; the browser's copy is removed only once dm has taken it.
    if item.state.as_deref() == Some("interrupted") {
        return Err("already stopped");
    }
    // A private window's download stays private: handed over, it would sit
    // in the app's queue and history after the window had closed.
    if item.incognito == Some(true) {
        return Err("from a private window");
    }
    let id = item.id.ok_or("the browser gave it no id")? as i64;
    let asked_url = text(&item.url);
    let url = match text(&item.final_url) {
        found if !found.is_empty() => found,
        _ => asked_url.clone(),
    };
    // blob:, data: and the like are bytes a page already holds; nothing
    // outside the browser can fetch them.
    if !is_web(&url) {
        return Err("not an http(s) download");
    }
    let size = [item.file_size, item.total_bytes]
        .into_iter()
        .flatten()
        .find(|n| *n > 0.0)
        .map(|n| n as u64);
    Ok(Catchable {
        id,
        url,
        asked_url,
        referrer: text(&item.referrer),
        filename: text(&item.filename),
        mime: text(&item.mime),
        size,
        store: Some(text(&item.cookie_store_id)).filter(|s| !s.is_empty()),
    })
}

/// Whether a download the browser would not hold can still be handed over,
/// from the state the browser reports for it once the pause failed: one
/// running or finished can; one stopped or gone has nothing to hand.
pub fn unheld(state: Option<&str>) -> Result<(), &'static str> {
    match state {
        Some("in_progress") | Some("complete") => Ok(()),
        Some(_) => Err("it stopped first"),
        None => Err("it is gone from the browser's list"),
    }
}

/// What becomes of the browser's copy once the app has taken a download,
/// from the state the browser reports for it then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCopy {
    /// Still running or held: cancelled, which removes its partial file.
    Cancel,
    /// Finished before it could be held, or despite the hold: the file is
    /// whole in the browser's folder and is removed, or it would sit there
    /// beside dm's.
    Remove,
}

pub fn after_taken(state: Option<&str>) -> BrowserCopy {
    if state == Some("complete") {
        BrowserCopy::Remove
    } else {
        BrowserCopy::Cancel
    }
}

/// How long Chrome's choice of a download's file name is held while the app
/// is asked about it: the handshake, the settings and the hand-off, with a
/// little to spare. Past it the browser carries on whatever happens.
pub const DETERMINE_WITHIN_MS: f64 =
    HELLO_WITHIN_MS + SETTINGS_WITHIN_MS + TAKEN_WITHIN_MS + 5_000.0;
// Held for longer than the app is given to answer and to take it, or the
// browser would ask where to save a download dm was about to take.
const _: () = assert!(DETERMINE_WITHIN_MS > HELLO_WITHIN_MS + TAKEN_WITHIN_MS);

/// Downloads whose file name Chrome is waiting on (gap 122). Chrome picks a
/// download's name, and asks where to save it when it must (a name too long
/// for the disk, or "Ask where to save each file" on), only once every
/// `onDeterminingFilename` listener has answered. So the answer is held
/// until dm has decided: one dm takes is cancelled before any Save dialog
/// can open, and one it leaves is answered at once, as if dm were not there.
///
/// `T` is the browser's answer function; plain here so it can be tested.
/// Chrome may ask before or after the download's `onCreated` is decided, so
/// a decision that comes first is remembered for the question that follows.
#[derive(Debug)]
pub struct Holds<T> {
    waiting: Vec<(i64, T)>,
    decided: Vec<i64>,
}

impl<T> Default for Holds<T> {
    fn default() -> Self {
        Holds {
            waiting: Vec::new(),
            decided: Vec::new(),
        }
    }
}

/// How many decided ids are remembered for a question still to come.
const DECIDED_KEPT: usize = 64;

impl<T> Holds<T> {
    /// Chrome asks about `id`. The answer comes back to be given now when
    /// that download is already decided; otherwise it is held.
    pub fn ask(&mut self, id: i64, answer: T) -> Option<T> {
        if let Some(at) = self.decided.iter().position(|d| *d == id) {
            self.decided.remove(at);
            return Some(answer);
        }
        self.waiting.push((id, answer));
        None
    }

    /// `id` is decided: the held answer, if Chrome asked, to be given now.
    pub fn decide(&mut self, id: i64) -> Option<T> {
        match self.waiting.iter().position(|(w, _)| *w == id) {
            Some(at) => Some(self.waiting.remove(at).1),
            None => {
                if self.decided.len() >= DECIDED_KEPT {
                    self.decided.remove(0);
                }
                self.decided.push(id);
                None
            }
        }
    }

    /// `id` was taken by dm and cancelled in the browser: the held answer
    /// is dropped, never given, since there is no download left to name.
    pub fn drop_taken(&mut self, id: i64) {
        self.waiting.retain(|(w, _)| *w != id);
    }

    /// The held answer for `id` when the time to hold it ran out.
    pub fn expire(&mut self, id: i64) -> Option<T> {
        let at = self.waiting.iter().position(|(w, _)| *w == id)?;
        Some(self.waiting.remove(at).1)
    }
}

/// Whether the browser lets an extension run a script in a page at `url`,
/// so a page opened before dm was installed can be given dm's links reader
/// (gap 122). Its own pages, other extensions' and the stores' are closed
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
fn handable(url: &str) -> bool {
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

/// The name a hand-off is filed under. The host keeps only letters, digits,
/// `-` and `_`, so that is all this uses.
pub fn hand_id(prefix: &str, n: i64, now_ms: f64) -> String {
    format!("{prefix}{n}-{}", now_ms.max(0.0) as u64)
}

/// How a reason for "no app" starts when there is an app, of a protocol
/// version this extension cannot talk to: its fix is updating one side,
/// not installing or registering anything.
pub const MISMATCH: &str = "protocol mismatch: ";

/// What to say of a host whose frames carry protocol version `theirs`,
/// naming the side to update.
pub fn mismatch(theirs: Option<u64>) -> String {
    let ours = dm_protocol::PROTOCOL_VERSION;
    let said = match theirs {
        Some(v) if v > ours => format!(
            "The dm app is newer than this extension (protocol {v}, the extension {ours}): \
             update the dm extension in this browser"
        ),
        Some(v) => format!(
            "The dm app is too old for this extension (protocol {v}, the extension \
             {ours}): update the dm app"
        ),
        None => "The dm app is too old for this extension (it says no protocol \
                 version): update the dm app"
            .to_string(),
    };
    format!("{MISMATCH}{said}")
}

/// Why a download the browser holds is not handed to an app that answered:
/// one older than [`dm_protocol::JUDGES_PAGES_SINCE`] ignores what the
/// browser said the download was, takes a sign-in page for the file, and
/// the browser's copy is erased. The menus and the panel still hand it
/// what a person asks for, which the browser has not started.
pub const OLD_APP: &str = "This dm app is too old to take downloads the browser started: \
                           update the dm app. Until then the browser keeps them.";

/// Whether a download the browser holds may be handed to this app.
pub fn takes_caught(hello: &Hello) -> Result<(), &'static str> {
    if hello.judges_pages() {
        Ok(())
    } else {
        Err(OLD_APP)
    }
}

/// The toolbar popup's status line for an app that answered, and whether it
/// is a warning.
pub fn status_line(hello: &Hello) -> (String, bool) {
    let mut line = format!("Connected to dm {}", hello.engine_version);
    if !hello.judges_pages() {
        line.push_str(" · update the dm app: until then the browser keeps its downloads");
        return (line, true);
    }
    if hello.ytdlp == Some(false) {
        line.push_str(" · update the dm app to download from video sites");
    }
    (line, false)
}

/// Whether the browser found no dm it may start: no host manifest, or one
/// that does not admit this extension. Either is fixed by dm itself, which
/// registers with every browser installed when it is opened — opened again
/// too, while it runs (gap 131). The extension cannot tell from here
/// whether dm is running: with no host there is nothing to ask.
pub fn not_connected(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    [
        // Chrome
        "host not found",
        "is forbidden",
        // Firefox
        "no such native application",
        "does not have permission to use native application",
    ]
    .iter()
    .any(|said| reason.contains(said))
}

/// The notification for "there is no app to hand this to", with the command
/// that fixes it for this browser — or, for an app of a protocol version
/// this extension cannot talk to, which side to update.
pub fn absent_text(reason: &str, own_id: &str, firefox: bool) -> String {
    absent_said(
        reason,
        own_id,
        firefox,
        "try again",
        "get it from github.com/pranavo72bex/Easy-Download-Manager/releases",
    )
}

/// [`absent_text`] for the toolbar popup, which is read again by opening it.
pub fn absent_popup_text(reason: &str, own_id: &str, firefox: bool) -> String {
    absent_said(
        reason,
        own_id,
        firefox,
        "reopen this popup",
        "click Get the dm app below",
    )
}

fn absent_said(reason: &str, own_id: &str, firefox: bool, then: &str, get: &str) -> String {
    if let Some(said) = reason.strip_prefix(MISMATCH) {
        return said.to_string();
    }
    // The ids every registration allows need no naming.
    let known = own_id == CHROME_EXTENSION_ID || own_id == EDGE_EXTENSION_ID;
    let register = if firefox || known || own_id.is_empty() {
        "dm-desktop --register".to_string()
    } else {
        format!("dm-desktop --register {own_id}")
    };
    if not_connected(reason) {
        // Someone who added the extension from a store first has no app
        // yet: told where it is, before how to connect one.
        format!(
            "dm isn't connected to this browser yet. No dm app on this computer yet? \
             {get}, install it and open it once. Already installed: open dm once (if it's \
             already open, open it again) so it can connect, then {then}. Advanced: {register}"
        )
    } else {
        format!(
            "dm did not answer ({reason}). Open dm (if it's already open, open it again), \
             then {then}. If it still does not answer: {register}"
        )
    }
}

/// Said alongside a page sent to an app that has no yt-dlp.
pub const NO_YTDLP: &str = "Sent to dm. If this is a video page rather than a file, update the \
                            dm app: the newest gets video sites ready by itself.";

/// What the extension knows of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum App {
    Present { hello: Hello, rules: CatchRules },
    Absent { reason: String },
}

/// An answer about the app, and when it was given.
#[derive(Debug, Clone)]
pub struct Known {
    pub app: App,
    pub at_ms: f64,
}

impl Known {
    pub fn fresh(&self, now_ms: f64) -> bool {
        let lasts = match self.app {
            App::Present { .. } => PRESENT_FOR_MS,
            App::Absent { .. } => ABSENT_FOR_MS,
        };
        // A clock that went backwards proves nothing either way.
        now_ms >= self.at_ms && now_ms - self.at_ms < lasts
    }
}

/// The handshake, as the host's answers arrive.
#[derive(Debug, Default, Clone)]
pub struct Probe {
    hello: Option<Hello>,
    rules: Option<CatchRules>,
    refused: Option<String>,
}

impl Probe {
    /// Takes one frame from the host.
    pub fn absorb(&mut self, frame: &Value) {
        match Reply::from_frame(frame) {
            Ok(Reply::Hello(hello)) => self.hello = Some(hello),
            Ok(Reply::Settings(settings)) => self.rules = Some(settings.rules),
            Ok(_) => {}
            // A host of another protocol version is not one to hand things
            // to; one that sent something this side cannot read is ignored.
            Err(dm_protocol::Unreadable::Version(v)) => self.refused = Some(mismatch(v)),
            Err(_) => {}
        }
    }

    pub fn has_hello(&self) -> bool {
        self.hello.is_some()
    }

    /// The answer, once there is a whole one.
    pub fn finished(&self) -> Option<App> {
        if let Some(reason) = &self.refused {
            return Some(App::Absent {
                reason: reason.clone(),
            });
        }
        match (&self.hello, &self.rules) {
            (Some(hello), Some(rules)) => Some(App::Present {
                hello: hello.clone(),
                rules: rules.clone(),
            }),
            _ => None,
        }
    }

    /// The answer when the host stopped talking: with a hello, an app too old
    /// to know about settings, which gets the default rules; without one, no
    /// app at all.
    pub fn give_up(&self, why: &str) -> App {
        if let Some(app) = self.finished() {
            return app;
        }
        match &self.hello {
            Some(hello) => App::Present {
                hello: hello.clone(),
                rules: CatchRules::default(),
            },
            None => App::Absent {
                reason: why.to_string(),
            },
        }
    }
}

/// Whether a frame answers the hand-off filed under `id`: `Some(Ok)` taken,
/// `Some(Err)` refused with the app's reason, `None` not about it.
pub fn taken(frame: &Value, id: &str) -> Option<Result<(), String>> {
    match Reply::from_frame(frame).ok()? {
        Reply::Started { id: said } if said == id => Some(Ok(())),
        Reply::Failed { id: said, failure } if said == id => Some(Err(failure
            .map(|f| f.message)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "dm would not take it".into()))),
        _ => None,
    }
}

/// URLs given back to the browser, so the download the extension restarts
/// for them is not caught again.
#[derive(Debug, Default)]
pub struct Released {
    urls: Vec<(String, f64)>,
}

impl Released {
    pub fn remember(&mut self, url: &str, now_ms: f64) {
        self.urls.retain(|(_, at)| now_ms - *at < RELEASED_FOR_MS);
        self.urls.push((url.to_string(), now_ms));
    }

    /// Whether `url` was given back lately; forgotten once asked.
    pub fn take(&mut self, url: &str, now_ms: f64) -> bool {
        self.urls.retain(|(_, at)| now_ms - *at < RELEASED_FOR_MS);
        match self.urls.iter().position(|(u, _)| u == url) {
            Some(at) => {
                self.urls.remove(at);
                true
            }
            None => false,
        }
    }
}

/* --------------------------------------------------------- the on-page panel
 *
 * "Download video": a panel at the top right of a video or audio
 * player, raised when the pointer is over it or it starts playing, that
 * hands the media to the app. The page script draws it; what it offers, where
 * it sits and what a click hands over are decided here.
 */

/// A video or audio element the page script found.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Media {
    /// `video` or `audio`.
    pub tag: String,
    /// `currentSrc`, falling back to `src`.
    pub src: String,
    pub width: f64,
    pub height: f64,
}

/// What the panel offers for one element.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub title: &'static str,
    /// Where the file comes from, or how it is fetched: the bar's tooltip.
    pub sub: String,
    /// A player page rather than a file: its qualities are listed, so the
    /// bar carries a ▾.
    pub page: bool,
}

/// Below these a media element is decoration — an avatar, a spacer, a muted
/// background loop — and a download button over it would be noise.
pub const MIN_VIDEO: (f64, f64) = (200.0, 120.0);
pub const MIN_AUDIO: (f64, f64) = (160.0, 24.0);

/// Whether a media element gets the panel, and what it says.
pub fn offer(media: &Media) -> Option<Offer> {
    let (min_w, min_h, title) = match media.tag.to_ascii_lowercase().as_str() {
        "video" => (MIN_VIDEO.0, MIN_VIDEO.1, "Download video"),
        "audio" => (MIN_AUDIO.0, MIN_AUDIO.1, "Download this audio"),
        _ => return None,
    };
    if !(media.width >= min_w && media.height >= min_h) {
        return None;
    }
    let src = media.src.trim();
    let sub = if handable(src) {
        dm_protocol::host_of(src)
    } else {
        // A `blob:` or Media Source player: no file exists to fetch, so the
        // page goes to the app, whose yt-dlp reads it.
        "this page, through the dm app".to_string()
    };
    Some(Offer {
        title,
        sub,
        page: !handable(src),
    })
}

/// Where the panel goes: inside the element's top right corner, kept on
/// screen. All in CSS pixels of the viewport.
pub fn panel_at(
    element: (f64, f64, f64, f64),
    panel: (f64, f64),
    viewport: (f64, f64),
) -> (f64, f64) {
    let (left, top, width, _height) = element;
    let (panel_w, panel_h) = panel;
    let (view_w, view_h) = viewport;
    let margin = 8.0;
    let x = (left + width - panel_w - margin)
        .min(view_w - panel_w - margin)
        .max(margin);
    let y = (top + margin).min(view_h - panel_h - margin).max(margin);
    // A NaN from a page's odd layout lands in the corner, not off screen.
    (
        if x.is_finite() { x } else { margin },
        if y.is_finite() { y } else { margin },
    )
}

/// What the page script sends when the panel is clicked.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelClick {
    #[serde(rename = "type")]
    pub kind: String,
    pub src: String,
    /// The address of the frame the player is in.
    pub frame: String,
    /// The quality picked from the panel's list, and what the list said.
    pub quality: Option<String>,
    pub label: Option<String>,
}

/// The page script asking what qualities a player's video comes in.
pub const FORMATS_MESSAGE: &str = "dm.formats";

/// One line of the panel's list: the format and size.
pub fn choice_line(label: &str, size: Option<u64>) -> String {
    match size {
        Some(bytes) => format!("{label} — {}", human_size(bytes)),
        None => label.to_string(),
    }
}

/// Bytes the way a download list says them.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    let unit = UNITS.get(unit).copied().unwrap_or("TB");
    if value < 10.0 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

/// The message type a panel click is sent as.
pub const PANEL_MESSAGE: &str = "dm.media";

/// A page that found a player asking that the app's rules be current, so
/// the panel switch in the app's Options reaches it within a minute of
/// being changed rather than at the next download.
pub const RULES_MESSAGE: &str = "dm.rules";

/// Alt-click: a link clicked with Alt held is left to the browser.
/// The page script says so before the browser starts the download.
pub const SKIP_MESSAGE: &str = "dm.skip";

/// A click with the rules' take key held: its download goes to the app
/// whatever the types and size floor say.
pub const TAKE_MESSAGE: &str = "dm.take";

/// A click on a magnet link on a page: the torrent is the app's, as a
/// clicked `.torrent` file is. Left alone, it went to whatever the system
/// opens magnets with, which on macOS and Windows was never dm.
pub const MAGNET_MESSAGE: &str = "dm.magnet";

/// The magnet a clicked link names, if it names a torrent.
pub fn magnet_url(click: &PanelClick) -> Result<String, String> {
    if click.kind != MAGNET_MESSAGE {
        return Err("that is not a magnet link's click".into());
    }
    let url = click.src.trim();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("magnet:?") && lower.contains("xt=urn:bt") {
        Ok(url.to_string())
    } else {
        Err("that magnet link names no torrent".into())
    }
}

/// What to call a magnet in a notice: its `dn=`, or "the torrent".
pub fn magnet_name(url: &str) -> String {
    url.split(['?', '&'])
        .find_map(|part| part.strip_prefix("dn="))
        .map(|name| name.replace('+', " "))
        .map(|name| percent_decoded(&name))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "the torrent".to_string())
}

/// `%XX` escapes undone, bytes read as UTF-8 where they make it.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16);
    while let Some(&byte) = bytes.get(i) {
        match (byte, bytes.get(i + 1), bytes.get(i + 2)) {
            (b'%', Some(&h), Some(&l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push((hex(h).unwrap_or(0) * 16 + hex(l).unwrap_or(0)) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The address a click with the take key held asks the app to take.
pub fn take_url(click: &PanelClick) -> Result<String, String> {
    if click.kind != TAKE_MESSAGE {
        return Err("that is not a click to hand over".into());
    }
    web_address(&click.src)
}

/// The address an Alt-click asks to leave with the browser, if it is one a
/// download could come from.
pub fn skip_url(click: &PanelClick) -> Result<String, String> {
    if click.kind != SKIP_MESSAGE {
        return Err("that is not an Alt-click".into());
    }
    web_address(&click.src)
}

fn web_address(src: &str) -> Result<String, String> {
    let url = src.trim();
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(url.to_string())
    } else {
        Err("only a web address is a download to leave alone".into())
    }
}

/// What a panel click hands over: the file, or the page for a player that
/// has none. `tab_url` is the top page, as the browser says it.
pub fn panel_target(click: &PanelClick, tab_url: &str, private: bool) -> Result<Target, String> {
    if click.kind != PANEL_MESSAGE && click.kind != FORMATS_MESSAGE {
        return Err("that is not a message from dm's panel".into());
    }
    let menu = MenuClick {
        menu_item_id: Value::String(MENU_MEDIA.to_string()),
        src_url: Some(click.src.clone()),
        page_url: Some(tab_url.to_string()),
        frame_url: Some(click.frame.clone()).filter(|f| !f.is_empty() && f != tab_url),
        ..MenuClick::default()
    };
    menu_target(&menu, private)
}

/* ---------------------------------------------------- the panel's sites */

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
/// and is dropped once that download is done.
pub fn cookies_sent(sites: &[String], url: &str, video: bool) -> Option<bool> {
    if cookies_wanted(sites, url) {
        Some(false)
    } else {
        video.then_some(true)
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

/* ------------------------------------------------------- streams seen */

/// One header of a `webRequest` response.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Header {
    pub name: String,
    pub value: Option<String>,
}

/// A response the browser received, as `webRequest.onResponseStarted`
/// reports it. Read for media, to find a player's stream on sites no page
/// reader knows.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Seen {
    pub url: String,
    pub tab_id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub status_code: u16,
    pub response_headers: Vec<Header>,
}

/// What kind of media a stream is, which decides how the app fetches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
pub enum StreamKind {
    /// An HLS playlist: the app fetches its segments and joins them.
    Hls,
    /// A DASH manifest: the app hands it to yt-dlp, which joins picture and
    /// sound.
    Dash,
    /// A whole audio or video file.
    File,
}

/// Media a page loaded, offered in the panel beside what yt-dlp reads.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Stream {
    pub url: String,
    pub kind: StreamKind,
    pub size: Option<u64>,
    pub label: String,
}

/// A whole file smaller than this is a thumbnail, a sound effect or an ad
/// beacon, not something a person came to download.
pub const STREAM_FLOOR: u64 = 1024 * 1024;

/// The media in a response, if it is a stream or a whole media file worth
/// offering. Pieces are not: an HLS or DASH segment, and a range of a
/// longer file that does not start at its beginning (a player fetching a
/// file in parts, as YouTube does), are parts of something else.
pub fn stream_seen(seen: &Seen) -> Option<Stream> {
    if seen.tab_id < 0 || !is_web(&seen.url) || !(200..300).contains(&seen.status_code) {
        return None;
    }
    let header = |name: &str| {
        seen.response_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| h.value.as_deref())
            .map(str::trim)
    };
    let mime = header("content-type")
        .and_then(|v| v.split(';').next())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let path = seen
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = path.rsplit('/').next().unwrap_or("").to_string();
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_string())
        .unwrap_or_default();

    if mime.contains("mpegurl") || ext == "m3u8" {
        return Some(Stream {
            url: seen.url.clone(),
            kind: StreamKind::Hls,
            size: None,
            label: "HLS stream".into(),
        });
    }
    if mime.contains("dash+xml") || ext == "mpd" {
        return Some(Stream {
            url: seen.url.clone(),
            kind: StreamKind::Dash,
            size: None,
            label: "DASH stream".into(),
        });
    }

    let media = mime.starts_with("video/") || mime.starts_with("audio/");
    let segment = matches!(ext.as_str(), "ts" | "m4s" | "m4f" | "cmfv" | "cmfa")
        || mime == "video/mp2t"
        || mime.contains("iso.segment")
        // Players that ask for a file in pieces name the piece in the
        // address; the piece is not the file.
        || seen.url.contains("range=")
        || seen.url.contains("&rn=");
    if !media || segment {
        return None;
    }
    // The whole length: a range answer says it after the slash, and is the
    // file only if it starts at its first byte.
    let size = match header("content-range") {
        Some(range) => {
            let (span, total) = range.trim_start_matches("bytes").trim().split_once('/')?;
            if !span.trim().starts_with("0-") {
                return None;
            }
            total.trim().parse::<u64>().ok()
        }
        None => header("content-length").and_then(|v| v.parse::<u64>().ok()),
    };
    if size.is_some_and(|bytes| bytes < STREAM_FLOOR) {
        return None;
    }
    let format = if ext.is_empty() || ext.len() > 5 {
        mime.split('/')
            .nth(1)
            .unwrap_or("media")
            .to_ascii_uppercase()
    } else {
        ext.to_ascii_uppercase()
    };
    let kind = if mime.starts_with("audio/") {
        "audio"
    } else {
        "video"
    };
    Some(Stream {
        url: seen.url.clone(),
        kind: StreamKind::File,
        size,
        label: format!("{format} {kind} file"),
    })
}

/// Whether a response is a new page in its tab, whose streams start afresh:
/// a top-level document. A download opened in the tab (`main_frame` too,
/// but a file and not a page) leaves the page, and its streams, in place.
pub fn new_page(seen: &Seen) -> bool {
    if seen.kind != "main_frame" || seen.tab_id < 0 {
        return false;
    }
    let header = |name: &str| {
        seen.response_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| h.value.as_deref())
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    if header("content-disposition").starts_with("attachment") {
        return false;
    }
    let mime = header("content-type");
    let essence = mime.split(';').next().unwrap_or("").trim();
    essence.is_empty() || essence.contains("html") || essence.contains("xml")
}

/// What a response means for the copy of a tab's streams kept in the
/// session's storage, which outlives the worker that heard them.
#[derive(Debug, Clone, PartialEq)]
pub enum Keep {
    /// A new page: the old page's copy is removed, whatever this worker
    /// remembers — a worker woken by the navigation remembers nothing, and
    /// the copy would otherwise be offered on the new page.
    Forget,
    /// A stream this tab has not been heard to load: the kept copy is read
    /// into memory before it is added, and the merge written back.
    Add,
    /// Nothing to keep.
    Nothing,
}

/// What to do with a response, for the kept copy.
pub fn keep(seen: &Seen) -> Keep {
    if new_page(seen) {
        Keep::Forget
    } else if stream_seen(seen).is_some() {
        Keep::Add
    } else {
        Keep::Nothing
    }
}

/// Which page each tab is on, counted, so a stream heard on one page whose
/// storage read is still in flight when the tab navigates is dropped rather
/// than written back over the new page's empty list. Responses are handled
/// concurrently: an `Add` awaits the kept copy, and a `Forget` for the next
/// page can run in that wait.
#[derive(Debug, Default)]
pub struct Pages {
    by_tab: std::collections::HashMap<i64, u64>,
}

impl Pages {
    /// The page a tab is on now.
    pub fn now(&self, tab: i64) -> u64 {
        self.by_tab.get(&tab).copied().unwrap_or(0)
    }

    /// A new page, or the tab closed: whatever was begun on the old one is
    /// stale from here.
    pub fn next(&mut self, tab: i64) {
        let n = self.now(tab).wrapping_add(1);
        self.by_tab.insert(tab, n);
    }

    /// Whether work begun on page `then` of a tab is still for its page.
    pub fn still(&self, tab: i64, then: u64) -> bool {
        self.now(tab) == then
    }
}

/// The streams each tab has loaded, newest last, a few per tab.
#[derive(Default, Debug)]
pub struct Streams {
    by_tab: std::collections::HashMap<i64, Vec<Stream>>,
}

/// How many streams a tab keeps: a page that loads more is cycling through
/// renditions or adverts, and the first and latest few are what matter.
pub const STREAMS_PER_TAB: usize = 8;

impl Streams {
    /// Takes in one response: a new page in a tab forgets the old page's
    /// streams, and media is remembered once per address.
    pub fn absorb(&mut self, seen: &Seen) {
        if new_page(seen) {
            self.by_tab.remove(&seen.tab_id);
            return;
        }
        let Some(stream) = stream_seen(seen) else {
            return;
        };
        let list = self.by_tab.entry(seen.tab_id).or_default();
        if list.iter().any(|known| known.url == stream.url) {
            return;
        }
        list.push(stream);
        if list.len() > STREAMS_PER_TAB {
            // The first (a master playlist, usually) stays; the oldest of
            // the rest goes.
            list.remove(1);
        }
    }

    pub fn forget(&mut self, tab: i64) {
        self.by_tab.remove(&tab);
    }

    /// Whether this worker has heard anything of a tab since it started.
    pub fn knows(&self, tab: i64) -> bool {
        self.by_tab.contains_key(&tab)
    }

    /// A tab's streams as kept before the worker last stopped, merged with
    /// what this worker has heard of the tab since: the kept ones first (a
    /// master playlist among them), then the new, each address once, as
    /// many as a tab keeps. Merging rather than replacing means a worker
    /// woken mid-page, which first hears only a variant playlist, neither
    /// loses the master to it nor has it lost again when the two are read
    /// in the other order.
    pub fn restore(&mut self, tab: i64, kept: Vec<Stream>) {
        let heard = self.by_tab.remove(&tab).unwrap_or_default();
        let mut list: Vec<Stream> = Vec::new();
        for stream in kept.into_iter().chain(heard) {
            if list.iter().any(|known| known.url == stream.url) {
                continue;
            }
            list.push(stream);
            if list.len() > STREAMS_PER_TAB {
                list.remove(1);
            }
        }
        self.by_tab.insert(tab, list);
    }

    /// A tab's streams as remembered, newest last, for keeping.
    pub fn raw(&self, tab: i64) -> Vec<Stream> {
        self.by_tab.get(&tab).cloned().unwrap_or_default()
    }

    /// What a tab has loaded, for the panel's list: manifests first, which
    /// are whole videos, then files, largest first. Two of a kind are told
    /// apart by their file name.
    pub fn of(&self, tab: i64) -> Vec<Stream> {
        let mut list = self.by_tab.get(&tab).cloned().unwrap_or_default();
        list.sort_by_key(|s| {
            (
                s.kind == StreamKind::File,
                std::cmp::Reverse(s.size.unwrap_or(0)),
            )
        });
        let labels: Vec<String> = list.iter().map(|s| s.label.clone()).collect();
        for stream in &mut list {
            if labels.iter().filter(|l| **l == stream.label).count() > 1 {
                let name = stream
                    .url
                    .split(['?', '#'])
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect::<String>();
                if !name.is_empty() {
                    stream.label = format!("{} · {name}", stream.label);
                }
            }
        }
        list
    }
}

/// What a person is told the app took: the file's own name where the
/// browser gave one, else the last part of its address. A caught link
/// otherwise vanished from the browser without a word (UI round 13).
pub fn taken_name(filename: &str, url: &str) -> String {
    let from_file = filename.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if !from_file.is_empty() {
        return from_file.to_string();
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        url.to_string()
    } else {
        last.to_string()
    }
}

#[cfg(test)]
#[path = "../../desktop/test-support/secrets.rs"]
mod test_secrets;

#[cfg(test)]
mod tests {
    #[test]
    fn a_clicked_magnet_is_taken_and_named_by_its_dn() {
        use super::{magnet_name, magnet_url, PanelClick, MAGNET_MESSAGE};
        let click = |src: &str| PanelClick {
            kind: MAGNET_MESSAGE.into(),
            src: src.into(),
            ..PanelClick::default()
        };
        let magnet =
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Some+Linux%20ISO";
        assert_eq!(magnet_url(&click(magnet)).as_deref(), Ok(magnet));
        assert!(magnet_url(&click("magnet:?dn=nothing")).is_err());
        assert!(magnet_url(&click("https://e.com/x.torrent")).is_err());
        assert_eq!(magnet_name(magnet), "Some Linux ISO");
        assert_eq!(magnet_name("magnet:?xt=urn:btih:00"), "the torrent");
    }

    use super::taken_name;

    #[test]
    fn what_was_taken_is_named_by_its_file() {
        assert_eq!(
            taken_name("/home/a/Downloads/big.zip", "https://e.com/x"),
            "big.zip"
        );
        assert_eq!(taken_name("C:\\Users\\a\\big.zip", ""), "big.zip");
        assert_eq!(
            taken_name("", "https://e.com/files/report.pdf?x=1"),
            "report.pdf"
        );
        assert_eq!(taken_name("", "https://e.com/"), "e.com");
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn the_plain_page_script_answers_by_the_names_the_background_uses() {
        // Alt-click and the page's links are plain JavaScript, so a page
        // that forbids WebAssembly keeps them; the names must still agree.
        let script = include_str!("../../extension/content.js");
        assert!(script.contains(&format!("'{SKIP_MESSAGE}'")));
        assert!(script.contains(&format!("'{TAKE_MESSAGE}'")));
        assert!(script.contains(&format!("'{LINKS_MESSAGE}'")));
        // The keys are read from the rules as the protocol names them.
        let rules = serde_json::to_value(dm_protocol::CatchRules::default()).unwrap();
        for field in ["keepKey", "takeKey"] {
            assert!(rules.get(field).is_some(), "{field}");
            assert!(script.contains(&format!("rules.{field}")), "{field}");
        }
        // Registered before the WebAssembly is imported, not after.
        let import = script.find("import(").unwrap();
        assert!(script.find("addEventListener('click'").unwrap() < import);
        assert!(script.find("onMessage.addListener").unwrap() < import);
    }

    #[test]
    fn chromes_file_name_question_is_held_until_dm_has_decided() {
        let mut holds: Holds<&str> = Holds::default();
        // Asked first, decided after: held, then given.
        assert_eq!(holds.ask(1, "one"), None);
        assert_eq!(holds.decide(1), Some("one"));
        assert_eq!(holds.decide(1), None, "given once");
        // Decided first (left with the browser): the question that follows
        // is answered at once, and only that once.
        holds.decide(2);
        assert_eq!(holds.ask(2, "two"), Some("two"));
        assert_eq!(holds.ask(2, "again"), None);
        assert_eq!(holds.expire(2), Some("again"));
        // Taken by dm: the answer is dropped, so no Save dialog can follow.
        assert_eq!(holds.ask(3, "three"), None);
        holds.drop_taken(3);
        assert_eq!(holds.decide(3), None);
        assert_eq!(holds.expire(3), None);
        // A hold nobody decides runs out, and is answered then.
        assert_eq!(holds.ask(4, "four"), None);
        assert_eq!(holds.expire(4), Some("four"));
        // Decisions no question follows are not kept for ever.
        for id in 100..300 {
            holds.decide(id);
        }
        assert!(holds.decided.len() <= DECIDED_KEPT);
    }

    #[test]
    fn a_page_open_before_dm_gets_the_links_reader_where_the_browser_allows() {
        assert!(injectable("https://e.com/files/"));
        assert!(injectable("http://127.0.0.1:8000/odd.html"));
        assert!(injectable("file:///home/u/list.html"));
        for closed in [
            "chrome://newtab/",
            "about:blank",
            "edge://settings",
            "chrome-extension://abc/popup.html",
            "moz-extension://abc/popup.html",
            "view-source:https://e.com/",
            "https://chromewebstore.google.com/detail/x",
            "https://chrome.google.com/webstore/detail/x",
            "https://addons.mozilla.org/firefox/addon/x",
            "",
        ] {
            assert!(!injectable(closed), "{closed}");
        }
    }

    #[test]
    fn an_ftp_link_is_handed_over_from_the_menu() {
        assert!(handable("ftp://ftp.gnu.org/gnu/bash/bash-5.2.tar.gz"));
        assert!(!handable("ftps://e.com/f") && !handable("file:///etc/passwd"));
    }

    #[test]
    fn a_sites_cookies_go_with_its_downloads_only_where_switched_on() {
        let sites = with_site(&[], "files.example", true);
        assert!(cookies_wanted(&sites, "https://dl.files.example/a.zip"));
        assert!(!cookies_wanted(&sites, "https://other.example/a.zip"));
        assert!(!cookies_wanted(&[], "https://files.example/a.zip"));
        // A video page's session goes with its video, for that one alone; a
        // listed site's is kept; anything else sends none.
        let video = "https://www.video.example/watch?v=1";
        assert_eq!(cookies_sent(&[], video, true), Some(true));
        assert_eq!(
            cookies_sent(&[], "https://other.example/a.zip", false),
            None
        );
        assert_eq!(
            cookies_sent(&sites, "https://dl.files.example/a.zip", false),
            Some(false)
        );
        assert_eq!(
            cookies_sent(&sites, "https://files.example/watch", true),
            Some(false)
        );
        let header = cookie_header(&[
            BrowserCookie {
                name: "session".into(),
                value: "abc".into(),
            },
            BrowserCookie {
                name: String::new(),
                value: "dropped".into(),
            },
            BrowserCookie {
                name: "theme".into(),
                value: "dark\r\nX-Injected: 1".into(),
            },
        ]);
        assert_eq!(header, "session=abc; theme=darkX-Injected: 1");
        assert_eq!(cookie_header(&[]), "");
    }

    #[test]
    fn a_switch_turned_off_on_part_of_a_listed_site_is_off_afterwards() {
        // "Never on youtube.com" from the panel, then the popup on
        // music.youtube.com: the panel switch reads off; turned on, the
        // entry that kept it off goes, and it is said.
        let off = with_site(&[], "youtube.com", true);
        assert!(panel_off(&off, "https://music.youtube.com/watch"));
        let note = wider_note(&off, "music.youtube.com");
        let back = with_site(&off, "music.youtube.com", false);
        assert!(!panel_off(&back, "https://music.youtube.com/watch"));
        assert!(note.is_some_and(|n| n.contains("youtube.com")));
        // The same for the other two switches, whose lists read "on".
        let all = with_site(&["other.org".into()], "e.com", true);
        let all = with_site(&all, "dl.e.com", true);
        assert!(forced(&all, "https://a.dl.e.com/f"));
        let left = with_site(&all, "a.dl.e.com", false);
        assert!(!forced(&left, "https://a.dl.e.com/f"));
        assert_eq!(left, vec!["other.org"], "only what covered it went");
        let sites = with_site(&[], "e.com", true);
        assert!(!cookies_wanted(
            &with_site(&sites, "m.e.com", false),
            "https://m.e.com/f"
        ));
        // Its own entry alone is not news, and a site beside it is kept.
        assert_eq!(wider_note(&with_site(&[], "e.com", true), "e.com"), None);
        assert_eq!(
            covering(&["notme.com".into()], "me.com"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_containers_download_is_asked_its_own_containers_cookies() {
        let item: DownloadItem = serde_json::from_value(json!({
            "id": 4, "url": "https://e.com/f.zip", "state": "in_progress",
            "cookieStoreId": "firefox-container-2",
        }))
        .unwrap();
        let found = first_look(&item, "x").unwrap();
        assert_eq!(found.store.as_deref(), Some("firefox-container-2"));
        assert_eq!(
            cookie_query(&found.url, found.store.as_deref()),
            json!({ "url": "https://e.com/f.zip", "storeId": "firefox-container-2" })
        );
        // Chrome says none: the default store, not a store named "".
        assert_eq!(
            cookie_query("https://e.com/f", Some("")),
            json!({ "url": "https://e.com/f" })
        );
        assert_eq!(
            cookie_query("https://e.com/f", None),
            json!({ "url": "https://e.com/f" })
        );
    }

    #[test]
    fn a_site_whose_every_download_is_taken_covers_its_parts_and_nothing_else() {
        let all = with_site(&[], "files.example", true);
        assert!(forced(&all, "https://files.example/any.weird?x=1"));
        assert!(forced(&all, "http://dl.files.example:8080/a"));
        assert!(!forced(&all, "https://notfiles.example/a"));
        assert!(!forced(&all, "https://example/a"));
        assert!(!forced(&[], "https://files.example/a"));
        // Taken off again.
        assert!(!forced(
            &with_site(&all, "files.example", false),
            "https://files.example/a"
        ));
    }

    #[test]
    fn the_panel_is_kept_off_a_site_and_its_parts_and_let_back() {
        assert_eq!(
            site_of("https://www.Example.com:8443/watch?v=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            site_of(&format!(
                "http://user:{}@m.e.com/x",
                super::test_secrets::secret("password")
            ))
            .as_deref(),
            Some("m.e.com")
        );
        assert_eq!(site_of("file:///tmp/a.html"), None);
        let off = with_site(&[], "e.com", true);
        assert!(panel_off(&off, "https://e.com/v"));
        assert!(
            panel_off(&off, "https://m.e.com/v"),
            "a part of the site is the site"
        );
        assert!(
            !panel_off(&off, "https://notme.com/v"),
            "only the site itself"
        );
        assert!(!panel_off(&off, "https://e.com.evil.net/v"));
        let back = with_site(&off, "e.com", false);
        assert!(!panel_off(&back, "https://e.com/v"));
        assert_eq!(
            with_site(&with_site(&off, "e.com", true), "a.com", true),
            vec!["a.com", "e.com"]
        );
    }

    fn seen(url: &str, kind: &str, headers: &[(&str, &str)]) -> Seen {
        Seen {
            url: url.into(),
            tab_id: 7,
            kind: kind.into(),
            status_code: 200,
            response_headers: headers
                .iter()
                .map(|(n, v)| Header {
                    name: (*n).into(),
                    value: Some((*v).into()),
                })
                .collect(),
        }
    }

    #[test]
    fn a_pages_manifests_and_whole_media_files_are_streams() {
        let hls = stream_seen(&seen(
            "https://cdn.e.com/live/master.m3u8?token=1",
            "xmlhttprequest",
            &[("Content-Type", "application/vnd.apple.mpegurl")],
        ))
        .unwrap();
        assert_eq!(
            (hls.kind, hls.label.as_str()),
            (StreamKind::Hls, "HLS stream")
        );
        // Named by its address alone, whatever the server calls it.
        assert_eq!(
            stream_seen(&seen(
                "https://e.com/v/index.m3u8",
                "other",
                &[("Content-Type", "text/plain")]
            ))
            .map(|s| s.kind),
            Some(StreamKind::Hls)
        );
        let dash = stream_seen(&seen(
            "https://e.com/manifest",
            "xmlhttprequest",
            &[("content-type", "application/dash+xml")],
        ))
        .unwrap();
        assert_eq!(dash.kind, StreamKind::Dash);

        let file = stream_seen(&seen(
            "https://e.com/media/talk.mp4",
            "media",
            &[
                ("Content-Type", "video/mp4"),
                ("Content-Range", "bytes 0-1048575/52428800"),
            ],
        ))
        .unwrap();
        assert_eq!(file.kind, StreamKind::File);
        assert_eq!(file.size, Some(52_428_800));
        assert_eq!(file.label, "MP4 video file");
        let audio = stream_seen(&seen(
            "https://e.com/ep1",
            "media",
            &[
                ("Content-Type", "audio/mpeg"),
                ("Content-Length", "9000000"),
            ],
        ))
        .unwrap();
        assert_eq!(audio.label, "MPEG audio file");
    }

    #[test]
    fn pieces_of_something_else_are_not_streams() {
        let none = |url: &str, headers: &[(&str, &str)]| {
            stream_seen(&seen(url, "media", headers)).is_none()
        };
        // Segments of a stream.
        assert!(none(
            "https://e.com/seg-12.ts",
            &[("Content-Type", "video/mp2t")]
        ));
        assert!(none(
            "https://e.com/chunk-3.m4s",
            &[("Content-Type", "video/mp4")]
        ));
        // A range further into a file, and a player's numbered pieces.
        assert!(none(
            "https://e.com/v.mp4",
            &[
                ("Content-Type", "video/mp4"),
                ("Content-Range", "bytes 1048576-2097151/52428800")
            ]
        ));
        assert!(none(
            "https://rr1.googlevideo.com/videoplayback?itag=18&range=0-100000",
            &[("Content-Type", "video/mp4")]
        ));
        // Too small to be what a person came for.
        assert!(none(
            "https://e.com/click.mp3",
            &[("Content-Type", "audio/mpeg"), ("Content-Length", "20000")]
        ));
        // Not media, not a web address, not a success.
        assert!(none(
            "https://e.com/app.js",
            &[("Content-Type", "text/javascript")]
        ));
        assert!(none(
            "blob:https://e.com/1",
            &[("Content-Type", "video/mp4")]
        ));
        let mut refused = seen(
            "https://e.com/v.mp4",
            "media",
            &[("Content-Type", "video/mp4")],
        );
        refused.status_code = 403;
        assert!(stream_seen(&refused).is_none());
    }

    #[test]
    fn a_tab_keeps_its_streams_until_it_loads_another_page() {
        let mut streams = Streams::default();
        let manifest = seen("https://e.com/a/master.m3u8", "xmlhttprequest", &[]);
        streams.absorb(&manifest);
        streams.absorb(&manifest);
        streams.absorb(&seen("https://e.com/b/other.m3u8", "xmlhttprequest", &[]));
        streams.absorb(&seen(
            "https://e.com/v.mp4",
            "media",
            &[("Content-Type", "video/mp4"), ("Content-Length", "9000000")],
        ));
        let listed = streams.of(7);
        assert_eq!(listed.len(), 3, "the same address twice is one stream");
        assert_eq!(
            listed[0].label, "HLS stream · master.m3u8",
            "two of a kind are told apart"
        );
        assert_eq!(listed[2].kind, StreamKind::File, "manifests first");
        assert!(streams.of(8).is_empty(), "another tab's are its own");

        streams.absorb(&seen("https://e.com/next", "main_frame", &[]));
        assert!(streams.of(7).is_empty(), "a new page forgets the old one's");

        for n in 0..20 {
            streams.absorb(&seen(
                &format!("https://e.com/{n}.m3u8"),
                "xmlhttprequest",
                &[],
            ));
        }
        let kept = streams.of(7);
        assert_eq!(kept.len(), STREAMS_PER_TAB);
        assert!(
            kept.iter().any(|s| s.url.ends_with("/0.m3u8")),
            "the first is kept"
        );
        assert!(
            kept.iter().any(|s| s.url.ends_with("/19.m3u8")),
            "and the latest"
        );
    }

    #[test]
    fn a_new_page_forgets_the_kept_copy_and_a_woken_worker_merges_rather_than_overwrites() {
        // The worker slept; the tab navigated. Its memory is empty, but the
        // kept copy must go, or the next page offers the old one's video.
        let page = seen(
            "https://other.example/",
            "main_frame",
            &[("Content-Type", "text/html; charset=utf-8")],
        );
        assert_eq!(keep(&page), Keep::Forget);
        // A download opened in the tab is not a new page.
        let file = seen(
            "https://e.com/f.zip",
            "main_frame",
            &[("Content-Type", "application/zip")],
        );
        assert_eq!(keep(&file), Keep::Nothing);
        let attached = seen(
            "https://e.com/report",
            "main_frame",
            &[
                ("Content-Type", "text/html"),
                ("Content-Disposition", "attachment; filename=r.html"),
            ],
        );
        assert_eq!(keep(&attached), Keep::Nothing);
        let variant = seen("https://e.com/v/720.m3u8", "xmlhttprequest", &[]);
        assert_eq!(keep(&variant), Keep::Add);
        assert_eq!(
            keep(&seen("https://e.com/x.js", "xmlhttprequest", &[])),
            Keep::Nothing
        );

        // Woken mid-page: the master was kept before the worker slept, and
        // the first thing this worker hears is a variant.
        let master =
            stream_seen(&seen("https://e.com/master.m3u8", "xmlhttprequest", &[])).unwrap();
        let mut streams = Streams::default();
        streams.absorb(&variant);
        streams.restore(7, vec![master.clone()]);
        let raw = streams.raw(7);
        assert_eq!(raw.len(), 2, "{raw:?}");
        assert_eq!(raw[0], master, "the master is kept, and first");
        // Read twice (two responses racing the storage read): still once each.
        streams.restore(7, vec![master.clone()]);
        assert_eq!(streams.raw(7).len(), 2);
    }

    #[test]
    fn a_stream_whose_read_outlives_its_page_is_dropped() {
        let mut pages = Pages::default();
        // An Add begins on the old page and awaits the kept copy...
        let then = pages.now(7);
        // ...while the new page's Forget runs.
        pages.next(7);
        assert!(
            !pages.still(7, then),
            "the old page's Add must not write back"
        );
        assert!(pages.still(7, pages.now(7)));
        assert!(pages.still(8, 0), "another tab is untouched");
    }

    #[test]
    fn the_panels_list_says_each_quality_and_its_size_as_a_download_list_does() {
        assert_eq!(
            choice_line("MP4 1280×720", Some(47_400_000)),
            "MP4 1280×720 — 45 MB"
        );
        assert_eq!(
            choice_line("M4A audio", Some(3_300_000)),
            "M4A audio — 3.1 MB"
        );
        assert_eq!(choice_line("WEBM 3840×2160", None), "WEBM 3840×2160");
        assert_eq!(human_size(900), "900 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
        let player = offer(&Media {
            tag: "VIDEO".into(),
            src: "blob:https://www.youtube.com/abc".into(),
            width: 800.0,
            height: 450.0,
        })
        .unwrap();
        assert!(player.page, "a player page has qualities to list");
        let file = offer(&Media {
            tag: "VIDEO".into(),
            src: "https://cdn.example/clip.mp4".into(),
            width: 800.0,
            height: 450.0,
        })
        .unwrap();
        assert!(!file.page, "a file is one quality");
    }

    #[test]
    fn a_pages_links_are_listed_once_each_and_only_the_web_ones() {
        let page = "https://e.com/list.html";
        let hrefs: Vec<String> = [
            "https://e.com/a.zip",
            "https://e.com/a.zip#mirror",
            "mailto:me@e.com",
            "javascript:void(0)",
            "https://e.com/list.html#top",
            "http://cdn.e.com/b.iso",
            " https://e.com/c.pdf ",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            links_from(&hrefs, page),
            [
                "https://e.com/a.zip",
                "http://cdn.e.com/b.iso",
                "https://e.com/c.pdf"
            ]
        );
        let many: Vec<String> = (0..5000)
            .map(|n| format!("https://e.com/{n}.bin"))
            .collect();
        assert_eq!(links_from(&many, page).len(), MOST_LINKS);
        let ids: Vec<&str> = MENUS.iter().map(|(id, _, _)| *id).collect();
        assert!(ids.contains(&MENU_ALL_LINKS) && ids.contains(&MENU_SELECTED_LINKS));
    }

    #[test]
    fn an_alt_click_leaves_a_web_address_with_the_browser_and_nothing_else() {
        let click = |kind: &str, src: &str| PanelClick {
            kind: kind.into(),
            src: src.into(),
            ..PanelClick::default()
        };
        assert_eq!(
            skip_url(&click(SKIP_MESSAGE, " https://e.com/f.zip ")),
            Ok("https://e.com/f.zip".to_string())
        );
        assert!(skip_url(&click(SKIP_MESSAGE, "javascript:void(0)")).is_err());
        assert_eq!(
            take_url(&click(TAKE_MESSAGE, "https://e.com/f.zip")).as_deref(),
            Ok("https://e.com/f.zip")
        );
        assert!(take_url(&click(SKIP_MESSAGE, "https://e.com/f.zip")).is_err());
        assert!(take_url(&click(TAKE_MESSAGE, "file:///etc/passwd")).is_err());
        assert!(skip_url(&click(PANEL_MESSAGE, "https://e.com/f.zip")).is_err());
        let mut released = Released::default();
        released.remember("https://e.com/f.zip", 0.0);
        assert!(
            released.take("https://e.com/f.zip", 10.0),
            "let through once"
        );
        assert!(!released.take("https://e.com/f.zip", 20.0), "and only once");
    }

    const OWN: &str = "ownextensionid";

    fn item(value: Value) -> DownloadItem {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn a_chrome_download_item_is_read_as_chrome_sends_it() {
        let found = first_look(
            &item(json!({
                "id": 7, "url": "https://e.com/get?id=1", "finalUrl": "https://cdn.e.com/f.zip",
                "referrer": "https://e.com/page", "filename": "", "mime": "application/zip",
                "fileSize": 2048, "totalBytes": 2048, "state": "in_progress", "incognito": false,
                "danger": "safe", "exists": true,
            })),
            OWN,
        )
        .unwrap();
        assert_eq!(found.id, 7);
        assert_eq!(
            found.url, "https://cdn.e.com/f.zip",
            "where the redirects ended"
        );
        assert_eq!(found.referrer, "https://e.com/page");
        assert_eq!(found.size, Some(2048));
    }

    #[test]
    fn a_firefox_download_item_with_nulls_is_read_too() {
        // Firefox has no finalUrl, sends null for an absent referrer, and -1
        // for a size it does not know.
        let found = first_look(
            &item(json!({
                "id": 3, "url": "https://e.com/f.iso", "referrer": null,
                "filename": "/home/x/Downloads/f.iso", "mime": null,
                "fileSize": -1, "totalBytes": -1, "state": "in_progress",
            })),
            OWN,
        )
        .unwrap();
        assert_eq!(found.url, "https://e.com/f.iso");
        assert_eq!(found.referrer, "");
        assert_eq!(found.size, None);
    }

    #[test]
    fn what_dm_restarted_itself_is_never_caught_again() {
        let ours = item(json!({ "id": 1, "url": "https://e.com/f.zip", "byExtensionId": OWN }));
        assert!(first_look(&ours, OWN).is_err());
        let theirs = item(json!({ "id": 1, "url": "https://e.com/f.zip", "byExtensionId": "x" }));
        assert!(first_look(&theirs, OWN).is_ok());
    }

    /// A PDF Chromium 155 (Debian) finished before the extension could hold
    /// it, as `chrome-check.py` logged it: 300 KB served at full speed with
    /// `Content-Disposition: attachment`. It used to be left with the
    /// browser as "already finished, usually from the cache" (gap 103).
    #[test]
    fn a_pdf_chromium_finished_before_it_could_be_held_is_still_handed_over() {
        let pdf = item(json!({
            "id": 3, "url": "http://127.0.0.1:41857/files/small.pdf",
            "finalUrl": "http://127.0.0.1:41857/files/small.pdf",
            "referrer": "http://127.0.0.1:41857/page.html",
            "filename": "/tmp/browser-downloads/small (1).pdf", "mime": "application/pdf",
            "state": "complete", "bytesReceived": 300000, "totalBytes": 300000,
            "fileSize": 300000, "danger": "safe", "exists": true, "paused": false,
            "canResume": false, "incognito": false,
        }));
        let found = first_look(&pdf, OWN).unwrap();
        assert_eq!(found.mime, "application/pdf");
        assert_eq!(found.size, Some(300000));
        assert!(
            dm_protocol::CatchRules::default()
                .takes(&found.candidate(), false)
                .is_ok(),
            "a PDF is a type caught by default"
        );
        // Its pause fails (or holds nothing); it was finished, so it goes,
        // and once dm has it the browser's whole file is removed rather
        // than cancelled, which would leave it.
        assert_eq!(unheld(Some("complete")), Ok(()));
        assert_eq!(after_taken(Some("complete")), BrowserCopy::Remove);
        // One the person stopped, or erased, is not handed.
        assert!(unheld(Some("interrupted")).is_err());
        assert!(unheld(None).is_err());
        // The usual case: held, then cancelled.
        assert_eq!(unheld(Some("in_progress")), Ok(()));
        assert_eq!(after_taken(Some("in_progress")), BrowserCopy::Cancel);
        // Still never one dm restarted itself, finished or not.
        let ours = item(json!({
            "id": 1, "url": "https://e.com/f.pdf", "state": "complete", "byExtensionId": OWN,
        }));
        assert!(first_look(&ours, OWN).is_err());
    }

    #[test]
    fn stopped_private_and_non_web_downloads_stay_with_the_browser() {
        for left in [
            json!({ "id": 1, "url": "https://e.com/f.zip", "state": "interrupted" }),
            json!({ "id": 1, "url": "https://e.com/f.zip", "incognito": true }),
            json!({ "id": 1, "url": "blob:https://e.com/1234" }),
            json!({ "id": 1, "url": "data:application/zip;base64,AAAA" }),
            json!({ "id": 1, "url": "file:///etc/passwd" }),
            json!({ "url": "https://e.com/f.zip" }),
        ] {
            assert!(first_look(&item(left.clone()), OWN).is_err(), "{left}");
        }
    }

    #[test]
    fn a_link_is_handed_with_the_page_it_was_on() {
        let click = MenuClick {
            menu_item_id: json!(MENU_LINK),
            link_url: Some("https://e.com/f.zip".into()),
            page_url: Some("https://e.com/list".into()),
            ..Default::default()
        };
        assert_eq!(
            menu_target(&click, false),
            Ok(Target {
                url: "https://e.com/f.zip".into(),
                referrer: "https://e.com/list".into(),
                page: false,
            })
        );
    }

    #[test]
    fn a_magnet_link_is_handed_over_and_a_javascript_one_is_not() {
        let mut click = MenuClick {
            menu_item_id: json!(MENU_LINK),
            link_url: Some("magnet:?xt=urn:btih:abc".into()),
            ..Default::default()
        };
        assert!(menu_target(&click, false).is_ok());
        click.link_url = Some("javascript:void(0)".into());
        let refused = menu_target(&click, false).unwrap_err();
        assert!(refused.contains("javascript:"), "{refused}");
    }

    #[test]
    fn media_with_a_real_source_is_a_file_and_a_blob_source_is_its_page() {
        let mut click = MenuClick {
            menu_item_id: json!(MENU_MEDIA),
            src_url: Some("https://cdn.e.com/clip.mp4".into()),
            page_url: Some("https://e.com/watch?v=1".into()),
            ..Default::default()
        };
        let file = menu_target(&click, false).unwrap();
        assert_eq!(file.url, "https://cdn.e.com/clip.mp4");
        assert!(!file.page);

        click.src_url = Some("blob:https://e.com/5c1d".into());
        let page = menu_target(&click, false).unwrap();
        assert_eq!(page.url, "https://e.com/watch?v=1");
        assert!(page.page, "a player page goes to the app's yt-dlp");

        // In a frame, the frame is the player's page.
        click.frame_url = Some("https://player.e.com/embed/1".into());
        assert_eq!(
            menu_target(&click, false).unwrap().url,
            "https://player.e.com/embed/1"
        );
    }

    #[test]
    fn no_menu_item_hands_over_a_whole_page() {
        // Links, media and selections, but no
        // item that downloads whatever page is open.
        assert!(MENUS.iter().all(|(id, _, _)| *id != "dm-page"));
        assert!(MENUS
            .iter()
            .all(|(_, _, contexts)| !contexts.contains(&"page") || contexts.len() == 1));
    }

    #[test]
    fn a_click_on_something_else_is_refused_with_its_name() {
        let click = MenuClick {
            menu_item_id: json!("someone-elses"),
            ..Default::default()
        };
        assert!(menu_target(&click, false)
            .unwrap_err()
            .contains("someone-elses"));
    }

    #[test]
    fn a_hand_off_is_filed_under_a_plain_name() {
        let id = hand_id("b", 42, 1_700_000_000_123.0);
        assert_eq!(id, "b42-1700000000123");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn the_fix_named_is_the_one_for_this_browser() {
        // The unpacked build's id is the one --register assumes; any other
        // has to be named.
        assert!(absent_text("x", CHROME_EXTENSION_ID, false).ends_with("dm-desktop --register"));
        assert!(absent_text("x", "storeid", false).ends_with("dm-desktop --register storeid"));
        assert!(absent_text("x", "dm@localhost", true).ends_with("dm-desktop --register"));
        assert!(absent_text("timed out", "a", false).contains("(timed out)"));
    }

    /// Gap 131: with no host the extension cannot know whether dm runs, so
    /// it says what works either way — open dm, again if it is open — and
    /// no longer says to install an app the person has.
    #[test]
    fn no_host_says_open_dm_and_not_install_it() {
        for (reason, firefox) in [
            ("Specified native messaging host not found.", false),
            (
                "Access to the specified native messaging host is forbidden.",
                false,
            ),
            ("No such native application com.dm.host", true),
            (
                "This extension does not have permission to use native application com.dm.host",
                true,
            ),
        ] {
            assert!(not_connected(reason), "{reason}");
            let popup = absent_popup_text(reason, CHROME_EXTENSION_ID, firefox);
            assert!(
                popup.starts_with("dm isn't connected to this browser yet."),
                "{popup}"
            );
            assert!(
                popup.contains("if it's already open, open it again"),
                "{popup}"
            );
            assert!(popup.contains("then reopen this popup."), "{popup}");
            assert!(
                popup.ends_with("Advanced: dm-desktop --register"),
                "{popup}"
            );
            assert!(!popup.contains("Install"), "{popup}");
            let notice = absent_text(reason, CHROME_EXTENSION_ID, firefox);
            assert!(notice.contains("then try again."), "{notice}");
        }
        // A host that is there and did not answer is not "not connected":
        // Open dm can still start it.
        for reason in [
            "the dm app did not answer in time",
            "Native host has exited.",
        ] {
            assert!(!not_connected(reason), "{reason}");
            let said = absent_popup_text(reason, CHROME_EXTENSION_ID, false);
            assert!(said.starts_with("dm did not answer ("), "{said}");
        }
    }

    #[test]
    fn an_answer_about_the_app_lasts_as_long_as_it_should() {
        let present = Known {
            app: App::Present {
                hello: Hello::default(),
                rules: CatchRules::default(),
            },
            at_ms: 1000.0,
        };
        assert!(present.fresh(1000.0 + PRESENT_FOR_MS - 1.0));
        assert!(!present.fresh(1000.0 + PRESENT_FOR_MS));
        assert!(!present.fresh(999.0), "a clock that went backwards");

        let absent = Known {
            app: App::Absent { reason: "x".into() },
            at_ms: 0.0,
        };
        assert!(absent.fresh(ABSENT_FOR_MS - 1.0));
        assert!(
            !absent.fresh(ABSENT_FOR_MS),
            "an app installed since is found"
        );
    }

    fn hello() -> Value {
        json!({ "protocolVersion": 1, "type": "engine.hello", "engineVersion": "0.1.0", "ytdlp": true })
    }

    #[test]
    fn a_handshake_is_whole_with_a_hello_and_the_rules() {
        let mut probe = Probe::default();
        probe.absorb(&hello());
        assert!(probe.has_hello());
        assert_eq!(probe.finished(), None, "still waiting for the rules");
        probe.absorb(&json!({
            "protocolVersion": 1, "type": "settings.get",
            "catch": { "enabled": true, "extensions": ["bin"], "minSizeKb": 0, "skipHosts": [] },
        }));
        let Some(App::Present { hello, rules }) = probe.finished() else {
            panic!("not present")
        };
        assert_eq!(hello.ytdlp, Some(true));
        assert_eq!(rules.extensions, vec!["bin".to_string()]);
    }

    #[test]
    fn a_host_too_old_to_send_rules_gets_the_defaults_and_no_host_is_absent() {
        let mut probe = Probe::default();
        probe.absorb(&hello());
        assert_eq!(
            probe.give_up("timed out"),
            App::Present {
                hello: Hello {
                    engine_version: "0.1.0".into(),
                    ytdlp: Some(true),
                    protocol: None,
                },
                rules: CatchRules::default(),
            }
        );
        assert_eq!(
            Probe::default().give_up("Specified native messaging host not found."),
            App::Absent {
                reason: "Specified native messaging host not found.".into()
            }
        );
    }

    #[test]
    fn a_host_of_another_protocol_is_treated_as_no_host_and_names_the_side_to_update() {
        let mut probe = Probe::default();
        probe.absorb(&json!({ "protocolVersion": 3, "type": "engine.hello" }));
        let Some(App::Absent { reason }) = probe.finished() else {
            panic!("a host of another version was used")
        };
        let said = absent_text(&reason, CHROME_EXTENSION_ID, false);
        assert!(said.contains("update the dm extension"), "{said}");
        assert!(
            !said.contains("--register") && !said.contains("Install"),
            "{said}"
        );

        let mut probe = Probe::default();
        probe.absorb(&json!({ "protocolVersion": 0, "type": "engine.hello" }));
        let Some(App::Absent { reason }) = probe.finished() else {
            panic!("a host of another version was used")
        };
        let said = absent_text(&reason, CHROME_EXTENSION_ID, false);
        assert!(said.contains("update the dm app"), "{said}");
        assert!(!said.contains("--register"), "{said}");
        // No host at all is still told how to install one.
        assert!(absent_text("not found", CHROME_EXTENSION_ID, false).contains("--register"));
    }

    #[test]
    fn a_download_the_browser_holds_goes_only_to_an_app_that_judges_pages() {
        // An app of protocol 1 says nothing of its protocol in its hello.
        let mut probe = Probe::default();
        probe.absorb(&hello());
        let Some(App::Present { hello: old, .. }) = Some(probe.give_up("no rules")) else {
            unreachable!()
        };
        assert_eq!(takes_caught(&old), Err(OLD_APP));
        assert!(OLD_APP.contains("update the dm app"));
        let (line, bad) = status_line(&old);
        assert!(bad && line.contains("update the dm app"), "{line}");

        let mut probe = Probe::default();
        probe.absorb(&json!({
            "protocolVersion": 1, "type": "engine.hello", "engineVersion": "3.0.1",
            "ytdlp": false, "protocol": 2,
        }));
        let App::Present { hello: new, .. } = probe.give_up("no rules") else {
            panic!("not present")
        };
        assert_eq!(takes_caught(&new), Ok(()));
        let (line, bad) = status_line(&new);
        assert!(!bad && line.contains("update the dm app"), "{line}");
    }

    #[test]
    fn only_the_answer_about_this_hand_off_counts() {
        let started = json!({ "protocolVersion": 1, "type": "download.started", "id": "b1-2" });
        assert_eq!(taken(&started, "b1-2"), Some(Ok(())));
        assert_eq!(taken(&started, "b9-9"), None);
        let failed = json!({
            "protocolVersion": 1, "type": "download.failed", "id": "b1-2",
            "failure": { "message": "dm could not take it: disk full" },
        });
        assert_eq!(
            taken(&failed, "b1-2"),
            Some(Err("dm could not take it: disk full".into()))
        );
        let bare = json!({ "protocolVersion": 1, "type": "download.failed", "id": "b1-2" });
        assert!(matches!(taken(&bare, "b1-2"), Some(Err(m)) if !m.is_empty()));
        assert_eq!(taken(&hello(), "b1-2"), None);
    }

    #[test]
    fn a_url_given_back_is_let_through_once_and_not_for_ever() {
        let mut released = Released::default();
        released.remember("https://e.com/f.zip", 0.0);
        assert!(released.take("https://e.com/f.zip", 10.0));
        assert!(!released.take("https://e.com/f.zip", 20.0), "only once");

        released.remember("https://e.com/g.zip", 0.0);
        assert!(!released.take("https://e.com/g.zip", RELEASED_FOR_MS + 1.0));
    }

    #[test]
    fn the_menus_cover_links_media_and_the_page() {
        let contexts: Vec<&str> = MENUS
            .iter()
            .flat_map(|(_, _, c)| c.iter().copied())
            .collect();
        for needed in ["link", "video", "audio", "page"] {
            assert!(contexts.contains(&needed), "{needed}");
        }
    }

    #[test]
    fn the_panel_is_for_real_players_only() {
        let video = |w: f64, h: f64, src: &str| Media {
            tag: "VIDEO".into(),
            src: src.into(),
            width: w,
            height: h,
        };
        assert_eq!(
            offer(&video(640.0, 360.0, "https://cdn.e.com/v/clip.mp4")),
            Some(Offer {
                title: "Download video",
                sub: "cdn.e.com".into(),
                page: false,
            })
        );
        assert!(
            offer(&video(120.0, 90.0, "https://e.com/a.mp4")).is_none(),
            "an avatar got a panel"
        );
        assert!(offer(&video(f64::NAN, 400.0, "https://e.com/a.mp4")).is_none());
        let player = offer(&video(800.0, 450.0, "blob:https://www.e.com/1f0a")).unwrap();
        assert!(player.sub.contains("through the dm app"));
        let audio = Media {
            tag: "audio".into(),
            src: "https://e.com/song.mp3".into(),
            width: 300.0,
            height: 40.0,
        };
        assert_eq!(offer(&audio).map(|o| o.title), Some("Download this audio"));
        assert!(offer(&Media {
            tag: "img".into(),
            ..audio
        })
        .is_none());
    }

    #[test]
    fn the_panel_sits_in_the_players_corner_and_stays_on_screen() {
        // Inside the top right of a player in the middle of the page.
        assert_eq!(
            panel_at((100.0, 50.0, 640.0, 360.0), (220.0, 44.0), (1200.0, 800.0)),
            (512.0, 58.0)
        );
        // A player wider than the window: kept inside the window.
        let (x, _) = panel_at((0.0, 0.0, 3000.0, 400.0), (220.0, 44.0), (1200.0, 800.0));
        assert_eq!(x, 1200.0 - 220.0 - 8.0);
        // A player scrolled above the top: the panel stays visible.
        let (_, y) = panel_at((0.0, -300.0, 640.0, 360.0), (220.0, 44.0), (1200.0, 800.0));
        assert_eq!(y, 8.0);
        assert_eq!(
            panel_at((f64::NAN, 0.0, 10.0, 10.0), (1.0, 1.0), (10.0, 10.0)).0,
            8.0
        );
    }

    #[test]
    fn a_panel_click_hands_the_file_or_the_page() {
        let click = |src: &str, frame: &str| PanelClick {
            kind: PANEL_MESSAGE.into(),
            src: src.into(),
            frame: frame.into(),
            ..PanelClick::default()
        };
        let page = "https://www.e.com/watch?v=1";
        let file = panel_target(&click("https://cdn.e.com/clip.mp4", page), page, false).unwrap();
        assert_eq!(
            (file.url.as_str(), file.referrer.as_str(), file.page),
            ("https://cdn.e.com/clip.mp4", page, false)
        );
        let player = panel_target(&click("blob:https://www.e.com/x", page), page, false).unwrap();
        assert_eq!((player.url.as_str(), player.page), (page, true));
        // An embedded player hands its own frame's page, which is the one
        // that knows the video.
        let embedded = panel_target(
            &click(
                "blob:https://player.v.com/x",
                "https://player.v.com/embed/9",
            ),
            page,
            false,
        )
        .unwrap();
        assert_eq!(embedded.url, "https://player.v.com/embed/9");
        assert!(panel_target(
            &PanelClick {
                kind: "other".into(),
                ..click("https://e.com/a.mp4", "")
            },
            page,
            false
        )
        .is_err());
    }

    #[test]
    fn nothing_from_a_private_window_is_handed_over_from_a_menu_or_the_panel() {
        // As a private window's downloads are left with the browser.
        let link = MenuClick {
            menu_item_id: json!(MENU_LINK),
            link_url: Some("https://e.com/f.zip".into()),
            page_url: Some("https://e.com/list".into()),
            ..Default::default()
        };
        let said = menu_target(&link, true).unwrap_err();
        assert!(said.contains("private window"), "{said}");
        assert!(menu_target(&link, false).is_ok());
        let media = MenuClick {
            menu_item_id: json!(MENU_MEDIA),
            src_url: Some("blob:https://e.com/1".into()),
            page_url: Some("https://e.com/watch?v=1".into()),
            ..Default::default()
        };
        assert!(menu_target(&media, true).is_err());
        let click = PanelClick {
            kind: PANEL_MESSAGE.into(),
            src: "https://cdn.e.com/clip.mp4".into(),
            ..PanelClick::default()
        };
        assert!(panel_target(&click, "https://e.com/watch", true).is_err());
        assert!(panel_target(&click, "https://e.com/watch", false).is_ok());
        let formats = PanelClick {
            kind: FORMATS_MESSAGE.into(),
            ..click
        };
        assert!(panel_target(&formats, "https://e.com/watch", true).is_err());
        assert!(not_private(true).is_err() && not_private(false).is_ok());
    }

    #[test]
    fn a_site_whose_every_download_is_taken_covers_its_redirects_and_its_pages() {
        let all = with_site(&[], "github.com", true);
        // Asked for on the site, redirected (Chrome's finalUrl) to its file host.
        let redirected = first_look(
            &item(json!({
                "id": 1,
                "url": "https://github.com/o/r/releases/download/v1/f.weird",
                "finalUrl": "https://objects.githubusercontent.com/123?sig=x",
            })),
            OWN,
        )
        .unwrap();
        assert!(forced_download(&all, &redirected));
        // Started from one of its pages, served from elsewhere.
        let from_page = first_look(
            &item(json!({
                "id": 2, "url": "https://cdn.example/f.weird",
                "referrer": "https://github.com/o/r",
            })),
            OWN,
        )
        .unwrap();
        assert!(forced_download(&all, &from_page));
        let elsewhere = first_look(
            &item(json!({ "id": 3, "url": "https://cdn.example/f.weird" })),
            OWN,
        )
        .unwrap();
        assert!(!forced_download(&all, &elsewhere));
    }

    #[test]
    fn a_forced_site_still_answers_to_the_apps_switch_and_skip_list() {
        let found = first_look(
            &item(json!({
                "id": 1, "url": "https://github.com/o/r/notes.txt",
                "referrer": "https://github.com/o/r",
            })),
            OWN,
        )
        .unwrap();
        let rules = CatchRules::default();
        assert!(
            rules.catches(&found.candidate()).is_err(),
            "not a type caught"
        );
        assert!(rules.takes(&found.candidate(), true).is_ok(), "but forced");
        let off = CatchRules {
            enabled: false,
            ..CatchRules::default()
        };
        assert!(off.takes(&found.candidate(), true).is_err());
        let skipping = CatchRules {
            skip_hosts: vec!["github.com".into()],
            ..CatchRules::default()
        };
        assert!(skipping.takes(&found.candidate(), true).is_err());
    }

    #[test]
    fn the_host_answers_well_before_the_extension_stops_waiting() {
        // The host's probe and its margin for answering fit inside the wait,
        // with room left for the host to start: a late "taken" after the
        // browser was given its download back is a file downloaded twice.
        let host_ms = (dm_protocol::PROBE_WITHIN_MS + dm_protocol::ANSWER_MARGIN_MS) as f64;
        assert!(host_ms + 5_000.0 <= TAKEN_WITHIN_MS, "{host_ms}");
    }
}
