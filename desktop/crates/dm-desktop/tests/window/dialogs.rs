use crate::support::*;
use crate::test_secrets;

/// A page's links, handed over by the browser: "Start now"
/// starts them, "Add for later" puts them in the Main queue, and the sizes
/// the dialog showed stay on the rows until each learns its own.
fn hand_links(harness: &mut Harness<'_, DmApp>, world: &World, links: Vec<String>) {
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("page.html"),
            referrer: world.url("page.html"),
            links,
            ask: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(2);
    // Titled for a page's every link or a selection's alike.
    assert!(harness.query_by_label("Download links").is_some());
}

#[test]
fn links_on_the_clipboard_are_picked_and_put_in_the_main_queue() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.clipboard.lock().unwrap() = Some(format!(
        "see {} and {} and not this",
        world.url("file.zip"),
        world.url("clip.bin")
    ));
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(2);
    let offered: Vec<String> = harness
        .state()
        .dialogs
        .clip_links
        .as_ref()
        .unwrap()
        .iter()
        .map(|l| l.0.clone())
        .collect();
    assert_eq!(offered, [world.url("file.zip"), world.url("clip.bin")]);
    click(&mut harness, "Add for later");
    harness.state_mut().refresh();
    let rows = harness.state().rows();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|r| r.queue == dm_core::MAIN_QUEUE && r.state == State::Paused));
}

#[test]
fn a_video_from_the_browsers_panel_opens_download_file_info_with_its_quality() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_panel_downloads = true;
    let page = world.url("file.zip");
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: page.clone(),
            referrer: "https://video.example/watch".into(),
            quality: Some("720".into()),
            label: Some("MP4 1280×720".into()),
            ask: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(3);
    assert!(harness.query_by_label("Download details").is_some());
    assert!(harness.query_by_label("MP4 1280×720").is_some());
    assert!(
        harness.state().rows().is_empty(),
        "nothing starts before it is asked"
    );

    click(&mut harness, "Start now");
    harness.state_mut().refresh();
    let row = harness.state().rows()[0].clone();
    assert_eq!(row.url, page);
    assert_eq!(row.quality.as_deref(), Some("720"));
    assert_eq!(row.referrer, "https://video.example/watch");
}

/// A new install's welcome offers the extension for each browser found
/// (each one's page where none is), opens the page asked for, and goes with
/// Later.
#[test]
fn the_welcome_offers_the_extension_for_each_browser() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().welcome();
    harness.run_steps(3);
    assert!(harness.query_by_label("Add dm to your browser").is_some());
    let offered = harness
        .query_all_by_label_contains("Add to ")
        .next()
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .unwrap_or_else(|| "Microsoft Edge".to_string());
    // With no browser found the offer is the fallback's name, which the
    // hint bar behind the dialog shows too: the dialog's is the last.
    harness
        .query_all_by_label(&offered)
        .last()
        .expect("the offer")
        .click();
    harness.run_steps(3);
    let opened = world.fake.opened.lock().unwrap().clone();
    assert!(
        opened.iter().any(|page| [
            dm_desktop::app::EXTENSION_EDGE,
            dm_desktop::app::EXTENSION_FIREFOX,
            dm_desktop::app::EXTENSION_CHROME
        ]
        .contains(&page.as_str())),
        "{opened:?}"
    );
    click(&mut harness, "Later");
    assert!(harness.state().dialogs.welcome.is_none());
}

#[test]
fn a_pages_links_open_the_download_links_dialog() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_panel_downloads = true;
    hand_links(
        &mut harness,
        &world,
        vec![
            world.url("file.zip"),
            world.url("other.pdf"),
            world.url("next.html"),
        ],
    );
    // Each link's server is asked for its kind and size.
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label("Archives").is_none()
        || harness.query_all_by_label("256.0 KB").count() < 2
    {
        assert!(Instant::now() < deadline, "the links were never checked");
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(50));
    }
    // "Hide HTML files" is on by default: the page is not offered.
    assert!(harness.query_by_label("next.html").is_none());
    click(&mut harness, "other.pdf");
    click(&mut harness, "Add for later");
    harness.state_mut().refresh();
    let rows = harness.state().rows().to_vec();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].url, world.url("file.zip"));
    assert_eq!(rows[0].queue, dm_core::MAIN_QUEUE);
    assert_eq!(rows[0].state, State::Paused, "for later: not started");
    assert_eq!(rows[0].referrer, world.url("page.html"));
    // The size the dialog showed is on the row, though nothing was fetched.
    assert_eq!(rows[0].progress.total, None);
    assert_eq!(
        harness.state().visible()[0].progress.total,
        Some(PAYLOAD as u64),
        "the size the dialog showed was lost"
    );
    harness.run_steps(2);
    assert!(harness.query_by_label("256.0 KB").is_some());

    // Only the types typed are listed, and only those are added.
    hand_links(
        &mut harness,
        &world,
        vec![world.url("file.zip"), world.url("other.pdf")],
    );
    type_into(&mut harness, "Only these types", "PDF");
    // file.zip is in the list behind the dialog, so its line here is
    // known by what follows its name.
    assert!(harness
        .query_by_label("file.zip (already in the list)")
        .is_none());
    let links = harness.state().dialogs.links.as_ref().unwrap();
    assert!(!links.shown(&world.url("file.zip")));
    assert!(links.shown(&world.url("other.pdf")));
    click(&mut harness, "Check all");
    let chosen = harness.state().dialogs.links.as_ref().unwrap().chosen();
    assert_eq!(chosen, [world.url("other.pdf")]);
    click(&mut harness, "Cancel");

    // OK's place is "Start now", and it starts them.
    hand_links(&mut harness, &world, vec![world.url("other.pdf")]);
    click(&mut harness, "Start now");
    until(&mut harness, "the started link to finish", |app| {
        app.rows()
            .iter()
            .any(|r| r.url.ends_with("/other.pdf") && r.state == State::Completed)
    });
}

