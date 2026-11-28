// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[test]
fn a_login_is_kept_for_its_own_host_and_port_only() {
    let landed = |u: &str| url::Url::parse(u).unwrap();
    let login = ["me", "pw"].join(":");
    let asked = format!("http://{login}@h.example/f");
    assert!(super::keep_login(&asked, &landed("http://h.example/g")).contains("me:"));
    assert!(super::keep_login(&asked, &landed("http://h.example:80/g")).contains("me:"));
    assert!(!super::keep_login(&asked, &landed("http://h.example:8081/g")).contains("me:"));
    assert!(!super::keep_login(&asked, &landed("http://other.example/g")).contains("me:"));
}

use super::*;

/* The cases live in `shared/naming-cases.json`. They were written when
 * the extension (then JavaScript) named files too and the two halves
 * named the same download differently — no reserved names here,
 * truncation that cut the extension off, no MIME fallback; the extension
 * now leaves naming to EDM, and the file holds EDM to the same cases. */
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

/// A first part is planned on only where it starts at byte 0, names the file's length and
/// comes unencoded; anything else is asked for again as the whole file.
#[test]
fn a_first_part_is_read_from_a_content_range_that_names_a_total() {
    let headers = |pairs: &[(&'static str, &'static str)]| {
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, reqwest::header::HeaderValue::from_static(value));
        }
        map
    };
    let part = |range: &'static str| first_part(&headers(&[("content-range", range)]));
    assert_eq!(
        part("bytes 0-1048575/536870912"),
        Some((1_048_575, 536_870_912))
    );
    assert_eq!(part(" bytes 0 - 99 / 100 "), Some((99, 100)), "spaces");
    assert_eq!(part("0-99/100"), None, "no `bytes ` before it");
    assert_eq!(part("bytes 0-1048575/*"), None, "no total");
    assert_eq!(part("bytes 512-1048575/9000000"), None, "not from byte 0");
    assert_eq!(part("bytes 0-200/100"), None, "past the total it names");
    assert_eq!(part("bytes */100"), None);
    assert_eq!(first_part(&headers(&[])), None);
    let encoded = headers(&[
        ("content-range", "bytes 0-99/100"),
        ("content-encoding", "gzip"),
    ]);
    assert_eq!(first_part(&encoded), None, "encoded");
}
