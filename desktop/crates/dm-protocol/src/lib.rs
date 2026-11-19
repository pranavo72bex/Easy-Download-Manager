// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the browser extension and the desktop app say to each other.
//!
//! Both are Rust, so the words, the shapes of the messages and the rule
//! that decides which downloads the app takes are written here once: the
//! native host in dm-shell answers with these types and the extension,
//! compiled to WebAssembly, asks with them.
//!
//! Native messaging frames are JSON objects. Every one carries
//! `protocolVersion` and a `type`; the rest depends on the type.
//!
//! A frame's `protocolVersion` is the oldest version that reads it
//! correctly, not the newest its writer knows: a host older than a frame's
//! version does not answer it, so everything an older side reads correctly
//! goes out at the old number and only what it would misread goes out at
//! the new one. Each side reads every version from [`OLDEST_PROTOCOL`] to
//! [`PROTOCOL_VERSION`], and the host's hello says the newest it reads
//! (`protocol`), which is how the extension knows what the app can judge.
//!
//! | version | what it added                                                |
//! |---------|--------------------------------------------------------------|
//! | 1       | everything else                                              |
//! | 2       | `download.start` with `caught`: the host judges `mime`, the  |
//! |         | `cookies` and the `deadline`, refusing a sign-in page served |
//! |         | in the file's place. A version-1 app ignored those fields    |
//! |         | and took the page, and the browser's copy was erased         |
//!
//! | extension sends   | host answers                                  |
//! |-------------------|-----------------------------------------------|
//! | `engine.hello`    | `engine.hello` with `engineVersion`           |
//! | `settings.get`    | `settings.get` with `catch` ([`CatchRules`])  |
//! | `download.start`  | `download.started`, or `download.failed`      |
//! | `media.formats`   | `media.formats` with `choices`, or a `reason` |
//! | `download.batch`  | `download.started`, or `download.failed`      |
//! | `catch.judge`     | `catch.judged`: the app's rules as saved now  |
//! | `menu.clicked`    | `download.started`, or `download.failed`      |
//! | `media.streams`   | `media.streams`: the panel's list             |
//! | `downloads.list`  | `downloads.list`: what the app is fetching    |
//!
//! **The extension does the bare minimum.** What needs the browser stays in it (catching and
//! holding a download, the cookies, headers and links only it sees, its menu, popup and panel);
//! what to do is the app's: the menu it serves ([`Hello::menus`]), whether a caught download is
//! taken ([`Request::Judge`]), the words said ([`Notice`], [`Hello::status`]) and the panel's
//! list ([`media::panel_list`]). Each is a [`feature`] name, so an older side goes on as before.
//!
//! **The app moves ahead of the extension.** A browser store can hold an extension's update
//! back for days while the app updates itself, so the rule is that a newer app never stops an
//! older extension working: the host reads every request version back to [`OLDEST_PROTOCOL`],
//! every reply goes out at version 1 and is only ever added to, the extension reads a reply of
//! any version for the fields it knows, and a new ability is a name in [`feature`] said in the
//! hellos, not a new version. The frames extension 3.4.0 sends are kept in
//! `tests/wire/extension-3.4.0.json`, and every app must answer them.
//!
//! The host is deliberately one-way past `download.started`: the extension
//! catches and the application shows what happened.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod catching;
mod error;
pub mod media;
mod reply;
mod rules;
use catching::{base_name, path_of};
pub use catching::{
    browser_named, extension_for_mime, extension_of, host_of, Candidate, CatchRules, Left,
    BROWSERS, CATCH_DEFAULTS_VERSION, CLICK_KEYS, DEFAULT_EXTENSIONS, MENU_ITEMS,
};
pub use error::Error;
pub use reply::{Failure, Hello, Reply, SettingsReply, Unreadable};
pub use rules::{
    AnswerRule, ExcludeRules, Finder, Rules, SiteProfile, MAX_RULES_BYTES, RULES_FORMAT,
};

/// The newest version this side reads and writes. A frame of a version
/// outside [`OLDEST_PROTOCOL`]`..=PROTOCOL_VERSION` is not read.
pub const PROTOCOL_VERSION: u64 = 2;

