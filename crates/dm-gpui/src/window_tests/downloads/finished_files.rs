// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Finished files: extracted, previewed, opened with, removed, put in the trash and checked.

use super::*;

/// Extract here and Extract to…; cancelling the folder panel is not tried here, as the test
/// platform's panel is the system's own.
#[test]
fn a_finished_zip_is_extracted_beside_itself_and_a_hostile_one_is_refused() {
    use std::io::Write;
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("file.zip to finish", |h| completed(h) == 1);
    h.select("file.zip");
    let path = std::path::PathBuf::from(h.row("file.zip").path);
    assert!(h.enabled(Command::ExtractHere));

    // The served bytes are not a ZIP: said so, and nothing is made.
    h.run(Command::ExtractHere);
    h.until("the refusal", |h| h.said().starts_with("Not extracted"));

    let write = |names: &[&str]| {
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for name in names {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"inside").unwrap();
        }
        w.finish().unwrap();
    };
    write(&["a.txt", "../escape.txt"]);
    h.run(Command::ExtractHere);
    h.until("the hostile archive's refusal", |h| {
        h.said().contains("leave its folder")
    });
    let folder = path.with_extension("");
    assert!(!folder.exists(), "a refused archive leaves no folder");
    assert!(!path.parent().unwrap().join("escape.txt").exists());

    write(&["a.txt", "sub/b.txt"]);
    h.run(Command::ExtractHere);
    h.until("the extraction", |h| {
        h.said().starts_with("Extracted 2 files")
    });
    assert_eq!(std::fs::read(folder.join("sub/b.txt")).unwrap(), b"inside");
    assert!(path.exists(), "the archive is kept");

    // Extract to…: into the folder the person picks, a new folder inside it.
    let elsewhere = tempfile::tempdir().unwrap();
    assert!(h.enabled(Command::ExtractTo));
    h.view(|view| view.say(String::new()));
    crate::platform::services::choose_ahead(elsewhere.path().to_path_buf());
    h.run(Command::ExtractTo);
    let name = path.file_stem().unwrap().to_owned();
    h.until("the extraction elsewhere", |_| {
        elsewhere.path().join(&name).join("a.txt").is_file()
    });
    h.until("it to be said", |h| {
        h.said().starts_with("Extracted 2 files")
    });
    assert_eq!(
        std::fs::read(elsewhere.path().join(&name).join("a.txt")).unwrap(),
        b"inside"
    );
}

#[test]
fn a_video_half_fetched_is_previewed_from_what_is_on_disk() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("half.mp4"), "Start now");
    h.until("the first bytes to arrive", |h| {
        h.rows().iter().any(|r| r.progress.downloaded >= 128 * 1024)
    });
    h.select("half.mp4");
    assert!(!h.enabled(Command::OpenFile), "not finished");
    assert!(h.enabled(Command::Preview));
    h.run(Command::Preview);
    let preview = |h: &Harness| opened(h).into_iter().find(|p| p.ends_with("preview.mp4"));
    h.until("the preview to be opened", |h| preview(h).is_some());
    let preview = preview(&h).unwrap();
    let bytes = std::fs::read(&preview).unwrap();
    // From the start, all of it but what is still in the engine's write buffer, 64 KiB at
    // most (the staged file is as long as the whole, zeros past that).
    assert!(bytes.len() >= 32 * 1024, "{} bytes", bytes.len());
    assert!(bytes[..32 * 1024].iter().all(|b| *b == 9));

    // A file no player shows is not offered.
    h.add_url(&world.url("stalled.bin"), "Start now");
    h.until("its row", |h| h.rows().len() == 2);
    h.select("stalled.bin");
    assert!(!h.enabled(Command::Preview));

    // Removing the row takes the preview with its parts.
    let id = h.row("half.mp4").id;
    h.view(|view| view.backend.remove(id));
    h.until("the preview to go with its row", |_| {
        !std::path::Path::new(&preview).exists()
    });
}

#[test]
fn a_finished_file_deleted_since_says_so_and_is_not_offered_to_open() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    h.select("file.zip");
    assert!(h.enabled(Command::OpenFile));

    std::fs::remove_file(&row.path).unwrap();
    h.until_shown("File missing");
    assert!(!h.enabled(Command::OpenFile));
    assert!(!h.enabled(Command::OpenWith));
    assert!(
        h.enabled(Command::OpenFolder),
        "Open folder still answers: it says the file is gone"
    );
    assert!(h.enabled(Command::Redownload));
}

