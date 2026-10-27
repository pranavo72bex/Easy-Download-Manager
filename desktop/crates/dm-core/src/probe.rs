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
    pub resumable: bool,
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
}

/// Learns a URL's length, name and whether it can be resumed.
///
/// A one-byte ranged GET rather than HEAD: many servers answer HEAD
/// inaccurately or not at all, and the status code a range request comes back
/// with is the only honest answer to "will you serve ranges". A 206 means
/// yes, from the one party whose opinion counts.
pub async fn probe(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Probe, Error> {
    let ask = |agent: &str| {
        let mut request = client
            .get(url)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .header(reqwest::header::USER_AGENT, agent)
            // The length of the file itself, as the download will ask for it.
            .header(reqwest::header::ACCEPT_ENCODING, crate::download::IDENTITY);
        if let Some(cookies) = crate::cookies::header_with(sign_in, url) {
            request = request.header(reqwest::header::COOKIE, cookies);
        }
        // A hotlink-protected server serves the file only to a request that
        // names the page it came from. Fetch cannot set Referer from a
        // browser, so the extension records it and hands it over; here it
        // goes on the wire.
        if let Some(referrer) = referrer {
            request = request.header(reqwest::header::REFERER, referrer);
        }
        // Signed in as the server asks, where the address carries a login.
        crate::auth::send(client, request, url)
    };
    let mut res = ask(crate::agent_for(url)).await?;
    // Refused for how dm named itself? Asked once more by its name alone,
    // then as a browser asks; the first that answers is that site's agent
    // for the rest of the download (see FALLBACK_AGENTS).
    if res.status() == reqwest::StatusCode::FORBIDDEN && crate::agent_for(url) == crate::USER_AGENT
    {
        for agent in crate::FALLBACK_AGENTS {
            let again = ask(agent).await?;
            if again.status() != reqwest::StatusCode::FORBIDDEN {
                crate::prefer_agent(url, agent);
                crate::prefer_agent(again.url().as_str(), agent);
                res = again;
                break;
            }
        }
    }

    let status = res.status();
    // `bytes=0-0` of an empty file is unsatisfiable, and the 416 says so
    // with `Content-Range: bytes */0`. That is a length, not a refusal.
    let empty = status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE
        && res
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim() == "bytes */0");
    if !empty && (status.is_client_error() || status.is_server_error()) {
        return Err(Error::from_response(res).await);
    }

    let headers = res.headers().clone();
    let final_url = keep_login(url, res.url());

    let header = |name: reqwest::header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };

    // 206 carries the real length in Content-Range's total, past the slash.
    // Content-Length on a partial response describes the one byte we asked
    // for, so reading that instead would report every file as one byte long.
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

    // A body in a coding is undone as it comes (`decode.rs`): its length is
    // that of the encoded bytes, and says nothing of the file's.
    let (size, resumable) = if crate::decode::coding_of(&headers).is_some() {
        (None, false)
    } else {
        (size, resumable)
    };

    let validator =
        header(reqwest::header::ETAG).or_else(|| header(reqwest::header::LAST_MODIFIED));

    /* The byte asked for is read, so the connection goes back to the pool.
     *
     * A response dropped with its body unread is a connection closed: the
     * download's first range then opened a new one, a TCP and a TLS
     * handshake — two or three round trips to a distant server — before its
     * first byte, beside a connection already open to the same server. Only
     * a small answer, and only for so long: a server that ignored the range
     * and is sending the whole file is not read here, and its connection
     * goes as before. */
    if res.content_length().is_some_and(|n| n <= REUSE_BELOW) {
        let _ = tokio::time::timeout(REUSE_WITHIN, res.bytes()).await;
    }

    let content_type = header(reqwest::header::CONTENT_TYPE);
    let modified = header(reqwest::header::LAST_MODIFIED);
    let filename = filename_for(
        headers
            .get(reqwest::header::CONTENT_DISPOSITION)
            .map(|v| header_text(v.as_bytes()))
            .as_deref(),
        &final_url,
        content_type.as_deref(),
    );

    Ok(Probe {
        modified,
        size,
        resumable,
        filename,
        validator,
        final_url,
        content_type,
    })
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
/// it was sending. Matches `MIME_EXTENSION` in `naming.js`.
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
/// The rules are `naming.js`'s, and `shared/naming-cases.json` holds them to
/// it: the same download saved under two names depending on which half of the
/// product took it is a bug people report as a lost file.
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
            extension.clone()
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
        out = keep
            .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
            .to_string()
            + &tail;
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
/// host is the one the login was given for.
fn keep_login(asked: &str, landed: &url::Url) -> String {
    let Ok(asked) = url::Url::parse(asked) else {
        return landed.to_string();
    };
    if asked.username().is_empty() || asked.host_str() != landed.host_str() {
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
/// back to the segment before it — `/downloads/report/` names something, and
/// `naming.js` has always read it that way.
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
mod tests {
    use super::*;

    /* The cases live in `shared/naming-cases.json` and the extension's suite
     * reads the same file. The two halves of the product were naming the same
     * download differently — no reserved names here, truncation that cut the
     * extension off, no MIME fallback — and a rule added to one and not the
     * other is exactly the sort of thing nobody notices until a file is
     * missing. Now it fails in whichever language lacks it. */
    #[test]
    fn the_naming_rules_are_the_same_as_the_extension_s() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../../../shared/naming-cases.json")).unwrap();
        let cases = cases["cases"].as_array().expect("no cases in the fixture");
        assert!(cases.len() > 10, "the fixture lost its cases");

        for case in cases {
            let text = |key: &str| case[key].as_str().map(str::to_owned);
            let got = filename_for(
                text("disposition").as_deref(),
                case["url"].as_str().unwrap_or(""),
                text("contentType").as_deref(),
            );
            let what = case["name"].as_str().unwrap_or("");

            if let Some(expected) = text("expect") {
                assert_eq!(got, expected, "{what}");
            }
            if let Some(tail) = text("expectEndsWith") {
                assert!(got.ends_with(&tail), "{what}: {got} does not end {tail}");
            }
            if let Some(most) = case["expectLengthAtMost"].as_u64() {
                assert!(
                    got.chars().count() as u64 <= most,
                    "{what}: {} characters",
                    got.chars().count()
                );
            }
            if let Some(most) = case["expectBytesAtMost"].as_u64() {
                assert!(got.len() as u64 <= most, "{what}: {} bytes", got.len());
            }
        }
    }

    #[test]
    fn a_name_cannot_become_a_path() {
        // The whole point: a server does not get to choose where a file goes.
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("C:\\Windows\\evil.exe"), "evil.exe");
    }

    #[test]
    fn a_name_made_entirely_of_things_that_are_stripped_is_not_a_name() {
        // Falls through to the URL, then to the stub, rather than producing
        // an empty path that would be an error at save time.
        assert_eq!(sanitize("..."), "");
        assert_eq!(sanitize("   "), "");
        assert_eq!(
            filename_for(
                Some("attachment; filename=\"...\""),
                "https://e.com/a.zip",
                None
            ),
            "a.zip"
        );
    }

    #[test]
    fn truncation_keeps_the_extension_rather_than_the_end_of_the_stem() {
        // A 300-character name with a short tail: the tail is the part that
        // decides what opens the file.
        let long = format!("{}.mkv", "a".repeat(300));
        let cut = sanitize(&long);
        assert!(cut.ends_with(".mkv"), "the extension was cut off: {cut}");
        assert_eq!(cut.chars().count(), 200);
        // A compressed tar keeps both halves of its extension.
        let tar = format!("{}.tar.gz", "a".repeat(258));
        let cut = sanitize(&tar);
        assert!(cut.ends_with("a.tar.gz"), "{cut}");
        assert_eq!(cut.chars().count(), 200);
        let upper = sanitize(&format!("{}.TAR.XZ", "b".repeat(258)));
        assert!(upper.ends_with("b.TAR.XZ"), "{upper}");
        // Only a tar: another name's dot is not taken for one.
        let plain = sanitize(&format!("{}.v2.gz", "c".repeat(258)));
        assert!(
            plain.ends_with("c.gz") || plain.ends_with("cc.gz"),
            "{plain}"
        );
    }

    #[test]
    fn a_name_sent_as_raw_utf8_or_latin1_is_kept() {
        let utf8 = header_text("attachment; filename=\"résumé.pdf\"".as_bytes());
        assert_eq!(
            filename_for(Some(&utf8), "https://e.com/utf8", None),
            "résumé.pdf"
        );
        let latin1 = header_text(b"attachment; filename=\"r\xe9sum\xe9.pdf\"");
        assert_eq!(
            filename_for(Some(&latin1), "https://e.com/lat1", None),
            "résumé.pdf"
        );
    }

    #[test]
    fn a_name_cannot_turn_its_extension_around() {
        assert_eq!(sanitize("invoice\u{202e}fdp.exe"), "invoicefdp.exe");
        assert_eq!(sanitize("a\u{2067}b\u{2069}.zip"), "ab.zip");
    }

    #[test]
    fn an_extension_too_long_to_be_one_is_not_preserved() {
        // Otherwise a "name" that is one long run after a dot keeps all of
        // itself as a tail and the truncation does nothing.
        let long = format!("a.{}", "b".repeat(300));
        assert_eq!(sanitize(&long).chars().count(), 200);
    }

    /// The reserved list lacked COM0, LPT0, the superscript ports
    /// and the console's names; and a cut could end on a space or a dot.
    #[test]
    fn every_windows_device_name_is_kept_off_and_a_cut_ends_cleanly() {
        for name in [
            "COM0.txt",
            "lpt0",
            "COM\u{b9}.log",
            "LPT\u{b3}",
            "CONIN$",
            "conout$.txt",
        ] {
            assert_eq!(sanitize(name), format!("_{name}"), "{name}");
        }
        assert_eq!(sanitize("COM10.txt"), "COM10.txt");
        let cut_on_a_space = format!("{} tail", "a".repeat(199));
        let once = sanitize(&cut_on_a_space);
        assert_eq!(once, "a".repeat(199));
        assert_eq!(sanitize(&once), once);
        let cut_on_a_dot = format!("{}.{}", "a".repeat(199), "b".repeat(40));
        assert_eq!(sanitize(&cut_on_a_dot), "a".repeat(199));
    }
}
