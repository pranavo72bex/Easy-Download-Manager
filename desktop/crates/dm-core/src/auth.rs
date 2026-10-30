//! Signing in to a server that asks for more than a Basic login.
//!
//! A user and password in a download's address go out as `Basic` on every
//! request: reqwest moves them into the header when the request is built.
//! A server that wants `Digest` instead answers 401 with a challenge, and
//! until this module every such download failed at the probe with "the
//! server answered 401" — the password was right and never used.
//!
//! RFC 7616: MD5 and SHA-256, their `-sess` forms, `qop=auth`, `opaque`
//! echoed, a stale nonce taken afresh. `auth-int` is not offered (it hashes
//! the body, which a GET has none of, and servers that ask for it alone are
//! not seen). The challenge a server gives is kept per server, so after the
//! probe's one 401 every range request carries its Digest from the start
//! rather than paying a round trip each.
//!
//! A server that asks for `NTLM`, or for `Negotiate` (which takes NTLM
//! under it), is signed in to with NTLMv2 (`ntlm.rs`). That sign-in holds
//! for a connection rather than a request, so every request goes through
//! its three messages; a server known to want it gets the first of them at
//! once rather than after a refusal.

// Reads outside input: an index out of range here is a crash a server could
// cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::collections::HashMap;
use std::sync::Mutex;

use md5::Md5;
use sha2::{Digest as _, Sha256};

use crate::Error;

/// The hash a challenge names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Md5,
    Md5Sess,
    Sha256,
    Sha256Sess,
}

impl Algorithm {
    fn named(name: Option<&str>) -> Option<Algorithm> {
        Some(match name.map(str::trim) {
            None => Algorithm::Md5,
            Some(n) if n.eq_ignore_ascii_case("MD5") => Algorithm::Md5,
            Some(n) if n.eq_ignore_ascii_case("MD5-sess") => Algorithm::Md5Sess,
            Some(n) if n.eq_ignore_ascii_case("SHA-256") => Algorithm::Sha256,
            Some(n) if n.eq_ignore_ascii_case("SHA-256-sess") => Algorithm::Sha256Sess,
            // SHA-512-256 and anything newer: not offered, so the 401 stands
            // and says what the server asked for.
            Some(_) => return None,
        })
    }

    /// As the server spelled it, to be echoed back.
    fn name(self) -> &'static str {
        match self {
            Algorithm::Md5 => "MD5",
            Algorithm::Md5Sess => "MD5-sess",
            Algorithm::Sha256 => "SHA-256",
            Algorithm::Sha256Sess => "SHA-256-sess",
        }
    }

    fn session(self) -> bool {
        matches!(self, Algorithm::Md5Sess | Algorithm::Sha256Sess)
    }

    fn hash(self, text: &str) -> String {
        match self {
            Algorithm::Md5 | Algorithm::Md5Sess => hex::encode(Md5::digest(text.as_bytes())),
            Algorithm::Sha256 | Algorithm::Sha256Sess => {
                hex::encode(Sha256::digest(text.as_bytes()))
            }
        }
    }
}

/// What a server's `WWW-Authenticate: Digest` asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// Whether `qop=auth` was offered; the older form without it is still
    /// answered, as RFC 2069 servers want.
    pub qop: bool,
    pub algorithm: Algorithm,
    /// The server said the nonce it was given has expired, and the login
    /// itself was fine.
    pub stale: bool,
}

/// Reads one `WWW-Authenticate` value, if it is a Digest dm can answer.
pub fn challenge(header: &str) -> Option<Challenge> {
    let rest = header.trim();
    let rest = rest
        .get(..6)
        .filter(|s| s.eq_ignore_ascii_case("digest"))
        .and_then(|_| rest.get(6..))?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let params = params(rest);
    let get = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    let qop = get("qop").is_none_or(|offered| {
        offered
            .split(',')
            .any(|q| q.trim().eq_ignore_ascii_case("auth"))
    });
    // A server offering only `auth-int` cannot be answered with a GET.
    if get("qop").is_some() && !qop {
        return None;
    }
    Some(Challenge {
        realm: get("realm").unwrap_or_default(),
        nonce: get("nonce")?,
        opaque: get("opaque"),
        qop: get("qop").is_some(),
        algorithm: Algorithm::named(get("algorithm").as_deref())?,
        stale: get("stale").is_some_and(|s| s.eq_ignore_ascii_case("true")),
    })
}

