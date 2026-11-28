// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Asking a server what it holds, before deciding how to fetch it.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use percent_encoding::percent_decode_str;

use crate::Error;

/// What one request taught us about a URL.
#[derive(Debug, Clone)]
pub struct Probe {
    /// `None` where the server would not say, which is not the same as zero.
    pub size: Option<u64>,
    /// Whether the server answers range requests.
    pub resumable: bool,
    /// The name the server suggested, or the one in the address; empty if none.
    pub filename: String,
    /// ETag or Last-Modified, whichever the server offered. Sent back as
    /// `If-Range` so a file republished mid-download is noticed rather than
    /// grafted onto the bytes of the old one.
    pub validator: Option<String>,
    /// After redirects. A refusal is often not from the address you asked
    /// for but from wherever it sent you.
    pub final_url: String,
    /// What the server called it. Only used to sort a file whose name says
    /// nothing — plenty of media arrives from a URL with no extension.
    pub content_type: Option<String>,
    /// When the server says the file last changed (`Last-Modified`), for a
    /// finished file to carry that date where asked.
    pub modified: Option<String>,
    /// The digest the answer's headers give for the whole file (`Repr-Digest`, `Digest`,
    /// `Content-MD5`), which a finished file is checked against as a checksum file's is (gaps
    /// row W19).
    pub digest: Option<crate::Checksum>,
}

/// Learns a URL's length, name and whether it can be resumed, draining the
/// body so the connection goes back to the pool.
///
/// For callers that do not need the body (mirror probes, rechecks). The
/// main download path calls [`probe_get`] instead and keeps the body
/// streaming as its first piece.
pub async fn probe(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Probe, Error> {
    // A file host's share address is asked about as its file is, as the download fetches it
    // (gaps row W7); every other address as it is.
    let shared = crate::filehost::file_address(client, url).await?;
    let url = shared.as_deref().unwrap_or(url);
    let ProbeWithBody {
        probe, response, ..
    } = probe_get(client, url, referrer, sign_in, None).await?;
    if response.content_length().is_some_and(|n| n <= REUSE_BELOW) {
        let _ = tokio::time::timeout(REUSE_WITHIN, response.bytes()).await;
    }
    Ok(probe)
}

/// A probe whose response body has not been read yet.
pub struct ProbeWithBody {
    /// What the first request taught.
    pub probe: Probe,
    /// The answer, its body not yet read.
    pub response: reqwest::Response,
    /// The last byte the body brings, where the first request asked for less
    /// than the whole file and the file goes on past it; `None` where the body
    /// runs to the file's end (or the server would not say where it stops).
    pub brings: Option<u64>,
}

/// How much of the file the first request asks for where the download will be
/// fetched on several connections. A file whose range 0 is longer (over 64 MiB
/// on eight connections) has the first answer end inside range 0, and the
/// connection that read it asks for the rest of the range: nothing past range
/// 0's end is sent only to be dropped. Where range 0 is shorter the answer is
/// read to range 0's end and let go of, as an answer running to the file's end
/// was, but no more than this is ever sent for it.
///
/// Not as short as a range can be (`plan::MIN_SEGMENT`): every part asked for
/// apart is one request more, and a server that takes few requests a second
/// refuses the one that comes soon after EDM's first wave of them, and nothing
/// was fetched on that connection for half a second (`tests/limits.rs`, a
/// mirror taking two connections and a request a second). Asked once eight
/// mebibytes have come, that request comes seconds later on such a server.
pub(crate) const FIRST_ASK: u64 = 8 * 1024 * 1024;

/// Learns a URL's length, name and whether it can be resumed, keeping the
/// body streaming so the caller can read it as the download's first piece.
///
/// Sends `Range: bytes=0-` rather than `bytes=0-0`: a 206 confirms ranges
/// honestly while delivering every byte from the start, and the connection
/// that learned about the file is the one that fetches its first piece —
/// no round trip wasted. A 200 (ranges not supported) delivers the whole
/// file on one connection.
///
/// With `first`, it asks for that many bytes at most (`bytes=0-N`): a file
/// fetched on several connections has range 0 end long before the file does,
/// and an answer running to the file's end was sent on past range 0 until EDM
/// let go of it. The length still comes from `Content-Range`. A 206 that
/// names no total, or comes encoded, says nothing a plan can be made on, and
/// its body is no more than `first` bytes of a file of unknown length: the
/// whole file is asked for instead, as without `first`.
///
/// Its future is boxed and named `Send`: it is made inside a retry's closure, and an async
/// call taking several references there leaves rustc unable to prove the download's spawned
/// task `Send` ("implementation of `Send` is not general enough").
pub fn probe_get<'a>(
    client: &'a reqwest::Client,
    url: &'a str,
    referrer: Option<&'a str>,
    sign_in: Option<&'a crate::cookies::SignIn>,
    first: Option<u64>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ProbeWithBody, Error>> + Send + 'a>>
{
    Box::pin(probe_get_now(client, url, referrer, sign_in, first))
}

/// [`probe_get`]'s work.
async fn probe_get_now(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
    first: Option<u64>,
) -> Result<ProbeWithBody, Error> {
    // A form's answer: sent once, as the browser sent it, with no range and
    // none of the second tries below, each of which would send it again.
    let posted = sign_in.and_then(|s| s.post_for(url)).is_some();
    let bounded = first.filter(|&n| n > 0 && !posted);
    let wanted = bounded.map_or_else(|| WHOLE.to_owned(), |n| format!("bytes=0-{}", n - 1));
    let asking = Asking {
        client,
        url,
        referrer,
        sign_in,
        posted,
        wanted,
    };
    let mut res = asking.answered().await?;

    let status = res.status();
    let empty = status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE
        && res
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim() == "bytes */0");
    if !empty && (status.is_client_error() || status.is_server_error()) {
        return Err(Error::from_response(res).await);
    }

    /* Asked for the first part only, and answered with a part that names no
     * total or comes encoded: no plan can be made on it, and read as the file
     * it would be the file cut short. Asked again for the whole, as an
     * unbounded probe asks, and that answer is the one read or refused. */
    let mut brings = None;
    if bounded.is_some() && status == reqwest::StatusCode::PARTIAL_CONTENT {
        match first_part(res.headers()) {
            Some((last, total)) => {
                brings = (last.saturating_add(1) < total).then_some(last);
            }
            None => {
                let whole = asking
                    .ask_as(crate::cookies::agent_with(sign_in, url), Some(WHOLE))
                    .await?;
                if !whole.status().is_success() {
                    return Err(Error::from_response(whole).await);
                }
                res = whole;
            }
        }
    }
    Ok(described(res, url, empty, posted, brings))
}

