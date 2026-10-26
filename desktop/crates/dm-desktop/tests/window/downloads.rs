use crate::support::*;
use crate::test_secrets;

#[test]
fn add_url_downloads_the_file_announces_it_once_and_says_it_is_complete() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;

    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);

    let row = harness.state().rows()[0].clone();
    assert_eq!(row.filename, "file.zip");
    assert_eq!(
        std::fs::read(&row.path).unwrap(),
        payload(),
        "the saved bytes are not the served ones"
    );
    assert_eq!(
        *world.fake.notified.lock().unwrap(),
        ["file.zip"],
        "announced not exactly once"
    );

    assert!(harness.query_by_label("Download complete").is_some());
    click(&mut harness, "Open");
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );
    assert!(harness.query_by_label("Download complete").is_none());
}

#[test]
fn a_newer_release_offers_its_installer_as_a_download_in_dm() {
    let world = world();
    let mut harness = world.harness();
    // The local server's files stand in for the release's installer.
    let sample = world.dir.path().join("sample");
    std::fs::write(&sample, payload()).unwrap();
    let sha256 = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(dm_core::checksum::digest(
            &sample,
            dm_core::checksum::Algorithm::Sha256,
        ))
        .unwrap();
    let offer = |harness: &mut Harness<'_, DmApp>, url: String, sha256: String| {
        *harness.state().update.lock().unwrap() = dm_desktop::update::Update::Newer(
            "9.9.9".into(),
            Some(dm_desktop::update::Installer { url, sha256 }),
        );
        harness.state_mut().dialogs.about_open = true;
        harness.run_steps(2);
        click(harness, "Download the update");
    };
    let installer = world.url("file.zip");
    offer(&mut harness, installer.clone(), sha256);
    until(&mut harness, "the installer to be fetched", |app| {
        app.rows()
            .iter()
            .any(|r| r.url == installer && r.state == State::Completed && r.verified)
    });
    // Whole and checked: offered for installing, dm closing for it.
    harness.state_mut().dialogs.about_open = false;
    harness.state_mut().refresh();
    harness.run_steps(3);
    assert!(harness
        .query_by_label("The dm update is downloaded and checked.")
        .is_some());
    let path = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.url == installer)
        .unwrap()
        .path
        .clone();
    if cfg!(target_os = "linux") {
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Open folder")
            .click();
        harness.run_steps(3);
    } else {
        click(&mut harness, "Install now");
        until(&mut harness, "the installer to be opened", |_| {
            world.fake.opened.lock().unwrap().contains(&path)
        });
        assert!(
            harness.state().quit_asked,
            "dm stayed open over its installer"
        );
    }
    assert!(harness.state().update_row.is_none());
    harness.state_mut().quit_asked = false;
    // One that does not match what the release published is not kept.
    let other = world.url("other.pdf");
    offer(&mut harness, other.clone(), "0".repeat(64));
    until(&mut harness, "the wrong installer to fail", |app| {
        app.rows()
            .iter()
            .any(|r| r.url == other && matches!(r.state, State::Failed(_)))
    });
    let row = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.url == other)
        .cloned()
        .unwrap();
    assert!(row.path.is_empty() || !std::path::Path::new(&row.path).exists());
    let out = harness.state().applied.download_dir.clone();
    let left: Vec<_> = walk(&out)
        .into_iter()
        .filter(|p| p.ends_with("other.pdf"))
        .collect();
    assert!(
        left.is_empty(),
        "the mismatched installer was kept: {left:?}"
    );
}

#[test]
fn a_finished_zip_is_extracted_beside_itself_and_a_hostile_one_is_refused() {
    use std::io::Write;
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "file.zip to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    select(&mut harness, "file.zip");
    let path = std::path::PathBuf::from(row(&harness, "file.zip").path);
    assert!(harness.state().enabled(Command::ExtractHere));

    // The served bytes are not a ZIP: said so, and nothing is made.
    harness.state_mut().run(Command::ExtractHere);
    until(&mut harness, "the refusal", |app| {
        app.said.english().starts_with("Not extracted")
    });

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
    harness.state_mut().run(Command::ExtractHere);
    until(&mut harness, "the hostile archive's refusal", |app| {
        app.said.english().contains("leave its folder")
    });
    let folder = path.with_extension("");
    assert!(!folder.exists(), "a refused archive leaves no folder");
    assert!(!path.parent().unwrap().join("escape.txt").exists());

    write(&["a.txt", "sub/b.txt"]);
    harness.state_mut().run(Command::ExtractHere);
    until(&mut harness, "the extraction", |app| {
        app.said.english().starts_with("Extracted 2 files")
    });
    assert_eq!(std::fs::read(folder.join("sub/b.txt")).unwrap(), b"inside");
    assert!(path.exists(), "the archive is kept");

    // Extract to…: into the folder the person picks, a new folder inside it.
    let elsewhere = tempfile::tempdir().unwrap();
    *world.fake.chosen.lock().unwrap() = Some(elsewhere.path().to_path_buf());
    assert!(harness.state().enabled(Command::ExtractTo));
    harness.state_mut().run(Command::ExtractTo);
    until(&mut harness, "the extraction elsewhere", |app| {
        app.said.english().starts_with("Extracted 2 files")
    });
    let name = path.file_stem().unwrap();
    assert_eq!(
        std::fs::read(elsewhere.path().join(name).join("a.txt")).unwrap(),
        b"inside"
    );
    // Cancelling the chooser extracts nothing.
    *world.fake.chosen.lock().unwrap() = None;
    harness.state_mut().said = "".into();
    harness.state_mut().run(Command::ExtractTo);
    harness.run_steps(3);
    assert!(!harness.state().said.english().starts_with("Extract"));
}

