// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A folder asked about many names at once is listed instead (Windows), and says what the
/// disk says: a name taken in another case is taken, `(1)` and `(2)` are counted, a name
/// nothing has is free.
#[test]
fn a_folder_asked_about_many_names_answers_as_the_disk_does() {
    let dir = tempfile::tempdir().unwrap();
    for taken in ["Big.BIN", "big (1).bin", "other.zip"] {
        std::fs::write(dir.path().join(taken), b"x").unwrap();
    }
    let asked = [
        ("big.bin", 0),
        ("big.bin", 1),
        ("BIG.bin", 0),
        ("other.zip", 0),
        ("free.txt", 0),
        ("free.txt", 2),
    ];
    // Past the count that lists the folder, and again within it.
    for _ in 0..30 {
        for (name, nth) in asked {
            assert_eq!(
                nth_free_in(dir.path(), name, nth),
                edm_core::nth_free_name(dir.path(), name, nth),
                "{name}, {nth}"
            );
        }
    }
    // A folder not made yet holds nothing.
    let gone = dir.path().join("gone");
    for _ in 0..100 {
        assert_eq!(nth_free_in(&gone, "a.bin", 0), "a.bin");
    }
}

fn row(id: u64, url: &str, state: State) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": url,
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": "",
        }))
        .unwrap(),
    )
}

#[test]
fn rows_bound_for_one_name_each_take_the_next_free_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.bin"), b"there").unwrap();
    let rows = vec![
        row(1, "https://e.com/big.bin", State::Queued),
        row(2, "https://e.com/big.bin", State::Queued),
        row(3, "https://e.com/other.zip", State::Queued),
    ];
    let before = earlier_of_the_same_name(&rows);
    assert_eq!((before[&1], before[&2], before[&3]), (0, 1, 0));
    let mut categories = edm_core::Categories {
        enabled: false,
        ..Default::default()
    };
    let named = |r: &Entry, known| {
        name_it_will_get_after(&rows, r, dir.path(), &mut categories.clone(), known)
    };
    assert_eq!(named(&rows[0], None), "big (1).bin");
    assert_eq!(named(&rows[1], Some(before[&2])), "big (2).bin");
    assert_eq!(named(&rows[2], None), "other.zip");
    assert_eq!(
        planned_folder(&rows[0], dir.path(), &mut categories),
        Some(dir.path().display().to_string())
    );
}

#[test]
fn a_settled_row_keeps_its_own_name_and_has_no_planned_folder() {
    let done = row(4, "https://e.com/done.iso", State::Completed);
    let mut categories = edm_core::Categories::default();
    assert_eq!(
        planned_folder(&done, Path::new("/nowhere"), &mut categories),
        None
    );
    // Not yet named by the server: its address's name, and its kind
    // from that.
    assert_eq!(name_of(&done), "done.iso");
    let film = row(5, "https://e.com/v/film.mp4?t=1", State::Queued);
    assert_eq!(name_of(&film), "film.mp4");
    assert_eq!(kind_of(&film), Some(Category::Videos));
}

/// A page named by a long query is cut to sixty characters; a name of no
/// known kind goes to a category a person made that takes it, and to no
/// planned folder otherwise.
#[test]
fn a_long_query_name_is_cut_and_a_persons_category_places_an_unknown_kind() {
    let long = format!("https://www.e.com/watch?v={}", "x".repeat(80));
    let shown = provisional(&long);
    assert!(shown.starts_with("e.com/watch?v=xx"), "{shown}");
    assert!(shown.ends_with('…'), "{shown}");
    assert_eq!(shown.chars().count(), 61);
    assert_eq!(
        provisional("https://e.com/watch?v=abc"),
        "e.com/watch?v=abc"
    );

    let dir = tempfile::tempdir().unwrap();
    let odd = row(6, "https://e.com/notes.zzq", State::Queued);
    let mut categories = edm_core::Categories {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(planned_folder(&odd, dir.path(), &mut categories), None);
    categories.custom = vec![edm_core::Custom {
        name: "Odd".into(),
        extensions: vec!["zzq".into()],
        ..Default::default()
    }];
    assert_eq!(
        planned_folder(&odd, dir.path(), &mut categories),
        Some(dir.path().join("Odd").display().to_string())
    );
}

#[test]
fn a_rename_waits_for_the_file_to_be_whole_and_is_dropped_if_it_never_is() {
    let mut renames = PendingRenames::default();
    assert_eq!(
        renames.ask(1, Some(&State::Completed), "a.bin").as_deref(),
        Some("a.bin"),
        "finished: at once"
    );
    assert_eq!(renames.ask(2, Some(&State::Running), "b.bin"), None);
    assert_eq!(renames.ask(3, Some(&State::Queued), "c.bin"), None);
    assert_eq!(renames.ask(4, Some(&State::Running), "d.bin"), None);
    assert_eq!(renames.ask(5, Some(&State::Cancelled), "e.bin"), None);
    assert_eq!(renames.ask(6, None, "f.bin"), None);
    let states = |id: Id| match id {
        2 => Some(State::Completed),
        3 => Some(State::Running),
        4 => Some(State::Failed("gone".into())),
        _ => None,
    };
    assert_eq!(renames.due(states), vec![(2, "b.bin".to_string())]);
    // 3 still waits; 4 failed and is dropped.
    let finished = |id: Id| (id == 3 || id == 4).then_some(State::Completed);
    assert_eq!(renames.due(finished), vec![(3, "c.bin".to_string())]);
    assert!(renames.due(finished).is_empty(), "each once");
}

#[test]
fn a_kinds_folder_is_its_categorys_and_another_word_a_folder_of_that_name() {
    let root = Path::new("/dl");
    let categories = edm_core::Categories::default();
    let videos = category_word(Category::Videos);
    assert!(!videos.is_empty());
    assert_eq!(
        category_folder(&videos, root, &categories),
        categories.destination(root, Category::Videos)
    );
    assert_eq!(
        category_folder("Work", root, &categories),
        root.join("Work")
    );
}

/// gaps row 181: the counts of earlier rows of the same name are those of
/// `earlier_of_the_same_name`, and the rows alike are those not yet placed bound for the same
/// folder under one of the names asked about, in the list's order, found in the same pass.
#[test]
fn the_rows_alike_are_found_beside_the_counts_in_one_pass() {
    let rows = vec![
        row(1, "https://e.com/big.bin", State::Queued),
        row(2, "https://e.com/small.bin", State::Queued),
        row(3, "https://e.com/big.bin", State::Completed),
        row(4, "https://e.com/big.bin", State::Paused),
        row(5, "https://e.com/other.bin", State::Running),
    ];
    let (counts, alike) = earlier_and_alike(&rows, &[(None, "big.bin".to_string())]);
    assert_eq!(counts, earlier_of_the_same_name(&rows));
    assert_eq!(counts.get(&4), Some(&1), "row 1 before it");
    // Row 3 is finished: placed, and not among them.
    assert_eq!(alike, vec![0, 3]);
    // Another folder is another name.
    let (_, elsewhere) = earlier_and_alike(&rows, &[(Some("/x".into()), "big.bin".into())]);
    assert!(elsewhere.is_empty());
    assert!(
        earlier_and_alike(&rows, &[]).1.is_empty(),
        "none asked about"
    );
}
