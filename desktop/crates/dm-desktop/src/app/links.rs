// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Links in text and in files (`dm_shell::app::links`), and a file egui hands
//! over as a drop read for them.

pub use dm_shell::app::links::*;

/// The links in a file dropped on the window: read from its path, else from
/// the bytes egui carries for it.
pub fn dropped_links(file: &dyn egui::DroppedFile) -> Vec<String> {
    links_at(file.path(), || file.bytes().ok())
}

#[cfg(test)]
mod two_addresses_tests {
    use super::*;
    use crate::app::{as_shown, worded_problem};

    #[test]
    fn the_saved_proxy_turned_off_at_launch_is_said_in_the_windows_words() {
        let said = worded_problem(
            "the saved proxy could not be used (the proxy has no host), so it is turned off and downloads \
             go out directly. Fix it under Options › Advanced and tick it again.",
        );
        assert!(
            said.starts_with("The proxy can't be used (the proxy has no host)"),
            "{said}"
        );
        assert_eq!(worded_problem("something else"), "something else");
        assert_eq!(
            as_shown("the cookies file x could not be read"),
            "The cookies file x could not be read"
        );
        assert_eq!(as_shown("ünlü"), "Ünlü");
        assert_eq!(as_shown(""), "");
    }

    #[test]
    fn two_links_pasted_into_one_field_are_seen() {
        for two in [
            "https://e.com/a.ziphttps://e.com/b.zip",
            "https://e.com/a.zip https://e.com/b.zip",
            "http://e.com/a.zipftp://e.com/b",
            "https://e.com/a.zipmagnet:?xt=urn:btih:abc",
        ] {
            assert!(two_addresses(two), "{two}");
        }
        for one in [
            "https://e.com/a.zip",
            "https://e.com/go?to=https://f.com/b.zip",
            "https://archive.example/web/2020/https://e.com/a.zip",
            "https://e.com/r?u=https%3A//f.com&next=http://g.com",
            "magnet:?xt=urn:btih:abc&tr=http://tracker.example/announce",
        ] {
            assert!(!two_addresses(one), "{one}");
        }
    }
}