#[test]
fn a_video_half_fetched_is_previewed_from_what_is_on_disk() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("half.mp4"), "Start now");
    until(&mut harness, "the first bytes to arrive", |app| {
        app.rows()
            .iter()
            .any(|r| r.progress.downloaded >= 64 * 1024)
    });
    select(&mut harness, "half.mp4");
    assert!(!harness.state().enabled(Command::OpenFile), "not finished");
    assert!(harness.state().enabled(Command::Preview));
    harness.state_mut().run(Command::Preview);
    let opened = || {
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.ends_with("preview.mp4"))
            .cloned()
    };
    until(&mut harness, "the preview to be opened", |_| {
        opened().is_some()
    });
    let preview = opened().unwrap();
    let bytes = std::fs::read(&preview).unwrap();
    // From the start, all of it but what is still in the engine's write
    // buffer (the staged file is as long as the whole, zeros past that).
    assert!(bytes.len() >= 32 * 1024, "{} bytes", bytes.len());
    assert!(bytes[..32 * 1024].iter().all(|b| *b == 9));

    // A file no player shows is not offered.
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    select(&mut harness, "stalled.bin");
    assert!(!harness.state().enabled(Command::Preview));

    // Removing the row takes the preview with its parts.
    let row = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.url.ends_with("half.mp4"))
        .unwrap()
        .id;
    harness.state().backend().remove(row);
    until(&mut harness, "the preview to go with its row", |_| {
        !std::path::Path::new(&preview).exists()
    });
}

#[test]
fn a_finished_file_deleted_since_says_so_and_is_not_offered_to_open() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    select(&mut harness, "file.zip");
    assert!(harness.state().enabled(Command::OpenFile));

    std::fs::remove_file(&row.path).unwrap();
    harness.run_steps(3);
    assert!(harness.query_by_label("File missing").is_some());
    assert!(!harness.state().enabled(Command::OpenFile));
    assert!(!harness.state().enabled(Command::OpenWith));
    assert!(
        harness.state().enabled(Command::OpenFolder),
        "Open folder still answers: it says the file is gone"
    );
    assert!(harness.state().enabled(Command::Redownload));
}

/// A download caught from the browser opens Download details where
/// Options says so, and starts at once where it does not, whatever the
/// panel's own setting.
#[test]
fn a_caught_download_asks_where_to_save_by_its_own_setting() {
    let world = world();
    let mut harness = world.harness();
    let caught = |harness: &mut Harness<'_, DmApp>, name: &str| {
        harness
            .state()
            .backend()
            .handed
            .lock()
            .unwrap()
            .push_back(dm_shell::host::Request {
                url: world.url(name),
                referrer: "https://e.com/page".into(),
                ask: true,
                caught: true,
                ..Default::default()
            });
        harness.state_mut().take_handed();
        harness.run_steps(3);
    };
    harness.state_mut().applied.ask_panel_downloads = false;
    harness.state_mut().applied.ask_caught_downloads = true;
    caught(&mut harness, "file.zip");
    assert!(harness.query_by_label("Download details").is_some());
    assert!(harness.state().rows().is_empty(), "started before asking");
    click(&mut harness, "Start now");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 1);

    harness.state_mut().applied.ask_panel_downloads = true;
    harness.state_mut().applied.ask_caught_downloads = false;
    caught(&mut harness, "other.pdf");
    assert!(harness.state().dialogs.file_info.is_none());
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 2, "not started at once");
}

/// A download's progress window limits that download alone: ticked, it
/// takes the KB/s shown; unticked, the limit goes.
#[test]
fn the_progress_window_limits_its_own_download() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    harness.state_mut().refresh();
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    harness.run_steps(3);
    click(&mut harness, "Limit this download to");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows()[0].speed_limit, 500);
    harness.run_steps(2);
    click(&mut harness, "Limit this download to");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows()[0].speed_limit, 0);
}

/// View › Drop target puts the floating drop target up, over every other
/// window, and takes it away; the choice is kept in the settings.
#[test]
fn view_drop_target_shows_and_hides_the_floating_target() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    press(&mut harness, &["View", "Drop target"]);
    harness.run_steps(4);
    assert!(harness.state().applied.drop_target);
    assert!(harness.state().backend().settings().drop_target, "not kept");
    assert!(
        harness
            .query_by_label_contains("Drop a link here to download it with dm")
            .is_some()
            || harness
                .query_by_label_contains(dm_desktop::drops::DROP_TARGET_TITLE)
                .is_some(),
        "no drop target drawn"
    );
    press(&mut harness, &["View", "Drop target"]);
    harness.run_steps(4);
    assert!(!harness.state().applied.drop_target);
}

#[test]
fn open_with_asks_for_an_application_and_opens_the_file_with_it() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    select(&mut harness, "file.zip");
    assert!(harness.state().enabled(Command::OpenWith));

    // Cancelled: nothing is opened.
    harness.state_mut().run(Command::OpenWith);
    assert!(world.fake.opened.lock().unwrap().is_empty());

    let app = std::path::PathBuf::from("/Applications/Archive Utility.app");
    *world.fake.chosen.lock().unwrap() = Some(app.clone());
    harness.get_by_label("file.zip").click_secondary();
    harness.run_steps(2);
    click(&mut harness, "Open with…");
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        [format!("{} with {}", row.path, app.display())]
    );

    // With no chooser to pick with, it says so rather than doing nothing.
    *world.fake.no_chooser.lock().unwrap() = true;
    harness.state_mut().run(Command::OpenWith);
    harness.run_steps(2);
    assert!(harness
        .state()
        .said
        .english()
        .starts_with("No file chooser on this desktop to pick an application"));
    assert_eq!(world.fake.opened.lock().unwrap().len(), 1);
    *world.fake.no_chooser.lock().unwrap() = false;
}

#[test]
fn download_later_waits_in_the_main_queue_until_start_queue() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    let row = harness.state().rows()[0].clone();
    assert_eq!(row.state, State::Paused);
    assert_eq!(row.queue, dm_core::MAIN_QUEUE);

    // The Queue column names the row's queue, as the tree does.
    harness.run_steps(2);
    assert!(
        harness.query_by_label("Main queue").is_some(),
        "the Queue column does not name the queue"
    );
    harness.get_by_label_contains("Main queue (").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().place,
        Place::Queue(dm_core::MAIN_QUEUE.into())
    );
    click(&mut harness, "Start queue");
    until(&mut harness, "the queue's download to complete", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
}