/// Open with… asks for an application and opens the file with it. A cancelled panel, and a
/// desktop with none, are the system's own panel's to show: not tried here.
#[test]
fn open_with_asks_for_an_application_and_opens_the_file_with_it() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    h.select("file.zip");
    assert!(h.enabled(Command::OpenWith));
    let app = std::path::PathBuf::from("/Applications/Archive Utility.app");
    crate::platform::services::choose_ahead(app.clone());
    h.run(Command::OpenWith);
    let wanted = format!("{} with {}", row.path, app.display());
    h.until("the file to be opened with it", |h| {
        opened(h).contains(&wanted)
    });
    assert_eq!(opened(&h), [wanted]);
}

/// Remove can take a finished file off the disk with its row, when asked; left alone, the
/// file stays.
#[test]
fn remove_deletes_the_file_too_only_when_asked() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let (zip, pdf) = (h.row("file.zip").path, h.row("other.pdf").path);
    h.select("file.zip");
    h.run(Command::Remove);
    h.until("the row to go", |h| h.rows().len() == 1);
    assert!(
        std::path::Path::new(&zip).is_file(),
        "the file went without being asked"
    );
    // Delete file is the one that asks, and says what happens to the file.
    h.select("other.pdf");
    h.run(Command::DeleteFile);
    h.until_shown("Delete file");
    h.click("Delete file");
    h.until("the row and its file to go", |h| h.rows().is_empty());
    h.until("the file to go", |_| !std::path::Path::new(&pdf).exists());
    assert!(
        opened(&h).contains(&format!("trash {pdf}")),
        "the file was erased, not put in the Recycle Bin or Trash"
    );
}

/// "Delete file after": the row says how long its file has left, and once the time comes the
/// file goes to the Trash and the row with it.
#[test]
fn a_file_given_a_time_goes_to_the_trash_when_it_comes() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let (zip, pdf) = (h.row("file.zip"), h.row("other.pdf"));
    // An hour: said in the row, and the file stays.
    h.view(|view| view.backend.set_delete_after(pdf.id, Some(3600)));
    h.until("the row to say so", |h| {
        h.texts().iter().any(|t| t.contains("to the Trash in"))
    });
    assert!(std::path::Path::new(&pdf.path).is_file());
    // A second: gone to the Trash, the row with it; the other row stays.
    h.view(|view| view.backend.set_delete_after(pdf.id, Some(1)));
    h.until("the file to be deleted", |h| {
        h.rows().iter().all(|r| r.id != pdf.id)
    });
    assert!(!std::path::Path::new(&pdf.path).exists(), "the file stayed");
    assert!(
        opened(&h).contains(&format!("trash {}", pdf.path)),
        "the file was erased, not put in the Trash"
    );
    assert!(
        std::path::Path::new(&zip.path).is_file(),
        "the other went too"
    );
    assert_eq!(h.rows().len(), 1);
}

/// Properties of a finished file shows its SHA-256, worked out beside the window, and says
/// whether a digest it was given matches.
#[test]
fn properties_show_the_files_sha256_and_check_a_given_digest() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the file to complete", |h| completed(h) == 1);
    let finished = h.row("file.zip");
    let expected = world
        .runtime
        .block_on(dm_core::digest(
            std::path::Path::new(&finished.path),
            dm_core::Algorithm::Sha256,
        ))
        .unwrap();
    h.select("file.zip");
    h.run(Command::Properties);
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until_shown(&expected);
    // A matching digest, given in any case.
    h.click_beside("Check against");
    h.type_text(&expected.to_uppercase());
    h.until_shown("It matches.");
    assert!(!h.shows("It does not match."));
}

/// Taking a finished download off the list asks nothing, offers Undo, and Undo puts it back
/// where it was; its file never moves.
#[test]
fn removing_a_finished_download_is_undone_from_the_status_bar() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.add_url(&world.url("other.pdf"), "Start now");
    h.until("both to complete", |h| completed(h) == 2);
    let first = h.row("file.zip").id;
    let path = h.row("file.zip").path;
    let order: Vec<_> = h.rows().iter().map(|r| r.id).collect();
    h.select("file.zip");
    h.run(Command::Remove);
    h.until("the row to go", |h| h.rows().len() == 1);
    h.until("Undo", |h| {
        h.texts().iter().any(|t| t.contains("from the list"))
    });
    h.click("Undo");
    h.until("the row to come back", |h| h.rows().len() == 2);
    let ids: Vec<_> = h.rows().iter().map(|r| r.id).collect();
    assert_eq!(ids, order, "not back in its place");
    assert!(ids.contains(&first));
    assert!(std::path::Path::new(&path).is_file());
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    assert!(!h.shows("Undo"), "Undo stayed after it was used");
}

