// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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

    let older = r#"{ "format": 1, "revision": 0, "exclude": { "advert_hosts": ["x.example"] } }"#;
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

/// Where the file cannot be put in place (something else holds its
/// name), the newer document is refused and nothing half-written is left.
#[test]
fn a_document_that_cannot_be_moved_into_place_is_refused_and_cleaned_up() {
    let dir = home();
    let shipped = Rules::parse(SHIPPED).expect("shipped");
    std::fs::create_dir_all(dir.join(FILE).join("held")).expect("a folder");
    assert_eq!(served(&dir), shipped, "a folder is no document");
    let newer = format!(r#"{{ "format": 1, "revision": {} }}"#, shipped.revision + 1);
    assert!(adopt(&dir, &newer).is_err());
    assert!(
        !dir.join(format!("{FILE}.new")).exists(),
        "the staged copy is gone"
    );
    assert!(dir.join(FILE).join("held").is_dir());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A newer document's YouTube clients are handed to the engine by `apply`, with no new build
/// (gaps row Y1).
#[test]
fn a_newer_documents_youtube_clients_reach_the_engine() {
    let dir = home();
    let shipped = Rules::parse(SHIPPED).expect("shipped");
    let doc = format!(
        r#"{{"format": {}, "revision": {}, "youtube": {{"clients": [{{"name": "ANDROID_VR", "version": "1.6", "id": "28", "agent": "VR"}}]}}}}"#,
        dm_protocol::RULES_FORMAT,
        shipped.revision + 1
    );
    assert_eq!(adopt(&dir, &doc), Ok(true));
    apply(&dir);
    let names: Vec<String> = dm_core::youtube::served_player_clients()
        .into_iter()
        .map(|c| c.name)
        .collect();
    dm_core::youtube::set_player_clients(Vec::new());
    assert_eq!(names, ["ANDROID_VR"]);
}
