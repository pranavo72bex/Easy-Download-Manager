// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Find next and Find previous, for every window: the next row whose name
//! holds the words, going round the list.

/// The place of the next name holding `needle` (in lower case) from `from`,
/// the selected row, going forward or back and wrapping round the list; the
/// selected row itself is the last one tried. From no selection a search
/// begins at the top going forward, at the bottom going back.
pub fn next_match(
    names: &[String],
    from: Option<usize>,
    needle: &str,
    forward: bool,
) -> Option<usize> {
    let n = names.len();
    if n == 0 || needle.is_empty() {
        return None;
    }
    (1..=n)
        .map(|step| match (from, forward) {
            (Some(i), true) => (i + step) % n,
            (Some(i), false) => (i + n - step % n) % n,
            (None, true) => step - 1,
            (None, false) => n - step,
        })
        .find(|&at| names[at].to_lowercase().contains(needle))
}

#[cfg(test)]
mod find_tests {
    use super::next_match;

    fn names() -> Vec<String> {
        ["alpha.zip", "beta.bin", "Alpha2.pdf", "gamma.iso"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn find_goes_forward_and_wraps_round() {
        let names = names();
        assert_eq!(next_match(&names, Some(0), "alpha", true), Some(2));
        assert_eq!(next_match(&names, Some(2), "alpha", true), Some(0), "wraps");
        assert_eq!(next_match(&names, Some(3), "alpha", true), Some(0));
    }

    #[test]
    fn find_previous_goes_back_and_wraps_round() {
        let names = names();
        assert_eq!(next_match(&names, Some(2), "alpha", false), Some(0));
        assert_eq!(
            next_match(&names, Some(0), "alpha", false),
            Some(2),
            "wraps"
        );
        assert_eq!(next_match(&names, Some(1), "alpha", false), Some(0));
    }

    #[test]
    fn the_only_match_is_found_again_from_itself() {
        let names = names();
        assert_eq!(next_match(&names, Some(1), "beta", true), Some(1));
        assert_eq!(next_match(&names, Some(1), "beta", false), Some(1));
    }

    #[test]
    fn with_nothing_selected_it_starts_at_an_end() {
        let names = names();
        assert_eq!(next_match(&names, None, "alpha", true), Some(0));
        assert_eq!(next_match(&names, None, "alpha", false), Some(2));
    }

    #[test]
    fn nothing_matching_or_nothing_typed_finds_nothing() {
        let names = names();
        assert_eq!(next_match(&names, Some(0), "zzz", true), None);
        assert_eq!(next_match(&names, Some(0), "", true), None);
        assert_eq!(next_match(&[], None, "a", true), None);
    }
}
