//! What the extension knows of the app: the handshake and what to say when it is absent.

use super::*;

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
