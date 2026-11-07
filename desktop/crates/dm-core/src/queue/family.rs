// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

/// Whether two addresses are of the same family of servers: the same host, or
/// hosts that share their registered name (`cdn1.example.com` and
/// `dl.example.com`). A signed address often moves between a site's own
/// servers when it is issued again; it does not move to another site.
pub(super) fn same_family(a: &str, b: &str) -> bool {
    matches!((family(a), family(b)), (Some(x), Some(y)) if x == y)
}

/// Whether two addresses are on one family of servers and have the very same path (their queries
/// may differ: a signature is in the query).
pub(super) fn same_path(a: &str, b: &str) -> bool {
    let path = |u: &str| {
        let url = reqwest::Url::parse(u).ok()?;
        let path = url.path().to_string();
        (path.len() > 1).then_some(path)
    };
    same_family(a, b) && matches!((path(a), path(b)), (Some(x), Some(y)) if x == y)
}

/// Names under which each customer of a host has a name of their own (a bucket, a site, an
/// app), so that two names under one of them are two customers, not one site's servers: from
/// the private part of the public suffix list, the hosts files are served from. Under these a
/// host is its own family (security review, 2026-11-07: `victim.s3.amazonaws.com` and
/// `evil.s3.amazonaws.com` were one family, and a waiting row could take a file from the
/// second).
const SHARED: [&str; 30] = [
    "amazonaws.com",
    "cloudfront.net",
    "blob.core.windows.net",
    "azureedge.net",
    "azurewebsites.net",
    "azurefd.net",
    "github.io",
    "githubusercontent.com",
    "gitlab.io",
    "herokuapp.com",
    "appspot.com",
    "firebaseapp.com",
    "web.app",
    "netlify.app",
    "vercel.app",
    "pages.dev",
    "workers.dev",
    "r2.dev",
    "r2.cloudflarestorage.com",
    "b-cdn.net",
    "digitaloceanspaces.com",
    "backblazeb2.com",
    "wasabisys.com",
    "onrender.com",
    "fly.dev",
    "ngrok.io",
    "ngrok-free.app",
    "glitch.me",
    "blogspot.com",
    "sharepoint.com",
];

fn family(url: &str) -> Option<String> {
    let host = reqwest::Url::parse(url)
        .ok()?
        .host_str()?
        .to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    // An address with no names in it (an IP) is its own family.
    if labels.len() < 3 || host.parse::<std::net::IpAddr>().is_ok() {
        return Some(host);
    }
    if SHARED.iter().any(|shared| {
        host.strip_suffix(shared)
            .is_some_and(|rest| rest.ends_with('.'))
    }) {
        return Some(host);
    }
    // `example.co.uk`: a short second-level label under a two-letter country
    // code belongs to the registry, not to the site.
    let n = labels.len();
    let registry = labels[n - 1].len() == 2
        && matches!(
            labels[n - 2],
            "co" | "com" | "org" | "net" | "gov" | "edu" | "ac"
        );
    let keep = if registry { 3 } else { 2 };
    Some(labels[n - keep..].join("."))
}

#[cfg(test)]
mod tests {
    use super::same_family;

    #[test]
    fn two_customers_of_a_shared_host_are_not_one_family() {
        for (a, b) in [
            (
                "https://victim.s3.amazonaws.com/f.zip",
                "https://evil.s3.amazonaws.com/f.zip",
            ),
            (
                "https://victim.github.io/f.zip",
                "https://evil.github.io/f.zip",
            ),
            (
                "https://d1.cloudfront.net/f.zip",
                "https://d2.cloudfront.net/f.zip",
            ),
            (
                "https://a.blob.core.windows.net/f",
                "https://b.blob.core.windows.net/f",
            ),
        ] {
            assert!(!same_family(a, b), "{a} {b}");
        }
        // The very same customer's host is still one family.
        assert!(same_family(
            "https://victim.s3.amazonaws.com/f.zip?sig=1",
            "https://victim.s3.amazonaws.com/f.zip?sig=2"
        ));
    }

    #[test]
    fn servers_of_one_site_are_a_family_and_other_sites_are_not() {
        assert!(same_family(
            "https://cdn1.example.com/a?x=1",
            "https://dl.example.com/b"
        ));
        assert!(same_family("http://127.0.0.1:80/a", "http://127.0.0.1:9/b"));
        assert!(same_family(
            "https://a.shop.example.co.uk/x",
            "https://b.shop.example.co.uk/y"
        ));
        assert!(!same_family(
            "https://example.com/a",
            "https://example.org/a"
        ));
        assert!(!same_family(
            "https://one.example.co.uk/a",
            "https://two.other.co.uk/a"
        ));
        assert!(!same_family("http://10.0.0.1/a", "http://10.0.0.2/a"));
        assert!(!same_family("not a url", "https://example.com/"));
    }
}