/// The oldest version still read: what an older extension or app sends.
pub const OLDEST_PROTOCOL: u64 = 1;

/// The first version whose host judges a caught download by what the
/// browser said it was (`mime`), so it refuses a sign-in page served to dm
/// in the file's place. The extension hands a download the browser holds,
/// and will erase, only to an app at least this new.
pub const JUDGES_PAGES_SINCE: u64 = 2;

/// The native messaging host's name, which is also its manifest's file name
/// without the `.json`.
pub const NATIVE_HOST: &str = "com.dm.engine";

/// The id `manifest.firefox.json` gives the extension. Firefox host manifests
/// name extensions by this, not by an origin.
pub const FIREFOX_EXTENSION_ID: &str = "easy-download-manager@pranavo72bex.github.io";

/// The id Chrome gives the unpacked extension, fixed by the public `key` in
/// `extension/manifest.json`. A store build gets another id from the store,
/// which is registered by naming it.
pub const CHROME_EXTENSION_ID: &str = "fojpndfjodldkfbamkofdmnfedgjjejo";

/// The id Microsoft Edge Add-ons gave the extension's listing, which every
/// registration allows beside the unpacked build's.
pub const EDGE_EXTENSION_ID: &str = "ooafpfjpliofcdlnbdkjhonaoebcpnej";

/// The frame types, for code that has to name one without building it.
pub mod kind {
    /// [`Request::Hello`] and its answer, [`Reply::Hello`].
    pub const HELLO: &str = "engine.hello";
    /// [`Request::Settings`] and its answer, [`Reply::Settings`].
    pub const SETTINGS: &str = "settings.get";
    /// [`Request::Start`]: a download handed over.
    pub const START: &str = "download.start";
    /// [`Reply::Started`]: the app has the download.
    pub const STARTED: &str = "download.started";
    /// [`Reply::Failed`]: the app did not take it; the browser keeps it.
    pub const FAILED: &str = "download.failed";
    /// [`Request::Show`]: bring the app's window to the front.
    pub const SHOW: &str = "app.show";
    /// [`Request::Judge`] and its answer, [`Reply::Judged`].
    pub const JUDGE: &str = "catch.judge";
    /// [`Reply::Judged`].
    pub const JUDGED: &str = "catch.judged";
    /// [`Request::MenuClicked`]: an item of the menu the app served was clicked.
    pub const MENU_CLICKED: &str = "menu.clicked";
    /// [`Request::Streams`] and its answer, [`Reply::Streams`].
    pub const STREAMS: &str = "media.streams";
    /// [`Request::Downloads`] and its answer, [`Reply::Downloads`].
    pub const DOWNLOADS: &str = "downloads.list";
}

/// What the extension asks.
// `Start` outgrew the rest once it carried the page's request headers (gaps
// row 116). A request is one message, read and dropped, never kept in bulk,
// so its size costs nothing; boxing it would change every place, in the
// app and the extension, that builds or reads one.
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum Request {
    /// Is an app there, which version, and can it resolve a player page. The extension says
    /// who it is ([`Greeting`]); an extension of 3.4.0 or older says nothing.
    #[serde(rename = "engine.hello")]
    Hello(Greeting),
    /// Which downloads the app wants caught. They live in the app's settings
    /// (Options › File types), not in the extension.
    #[serde(rename = "settings.get")]
    Settings,
    /// Take this download.
    #[serde(rename = "download.start")]
    Start(Start),
    /// What qualities a page's video comes in, for the "Download video"
    /// panel's list.
    #[serde(rename = "media.formats")]
    Formats(FormatsAsked),
    /// A page's links, all or the selected ones, for the app's "Download all
    /// links" dialog, as the browser's menus hand them over.
    #[serde(rename = "download.batch")]
    Batch(Batch),
    /// Bring the app's window to the front, starting the app if it is not
    /// running: what the toolbar button does. It hands over nothing.
    #[serde(rename = "app.show")]
    Show,
    /// Would the app take this download the browser started, by its catch rules as they are
    /// now. Sent only to an app whose hello says [`feature::JUDGES_CATCH`]; the extension
    /// judges with the rules' code it was built with otherwise.
    #[serde(rename = "catch.judge")]
    Judge(Judge),
    /// An item of the right-click menu the app served ([`Hello::menus`]) whose
    /// [`MenuItem::does`] is [`menu_does::APP`] was clicked: what the browser said of the
    /// click, for the app to choose what it takes. Answered as a hand-off is.
    #[serde(rename = "menu.clicked")]
    MenuClicked(MenuClicked),
    /// The streams a tab's player loaded, as the extension kept them, for the app to make the
    /// panel's list of ([`media::panel_list`]). Sent only to an app whose hello says
    /// [`feature::LISTS_STREAMS`].
    #[serde(rename = "media.streams")]
    Streams(StreamsAsked),
    /// What the app is fetching now and its last failure, for the toolbar popup (gaps row U18).
    /// Sent only to an app whose hello says [`feature::LISTS_DOWNLOADS`].
    #[serde(rename = "downloads.list")]
    Downloads,
}

