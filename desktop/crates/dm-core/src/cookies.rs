// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Cookies from a `cookies.txt` file, sent on dm's own requests.
//!
//! A download that needs a signed-in session reaches dm with it one of two
//! ways. The browser hands a site's sign-in over with the download, where
//! the person asked it to (`SignIn`, carried by that download alone). Or a
//! person exports a `cookies.txt` (the Netscape format every "get
//! cookies.txt" add-on writes, and that curl reads)
//! and points dm at it; dm then sends the matching cookies with each
//! request of its own.
//!
//! The file is read when it is chosen and whenever the settings are applied;
//! nothing is written back, and the cookies never leave this machine except
//! to the sites they belong to.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// One line of a `cookies.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    /// The host, without a leading dot.
    pub domain: String,
    /// Sent to the host's subdomains too.
    pub subdomains: bool,
    /// The path the cookie is sent for, and below it.
    pub path: String,
    /// Sent only over https.
    pub secure: bool,
    /// Seconds since 1970; 0 for a session cookie.
    pub expires: u64,
    /// The cookie's name.
    pub name: String,
    /// The cookie's value.
    pub value: String,
}

/// Reads the Netscape format: seven tab-separated fields a line, `#` lines
/// are comments except `#HttpOnly_`, which marks a cookie. A line that is
/// not one is skipped rather than failing the file.
pub fn parse(text: &str) -> Vec<Cookie> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let line = match line.strip_prefix("#HttpOnly_") {
                Some(rest) => rest,
                None if line.starts_with('#') => return None,
                None => line,
            };
            let mut fields = line.split('\t');
            let domain = fields.next()?.trim();
            let subdomains = fields.next()?.trim().eq_ignore_ascii_case("TRUE");
            let path = fields.next()?.trim();
            let secure = fields.next()?.trim().eq_ignore_ascii_case("TRUE");
            let expires = fields.next()?.trim().parse::<u64>().unwrap_or(0);
            let name = fields.next()?.trim();
            let value = fields.next().unwrap_or("").trim();
            if domain.is_empty() || name.is_empty() || name.contains([';', '\n']) {
                return None;
            }
            Some(Cookie {
                subdomains: subdomains || domain.starts_with('.'),
                domain: domain.trim_start_matches('.').to_ascii_lowercase(),
                path: if path.starts_with('/') {
                    path.to_string()
                } else {
                    "/".into()
                },
                secure,
                expires,
                name: name.to_string(),
                value: value.replace([';', '\n', '\r'], ""),
            })
        })
        .collect()
}

/// The `Cookie` header for `url` from `cookies`, at time `now`: the ones
/// whose site, path and scheme match and that have not expired.
pub fn header_for(cookies: &[Cookie], url: &str, now: u64) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let https = parsed.scheme() == "https";
    let path = parsed.path();
    let pairs: Vec<String> = cookies
        .iter()
        .filter(|c| {
            let site = host == c.domain
                || (c.subdomains
                    && host
                        .strip_suffix(c.domain.as_str())
                        .is_some_and(|before| before.ends_with('.')));
            let under = path == c.path
                || path
                    .strip_prefix(c.path.as_str())
                    .is_some_and(|rest| c.path.ends_with('/') || rest.starts_with('/'));
            site && under && (https || !c.secure) && (c.expires == 0 || c.expires > now)
        })
        .map(|c| format!("{}={}", c.name, c.value))
        .collect();
    (!pairs.is_empty()).then(|| pairs.join("; "))
}

/// The file in use and what it held, for every request dm makes.
#[derive(Default)]
struct InUse {
    file: Option<PathBuf>,
    cookies: Vec<Cookie>,
}

fn in_use() -> &'static RwLock<InUse> {
    static IN_USE: std::sync::OnceLock<RwLock<InUse>> = std::sync::OnceLock::new();
    IN_USE.get_or_init(Default::default)
}

/// Uses the cookies in `file` from now on, or none for an empty path.
/// Returns how many it read; a file that cannot be read is an error and the
/// cookies in use are left as they were.
pub fn use_file(file: &Path) -> Result<usize, String> {
    if file.as_os_str().is_empty() {
        let mut held = in_use().write().unwrap_or_else(|p| p.into_inner());
        *held = InUse::default();
        return Ok(0);
    }
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("the cookies file {} could not be read: {e}", file.display()))?;
    let cookies = parse(&text);
    let count = cookies.len();
    let mut held = in_use().write().unwrap_or_else(|p| p.into_inner());
    *held = InUse {
        file: Some(file.to_path_buf()),
        cookies,
    };
    Ok(count)
}

