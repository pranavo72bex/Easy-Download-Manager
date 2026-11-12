// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Readers of outside input every window shares, fed what a fuzzer makes: the control
//! socket, an address as it is shown, and the links read out of text and files. A panic is a
//! failure. Moved from the egui window's tests.

use std::io::{BufRead, BufReader, Write};

use proptest::prelude::*;
use serde_json::{json, Value};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64)
    ))]

    #[test]
    fn any_line_on_the_control_socket_gets_a_line_back_or_the_door(line in prop_oneof![
        any::<String>(),
        "\\{\"id\":[0-9]{0,3},\"method\":\"[a-z.]{0,12}\"(,\"params\":\\{[^}]{0,20}\\})?\\}".prop_map(|s| s),
    ]) {
        // One listener for every case, as dm has one: a listener per case
        // left an accept thread per case, and a deep run ran out of threads.
        let port = control_port();
        // A deep run opens connections faster than the system frees its
        // ports; a case the system will not connect is not one dm answered.
        let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
            return Ok(());
        };
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let line = line.replace('\n', " ");
        stream.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        let _ = BufReader::new(stream).read_line(&mut reply);
        if !reply.is_empty() {
            let parsed: Value = serde_json::from_str(&reply).unwrap();
            // Nothing is answered before the token.
            prop_assert_eq!(&parsed["ok"], &json!(false));
        }
    }

    #[test]
    fn any_address_is_shown_without_its_password(url in prop_oneof![
        any::<String>(),
        "https?://[a-z]{0,6}(:[a-z]{0,6})?@[a-z.]{0,10}/[^ ]{0,10}".prop_map(|s| s),
    ]) {
        let shown = dm_desktop_base::format::shown_url(&url);
        if let Ok(parsed) = url::Url::parse(&url) {
            if let Some(password) = parsed.password().filter(|p| p.len() > 2) {
                prop_assert!(!shown.contains(&format!(":{password}@")), "{shown}");
            }
        }
        let _ = dm_desktop_base::format::provisional(&url);
        let _ = dm_desktop_base::format::host(&url);
    }
}

/// The control listener the socket fuzzing talks to, made once.
fn control_port() -> u16 {
    static PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| {
        let (to_window, requests) = std::sync::mpsc::channel();
        // Kept, or the listener's sends would fail and it would close.
        std::mem::forget(requests);
        dm_desktop_base::control::listen(0, "a".repeat(64), to_window, || {}).unwrap()
    })
}

/// A real address's shape: a scheme, a host (a name, IPv4 or a bracketed
/// IPv6), a port, a percent-encoded path, a query string and a fragment.
fn any_address() -> impl Strategy<Value = String> {
    let host = prop_oneof![
        "[a-z][a-z0-9-]{0,10}(\\.[a-z]{2,6}){1,2}",
        "(25[0-5]|1[0-9]{2}|[1-9]?[0-9])(\\.(25[0-5]|1[0-9]{2}|[1-9]?[0-9])){3}",
        Just("[::1]".to_string()),
        Just("[2001:db8::8a2e:370:7334]".to_string()),
        "\\[fe80::[0-9a-f]{1,4}\\]",
    ];
    let piece = "([A-Za-z0-9._~!$+,;:@-]|%[0-9A-F]{2}){1,10}";
    (
        prop_oneof![Just("http"), Just("https"), Just("ftp"), Just("HTTPS")],
        host,
        prop::option::of(1u16..),
        prop::collection::vec(piece, 0..4),
        prop::option::of(prop::collection::vec(
            (piece, prop::option::of(piece)),
            1..4,
        )),
        prop::option::of("[A-Za-z0-9/?=&%._-]{0,12}"),
    )
        .prop_map(|(scheme, host, port, path, query, fragment)| {
            let mut url = format!("{scheme}://{host}");
            if let Some(port) = port {
                url.push_str(&format!(":{port}"));
            }
            for p in &path {
                url.push('/');
                url.push_str(p);
            }
            if let Some(query) = query {
                url.push('?');
                let pairs: Vec<String> = query
                    .into_iter()
                    .map(|(k, v)| match v {
                        Some(v) => format!("{k}={v}"),
                        None => k,
                    })
                    .collect();
                url.push_str(&pairs.join("&"));
            }
            if let Some(fragment) = fragment {
                url.push('#');
                url.push_str(&fragment);
            }
            // A mark that ends a sentence is taken for the sentence's, as
            // every reader of links in text takes it: an address meant to
            // end in one is not one this generates.
            if url.ends_with(['.', ',', ';', ':', '!', '?']) {
                url.push('x');
            }
            url
        })
        // `&amp;` in an address is read as the page's escaped `&`.
        .prop_filter("no literal &amp;", |url| !url.contains("&amp;"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(256)
    ))]

    /// An address comes out of plain text, a `.url` file, a
    /// `.webloc` plist and a page's `<a href>` exactly as it went in —
    /// its query string, `=` and `&` and all.
    #[test]
    fn every_address_survives_every_kind_of_text_it_arrives_in(
        url in any_address(),
        other in any_address(),
    ) {
        use dm_shell::app::links::links_in;
        let escaped = url.replace('&', "&amp;");
        let plain = format!("see {url}\nand {other} too");
        let shortcut = format!("[InternetShortcut]\r\nURL={url}\r\nIconIndex=0\r\n");
        let webloc = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
             \t<key>URL</key>\n\t<string>{escaped}</string>\n</dict>\n</plist>\n"
        );
        let html = format!("<p><a href=\"{escaped}\">x</a> <a href='{other}'>y</a></p>");
        let bare = format!("<img src={url}>");
        let mut both = vec![url.clone()];
        if other != url {
            both.push(other.clone());
        }
        prop_assert_eq!(links_in(&plain), both.clone());
        prop_assert_eq!(links_in(&shortcut), vec![url.clone()]);
        prop_assert_eq!(links_in(&webloc), vec![url.clone()]);
        prop_assert_eq!(links_in(&html), both);
        prop_assert_eq!(links_in(&bare), vec![url.clone()]);
        // As files: a .webloc with the DOCTYPE macOS writes, and a .url
        // with an icon's address, give the link alone.
        use dm_shell::app::links::links_in_file;
        let real_webloc = webloc.replace(
            "<plist",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist",
        );
        prop_assert_eq!(links_in_file(Some("webloc"), &real_webloc), vec![url.clone()]);
        let iconed = format!("[InternetShortcut]\r\nIconFile=https://e.com/favicon.ico\r\nURL={url}\r\n");
        prop_assert_eq!(links_in_file(Some("url"), &iconed), vec![url.clone()]);
        prop_assert_eq!(links_in_file(Some("txt"), &plain), links_in(&plain));
        // Behind any other key, a refresh's `0;url=`, a forum's `[url=…]`
        // and a note's `(…)`: found, and only the address.
        for wrapped in [
            format!("BASEURL={url}"),
            format!("<div data-src={url}>"),
            format!("<meta http-equiv=refresh content=\"0;url={escaped}\">"),
            format!("[url={url}]x[/url]"),
            format!("see the file ({url}) here"),
            format!("Get {url}. Or that one, {url}, or {url}! (see {url})"),
        ] {
            prop_assert_eq!(links_in(&wrapped), vec![url.clone()], "{}", wrapped);
        }
    }
}