/// How `probe_get` asks about an address: by which client, with what referrer and sign-in, and
/// for what range.
struct Asking<'a> {
    client: &'a reqwest::Client,
    url: &'a str,
    referrer: Option<&'a str>,
    sign_in: Option<&'a crate::cookies::SignIn>,
    /// A form's answer: sent once, with no range and no second tries.
    posted: bool,
    /// The `Range` asked for.
    wanted: String,
}

impl Asking<'_> {
    /// One request by the name `agent`, for `range` where a range goes.
    async fn ask_as(&self, agent: &str, range: Option<&str>) -> Result<reqwest::Response, Error> {
        let (client, url, referrer, sign_in, posted) = (
            self.client,
            self.url,
            self.referrer,
            self.sign_in,
            self.posted,
        );
        let mut request = crate::download::with_form(client, url, sign_in)
            .header(reqwest::header::USER_AGENT, agent)
            .header(reqwest::header::ACCEPT_ENCODING, crate::download::IDENTITY);
        if let Some(range) = range.filter(|_| !posted) {
            request = request.header(reqwest::header::RANGE, range);
        }
        let cookies = crate::cookies::header_with(sign_in, url);
        request =
            crate::download::stream_headers().onto(request, cookies.is_some(), referrer.is_some());
        if let Some(cookies) = cookies {
            request = request.header(reqwest::header::COOKIE, cookies);
        }
        match referrer {
            Some(referrer) => request = request.header(reqwest::header::REFERER, referrer),
            None => {
                if let Some(folder) = crate::download::folder_referer(url) {
                    request = request.header(reqwest::header::REFERER, folder);
                }
            }
        }
        request = crate::cookies::add_headers(request, sign_in, url);
        crate::auth::send(client, request, url).await
    }

    /// One request by the name `agent`, for the range wanted.
    async fn ask(&self, agent: &str) -> Result<reqwest::Response, Error> {
        self.ask_as(agent, Some(self.wanted.as_str())).await
    }

    /// The answer to the first request, or to the one asked again where a server refused it in
    /// a way another name, no made-up `Referer` or a second try can mend.
    async fn answered(&self) -> Result<reqwest::Response, Error> {
        let (url, referrer, sign_in, posted) = (self.url, self.referrer, self.sign_in, self.posted);
        // A name the person gave this download is the one it is asked by, every time: none of the
        // other names below is tried in its place.
        let own = sign_in.and_then(crate::cookies::SignIn::agent);
        let mut res = self.ask(crate::cookies::agent_with(sign_in, url)).await?;
        if !posted
            && own.is_none()
            && res.status() == reqwest::StatusCode::FORBIDDEN
            && crate::agent_for(url) == crate::first_agent()
        {
            for agent in crate::FALLBACK_AGENTS
                .into_iter()
                .filter(|a| *a != crate::first_agent())
            {
                let again = self.ask(agent).await?;
                if again.status() != reqwest::StatusCode::FORBIDDEN {
                    crate::prefer_agent(url, agent);
                    crate::prefer_agent(again.url().as_str(), agent);
                    res = again;
                    break;
                }
            }
        }
        /* A browser's name told "429 Too many requests" at once, where EDM's own is
         * served: Wikimedia's media servers (2026-11-07) gave Internet Explorer's
         * and Firefox's names `Retry-After: 600` from the first request ("please
         * contact noc@wikimedia.org to discuss a less disruptive approach") and
         * EDM's name the file, as their policy asks a program to say what it is.
         * Asked once more by EDM's own name, and by its name alone where a site
         * refuses the full one; a server that limits every name answers 429 to
         * that too and is waited out as before, one request more than it was. */
        if !posted
            && own.is_none()
            && res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            && crate::agent_for(url) == crate::IE_AGENT
        {
            for agent in [crate::USER_AGENT, crate::PLAIN_AGENT] {
                let again = self.ask(agent).await?;
                match again.status() {
                    reqwest::StatusCode::FORBIDDEN => continue,
                    reqwest::StatusCode::TOO_MANY_REQUESTS => break,
                    _ => {
                        crate::prefer_agent(url, agent);
                        crate::prefer_agent(again.url().as_str(), agent);
                        res = again;
                        break;
                    }
                }
            }
        }
        // Still refused, with a `Referer` EDM made up (none was given): asked once
        // more without it. A server that refuses any `Referer` but its own pages
        // (thinkbroadband) served reference A and refused EDM on every agent.
        if !posted
            && res.status() == reqwest::StatusCode::FORBIDDEN
            && referrer.is_none()
            && crate::download::folder_referer(url).is_some()
        {
            crate::drop_made_up_referer(url);
            let again = self.ask(crate::cookies::agent_with(sign_in, url)).await?;
            if again.status() == reqwest::StatusCode::FORBIDDEN {
                crate::forget_dropped_referer(url);
            } else {
                crate::drop_made_up_referer(again.url().as_str());
                res = again;
            }
        }

        // A 404 is asked for a second time before it is said, as reference A does.
        if !posted && res.status() == reqwest::StatusCode::NOT_FOUND {
            res = self.ask(crate::cookies::agent_with(sign_in, url)).await?;
        }

        // Some servers fail a ranged request and serve the plain one, which is how
        // reference A asks first. Taken only if the plain request succeeds; the
        // ranged answer's error is what is said otherwise.
        if !posted && res.status().is_server_error() {
            if let Ok(plain) = self
                .ask_as(crate::cookies::agent_with(sign_in, url), None)
                .await
            {
                if plain.status().is_success() {
                    res = plain;
                }
            }
        }
        Ok(res)
    }
}