/// `name=value, name="quoted, value"` pairs, quotes and escapes undone.
fn params(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        // Skip separators.
        while matches!(chars.peek(), Some(c) if c.is_whitespace() || *c == ',') {
            chars.next();
        }
        let mut name = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c == ',' || c.is_whitespace() {
                break;
            }
            name.push(c);
            chars.next();
        }
        if name.is_empty() {
            break;
        }
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        if chars.peek() != Some(&'=') {
            // A token without a value: kept, so the loop moves on.
            out.push((name, String::new()));
            continue;
        }
        chars.next();
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => {
                        if let Some(escaped) = chars.next() {
                            value.push(escaped);
                        }
                    }
                    c => value.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                value.push(c);
                chars.next();
            }
            value = value.trim().to_string();
        }
        out.push((name, value));
    }
    out
}

/// A quoted-string value: quotes and backslashes in it escaped.
fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// A client nonce: unguessable enough, and different every time. From the
/// clock and a counter rather than a random source, which dm does not
/// otherwise carry; it is mixed with the server's nonce, so it need only be
/// fresh, not secret.
fn cnonce(server_nonce: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!("{nanos}:{count}:{server_nonce}:{}", std::process::id());
    hex::encode(Sha256::digest(seed.as_bytes()))
}

/// The `Authorization` value answering `challenge` for `method` on `url`,
/// as the `nc`th request under this nonce.
pub fn authorization(
    challenge: &Challenge,
    nc: u32,
    method: &str,
    url: &url::Url,
    username: &str,
    password: &str,
) -> String {
    let algorithm = challenge.algorithm;
    let uri = match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    };
    let cnonce = cnonce(&challenge.nonce);
    let nc_text = format!("{nc:08x}");

    let mut a1 = algorithm.hash(&format!("{username}:{}:{password}", challenge.realm));
    if algorithm.session() {
        a1 = algorithm.hash(&format!("{a1}:{}:{cnonce}", challenge.nonce));
    }
    let a2 = algorithm.hash(&format!("{method}:{uri}"));
    let response = if challenge.qop {
        algorithm.hash(&format!(
            "{a1}:{}:{nc_text}:{cnonce}:auth:{a2}",
            challenge.nonce
        ))
    } else {
        algorithm.hash(&format!("{a1}:{}:{a2}", challenge.nonce))
    };

    let mut out = format!(
        "Digest username={}, realm={}, nonce={}, uri={}, response={}, algorithm={}",
        quoted(username),
        quoted(&challenge.realm),
        quoted(&challenge.nonce),
        quoted(&uri),
        quoted(&response),
        algorithm.name(),
    );
    if challenge.qop {
        out.push_str(&format!(
            ", qop=auth, nc={nc_text}, cnonce={}",
            quoted(&cnonce)
        ));
    }
    if let Some(opaque) = &challenge.opaque {
        out.push_str(&format!(", opaque={}", quoted(opaque)));
    }
    out
}

/// The challenge each server last gave, and how many requests have answered
/// under its nonce. Keyed by host and port: two servers on one host are two
/// servers.
struct Known {
    challenge: Challenge,
    nc: u32,
}

fn known() -> &'static Mutex<HashMap<String, Known>> {
    static KNOWN: std::sync::OnceLock<Mutex<HashMap<String, Known>>> = std::sync::OnceLock::new();
    KNOWN.get_or_init(Default::default)
}

fn server_of(url: &url::Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// The user and password `url` carries, decoded, if it carries any.
fn login_in(url: &str) -> Option<(String, String)> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.username().is_empty() {
        return None;
    }
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    };
    Some((
        decode(parsed.username()),
        decode(parsed.password().unwrap_or_default()),
    ))
}

/// The Digest header to send `request` with from the start, where its
/// server is known to want one.
fn remembered(request: &reqwest::Request, username: &str, password: &str) -> Option<String> {
    let server = server_of(request.url())?;
    let mut known = crate::lock(known());
    let entry = known.get_mut(&server)?;
    entry.nc = entry.nc.saturating_add(1);
    Some(authorization(
        &entry.challenge,
        entry.nc,
        request.method().as_str(),
        request.url(),
        username,
        password,
    ))
}