/// The cookies file in use.
pub fn file() -> Option<PathBuf> {
    in_use()
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .file
        .clone()
}

fn host_of(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some(match parsed.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// A site's sign-in, handed over by the browser with one download: the
/// `Cookie` header it would have sent, and the one server it is for.
/// Carried by that download alone and sent to that server alone — not to a
/// mirror, a redirect's other host, or a later download that never had it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignIn {
    host: String,
    header: String,
    /// Request headers the browser sent for this download (a bearer token, an
    /// `Origin`, a site's own `X-…` header), sent to the same one server.
    extra: Vec<(String, String)>,
    /// The form this download is the answer to, sent again to its one address.
    post: Option<Posted>,
}

/// A form a page sent, whose answer is the file: its address, its body and
/// the body's type. Fetched by sending it again, as the browser did; never
/// written to disk, as a form can hold what a person typed.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Posted {
    /// Where the form was sent.
    pub url: String,
    /// What it sent.
    pub body: Vec<u8>,
    /// The body's type (`application/x-www-form-urlencoded` for a form).
    pub content_type: String,
}

impl std::fmt::Debug for Posted {
    // The body is a person's: its size, not its words, in a log or a panic.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Posted")
            .field("url", &self.url)
            .field("body", &format_args!("{} bytes", self.body.len()))
            .field("content_type", &self.content_type)
            .finish()
    }
}

/// The largest form body carried: an export button's form is a few fields.
pub const POST_MOST: usize = 64 * 1024;

/// Headers dm sets itself or that belong to one connection, never taken
/// from the browser's request. `Cookie` keeps its own field.
const NOT_FORWARDED: [&str; 18] = [
    "user-agent",
    "referer",
    "if-none-match",
    "if-modified-since",
    "if-match",
    "if-unmodified-since",
    "host",
    "range",
    "if-range",
    "content-length",
    "content-type",
    "connection",
    "accept-encoding",
    "cookie",
    "transfer-encoding",
    "upgrade",
    "te",
    "expect",
];

/// The most headers, and the longest value, taken from one hand-off.
const MAX_FORWARDED: usize = 32;
const MAX_FORWARDED_VALUE: usize = 4096;

/// The headers of `headers` dm may send on, in order, valid and bounded.
pub fn forwardable(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, value)| {
            let name = name.trim();
            !name.is_empty()
                && !NOT_FORWARDED
                    .iter()
                    .any(|own| own.eq_ignore_ascii_case(name))
                && value.len() <= MAX_FORWARDED_VALUE
                && reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_ok()
                && reqwest::header::HeaderValue::from_str(value).is_ok()
        })
        .take(MAX_FORWARDED)
        .map(|(n, v)| (n.trim().to_string(), v.clone()))
        .collect()
}

/// Adds the browser's own request headers for `url`'s server to `request`.
pub fn add_headers(
    mut request: reqwest::RequestBuilder,
    sign_in: Option<&SignIn>,
    url: &str,
) -> reqwest::RequestBuilder {
    for (name, value) in sign_in.map_or(&[][..], |s| s.headers_for(url)) {
        request = request.header(name.as_str(), value.as_str());
    }
    request
}

impl SignIn {
    /// The sign-in for `url`'s server, or `None` for an empty header or an
    /// address with no host. Line breaks are dropped: a header has none.
    pub fn new(url: &str, header: &str) -> Option<SignIn> {
        let header = header.replace(['\r', '\n'], "");
        let header = header.trim();
        if header.is_empty() {
            return None;
        }
        Some(SignIn {
            host: host_of(url)?,
            header: header.to_string(),
            extra: Vec::new(),
            post: None,
        })
    }

    /// `sign_in` (or none) with the form this download is the answer to,
    /// for its own address: `None` for an address with no host or a body
    /// over [`POST_MOST`].
    pub fn posting(sign_in: Option<SignIn>, post: Posted) -> Option<SignIn> {
        if post.body.len() > POST_MOST {
            return sign_in;
        }
        let host = host_of(&post.url)?;
        let mut sign_in = sign_in.unwrap_or(SignIn {
            host,
            header: String::new(),
            extra: Vec::new(),
            post: None,
        });
        sign_in.post = Some(post);
        Some(sign_in)
    }

    /// The form to send again, where `url` is its very address.
    pub fn post_for(&self, url: &str) -> Option<&Posted> {
        self.post.as_ref().filter(|p| p.url == url)
    }

