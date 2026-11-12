// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The words the extension shows a person, in the browser's language: the
//! toolbar popup, the four context menu items and the "Download video"
//! panel, and the name and description the browser lists it under.
//!
//! Each [`Word`] is named by the key the browser's own catalog keeps it
//! under (`extension/_locales/<language>/messages.json`, read with
//! `i18n.getMessage`), and carries its English, which is what it says where
//! the browser has no catalog to give (a host test, an API missing). The
//! catalogs cover the app's languages; `tests.rs` holds every one of them to
//! the list in [`ALL`].
//!
//! A word with a gap in it (`Never on $SITE$`) names its gaps in `places`,
//! in the order they are filled; each catalog declares them as the
//! browser's placeholders, the first as `$1`.

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue};

/// One thing the extension says, by its catalog key, with its English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word {
    /// The key in each `messages.json`.
    pub key: &'static str,
    /// What it says in English, gaps written as `$NAME$`.
    pub en: &'static str,
    /// The names of its gaps, in the order they are filled.
    pub places: &'static [&'static str],
}

impl Word {
    /// In the browser's language, or the English where the browser has none.
    pub fn say(&self) -> String {
        self.with(&[])
    }

    /// In the browser's language with its gaps filled, in order, or the
    /// English filled the same way.
    pub fn with(&self, fill: &[&str]) -> String {
        #[cfg(target_arch = "wasm32")]
        if let Some(said) = from_browser(self.key, fill) {
            return said;
        }
        self.english(fill)
    }

