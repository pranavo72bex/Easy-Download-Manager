use super::*;

/// An HTTP date (`Sun, 06 Nov 1994 08:49:37 GMT`) as a time a file can
/// carry. `None` for anything else.
pub(crate) fn http_date(text: &str) -> Option<std::time::SystemTime> {
    let when = chrono::DateTime::parse_from_rfc2822(text.trim()).ok()?;
    let seconds = u64::try_from(when.timestamp()).ok()?;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
}

/// A playlist is text and small; anything claiming otherwise is not one.
pub(super) const MANIFEST_CAP: usize = 8 * 1024 * 1024;

/// Whether the address's last path segment names a file by a type dm knows
/// as one — an archive, a program, a document, media. Not any dotted name:
/// `/details/night.of.the.living.dead` and `/@some.handle` are pages.
pub(super) fn names_a_file(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let name = parsed
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("");
    name.contains('.') && crate::category::classify(name, None) != crate::category::Category::Other
}

/// Whether the address's last segment names a document by its extension —
/// `.html`, `.xml`, `.txt` — rather than a script that makes pages
/// (`.php`, `.aspx`, `.cgi`), which is how a watch page is often reached.
pub fn names_a_document(url: &str) -> bool {
    const SCRIPTS: [&str; 12] = [
        "php", "php5", "asp", "aspx", "jsp", "jspx", "cgi", "pl", "py", "do", "action", "cfm",
    ];
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let name = parsed
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("");
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=5).contains(&extension.len())
        && extension.chars().all(|c| c.is_ascii_alphanumeric())
        && !SCRIPTS.iter().any(|s| s.eq_ignore_ascii_case(extension))
}

/// Whether the body at `url` begins as an HTML document, from its first
/// bytes: a few hundred are asked for, and a server that sends the whole
/// body anyway is read no further than that.
pub(super) async fn starts_as_html(client: &reqwest::Client, url: &str, options: &Options) -> bool {
    let request = get(
        client,
        url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    )
    .header(reqwest::header::RANGE, "bytes=0-1023");
    let Ok(res) = request.send().await else {
        return false;
    };
    // An error page is not the file either, whatever it starts with.
    if !res.status().is_success() {
        return true;
    }
    let mut stream = res.bytes_stream();
    let mut head = Vec::new();
    while head.len() < 1024 {
        match futures_util::StreamExt::next(&mut stream).await {
            Some(Ok(chunk)) => head.extend_from_slice(&chunk),
            _ => break,
        }
    }
    looks_like_html(&head)
}

/// HTML by its opening: past any byte-order mark and white space, a
/// doctype, `<html`, `<head`, or a comment followed by one of them.
pub(super) fn looks_like_html(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head.get(..1024).unwrap_or(head)).to_ascii_lowercase();
    let text = text.trim_start_matches('\u{feff}').trim_start();
    [
        "<!doctype html",
        "<html",
        "<head",
        "<!--",
        "<body",
        "<title",
        "<meta",
        "<script",
        "<link",
    ]
    .iter()
    .any(|tag| text.starts_with(tag))
}

/// The file as the server keeps it, never a compressed form of it. A
/// server that compresses text ranges over the compressed bytes (GitHub's
/// raw files, apple.com), so a client that asked for gzip and unpacked it
/// got each range longer than it asked for, and every such file failed as
/// a server that does not serve ranges honestly. Also what a download is:
/// the bytes of the file, which a probe's length and a digest are of.
pub(crate) const IDENTITY: &str = "identity";

/// A request the way every request of this download is made.
///
/// One place for the `Referer`, because it was on the probe and the ranges
/// and on nothing else: a stream's playlists, key and segments, a Metalink's
/// body and a `.torrent` file all went out bare, so a download the browser
/// had just made with its page named failed in the app with a 403 one
/// request after the probe had passed.
pub(crate) fn get(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::agent_for(url))
        .header(reqwest::header::ACCEPT_ENCODING, IDENTITY);
    if let Some(cookies) = crate::cookies::header_with(sign_in, url) {
        request = request.header(reqwest::header::COOKIE, cookies);
    }
    if let Some(referrer) = referrer {
        request = request.header(reqwest::header::REFERER, referrer);
    }
    request
}

/// Fetches something small enough to hold: a manifest, a mirror list, a key.
///
/// Capped, because these are read into memory whole and the URL that produced
/// them is not necessarily one anybody vetted. The cap is enforced while
/// reading — a body that keeps coming is stopped, not buffered whole and
/// then trimmed — and the bytes count against the ceiling and can be paused
/// like any others.
///
/// With the address it was finally served from: a playlist's relative
/// entries are relative to that, which after a redirect is not where it was
/// asked for — a short link that sends a player to a CDN's directory has its
/// renditions and segments there. Joined against the asked address, every
/// one of them was a 404.
pub(crate) async fn fetch_capped(
    client: &reqwest::Client,
    url: &str,
    most: usize,
    options: &Options,
) -> Result<Fetched, Error> {
    let res = retry::run_up_to(&options.control, options.tries(), || async {
        or_stop(
            &options.control,
            crate::auth::send(
                client,
                get(
                    client,
                    url,
                    options.referrer.as_deref(),
                    options.sign_in.as_ref(),
                ),
                url,
            ),
        )
        .await
    })
    .await?;
    let res = Error::refused_or(res).await?;
    let landed = res.url().to_string();
    let mut body = Vec::new();
    // Undone where a server compressed it unasked (`decode.rs`).
    let stopped: crate::decode::Stopped = Default::default();
    let mut chunks: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>,
    > = match crate::decode::coding_of(res.headers()) {
        Some(coding) => {
            crate::decode::decoded(coding, Box::pin(res.bytes_stream()), stopped.clone())
        }
        None => Box::pin(res.bytes_stream()),
    };
    while let Some(chunk) = next_or_stop(&mut chunks, &options.control).await? {
        let chunk = chunk?;
        if body.len() + chunk.len() > most {
            return Err(Error::Internal(format!(
                "{url} is larger than anything of its kind should be"
            )));
        }
        body.extend_from_slice(&chunk);
        or_stop(&options.control, async {
            options.limiter.take(chunk.len() as u64).await;
            Ok(())
        })
        .await?;
    }
    let why = stopped.lock().ok().and_then(|slot| slot.clone());
    if let Some(why) = why {
        return Err(Error::Internal(why));
    }
    Ok(Fetched { body, landed })
}

/// Something small fetched whole, and where it was finally served from.
pub(crate) struct Fetched {
    pub body: Vec<u8>,
    pub landed: String,
}

impl Fetched {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Reads the mirror list a Metalink URL points at.
pub(crate) async fn fetch_metalink(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Metalink, Error> {
    let text = fetch_capped(client, url, 4 * 1024 * 1024, options)
        .await?
        .text();
    metalink::parse(&text)
        .ok_or_else(|| Error::Internal("that Metalink names no file dm can fetch over HTTP".into()))
}
