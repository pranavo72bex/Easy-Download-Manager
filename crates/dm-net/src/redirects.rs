// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Redirects, followed by dm itself rather than by reqwest.
//!
//! reqwest follows a redirect with the headers it was given, drops the
//! sign-in and cookies when the host changes, and writes the address it
//! came from into `Referer` on every hop. That last part sent the page a
//! download was clicked on, or the address the file was fetched from, to
//! whatever other host a redirect led to: a signed link's query, a private
//! page's path. A browser sends only the origin across hosts. reqwest's
//! policy cannot change a header, so dm's clients follow nothing
//! (`builder_with`) and every request goes through [`send`] or
//! [`execute`], which follow with dm's rules: ten hops at most, never from
//! `https` to `http`, never from a public address inward
//! ([`inward`]), the sign-in and cookies dropped across hosts as reqwest
//! dropped them, and `Referer` cut to its origin across hosts (and dropped
//! from a secure page to a plain host, as browsers drop it).

use reqwest::header::{
    HeaderValue, AUTHORIZATION, COOKIE, LOCATION, PROXY_AUTHORIZATION, REFERER, WWW_AUTHENTICATE,
};
use reqwest::{Method, StatusCode, Url};

use crate::Error;

/// How many times an address may be sent on before dm gives up on it: a
/// loop is a server problem that should not become a hang.
pub const MOST_HOPS: usize = 10;

/// The headers a hop to another host keeps: those any request of dm's
/// carries, whatever it is for (`Referer` is cut to its origin below). Every
/// other header was set for the first host, by the browser that handed the
/// download over or by a site's own player, and stays there.
const CARRIED_ACROSS: [&str; 10] = [
    "user-agent",
    "accept",
    "accept-encoding",
    "accept-language",
    "range",
    "if-range",
    "referer",
    "origin",
    "content-type",
    "pragma",
];

/// Sends the request and follows what it is sent on to, by dm's rules.
///
/// The drop-in for `RequestBuilder::send` on a client of [`crate::builder`].
pub async fn send(builder: reqwest::RequestBuilder) -> Result<reqwest::Response, Error> {
    let (client, request) = builder.build_split();
    execute(&client, request?).await
}

/// Executes the request and follows what it is sent on to, by dm's rules.
///
/// The drop-in for `Client::execute` on a client of [`crate::builder`].
pub async fn execute(
    client: &reqwest::Client,
    mut request: reqwest::Request,
) -> Result<reqwest::Response, Error> {
    let mut previous: Vec<Url> = Vec::new();
    loop {
        let again = request.try_clone();
        let res = match client.execute(request).await {
            Ok(res) => res,
            // A name a public address led to that the resolver found inside alone: refused as
            // an inward redirect is, and not asked again (gaps row 200).
            Err(e) if crate::dns::refused_inside(&e) => {
                let to = e.url().map(Url::as_str).unwrap_or_default().to_string();
                return Err(Error::Internal(format!(
                    "{} ({to})",
                    crate::INWARD_REDIRECT
                )));
            }
            Err(e) => return Err(e.into()),
        };
        let status = res.status();
        if !sends_on(status) {
            return Ok(res);
        }
        let to = res
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|location| res.url().join(location).ok());
        // Nowhere sayable, a body dm cannot send twice: the answer is the
        // redirect itself, which `Error::refused_or` says in words.
        let (Some(to), Some(mut next)) = (to, again) else {
            return Ok(res);
        };
        if to.scheme() != "http" && to.scheme() != "https" {
            return Err(Error::Internal(format!(
                "the server sent dm on to {}, an address dm does not follow",
                to.as_str().chars().take(200).collect::<String>()
            )));
        }
        let from = res.url().clone();
        drop(res);
        previous.push(from.clone());
        if previous.len() > MOST_HOPS {
            return Err(Error::Internal(format!(
                "too many redirects: the address was sent on more than {MOST_HOPS} times, last to {to}"
            )));
        }
        if to.scheme() == "http" && previous.iter().any(|before| before.scheme() == "https") {
            return Err(Error::Internal(format!(
                "{} ({to})",
                crate::INSECURE_REDIRECT
            )));
        }
        if inward(&to, &previous) {
            return Err(Error::Internal(format!(
                "{} ({to})",
                crate::INWARD_REDIRECT
            )));
        }
        // A 303, and a 301 or 302 answered to a POST, are answered by a GET
        // of what they name, as browsers and reqwest answer them; 307 and
        // 308 repeat the request as it was.
        let to_get = status == StatusCode::SEE_OTHER
            || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND)
                && *next.method() == Method::POST);
        if to_get && *next.method() != Method::HEAD {
            *next.method_mut() = Method::GET;
            *next.body_mut() = None;
        }
        if other_host(&from, &to) {
            let headers = next.headers_mut();
            for name in [AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION, WWW_AUTHENTICATE] {
                headers.remove(name);
            }
            headers.remove("cookie2");
            // The browser's own request headers (`SignIn::extra`: a site's
            // `X-Api-Key`, `X-Auth-Token` …) are the first host's, as its
            // cookies are: only what any request of dm's carries goes on.
            let dropped: Vec<_> = headers
                .keys()
                .filter(|name| !CARRIED_ACROSS.contains(&name.as_str()))
                .cloned()
                .collect();
            for name in dropped {
                headers.remove(name);
            }
            let origin = headers
                .get(REFERER)
                .and_then(|page| referer_across(page, &to));
            match origin {
                Some(origin) => {
                    headers.insert(REFERER, origin);
                }
                None => {
                    headers.remove(REFERER);
                }
            }
        }
        *next.url_mut() = to;
        request = next;
    }
}