/// What the answer `res` to the address `url` says of the file: its size, whether it resumes,
/// its name and the rest. `empty` where it is a file of no bytes, `posted` for a form's answer,
/// `brings` the last byte the body brings where it stops short of the file's end.
fn described(
    res: reqwest::Response,
    url: &str,
    empty: bool,
    posted: bool,
    brings: Option<u64>,
) -> ProbeWithBody {
    let status = res.status();

    let headers = res.headers().clone();
    let final_url = keep_login(url, res.url());

    let header = |name: reqwest::header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };

    let (size, resumable) = if empty {
        (Some(0), false)
    } else if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let total = header(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.rsplit('/').next().map(str::to_owned))
            .and_then(|t| t.parse::<u64>().ok());
        (total, total.is_some())
    } else {
        (
            header(reqwest::header::CONTENT_LENGTH).and_then(|v| v.parse().ok()),
            false,
        )
    };

    let (size, resumable) = if crate::decode::coding_of(&headers).is_some() {
        (None, false)
    } else {
        // A form's answer is fetched whole, by sending the form again.
        (size, resumable && !posted)
    };
    // Only a part of a file whose plan can go on past it.
    let brings = brings.filter(|_| resumable && size.is_some());

    let validator =
        header(reqwest::header::ETAG).or_else(|| header(reqwest::header::LAST_MODIFIED));

    let content_type = header(reqwest::header::CONTENT_TYPE);
    let modified = header(reqwest::header::LAST_MODIFIED);
    // A 206 from the file's first byte to its last is the whole file, as a 200 is.
    let whole = status == reqwest::StatusCode::OK
        || (status == reqwest::StatusCode::PARTIAL_CONTENT
            && first_part(&headers).is_some_and(|(last, total)| last.saturating_add(1) == total));
    let digest = if empty {
        None
    } else {
        crate::checksum::from_headers(&headers, whole)
    };
    let filename = filename_for(
        headers
            .get(reqwest::header::CONTENT_DISPOSITION)
            .map(|v| header_text(v.as_bytes()))
            .as_deref(),
        &final_url,
        content_type.as_deref(),
    );

    ProbeWithBody {
        probe: Probe {
            modified,
            digest,
            size,
            resumable,
            filename,
            validator,
            final_url,
            content_type,
        },
        response: res,
        brings,
    }
}

