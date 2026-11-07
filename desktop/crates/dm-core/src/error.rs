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

    /// A sentence written for the person reading the row. It used to be
    /// prefixed "internal:", which made a site that wants a sign-in read as
    /// a fault in dm.
    #[error("{0}")]
    Internal(String),
}

impl Error {
    /// The delay a refusal asked for, where it asked for one.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Error::Refused { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

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
        match self {
            Error::Refused { status, .. } => matches!(status, 403 | 410),
            Error::Internal(said) => said.contains("web page rather than the file"),
            _ => false,
        }
    }

    /// Turns a refused response into an error that says what refused and why.
    ///
    /// One place, because both the probe and the range fetch need it and the
    /// second one used to build it by hand with an empty body — so a refusal
    /// that happened mid-download explained nothing at all.
    pub async fn from_response(mut res: reqwest::Response) -> Error {
        let status = res.status().as_u16();
        let url = res.url().to_string();
        if status == 429 {
            crate::agents::limited(&url);
        }
        // A refusal's body is either a short explanation or a whole error
        // page, whose sentence comes after its styles; it is read down to
        // that sentence rather than shown as markup.
        let retry_after = crate::retry::retry_after(res.headers());
        /* Read only as far as an explanation goes, and for no longer than
         * a person would wait for one. Read whole (and decompressed whole)
         * before it was cut down, a 500 whose body never ended, or a small
         * gzip that unpacks to gigabytes, ran dm out of memory. */
        const MOST: usize = 64 * 1024;
        let mut head = Vec::new();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
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