/// The `Referer` for the requests that go straight to where a probe
/// landed: the page as given where the landing host is the one asked, its
/// origin alone where a redirect led to another host (what the hop itself
/// carried), and none from a secure page to a plain host.
pub fn referrer_for(page: Option<&str>, asked: &str, landed: &str) -> Option<String> {
    let page = page?;
    let (Ok(asked), Ok(landed)) = (Url::parse(asked), Url::parse(landed)) else {
        return Some(page.to_string());
    };
    if !other_host(&asked, &landed) {
        return Some(page.to_string());
    }
    referer_across(&HeaderValue::from_str(page).ok()?, &landed)
        .and_then(|origin| origin.to_str().ok().map(str::to_string))
}

/// Whether a status sends the request on somewhere else.
fn sends_on(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

/// Whether a hop lands on another server: the host or the port differs.
fn other_host(from: &Url, to: &Url) -> bool {
    from.host_str() != to.host_str() || from.port_or_known_default() != to.port_or_known_default()
}

/// What of a `Referer` goes to another host: its origin alone, with no path
/// and no query, and nothing at all from a secure page to a plain host.
/// That is what a browser sends across sites unless the page says
/// otherwise.
fn referer_across(page: &HeaderValue, to: &Url) -> Option<HeaderValue> {
    let page = Url::parse(page.to_str().ok()?).ok()?;
    if page.scheme() == "https" && to.scheme() != "https" {
        return None;
    }
    let url::Origin::Tuple(..) = page.origin() else {
        return None;
    };
    HeaderValue::from_str(&format!("{}/", page.origin().ascii_serialization())).ok()
}

/// Whether a redirect sends dm from a public address to one inside: this
/// computer (loopback, `localhost`), its link (link-local, where cloud
/// machines keep their metadata service) or its own network (the private
/// ranges). A page out on the internet that sends dm to such an address is
/// using dm to reach what it cannot reach itself; a download that started
/// inside (a server at home, a test's) may go on to another inside address.
/// Judged by the address as written; a public-looking name it judges is then
/// held by the resolver to public addresses (`dns::must_be_public`), so the
/// address connected to is judged too, after the lookup (gaps row 200).
pub(crate) fn inward(to: &Url, previous: &[Url]) -> bool {
    // A hop counts as inside by its address as written, or by where its name was found to lead
    // when dm connected to it: an intranet's own names (`files.company.example` at 10.x) lead
    // from one to another as a home server's addresses do.
    let reached_inside = |url: &Url| {
        inside(url)
            || matches!(url.host(), Some(url::Host::Domain(name)) if crate::dns::leads_inside(name))
    };
    let from_public = !previous.is_empty() && !previous.iter().any(reached_inside);
    // A name a public address leads to, which looks public: the resolver holds it to public
    // addresses, so it cannot be pointed inside once judged here (DNS rebinding, gaps row 200).
    if from_public && !inside(to) {
        if let Some(url::Host::Domain(name)) = to.host() {
            // Where it cannot be held, it is not let through unheld.
            if !crate::dns::must_be_public(name) {
                return true;
            }
        }
    }
    from_public && inside(to)
}

/// Whether an address is inside: this computer, a private or link-local network, a provider's
/// shared address space. The same hosts as the extension's `decide::streams::inside` (dm-core may
/// not depend on dm-protocol, where one copy for both would go).
pub(crate) fn inside_ip(ip: std::net::IpAddr) -> bool {
    fn inside_v4(ip: std::net::Ipv4Addr) -> bool {
        let [a, b, ..] = ip.octets();
        ip.is_loopback()
            || ip.is_link_local()
            || ip.is_private()
            || ip.is_unspecified()
            || (a == 100 && (64..128).contains(&b))
    }
    match ip {
        std::net::IpAddr::V4(ip) => inside_v4(ip),
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.to_ipv4_mapped().is_some_and(inside_v4)
        }
    }
}

/// Whether an address's host is inside, as written: an inside address ([`inside_ip`]), or a name
/// only a local network answers.
fn inside(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => inside_ip(ip.into()),
        Some(url::Host::Ipv6(ip)) => inside_ip(ip.into()),
        Some(url::Host::Domain(name)) => {
            let name = name.to_ascii_lowercase();
            name == "localhost"
                || [".localhost", ".local", ".internal", ".lan", ".home.arpa"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        }
        None => false,
    }
}

/// Whether `to`, an address that what was fetched from `from` names (a playlist's segment, key
/// or map, a DASH piece, a Metalink mirror, a page's link), would take dm from a public address
/// to one inside, as [`inward`] judges a redirect. Those addresses are requested as first hops,
/// which `inward` never judges, so a public playlist naming `http://192.168.1.1/…` would send dm
/// there with the page's Referer and any cookies held for that host (security audit).
/// Either not an address: not judged inside.
pub fn named_inside(from: &str, to: &str) -> bool {
    match (Url::parse(from.trim()), Url::parse(to.trim())) {
        (Ok(from), Ok(to)) => inward(&to, &[from]),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