/// The `Range` that asks for the whole file from its first byte.
const WHOLE: &str = "bytes=0-";

/// The last byte and the file's length a first answer's `Content-Range` names,
/// where it starts at byte 0, names a total and comes unencoded; `None`
/// otherwise.
fn first_part(headers: &reqwest::header::HeaderMap) -> Option<(u64, u64)> {
    if crate::decode::coding_of(headers).is_some() {
        return None;
    }
    let said = headers
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .trim()
        .strip_prefix("bytes ")?;
    let (span, total) = said.split_once('/')?;
    let (from, last) = span.split_once('-')?;
    let (from, last) = (
        from.trim().parse::<u64>().ok()?,
        last.trim().parse::<u64>().ok()?,
    );
    let total = total.trim().parse::<u64>().ok()?;
    (from == 0 && last < total).then_some((last, total))
}

/// The largest probe answer read to its end so its connection is kept for
/// the download (one byte, or a short error page's worth), and how long that
/// reading may take.
const REUSE_BELOW: u64 = 16 * 1024;
const REUSE_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// Names a Windows filesystem refuses outright, whatever the extension.
///
/// Kept out on every platform: a download folder is copied between machines,
/// and a file called `con.txt` is one that cannot be written on the other end.
// COM0/LPT0, the superscript digits Windows also takes as ports, and the
// console's CONIN$/CONOUT$ were missing.
const RESERVED: &[&str] = &[
    "con",
    "prn",
    "aux",
    "nul",
    "conin$",
    "conout$",
    "com0",
    "com1",
    "com2",
    "com3",
    "com4",
    "com5",
    "com6",
    "com7",
    "com8",
    "com9",
    "com\u{b9}",
    "com\u{b2}",
    "com\u{b3}",
    "lpt0",
    "lpt1",
    "lpt2",
    "lpt3",
    "lpt4",
    "lpt5",
    "lpt6",
    "lpt7",
    "lpt8",
    "lpt9",
    "lpt\u{b9}",
    "lpt\u{b2}",
    "lpt\u{b3}",
];

/// Whether Windows takes a name for a device rather than a file: the part
/// before the first dot, any case, trailing spaces ignored. `CON.tar.gz`
/// is refused as surely as `CON`.
pub fn reserved_on_windows(name: &str) -> bool {
    let device = name.split('.').next().unwrap_or("").trim_end();
    RESERVED.contains(&device.to_ascii_lowercase().as_str())
}