/// A download the browser started, as far as the catch rules judge it ([`Request::Judge`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Judge {
    /// The extension's name for this question, echoed in the answer.
    pub id: String,
    /// Where the bytes come from, after redirects when the browser knows.
    pub url: String,
    /// The address the download was asked for, before any redirect.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub asked_url: String,
    /// The page it was started from.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub referrer: String,
    /// The name the browser chose, where it has chosen.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub filename: String,
    /// What the server said it is sending.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mime: String,
    /// Its size in bytes, where the browser knows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Which of [`BROWSERS`] it was started in.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub browser: String,
    /// The person asked for this one (a click with the take key held, or the toolbar popup's
    /// "every download from this site"): type and size do not count, the app's switch and its
    /// sites to leave alone still do.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub every_type: bool,
}

impl Judge {
    /// The download, as [`CatchRules::takes`] reads one.
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

    /// Whether `rules` take it, or why not, in words for the extension's log.
    pub fn judged_by(&self, rules: &CatchRules) -> Result<(), String> {
        if !self.browser.is_empty() && !rules.takes_from(&self.browser) {
            return Err(format!(
                "dm takes no downloads from {} (Options › Browser)",
                self.browser
            ));
        }
        rules
            .takes(&self.candidate(), self.every_type)
            .map_err(|left| left.reason().to_string())
    }
}

/// A click on an item of the menu the app served ([`Request::MenuClicked`]): the browser's
/// `contextMenus.OnClickData`, as far as a hand-off can use it, and what the extension reads
/// beside it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct MenuClicked {
    /// The extension's name for this hand-off, echoed in the answer.
    pub id: String,
    /// Which item: its [`MenuItem::id`].
    pub item: String,
    /// The link right-clicked, if any.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub link_url: String,
    /// The picture's, video's or sound's address, if any.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub src_url: String,
    /// The page's address.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub page_url: String,
    /// The address of the frame the click was in.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub frame_url: String,
    /// The text selected, if any.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub selection_text: String,
    /// "image", "video" or "audio", where the click was on one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub media_type: String,
    /// The tab's title.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// The browser's cookies for the clicked address's site, where the person hands that
    /// site's sign-in over (the toolbar popup, or Options for every site).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cookies: String,
    /// Those cookies are for this download alone.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cookies_once: bool,
}

/// A tab's streams, for the app to make the panel's list of ([`Request::Streams`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct StreamsAsked {
    /// The page the tab is on.
    pub page: String,
    /// What the tab's player loaded, as the extension kept it, oldest first.
    pub streams: Vec<media::Stream>,
}