/// Move/Rename: a name taken in that folder is said, with the name given
/// instead; Cancel closes it untouched.
#[test]
fn renaming_onto_a_taken_name_says_what_it_is_called() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "both to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    let pdf = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.filename == "other.pdf")
        .unwrap()
        .id;
    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![pdf],
        name: "other.pdf".into(),
        ..Default::default()
    });
    harness.run_steps(2);
    click(&mut harness, "Cancel");
    assert!(
        harness.state().dialogs.rename.is_none(),
        "Cancel did not close it"
    );
    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![pdf],
        name: "file.zip".into(),
        ..Default::default()
    });
    harness.run_steps(2);
    click(&mut harness, "OK");
    let given = harness.state().row(pdf).unwrap().filename.clone();
    assert_ne!(given, "file.zip", "the other file's name was taken over");
    assert_eq!(
        harness.state().said.english(),
        format!("file.zip is taken in that folder, so it is called {given}.")
    );
    // And a screen reader reads it out as it appears.
    harness.run_steps(2);
    let said = harness
        .query_by_label_contains("is taken in that folder")
        .expect("the sentence is not in the status bar");
    assert_eq!(
        said.accesskit_node().live(),
        egui::accesskit::Live::Polite,
        "the status bar's sentence is not a live region"
    );
}

/// "Download all links" offers a link already in the list unticked, and
/// says so: it was fetched again as `name (1).zip` without a word.
#[test]
fn a_pages_link_already_listed_is_offered_unticked_and_said() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    let request = dm_shell::host::Request {
        url: world.url("page.html"),
        referrer: world.url("page.html"),
        links: vec![world.url("file.zip"), world.url("other.pdf")],
        ask: true,
        ..Default::default()
    };
    let app = harness.state_mut();
    let backend = app.backend.clone();
    app.dialogs.open_links(request, &backend);
    harness.run_steps(3);
    let ticks: Vec<bool> = harness
        .state()
        .dialogs
        .links
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.1)
        .collect();
    assert_eq!(ticks, [false, true]);
    assert!(harness
        .query_by_label_contains("(already in the list)")
        .is_some());
}

#[test]
fn delete_waits_for_a_download_to_stop() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "the row to start", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    select(&mut harness, "stalled.bin");
    assert!(
        !harness.state().enabled(Command::Remove),
        "Delete is offered for a download still running"
    );
    harness.state_mut().run(Command::Remove);
    assert_eq!(
        harness.state().rows().len(),
        1,
        "a running download was deleted"
    );
}

/// Copy link puts the row's address on the clipboard, without the sign-in
/// that is the row's own; the row menu offers it.
#[test]
fn copy_link_copies_the_address_without_its_sign_in() {
    let password = test_secrets::secret("password");
    let world = world();
    let mut harness = world.harness();
    let plain = world.url("private.bin");
    harness
        .state()
        .backend()
        .add_with(
            &plain.replace("http://", &format!("http://me:{password}@")),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    select(&mut harness, "private.bin");
    assert!(harness.state().enabled(Command::CopyLink));
    harness.state_mut().run(Command::CopyLink);
    harness.run_steps(1);
    let copied: Vec<String> = harness
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(copied, [plain], "copied {copied:?}");
    assert!(
        !copied.concat().contains(&password),
        "the sign-in went with it"
    );
}

#[test]
fn a_batch_adds_every_address_the_star_makes() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    type_into(&mut harness, "Address with *", &world.url("part*.bin"));
    type_into(&mut harness, "From", "1");
    type_into(&mut harness, "To", "12");
    assert!(harness.query_by_label_contains("12 addresses").is_some());
    // Each address is listed, not only the first.
    for n in ["part1.bin", "part2.bin", "part5.bin"] {
        assert!(
            harness.query_by_label(&world.url(n)).is_some(),
            "{n} is not in the preview"
        );
    }
    click(&mut harness, "Add for later");
    let urls: Vec<String> = harness
        .state()
        .rows()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(urls.len(), 12);
    assert!(urls[0].ends_with("/part1.bin") && urls[11].ends_with("/part12.bin"));
}

#[test]
fn a_failed_download_plays_the_failure_sound_when_asked() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let mut wanted = harness.state().backend().settings();
    wanted.sound_on_failure = true;
    harness.state_mut().apply_settings(wanted).unwrap();
    // Nothing is served at this name: a 404, failed and not retried.
    add_url(&mut harness, &world.url("missing.bin"), "Start now");
    until(&mut harness, "the row failed", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(3);
    let opened = world.fake.opened.lock().unwrap().clone();
    assert!(opened.iter().any(|o| o == "alert"), "{opened:?}");
    assert!(!opened.iter().any(|o| o == "chime"), "{opened:?}");
    // Once: a redraw does not sound it again.
    harness.run_steps(5);
    assert_eq!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|o| *o == "alert")
            .count(),
        1
    );
    let mut wanted = harness.state().backend().settings();
    wanted.sound_on_failure = false;
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// A double-click on a link in the browser hands it over twice in a moment:
/// one row, not `name (1)` and `name (2)`.
#[test]
fn the_same_address_handed_twice_at_once_is_taken_once() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_panel_downloads = false;
    for _ in 0..2 {
        harness
            .state()
            .backend()
            .handed
            .lock()
            .unwrap()
            .push_back(dm_shell::host::Request {
                url: world.url("file.zip"),
                ..Default::default()
            });
        harness.state_mut().take_handed();
    }
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 1);
}

/// Under the Remove question the list takes no clicks: a row selected
/// there was not the one removed. The question names its row.
#[test]
fn the_list_behind_the_remove_question_takes_no_clicks() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    harness.state_mut().refresh();
    select(&mut harness, "file.zip");
    let chosen = harness.state().selected.clone();
    harness.state_mut().run(Command::Remove);
    harness.run_steps(3);
    assert!(harness.state().dialogs.confirm_remove.is_some());
    assert!(
        harness.query_all_by_label("file.zip").count() >= 2,
        "the question names no row"
    );
    harness.get_by_label("other.pdf").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().selected,
        chosen,
        "a row was selected under the question"
    );
}

/// Remove can take a finished file off the disk with its row, when asked;
/// left alone, the file stays.
#[test]
fn remove_deletes_the_file_too_only_when_asked() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "both to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    let path_of = |h: &Harness<'_, DmApp>, name: &str| {
        h.state()
            .rows()
            .iter()
            .find(|r| r.filename == name)
            .unwrap()
            .path
            .clone()
    };
    let (zip, pdf) = (
        path_of(&harness, "file.zip"),
        path_of(&harness, "other.pdf"),
    );
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::Remove);
    harness.run_steps(3);
    click(&mut harness, "Remove it");
    harness.state_mut().refresh();
    assert!(
        std::path::Path::new(&zip).is_file(),
        "the file went without being asked"
    );
    select(&mut harness, "other.pdf");
    harness.state_mut().run(Command::Remove);
    harness.run_steps(3);
    click(&mut harness, "Also delete the file");
    click(&mut harness, "Yes, delete");
    harness.state_mut().refresh();
    assert!(!std::path::Path::new(&pdf).exists(), "the file stayed");
    assert!(harness.state().rows().is_empty());
}