/// A finished program asks before it opens, once: Cancel leaves it unopened, Open opens it, and
/// the next Open opens it without asking again (gaps row H1). A document opens at once.
#[test]
fn a_finished_program_asks_once_before_it_opens() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("setup.exe"), "Start now");
    h.until("the download to complete", |h| completed(h) == 1);
    let path = h.rows()[0].path.clone();
    h.select("setup.exe");
    h.run(Command::OpenFile);
    h.until_shown("Open a program?");
    h.click("Cancel");
    h.until("the question closed", |h| !h.shows("Open a program?"));
    assert!(opened(&h).is_empty(), "opened though Cancel was chosen");
    h.run(Command::OpenFile);
    h.until_shown("Open a program?");
    // As a person presses it: the control port does not open a file for a script.
    h.click_as_person("Open", 0);
    h.until("the program opened", |h| opened(h).contains(&path));
    h.run(Command::OpenFile);
    h.until("opened again", |h| opened(h).len() == 2);
    assert!(!h.shows("Open a program?"), "asked a second time");
}

/// Extract here opens an archive with a password saved in Options › Passwords: each saved one
/// is tried, and the one that opened it goes to the top of the list (gaps row C2a). It said
/// "Not extracted" for every archive with a password before.
#[test]
fn extract_here_opens_an_archive_with_a_saved_password() {
    let _file = crate::harness::settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("file.zip to finish", |h| completed(h) == 1);
    h.select("file.zip");
    let path = std::path::PathBuf::from(h.row("file.zip").path);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dm-core/tests/fixtures/archives/zip/aes256.zip");
    std::fs::copy(&fixture, &path).unwrap();
    let home = dm_shell::settings::home();
    let saved = ["not this one".to_string(), "dm secret".to_string()];
    dm_core::extract::passwords::save(&home, &saved).unwrap();
    h.run(Command::ExtractHere);
    h.until("the extraction", |h| {
        h.said().starts_with("Extracted 4 files") || h.said().starts_with("Not extracted")
    });
    let said = h.said();
    let first = dm_core::extract::passwords::load(&home).first().cloned();
    dm_core::extract::passwords::save(&home, &[]).unwrap();
    assert!(said.starts_with("Extracted 4 files"), "{said}");
    assert_eq!(
        std::fs::read(path.with_extension("").join("docs/a.txt")).unwrap(),
        b"hello, zip\n"
    );
    assert_eq!(
        first.as_deref(),
        Some("dm secret"),
        "the password that opened it is not first"
    );
}

/// Extract here on an archive none of the saved passwords opens asks for its password: a
/// wrong one is said in the dialog, which stays open, and the right one, with "Keep this
/// password" on, extracts it and is saved first (gaps row C2b). It said "Not extracted" before.
#[test]
fn extract_here_asks_for_a_password_when_no_saved_one_fits() {
    let _file = crate::harness::settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Start now");
    h.until("file.zip to finish", |h| completed(h) == 1);
    h.select("file.zip");
    let path = std::path::PathBuf::from(h.row("file.zip").path);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dm-core/tests/fixtures/archives/zip/aes256.zip");
    std::fs::copy(&fixture, &path).unwrap();
    let home = dm_shell::settings::home();
    dm_core::extract::passwords::save(&home, &["not this one".to_string()]).unwrap();
    h.run(Command::ExtractHere);
    h.until("the password to be asked for", |h| {
        h.dialog_open("Archive password")
    });
    h.fill("Password", "wrong");
    h.click("Extract");
    h.until_shown("That password does not open it.");
    assert!(
        h.dialog_open("Archive password"),
        "a wrong password closed the dialog"
    );
    h.fill("Password", "dm secret");
    h.click("Keep this password");
    h.click("Extract");
    h.until("the extraction", |h| {
        h.said().starts_with("Extracted 4 files")
    });
    let first = dm_core::extract::passwords::load(&home).first().cloned();
    dm_core::extract::passwords::save(&home, &[]).unwrap();
    assert!(!h.dialog_open("Archive password"), "the dialog stayed open");
    assert_eq!(
        std::fs::read(path.with_extension("").join("docs/a.txt")).unwrap(),
        b"hello, zip\n"
    );
    assert_eq!(
        first.as_deref(),
        Some("dm secret"),
        "the password was not kept"
    );
}
