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
    // pause, where a `.bin` of the same size waits for its danger check.
    // A cache hit looks no different. Handed over, dm fetches it
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

/// Downloads whose file name Chrome is waiting on. Chrome picks a
/// download's name, and asks where to save it when it must (a name too long
/// for the disk, or "Ask where to save each file" on), only once every
/// `onDeterminingFilename` listener has answered. So the answer is held
/// until dm has decided: one dm takes is cancelled before any Save dialog
/// can open, and one it leaves is answered at once, as if dm were not there.
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
/// too, while it runs. The extension cannot tell from here
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

/// Where the panel goes moved by `by` from `spot` (where `panel_at` puts
/// it), kept whole inside the viewport.
pub fn dragged(
    spot: (f64, f64),
    by: (f64, f64),
    panel: (f64, f64),
    viewport: (f64, f64),
) -> (f64, f64) {
    let margin = 8.0;
    let clamp = |v: f64, size: f64, room: f64| {
        let most = (room - size - margin).max(margin);
        if v.is_finite() {
            v.clamp(margin, most)
        } else {
            margin
        }
    };
    (
        clamp(spot.0 + by.0, panel.0, viewport.0),
        clamp(spot.1 + by.1, panel.1, viewport.1),
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

/// What the panel says over a copy-protected player (one the page plays
/// through the browser's DRM, EME): offered nothing, and said so, rather
/// than handing over something that could only fail or come out scrambled.
pub const PROTECTED_TITLE: &str = "Protected video";
pub const PROTECTED_SAID: &str =
    "This video is copy-protected, so only the site's own player can play it. dm does not download protected video.";

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
/// otherwise vanished from the browser without a word.
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
mod tests;
