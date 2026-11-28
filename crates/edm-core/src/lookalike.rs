// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A host name that looks like another (gaps row H7): written in letters of another alphabet
//! that look like Latin ones ("аpple.com" with a Cyrillic "а"), it reads as the site it copies.
//! Such a host is shown in its `xn--` form, which cannot pass for the real one, as browsers do.
//!
//! EDM's own rule, not a table of every confusable: a label that mixes Latin letters with
//! Cyrillic, Greek or Armenian ones; or a label written wholly in Cyrillic or Greek letters that
//! each look like a Latin one, under a top-level domain that is not in that alphabet (".рф" is
//! Russian's own, and a Russian name under it is not a disguise).

/// The alphabets a host's letters are told apart by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Script {
    Latin,
    Cyrillic,
    Greek,
    Armenian,
    /// Digits, the hyphen, and every other alphabet: they neither disguise nor are disguised.
    Other,
}

fn script(c: char) -> Script {
    match c as u32 {
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x1E00..=0x1EFF => Script::Latin,
        0x400..=0x52F | 0x1C80..=0x1C8F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => Script::Cyrillic,
        0x370..=0x3FF | 0x1F00..=0x1FFF => Script::Greek,
        0x530..=0x58F => Script::Armenian,
        _ => Script::Other,
    }
}

/// Cyrillic and Greek letters that look like a Latin one, small and capital.
const LOOKS_LATIN: &str = "аеорсуххіјѕԁԛԝһӏьвкмнтАВЕКМНОРСТУХІЈЅԜ\
                           αβεικνορτυχοΑΒΕΖΗΙΚΜΝΟΡΤΥΧ";

/// Whether one label of a host disguises itself, under a top-level domain in `tld_script`.
fn label_disguised(label: &str, tld_script: Script) -> bool {
    let scripts: Vec<Script> = label
        .chars()
        .map(script)
        .filter(|s| *s != Script::Other)
        .collect();
    let has = |s: Script| scripts.contains(&s);
    let foreign = [Script::Cyrillic, Script::Greek, Script::Armenian];
    // Latin mixed with a lookalike alphabet.
    if has(Script::Latin) && foreign.iter().any(|s| has(*s)) {
        return true;
    }
    // Wholly in one lookalike alphabet, every letter a Latin one's double, under a top-level
    // domain of another alphabet.
    match scripts.first() {
        Some(&first @ (Script::Cyrillic | Script::Greek))
            if scripts.iter().all(|s| *s == first) && tld_script != first =>
        {
            label
                .chars()
                .filter(|c| script(*c) == first)
                .all(|c| LOOKS_LATIN.contains(c))
        }
        _ => false,
    }
}

/// The host of `url` as written, where it is a lookalike: with its `xn--` form, which the
/// address is shown with. `None` for every other host, a plain or an honest one.
#[must_use]
pub fn lookalike_host(url: &str) -> Option<(String, String)> {
    let parsed = url::Url::parse(url).ok()?;
    let ascii = parsed.host_str()?.to_string();
    let written = written_host(url)?;
    if written.is_ascii() {
        return None;
    }
    let labels: Vec<&str> = written.trim_end_matches('.').split('.').collect();
    let tld_script = labels
        .last()
        .and_then(|tld| tld.chars().map(script).find(|s| *s != Script::Other))
        .unwrap_or(Script::Other);
    labels
        .iter()
        .any(|label| label_disguised(label, tld_script))
        .then_some((written.to_string(), ascii))
}

/// The host as the address writes it: after the scheme's `//` and any sign-in, before the port,
/// the path, the query or the fragment.
fn written_host(url: &str) -> Option<&str> {
    let rest = &url[url.find("://")? + 3..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if host.starts_with('[') {
        host
    } else {
        host.rsplit_once(':').map_or(host, |(host, _)| host)
    };
    (!host.is_empty()).then_some(host)
}

/// `url` with a lookalike host in its `xn--` form, the rest as written.
#[must_use]
pub fn honest(url: &str) -> String {
    match lookalike_host(url) {
        Some((written, ascii)) => url.replacen(&written, &ascii, 1),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host that copies a Latin one with another alphabet's letters is shown in its `xn--`
    /// form; an honest name in another alphabet, and every plain one, as written (gaps row H7).
    #[test]
    fn a_lookalike_host_is_shown_in_its_xn_form() {
        // A Cyrillic "а" in "apple".
        let fake = "https://\u{430}pple.com/setup.exe";
        assert_eq!(honest(fake), "https://xn--pple-43d.com/setup.exe");
        assert!(lookalike_host(fake).is_some());
        // Wholly Cyrillic, every letter a Latin one's double, under ".com".
        let whole = "https://\u{440}\u{430}\u{443}\u{440}\u{430}\u{4cf}.com/";
        assert!(honest(whole).contains("xn--"), "{}", honest(whole));
        // A Greek omicron in "google".
        assert!(honest("https://g\u{3bf}ogle.com/").contains("xn--"));
        // Sign-in and port kept where they were.
        let signed = "https://user@\u{430}pple.com:8443/f";
        assert_eq!(honest(signed), "https://user@xn--pple-43d.com:8443/f");
    }

    #[test]
    fn an_honest_host_is_shown_as_written() {
        for url in [
            "https://example.com/f.zip",
            // Russian under Russia's own domain.
            "https://\u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440}.\u{440}\u{444}/",
            // A German name: Latin letters alone.
            "https://m\u{fc}nchen.de/plan.pdf",
            // Cyrillic letters that look like no Latin one ("д", "ж").
            "https://\u{434}\u{436}.com/",
            // Japanese.
            "https://\u{4f8b}\u{3048}.jp/",
            "https://[::1]:8080/f",
            "not an address",
        ] {
            assert_eq!(honest(url), url, "{url}");
        }
    }
}