/// An extension for a URL that ends in a bare path, from what the server said
/// it was sending.
const BY_MIME: &[(&str, &str)] = &[
    ("application/pdf", ".pdf"),
    ("application/zip", ".zip"),
    ("application/x-tar", ".tar"),
    ("application/gzip", ".gz"),
    ("application/x-gzip", ".gz"),
    ("application/json", ".json"),
    ("video/mp4", ".mp4"),
    ("video/webm", ".webm"),
    ("video/x-matroska", ".mkv"),
    ("video/mp2t", ".ts"),
    ("audio/mpeg", ".mp3"),
    ("audio/mp4", ".m4a"),
    ("audio/ogg", ".ogg"),
    ("image/jpeg", ".jpg"),
    ("image/png", ".png"),
    ("image/gif", ".gif"),
    ("image/webp", ".webp"),
    ("text/html", ".html"),
    ("text/plain", ".txt"),
];

/// A name to save under.
///
/// Content-Disposition when the server offered one, the last path segment
/// otherwise, and failing both a stub with an extension taken from the
/// content type. A server's suggestion is not to be trusted as a path —
/// anything that looks like one is reduced to its final component, so a name
/// cannot place the file somewhere it was not meant to go.
///
/// `shared/naming-cases.json` holds the rules (the extension named files by
/// the same cases when it was JavaScript; it now leaves naming to EDM).
pub fn filename_for(disposition: Option<&str>, url: &str, content_type: Option<&str>) -> String {
    if let Some(value) = disposition {
        if let Some(name) = from_disposition(value) {
            let cleaned = sanitize(&name);
            if !cleaned.is_empty() {
                return cleaned;
            }
        }
    }

    let mime = mime_essence(content_type);
    let extension = BY_MIME
        .iter()
        .find(|(m, _)| *m == mime)
        .map(|(_, e)| *e)
        .unwrap_or("");

    let cleaned = sanitize(&decode(last_path_segment(url)));
    if !cleaned.is_empty() {
        // A name from the address with no extension — `/get`, `/download`
        // — takes the one its type says, so the file opens with the right
        // program. Found by the server zoo: a PDF was saved as "get".
        if !cleaned.contains('.') && !extension.is_empty() {
            return sanitize(&format!("{cleaned}{extension}"));
        }
        return cleaned;
    }

    format!("download{extension}")
}

fn from_disposition(value: &str) -> Option<String> {
    // RFC 5987's filename* wins where present: it carries an encoding and a
    // charset, which is how a non-ASCII name survives at all.
    if let Some(at) = value.to_ascii_lowercase().find("filename*=") {
        let rest = &value[at + "filename*=".len()..];
        let raw = rest.split(';').next()?.trim();
        let encoded = raw.rsplit('\'').next()?;
        return Some(decode(encoded));
    }
    let at = value.to_ascii_lowercase().find("filename=")?;
    let rest = &value[at + "filename=".len()..];
    let raw = rest.split(';').next()?.trim();
    Some(raw.trim_matches('"').to_string())
}