/// Sends `request`, signing in where the server asks for a Digest and
/// `url` — the address as the person gave it — carries a user and password.
///
/// Everything else is as `send` would have had it: a server that takes the
/// Basic login, asks for nothing, or asks for something dm does not speak
/// gets the same answer as before, and only one extra request is ever made.
pub async fn send(
    client: &reqwest::Client,
    request: reqwest::RequestBuilder,
    url: &str,
) -> Result<reqwest::Response, Error> {
    let Some((username, password)) = login_in(url) else {
        return crate::redirects::send(request).await;
    };
    let mut request = request.build()?;
    let ntlm_known = server_of(request.url())
        .and_then(|server| crate::lock(ntlm_servers()).get(&server).copied());
    if let Some(scheme) = ntlm_known {
        return ntlm_sign_in(client, request, scheme, &username, &password).await;
    }
    if let Some(header) = remembered(&request, &username, &password) {
        set_authorization(&mut request, &header);
    }
    /* One answer to a challenge, and up to two more where the server says
     * only that its nonce has expired (`stale=true`) — a login that was
     * right is not refused for a nonce that ran out between two requests.
     * Anything else that comes back 401 is the server's answer. */
    let mut rounds = 0;
    loop {
        let Some(mut again) = request.try_clone() else {
            return crate::redirects::execute(client, request).await;
        };
        let res = crate::redirects::execute(client, request).await?;
        if res.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(res);
        }
        let Some(challenge) = res
            .headers()
            .get_all(reqwest::header::WWW_AUTHENTICATE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(challenge)
        else {
            // No Digest: NTLM, where the server offers it (as NTLM, or
            // as Negotiate, which takes it), on its own server only.
            let offered = res
                .headers()
                .get_all(reqwest::header::WWW_AUTHENTICATE)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .find_map(crate::ntlm::offered);
            let landed = res.url().clone();
            let (Some(scheme), true, 0) = (
                offered,
                server_of(&landed) == server_of(again.url()),
                rounds,
            ) else {
                return Ok(res);
            };
            drain(res).await;
            *again.url_mut() = landed;
            if let Some(server) = server_of(again.url()) {
                crate::lock(ntlm_servers()).insert(server, scheme);
            }
            return ntlm_sign_in(client, again, scheme, &username, &password).await;
        };
        rounds += 1;
        if rounds > 3 || (rounds > 1 && !challenge.stale) {
            return Ok(res);
        }
        // The challenge came from wherever the redirects ended, and the
        // answer names that path. On the same server it is asked there
        // directly; a login is never carried to another server.
        let landed = res.url().clone();
        if landed != *again.url() {
            if server_of(&landed) != server_of(again.url()) {
                return Ok(res);
            }
            *again.url_mut() = landed;
        }
        let header = authorization(
            &challenge,
            1,
            again.method().as_str(),
            again.url(),
            &username,
            &password,
        );
        if let Some(server) = server_of(again.url()) {
            crate::lock(known()).insert(server, Known { challenge, nc: 1 });
        }
        set_authorization(&mut again, &header);
        request = again;
    }
}

/// The servers that asked for NTLM, and under which name.
fn ntlm_servers() -> &'static Mutex<HashMap<String, &'static str>> {
    static SERVERS: std::sync::OnceLock<Mutex<HashMap<String, &'static str>>> =
        std::sync::OnceLock::new();
    SERVERS.get_or_init(Default::default)
}

/// A refusal's body read to its end (as much of it as a refusal should
/// have), so its connection goes back to the pool for the next message of
/// an NTLM sign-in, which must come on the same connection.
async fn drain(mut res: reqwest::Response) {
    let mut read = 0usize;
    while let Ok(Some(chunk)) = res.chunk().await {
        read += chunk.len();
        if read > 1 << 20 {
            break;
        }
    }
}

/// Signs `request` in with NTLM under `scheme`: the negotiate message, the
/// server's challenge, the answer. Once more where the answer is refused,
/// which is an answer that went out on another connection than its
/// challenge came on (two ranges of one download signing in at once).
async fn ntlm_sign_in(
    client: &reqwest::Client,
    request: reqwest::Request,
    scheme: &'static str,
    username: &str,
    password: &str,
) -> Result<reqwest::Response, Error> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut tries = 0;
    loop {
        tries += 1;
        let Some(mut first) = request.try_clone() else {
            return crate::redirects::execute(client, request).await;
        };
        set_authorization(
            &mut first,
            &format!("{scheme} {}", b64.encode(crate::ntlm::negotiate())),
        );
        let res = crate::redirects::execute(client, first).await?;
        if res.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(res);
        }
        let Some(challenge) = res
            .headers()
            .get_all(reqwest::header::WWW_AUTHENTICATE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|v| crate::ntlm::challenge_in(v, scheme))
        else {
            return Ok(res);
        };
        drain(res).await;
        let Some(answer) = crate::ntlm::authenticate(&challenge, username, password) else {
            return Err(Error::Internal(
                "the NTLM sign-in could not be made: no random numbers to make it with".into(),
            ));
        };
        let Some(mut last) = request.try_clone() else {
            return crate::redirects::execute(client, request).await;
        };
        set_authorization(&mut last, &format!("{scheme} {}", b64.encode(answer)));
        let res = crate::redirects::execute(client, last).await?;
        if res.status() != reqwest::StatusCode::UNAUTHORIZED || tries >= 2 {
            return Ok(res);
        }
        drain(res).await;
    }
}