/// Add URL remembers the addresses added through it, never with a login,
/// offers them again, and forgets them on Clear history.
#[test]
fn add_url_remembers_addresses_without_their_login_and_clears_them() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "Needs a sign-in");
    type_into(&mut harness, "Login", "me");
    click(&mut harness, "OK");
    assert_eq!(harness.state().applied.recent_urls, [world.url("file.zip")]);
    harness.state_mut().dialogs.file_info = None;
    harness.run_steps(2);
    click(&mut harness, "Add URL");
    assert!(harness.query_by_label("Recent").is_some());
    click(&mut harness, "Clear history");
    assert!(harness.state().applied.recent_urls.is_empty());
}

/// Options › General saves the settings to a file without any password,
/// and loads them back into the dialog, to be used on OK.
#[test]
fn settings_are_saved_to_a_file_without_passwords_and_loaded_back() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Options);
    harness.run_steps(3);
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.connections = 5;
        d.draft.proxy.username = "someone".into();
        d.draft.proxy.password = "not-in-the-file".into();
    }
    let file = world.dir.path().join("dm-settings.json");
    *world.fake.chosen.lock().unwrap() = Some(file.clone());

    // By the keyboard, as a person reaches a button below the fold: the
    // focus brings it into view, and Space presses it.
    let press_by_keyboard = |harness: &mut Harness<'_, DmApp>, name: &str| {
        harness.get_by_label(name).focus();
        harness.run_steps(3);
        harness.key_press(egui::Key::Space);
        harness.run_steps(3);
    };
    press_by_keyboard(&mut harness, "Save settings to a file…");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"connections\": 5"), "{text}");
    assert!(
        !text.contains("not-in-the-file"),
        "a password went into the file"
    );
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.connections = 2;
    }
    press_by_keyboard(&mut harness, "Load settings from a file…");
    let d = harness.state().dialogs.options.as_ref().unwrap();
    assert_eq!(d.draft.connections, 5);
    assert_eq!(
        d.draft.proxy.password, "not-in-the-file",
        "this computer's password was lost"
    );
    *world.fake.chosen.lock().unwrap() = None;
}

/// The progress window's Cancel download asks first; Keep downloading
/// (and Enter) leaves it running.
#[test]
fn cancel_download_in_the_progress_window_asks_first() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("stalled.bin"))
        .unwrap();
    until(&mut harness, "it to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    harness.run_steps(3);
    click(&mut harness, "Cancel download");
    assert!(
        harness.query_by_label("Cancel this download?").is_some(),
        "nothing asked"
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_ne!(
        harness.state().rows()[0].state,
        State::Cancelled,
        "Enter cancelled it"
    );
    click(&mut harness, "Cancel download");
    click(&mut harness, "Yes, cancel");
    until(&mut harness, "it to be cancelled", |app| {
        app.rows()[0].state == State::Cancelled
    });
}

#[test]
fn a_progress_window_shows_each_connection() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_progress = true;
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    // Running comes a moment before the plan is written; until then the
    // window says "Unknown until the server answers", which is right.
    until(&mut harness, "the download to be planned", |app| {
        app.rows()
            .iter()
            .any(|r| r.state == State::Running && app.backend().resumable(r.id).is_some())
    });
    harness.run_steps(3);
    assert!(harness
        .query_by_label_contains("Download progress")
        .is_some());
    assert!(harness.query_by_label("Can resume").is_some());
    // That server does not serve ranges, and the window says so rather than
    // guessing "Yes" from a known size.
    assert!(harness
        .query_by_label_contains("No — a stop starts it again")
        .is_some());
    assert!(harness.query_by_label("Connections").is_some());
    click(&mut harness, "Stop download");
    assert_eq!(harness.state().rows()[0].state, State::Paused);
}

#[cfg(unix)]
#[test]
fn a_finished_download_is_handed_to_the_command_set_to_run_after() {
    /* A command run on each finished file, its path added as one argument,
     * whatever characters the name has. */
    use std::os::unix::fs::PermissionsExt;
    let world = world();
    let scratch = tempfile::tempdir().unwrap();
    let record = scratch.path().join("checked");
    let script = scratch.path().join("checker with space");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s|%s' \"$1\" \"$2\" > '{}'\n",
            record.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut harness = world.harness();
    harness.state_mut().applied.on_complete = format!("'{}' --scan", script.display());
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let path = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.state == State::Completed)
        .map(|r| r.path.clone())
        .unwrap();
    let ran = (0..100).find_map(|_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::read_to_string(&record).ok()
    });
    assert_eq!(
        ran.as_deref(),
        Some(format!("--scan|{path}").as_str()),
        "the command was not run on the file"
    );
}

/* ------------------------------------------------- every click, checked */

/// Several downloads finishing make one "Download complete" window
/// listing each, with its own Open and Open folder.
#[test]
fn downloads_finishing_together_share_one_complete_window() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    harness.run_steps(2);
    let urls = [
        world.url("file.zip"),
        world.url("other.pdf"),
        world.url("deep.pdf"),
    ];
    harness.state_mut().add(&urls, Default::default()).unwrap();
    until(&mut harness, "all three to finish", |app| {
        app.rows().len() == 3 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    assert_eq!(harness.query_all_by_label("Download complete").count(), 1);
    assert!(harness.query_by_label("3 downloads finished.").is_some());
    click(&mut harness, "Open other.pdf");
    let other = row(&harness, "other.pdf");
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        std::slice::from_ref(&other.path),
        "rows: {:?}",
        harness
            .state()
            .rows()
            .iter()
            .map(|r| (r.url.clone(), r.path.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(harness.state().dialogs.complete.len(), 2, "the rest stay");
    click(&mut harness, "Open the folder of deep.pdf");
    assert!(world.fake.opened.lock().unwrap()[1].starts_with("reveal "));
    click(&mut harness, "Close");
    assert!(harness.query_by_label("Download complete").is_none());
}

/// A server that wants a sign-in says where to give one, and
/// Properties opens on its Login.
#[test]
fn a_refused_sign_in_says_where_to_give_one() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let failed = harness.state().rows()[0].clone();
    let said = dm_desktop::app::status_of(&failed);
    // In plain words, saying where a sign-in goes, without the address or
    // the server's page.
    assert!(said.contains("Properties › Login"), "{said}");
    assert!(!said.contains("http"), "{said}");
    select(&mut harness, "secret.zip");
    // Retry, not Resume, on the toolbar, in the File menu and the row menu.
    assert!(harness.query_by_label("Retry").is_some());
    assert_eq!(harness.state().title_of(Command::DownloadNow), "Retry");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(3);
    harness.event(egui::Event::Text("user".into()));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .properties
            .get(&failed.id)
            .map(|d| d.login.clone()),
        Some("user".into()),
        "the keyboard is not in Login"
    );
}

/// Save as takes the name the server gives, and leaves one a
/// person typed alone.
#[test]
fn save_as_takes_the_servers_name() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("named")]);
    until(&mut harness, "the server's name", |app| {
        app.dialogs
            .file_info
            .as_ref()
            .is_some_and(|d| d.name == "report-2026.pdf")
    });
    // Laid out once it is open, before it is clicked.
    harness.run_steps(3);
    click(&mut harness, "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    assert!(harness.state().rows()[0].path.ends_with("report-2026.pdf"));

    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("named")]);
    harness.run_steps(1);
    if let Some(d) = harness.state_mut().dialogs.file_info.as_mut() {
        d.name = String::new();
        d.name_typed = true;
    }
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        harness
            .state()
            .dialogs
            .file_info
            .as_ref()
            .map(|d| d.name.as_str()),
        Some(""),
        "what a person typed was replaced"
    );
}