/// The bare MIME type, with any `;charset=` parameter removed.
///
/// Both the naming rules and the category rules want this, and both had their
/// own copy of the same four calls.
pub fn mime_essence(content_type: Option<&str>) -> String {
    content_type
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Percent-decoding, from the crate reqwest already brings in.
pub fn decode(input: &str) -> String {
    percent_decode_str(input).decode_utf8_lossy().into_owned()
}

/// Reduces a server's suggestion to something safe to create.
///
/// Separators go first so a name can only ever be a name — `../../etc/passwd`
/// becomes `passwd`, not a path. The reserved characters that follow are the
/// ones Windows refuses outright, kept out on every platform so a download
/// folder copied between machines does not become unreadable.
/// Below the 255-byte component limit, with room for ` (12)` and a
/// `.part`-style suffix.
const MAX_NAME_BYTES: usize = 240;

/// A header's text when it is not plain ASCII. Servers send a name like
/// `filename="résumé.pdf"` as raw UTF-8 more often than the standard's
/// `filename*=`, and older ones as Latin-1; taking only ASCII threw the
/// name away and saved the file under the address's last segment.
fn header_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// A name made safe to use as a file name: its folder part cut off and characters the disk refuses replaced.
#[must_use]
pub fn sanitize(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = last
        .chars()
        // Direction controls go: a right-to-left override shows
        // "invoice\u{202e}fdp.exe" as "invoiceexe.pdf", a program
        // dressed as a document.
        .filter(|c| {
            !matches!(
                *c,
                '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}'
            )
        })
        .map(|c| {
            if "<>:\"|?*".contains(c) || (c as u32) < 0x20 || c as u32 == 0x7f {
                '_'
            } else {
                c
            }
        })
        .collect();

    // Windows silently drops trailing dots and spaces, which would leave the
    // name recorded and the name on disk out of step. Leading dots go too, so
    // a download cannot arrive hidden.
    // Until nothing changes: removing a leading dot can uncover a space,
    // and ". A" became " A" — a name that cleaned again came out different,
    // found by the fuzz tests.
    let mut trimmed = cleaned.as_str();
    loop {
        let next = trimmed
            .trim()
            .trim_end_matches(['.', ' '])
            .trim_start_matches('.');
        if next == trimmed {
            break;
        }
        trimmed = next;
    }
    let trimmed = trimmed.to_string();
    if trimmed.is_empty() {
        return trimmed;
    }

    let extension = extension_of(&trimmed);

    let mut out = if reserved_on_windows(&trimmed) {
        format!("_{trimmed}")
    } else {
        trimmed
    };

    /* Most filesystems cap one path component at 255 bytes, and a refusal at
     * save time loses a download that has already arrived. The stem is cut
     * rather than the tail, so the file stays openable by the thing that
     * knows what to do with a .mkv. */
    // Bytes as well as characters: two hundred CJK characters are six
    // hundred bytes, which ext4 and NTFS refuse after the whole download.
    if out.chars().count() > 200 || out.len() > MAX_NAME_BYTES {
        let mut tail = if !extension.is_empty() && extension.chars().count() <= 21 {
            extension
        } else {
            String::new()
        };
        // A compressed tar keeps both halves: `.tar.gz` cut to `.gz` was a
        // file nothing offered to unpack as an archive of files.
        let stem = out.get(..out.len() - tail.len()).unwrap_or("");
        if !tail.is_empty()
            && stem
                .get(stem.len().saturating_sub(4)..)
                .is_some_and(|s| s.eq_ignore_ascii_case(".tar"))
        {
            tail = format!("{}{tail}", stem.get(stem.len() - 4..).unwrap_or(""));
        }
        let mut keep = String::new();
        for c in out.chars().take(200 - tail.chars().count()) {
            if keep.len() + c.len_utf8() + tail.len() > MAX_NAME_BYTES {
                break;
            }
            keep.push(c);
        }
        // A cut can end on a space or a dot, which Windows drops and the
        // next clean trims: the name recorded and the name on disk differ.
        out = format!(
            "{}{tail}",
            keep.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
        );
    }
    out
}

/// The address a download is fetched from after its probe, keeping the
/// login it was given.
///
/// reqwest sends a user and password carried in the address — "Needs a
/// sign-in" and Sign-ins put them there — but the address it
/// reports back has them removed, and every range was fetched from that one:
/// the probe passed and each range was answered 401. Kept only where the
/// host and port are the ones the login was given for: another port is
/// another server, as it is for a redirect (`redirects::other_host`).
fn keep_login(asked: &str, landed: &url::Url) -> String {
    let Ok(asked) = url::Url::parse(asked) else {
        return landed.to_string();
    };
    if asked.username().is_empty()
        || asked.host_str() != landed.host_str()
        || asked.port_or_known_default() != landed.port_or_known_default()
    {
        return landed.to_string();
    }
    let mut landed = landed.clone();
    let _ = landed.set_username(asked.username());
    let _ = landed.set_password(asked.password());
    landed.to_string()
}

/// The last non-empty segment of a URL's path, and nothing else.
///
/// The path has to be separated from the host first. Splitting the whole URL
/// on `/` and taking the last non-empty part answers `e.com` for
/// `https://e.com/`, which is a hostname saved as a filename.
///
/// Empty segments are skipped after that, so a URL ending in a slash falls
/// back to the segment before it — `/downloads/report/` names something.
fn last_path_segment(url: &str) -> &str {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let path = match without_query.split_once("://") {
        Some((_, authority_and_path)) => match authority_and_path.split_once('/') {
            Some((_, path)) => path,
            None => "",
        },
        None => without_query,
    };
    path.rsplit('/').find(|part| !part.is_empty()).unwrap_or("")
}

/// The trailing `.ext`, dot included, or empty where there is not one.
fn extension_of(name: &str) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot < name.len() - 1 => name[dot..].to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
