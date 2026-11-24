// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A file's name: taken, given again, named by a pattern; and the watched folder.

use super::*;

/// Move/Rename: a name taken in that folder is said, with the name given instead; Cancel
/// closes it untouched. (pack.zip is renamed onto file.zip: typing replaces the name and
/// keeps its ending, as Finder's rename does.)
#[test]
fn renaming_onto_a_taken_name_says_what_it_is_called() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("pack.zip"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let pack = h.row("pack.zip").id;
    h.select("pack.zip");
    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.click("Cancel");
    h.until("Cancel to close it", |h| !h.dialog_open("Move/Rename"));
    assert_eq!(h.row("pack.zip").id, pack);
    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.type_text("file");
    h.click("OK");
    h.until("it to be said", |h| {
        h.said().contains("is taken in that folder")
    });
    let given = h
        .rows()
        .into_iter()
        .find(|r| r.id == pack)
        .unwrap()
        .filename;
    assert_ne!(given, "file.zip", "the other file's name was taken over");
    let said = format!("file.zip is taken in that folder, so it is called {given}.");
    assert_eq!(h.said(), said);
    // In the status bar, where a screen reader reads it as it appears.
    h.until("the status bar to say it", |h| {
        h.texts().iter().any(|t| t.contains(&said))
    });
}

/// Save as takes the name the server gives, and leaves one a person typed alone.
#[test]
fn save_as_takes_the_servers_name() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("named"), "Start now");
    // dm asks whether to keep the server's name (`asks.rs`); kept, it is the file's.
    h.until_shown("Name sent by the server");
    h.click("Use report-2026.pdf");
    h.until("it to finish", |h| completed(h) == 1);
    assert!(h.rows()[0].path.ends_with("report-2026.pdf"));
}

/// A file added again, while its first copy is in the folder, is listed under the name it will
/// be placed under, not the one it will not.
#[test]
fn a_file_added_again_is_listed_under_the_name_it_will_get() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the first copy to finish", |h| completed(h) == 1);
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until_shown("file (1).zip");
    // And the control protocol says so too.
    let seen = h.call("ui.inspect", serde_json::json!({})).unwrap();
    let names: Vec<&str> = seen["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert!(names.contains(&"file (1).zip"), "{names:?}");
    // And it is placed there once fetched.
    h.click("Start queue");
    h.until("the second copy to finish", |h| completed(h) == 2);
    assert!(h.rows().iter().any(|r| r.path.ends_with("file (1).zip")));
}

/// Copies of one file added for later are each named as they will be saved, not all
/// "other (1).pdf".
#[test]
fn copies_waiting_in_the_list_each_get_their_own_name() {
    let world = world();
    let mut h = world.harness();
    for _ in 0..3 {
        h.add_url(&world.url("other.pdf"), "Add for later");
    }
    h.until("three rows", |h| h.rows().len() == 3);
    for name in ["other.pdf", "other (1).pdf", "other (2).pdf"] {
        h.until_shown(name);
    }
}

/// A file saved a minute ago, so the folder's reader does not take it for one still being
/// written.
fn aged(path: &std::path::Path) {
    let old = std::time::SystemTime::now() - Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(old)
        .unwrap();
}

#[test]
fn a_link_saved_in_the_watched_folder_is_added_and_its_file_put_aside() {
    let world = world();
    let mut h = world.harness();
    let watched = tempfile::tempdir().unwrap();
    let dir = watched.path();
    std::fs::write(
        dir.join("one.url"),
        format!("[InternetShortcut]\nURL={}\n", world.url("watched-one.zip")),
    )
    .unwrap();
    std::fs::write(
        dir.join("two.txt"),
        format!("{}\n", world.url("watched-two.zip")),
    )
    .unwrap();
    std::fs::write(dir.join("nothing.txt"), "no address in this\n").unwrap();
    std::fs::write(dir.join("picture.png"), b"\x89PNG not read at all").unwrap();
    for f in ["one.url", "two.txt", "nothing.txt", "picture.png"] {
        aged(&dir.join(f));
    }

    // Off until it is switched on in Options.
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(h.rows().is_empty(), "nothing is read while it is off");
    let folder = dir.display().to_string();
    h.apply(|s| s.downloads.watch_folder = folder);

    h.until("both links to be added", |h| {
        let rows = h.rows();
        rows.iter().any(|r| r.url.ends_with("watched-one.zip"))
            && rows.iter().any(|r| r.url.ends_with("watched-two.zip"))
    });
    let done = dir.join("done");
    h.until("both put aside", |_| {
        done.join("one.url").exists() && done.join("two.txt").exists()
    });
    assert!(!dir.join("one.url").exists() && !dir.join("two.txt").exists());
    assert!(
        dir.join("nothing.txt").exists(),
        "a file with no link is left where it is"
    );
    assert!(dir.join("picture.png").exists(), "other kinds are not read");
    assert_eq!(h.rows().len(), 2, "the file with no link added nothing");
    h.until("the file with no link to be said once", |h| {
        h.view(|view| {
            view.status
                .problems
                .iter()
                .any(|p| p.contains("nothing.txt") && p.contains("no link"))
        })
    });

    // The same link saved again is already in the list: put aside, not added.
    std::fs::write(
        dir.join("again.url"),
        format!("[InternetShortcut]\nURL={}\n", world.url("watched-one.zip")),
    )
    .unwrap();
    aged(&dir.join("again.url"));
    h.until_within(
        "the link already listed to be put aside",
        Duration::from_secs(15),
        &mut |_| done.join("again.url").exists(),
    );
    assert_eq!(
        h.rows().len(),
        2,
        "a link already listed is not added again"
    );
}

/// Move/Rename of several finished files, with a pattern: `{n}` counts from 1 in the order
/// listed, `{name}` is the old name, the ending is kept, and an empty pattern leaves the names
/// alone.
#[test]
fn several_finished_files_are_named_by_a_pattern_and_keep_their_endings() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let (zip, pdf) = (h.row("file.zip").id, h.row("other.pdf").id);
    // In the order the list shows them.
    let listed: Vec<String> = h.call("ui.inspect", serde_json::json!({})).unwrap()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str().map(str::to_string))
        .collect();
    h.run(Command::SelectAll);
    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    assert!(
        h.shows("Name pattern"),
        "several files are named by a pattern: {:?}",
        h.texts()
    );
    h.click("OK");
    h.until("it to close", |h| !h.dialog_open("Move/Rename"));
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert_eq!(h.row("file.zip").id, zip, "empty keeps names");

    h.run(Command::MoveRename);
    h.until("Move/Rename", |h| h.dialog_open("Move/Rename"));
    h.type_text("Trip {n} ({name})");
    h.click("OK");
    let now = |h: &mut Harness, id| h.rows().into_iter().find(|r| r.id == id).unwrap();
    h.until("the new names", |h| {
        now(h, zip).filename.starts_with("Trip") && now(h, pdf).filename.starts_with("Trip")
    });
    let (zip_row, pdf_row) = (now(&mut h, zip), now(&mut h, pdf));
    let wanted = |nth: usize| {
        let (stem, ending) = listed[nth].rsplit_once('.').unwrap();
        format!("Trip {} ({stem}).{ending}", nth + 1)
    };
    let mut names = vec![zip_row.filename.clone(), pdf_row.filename.clone()];
    names.sort();
    let mut expected = vec![wanted(0), wanted(1)];
    expected.sort();
    assert_eq!(names, expected);
    assert!(
        std::path::Path::new(&zip_row.path).exists()
            && std::path::Path::new(&pdf_row.path).exists(),
        "the files on disk were renamed with them"
    );
}
