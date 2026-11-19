// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What went wrong with a download, said so a person can act on it.

/// Why a download stopped.
///
/// The variants are the distinctions worth acting on, not a catalogue of
/// everything that can go wrong: a refusal names what refused and what it
/// said, because "it failed" sends people looking in the wrong place.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The server answered with a status that is not a success.
    #[error("the server answered {} for {url}{}", status_line(*.status), if .body.is_empty() { String::new() } else { format!(" — {body}") })]
    Refused {
        /// The HTTP status code.
        status: u16,
        /// The URL that was refused.
        url: String,
        /// The body of the error response, if any.
        body: String,
        /// What the server asked us to wait, where it said.
        retry_after: Option<std::time::Duration>,
    },

    /// The body ended before the range did. Usually a dropped connection,
    /// which is worth another attempt; a server that does it every time is
    /// caught by the length check instead.
    #[error("the connection ended after {got} of {expected} bytes")]
    Incomplete {
        /// How many bytes arrived before the connection dropped.
        got: u64,
        /// How many bytes were expected.
        expected: u64,
    },

    /// The file was republished while it was being fetched. Everything staged
    /// belongs to the version that is gone.
    #[error("the file changed on the server, so the download has to start again")]
    Changed,

    /// A range came back longer than it was asked for, so the bytes are not
    /// the bytes of that range. Almost always a compressed representation
    /// ranged over its encoded length and served decoded.
    #[error("this server does not serve ranges honestly")]
    DishonestRanges,

    /// A range was answered with the whole file, and the file had not
    /// changed: the server stopped serving ranges for now. Wikimedia's
    /// caches do it to several connections at once and serve one connection
    /// its range. Not a changed file, so not reported as one.
    #[error("the server stopped serving parts of the file")]
    RangesIgnored,

    /// Stopped on request. Not a failure — the bytes so far are staged and a
    /// later attempt resumes from them.
    #[error("paused")]
    Paused,

    /// The download was cancelled.
    #[error("cancelled")]
    Cancelled,

    /// The parts add up to less than the length the server stated.
    #[error("the server sent {got} bytes of a file it said was {expected}")]
    ShortFile {
        /// How many bytes the server actually sent.
        got: u64,
        /// How many bytes the server said the file was.
        expected: u64,
    },

    /// The common causes in plain words, with what the system said after
    /// them; anything else with its causes, since reqwest's own message is
    /// "error sending request" and why is underneath it.
    #[error("{}", network(.0))]
    Network(#[from] reqwest::Error),

    /// A connection of dm's own (FTP's) dropped, timed out, or was told to
    /// try later. Worth another attempt, as a dropped HTTP request is.
    #[error("network: {0}")]
    Connection(String),

    /// The disk refused a read or a write.
    #[error("disk: {0}")]
    Disk(#[from] std::io::Error),

    /// What arrived is not what was published. The only evidence from
    /// outside the transfer that the transfer was honest, so it names both
    /// digests rather than saying the file is bad.
    #[error("{algorithm} was {got}, but {expected} was published")]
    Checksum {
        /// The name of the hash algorithm (e.g. `"SHA-256"`).
        algorithm: &'static str,
        /// The digest that was published alongside the file.
        expected: String,
        /// The digest computed from the bytes that arrived.
        got: String,
    },

    /// A sentence written for the person reading the row, with no prefix
    /// such as "internal:", which would make a site that wants a sign-in
    /// read as a fault in dm.
    #[error("{0}")]
    Internal(String),

    /// A sentence written for the person reading the row, as [`Error::Internal`] is, for a
    /// failure of a kind dm acts on: what is done about it is decided by its [`FailKind`],
    /// never by its words (gaps row A9).
    #[error("{said}")]
    Known {
        /// What kind of failure it is.
        kind: FailKind,
        /// The sentence the person reads.
        said: String,
    },
}

/// What kind of failure a download's is, where dm does something about that kind: asks
/// whether to save a web page, asks about a link that is gone, waits for room. Carried by the
/// error that stopped the download and kept on its row (`Entry::fail_kind`), so nothing
/// decides by reading the sentence a person sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(from = "String", into = "String")]
pub enum FailKind {
    /// Any failure dm does nothing particular about.
    #[default]
    Other,
    /// The server answered with this HTTP status, which is not a success.
    Refused(u16),
    /// A web page answered where the file was asked for: a sign-in, an expired link.
    WebPage,
    /// The system said the disk is full.
    DiskFull,
    /// dm's own look before writing found no room: too little free space for the file, or a
    /// download folder that cannot be written to.
    NoRoom,
}

impl FailKind {
    /// Its word in a saved queue: "refused 404", "web page", and so on.
    pub fn word(self) -> String {
        match self {
            FailKind::Other => "other".into(),
            FailKind::Refused(status) => format!("refused {status}"),
            FailKind::WebPage => "web page".into(),
            FailKind::DiskFull => "disk full".into(),
            FailKind::NoRoom => "no room".into(),
        }
    }

    /// The kind a saved queue's word names; a word this dm does not know (written by a later
    /// one) is [`FailKind::Other`], so the queue is still read.
    pub fn from_word(word: &str) -> FailKind {
        match word {
            "web page" => FailKind::WebPage,
            "disk full" => FailKind::DiskFull,
            "no room" => FailKind::NoRoom,
            _ => word
                .strip_prefix("refused ")
                .and_then(|status| status.parse().ok())
                .map_or(FailKind::Other, FailKind::Refused),
        }
    }

    /// The kind of a failure saved by a dm from before failures carried one, worked out once
    /// from its sentence by the matching that dm did. Only a row read from such a file (or
    /// made without a kind) is read this way: the one place a failure's words decide anything.
    pub fn of_old_text(said: &str) -> FailKind {
        let lower = said.to_ascii_lowercase();
        if lower.contains("web page rather than the file") {
            return FailKind::WebPage;
        }
        if let Some(status) = answered(&lower) {
            return FailKind::Refused(status);
        }
        if lower.contains("not enough room") || lower.contains("cannot be written to") {
            return FailKind::NoRoom;
        }
        if ["no space left", "disk full", "disk is full"]
            .iter()
            .any(|w| lower.contains(w))
        {
            return FailKind::DiskFull;
        }
        FailKind::Other
    }
}

/// The status an old failure's sentence says a server answered: "the server answered 404 Not
/// Found for …", or a link's 403, 404 or 410 in another server's words ("the MMS server
/// answered 404 Not Found"). An FTP server's reply is no HTTP status, and was never read as one.
fn answered(lower: &str) -> Option<u16> {
    lower.match_indices("answered ").find_map(|(at, word)| {
        let digits = lower.get(at + word.len()..)?.get(..3)?;
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let status: u16 = digits.parse().ok()?;
        let ours = lower
            .get(..at)
            .is_some_and(|before| before.ends_with("the server "));
        (ours || matches!(status, 403 | 404 | 410)).then_some(status)
    })
}

impl From<String> for FailKind {
    fn from(word: String) -> FailKind {
        FailKind::from_word(&word)
    }
}

impl From<FailKind> for String {
    fn from(kind: FailKind) -> String {
        kind.word()
    }
}

/// Whether the system said the disk is full: ENOSPC, and Windows' ERROR_DISK_FULL and
/// ERROR_HANDLE_DISK_FULL.
fn disk_full(io: &std::io::Error) -> bool {
    io.kind() == std::io::ErrorKind::StorageFull || matches!(io.raw_os_error(), Some(28 | 112 | 39))
}

impl Error {
    /// The delay a refusal asked for, where it asked for one.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Error::Refused { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// A failure of a kind dm acts on, said in `said`.
    pub fn known(kind: FailKind, said: impl Into<String>) -> Error {
        Error::Known {
            kind,
            said: said.into(),
        }
    }

    /// What kind of failure this is: a refusal's status, a full disk, or the kind it was made
    /// with ([`Error::known`]); [`FailKind::Other`] for anything else.
    pub fn fail_kind(&self) -> FailKind {
        match self {
            Error::Refused { status, .. } => FailKind::Refused(*status),
            Error::Disk(io) if disk_full(io) => FailKind::DiskFull,
            Error::Known { kind, .. } => *kind,
            _ => FailKind::Other,
        }
    }
}

/// The longest a refusal's body is read for its explanation: no longer than a person would wait
/// for one.
const EXPLAINED_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

impl Error {
    /// Passes a response through, or turns a refusal into an error.
    ///
    /// The predicate belongs next to the constructor. Left at the call sites
    /// it was written out six times, and the seventh fetch anybody adds is
    /// the one that writes a 404 page to disk and calls it a download.
    pub async fn refused_or(res: reqwest::Response) -> Result<reqwest::Response, Error> {
        let status = res.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(Error::from_response(res).await);
        }
        // A redirect the client did not follow (to a `file://` or `ftp://`
        // address, say) is no file: listed as a finished download of its
        // empty body, it said the download had worked.
        if status.is_redirection() && status != reqwest::StatusCode::NOT_MODIFIED {
            let to = res
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("somewhere else")
                .chars()
                .take(200)
                .collect::<String>();
            return Err(Error::Internal(format!(
                "the server sent dm on to {to}, an address dm does not follow"
            )));
        }
        Ok(res)
    }

    /// Whether this says the address itself has run out: the server refused it
    /// (403, or 410 Gone), or a web page answered where the file should be. A
    /// signed address from a browser stops working after a while, and what
    /// is staged under it is still good (`Queue::adopt_fresh_link`).
    pub fn link_expired(&self) -> bool {
        matches!(
            self.fail_kind(),
            FailKind::Refused(403 | 410) | FailKind::WebPage
        )
    }

    /// Turns a refused response into an error that says what refused and why.
    ///
    /// One place, because both the probe and the range fetch need it; built
    /// by hand with an empty body, a refusal that happened mid-download would
    /// explain nothing at all.
    pub async fn from_response(mut res: reqwest::Response) -> Error {
        let status = res.status().as_u16();
        let url = res.url().to_string();
        if status == 429 {
            crate::agents::limited(&url);
        }
        // A refusal's body is either a short explanation or a whole error
        // page, whose sentence comes after its styles; it is read down to
        // that sentence rather than shown as markup.
        let retry_after = crate::retry_after(res.headers());
        /* Read only as far as an explanation goes, and for no longer than
         * a person would wait for one. Read whole (and decompressed whole)
         * before it was cut down, a 500 whose body never ended, or a small
         * gzip that unpacks to gigabytes, ran dm out of memory. */
        const MOST: usize = 64 * 1024;
        let mut head = Vec::new();
        let _ = tokio::time::timeout(EXPLAINED_WITHIN, async {
            while head.len() < MOST {
                match res.chunk().await {
                    Ok(Some(chunk)) => head.extend_from_slice(&chunk),
                    _ => break,
                }
            }
        })
        .await;
        drop(res);
        head.truncate(MOST);
        let body = worth_saying(
            &readable(
                &String::from_utf8_lossy(&head)
                    .chars()
                    .take(16_000)
                    .collect::<String>(),
            ),
            status,
        );
        Error::Refused {
            status,
            url,
            body,
            retry_after,
        }
    }
}

