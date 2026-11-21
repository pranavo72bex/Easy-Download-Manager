// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the host answers: the replies, the host's hello, and a frame that could not be read.

use super::*;

/// What the host answers.
// `Hello` outgrew the rest once it carried the rules and the abilities. A reply is one
// message, read and dropped, so its size costs nothing; boxing it would change every place
// that builds or reads one, in the app and the extension.
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum Reply {
    /// The app is there: its version, and what it can do.
    #[serde(rename = "engine.hello")]
    Hello(Hello),
    /// The app's catch rules.
    #[serde(rename = "settings.get")]
    Settings(SettingsReply),
    /// The app has the download; the browser's copy can go.
    #[serde(rename = "download.started")]
    Started {
        /// The hand-off's [`Start::id`].
        id: String,
        /// Whether the app's own window comes forward to ask about it (Download details,
        /// Download links): the browser posts no toast of its own then (gaps row U11). Not
        /// sent when false, and read as false from an app that does not say.
        #[serde(default, skip_serializing_if = "is_false")]
        shown: bool,
        /// What the browser says of it, in place of the extension's own toast
        /// ([`feature::SAYS`]). Absent: the extension's own words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notice: Option<Notice>,
    },
    /// The app did not take the download; the browser keeps it.
    #[serde(rename = "download.failed")]
    Failed {
        /// The hand-off's [`Start::id`].
        id: String,
        /// Why, where the host can say.
        #[serde(default)]
        failure: Option<Failure>,
        /// What the browser says of it, in place of the extension's own notification
        /// ([`feature::SAYS`]). Absent: the extension's own title, with `failure`'s words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notice: Option<Notice>,
    },
    /// The qualities on offer, best first; empty with a `reason` when the
    /// page could not be read.
    #[serde(rename = "media.formats")]
    Formats {
        /// The qualities, best first.
        #[serde(default)]
        choices: Vec<Choice>,
        /// Why there are none, in words for the panel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The window was asked to show itself; a `reason` when it could not be.
    #[serde(rename = "app.shown")]
    Shown {
        /// Why the window could not be shown, in words for the popup.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The answer to [`Request::Judge`]: whether the app takes the download.
    #[serde(rename = "catch.judged")]
    Judged {
        /// The question's [`Judge::id`].
        id: String,
        /// The app takes it; the extension holds it and hands it over.
        take: bool,
        /// Why not, in words for the extension's log.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The panel's list of a tab's streams, made by the app ([`media::panel_list`]).
    #[serde(rename = "media.streams")]
    Streams {
        /// The lines, in the panel's order.
        #[serde(default)]
        streams: Vec<media::Stream>,
    },
    /// The answer to [`Request::Downloads`]: what the app is fetching, the most the popup shows,
    /// and its last failure.
    #[serde(rename = "downloads.list")]
    Downloads {
        /// The downloads running, the fastest first.
        #[serde(default)]
        downloads: Vec<Going>,
        /// How many run in all, where more run than are listed.
        #[serde(default)]
        running: usize,
        /// The last download that failed, while the app remembers it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failed: Option<LastFailure>,
    },
}

/// The app's answer to [`Request::Hello`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Hello {
    /// The app's version, such as "0.8.4".
    #[serde(rename = "engineVersion", default)]
    pub engine_version: String,
    /// The newest protocol version the app reads. Absent from an app of
    /// version 1, which said nothing of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u64>,
    /// The site rules the app serves (what is never caught, and which answers of a site's own
    /// requests name streams), which the extension adds to the tables it was built with. Absent
    /// from an app that serves none.
    /// Rules this side cannot read (a later format, a field of another type) are taken as none,
    /// and the extension keeps the tables it was built with: the rest of the hello still
    /// stands, so the app is not taken for absent and downloads go on being handed over (gaps
    /// row 219).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "rules_or_none"
    )]
    pub rules: Option<Rules>,
    /// The app takes a newer address of a stream it was handed (`Start::renews`) for a row whose
    /// address ran out. False from an older app, which would add it as another download.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub renews: bool,
    /// What the app can do, by the names in [`feature`]. Empty from an app older than this
    /// field, which said each ability in a field of its own (`renews`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// The right-click menu the app wants made ([`feature::MENUS`]). Absent from an app that
    /// serves none, whose extension makes its own; empty is a menu with nothing in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menus: Option<Vec<MenuItem>>,
    /// The toolbar popup's status line as the app would have it said ([`feature::SAYS`]).
    /// Absent: the extension's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusLine>,
}

