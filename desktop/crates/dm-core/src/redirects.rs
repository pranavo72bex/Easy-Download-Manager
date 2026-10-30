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
        let res = client.execute(request).await?;
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
/// Judged by the address as written, not by what a name resolves to.
pub(crate) fn inward(to: &Url, previous: &[Url]) -> bool {
    fn inside(url: &Url) -> bool {
        match url.host() {
            Some(url::Host::Ipv4(ip)) => {
                ip.is_loopback() || ip.is_link_local() || ip.is_private() || ip.is_unspecified()
            }
            Some(url::Host::Ipv6(ip)) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip
                        .to_ipv4_mapped()
                        .is_some_and(|v4| v4.is_loopback() || v4.is_link_local() || v4.is_private())
            }
            Some(url::Host::Domain(name)) => {
                let name = name.to_ascii_lowercase();
                name == "localhost" || name.ends_with(".localhost")
            }
            None => false,
        }
    }
    !previous.is_empty() && inside(to) && !previous.iter().any(inside)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A public address may not send dm inward; an inside one may go on
    /// to another inside address, and public to public is anyone's.
    #[test]
    fn a_redirect_inward_from_a_public_address_is_refused_and_no_other_is() {
        let url = |s: &str| Url::parse(s).unwrap();
        let public = [url("https://cdn.example/file")];
        let home = [url("http://192.168.1.4/file")];
        for to in [
            "http://127.0.0.1:9/",
            "http://localhost/x",
            "http://a.localhost/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.5/f",
            "http://172.16.3.3/f",
            "http://192.168.1.5/f",
            "http://[::1]/f",
            "http://[fe80::1]/f",
            "http://[fd00::5]/f",
            "http://[::ffff:127.0.0.1]/f",
            "http://0.0.0.0/f",
        ] {
            assert!(inward(&url(to), &public), "from public to {to}");
            assert!(!inward(&url(to), &home), "from inside to {to}");
        }
        for to in [
            "https://other.example/f",
            "http://93.184.216.34/f",
            "http://[2606:4700::1]/f",
        ] {
            assert!(!inward(&url(to), &public), "public to public: {to}");
            assert!(!inward(&url(to), &home), "inside to public: {to}");
        }
        assert!(
            !inward(&url("http://127.0.0.1:9/"), &[]),
            "no hop before: not a redirect inward"
        );
    }

    /// Across hosts a page goes as its origin alone; from a secure page to
    /// a plain host it does not go at all; a page that is no address is
    /// dropped rather than sent.
    #[test]
    fn a_referer_crosses_hosts_as_its_origin_alone() {
        let page = HeaderValue::from_static("https://site.example:8443/private/page?token=1#x");
        let plain_to = Url::parse("http://files.example/f").unwrap();
        let secure_to = Url::parse("https://files.example/f").unwrap();
        assert_eq!(
            referer_across(&page, &secure_to)
                .as_ref()
                .and_then(|v| v.to_str().ok()),
            Some("https://site.example:8443/")
        );
        assert!(referer_across(&page, &plain_to).is_none());
        let plain_page = HeaderValue::from_static("http://site.example/page");
        assert_eq!(
            referer_across(&plain_page, &plain_to)
                .as_ref()
                .and_then(|v| v.to_str().ok()),
            Some("http://site.example/")
        );
        assert!(referer_across(&HeaderValue::from_static("not an address"), &secure_to).is_none());
    }

    /// Where the probe landed on the host asked, the ranges carry the page;
    /// on another host, its origin; on a plain host from a secure page,
    /// nothing.
    #[test]
    fn the_ranges_carry_what_the_hop_to_the_landing_host_carried() {
        let page = Some("https://pages.example/private/page?t=1");
        assert_eq!(
            referrer_for(page, "https://a.example/f", "https://a.example/files/f").as_deref(),
            page
        );
        assert_eq!(
            referrer_for(page, "https://a.example/f", "https://b.example/f").as_deref(),
            Some("https://pages.example/")
        );
        assert_eq!(
            referrer_for(page, "https://a.example/f", "http://b.example/f"),
            None
        );
        assert_eq!(
            referrer_for(None, "https://a.example/f", "https://b.example/f"),
            None
        );
    }

    /// Another port is another server: a sign-in for one must not reach
    /// the other.
    #[test]
    fn another_port_is_another_host() {
        let a = Url::parse("http://h.example/a").unwrap();
        let b = Url::parse("http://h.example:8080/b").unwrap();
        let c = Url::parse("http://h.example:80/c").unwrap();
        assert!(other_host(&a, &b));
        assert!(!other_host(&a, &c));
        assert!(other_host(&a, &Url::parse("http://H2.example/a").unwrap()));
    }
}