/// A status as a person reads it: "404 Not Found", not "404".
pub(crate) fn status_line(status: u16) -> String {
    match reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
    {
        Some(reason) => format!("{status} {reason}"),
        None => status.to_string(),
    }
}

/// What a refusal's body adds to its status, or nothing when it adds
/// nothing. Most error pages only restate the status in the server's
/// boilerplate: Python's "Error response Error code: 404 Message: File not
/// found. Error code explanation: …", nginx's "404 Not Found nginx/1.24",
/// Go's "404 page not found". A failed row read like a stack of them; it
/// now says "the server answered 404 Not Found", and keeps a body that
/// says something of its own ("Too many requests, please contact …").
pub(crate) fn worth_saying(body: &str, status: u16) -> String {
    // Python's http.server: boilerplate around a message, which is its own
    // ("Token expired") or a restatement ("File not found"). Only the
    // message is judged, and said.
    let body = match body.strip_prefix("Error response Error code:") {
        Some(rest) => {
            let message = rest.split_once("Message:").map_or("", |(_, m)| m);
            let message = message
                .split_once("Error code explanation:")
                .map_or(message, |(m, _)| m);
            message.trim().trim_end_matches('.').trim()
        }
        None => body,
    };
    let lower = body.to_ascii_lowercase();
    let reason = reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let code = status.to_string();
    let rest = lower.replace(&reason, " ").replace(&code, " ");
    const FILLER: [&str; 9] = [
        "error", "page", "response", "http", "status", "code", "the", "message", "file",
    ];
    // What is left once the status and its filler are gone; a server's
    // signature ("nginx/1.24.0") is not a reason either.
    let said = rest
        .split(|c: char| !c.is_alphanumeric() && c != '/' && c != '.')
        .map(|word| word.trim_matches('.'))
        .filter(|word| !FILLER.contains(word))
        .filter(|word| {
            !word
                .split_once('/')
                .is_some_and(|(_, version)| version.starts_with(|c: char| c.is_ascii_digit()))
        })
        .any(|word| word.chars().any(char::is_alphanumeric));
    if said {
        body.to_string()
    } else {
        String::new()
    }
}