/// [`Hello::rules`] as served, or none where they cannot be read.
fn rules_or_none<'de, D>(deserializer: D) -> Result<Option<Rules>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value::<Rules>(value).ok())
}

impl Hello {
    /// Whether the app said it can do `name` (one of [`feature`]).
    pub fn has(&self, name: &str) -> bool {
        self.features.iter().any(|f| f == name)
    }

    /// Whether the app takes a newer address of a stream it was handed, said either way.
    pub fn takes_renewals(&self) -> bool {
        self.renews || self.has(feature::RENEWS)
    }

    /// The newest protocol version the app reads.
    pub fn protocol(&self) -> u64 {
        self.protocol.unwrap_or(OLDEST_PROTOCOL)
    }

    /// Whether the app refuses a sign-in page served to it in place of a
    /// file the browser is downloading — without which a download the
    /// browser holds must not be handed to it, since the browser's copy is
    /// erased once the app has taken it.
    pub fn judges_pages(&self) -> bool {
        self.protocol() >= JUDGES_PAGES_SINCE
    }
}

/// The app's answer to [`Request::Settings`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SettingsReply {
    /// Which downloads the app wants caught (Options › File types).
    #[serde(rename = "catch", default)]
    pub rules: CatchRules,
}

/// Why the app did not take a download.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Failure {
    /// The reason, in words a person reads in the extension's log.
    #[serde(default)]
    pub message: String,
}

/// Why a frame was not read.
#[derive(Debug, Clone, PartialEq)]
pub enum Unreadable {
    /// Another protocol version, or none.
    Version(Option<u64>),
    /// The right version, but not a message this side knows.
    Shape(String),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::Version(Some(v)) => write!(
                f,
                "protocol version {v} is not one of {OLDEST_PROTOCOL} to {PROTOCOL_VERSION}"
            ),
            Unreadable::Version(None) => write!(f, "a frame with no protocol version"),
            Unreadable::Shape(why) => write!(f, "a frame that could not be read: {why}"),
        }
    }
}

impl Reply {
    /// The frame, at version 1: every reply is one a version-1 extension
    /// reads correctly (the hello's `protocol` is a key it ignores).
    pub fn to_frame(&self) -> Value {
        envelope(self, OLDEST_PROTOCOL)
    }

    /// Reads a reply frame. A reply of a newer version than this side knows is read all the
    /// same, for the fields it knows: the app is updated ahead of a store's extension, and a
    /// reply is only ever added to (a new field, a new type, or a new [`feature`]), so an
    /// older extension reads what it always read and the hand-off goes on working. Only a
    /// frame with no version, or one older than [`OLDEST_PROTOCOL`], is refused.
    pub fn from_frame(frame: &Value) -> Result<Self, Unreadable> {
        open_within(frame, u64::MAX)
    }

    /// A refusal, in the one shape every refusal takes.
    pub fn failed(id: &str, message: impl Into<String>) -> Self {
        Reply::Failed {
            id: id.into(),
            failure: Some(Failure {
                message: message.into(),
            }),
            notice: None,
        }
    }

    /// A hand-off taken, `shown` where the app's own window comes forward to ask about it.
    pub fn started(id: &str, shown: bool) -> Self {
        Reply::Started {
            id: id.into(),
            shown,
            notice: None,
        }
    }
}