#[test]
fn a_double_click_opens_properties_and_open_runs_a_finished_file() {
    let world = world();
    let mut harness = world.quick_harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    until(&mut harness, "one to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(2);

    // An unfinished one: Properties, with nothing yet to open.
    double_click(&mut harness, "stalled.bin");
    harness.run_steps(3);
    assert!(harness.query_by_label("File properties").is_some());
    let open = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Open");
    assert!(open.accesskit_node().is_disabled(), "nothing to open yet");
    harness.state_mut().dialogs.properties.clear();
    harness.run_steps(2);

    // A finished one: Properties, whose Open runs the file.
    let row = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.state == State::Completed)
        .cloned()
        .unwrap();
    double_click(&mut harness, "file.zip");
    harness.run_steps(3);
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Open")
        .click();
    harness.run_steps(2);
    assert_eq!(
        *world.fake.opened.lock().unwrap(),
        std::slice::from_ref(&row.path)
    );

    // Options › General's "opens the file": the double-click opens a
    // finished one straight away; one not finished still shows Properties.
    harness.state_mut().dialogs.properties.clear();
    world.fake.opened.lock().unwrap().clear();
    harness.state_mut().applied.double_click = "open".into();
    double_click(&mut harness, "file.zip");
    harness.run_steps(3);
    assert_eq!(*world.fake.opened.lock().unwrap(), [row.path]);
    assert!(
        harness.state().dialogs.properties.is_empty(),
        "Properties opened instead"
    );
    double_click(&mut harness, "stalled.bin");
    harness.run_steps(3);
    assert!(harness.query_by_label("File properties").is_some());
}

/// Download details offers the folder a kind of file was last put in, and
/// only for that kind; choosing the kind's own folder again forgets it.
#[test]
fn download_details_offers_the_folder_last_used_for_that_kind() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let folder = world.dir.path().join("my zips");
    std::fs::create_dir_all(&folder).unwrap();
    let details = |harness: &mut Harness<'_, DmApp>, url: String| {
        harness.state_mut().dialogs.open_file_info(vec![url]);
        harness.run_steps(2);
        settle_details(harness);
        harness.run_steps(2);
        harness
            .state()
            .dialogs
            .file_info
            .as_ref()
            .map(|d| d.folder.clone())
            .unwrap()
    };
    assert_eq!(details(&mut harness, world.url("file.zip")), "");
    type_into(&mut harness, "Save to folder", &folder.to_string_lossy());
    click(&mut harness, "Add for later");
    assert_eq!(
        harness.state().applied.last_folders.get("Archives"),
        Some(&folder.to_string_lossy().to_string())
    );
    // A document is not an archive.
    assert_eq!(details(&mut harness, world.url("other.pdf")), "");
    click(&mut harness, "Cancel");
    // The next archive is offered it, and saved there.
    let offered = details(&mut harness, world.url("file.zip"));
    assert_eq!(offered, folder.to_string_lossy());
    // Its own folder chosen again: forgotten.
    harness
        .state_mut()
        .dialogs
        .file_info
        .as_mut()
        .unwrap()
        .folder
        .clear();
    harness.run_steps(2);
    click(&mut harness, "Add for later");
    assert!(harness.state().applied.last_folders.is_empty());
}

#[test]
fn the_file_info_dialog_puts_the_file_where_and_as_what_it_says() {
    // Its folder is remembered for the kind, in the settings file.
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    let folder = world.dir.path().join("picked");
    type_into(&mut harness, "Save to folder", &folder.to_string_lossy());
    // Save as holds the server's name once it answers: replaced, as a
    // person replaces it, by selecting it all first.
    until(&mut harness, "the server's name in Save as", |app| {
        app.dialogs
            .file_info
            .as_ref()
            .is_some_and(|d| d.name == "file.zip")
    });
    harness.get_by_label("Save as").click();
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("renamed.zip".into()));
    harness.run_steps(2);
    type_role(&mut harness, "Note", "the one from the page");
    click(&mut harness, "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    assert_eq!(std::path::Path::new(&row.path), folder.join("renamed.zip"));
    assert_eq!(row.description, "the one from the page");
    harness.run_steps(2);
    assert!(
        harness.query_by_label("the one from the page").is_some(),
        "the Note column is empty"
    );
}

/// Add links from clipboard offers an address already in the list
/// unticked and says so.
#[test]
fn a_clipboard_link_already_listed_is_offered_unticked() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.state_mut().refresh();
    *world.fake.clipboard.lock().unwrap() = Some(format!(
        "{} {}",
        world.url("file.zip"),
        world.url("other.pdf")
    ));
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(3);
    let ticks: Vec<bool> = harness
        .state()
        .dialogs
        .clip_links
        .as_ref()
        .unwrap()
        .iter()
        .map(|l| l.1)
        .collect();
    assert_eq!(ticks, [false, true]);
    assert!(harness
        .query_by_label_contains("(already in the list)")
        .is_some());
}

#[test]
fn properties_keep_the_login_out_of_the_address_box() {
    let password = test_secrets::secret("password");
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world
                .url("private.bin")
                .replace("http://", &format!("http://me:{password}@")),
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    let row = harness.state().rows()[0].clone();
    harness.state_mut().dialogs.open_properties(&row);
    harness.run_steps(3);
    use egui::accesskit::Role;
    let address = harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .value()
        .unwrap_or_default();
    assert!(
        !address.contains(&password),
        "the password is in the address box: {address}"
    );
    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "Login")
            .value()
            .as_deref(),
        Some("me")
    );
    // Changing the password changes it on the download.
    harness
        .get_by_role_and_label(Role::PasswordInput, "Password")
        .focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::PasswordInput, "Password")
        .type_text("better");
    harness.run_steps(2);
    click(&mut harness, "OK");
    let url = harness.state().rows()[0].url.clone();
    assert!(url.contains("me:better@"), "{url}");
}

