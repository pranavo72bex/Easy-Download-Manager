// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The site rules the app serves to the extension in its hello: the document it ships, or a newer
//! `rules.json` in dm's home folder. Nothing here can fail a hello: a missing, unreadable, too large,
//! malformed or older file is passed over, and the extension keeps its compiled tables regardless.

use std::path::Path;

use dm_protocol::Rules;

/// The document shipped inside the app.
const SHIPPED: &str = include_str!("../../resources/rules.json");

/// The file a newer document is put in, in dm's home folder.
pub const FILE: &str = "rules.json";

/// The rules to serve: the newer of the shipped document and the home folder's.
pub fn served(home: &Path) -> Rules {
    let shipped = Rules::parse(SHIPPED).unwrap_or_default();
    Rules::newest(shipped, found(home))
}

/// Keeps `text`, a rules document fetched by the update check, in the home folder when it is one
/// this side reads and newer than what is served now (the shipped document being the floor).
/// `Ok(true)` when it was kept, `Ok(false)` when it is not newer; the previous file is untouched
/// on any error. Written whole under another name and moved into place.
pub fn adopt(home: &Path, text: &str) -> Result<bool, String> {
    let rules = Rules::parse(text)?;
    if rules.revision <= served(home).revision {
        return Ok(false);
    }
    let temp = home.join(format!("{FILE}.new"));
    std::fs::write(&temp, text).map_err(|why| why.to_string())?;
    std::fs::rename(&temp, home.join(FILE)).map_err(|why| {
        let _ = std::fs::remove_file(&temp);
        why.to_string()
    })?;
    Ok(true)
}

/// The home folder's document, if it is there and is one.
fn found(home: &Path) -> Option<Rules> {
    let path = home.join(FILE);
    // Not read past the cap: a file beyond it is refused whole, whatever it holds.
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > dm_protocol::MAX_RULES_BYTES as u64 {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    Rules::parse(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dm-served-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a folder");
        dir
    }

    #[test]
    fn the_shipped_document_is_one_the_extension_reads() {
        let shipped = Rules::parse(SHIPPED).expect("the shipped rules.json parses");
        assert_eq!(shipped.format, dm_protocol::RULES_FORMAT);
        assert!(shipped.revision >= 1);
    }

    #[test]
    fn with_no_file_the_shipped_rules_are_served() {
        let dir = home();
        assert_eq!(served(&dir), Rules::parse(SHIPPED).expect("shipped"));
    }

    #[test]
    fn a_newer_file_in_the_home_wins_and_an_older_or_broken_one_changes_nothing() {
        let dir = home();
        let shipped = Rules::parse(SHIPPED).expect("shipped");
        let newer = format!(
            r#"{{ "format": 1, "revision": {}, "exclude": {{ "advert_hosts": ["ads.example"] }} }}"#,
            shipped.revision + 1
        );
        std::fs::write(dir.join(FILE), &newer).expect("written");
        let got = served(&dir);
        assert_eq!(got.revision, shipped.revision + 1);
        assert_eq!(got.exclude.advert_hosts, vec!["ads.example"]);

        let older =
            r#"{ "format": 1, "revision": 0, "exclude": { "advert_hosts": ["x.example"] } }"#;
        std::fs::write(dir.join(FILE), older).expect("written");
        assert_eq!(served(&dir), shipped);

        for bad in ["not json", "{}", r#"{ "format": 9, "revision": 99 }"#, ""] {
            std::fs::write(dir.join(FILE), bad).expect("written");
            assert_eq!(served(&dir), shipped, "{bad:?}");
        }

        let huge = vec![b' '; dm_protocol::MAX_RULES_BYTES + 1];
        std::fs::write(dir.join(FILE), huge).expect("written");
        assert_eq!(served(&dir), shipped);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopting_keeps_only_a_newer_document_and_leaves_the_old_file_on_any_refusal() {
        let dir = home();
        let shipped = Rules::parse(SHIPPED).expect("shipped");
        let doc = |revision: u64, host: &str| {
            format!(
                r#"{{ "format": 1, "revision": {revision}, "exclude": {{ "advert_hosts": ["{host}"] }} }}"#
            )
        };
        // Not newer than the shipped floor: not kept.
        assert_eq!(adopt(&dir, &doc(shipped.revision, "a.example")), Ok(false));
        assert!(!dir.join(FILE).exists());
        // Newer: kept and served.
        assert_eq!(
            adopt(&dir, &doc(shipped.revision + 5, "b.example")),
            Ok(true)
        );
        assert_eq!(served(&dir).exclude.advert_hosts, vec!["b.example"]);
        // Older than the kept one: not kept, the kept one stays.
        assert_eq!(
            adopt(&dir, &doc(shipped.revision + 2, "c.example")),
            Ok(false)
        );
        assert_eq!(served(&dir).exclude.advert_hosts, vec!["b.example"]);
        // Garbage, a format not read, too large: refused, the kept one stays.
        for bad in ["not json", r#"{ "format": 9, "revision": 99 }"#] {
            assert!(adopt(&dir, bad).is_err(), "{bad:?}");
        }
        let huge = " ".repeat(dm_protocol::MAX_RULES_BYTES + 1);
        assert!(adopt(&dir, &huge).is_err());
        assert_eq!(served(&dir).exclude.advert_hosts, vec!["b.example"]);
        assert!(!dir.join(format!("{FILE}.new")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