/// What an item of the menu the app serves does, by a mechanism the extension knows
/// ([`MenuItem::does`]). A new item the app adds uses [`menu_does::APP`]: the click goes to
/// the app, which chooses what to take from it, so an extension as released carries it.
pub mod menu_does {
    /// The link right-clicked, handed over (or downloaded by the browser where dm cannot).
    pub const LINK: &str = "link";
    /// The video or sound right-clicked, or its page where its address is a player's own.
    pub const MEDIA: &str = "media";
    /// Every link of the page, for the app's "Download links" dialog.
    pub const ALL_LINKS: &str = "all-links";
    /// The links of the selection, for the same dialog.
    pub const SELECTED_LINKS: &str = "selected-links";
    /// Sent to the app as it was clicked ([`super::Request::MenuClicked`]).
    pub const APP: &str = "app";
    /// Every mechanism an extension that says [`super::feature::MENUS`] knows.
    pub const ALL: [&str; 5] = [LINK, MEDIA, ALL_LINKS, SELECTED_LINKS, APP];
}

/// One item of the browser's right-click menu, as the app serves it ([`Hello::menus`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct MenuItem {
    /// Its id, which a click names.
    pub id: String,
    /// Its words, in English. The extension says an item it knows by id in the browser's
    /// language instead.
    pub title: String,
    /// Where the browser offers it: "link", "page", "selection", "video", "audio", "image".
    pub contexts: Vec<String>,
    /// What a click does: one of [`menu_does`]. An item the extension does not know how to
    /// carry out is not made.
    pub does: String,
}

/// The right-click menu as the app serves it: [`MENU_ITEMS`] less those Options leaves out
/// (`hidden_menus`), each with where the browser offers it and what it does — the menu an
/// extension of 3.4.0 makes for itself.
pub fn served_menus(rules: &CatchRules) -> Vec<MenuItem> {
    MENU_ITEMS
        .iter()
        .filter(|(id, _)| rules.shows_menu(id))
        .map(|(id, title)| {
            let (contexts, does): (&[&str], &str) = match *id {
                "dm-link" => (&["link"], menu_does::LINK),
                "dm-media" => (&["video", "audio"], menu_does::MEDIA),
                "dm-all-links" => (&["page"], menu_does::ALL_LINKS),
                _ => (&["selection"], menu_does::SELECTED_LINKS),
            };
            MenuItem {
                id: (*id).to_string(),
                title: (*title).to_string(),
                contexts: contexts.iter().map(|c| (*c).to_string()).collect(),
                does: does.to_string(),
            }
        })
        .collect()
}

/// Words the app sends for the extension to show in place of its own
/// ([`feature::SAYS`]): a notification's title and body.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Notice {
    /// The notification's title.
    pub title: String,
    /// Its body.
    pub body: String,
}

/// The toolbar popup's status line, as the app would have it said ([`Hello::status`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct StatusLine {
    /// The line.
    pub line: String,
    /// Said as something wrong (in the popup's warning colour).
    pub bad: bool,
}

/// What the extension says of itself in its hello, so that the app, which is updated more
/// often than a store lets the extension be, works out what an older extension can do rather
/// than the extension having to keep up with the app. Both fields are empty from an
/// extension of 3.4.0 or older, which sent a bare hello; an app older than this field
/// ignores it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Greeting {
    /// The extension's version, such as "3.5.0".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub extension_version: String,
    /// What it can do, by the names in [`feature`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

impl Greeting {
    /// Whether the extension said it can do `name` (one of [`feature`]).
    pub fn has(&self, name: &str) -> bool {
        self.features.iter().any(|f| f == name)
    }
}