#[test]
fn properties_change_the_description_and_refresh_an_expired_address() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("gone.bin"), "Start now");
    until(&mut harness, "the missing file to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    select(&mut harness, "gone.bin");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(3);
    assert!(harness.query_by_label("File properties").is_some());
    type_role(&mut harness, "Note", "fixed link");
    use egui::accesskit::Role;
    harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .type_text(&world.url("file.zip"));
    harness.run_steps(2);
    click(&mut harness, "OK");

    let row = harness.state().rows()[0].clone();
    assert_eq!(row.description, "fixed link");
    assert!(row.url.ends_with("/file.zip"), "{}", row.url);
    // Not yet named by a server, the row goes by its new address's name.
    select(&mut harness, "file.zip");
    // A failed row is retried, and its button says so.
    assert!(harness.query_by_label("Resume").is_none());
    click(&mut harness, "Retry");
    until(&mut harness, "the refreshed download to complete", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
}

#[test]
fn help_opens_the_documentation() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Help);
    let opened = world.fake.opened.lock().unwrap().clone();
    assert_eq!(opened, [dm_desktop::update::HELP]);
}

/// What a press of Start said goes once the range is changed: it stood
/// over the count of what was typed since.
#[test]
fn the_series_error_from_start_goes_when_the_range_is_fixed() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    type_into(&mut harness, "Address with *", &world.url("part*.bin"));
    type_into(&mut harness, "From", "5");
    type_into(&mut harness, "To", "1");
    click(&mut harness, "Start now");
    let said = harness
        .state()
        .dialogs
        .series
        .as_ref()
        .unwrap()
        .says
        .clone();
    assert!(!said.is_empty(), "Start said nothing of a backwards range");
    type_into(&mut harness, "To", "9");
    assert!(
        harness.query_by_label(&said).is_none(),
        "the old error still stands"
    );
    assert!(harness.query_by_label_contains("15 addresses").is_some());
    assert!(harness.state().rows().is_empty());
}

#[test]
fn the_site_file_finder_finds_the_files_a_page_links_to() {
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Site files");
    type_into(&mut harness, "Start page", &world.url("page.html"));
    let types = harness.get_by_label("File types");
    types.focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label("File types").type_text("zip pdf");
    harness.run_steps(1);
    click(&mut harness, "Find files");
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label_contains("Files found: 2 ").is_none() {
        assert!(
            Instant::now() < deadline,
            "the site file finder found nothing"
        );
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(25));
    }
    // The window grew to hold the list and, centred, moved: let it settle
    // before clicking where its button now is.
    harness.run_steps(3);
    // Below the found files, which scroll under the dialog's buttons:
    // scrolled to, as a person would.
    harness.get_by_label("Add for later").scroll_to_me();
    harness.run_steps(3);
    click(&mut harness, "Add for later");
    let urls: Vec<String> = harness
        .state()
        .rows()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(urls.len(), 2, "{urls:?}");
    assert!(urls.iter().any(|u| u.ends_with("/file.zip")));
    assert!(urls.iter().any(|u| u.ends_with("/other.pdf")));
}

#[test]
fn a_site_search_reads_pages_deep_and_a_saved_one_runs_on_its_own() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Site files");
    type_into(&mut harness, "Start page", &world.url("page.html"));
    assert!(harness
        .query_by_label("Page depth (0 for the start page only)")
        .is_some());
    if let Some(d) = harness.state_mut().dialogs.site_files.as_mut() {
        d.depth = 1;
        d.types = "zip pdf".into();
    }
    click(&mut harness, "Find files");
    // page.html and next.html on this site; the page elsewhere is not read.
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness
        .query_by_label_contains("Files found: 3 · pages read: 2")
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "the search did not go a page deep: {:?}",
            {
                let d = harness.state().dialogs.site_files.as_ref();
                d.map(|d| format!("{:?}", dm_core::lock(&d.state)))
            }
        );
        harness.run_steps(1);
        std::thread::sleep(Duration::from_millis(25));
    }

    // The window grew to hold the list and, centred, moved: let it settle
    // before clicking where its fields now are.
    harness.run_steps(3);

    // Saved to run every hour: it runs now, and what it finds is added for
    // later; run again by hand it adds nothing twice.
    // Below the found files, which scroll under the dialog's buttons.
    harness.get_by_label("Save this search as").scroll_to_me();
    harness.run_steps(3);
    type_into(&mut harness, "Save this search as", "Nightly");
    if let Some(d) = harness.state_mut().dialogs.site_files.as_mut() {
        d.every_hours = 1;
    }
    harness.run_steps(1);
    harness.get_by_label("Save search").scroll_to_me();
    harness.run_steps(3);
    click(&mut harness, "Save search");
    let saved = harness.state().backend().settings().site_searches;
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].name.as_str(), saved[0].depth, saved[0].every_hours),
        ("Nightly", 1, 1)
    );
    assert_eq!(saved[0].types, ["zip", "pdf"]);
    assert!(harness.query_by_label_contains("Nightly").is_some());
    // Not at once: its first run on the timer is an hour from saving (run at once, its files were queued with nothing said).
    harness.run_steps(5);
    assert!(harness.state().rows().is_empty(), "it ran as it was saved");
    // An hour on.
    let mut later = harness.state().backend().settings();
    later.site_searches[0].last_run -= 3600;
    harness.state_mut().apply_settings(later).unwrap();
    until(&mut harness, "the saved search's files", |app| {
        app.rows().len() == 3
    });
    assert!(harness
        .state()
        .rows()
        .iter()
        .all(|r| r.state == State::Paused && r.queue == dm_core::MAIN_QUEUE));
    assert!(harness.state().backend().settings().site_searches[0].last_run > 0);

    click(&mut harness, "Run");
    until(&mut harness, "the search to come back", |app| {
        app.searches_running == 0
    });
    harness.run_steps(2);
    assert_eq!(harness.state().rows().len(), 3, "found twice, added twice");
    // The list emptied: what the search found before is still not new.
    let ids: Vec<_> = harness.state().rows().iter().map(|r| r.id).collect();
    for id in ids {
        harness.state().backend().remove(id);
    }
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());
    click(&mut harness, "Run");
    until(&mut harness, "the search to come back again", |app| {
        app.searches_running == 0
    });
    harness.run_steps(2);
    assert!(
        harness.state().rows().is_empty(),
        "found again after the list was cleared"
    );
    assert!(harness
        .query_by_label_contains("found nothing new")
        .is_some());

    click(&mut harness, "Delete search");
    assert!(harness
        .state()
        .backend()
        .settings()
        .site_searches
        .is_empty());
}

