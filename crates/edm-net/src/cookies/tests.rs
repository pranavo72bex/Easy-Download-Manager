// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The cookies file read and matched, and a download's own sign-in kept to its server.

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

/// A sign-in's clones (one for each request and each connection) share one cookie buffer and
/// one set of headers, so wiping it once leaves no copy behind; each clone was a fresh copy
/// that a wipe would miss (gaps row H15c).
#[test]
fn a_sign_ins_copies_share_one_cookie_buffer() {
    let headers = vec![("Authorization".to_string(), "Bearer t".to_string())];
    let first = SignIn::with_headers("https://files.example/a.zip", "session=1", &headers).unwrap();
    let copy = first.clone();
    let url = "https://files.example/b.zip";
    assert_eq!(
        first.for_url(url).unwrap().as_ptr(),
        copy.for_url(url).unwrap().as_ptr(),
        "the clone's cookie header is a copy of its own"
    );
    assert_eq!(
        first.headers_for(url).as_ptr(),
        copy.headers_for(url).as_ptr(),
        "the clone's headers are a copy of their own"
    );
}