/// Names of what one side can do, said in its hello ([`Greeting::features`],
/// [`Hello::features`]). A new ability is a new name here, never a new protocol version: a
/// side that does not know a name ignores it and goes on as before, so an app updated ahead
/// of its extension (or the other way round) keeps working with what both know.
pub mod feature {
    /// Extension: reads a reply whatever its `protocolVersion`, taking the fields it knows.
    pub const READS_ANY_REPLY: &str = "reads-any-reply";
    /// Extension: sends a newer address of a stream it handed over (`Start::renews`).
    /// App: takes one for a row whose address ran out (the same as [`super::Hello::renews`]).
    pub const RENEWS: &str = "renews";
    /// App: serves the right-click menu in its hello ([`super::Hello::menus`]) and answers
    /// `menu.clicked`. Extension: makes the menu the app serves, in place of its own.
    pub const MENUS: &str = "menus";
    /// App: answers `catch.judge` by its catch rules as they are now. Extension: asks, in
    /// place of judging with the rules' code it was built with, so a rule added to the app
    /// works with the extension as released.
    pub const JUDGES_CATCH: &str = "judges-catch";
    /// Extension: shows the words a reply carries ([`super::Notice`],
    /// [`super::Hello::status`]) in place of its own; its own where none come.
    pub const SAYS: &str = "says";
    /// App: answers `media.streams` with the panel's list. Extension: asks, in place of making
    /// the list with the code it was built with.
    pub const LISTS_STREAMS: &str = "lists-streams";
    /// App: answers `downloads.list` with what it is fetching ([`super::Going`]). Extension:
    /// asks while its toolbar popup is open, and lists them there.
    pub const LISTS_DOWNLOADS: &str = "lists-downloads";
    /// App: keeps no record of a download handed over from a private window
    /// ([`super::Start::private_window`]) once its file is whole. Extension: hands one over only
    /// to an app that says so.
    pub const KEEPS_NO_RECORD: &str = "keeps-no-record";
}

/// A download the app is fetching, as the toolbar popup lists it ([`Reply::Downloads`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Going {
    /// Its file's name.
    pub name: String,
    /// How much of it is here, 0 to 100, where its size is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    /// Bytes a second now.
    pub speed: u64,
}

/// The app's last download that failed, as the toolbar popup says it ([`Reply::Downloads`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct LastFailure {
    /// Its file's name.
    pub name: String,
    /// Why, in the app's words.
    pub why: String,
}

/// Links handed over together.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Batch {
    /// The extension's name for this hand-off, echoed in the answer.
    pub id: String,
    /// The page they are on, sent as Referer.
    pub page: String,
    /// The links, in the page's order; the app's dialog lets the person choose.
    pub urls: Vec<String>,
    /// The page's site's cookies, where the toolbar popup hands over its
    /// sign-in: they go with its links on that site only, as with a single
    /// download. Empty from an extension that sends none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
    /// Each link's own words on the page (its text, an image's `alt`), by
    /// address, where it has any: the app shows them beside the links and
    /// keeps them as the downloads' descriptions. Empty from an extension
    /// that sends none.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub texts: std::collections::BTreeMap<String, String>,
    /// The file name a link's `download` attribute gives, by address, where
    /// it gives one: the download is saved under it, as the browser would.
    /// Empty from an extension that sends none.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub names: std::collections::BTreeMap<String, String>,
}

/// A form a page sent whose answer is the file (`Start::post`): sent again
/// by dm, once, to fetch it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Posted {
    /// The form's fields as `application/x-www-form-urlencoded` text, where
    /// the browser gave them as fields.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub form: String,
    /// Or the request's raw body, base64, where it gave bytes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw: String,
    /// The body's type, where the browser's request named one.
    #[serde(
        default,
        rename = "contentType",
        skip_serializing_if = "String::is_empty"
    )]
    pub content_type: String,
}

impl std::fmt::Debug for Posted {
    // A person's own words may be in a form: their size, never them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Posted")
            .field("form", &format_args!("{} bytes", self.form.len()))
            .field("raw", &format_args!("{} bytes", self.raw.len()))
            .field("content_type", &self.content_type)
            .finish()
    }
}

/// A page to list the qualities of.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct FormatsAsked {
    /// The page whose video is listed.
    pub url: String,
    /// The page the video page was reached from, if any, sent as Referer.
    #[serde(default)]
    pub referrer: String,
    /// The page's site's cookies, as the `Cookie` header the browser would
    /// send: a video site checks them before it lists anything ("confirm
    /// you're not a bot"). Used for this one query and kept nowhere.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
}