#[test]
fn export_and_import_carry_the_list_as_a_dmlist_file() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                referrer: "https://page.example/".into(),
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    let file = world.dir.path().join("list.dmlist");
    *world.fake.chosen.lock().unwrap() = Some(file.clone());
    harness.state_mut().run(Command::Export);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("\"referrer\": \"https://page.example/\""),
        "{text}"
    );

    let other = world.harness();
    let mut other = other;
    other.state_mut().run(Command::Import);
    let rows = other.state().rows().to_vec();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].referrer, "https://page.example/");
}

#[test]
fn an_address_already_in_the_list_is_asked_about_before_it_is_added_again() {
    let world = world();
    let mut harness = world.harness();
    let url = world.url("file.zip");
    add_url(&mut harness, &url, "Add for later");
    let id = harness.state().rows()[0].id;

    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    assert!(
        harness.query_by_label("Already in the list").is_some(),
        "a second row was made without asking"
    );
    assert!(harness.query_by_label("Download details").is_none());
    click(&mut harness, "Show it");
    assert!(harness.query_by_label("Already in the list").is_none());
    assert!(
        harness.state().selected.contains(&id),
        "the row was not shown"
    );
    assert_eq!(harness.state().rows().len(), 1);

    // Asked again, and this time wanted again: the usual dialog follows.
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    click(&mut harness, "Download again");
    assert!(harness.query_by_label("Download details").is_some());
    click(&mut harness, "Add for later");
    assert_eq!(harness.state().rows().len(), 2);
}

/// Download details' Category sends a file to the folder of the kind
/// picked, here a .zip to Documents rather than Archives.
#[test]
fn download_details_category_picks_the_folder() {
    let world = world();
    // With sorting into kind folders off, as it is unless turned on: the
    // pick is still that kind's folder.
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        categorize: false,
        show_progress: false,
        show_complete: false,
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 760.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    harness.run_steps(2);
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(4);
    click(&mut harness, "Category");
    harness.get_by_label("Documents").click();
    harness.run_steps(3);
    let folder = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .unwrap()
        .folder
        .clone();
    assert!(folder.ends_with("Documents"), "{folder}");
    click(&mut harness, "Start now");
    until(&mut harness, "it to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let path = harness.state().rows()[0].path.clone();
    assert!(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .ends_with("Documents"),
        "{path}"
    );
}

/// Move/Rename moves several finished files to another folder at once,
/// and their rows follow them.
#[test]
fn move_rename_moves_several_finished_files_together() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("other.pdf"), "Start now");
    until(&mut harness, "both to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    harness.state_mut().run(Command::SelectAll);
    harness.state_mut().run(Command::MoveRename);
    harness.run_steps(3);
    assert!(harness.query_by_label("Moves 2 finished files.").is_some());
    let to = world.dir.path().join("moved");
    harness.state_mut().dialogs.rename.as_mut().unwrap().folder = to.display().to_string();
    harness.run_steps(2);
    click(&mut harness, "OK");
    harness.state_mut().refresh();
    for row in harness.state().rows() {
        let path = std::path::Path::new(&row.path);
        assert_eq!(path.parent(), Some(to.as_path()), "{}", row.path);
        assert!(path.is_file(), "{} is not there", row.path);
    }
}

#[test]
fn two_cancelled_hand_offs_from_a_site_offer_to_leave_it_to_the_browser() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_panel_downloads = true;
    let hand = |harness: &mut Harness<'_, DmApp>, name: &str| {
        harness
            .state()
            .backend()
            .handed
            .lock()
            .unwrap()
            .push_back(dm_shell::host::Request {
                url: world.url(name),
                referrer: world.url("page.html"),
                ask: true,
                ..Default::default()
            });
        harness.state_mut().take_handed();
        harness.run_steps(2);
        assert!(harness.query_by_label("Download details").is_some());
    };
    // One cancelled: nothing asked. A typed-in one cancelled: nothing either.
    hand(&mut harness, "file.zip");
    click(&mut harness, "Cancel");
    assert!(harness
        .query_by_label("Leave this site to the browser?")
        .is_none());
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("other.pdf"));
    click(&mut harness, "OK");
    click(&mut harness, "Cancel");
    assert!(harness
        .query_by_label("Leave this site to the browser?")
        .is_none());
    // The second handed one: asked, and "keep" leaves the rules alone.
    hand(&mut harness, "other.pdf");
    click(&mut harness, "Cancel");
    assert!(harness
        .query_by_label("Leave this site to the browser?")
        .is_some());
    click(&mut harness, "Keep handing them over");
    assert!(harness
        .state()
        .backend()
        .settings()
        .catch
        .skip_hosts
        .is_empty());
    // Twice more, and this time left to the browser: the site joins the
    // list the extension reads.
    hand(&mut harness, "file.zip");
    click(&mut harness, "Cancel");
    hand(&mut harness, "file.zip");
    click(&mut harness, "Cancel");
    click(&mut harness, "Leave them to the browser");
    assert_eq!(
        harness.state().backend().settings().catch.skip_hosts,
        ["127.0.0.1"]
    );
    // Put back for the other tests.
    let mut wanted = harness.state().backend().settings();
    wanted.catch.skip_hosts.clear();
    harness.state_mut().apply_settings(wanted).unwrap();
}