    /// The sign-in for `url`'s server with the browser's own request headers
    /// as well: `None` when there are neither cookies nor a usable header.
    /// Headers dm sets itself, and any that is not valid, are left out.
    pub fn with_headers(url: &str, header: &str, headers: &[(String, String)]) -> Option<SignIn> {
        let extra = forwardable(headers);
        if extra.is_empty() {
            return SignIn::new(url, header);
        }
        Some(SignIn {
            host: host_of(url)?,
            header: header.replace(['\r', '\n'], "").trim().to_string(),
            extra,
            post: None,
        })
    }

    /// The browser's request headers, where `url` is on the server they are for.
    pub fn headers_for(&self, url: &str) -> &[(String, String)] {
        if host_of(url).as_deref() == Some(self.host.as_str()) {
            &self.extra
        } else {
            &[]
        }
    }

    /// The header, where `url` is on the server it is for.
    pub fn for_url(&self, url: &str) -> Option<&str> {
        (host_of(url).as_deref() == Some(self.host.as_str()) && !self.header.is_empty())
            .then_some(self.header.as_str())
    }

    /// The cookies as `cookies.txt` lines for that server.
    pub fn netscape_lines(&self) -> String {
        let domain = self
            .host
            .rsplit_once(':')
            .map_or(self.host.as_str(), |(h, _)| h);
        self.header
            .split(';')
            .filter_map(|pair| pair.trim().split_once('='))
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, value)| {
                format!(
                    "{domain}\tFALSE\t/\tFALSE\t0\t{}\t{}\n",
                    name.trim(),
                    value.trim()
                )
            })
            .collect()
    }
}

/// The `Cookie` header a request to `url` carries: the download's own
/// sign-in where it is for that server, else what the cookies file holds.
pub fn header_with(sign_in: Option<&SignIn>, url: &str) -> Option<String> {
    sign_in
        .and_then(|s| s.for_url(url))
        .map(str::to_owned)
        .or_else(|| header(url))
}

/// The `Cookie` header the cookies file holds for `url`, if any.
pub fn header(url: &str) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let held = in_use().read().unwrap_or_else(|p| p.into_inner());
    header_for(&held.cookies, url, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# Netscape HTTP Cookie File\n\
        .example.com\tTRUE\t/\tTRUE\t0\tsession\tabc\n\
        #HttpOnly_files.example.com\tFALSE\t/private\tFALSE\t4102444800\tkey\tv2\n\
        old.example.com\tFALSE\t/\tFALSE\t1\tgone\tx\n\
        not a cookie line\n";

    #[test]
    fn a_sign_in_is_for_its_own_server_only() {
        let s = SignIn::new("https://Files.example/a.zip", " session=1; theme=dark\r\n").unwrap();
        assert_eq!(
            s.for_url("https://files.example/b.zip"),
            Some("session=1; theme=dark")
        );
        assert_eq!(
            s.for_url("https://cdn.files.example/b.zip"),
            None,
            "another host"
        );
        assert_eq!(
            s.for_url("http://files.example/b.zip"),
            None,
            "another port"
        );
        assert_eq!(s.for_url("https://other.example/"), None);
        assert!(SignIn::new("https://e.com/", "  ").is_none());
        assert!(SignIn::new("not a url", "a=1").is_none());
        assert_eq!(
            s.netscape_lines(),
            "files.example\tFALSE\t/\tFALSE\t0\tsession\t1\nfiles.example\tFALSE\t/\tFALSE\t0\ttheme\tdark\n"
        );
    }

    #[test]
    fn a_cookies_txt_is_read_as_browsers_and_curl_write_it() {
        let cookies = parse(FILE);
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0].domain, "example.com");
        assert!(cookies[0].subdomains && cookies[0].secure);
        assert_eq!(cookies[1].name, "key", "an HttpOnly line is a cookie");
        assert_eq!(cookies[1].path, "/private");
    }

    #[test]
    fn only_the_matching_unexpired_cookies_go_with_a_request() {
        let cookies = parse(FILE);
        let now = 1_800_000_000;
        assert_eq!(
            header_for(&cookies, "https://example.com/a.zip", now).as_deref(),
            Some("session=abc")
        );
        assert_eq!(
            header_for(&cookies, "https://files.example.com/private/a.zip", now).as_deref(),
            Some("session=abc; key=v2")
        );
        assert_eq!(
            header_for(&cookies, "http://example.com/a.zip", now),
            None,
            "a secure cookie over http"
        );
        assert_eq!(
            header_for(&cookies, "https://files.example.com/privateer/a", now).as_deref(),
            Some("session=abc"),
            "a path is a path, not a prefix of a name"
        );
        assert_eq!(
            header_for(&cookies, "https://old.example.com/", now).as_deref(),
            Some("session=abc"),
            "expired is not sent"
        );
        assert_eq!(header_for(&cookies, "https://notexample.com/", now), None);
    }
}