/// Replaces the Basic header reqwest put on from the address.
fn set_authorization(request: &mut reqwest::Request, header: &str) {
    if let Ok(value) = reqwest::header::HeaderValue::from_str(header) {
        request
            .headers_mut()
            .insert(reqwest::header::AUTHORIZATION, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_challenge_is_read_with_its_quotes_and_commas() {
        let c = challenge(
            r#"Digest realm="files, inc", qop="auth,auth-int", nonce="abc\"def", opaque="xyz", algorithm=SHA-256, stale=TRUE"#,
        )
        .unwrap();
        assert_eq!(c.realm, "files, inc");
        assert_eq!(c.nonce, "abc\"def");
        assert_eq!(c.opaque.as_deref(), Some("xyz"));
        assert!(c.qop);
        assert_eq!(c.algorithm, Algorithm::Sha256);
        assert!(c.stale);
    }

    #[test]
    fn what_dm_does_not_speak_is_not_a_challenge() {
        assert!(challenge("Basic realm=\"x\"").is_none());
        assert!(challenge("Digest realm=\"x\"").is_none(), "no nonce");
        assert!(
            challenge("Digest realm=\"x\", nonce=\"n\", algorithm=SHA-512-256").is_none(),
            "an algorithm dm has not got"
        );
        assert!(
            challenge("Digest realm=\"x\", nonce=\"n\", qop=\"auth-int\"").is_none(),
            "a body hash on a GET"
        );
        assert!(challenge("Digestive nonce=\"n\"").is_none());
    }

    #[test]
    fn the_rfc_7616_example_is_answered_as_the_rfc_shows() {
        // RFC 7616 §3.9.1, MD5, with its cnonce and nc. The cnonce is
        // dm's own at run time, so the response is checked by recomputing
        // the RFC's with the RFC's cnonce through the same arithmetic.
        let c = challenge(
            r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#,
        )
        .unwrap();
        let a1 = c
            .algorithm
            .hash("Mufasa:http-auth@example.org:Circle of Life");
        let a2 = c.algorithm.hash("GET:/dir/index.html");
        let response = c.algorithm.hash(&format!(
            "{a1}:{}:00000001:f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ:auth:{a2}",
            c.nonce
        ));
        assert_eq!(response, "8ca523f5e9506fed4657c9700eebdbec");
    }

    #[test]
    fn the_header_names_everything_the_server_needs() {
        let c = Challenge {
            realm: "r".into(),
            nonce: "n".into(),
            opaque: Some("o".into()),
            qop: true,
            algorithm: Algorithm::Sha256,
            stale: false,
        };
        let url = url::Url::parse("http://h/a/b.zip?x=1").unwrap();
        let h = authorization(&c, 3, "GET", &url, "me\"you", "p");
        assert!(h.starts_with("Digest "), "{h}");
        assert!(h.contains(r#"username="me\"you""#), "{h}");
        assert!(h.contains(r#"uri="/a/b.zip?x=1""#), "{h}");
        assert!(h.contains("nc=00000003"), "{h}");
        assert!(h.contains("qop=auth"), "{h}");
        assert!(h.contains(r#"opaque="o""#), "{h}");
        assert!(h.contains("algorithm=SHA-256"), "{h}");
        // Without qop, the older form: no nc or cnonce.
        let old = authorization(
            &Challenge {
                qop: false,
                ..c.clone()
            },
            1,
            "GET",
            &url,
            "me",
            "p",
        );
        assert!(!old.contains("nc="), "{old}");
        assert!(!old.contains("cnonce"), "{old}");
    }

    #[test]
    fn a_login_is_read_out_of_the_address_decoded() {
        use crate::test_secrets::{escaped, marked};
        let password = marked("password");
        assert_eq!(
            login_in(&format!("https://me:{}@h/f", escaped(&password))),
            Some(("me".into(), password.clone()))
        );
        assert_eq!(login_in("https://h/f"), None);
        assert_eq!(login_in("not a url"), None);
    }

    #[test]
    fn client_nonces_differ() {
        assert_ne!(cnonce("n"), cnonce("n"));
    }
}