/// One quality the panel offers: what the list says, the quality to fetch it
/// at ("audio", a height such as "720", or "best"), and its size in bytes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Choice {
    /// What the panel's list says, such as "1080p MP4".
    pub label: String,
    /// The quality to fetch it at: "audio", a height such as "720", or "best".
    pub quality: String,
    /// Its size in bytes, where the reading of the page says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// One download handed over.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Start {
    /// The extension's name for this hand-off, echoed in the answer. The host
    /// files the request under it, so it holds it to a plain name.
    pub id: String,
    /// The form this download is the answer to, where the page sent one to
    /// the download's own address: dm sends it again to fetch the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<Posted>,
    /// The download's address, or the page's for a video page.
    pub url: String,
    /// The page the download came from. A media URL alone is often not
    /// something anything can fetch twice; the page it was on usually is.
    #[serde(default)]
    pub referrer: String,
    /// A download the browser had started and is holding: the host checks
    /// that dm can fetch it before taking it, because a refusal now leaves
    /// it with the browser and a failure later loses it. A file asked for
    /// from the panel or a menu is checked too: refused, the extension has
    /// the browser download it instead. A video page is not checked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub caught: bool,
    /// The quality picked in the panel's list; absent is the app's setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    /// A separate sound track for the picture at `url`, from a player that
    /// fetched them apart; dm fetches both and joins them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    /// What the list called it, for the app's dialog to show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A video from the panel: the app opens "Download details" for it, to name it and choose where it goes, before starting.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ask: bool,
    /// A part of the video rather than the whole, as the panel's "Part"
    /// line has it: where it starts and where it ends, as typed ("1:30",
    /// "90"), either left empty. The app opens "Download details" with
    /// them filled; an older app ignores them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    /// Where the wanted part ends, as typed ("2:00"), left empty for the end of the video.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    /// The browser's cookies for the download's site, as the `Cookie`
    /// header it would send, where the person switched "hand over this
    /// site's sign-in" on in the toolbar popup. Empty otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
    /// The cookies are for this download alone: a video page's session,
    /// handed over with every video so the site lets dm in, rather than
    /// a sign-in the person asked dm to keep. The row drops them once it
    /// finishes. False from an extension that sends only kept ones.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cookies_once: bool,
    /// The headers the browser sent with the request that began this
    /// download (a bearer token, `Origin`, a site's own `X-…`), minus the
    /// ones that belong to one connection and `Cookie`, which has its own
    /// field. Kept like cookies and never logged; absent from an older extension.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    /// The title of the page the download was found on: names a file whose own name says nothing. An older app ignores it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// What the browser said the download was (its `DownloadItem.mime`), for
    /// a download it had started. A server that sends the browser a zip and
    /// dm, without the browser's sign-in, a login page is not one dm can
    /// fetch; this is how the host tells. Empty where the browser did not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mime: String,
    /// When the extension stops waiting for an answer, in milliseconds since
    /// the Unix epoch on this computer's clock, which both sides read. Past
    /// it the extension has given the download back to the browser, so a
    /// host that would answer later refuses instead of filing a second copy.
    /// Absent from an older extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<u64>,
    /// The browser holds this download, paused, and erases its copy once
    /// the app has taken it: a sign-in page taken for the file loses it.
    /// Only such a hand-off needs a host that judges `mime` (version 2); a
    /// file a person asked for from a menu or the panel, which the browser
    /// never started, is checked (`caught`) at version 1 as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    /// A newer signed address of a stream already handed to dm, seen as the
    /// page's player asked for it: for a row whose address ran out (403 or
    /// 410) to go on with, and nothing else. No row waiting for it, it is
    /// dropped: never a new download, never a dialog, and the app is not
    /// started for it. An older app takes it as an ordinary hand-off, so
    /// the extension sends it only to an app whose hello says `renews`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub renews: bool,
    /// Started in a private browser window: the app keeps no record of it once its file is
    /// whole, its row, address, page and sign-in gone (gaps row W17). Sent only when set, at
    /// version 1: an older app passes the key over, as it does every key it does not know, and
    /// keeps the row as any other, so the extension sends one only to an app whose hello says
    /// [`feature::KEEPS_NO_RECORD`]. Absent from an older extension: kept as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private_window: bool,
}

/// The longest the host spends asking a server about a caught download.
/// Inside the extension's wait for an answer (20 seconds), which also has
/// to cover the host's own start (measured at 50 ms) and the answer's
/// margin below. It was 10 s; a server slow to answer dm's first request
/// (12 s, in a check) left its download with the browser, every time.
pub const PROBE_WITHIN_MS: u64 = 16_000;

