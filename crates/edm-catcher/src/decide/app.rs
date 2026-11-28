// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the extension knows of the app: the handshake and what to say when it is absent.

use super::*;
use crate::words;

/// The name a hand-off is filed under. The host keeps only letters, digits,
/// `-` and `_`, so that is all this uses.
pub fn hand_id(prefix: &str, n: i64, now_ms: f64) -> String {
    format!("{prefix}{n}-{}", now_ms.max(0.0) as u64)
}

/// What this extension says of itself in its hello: its version, and what it can do, so the
/// app (updated more often than a store lets this be) works out what this extension can do
/// rather than this extension having to keep up with the app.
pub fn greeting() -> edm_protocol::Greeting {
    edm_protocol::Greeting {
        extension_version: env!("CARGO_PKG_VERSION").to_string(),
        features: vec![
            edm_protocol::feature::READS_ANY_REPLY.to_string(),
            edm_protocol::feature::RENEWS.to_string(),
            edm_protocol::feature::MENUS.to_string(),
            edm_protocol::feature::JUDGES_CATCH.to_string(),
            edm_protocol::feature::SAYS.to_string(),
            edm_protocol::feature::LISTS_STREAMS.to_string(),
        ],
    }
}

/// Whether the extension whose own pages are at `base` (`runtime.getURL("")`) talks to the app
/// one message at a time (`runtime.sendNativeMessage`) rather than on a port: Safari's, whose
/// pages are `safari-web-extension://`. Safari gives a message to EDM's extension inside EDM.app
/// (crates/edm-safari), which answers each on its own; a port's messages get no answer
/// there.
pub fn one_message_at_a_time(base: &str) -> bool {
    base.get(..23)
        .is_some_and(|s| s.eq_ignore_ascii_case("safari-web-extension://"))
}

/// How a reason for "no app" starts when there is an app, of a protocol
/// version this extension cannot talk to: its fix is updating one side,
/// not installing or registering anything.
pub const MISMATCH: &str = "protocol mismatch: ";

/// What to say of a host whose frames carry protocol version `theirs`, older than any this
/// extension reads, naming the side to update. A newer one is read
/// ([`edm_protocol::Reply::from_frame`]): an app updated ahead of the extension keeps working.
pub fn mismatch(theirs: Option<u64>) -> String {
    let ours = edm_protocol::PROTOCOL_VERSION;
    let said = match theirs {
        Some(v) => format!(
            "The EDM app is too old for this extension (protocol {v}, the extension \
             {ours}): update the EDM app"
        ),
        None => "The EDM app is too old for this extension (it says no protocol \
                 version): update the EDM app"
            .to_string(),
    };
    format!("{MISMATCH}{said}")
}

/// Why a download the browser holds is not handed to an app that answered:
/// one older than [`edm_protocol::JUDGES_PAGES_SINCE`] ignores what the
/// browser said the download was, takes a sign-in page for the file, and
/// the browser's copy is erased. The menus and the panel still hand it
/// what a person asks for, which the browser has not started.
pub const OLD_APP: &str = "This EDM app is too old to take downloads the browser started: \
                           update the EDM app. Until then the browser keeps them.";

/// Whether a download the browser holds may be handed to this app.
pub fn takes_caught(hello: &Hello) -> Result<(), &'static str> {
    if hello.judges_pages() {
        Ok(())
    } else {
        Err(OLD_APP)
    }
}

/// The toolbar popup's status line for an app that answered, in the
/// browser's language, and whether it is a warning.
pub fn status_line(hello: &Hello) -> (String, bool) {
    // The app's own words where it sends them: it knows itself better than this extension can.
    if let Some(status) = hello.status.as_ref().filter(|s| !s.line.trim().is_empty()) {
        return (status.line.clone(), status.bad);
    }
    let version = hello.engine_version.as_str();
    if !hello.judges_pages() {
        return (words::POPUP_CONNECTED_OLD.with(&[version]), true);
    }
    (words::POPUP_CONNECTED.with(&[version]), false)
}

/// What the toolbar icon's badge says of the app (gaps row U18): "!" while the app does not
/// answer, so a person sees it before a download is left with the browser; nothing once it does.
pub fn badge(app: &App) -> &'static str {
    match app {
        App::Present { .. } => "",
        App::Absent { .. } => "!",
    }
}

/// The badge's colour while it says something: the popup's own red for a problem.
pub const BADGE_COLOUR: &str = "#c50f1f";

/// Whether the browser found no EDM it may start: no host manifest, or one
/// that does not admit this extension. Either is fixed by EDM itself, which
/// registers with every browser installed when it is opened — opened again
/// too, while it runs. The extension cannot tell from here
/// whether EDM is running: with no host there is nothing to ask.
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
    if let Some(said) = reason.strip_prefix(MISMATCH) {
        return said.to_string();
    }
    let register = register_command(own_id, firefox);
    if not_connected(reason) {
        // Someone who added the extension from a store first has no app
        // yet: told where it is, before how to connect one.
        format!(
            "EDM isn't connected to this browser yet. No EDM app on this computer yet? \
             get it from github.com/pranavo72bex/Easy-Download-Manager/releases, install it and \
             open it once. Already installed: open EDM once (if it's already open, open it again) \
             so it can connect, then try again. Advanced: {register}"
        )
    } else {
        format!(
            "EDM did not answer ({reason}). Open EDM (if it's already open, open it again), \
             then try again. If it still does not answer: {register}"
        )
    }
}

