// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn row(url: &str) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": 1, "url": url,
            "state": serde_json::to_value(edm_core::State::Cancelled).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
        }))
        .unwrap(),
    )
}

/// A file in the list (even a cancelled row) or seen before is not new; a
/// listed one is remembered as seen.
#[test]
fn a_find_is_new_only_if_neither_listed_nor_seen() {
    let found = vec![
        "https://h.example/a".to_string(),
        "https://h.example/b".to_string(),
        "https://h.example/c".to_string(),
    ];
    let finds = sort_finds(
        &found,
        &[row("https://h.example/a")],
        &["https://h.example/b".to_string()],
    );
    assert_eq!(finds.new, vec!["https://h.example/c".to_string()]);
    assert_eq!(finds.listed, vec!["https://h.example/a".to_string()]);
}

/// Each remembered once, and no more than `MOST_SEEN`, the oldest going.
#[test]
fn what_is_seen_is_kept_once_and_capped() {
    let seen = remembered(&["a".to_string()], ["a".to_string(), "b".to_string()]);
    assert_eq!(seen, vec!["a".to_string(), "b".to_string()]);
    let many: Vec<String> = (0..MOST_SEEN + 3).map(|n| n.to_string()).collect();
    let kept = remembered(&[], many);
    assert_eq!(kept.len(), MOST_SEEN);
    assert_eq!(kept[0], "3");
}

fn search(name: &str, every_hours: u64, last_run: u64) -> crate::lists::Search {
    crate::lists::Search {
        name: name.into(),
        page: "https://example.com/files/".into(),
        every_hours,
        last_run,
        ..Default::default()
    }
}

#[test]
fn the_searches_due_are_given_with_their_last_run_stamped() {
    let hour = 3600;
    let settings = crate::Settings {
        site_searches: vec![
            search("hourly", 1, 0),
            search("daily", 24, 10 * hour),
            search("by hand", 0, 0),
        ],
        ..crate::Settings::default()
    };
    let (due, wanted) = due_now(&settings, 12 * hour).unwrap();
    assert_eq!(
        due.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["hourly"]
    );
    let runs: Vec<u64> = wanted.site_searches.iter().map(|s| s.last_run).collect();
    assert_eq!(runs, [12 * hour, 10 * hour, 0]);
    assert!(
        due_now(&wanted, 12 * hour + 60).is_none(),
        "not again within the hour"
    );
}

#[test]
fn finds_are_grouped_by_the_folder_their_address_mirrors() {
    let urls = [
        "https://example.com/a/b/one.zip".to_string(),
        "https://example.com/two.zip".to_string(),
        "https://example.com/a/b/three.zip".to_string(),
    ];
    let groups = by_site_folder(&urls, std::path::Path::new("/dl"));
    let folder = crate::lists::layout_folder(&urls[0]).map(|f| {
        std::path::Path::new("/dl")
            .join(f)
            .to_string_lossy()
            .into_owned()
    });
    let top = crate::lists::layout_folder(&urls[1]).map(|f| {
        std::path::Path::new("/dl")
            .join(f)
            .to_string_lossy()
            .into_owned()
    });
    let both = groups.iter().find(|(dir, _)| *dir == folder).unwrap();
    assert_eq!(both.1, [urls[0].clone(), urls[2].clone()]);
    assert!(groups
        .iter()
        .any(|(dir, files)| *dir == top && files == &[urls[1].clone()]));
    assert_eq!(groups.iter().map(|(_, f)| f.len()).sum::<usize>(), 3);
}

#[test]
fn what_a_search_has_seen_is_kept_with_it() {
    let settings = crate::Settings {
        site_searches: vec![search("s", 0, 0)],
        ..crate::Settings::default()
    };
    let wanted = seen_saved(&settings, "s", ["https://e.com/f.zip".to_string()]).unwrap();
    assert_eq!(wanted.site_searches[0].seen, ["https://e.com/f.zip"]);
    assert!(seen_saved(&settings, "none such", Vec::new()).is_none());
}