/// The limiter's field takes what is typed in place of what it
/// held; the toolbar's buttons stay put when Resume becomes Retry; the
/// progress window keeps its place and its percentage in view, and says
/// what a refusal leaves unknown; the empty list's words are under the
/// headings.
#[test]
fn the_small_things_of_round_eight() {
    let _file = settings_file();
    let world = world();
    // Under the headings, with nothing in the list.
    {
        let mut harness = world.harness();
        harness.run_steps(3);
        let heading = harness
            .get_by_label("Name")
            .accesskit_node()
            .bounding_box()
            .unwrap();
        let words = harness
            .get_by_label("Nothing queued")
            .accesskit_node()
            .bounding_box()
            .unwrap();
        assert!(
            words.y0 > heading.y1,
            "the empty list's words are above its headings"
        );
    }

    // The limiter: 500 typed over 64 is 500, not 64500.
    let mut harness = world.harness();
    let mut wanted = harness.state().applied.clone();
    wanted.speed_limit = 64;
    harness.state_mut().apply_settings(wanted).unwrap();
    harness.state_mut().run(Command::SpeedLimiterSettings);
    harness.run_steps(4);
    harness.event(egui::Event::Text("500".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert_eq!(harness.state().applied.speed_limit, 500);
    let mut off = harness.state().applied.clone();
    off.speed_limit = 0;
    harness.state_mut().apply_settings(off).unwrap();

    // The toolbar: one finished row, one refused.
    let mut wanted = harness.state().applied.clone();
    wanted.show_progress = true;
    harness.state_mut().apply_settings(wanted).unwrap();
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    let progress = |h: &Harness<'_, DmApp>| {
        let id = h.state().rows()[0].id;
        h.ctx
            .memory(|m| m.area_rect(egui::Id::unique(("progress", id))))
    };
    harness.run_steps(3);
    let before = progress(&harness).expect("the progress window is open");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(4);
    let after = progress(&harness).expect("the progress window is open");
    assert!(
        (after.min - before.min).length() < 1.0,
        "the progress window moved from {before:?} to {after:?}"
    );
    // The server answered, with a refusal: said as one.
    assert!(harness
        .query_by_label("No — the server refused it")
        .is_some());
    // The bar no longer says the percentage beside itself (Status above says it); no words of its own to be cut off.
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.remove(&id);
    let mut quiet = harness.state().applied.clone();
    quiet.show_progress = false;
    harness.state_mut().apply_settings(quiet).unwrap();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let delete_at = |h: &Harness<'_, DmApp>| {
        h.get_by_label("Remove")
            .accesskit_node()
            .bounding_box()
            .unwrap()
            .x0
    };
    select(&mut harness, "file.zip");
    harness.run_steps(2);
    assert!(harness.query_by_label("Resume").is_some());
    let resume = delete_at(&harness);
    select(&mut harness, "secret.zip");
    harness.run_steps(2);
    assert!(harness.query_by_label("Retry").is_some());
    let retry = delete_at(&harness);
    assert_eq!(resume, retry, "Delete moved when Resume became Retry");
}

/// The link Add URL opens with is selected, so a link pasted over
/// it replaces it; two addresses run together are refused with a reason.
#[test]
fn add_url_selects_its_link_and_refuses_two_run_together() {
    let world = world();
    let mut harness = world.harness();
    let first = world.url("file.zip");
    let second = world.url("other.pdf");
    harness
        .state_mut()
        .dialogs
        .open_add_url(Some(first.clone()));
    harness.run_steps(4);
    harness.event(egui::Event::Text(second.clone()));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .add_url
            .as_ref()
            .map(|d| d.address.clone()),
        Some(second.clone()),
        "the pasted link joined the one already there"
    );
    if let Some(d) = harness.state_mut().dialogs.add_url.as_mut() {
        d.address = format!("{first}{second}");
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("looks like two links")
        .is_some());
    assert!(harness.query_by_label("Download details").is_none());
    assert!(harness.state().rows().is_empty());
    // The address keeps the keyboard: what is typed next replaces it.
    harness.run_steps(3);
    harness.event(egui::Event::Text(second.clone()));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .add_url
            .as_ref()
            .map(|d| d.address.clone()),
        Some(second),
        "typing after the warning went nowhere"
    );
}

/// A drop with no link in it says how links are added, rather
/// than nothing; the empty list says so too.
#[test]
fn a_drop_with_no_link_says_how_to_add_one() {
    let world = world();
    let mut harness = world.harness();
    assert!(harness
        .query_by_label_contains(dm_desktop::app::drop_hint())
        .is_some());
    let file = world.dir.path().join("picture.png");
    std::fs::write(&file, [0u8, 1, 2, 3]).unwrap();
    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(Dropped(file)));
    harness.run_steps(3);
    assert!(harness.state().rows().is_empty());
    assert!(
        harness
            .query_all_by_label_contains(dm_desktop::app::drop_hint())
            .count()
            >= 2,
        "the drop was ignored without a word"
    );
}