    /// The English with its gaps filled, in one pass, so what fills a gap is
    /// never read for gaps of its own. A gap with nothing given is left empty.
    pub fn english(&self, fill: &[&str]) -> String {
        let mut out = String::with_capacity(self.en.len());
        let mut rest = self.en;
        while let Some(start) = rest.find('$') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let gap = after.find('$').map(|end| &after[..end]).and_then(|name| {
                self.places
                    .iter()
                    .position(|p| p.eq_ignore_ascii_case(name))
                    .map(|at| (at, name.len()))
            });
            match gap {
                Some((at, len)) => {
                    out.push_str(fill.get(at).copied().unwrap_or_default());
                    rest = &after[len + 1..];
                }
                None => {
                    out.push('$');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The extension API the page script was handed, where the global one
    /// may not be the extension's own.
    static API: RefCell<JsValue> = const { RefCell::new(JsValue::UNDEFINED) };
}

/// Reads the words from `api` (the `browser` or `chrome` object) from now on.
#[cfg(target_arch = "wasm32")]
pub fn use_api(api: &JsValue) {
    if api.is_object() {
        API.with(|cell| *cell.borrow_mut() = api.clone());
    }
}

/// The browser's catalog entry for `key`, its gaps filled; `None` where it
/// has none, or no `i18n` to ask.
#[cfg(target_arch = "wasm32")]
fn from_browser(key: &str, fill: &[&str]) -> Option<String> {
    use js_sys::{Array, Function, Reflect};
    let given = API.with(|cell| cell.borrow().clone());
    let api = if given.is_object() {
        given
    } else {
        let global: JsValue = js_sys::global().into();
        ["browser", "chrome"]
            .iter()
            .filter_map(|name| Reflect::get(&global, &(*name).into()).ok())
            .find(JsValue::is_object)?
    };
    let i18n = Reflect::get(&api, &"i18n".into())
        .ok()
        .filter(JsValue::is_object)?;
    let get: Function = Reflect::get(&i18n, &"getMessage".into())
        .ok()?
        .dyn_into()
        .ok()?;
    let gaps: Array = fill.iter().map(|text| JsValue::from_str(text)).collect();
    let said = get.call2(&i18n, &key.into(), &gaps).ok()?.as_string()?;
    (!said.is_empty()).then_some(said)
}

/// The browser's language as a page names one (`pt-BR`), for the popup's
/// `lang`; `None` where the browser does not say.
#[cfg(target_arch = "wasm32")]
pub fn ui_language() -> Option<String> {
    from_browser("@@ui_locale", &[]).map(|tag| tag.replace('_', "-"))
}

const NONE: &[&str] = &[];
const SITE: &[&str] = &["site"];

// The browser's list of extensions (the manifests' `__MSG_…__`).

/// The extension's name.
pub const EXT_NAME: Word = Word {
    key: "extName",
    en: "dm — Download Manager",
    places: NONE,
};
/// The extension's description; the stores take 132 characters at most.
pub const EXT_DESCRIPTION: Word = Word {
    key: "extDescription",
    en: "Hands downloads and videos to the dm app, which downloads them. With no app, the browser keeps them.",
    places: NONE,
};

// The context menu items.

/// The menu item on a link.
pub const MENU_LINK: Word = Word {
    key: "menuLink",
    en: "Download link with dm",
    places: NONE,
};
/// The menu item on a video or audio element.
pub const MENU_MEDIA: Word = Word {
    key: "menuMedia",
    en: "Download media with dm",
    places: NONE,
};
/// The menu item on a page.
pub const MENU_ALL_LINKS: Word = Word {
    key: "menuAllLinks",
    en: "Download all links with dm",
    places: NONE,
};
/// The menu item on a selection.
pub const MENU_SELECTED_LINKS: Word = Word {
    key: "menuSelectedLinks",
    en: "Download selected links with dm",
    places: NONE,
};

// The toolbar popup (`extension/popup.html` and `popup.rs`).

/// The status line while the app is asked how it is.
pub const POPUP_LOOKING: Word = Word {
    key: "popupLooking",
    en: "Looking for the dm app…",
    places: NONE,
};
/// The switch that catches downloads at all.
pub const POPUP_CATCH: Word = Word {
    key: "popupCatch",
    en: "Catch downloads",
    places: NONE,
};
/// The panel's switch before the site is known.
pub const POPUP_PANEL_HERE: Word = Word {
    key: "popupPanelHere",
    en: "Show “Download video” on this site",
    places: NONE,
};
/// The panel's switch for a site.
pub const POPUP_PANEL_ON: Word = Word {
    key: "popupPanelOn",
    en: "Show “Download video” on $SITE$",
    places: SITE,
};
/// The panel's switch on a page that is not a site.
pub const POPUP_PANEL_NONE: Word = Word {
    key: "popupPanelNone",
    en: "No video panel on this page",
    places: NONE,
};
/// The take-everything switch before the site is known.
pub const POPUP_CATCH_ALL_HERE: Word = Word {
    key: "popupCatchAllHere",
    en: "Take every download from this site",
    places: NONE,
};
/// The take-everything switch for a site.
pub const POPUP_CATCH_ALL_ON: Word = Word {
    key: "popupCatchAllOn",
    en: "Take every download from $SITE$",
    places: SITE,
};
/// The take-everything switch on a page that is not a site.
pub const POPUP_CATCH_ALL_NONE: Word = Word {
    key: "popupCatchAllNone",
    en: "No downloads to take from this page",
    places: NONE,
};
/// The pause switch before the site is known.
pub const POPUP_PAUSED_HERE: Word = Word {
    key: "popupPausedHere",
    en: "Pause capturing on this site",
    places: NONE,
};
/// The pause switch for a site.
pub const POPUP_PAUSED_ON: Word = Word {
    key: "popupPausedOn",
    en: "Pause capturing on $SITE$",
    places: SITE,
};
/// The pause switch on a page that is not a site.
pub const POPUP_PAUSED_NONE: Word = Word {
    key: "popupPausedNone",
    en: "No downloads to pause on this page",
    places: NONE,
};
/// The sign-in switch before the site is known.
pub const POPUP_COOKIES_HERE: Word = Word {
    key: "popupCookiesHere",
    en: "Hand over this site's sign-in with its downloads",
    places: NONE,
};
/// The sign-in switch for a site.
pub const POPUP_COOKIES_ON: Word = Word {
    key: "popupCookiesOn",
    en: "Hand over $SITE$'s sign-in with its downloads",
    places: SITE,
};
/// The sign-in switch on a page that is not a site.
pub const POPUP_COOKIES_NONE: Word = Word {
    key: "popupCookiesNone",
    en: "No sign-in to hand over from this page",
    places: NONE,
};
/// The button that brings the app up.
pub const POPUP_OPEN: Word = Word {
    key: "popupOpen",
    en: "Open dm",
    places: NONE,
};
/// The link to the app's download page, with no app to open.
pub const POPUP_GET_APP: Word = Word {
    key: "popupGetApp",
    en: "Get the dm app",
    places: NONE,
};
/// The status line when the app gave no answer and nothing said why.
pub const POPUP_NO_ANSWER: Word = Word {
    key: "popupNoAnswer",
    en: "The dm app did not answer",
    places: NONE,
};
/// While Open dm is asked.
pub const POPUP_OPENING: Word = Word {
    key: "popupOpening",
    en: "Opening dm…",
    places: NONE,
};
/// Open dm worked.
pub const POPUP_IS_OPEN: Word = Word {
    key: "popupIsOpen",
    en: "dm is open",
    places: NONE,
};
/// The status line for an app that answered.
pub const POPUP_CONNECTED: Word = Word {
    key: "popupConnected",
    en: "Connected to dm $VERSION$",
    places: &["version"],
};
/// The status line for an app too old to judge pages.
pub const POPUP_CONNECTED_OLD: Word = Word {
    key: "popupConnectedOld",
    en: "Connected to dm $VERSION$ · update the dm app: until then the browser keeps its downloads",
    places: &["version"],
};
/// A switch turned off on part of a site listed whole.
pub const POPUP_WIDER: Word = Word {
    key: "popupWider",
    en: "That was set for all of $SITES$, so it is changed there too",
    places: &["sites"],
};
/// Two sites, or a list and one more, said together.
pub const POPUP_AND: Word = Word {
    key: "popupAnd",
    en: "$FIRST$ and $NEXT$",
    places: &["first", "next"],
};
/// The status line when the browser finds no dm to talk to.
pub const POPUP_NOT_CONNECTED: Word = Word {
    key: "popupNotConnected",
    en: "dm isn't connected to this browser yet. No dm app on this computer yet? click Get the dm app below, install it and open it once. Already installed: open dm once (if it's already open, open it again) so it can connect, then reopen this popup. Advanced: $COMMAND$",
    places: &["command"],
};
/// The status line when dm is there and did not answer.
pub const POPUP_NOT_ANSWERING: Word = Word {
    key: "popupNotAnswering",
    en: "dm did not answer ($REASON$). Open dm (if it's already open, open it again), then reopen this popup. If it still does not answer: $COMMAND$",
    places: &["reason", "command"],
};

// The "Download video" panel (`content/`), and the plain one in
// `extension/content.js` where the full one cannot load.

/// The bar over a video, and its list's heading.
pub const PANEL_DOWNLOAD_VIDEO: Word = Word {
    key: "panelDownloadVideo",
    en: "Download video",
    places: NONE,
};
/// The bar over an audio player.
pub const PANEL_DOWNLOAD_AUDIO: Word = Word {
    key: "panelDownloadAudio",
    en: "Download this audio",
    places: NONE,
};
/// The bar over a copy-protected player.
pub const PANEL_PROTECTED: Word = Word {
    key: "panelProtected",
    en: "Protected video",
    places: NONE,
};
/// What a click on a copy-protected player's bar says.
pub const PANEL_PROTECTED_SAID: Word = Word {
    key: "panelProtectedSaid",
    en: "This video is copy-protected, so only the site's own player can play it. dm does not download protected video.",
    places: NONE,
};
/// Where a player page's video comes from, on hover.
pub const PANEL_THROUGH_APP: Word = Word {
    key: "panelThroughApp",
    en: "this page, through the dm app",
    places: NONE,
};
/// What ? says.
pub const PANEL_HELP: Word = Word {
    key: "panelHelp",
    en: "dm downloads this video with the dm app, where you choose its name and folder. A link clicked with the keep key held (Alt unless changed in dm's Options) stays in the browser.",
    places: NONE,
};
/// The grip's tooltip.
pub const PANEL_DRAG: Word = Word {
    key: "panelDrag",
    en: "Drag to move",
    places: NONE,
};
/// The ? button's name.
pub const PANEL_WHAT: Word = Word {
    key: "panelWhat",
    en: "What is this?",
    places: NONE,
};
/// The × button's name.
pub const PANEL_HIDE: Word = Word {
    key: "panelHide",
    en: "Hide",
    places: NONE,
};
/// The × menu's first line.
pub const PANEL_HIDE_FOR_NOW: Word = Word {
    key: "panelHideForNow",
    en: "Hide for now",
    places: NONE,
};
/// The × menu's second line.
pub const PANEL_NEVER_ON: Word = Word {
    key: "panelNeverOn",
    en: "Never on $SITE$",
    places: SITE,
};
/// The site, where the page's address names none.
pub const PANEL_THIS_SITE: Word = Word {
    key: "panelThisSite",
    en: "this site",
    places: NONE,
};
/// The bar while the app reads the video's qualities.
pub const PANEL_READING: Word = Word {
    key: "panelReading",
    en: "Reading qualities…",
    places: NONE,
};
/// The line under it, so a wait is not taken for a hang.
pub const PANEL_READING_SAID: Word = Word {
    key: "panelReadingSaid",
    en: "dm is reading this page's video qualities; this can take up to half a minute.",
    places: NONE,
};
/// The app gave no list of qualities in time.
pub const PANEL_TIMED_OUT: Word = Word {
    key: "panelTimedOut",
    en: "dm did not answer in time. Try again, or right-click the video and choose Download media with dm.",
    places: NONE,
};
/// The bar while the choice is handed to the app.
pub const PANEL_SENDING: Word = Word {
    key: "panelSending",
    en: "Sending to dm…",
    places: NONE,
};
/// The bar when the app refused it.
pub const PANEL_NOT_TAKEN: Word = Word {
    key: "panelNotTaken",
    en: "dm could not take it",
    places: NONE,
};
/// The page's script is older than the extension it talks to.
pub const PANEL_UPDATED: Word = Word {
    key: "panelUpdated",
    en: "dm was updated: reload this page to use it",
    places: NONE,
};
/// The bar when the app took it.
pub const PANEL_SENT: Word = Word {
    key: "panelSent",
    en: "Sent to dm",
    places: NONE,
};
/// The bar when the browser is downloading it instead.
pub const PANEL_IN_BROWSER: Word = Word {
    key: "panelInBrowser",
    en: "Downloading in the browser",
    places: NONE,
};
/// The list's first line: every quality at once.
pub const PANEL_DOWNLOAD_ALL: Word = Word {
    key: "panelDownloadAll",
    en: "Download all",
    places: NONE,
};
/// The list's "Part" line, before its start.
pub const PANEL_PART: Word = Word {
    key: "panelPart",
    en: "Part",
    places: NONE,
};
/// The "Part" line, before its end.
pub const PANEL_PART_TO: Word = Word {
    key: "panelPartTo",
    en: "to",
    places: NONE,
};
/// The name of the part's start box.
pub const PANEL_PART_START: Word = Word {
    key: "panelPartStart",
    en: "Part start",
    places: NONE,
};
/// The name of the part's end box.
pub const PANEL_PART_END: Word = Word {
    key: "panelPartEnd",
    en: "Part end",
    places: NONE,
};
/// A quality line for a height the player named: "MP4 file, quality 1080p HD".
pub const PANEL_FILE_QUALITY: Word = Word {
    key: "panelFileQuality",
    en: "$KIND$ file, quality $QUALITY$",
    places: &["kind", "quality"],
};
/// The background's answer for a page handed to the app to read.
pub const PANEL_APP_READING: Word = Word {
    key: "panelAppReading",
    en: "the dm app is reading this page for its video",
    places: NONE,
};
/// The background's answer for a hand-off the app asks about first.
pub const PANEL_APP_HAS: Word = Word {
    key: "panelAppHas",
    en: "the dm app has it",
    places: NONE,
};
/// The background's answer for a hand-off the app started.
pub const PANEL_APP_DOWNLOADING: Word = Word {
    key: "panelAppDownloading",
    en: "the dm app is downloading it",
    places: NONE,
};
/// The plain panel: the app gave no answer to the list of qualities.
pub const PANEL_NO_ANSWER: Word = Word {
    key: "panelNoAnswer",
    en: "dm did not answer",
    places: NONE,
};
/// The plain panel: asking for the list failed.
pub const PANEL_FAILED: Word = Word {
    key: "panelFailed",
    en: "dm did not answer. Try again, or right-click the video and choose Download media with dm.",
    places: NONE,
};
/// The plain panel's line for the HLS playlist the page's own player names.
pub const PANEL_HLS: Word = Word {
    key: "panelHls",
    en: "HLS stream, best quality",
    places: NONE,
};
/// The plain panel's line for a choice that names nothing.
pub const PANEL_DOWNLOAD: Word = Word {
    key: "panelDownload",
    en: "Download",
    places: NONE,
};

/// Every word, so each catalog can be held to the list.
pub const ALL: &[Word] = &[
    EXT_NAME,
    EXT_DESCRIPTION,
    MENU_LINK,
    MENU_MEDIA,
    MENU_ALL_LINKS,
    MENU_SELECTED_LINKS,
    POPUP_LOOKING,
    POPUP_CATCH,
    POPUP_PANEL_HERE,
    POPUP_PANEL_ON,
    POPUP_PANEL_NONE,
    POPUP_CATCH_ALL_HERE,
    POPUP_CATCH_ALL_ON,
    POPUP_CATCH_ALL_NONE,
    POPUP_PAUSED_HERE,
    POPUP_PAUSED_ON,
    POPUP_PAUSED_NONE,
    POPUP_COOKIES_HERE,
    POPUP_COOKIES_ON,
    POPUP_COOKIES_NONE,
    POPUP_OPEN,
    POPUP_GET_APP,
    POPUP_NO_ANSWER,
    POPUP_OPENING,
    POPUP_IS_OPEN,
    POPUP_CONNECTED,
    POPUP_CONNECTED_OLD,
    POPUP_WIDER,
    POPUP_AND,
    POPUP_NOT_CONNECTED,
    POPUP_NOT_ANSWERING,
    PANEL_DOWNLOAD_VIDEO,
    PANEL_DOWNLOAD_AUDIO,
    PANEL_PROTECTED,
    PANEL_PROTECTED_SAID,
    PANEL_THROUGH_APP,
    PANEL_HELP,
    PANEL_DRAG,
    PANEL_WHAT,
    PANEL_HIDE,
    PANEL_HIDE_FOR_NOW,
    PANEL_NEVER_ON,
    PANEL_THIS_SITE,
    PANEL_READING,
    PANEL_READING_SAID,
    PANEL_TIMED_OUT,
    PANEL_SENDING,
    PANEL_NOT_TAKEN,
    PANEL_UPDATED,
    PANEL_SENT,
    PANEL_IN_BROWSER,
    PANEL_DOWNLOAD_ALL,
    PANEL_PART,
    PANEL_PART_TO,
    PANEL_PART_START,
    PANEL_PART_END,
    PANEL_FILE_QUALITY,
    PANEL_APP_READING,
    PANEL_APP_HAS,
    PANEL_APP_DOWNLOADING,
    PANEL_NO_ANSWER,
    PANEL_FAILED,
    PANEL_HLS,
    PANEL_DOWNLOAD,
];

/// The panel's line under the bar for what the background answered: its
/// own words for the answers it knows, anything else (a reason from the app)
/// as it came.
pub fn answered(said: &str) -> String {
    [PANEL_APP_READING, PANEL_APP_HAS, PANEL_APP_DOWNLOADING]
        .iter()
        .find(|word| word.en == said)
        .map_or_else(|| said.to_string(), Word::say)
}

#[cfg(test)]
mod tests;