/// Kept back from the extension's deadline for filing the download and
/// sending the answer back.
pub const ANSWER_MARGIN_MS: u64 = 2_000;

/// How long the host may still spend on a hand-off whose answer is due by
/// `deadline`, or `None` when it is too late to take it at all: the extension
/// will have given it back to the browser before an answer could arrive, and
/// taking it then has the file downloaded twice.
pub fn time_left(now_ms: u64, deadline: Option<u64>) -> Option<u64> {
    let Some(by) = deadline else {
        return Some(PROBE_WITHIN_MS);
    };
    by.checked_sub(now_ms)?
        .checked_sub(ANSWER_MARGIN_MS)
        .filter(|left| *left > 0)
        .map(|left| left.min(PROBE_WITHIN_MS))
}

/// Whether what dm was served is a page standing in for the file the browser
/// is downloading: typically a sign-in page that a site redirects a request
/// without its cookies to. `served` is the type dm's request ended at, after
/// redirects; `browser_mime` what the browser was given; `url` the download.
///
/// Only an HTML answer is suspect, and only when the browser was not getting
/// HTML itself. Where the browser did not say, the address's own file type
/// decides: `report.pdf` answered with a page is not the report.
pub fn page_instead_of_file(browser_mime: &str, url: &str, served: &str) -> bool {
    let is_page = |mime: &str| {
        let essence = mime
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        essence == "text/html" || essence == "application/xhtml+xml"
    };
    if !is_page(served) {
        return false;
    }
    if !browser_mime.trim().is_empty() {
        return !is_page(browser_mime);
    }
    // Names that are pages, or scripts that usually answer with one.
    const PAGES: &[&str] = &[
        "html", "htm", "xhtml", "shtml", "php", "asp", "aspx", "jsp", "cgi", "pl",
    ];
    extension_of(base_name(path_of(url))).is_some_and(|ext| !PAGES.contains(&ext.as_str()))
}

fn envelope<T: Serialize>(message: &T, version: u64) -> Value {
    let mut value = serde_json::to_value(message).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.insert("protocolVersion".into(), version.into());
    }
    value
}

fn open<T: serde::de::DeserializeOwned>(frame: &Value) -> Result<T, Unreadable> {
    open_within(frame, PROTOCOL_VERSION)
}

/// [`open`], reading every version from [`OLDEST_PROTOCOL`] to `newest`.
fn open_within<T: serde::de::DeserializeOwned>(
    frame: &Value,
    newest: u64,
) -> Result<T, Unreadable> {
    match frame.get("protocolVersion").and_then(Value::as_u64) {
        Some(v) if (OLDEST_PROTOCOL..=newest).contains(&v) => {}
        other => return Err(Unreadable::Version(other)),
    }
    // Keys a message does not use — `protocolVersion`, or the `filename` an
    // older extension sent — are ignored rather than refused.
    serde_json::from_value(frame.clone()).map_err(|e| Unreadable::Shape(e.to_string()))
}

impl Request {
    /// The frame, at the oldest version that reads it correctly.
    pub fn to_frame(&self) -> Value {
        envelope(self, self.needs())
    }

    /// The oldest protocol version that reads this request correctly. A
    /// download the browser holds (`held`) needs a host that judges what it
    /// is served (version 2): a version-1 host would take a sign-in page for
    /// the file, so it is sent at 2, which such a host does not answer, and
    /// the extension gives the download back when no answer comes. Anything
    /// else, a file asked for from a menu included, a version-1 host reads.
    pub fn needs(&self) -> u64 {
        match self {
            Request::Start(start) if start.held => JUDGES_PAGES_SINCE,
            _ => OLDEST_PROTOCOL,
        }
    }

    /// Reads a request frame, refusing one of a version this side does not read.
    pub fn from_frame(frame: &Value) -> Result<Self, Unreadable> {
        open(frame)
    }
}

#[cfg(test)]
use dm_test_support::secrets as test_secrets;

#[cfg(test)]
mod tests;

/// For `skip_serializing_if`: a flag sent only when set.
fn is_false(flag: &bool) -> bool {
    !*flag
}
