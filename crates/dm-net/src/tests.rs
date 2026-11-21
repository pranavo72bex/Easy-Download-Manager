// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// An AppImage is Linux's: its path lists are split on ':', which Windows
// does not split on.
#[cfg(unix)]
#[test]
fn a_helper_started_from_the_appimage_gets_none_of_its_folders() {
    use std::ffi::OsString;
    // What AppRun hands dm on a CachyOS desktop.
    let env = |name: &str| -> Option<OsString> {
        Some(OsString::from(match name {
            "PYTHONHOME" => "/tmp/.mount_dmX/usr/",
            "PYTHONPATH" => "/tmp/.mount_dmX/usr/share/pyshared/:",
            "PYTHONDONTWRITEBYTECODE" => "1",
            "LD_LIBRARY_PATH" => "/tmp/.mount_dmX/usr/lib/:/tmp/.mount_dmX/lib/:/opt/cuda/lib",
            "PATH" => "/tmp/.mount_dmX/usr/bin/:/usr/local/bin:/usr/bin",
            "XDG_DATA_DIRS" => "/usr/local/share:/usr/share",
            _ => return None,
        }))
    };
    let out = super::without_appdir(std::path::Path::new("/tmp/.mount_dmX"), env);
    let get = |n: &str| out.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone());
    assert_eq!(get("PYTHONHOME"), Some(None));
    assert_eq!(get("PYTHONPATH"), Some(None));
    assert_eq!(get("PYTHONDONTWRITEBYTECODE"), Some(None));
    assert_eq!(
        get("LD_LIBRARY_PATH"),
        Some(Some(OsString::from("/opt/cuda/lib")))
    );
    assert_eq!(
        get("PATH"),
        Some(Some(OsString::from("/usr/local/bin:/usr/bin")))
    );
    // Left as it is: nothing of the AppImage's in it.
    assert_eq!(get("XDG_DATA_DIRS"), None);
    assert_eq!(get("QT_PLUGIN_PATH"), None);
}

#[test]
fn a_file_error_is_said_without_its_system_number() {
    use std::io::{Error, ErrorKind};
    assert_eq!(
        super::file_error(&Error::from(ErrorKind::NotFound)),
        "it is not there"
    );
    assert_eq!(
        super::file_error(&Error::from(ErrorKind::PermissionDenied)),
        "dm is not allowed to use it"
    );
    assert!(!super::file_error(&Error::from_raw_os_error(5)).contains("os error"));
}

use super::readable;

#[tokio::test]
async fn a_network_error_says_why_not_only_that_it_failed() {
    // A port nothing listens on: the refusal is the reason, and it is
    // under reqwest's "error sending request".
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = super::client()
        .get(format!("http://127.0.0.1:{port}/f"))
        .send()
        .await
        .unwrap_err();
    let said = super::Error::Network(err).to_string();
    assert!(said.to_ascii_lowercase().contains("refused"), "{said}");
}

#[test]
fn an_error_page_is_read_down_to_its_sentence() {
    // Wikimedia's 403, abridged: a head of CSS, then the sentence.
    let page = "<!DOCTYPE html><html lang=\"en\"><meta charset=\"utf-8\">\
        <title>Wikimedia Error</title><style>* { margin: 0; } body { font: 15px }</style>\
        <div class=\"content\"><h1>Error</h1>\
        <p>Too many requests. Please contact noc@wikimedia.org &amp; say so</p></div>";
    assert_eq!(
        readable(page),
        "Error Too many requests. Please contact noc@wikimedia.org & say so"
    );
}

#[test]
fn a_plain_refusal_stays_as_it_was_and_a_long_one_is_cut() {
    assert_eq!(
        readable("  rate limited,\n try later "),
        "rate limited, try later"
    );
    let long = "word ".repeat(100);
    let cut = readable(&long);
    assert!(cut.ends_with('…') && cut.chars().count() <= 201, "{cut}");
}

/// A failed row carried the server's error page —
/// "Error response Error code: 404 Message: File not found. …" — where
/// "404 Not Found" says it all. A body with a sentence of its own stays.
#[test]
fn an_error_page_that_only_restates_the_status_is_not_repeated() {
    let python = "<!DOCTYPE HTML><html lang=\"en\"><head><meta charset=\"utf-8\">\
        <title>Error response</title></head><body><h1>Error response</h1>\
        <p>Error code: 404</p><p>Message: File not found.</p>\
        <p>Error code explanation: 404 - Nothing matches the given URI.</p></body></html>";
    assert_eq!(super::worth_saying(&readable(python), 404), "");
    // A message of its own in the same page is kept.
    let own = python
        .replace("404", "401")
        .replace("File not found", "Token expired");
    assert_eq!(super::worth_saying(&readable(&own), 401), "Token expired");
    let nginx = "<html><head><title>404 Not Found</title></head><body>\
        <center><h1>404 Not Found</h1></center><hr><center>nginx/1.24.0</center></body></html>";
    assert_eq!(super::worth_saying(&readable(nginx), 404), "");
    assert_eq!(super::worth_saying("404 page not found\n", 404), "");
    assert_eq!(super::worth_saying("Forbidden", 403), "");
    assert_eq!(
        super::worth_saying("rate limited, try later", 429),
        "rate limited, try later"
    );
    let wikimedia = "Error Too many requests. Please contact noc@wikimedia.org";
    assert_eq!(super::worth_saying(wikimedia, 429), wikimedia);
    let refused = super::Error::Refused {
        status: 404,
        url: "http://e.com/a".into(),
        body: String::new(),
        retry_after: None,
    };
    assert_eq!(
        refused.to_string(),
        "the server answered 404 Not Found for http://e.com/a"
    );
}

#[test]
fn markup_that_never_closes_ends_the_reading() {
    assert_eq!(readable("before <style> never closed"), "before");
    assert_eq!(readable("before <p never closed"), "before");
}