/// A link that runs out part-way asks for a fresh one at once, and the
/// fresh one goes on from the parts already fetched.
#[test]
fn a_link_that_runs_out_part_way_asks_for_a_fresh_one() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(
            |ui, app: &mut DmApp| app.ui(ui),
            world.app_with(Settings {
                download_dir: world.dir.path().join("out"),
                connections: 1,
                attempts: 3,
                retry_wait: 0,
                record_minutes: 0,
                show_progress: false,
                show_complete: false,
                language: "en".into(),
                ..Settings::default()
            }),
        );
    harness
        .state()
        .backend()
        .add(&world.url("expires.bin"))
        .unwrap();
    until(&mut harness, "the expired link to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let row = harness.state().rows()[0].clone();
    assert!(dm_desktop::app::link_expired(&row), "{:?}", row.state);
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Update download link").is_some(),
        "no fresh link asked for"
    );
    // The fresh link to the same file: it goes on, and finishes whole.
    type_into(&mut harness, "New address", &world.url("renewed.bin"));
    click(&mut harness, "OK");
    assert!(harness.state().dialogs.update_link.is_none());
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    let bytes = std::fs::read(&row.path).unwrap();
    assert_eq!(bytes.len(), 1_000_000);
    assert!(bytes.iter().all(|b| *b == 7));
}

/// A download whose folder cannot be reached (as when its drive is taken
/// out) is paused with why, not failed; the bar over the list offers
/// another folder, and there it finishes, its parts carried over.
#[test]
fn an_unreachable_folder_pauses_and_another_folder_finishes_it() {
    let world = world();
    let blocker = world.dir.path().join("not-a-drive");
    std::fs::write(&blocker, b"a file where the folder should be").unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(
            |ui, app: &mut DmApp| app.ui(ui),
            world.app_with(Settings {
                download_dir: blocker.join("out"),
                show_progress: false,
                show_complete: false,
                language: "en".into(),
                ..Settings::default()
            }),
        );
    harness
        .state()
        .backend()
        .add(&world.url("file.zip"))
        .unwrap();
    until(&mut harness, "the download to pause by itself", |app| {
        app.rows()
            .iter()
            .any(|r| r.state == State::Paused && !r.stalled.is_empty())
    });
    let row = harness.state().rows()[0].clone();
    assert!(
        dm_desktop::app::status_of(&row).starts_with("Paused: "),
        "{}",
        dm_desktop::app::status_of(&row)
    );
    harness.run_steps(3);
    let elsewhere = world.dir.path().join("elsewhere");
    *world.fake.chosen.lock().unwrap() = Some(elsewhere.clone());
    click(&mut harness, "Choose another folder…");
    until(&mut harness, "the download to finish elsewhere", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    assert!(
        std::path::Path::new(&row.path).starts_with(&elsewhere),
        "{}",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap(), payload());
}

/// A failed row's status is a plain reason, in Properties held to
/// its column; the engine's words are kept for hover.
#[test]
fn a_cut_connection_is_said_plainly() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(
            |ui, app: &mut DmApp| app.ui(ui),
            world.app_with(Settings {
                download_dir: world.dir.path().join("out"),
                connections: 1,
                attempts: 1,
                show_progress: false,
                show_complete: false,
                language: "en".into(),
                ..Settings::default()
            }),
        );
    harness
        .state()
        .backend()
        .add(&world.url("drops.bin"))
        .unwrap();
    until(&mut harness, "the cut download to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let row = harness.state().rows()[0].clone();
    let status = dm_desktop::app::status_of(&row);
    assert_eq!(
        status, "Error: the server ended the connection before the file was complete",
        "{:?}",
        row.state
    );
    assert!(dm_desktop::app::failure_as_said(&row).is_some());
    harness.state_mut().selected = [row.id].into();
    harness.state_mut().queued.push(Command::Properties);
    harness.run_steps(4);
    // In the list's row and in Properties.
    for shown in harness.get_all_by_label(&status) {
        let rect = shown.rect();
        assert!(rect.height() < 60.0, "the status stands {rect:?} tall");
    }
}

/// While a range waits to be tried again, the row says which
/// attempt it is on.
#[test]
fn a_retry_says_its_attempt() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(
            |ui, app: &mut DmApp| app.ui(ui),
            world.app_with(Settings {
                download_dir: world.dir.path().join("out"),
                connections: 1,
                attempts: 5,
                show_progress: false,
                show_complete: false,
                language: "en".into(),
                ..Settings::default()
            }),
        );
    harness
        .state()
        .backend()
        .add(&world.url("drops.bin"))
        .unwrap();
    until(&mut harness, "the row to say it retries", |app| {
        app.rows()
            .first()
            .is_some_and(|r| dm_desktop::app::status_of(r) == "Connection dropped (retry 2 of 5)")
    });
    // A new address is taken while it retries, and finishes it (the field was greyed as for a running download).
    let id = harness.state().rows()[0].id;
    harness.state_mut().selected = [id].into();
    assert!(harness.state().enabled(Command::UpdateLink));
    let good = world.url("file.zip");
    harness.state().backend().set_url(id, &good).unwrap();
    until(&mut harness, "the new address to finish it", |app| {
        app.rows()
            .first()
            .is_some_and(|r| r.state == State::Completed && r.url == good)
    });
}

/// A hand-off of an address already listed is added, and said.
#[test]
fn a_hand_off_of_a_listed_address_is_said_to_be_added_again() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    harness.run_steps(2);
    // As the inbox adds a browser's hand-off: to the queue, not by the
    // window.
    harness
        .state()
        .backend()
        .add_with(&world.url("file.zip"), dm_core::AddOptions::default())
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    assert_eq!(
        harness.state().said.english(),
        "Added file.zip again: that address was already in the list."
    );
}

/// A file added again, while its first copy is in the folder, is
/// listed under the name it will be placed under, not the one it will not.
#[test]
fn a_file_added_again_is_listed_under_the_name_it_will_get() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the first copy to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    assert!(
        harness.query_by_label("file (1).zip").is_some(),
        "the second copy is not listed as file (1).zip"
    );
    // And the control protocol says so too.
    let seen = harness
        .state_mut()
        .control_call("ui.inspect", &serde_json::json!({}))
        .unwrap();
    let names: Vec<&str> = seen["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert!(names.contains(&"file (1).zip"), "{names:?}");
    // And it is placed there once fetched.
    click(&mut harness, "Start queue");
    until(&mut harness, "the second copy to finish", |app| {
        app.rows()
            .iter()
            .filter(|r| r.state == State::Completed)
            .count()
            == 2
    });
    assert!(harness
        .state()
        .rows()
        .iter()
        .any(|r| r.path.ends_with("file (1).zip")));
}