#[test]
fn a_handed_over_videos_sign_in_survives_download_details() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.ask_panel_downloads = true;
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("file.zip"),
            referrer: world.url("page.html"),
            ask: true,
            cookies: "session=1".into(),
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(2);
    click(&mut harness, "Add for later");
    let rows = harness.state().rows().to_vec();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].cookies, "session=1",
        "Download details dropped the sign-in"
    );

    // A page's links: its sign-in goes with the links on its own server only.
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("page.html"),
            referrer: world.url("page.html"),
            links: vec![
                world.url("other.pdf"),
                "http://elsewhere.invalid/x.zip".into(),
            ],
            ask: true,
            cookies: "session=2".into(),
            ..Default::default()
        });
    harness.state_mut().take_handed();
    harness.run_steps(2);
    // The made-up server cannot be asked, so its link is unticked: ticked
    // again, as a person would who wants it anyway.
    until(
        &mut harness,
        "the made-up server's link to be refused",
        |app| {
            app.dialogs
                .links
                .as_ref()
                .is_some_and(|d| d.refused.iter().any(|u| u.ends_with("/x.zip")))
        },
    );
    click(&mut harness, "Check all");
    click(&mut harness, "Add for later");
    harness.state_mut().refresh();
    let rows = harness.state().rows().to_vec();
    let of = |end: &str| {
        rows.iter()
            .find(|r| r.url.ends_with(end))
            .map(|r| r.cookies.clone())
    };
    assert_eq!(of("/other.pdf").as_deref(), Some("session=2"));
    assert_eq!(
        of("/x.zip").as_deref(),
        Some(""),
        "another server was given the page's sign-in"
    );
}

#[test]
fn the_clipboard_watcher_offers_only_new_links_and_only_when_on() {
    let _file = settings_file();
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some(world.url("old.iso"));
    let mut harness = world.harness();
    let clip = |h: &mut Harness<'_, DmApp>, text: &str| {
        *world.fake.clipboard.lock().unwrap() = Some(text.to_string());
        h.state_mut().check_clipboard();
        h.run_steps(1);
        h.state().clipboard_offer().map(str::to_owned)
    };
    assert_eq!(
        clip(&mut harness, &world.url("new.iso")),
        None,
        "offered while off"
    );

    click(&mut harness, "Options");
    // Scrolled to first, as a person scrolls to it: where Windows 11 adds
    // the Mica box above it, the box is below the tab's visible edge and a
    // click at it reached nothing.
    let watch = "Offer to download links I copy";
    harness.get_by_label(watch).scroll_to_me();
    // The scroll is animated: clicked mid-way, the box had moved on.
    harness.run();
    click(&mut harness, watch);
    click(&mut harness, "OK");
    assert!(
        harness.state().backend().settings().watch_clipboard,
        "ticking the box did nothing"
    );
    harness.state_mut().check_clipboard();
    assert_eq!(
        harness.state().clipboard_offer(),
        None,
        "offered what was there before"
    );

    assert_eq!(clip(&mut harness, "just some words"), None);
    let fresh = world.url("fresh.iso");
    assert_eq!(clip(&mut harness, &fresh).as_deref(), Some(fresh.as_str()));
    click(&mut harness, "Dismiss");
    clip(&mut harness, "elsewhere");
    assert_eq!(
        clip(&mut harness, &fresh),
        None,
        "a dismissed link came back"
    );
}

#[test]
fn a_dialog_asked_for_while_open_keeps_what_was_typed_in_it() {
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", "https://e.com/typed.zip");
    harness.state_mut().run(Command::AddUrl);
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .add_url
            .as_ref()
            .map(|d| d.address.as_str()),
        Some("https://e.com/typed.zip"),
        "Add URL asked for again threw away the address"
    );
    harness.state_mut().run(Command::Options);
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .connections = 3;
    harness.state_mut().run(Command::Options);
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .options
            .as_ref()
            .map(|o| o.draft.connections),
        Some(3),
        "Options asked for again lost its unsaved change"
    );
    for (command, open) in [
        (Command::Scheduler, "Scheduler"),
        (Command::AddSeries, "Add numbered series"),
        (Command::SiteFiles, "Site files"),
        (Command::SpeedLimiterSettings, "Speed limiter"),
    ] {
        harness.state_mut().run(command);
        harness.state_mut().run(command);
        assert!(harness
            .state()
            .dialogs
            .open_names()
            .iter()
            .any(|n| n == open));
    }
}

