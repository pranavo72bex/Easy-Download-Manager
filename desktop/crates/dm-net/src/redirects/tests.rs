// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// What a public playlist, manifest, mirror list or page names inside is judged inside; what
/// an inside one names, or a public address, is not.
#[test]
fn an_address_a_public_document_names_inside_is_judged_inside() {
    let public = "https://cdn.example/live/index.m3u8";
    for inside in [
        "http://192.168.1.1/cgi?reboot",
        "http://169.254.169.254/latest/meta-data/",
        "http://127.0.0.1:8080/x",
        "http://router.local/apply",
        "http://[::1]/x",
    ] {
        assert!(named_inside(public, inside), "{inside}");
    }
    assert!(!named_inside(public, "https://cdn.example/seg1.ts"));
    assert!(!named_inside(
        "http://192.168.1.5/index.m3u8",
        "http://192.168.1.5/seg1.ts"
    ));
    assert!(!named_inside("not an address", "http://127.0.0.1/"));
}

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
        "http://printer.local/f",
        "http://nas.lan/f",
        "http://router.home.arpa/f",
        "http://100.64.0.1/f",
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