/// Copies of one file added for later are each named as they
/// will be saved, not all "other (1).pdf".
#[test]
fn copies_waiting_in_the_list_each_get_their_own_name() {
    let world = world();
    let mut harness = world.harness();
    for _ in 0..3 {
        add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    }
    harness.run_steps(3);
    for name in ["other.pdf", "other (1).pdf", "other (2).pdf"] {
        assert!(
            harness.query_by_label(name).is_some(),
            "no row reads {name}"
        );
    }
}

/// A download refused for want of a sign-in is retried with
/// one saved in Options afterwards, as a fresh copy of the address was.
#[test]
fn retry_uses_a_sign_in_saved_since_it_failed() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("locked.zip"))
        .unwrap();
    until(&mut harness, "it to be refused", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    let mut wanted = harness.state().backend().settings();
    wanted.site_logins = vec![dm_shell::settings::SiteLogin {
        host: "127.0.0.1".into(),
        username: "me".into(),
        password: test_secrets::secret("password"),
    }];
    harness.state_mut().apply_settings(wanted).unwrap();
    let id = harness.state().rows()[0].id;
    harness.state().backend().retry(id);
    until(
        &mut harness,
        "it to complete with the saved sign-in",
        |app| app.rows().iter().any(|r| r.state == State::Completed),
    );
    let mut back = harness.state().backend().settings();
    back.site_logins.clear();
    harness.state_mut().apply_settings(back).unwrap();
}

/// A .torrent on this computer is added by its path or its
/// file address in Add URL; only a drop took it.
#[test]
fn add_url_takes_a_torrent_on_this_computer() {
    let world = world();
    let mut harness = world.harness();
    let file = world.dir.path().join("a set.torrent");
    std::fs::write(&file, b"d4:infodee").unwrap();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", file.to_str().unwrap());
    click(&mut harness, "OK");
    let urls = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .map(|d| d.urls.clone())
        .expect("Download details did not open");
    assert_eq!(urls, [dm_core::torrent::file_url(&file).unwrap()]);
}

/// A stream server's address goes in through Add URL like any other, is
/// named as it will be saved, and is queued; dm's own stream client records
/// it (the engine's `tests/live.rs`).
#[test]
fn a_stream_servers_address_is_taken_through_add_url() {
    let world = world();
    let mut harness = world.harness();
    add_url(
        &mut harness,
        "rtmp://127.0.0.1:1/live/show",
        "Add for later",
    );
    harness.state_mut().refresh();
    harness.run_steps(3);
    let rows = harness.state().rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].url, "rtmp://127.0.0.1:1/live/show");
    assert!(harness.query_by_label_contains("show").is_some());
    assert!(dm_desktop::app::looks_downloadable("rtmp://h/live/x").is_some());
    assert!(dm_desktop::app::looks_downloadable("mms://h/clip.asf").is_some());
}

/// A failure is announced as a finish is, saying why in plain words; it was
/// only a sound, if that was on.
#[test]
fn a_failed_download_is_announced_with_why() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(3);
    let told = world.fake.notified.lock().unwrap().clone();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("secret.zip — "), "{told:?}");
    assert!(!told[0].contains("http"), "{told:?}");
}

/// Download complete can be turned off from itself ("Don't show this
/// window again"), and offers Open with beside Open.
#[test]
fn download_complete_offers_open_with_and_not_again() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    assert!(harness.query_by_label("Open with…").is_some());
    // Ticking marks the box and leaves the window open; nothing is saved yet.
    click(&mut harness, "Don't show this window again");
    harness.run_steps(2);
    assert!(harness.query_by_label("Download complete").is_some());
    assert!(
        harness.state().dialogs.complete_never,
        "the box is not kept ticked"
    );
    assert!(
        harness.state().applied.show_complete,
        "saved before it was closed"
    );
    // Closing the window saves it.
    click(&mut harness, "Close");
    harness.run_steps(2);
    assert!(harness.query_by_label("Download complete").is_none());
    assert!(
        !harness.state().applied.show_complete,
        "the setting stays on"
    );
    assert!(!harness.state().dialogs.complete_never);
    // Kept, not only for this window: the engine has it, and the next
    // download that finishes opens nothing.
    assert!(!harness.state().backend().settings().show_complete);
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "the second download to complete", |app| {
        app.rows()
            .iter()
            .filter(|r| r.state == State::Completed)
            .count()
            == 2
    });
    harness.run_steps(3);
    assert!(harness.query_by_label("Download complete").is_none());
    // Put back for the tests after.
    let mut wanted = harness.state().applied.clone();
    wanted.show_complete = true;
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// A file saved a minute ago, so the folder's reader does not take it for one
/// still being written.
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
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
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
    harness.run_steps(3);
    assert!(
        harness.state().rows().is_empty(),
        "nothing is read while it is off"
    );
    click(&mut harness, "Options");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.watch_folder = dir.display().to_string();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");

    until(&mut harness, "both links to be added", |app| {
        app.rows()
            .iter()
            .any(|r| r.url.ends_with("watched-one.zip"))
            && app
                .rows()
                .iter()
                .any(|r| r.url.ends_with("watched-two.zip"))
    });
    let done = dir.join("done");
    assert!(
        done.join("one.url").exists() && done.join("two.txt").exists(),
        "put aside, not deleted"
    );
    assert!(!dir.join("one.url").exists() && !dir.join("two.txt").exists());
    assert!(
        dir.join("nothing.txt").exists(),
        "a file with no link is left where it is"
    );
    assert!(dir.join("picture.png").exists(), "other kinds are not read");
    assert_eq!(
        harness.state().rows().len(),
        2,
        "the file with no link added nothing"
    );
    assert!(
        harness
            .state()
            .problems
            .iter()
            .any(|p| p.contains("nothing.txt") && p.contains("no link")),
        "and it is said once: {:?}",
        harness.state().problems
    );

    // The same link saved again is already in the list: put aside, not added.
    std::fs::write(
        dir.join("again.url"),
        format!("[InternetShortcut]\nURL={}\n", world.url("watched-one.zip")),
    )
    .unwrap();
    aged(&dir.join("again.url"));
    harness.state_mut().refresh();
    let started = Instant::now();
    while dir.join("again.url").exists() && started.elapsed() < Duration::from_secs(12) {
        harness.state_mut().refresh();
        harness.step();
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(done.join("again.url").exists(), "put aside");
    assert_eq!(
        harness.state().rows().len(),
        2,
        "a link already listed is not added again"
    );
}