/// A download that starts without the window starting it — a
/// browser's hand-off — opens its progress window too, when that is on.
#[test]
fn a_browsers_hand_off_opens_its_progress_window() {
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: true,
        show_complete: false,
        language: "en".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    harness.run_steps(2);
    // As the native host's hand-off lands: in the queue, not through the
    // window's own Add.
    let id = harness
        .state()
        .backend()
        .add_with(&world.url("stalled.bin"), dm_core::AddOptions::default())
        .unwrap();
    until(&mut harness, "its progress window", |app| {
        app.dialogs.progress.contains(&id)
    });
    // One added for later has not started, and opens nothing.
    let later = harness
        .state()
        .backend()
        .add_with(
            &world.url("file.zip"),
            dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    assert!(!harness.state().dialogs.progress.contains(&later));
}

/// Links on the clipboard can be started at once, or not at all.
#[test]
fn links_on_the_clipboard_can_start_now_or_be_cancelled() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.clipboard.lock().unwrap() = Some(world.url("file.zip"));
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(2);
    click(&mut harness, "Cancel");
    assert!(harness.state().dialogs.clip_links.is_none());
    assert!(harness.state().rows().is_empty());
    harness.state_mut().run(Command::AddFromClipboard);
    harness.run_steps(2);
    click(&mut harness, "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
}

/// A folder's field has a Browse… button, and what it picks is
/// where the file goes.
#[test]
fn browse_picks_the_folder_a_download_goes_to() {
    let world = world();
    let mut harness = world.harness();
    let picked = world.dir.path().join("picked");
    std::fs::create_dir_all(&picked).unwrap();
    *world.fake.chosen.lock().unwrap() = Some(picked.clone());
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    click(&mut harness, "Browse…");
    let folder = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .map(|d| d.folder.clone());
    assert_eq!(folder, Some(picked.display().to_string()));
    click(&mut harness, "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    assert!(std::path::Path::new(&harness.state().rows()[0].path).starts_with(&picked));

    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    assert_eq!(harness.query_all_by_label("Browse…").count(), 2);
}

/// Browse… on a desktop with no folder chooser says so.
#[test]
fn browse_with_no_folder_chooser_says_to_type_the_path() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.no_chooser.lock().unwrap() = true;
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    click(&mut harness, "Browse…");
    assert!(harness
        .query_by_label("No folder picker here: type the path.")
        .is_some());
}

/// A sign-in saved in Properties on a download refused for want
/// of one tries it again, and it downloads.
#[test]
fn a_sign_in_saved_in_properties_retries_the_download() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("locked.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    select(&mut harness, "locked.zip");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(3);
    // The keyboard is in Login already.
    harness.event(egui::Event::Text("user".into()));
    harness.run_steps(2);
    let id = harness.state().rows()[0].id;
    if let Some(d) = harness.state_mut().dialogs.properties.get_mut(&id) {
        assert_eq!(d.login, "user", "the keyboard is not in Login");
        d.password = "pass".into();
    }
    click(&mut harness, "OK");
    assert!(harness
        .state()
        .said
        .english()
        .contains("again with the sign-in"));
    until(&mut harness, "it to download with the sign-in", |app| {
        app.rows().iter().all(|r| r.state == State::Completed)
    });
    // "Trying … again" goes once it has been tried.
    until(&mut harness, "the sentence to go", |app| {
        !app.said.english().contains("again with the sign-in")
    });
}

/// Import asks, once, about addresses already in the list: Skip
/// them adds only the new ones, Add them again adds them all.
#[test]
fn import_asks_about_addresses_already_listed() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    let file = world.dir.path().join("two.txt");
    std::fs::write(
        &file,
        format!("{}\n{}\n", world.url("file.zip"), world.url("other.pdf")),
    )
    .unwrap();
    *world.fake.chosen.lock().unwrap() = Some(file.clone());
    harness.state().backend().pause_all();
    harness.state_mut().run(Command::Import);
    harness.run_steps(3);
    assert_eq!(harness.state().rows().len(), 1, "imported without asking");
    assert!(harness
        .query_by_label_contains("1 of the 2 addresses in this list")
        .is_some());
    click(&mut harness, "Skip them");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 2, "only the new one is added");

    harness.state_mut().run(Command::Import);
    harness.run_steps(3);
    click(&mut harness, "Add them again");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 4);
}

/// Where there is no file chooser, Import and Export ask for the
/// file's path, rather than doing nothing.
#[test]
fn import_and_export_take_a_typed_path_without_a_file_chooser() {
    let world = world();
    let mut harness = world.harness();
    *world.fake.no_chooser.lock().unwrap() = true;
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    let file = world.dir.path().join("typed.dmlist");
    harness.state_mut().run(Command::Export);
    harness.run_steps(3);
    assert!(harness
        .query_by_label("No file picker here: type the path.")
        .is_some());
    if let Some(d) = harness.state_mut().dialogs.list_file.as_mut() {
        assert!(d.path.ends_with("downloads.dmlist"), "{}", d.path);
        d.path = file.display().to_string();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(file.is_file(), "nothing was exported");

    let mut other = world.harness();
    other.state_mut().run(Command::Import);
    other.run_steps(3);
    type_into(&mut other, "File's path", "relative.dmlist");
    click(&mut other, "OK");
    assert!(other.query_by_label_contains("full path").is_some());
    if let Some(d) = other.state_mut().dialogs.list_file.as_mut() {
        d.path = file.display().to_string();
    }
    other.run_steps(2);
    click(&mut other, "OK");
    other.state_mut().refresh();
    assert_eq!(other.state().rows().len(), 1, "nothing was imported");
    *world.fake.no_chooser.lock().unwrap() = false;
}

/// A fresh link can be got from the browser: its page opens there, and
/// the next download caught becomes the row's link, with no row of its own.
#[test]
fn a_fresh_link_is_taken_from_the_browser() {
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
        .add_with(
            &world.url("expires.bin"),
            dm_core::AddOptions {
                referrer: world.url("page.html"),
                ..Default::default()
            },
        )
        .unwrap();
    until(&mut harness, "the expired link to fail", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(3);
    click(&mut harness, "Get it from the browser");
    assert!(world
        .fake
        .opened
        .lock()
        .unwrap()
        .contains(&world.url("page.html")));
    assert!(harness.state().awaiting_link.is_some());
    // The person clicks the download on the page; the browser hands it over.
    harness
        .state()
        .backend()
        .handed
        .lock()
        .unwrap()
        .push_back(dm_shell::host::Request {
            url: world.url("renewed.bin"),
            referrer: world.url("page.html"),
            caught: true,
            ..Default::default()
        });
    harness.state_mut().take_handed();
    assert!(harness.state().awaiting_link.is_none());
    until(&mut harness, "the download to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    assert_eq!(
        harness.state().rows().len(),
        1,
        "a row of its own was added"
    );
    assert_eq!(harness.state().rows()[0].url, world.url("renewed.bin"));
}

/// A file of the name already in the folder: Download details offers to
/// replace it, and it is replaced only once the new one is whole — a
/// number is still the default.
#[test]
fn download_details_can_replace_a_file_of_the_same_name() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let first = std::path::PathBuf::from(&harness.state().rows()[0].path);
    std::fs::write(&first, b"an older version").unwrap();
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    if harness.query_by_label("Already in the list").is_some() {
        click(&mut harness, "Download again");
    }
    let replace = "Replace that file once this one is whole";
    for _ in 0..50 {
        if harness.query_by_label(replace).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        harness.run_steps(1);
    }
    assert!(harness
        .query_by_label("Save this one as file (1).zip")
        .is_some());
    click(&mut harness, replace);
    click(&mut harness, "Start now");
    until(&mut harness, "the second download to complete", |app| {
        app.rows().len() == 2 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    assert_eq!(std::fs::read(&first).unwrap(), payload(), "not replaced");
    let dir = first.parent().unwrap();
    assert!(
        !dir.join("file (1).zip").exists(),
        "a numbered copy was left"
    );
    assert!(harness
        .state()
        .rows()
        .iter()
        .all(|r| std::path::Path::new(&r.path) == first));
}

/// Download details says the name a file will really get
/// where its folder has one of that name already, and that the server has
/// no file at an address, before Start now rather than after.
#[test]
fn download_details_says_the_name_it_will_get_and_a_server_refusal() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    if harness.query_by_label("Already in the list").is_some() {
        click(&mut harness, "Download again");
    }
    let said = "Save this one as file (1).zip";
    for _ in 0..50 {
        if harness.query_by_label(said).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        harness.run_steps(1);
    }
    assert!(
        harness.query_by_label(said).is_some(),
        "the renaming was not said"
    );
    click(&mut harness, "Cancel");

    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("missing.bin"));
    click(&mut harness, "OK");
    let refused = "Not available: this link no longer works (404).";
    for _ in 0..100 {
        if harness.query_by_label(refused).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        harness.run_steps(1);
    }
    assert!(
        harness.query_by_label(refused).is_some(),
        "the 404 was not said"
    );

    // The first copy still under way, not yet on disk: said as well.
    click(&mut harness, "Cancel");
    add_url(&mut harness, &world.url("stalled.bin"), "Start now");
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("stalled.bin"));
    click(&mut harness, "OK");
    if harness.query_by_label("Already in the list").is_some() {
        click(&mut harness, "Download again");
    }
    let pending = "A download with this name exists. This one will be numbered.";
    for _ in 0..100 {
        if harness.query_by_label(pending).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        harness.run_steps(1);
    }
    assert!(
        harness.query_by_label(pending).is_some(),
        "the numbering was not said"
    );
}

