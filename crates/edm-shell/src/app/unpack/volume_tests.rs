// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn the_parts_of_one_archive_are_known_by_their_names() {
    assert_eq!(part_of("a.part1.rar"), Some(("a.rar".into(), 1)));
    assert_eq!(part_of("a.part2.rar"), Some(("a.rar".into(), 2)));
    assert_eq!(part_of("Film.Part03.RAR"), Some(("Film.rar".into(), 3)));
    assert_eq!(part_of("b.7z.001"), Some(("b.7z".into(), 1)));
    assert_eq!(part_of("b.7z.002"), Some(("b.7z".into(), 2)));
    assert_eq!(part_of("c.zip.010"), Some(("c.zip".into(), 10)));
    for none in [
        "a.rar",
        "x.001.txt",
        "x.001",
        "b.7z",
        "b.7z.1",
        "a.part.rar",
        "a.part0.rar",
        ".part1.rar",
        ".7z.001",
        "a.partx.rar",
    ] {
        assert_eq!(part_of(none), None, "{none}");
    }
}

#[test]
fn a_set_is_whole_when_its_parts_run_from_the_first_with_none_missing() {
    assert!(all_present(
        "a.rar",
        &["a.part1.rar", "a.part2.rar", "other.zip"]
    ));
    assert!(all_present("b.7z", &["b.7z.002", "b.7z.001", "b.7z.003"]));
    assert!(
        !all_present("a.rar", &["a.part2.rar", "a.part3.rar"]),
        "no first part"
    );
    assert!(
        !all_present("b.7z", &["b.7z.001", "b.7z.003"]),
        "the second missing"
    );
    assert!(!all_present("a.rar", &["b.7z.001"]), "another set");
    assert!(!all_present("a.rar", &[]));
}