/// Move/Rename of several finished files, with a pattern: `{n}` counts from
/// 1 in the order listed, `{name}` is the old name, the ending is kept, and
/// an empty pattern leaves the names alone.
#[test]
fn several_finished_files_are_named_by_a_pattern_and_keep_their_endings() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "both to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    let (zip, pdf) = (row(&harness, "file.zip").id, row(&harness, "other.pdf").id);
    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![zip, pdf],
        pattern: String::new(),
        ..Default::default()
    });
    harness.run_steps(2);
    assert!(
        harness.query_by_label("Name pattern").is_some(),
        "several files are named by a pattern"
    );
    click(&mut harness, "OK");
    assert_eq!(
        row(&harness, "file.zip").filename,
        "file.zip",
        "empty keeps names"
    );

    harness.state_mut().dialogs.rename = Some(dm_desktop::dialogs::MoveRename {
        ids: vec![zip, pdf],
        pattern: "Trip {n} ({name})".into(),
        ..Default::default()
    });
    harness.run_steps(2);
    click(&mut harness, "OK");
    harness.run_steps(2);
    let now = |id| harness.state().row(id).unwrap().clone();
    assert_eq!(now(zip).filename, "Trip 1 (file).zip");
    assert_eq!(now(pdf).filename, "Trip 2 (other).pdf");
    assert!(
        std::path::Path::new(&now(zip).path).exists()
            && std::path::Path::new(&now(pdf).path).exists(),
        "the files on disk were renamed with them"
    );
}

/// Properties of a finished file shows its SHA-256, worked out beside the
/// window, and says whether a digest it was given matches.
#[test]
fn properties_show_the_files_sha256_and_check_a_given_digest() {
    use sha2::{Digest, Sha256};
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the file to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let finished = row(&harness, "file.zip");
    let expected = hex::encode(Sha256::digest(std::fs::read(&finished.path).unwrap()));
    let id = finished.id;
    harness.state_mut().dialogs.open_properties(&finished);
    until(&mut harness, "its SHA-256 to be shown", |_| true);
    let started = Instant::now();
    while harness.query_by_label(&expected).is_none() {
        harness.state_mut().refresh();
        harness.step();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the SHA-256 never showed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // A matching digest, given in any case, and a different one.
    for (given, says) in [
        (expected.to_uppercase(), "It matches."),
        ("0".repeat(64), "It does not match."),
    ] {
        harness
            .state_mut()
            .dialogs
            .properties
            .get_mut(&id)
            .unwrap()
            .check = given;
        harness.run_steps(3);
        assert!(harness.query_by_label(says).is_some(), "{says}");
    }
    // Not a digest at all: nothing is said about it.
    harness
        .state_mut()
        .dialogs
        .properties
        .get_mut(&id)
        .unwrap()
        .check = "not a digest".into();
    harness.run_steps(3);
    assert!(harness.query_by_label("It matches.").is_none());
    assert!(harness.query_by_label("It does not match.").is_none());
}

#[test]
fn a_caught_download_of_a_listed_address_asks_before_fetching_again() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_caught_downloads = false;
    let url = world.url("file.zip");
    harness
        .state()
        .backend()
        .add_with(&url, dm_core::AddOptions::default())
        .unwrap();
    harness.state_mut().refresh();
    let before = harness.state().rows().len();
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: url.clone(),
            referrer: "https://e.com/page".into(),
            caught: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(3);
    assert!(harness.state().dialogs.duplicate.is_some());
    harness.state_mut().refresh();
    assert_eq!(
        harness.state().rows().len(),
        before,
        "fetched again unasked"
    );
}

/// Escape closes Download complete as its Close does, and a ticked "Don't
/// show this window again" is kept either way.
#[test]
fn escape_keeps_a_ticked_dont_show_again_as_close_does() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    click(&mut harness, "Don't show this window again");
    harness.run_steps(2);
    assert!(harness.state().applied.show_complete);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.query_by_label("Download complete").is_none());
    assert!(!harness.state().applied.show_complete);
    let mut wanted = harness.state().applied.clone();
    wanted.show_complete = true;
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// The row under the pointer has its own Stop/Resume and Remove, so a
/// download is paused and resumed where it is, not from the toolbar.
#[test]
fn the_hovered_row_offers_stop_and_remove_on_itself() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("slow.bin"), "Add for later");
    harness.state_mut().refresh();
    harness.run_steps(3);
    let id = harness.state().rows()[0].id;
    let rect = *harness
        .state()
        .row_rects
        .get(&id)
        .expect("the row is drawn");
    harness.hover_at(rect.center());
    harness.run_steps(3);
    let name = dm_desktop::app::name_of(&harness.state().rows()[0].clone());
    assert!(
        harness.query_by_label(&format!("Remove {name}")).is_some(),
        "no buttons on the hovered row"
    );
}

/// A download handed over and waiting for an answer has left the browser: the
/// window comes forward for it. One that starts at once does not take the
/// person's screen.
#[test]
fn a_hand_over_that_asks_brings_the_window_forward_and_one_that_starts_does_not() {
    let world = world();
    let mut harness = world.harness();
    let handed = |harness: &mut Harness<'_, DmApp>, name: &str| {
        harness
            .state()
            .backend()
            .handed
            .lock()
            .unwrap()
            .push_back(dm_shell::host::Request {
                url: world.url(name),
                referrer: "https://e.com/page".into(),
                caught: true,
                ..Default::default()
            });
        let ctx = harness.ctx.clone();
        harness.state_mut().logic(&ctx);
    };
    harness.state_mut().applied.ask_caught_downloads = false;
    handed(&mut harness, "quiet.zip");
    assert!(
        !harness.state().show_asked,
        "raised for a download that started"
    );
    harness.state_mut().applied.ask_caught_downloads = true;
    handed(&mut harness, "asks.zip");
    assert!(harness.state().dialogs.file_info.is_some());
    assert!(
        harness.state().show_asked,
        "the waiting dialog stayed behind"
    );
}