/// A network failure as a person reads it. "network: error sending request
/// for url (…): client error (Connect): dns error: …" was what a mistyped
/// address showed; the cause is said first now, plainly, and the system's
/// own words for it after.
pub(crate) fn network(error: &reqwest::Error) -> String {
    // Sent nowhere because the proxy configuration script gave no way there:
    // the script's trouble is the cause, not the refused connection.
    if let Some(why) = error.url().and_then(crate::pac::refusal) {
        return format!("the proxy configuration script gave no way to this site: {why}");
    }
    // A name a public address led to, found inside alone (`dns::LedInside`): an inward
    // redirect's words, not a name that could not be found (gaps row 200).
    if crate::dns::refused_inside(error) {
        let to = error.url().map(reqwest::Url::as_str).unwrap_or_default();
        return format!("{} ({to})", crate::INWARD_REDIRECT);
    }
    let mut innermost = String::new();
    let mut refused = false;
    let mut cause: Option<&dyn std::error::Error> = Some(error);
    while let Some(next) = cause {
        if let Some(io) = next.downcast_ref::<std::io::Error>() {
            refused |= io.kind() == std::io::ErrorKind::ConnectionRefused;
        }
        let text = next.to_string();
        if !text.is_empty() {
            innermost = text;
        }
        cause = next.source();
    }
    let all = with_causes(error).to_ascii_lowercase();
    let plain = if all.contains("dns error")
        || all.contains("failed to lookup address")
        || all.contains("name or service not known")
        || all.contains("nodename nor servname")
        || all.contains("no such host")
    {
        "the server's name could not be found (check the address)"
    } else if refused || all.contains("connection refused") {
        "the server refused the connection"
    } else if error.is_timeout() || all.contains("timed out") {
        "the server did not answer in time"
    } else {
        return format!("network: {}", with_causes(error));
    };
    if innermost.is_empty() {
        plain.to_string()
    } else {
        format!("{plain}: {innermost}")
    }
}