/// Behind Options the toolbar and the places take no clicks,
/// and a progress window takes none under a question.
#[test]
fn the_window_behind_a_dialog_takes_no_clicks() {
    use egui::accesskit::Role;
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
    click(&mut harness, "Options");
    harness
        .get_by_role_and_label(Role::Button, "Add URL")
        .click();
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.add_url.is_none(),
        "Add URL opened behind Options"
    );
    let before = harness.state().place.clone();
    harness.get_by_label("Done (0)").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().place,
        before,
        "a place was picked behind Options"
    );
    // A progress window behind Options takes none either.
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    harness.run_steps(3);
    harness.get_by_label("Stop download").click();
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_eq!(
        harness.state().rows()[0].state,
        State::Running,
        "Stop download worked behind Options"
    );
    click(&mut harness, "Cancel");

    harness.state_mut().dialogs.confirm_exit = true;
    harness.run_steps(3);
    harness.get_by_label("Stop download").click();
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_eq!(
        harness.state().rows()[0].state,
        State::Running,
        "Stop download worked under the Exit question"
    );
}

/// Find has a Close button, as every other dialog has.
#[test]
fn find_closes_with_its_close_button() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::Find);
    harness.run_steps(3);
    assert!(harness.state().dialogs.find_open);
    click(&mut harness, "Close");
    assert!(!harness.state().dialogs.find_open, "Close left Find open");
}

/// Export to a folder that is not there keeps its dialog
/// open, with the path as typed and why, rather than closing and saying
/// it only in the status bar.
#[test]
fn a_list_that_cannot_be_written_keeps_its_dialog_open() {
    let world = world();
    let mut harness = world.harness();
    let path = world.dir.path().join("no such folder").join("list.dmlist");
    harness.state_mut().dialogs.list_file = Some(dm_desktop::dialogs::ListFile {
        save: true,
        path: path.display().to_string(),
        says: String::new(),
    });
    harness.run_steps(3);
    click(&mut harness, "OK");
    let d = harness.state().dialogs.list_file.as_ref();
    assert!(d.is_some(), "the dialog closed on a failure");
    let d = d.unwrap();
    assert_eq!(d.path, path.display().to_string());
    assert!(d.says.contains("could not be written"), "{}", d.says);
}

/// Import of a plain list says how many lines it skipped for
/// having no address dm can read, rather than passing over them.
#[test]
fn import_says_the_lines_it_could_not_read() {
    let world = world();
    let mut harness = world.harness();
    let text = format!(
        "{}\n127.0.0.1:8000/notes.txt\n# a comment\n\n{}\n",
        world.url("other.pdf"),
        world.url("deep.pdf")
    );
    harness.state_mut().import_text(&text, false);
    harness.run_steps(2);
    let said = harness.state().said.shown();
    assert!(said.starts_with("Imported: 2."), "{said}");
    assert!(said.ends_with(": 1."), "{said}");
}

