// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::Mode;

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(String::from).collect()
}

fn typo(words: &[&str]) -> Mode {
    Mode::Typo(words.iter().map(|w| w.to_string()).collect())
}

/// Each command line, and the one mode it asks for: the first that fits,
/// as `main` sent them before there was one `Mode`.
#[test]
fn each_command_line_is_one_mode() {
    let table = [
        // The window, with what it is given.
        ("", Mode::Window),
        ("https://example.com/file.zip", Mode::Window),
        (
            "--save-to Videos --name talk.mp4 https://example.com/v.mp4",
            Mode::Window,
        ),
        (
            "--later https://example.com/a.iso --later https://example.com/b.iso",
            Mode::Window,
        ),
        ("--start-queue --when-done shutdown", Mode::Window),
        ("--hidden", Mode::Window),
        ("--quiet --new-download", Mode::Window),
        ("magnet:?xt=urn:btih:abc", Mode::Window),
        // No window at all, anywhere on the line.
        ("--headless", Mode::Headless),
        ("--control 0 --headless", Mode::Headless),
        ("--warm-reading --headless", Mode::Headless),
        // The warm page reader: its word alone.
        ("--warm-reading", Mode::Warm),
        // The browser's host.
        ("--native-host", Mode::BrowserHost),
        ("chrome-extension://abcdefghijklmnop/", Mode::BrowserHost),
        (
            "chrome-extension://abcdefghijklmnop/ --parent-window=0",
            Mode::BrowserHost,
        ),
        ("moz-extension://abcdefghijklmnop/", Mode::BrowserHost),
        // The first word's own commands.
        ("--version", Mode::Version),
        ("-V", Mode::Version),
        ("--version --headless", Mode::Version),
        ("--help", Mode::Help),
        ("-h", Mode::Help),
        ("/?", Mode::Help),
        ("list", Mode::Command),
        ("list --json", Mode::Command),
        ("status 3", Mode::Command),
        ("pause 1 2", Mode::Command),
        ("resume 1", Mode::Command),
        ("cancel 4", Mode::Command),
        ("mcp", Mode::Mcp),
        ("mcp --port 9 --token-file token", Mode::Mcp),
        ("--register", Mode::Register),
        (
            "--register abcdefghijklmnopabcdefghijklmnop",
            Mode::Register,
        ),
        ("--unregister", Mode::Unregister),
        // A word that is no link, file or command, before any window.
        ("lst", typo(&["lst"])),
        ("--headless lst", typo(&["lst"])),
        ("lst stauts", typo(&["lst", "stauts"])),
        ("--save-to lst https://example.com/a.zip", Mode::Window),
    ];
    for (line, mode) in table {
        assert_eq!(Mode::parse(&words(line)), mode, "{line:?}");
    }
}

/// The two modes with a window are the window's `main`'s to start: running
/// them here does nothing.
#[test]
fn the_window_and_headless_are_left_to_main() {
    assert_eq!(Mode::Window.run(&[]), None);
    assert_eq!(Mode::Headless.run(&words("--headless")), None);
}

/// Every switch the command line's `orders` reads is named in `--help`: read off `orders`'s
/// own code, so a switch added there without a line here fails (gaps row C7b, `--queue`).
#[test]
fn the_usage_names_every_switch_orders_reads() {
    let source = include_str!("../../../../edm-shell/src/shell/args.rs");
    let start = source.find("pub fn orders(").expect("orders is in args.rs");
    let body = &source[start..];
    let body = &body[..body.find("\n}\n").expect("orders ends")];
    let switches: Vec<&str> = body
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|word| word.starts_with("--") && word.len() > 2)
        .collect();
    assert!(switches.contains(&"--queue"), "{switches:?}");
    for switch in switches {
        assert!(
            super::USAGE.contains(switch),
            "--help does not name {switch}"
        );
    }
}