/// An error and what caused it, each said once: "error sending request
/// for url (…): client error (Connect): tcp connect error: Connection
/// refused (os error 61)" rather than the first of those alone.
pub(crate) fn with_causes(error: &dyn std::error::Error) -> String {
    let mut said = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        let text = next.to_string();
        if !text.is_empty() && !said.contains(&text) {
            said.push_str(": ");
            said.push_str(&text);
        }
        cause = next.source();
    }
    said
}

/// What an error page says, as a sentence: the markup, its styles and
/// scripts dropped, its entities decoded, and the first two hundred
/// characters kept. Wikimedia's 403 put a page of CSS in front of "Too many
/// requests", and dm's row showed the CSS.
pub fn readable(body: &str) -> String {
    const MOST: usize = 200;
    let mut text = String::new();
    let lower = body.to_ascii_lowercase();
    let mut at = 0;
    while at < body.len() {
        let Some(rest) = body.get(at..) else { break };
        let Some(open) = rest.find('<') else {
            text.push_str(rest);
            break;
        };
        text.push_str(rest.get(..open).unwrap_or_default());
        let tag_at = at + open;
        let tag = lower.get(tag_at + 1..).unwrap_or_default();
        // A block whose content is not words is skipped to its end tag.
        let skipped = ["style", "script", "title", "head"]
            .iter()
            .find_map(|name| {
                let named = tag.starts_with(name)
                    && tag.get(name.len()..).is_some_and(|after| {
                        after.starts_with(|c: char| c == '>' || c.is_whitespace())
                    });
                named.then(|| {
                    lower
                        .get(tag_at..)
                        .and_then(|from| from.find(&format!("</{name}")))
                        .map(|end| tag_at + end)
                })
            });
        let from = match skipped {
            Some(Some(end)) => end,
            Some(None) => break,
            None => tag_at,
        };
        match body.get(from..).and_then(|from_here| from_here.find('>')) {
            Some(close) => {
                at = from + close + 1;
                text.push(' ');
            }
            None => break,
        }
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let words: Vec<&str> = decoded.split_whitespace().collect();
    let joined = words.join(" ");
    if joined.chars().count() <= MOST {
        return joined;
    }
    let cut: String = joined.chars().take(MOST).collect();
    format!("{}…", cut.trim_end())
}

/// What a file error means, in words a person reads: "No such file or
/// directory (os error 2)" said nothing a person could act on.
pub fn file_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "it is not there".into(),
        std::io::ErrorKind::PermissionDenied => "dm is not allowed to use it".into(),
        std::io::ErrorKind::AlreadyExists => "something of that name is already there".into(),
        _ => {
            // The system's words without its error number.
            let said = e.to_string();
            match said.rfind(" (os error ") {
                Some(at) => said[..at].to_string(),
                None => said,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_and_a_file_error_are_said_in_words() {
        assert_eq!(status_line(404), "404 Not Found");
        assert_eq!(status_line(599), "599");
        let said = |kind| file_error(&std::io::Error::from(kind));
        assert_eq!(said(std::io::ErrorKind::NotFound), "it is not there");
        assert_eq!(
            said(std::io::ErrorKind::PermissionDenied),
            "dm is not allowed to use it"
        );
        assert_eq!(
            said(std::io::ErrorKind::AlreadyExists),
            "something of that name is already there"
        );
        let other = file_error(&std::io::Error::from_raw_os_error(28));
        assert!(!other.contains("os error"), "{other}");
        assert_eq!(file_error(&std::io::Error::other("worn out")), "worn out");
    }

    #[tokio::test]
    async fn a_redirect_dm_does_not_follow_is_no_file() {
        let app = axum::Router::new()
            .route(
                "/away",
                axum::routing::get(|| async {
                    (
                        axum::http::StatusCode::FOUND,
                        [(axum::http::header::LOCATION, "file:///etc/passwd")],
                    )
                }),
            )
            .route(
                "/nowhere",
                axum::routing::get(|| async { axum::http::StatusCode::MOVED_PERMANENTLY }),
            )
            .route(
                "/same",
                axum::routing::get(|| async { axum::http::StatusCode::NOT_MODIFIED }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = crate::with_tls(reqwest::Client::builder())
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let ask = |path: &str| client.get(format!("http://{addr}{path}")).send();
        let why = Error::refused_or(ask("/away").await.unwrap())
            .await
            .unwrap_err()
            .to_string();
        assert!(why.contains("sent dm on to file:///etc/passwd"), "{why}");
        let why = Error::refused_or(ask("/nowhere").await.unwrap())
            .await
            .unwrap_err()
            .to_string();
        assert!(why.contains("somewhere else"), "{why}");
        assert!(Error::refused_or(ask("/same").await.unwrap()).await.is_ok());
    }
}

#[cfg(test)]
mod kind_tests {
    use super::*;

    #[test]
    fn a_kind_is_saved_as_a_word_and_read_back_and_an_unknown_word_is_other() {
        let kinds = [
            FailKind::Other,
            FailKind::Refused(404),
            FailKind::WebPage,
            FailKind::DiskFull,
            FailKind::NoRoom,
        ];
        for kind in kinds {
            let saved = serde_json::to_string(&kind).unwrap();
            assert_eq!(
                serde_json::from_str::<FailKind>(&saved).unwrap(),
                kind,
                "{saved}"
            );
        }
        assert_eq!(
            serde_json::to_string(&FailKind::Refused(410)).unwrap(),
            "\"refused 410\""
        );
        let later: FailKind = serde_json::from_str("\"a kind from a later dm\"").unwrap();
        assert_eq!(later, FailKind::Other);
    }

    #[test]
    fn an_old_sentence_gives_the_kind_its_words_were_matched_for() {
        let of = FailKind::of_old_text;
        assert_eq!(
            of("that link answered with a web page rather than the file — it may have expired"),
            FailKind::WebPage
        );
        assert_eq!(
            of("the server answered 404 Not Found for https://e.com/a.zip"),
            FailKind::Refused(404)
        );
        assert_eq!(
            of(
                "the link has expired: dm is waiting for the browser to hand over a new one \
                (the server answered 403 Forbidden for https://e.com/a)"
            ),
            FailKind::Refused(403)
        );
        assert_eq!(
            of("the MMS server answered 404 Not Found"),
            FailKind::Refused(404)
        );
        assert_eq!(
            of("the FTP server answered 550 No such file"),
            FailKind::Other
        );
        assert_eq!(
            of("not enough room: the file needs 2 GB, and /d has 1 GB free"),
            FailKind::NoRoom
        );
        assert_eq!(
            of("disk: No space left on device (os error 28)"),
            FailKind::DiskFull
        );
        assert_eq!(of("the server refused the connection"), FailKind::Other);
    }

    #[test]
    fn an_error_says_its_kind_and_its_words_are_as_made() {
        let page = Error::known(FailKind::WebPage, "a page");
        assert_eq!(page.to_string(), "a page");
        assert!(page.link_expired());
        let full = Error::Disk(std::io::Error::from(std::io::ErrorKind::StorageFull));
        assert_eq!(full.fail_kind(), FailKind::DiskFull);
        assert_eq!(
            Error::Internal("web page rather than the file".into()).fail_kind(),
            FailKind::Other
        );
    }
}