/// [`absent_text`] for the toolbar popup, which is read again by opening it,
/// in the browser's language.
pub fn absent_popup_text(reason: &str, own_id: &str, firefox: bool) -> String {
    if let Some(said) = reason.strip_prefix(MISMATCH) {
        return said.to_string();
    }
    let register = register_command(own_id, firefox);
    if not_connected(reason) {
        // The steps and the command follow it in the popup (`popup_register`).
        words::POPUP_NOT_CONNECTED.say()
    } else {
        words::POPUP_NOT_ANSWERING.with(&[reason, &register])
    }
}

/// The popup's folded-away line under the steps for a browser that finds no EDM: the command that
/// registers EDM with this browser.
pub fn popup_register(own_id: &str, firefox: bool) -> String {
    words::POPUP_ADVANCED.with(&[&register_command(own_id, firefox)])
}

/// The command that registers EDM with this browser. The ids every
/// registration allows need no naming.
fn register_command(own_id: &str, firefox: bool) -> String {
    let known = own_id == CHROME_EXTENSION_ID || own_id == EDGE_EXTENSION_ID;
    if firefox || known || own_id.is_empty() {
        "edm-desktop --register".to_string()
    } else {
        format!("edm-desktop --register {own_id}")
    }
}

/// What the extension knows of the app.
#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "one value, kept for the session; boxing it buys nothing"
)]
pub enum App {
    /// The app answered: its hello, and the rules it serves for what to catch.
    Present {
        /// What the app said of itself.
        hello: Hello,
        /// The rules it serves for what to catch.
        rules: CatchRules,
    },
    /// The app did not answer, and why.
    Absent {
        /// Why, in words.
        reason: String,
    },
}

/// An answer about the app, and when it was given.
#[derive(Debug, Clone)]
pub struct Known {
    /// What was learned of the app.
    pub app: App,
    /// When it was learned, in milliseconds.
    pub at_ms: f64,
}

impl Known {
    /// Whether the answer is recent enough to rely on at `now_ms`.
    #[must_use]
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
            Err(edm_protocol::Unreadable::Version(v)) => self.refused = Some(mismatch(v)),
            Err(_) => {}
        }
    }

    /// Whether the app has said hello.
    #[must_use]
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
pub fn taken(frame: &Value, id: &str) -> Option<Result<bool, String>> {
    taken_said(frame, id).map(|(outcome, _)| outcome)
}

/// [`taken`], with the words the app sent for the browser to say of it in place of this
/// extension's own (`feature::SAYS`), where it sent any.
pub fn taken_said(
    frame: &Value,
    id: &str,
) -> Option<(Result<bool, String>, Option<edm_protocol::Notice>)> {
    let said = |notice: Option<edm_protocol::Notice>| {
        notice.filter(|n| !n.title.trim().is_empty() || !n.body.trim().is_empty())
    };
    match Reply::from_frame(frame).ok()? {
        // Whether the app's own window comes forward to ask about it: then the browser posts
        // no toast of its own (gaps row U11).
        Reply::Started {
            id: answered,
            shown,
            notice,
        } if answered == id => Some((Ok(shown), said(notice))),
        Reply::Failed {
            id: answered,
            failure,
            notice,
        } if answered == id => Some((
            Err(failure
                .map(|f| f.message)
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "EDM would not take it".into())),
            said(notice),
        )),
        _ => None,
    }
}

/// Whether a frame answers the catch question asked under `id` (`catch.judge`): `Some(Ok)`
/// the app takes the download, `Some(Err)` it leaves it with the browser, and why.
pub fn judged(frame: &Value, id: &str) -> Option<Result<(), String>> {
    match Reply::from_frame(frame).ok()? {
        Reply::Judged {
            id: answered,
            take,
            reason,
        } if answered == id => Some(if take {
            Ok(())
        } else {
            Err(reason
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| "EDM's settings leave it with the browser".into()))
        }),
        _ => None,
    }
}

/// The panel's list the app made of a tab's streams (`media.streams`), where the frame is it.
pub fn streams_listed(frame: &Value) -> Option<Vec<edm_protocol::media::Stream>> {
    match Reply::from_frame(frame).ok()? {
        Reply::Streams { streams } => Some(streams),
        _ => None,
    }
}

/// The question the app answers for a download the browser started (`catch.judge`): the
/// download as the rules read it, the browser it is in, and whether the person asked for
/// this one whatever its type and size.
pub fn judge_of(
    found: &Catchable,
    id: &str,
    browser: &str,
    every_type: bool,
) -> edm_protocol::Judge {
    edm_protocol::Judge {
        id: id.to_string(),
        url: found.url.clone(),
        asked_url: found.asked_url.clone(),
        referrer: found.referrer.clone(),
        filename: found.filename.clone(),
        mime: found.mime.clone(),
        size: found.size,
        browser: browser.to_string(),
        every_type,
    }
}

/// URLs given back to the browser, so the download the extension restarts
/// for them is not caught again.
#[derive(Debug, Default)]
pub struct Released {
    urls: Vec<(String, f64)>,
}

impl Released {
    /// Notes that the app took the download of this address, so a repeat is not handed over again for a while.
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

/// One download's line in the toolbar popup: its name, how far it is where its size is known,
/// and its speed (`a.iso · 12% · 3.0 MB/s`).
pub fn going_line(going: &edm_protocol::Going) -> String {
    let mut line = going.name.clone();
    if let Some(percent) = going.percent {
        line.push_str(&format!(" · {percent}%"));
    }
    if going.speed > 0 {
        line.push_str(&format!(" · {}/s", super::human_size(going.speed)));
    }
    line
}