/// Several links copied at once are all offered, through the
/// dialog that lists them; only the first was, and the rest dropped.
#[test]
fn the_clipboard_watcher_offers_every_link_copied_together() {
    let _file = settings_file();
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut harness = world.harness();
    let mut on = harness.state().backend().settings();
    on.watch_clipboard = true;
    harness.state_mut().apply_settings(on).unwrap();
    let links = [
        world.url("file.zip"),
        world.url("other.pdf"),
        world.url("deep.pdf"),
    ];
    *world.fake.clipboard.lock().unwrap() = Some(links.join("\n"));
    harness.state_mut().check_clipboard();
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Links on your clipboard: 3")
        .is_some());
    click(&mut harness, "Download…");
    let offered: Vec<String> = harness
        .state()
        .dialogs
        .clip_links
        .as_ref()
        .expect("the links dialog did not open")
        .iter()
        .map(|l| l.0.clone())
        .collect();
    assert_eq!(offered, links);
    let mut off = harness.state().backend().settings();
    off.watch_clipboard = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// A new queue's name typed in Move to queue is the name, not
/// "Main queueNight" after the picked queue's name standing in the field.
#[test]
fn move_to_queue_makes_the_queue_named() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    select(&mut harness, "other.pdf");
    harness.state_mut().run(Command::MoveToQueue);
    harness.run_steps(3);
    type_into(&mut harness, "Or a new queue", "Night");
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows()[0].queue, "Night");
}

/// A long link on the clipboard is cut short, and its
/// Download and Dismiss stay in the window.
#[test]
fn the_clipboard_bar_keeps_its_buttons_in_the_window() {
    let _file = settings_file();
    let world = world();
    *world.fake.clipboard.lock().unwrap() = Some("old".into());
    let mut harness = world.harness();
    let mut on = harness.state().backend().settings();
    on.watch_clipboard = true;
    harness.state_mut().apply_settings(on).unwrap();
    let long = world.url(&format!("{}.zip", "a-very-long-name".repeat(8)));
    *world.fake.clipboard.lock().unwrap() = Some(long);
    harness.state_mut().check_clipboard();
    harness.run_steps(3);
    let screen = harness.ctx.content_rect();
    for name in ["Download", "Dismiss"] {
        let button = harness.get_by_label(name).rect();
        assert!(
            button.right() <= screen.right(),
            "{name} at {button:?} past {screen:?}"
        );
    }
    let mut off = harness.state().backend().settings();
    off.watch_clipboard = false;
    harness.state_mut().apply_settings(off).unwrap();
}

/// Letters typed into a number box leave its number as it was, and the box
/// says so once it is left, rather than appearing to have taken them ("abc" in a size floor silently stayed 0).
#[test]
fn a_number_box_says_when_what_was_typed_is_not_a_number() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    let label = "Time out a silent connection after (seconds)";
    let before = harness
        .state()
        .dialogs
        .options
        .as_ref()
        .map(|d| d.draft.read_timeout)
        .unwrap();
    harness.get_by_label(label).click();
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.get_by_label(label).type_text("abc");
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label("Numbers only: the one before was kept")
            .is_none(),
        "said while still typing"
    );
    harness.key_press(egui::Key::Tab);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Numbers only: the one before was kept")
            .is_some(),
        "nothing said about the letters"
    );
    let after = harness
        .state()
        .dialogs
        .options
        .as_ref()
        .map(|d| d.draft.read_timeout)
        .unwrap();
    assert_eq!(after, before, "the number changed");
    // Typed in again, the word goes.
    harness.get_by_label(label).click();
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Numbers only: the one before was kept")
        .is_none());
}

/// Someone who installed only the app is shown how to add dm to their
/// browser, one click from its store page, until a browser connects or
/// they say not now.
#[test]
fn a_new_user_is_shown_how_to_add_dm_to_their_browser() {
    let _file = settings_file();
    let seen = home().join("browser.seen");
    let hidden = home().join("browser-hint.hidden");
    let _ = std::fs::remove_file(&seen);
    let _ = std::fs::remove_file(&hidden);
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    click(&mut harness, "Microsoft Edge");
    assert!(
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o == dm_desktop::app::EXTENSION_EDGE),
        "{:?}",
        world.fake.opened.lock().unwrap()
    );
    click(&mut harness, "Not now");
    assert!(harness.query_by_label("Microsoft Edge").is_none());
    assert!(hidden.exists(), "not now is remembered");

    // A browser that has connected: nothing offered, from the start.
    std::fs::remove_file(&hidden).unwrap();
    std::fs::write(&seen, b"").unwrap();
    let mut harness = world.harness();
    harness.run_steps(2);
    let offered = harness.query_by_label("Microsoft Edge").is_some();
    std::fs::remove_file(&seen).unwrap();
    assert!(!offered, "offered to someone whose browser has connected");
}

/// With a dialog open, a person's click on the menu bar
/// opens nothing, so dialogs are not stacked by hand. (The control
/// protocol runs commands directly and may open one over another; that is
/// a script's doing.) Without the dialog, the same clicks open About.
#[test]
fn the_menus_do_not_open_dialogs_over_a_dialog() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    click(&mut harness, "Help");
    harness.run_steps(2);
    assert!(harness.query_by_label_contains("About dm").is_none());
    assert!(!harness.state().dialogs.about_open);
    assert!(harness.state().dialogs.series.is_some());

    harness.state_mut().dialogs.series = None;
    harness.run_steps(2);
    click(&mut harness, "Help");
    harness.run_steps(2);
    harness.get_by_label_contains("About dm").click();
    harness.run_steps(3);
    assert!(harness.state().dialogs.about_open);
}

/// egui outlines a focused field in the selection stroke, which showed white
/// at the rounded corners of every field, not only Options'; the look that
/// draws its own focus frame gives that stroke no width.
#[test]
fn no_field_is_outlined_in_the_selection_stroke_where_the_look_draws_its_own() {
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    let modern = dm_desktop::theme::Tokens::get(&harness.ctx).modern;
    let width = harness.ctx.global_style().visuals.selection.stroke.width;
    assert_eq!(width == 0.0, modern, "selection stroke width {width}");
}
