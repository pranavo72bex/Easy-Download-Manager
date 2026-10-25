use crate::support::*;

#[test]
fn options_are_put_into_force_on_ok_and_written_down() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    click(&mut harness, "Sort finished files into folders by kind");
    click(&mut harness, "OK");
    assert!(harness.state().backend().settings().categorize);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
    assert_eq!(saved["categorize"], true);
}

#[test]
fn a_category_made_in_options_takes_its_files_into_its_own_folder() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    click(&mut harness, "Sort finished files into folders by kind");
    // At the foot of the page, scrolled to as a person would.
    harness.get_by_label("Add a category").scroll_to_me();
    harness.run_steps(3);
    click(&mut harness, "Add a category");
    type_into(&mut harness, "Category", "Lectures");
    type_into(&mut harness, "Types", "zip");
    type_into(&mut harness, "Sites", "127.0.0.1");
    harness.run_steps(2);
    click(&mut harness, "OK");
    let made = harness.state().backend().settings().custom_categories;
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].extensions, ["zip"]);

    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = harness.state().rows()[0].clone();
    assert!(
        std::path::Path::new(&row.path)
            .parent()
            .unwrap()
            .ends_with("Lectures"),
        "{} is not in the category's folder",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap(), payload());
    harness.run_steps(2);
    click(&mut harness, "Lectures (1)");
    assert_eq!(harness.state().visible().len(), 1);

    // Deleted, it takes nothing more.
    click(&mut harness, "Options");
    click(&mut harness, "Folders");
    // Under the page's fold, scrolled to as a person would.
    harness.get_by_label("Delete category").scroll_to_me();
    harness.run_steps(3);
    click(&mut harness, "Delete category");
    click(&mut harness, "OK");
    assert!(harness
        .state()
        .backend()
        .settings()
        .custom_categories
        .is_empty());
}

#[test]
fn the_file_types_the_browser_hands_over_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Browser")
        .click();
    harness.run_steps(3);
    use egui::accesskit::Role;
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "File types")
        .focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_role_and_label(Role::MultilineTextInput, "File types")
        .type_text("ZIP .iso, mkv");
    harness.run_steps(2);
    // Scrolled to, as a person scrolls to a field below the fold: how far
    // down it is depends on the computer's type.
    harness
        .get_by_label("Never take downloads from these sites")
        .scroll_to_me();
    harness.run();
    type_into(
        &mut harness,
        "Never take downloads from these sites",
        "intranet.example",
    );
    // Sites whose links work once.
    harness
        .get_by_label("Sites whose links work only once")
        .scroll_to_me();
    harness.run();
    type_into(
        &mut harness,
        "Sites whose links work only once",
        "*.once.example",
    );
    click(&mut harness, "OK");
    let catch = harness.state().backend().settings().catch;
    assert_eq!(catch.extensions, ["zip", "iso", "mkv"]);
    assert_eq!(catch.skip_hosts, ["intranet.example"]);
    assert_eq!(catch.once_hosts, ["*.once.example"]);
    // What the extension is sent is what was typed.
    let rules = dm_protocol::CatchRules { ..catch };
    assert!(rules.enabled);
}

#[test]
fn site_limits_attempts_and_the_timeout_are_set_in_options_and_reach_the_queue() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    assert!(harness
        .query_by_label("Attempts per connection (0 to keep trying)")
        .is_some());
    assert!(harness
        .query_by_label("Time out a silent connection after (seconds)")
        .is_some());
    click(&mut harness, "Add a site limit");
    type_into(&mut harness, "Site", "Slow.Example");
    // The numbers are set on the draft, as dragging them would.
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.attempts = 0;
        d.draft.read_timeout = 10;
        if let Some(limit) = d.draft.host_limits.first_mut() {
            limit.connections = 2;
        }
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let settings = harness.state().backend().settings();
    assert_eq!((settings.attempts, settings.read_timeout), (0, 10));
    assert_eq!(
        settings.host_limits,
        [dm_core::HostLimit {
            host: "slow.example".into(),
            connections: 2
        }]
    );
    // In force on the queue, not only written down.
    let options = world
        .runtime
        .block_on(harness.state().backend().queue.options());
    assert_eq!(options.attempts, 0);
    assert_eq!(options.host_limits.len(), 1);
    assert_eq!(
        options.connections_for("https://dl.slow.example/f"),
        1,
        "these tests run on one connection, and a site's limit never raises that"
    );
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
    assert_eq!(saved["read_timeout"], 10);
    assert_eq!(saved["host_limits"][0]["host"], "slow.example");
    // The limit can be taken off again.
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    click(&mut harness, "Delete site limit");
    click(&mut harness, "OK");
    assert!(harness.state().backend().settings().host_limits.is_empty());
}

#[test]
fn the_recording_limit_is_set_in_options_and_reaches_the_queue() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.tab = dm_desktop::dialogs::Tab::Downloads;
    }
    harness.run_steps(2);
    assert!(harness
        .query_by_label("Stop recording after (minutes, 0 for no limit)")
        .is_some());
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.record_minutes = 90;
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert_eq!(harness.state().backend().settings().record_minutes, 90);
    let options = world
        .runtime
        .block_on(harness.state().backend().queue.options());
    assert_eq!(
        options.record_limit,
        Some(std::time::Duration::from_secs(90 * 60)),
        "in force on the queue, not only written down"
    );
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
    assert_eq!(saved["record_minutes"], 90);
}

#[test]
fn the_proxys_bypass_list_and_the_recheck_timer_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    type_into(&mut harness, "Host", "proxy.example");
    type_into(
        &mut harness,
        "Not for these sites (fetched directly)",
        "Intranet.example 10.0.0.5",
    );
    // The recheck timer lives on the Downloads tab; the draft is one
    // across the tabs, so it is set here as dragging it there would.
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.recheck_hours = 6;
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let settings = harness.state().backend().settings();
    assert!(settings.proxy.enabled);
    assert_eq!(settings.proxy.host, "proxy.example");
    assert_eq!(settings.proxy.bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(settings.recheck_hours, 6);
    let options = world
        .runtime
        .block_on(harness.state().backend().queue.options());
    assert_eq!(options.proxy_bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(options.proxy_for("http://files.intranet.example/a"), None);
    assert!(options.proxy_for("http://outside.example/a").is_some());
    // Put back, so the other tests' downloads do not go looking for it.
    click(&mut harness, "Options");
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    click(&mut harness, "OK");
    assert!(!harness.state().backend().settings().proxy.enabled);
}

/// Options stays inside the window at large text sizes: at 200% in a
/// 960×660 window, and at 150% in the smallest, its tabs and OK were off
/// the window's left edge.
#[test]
fn options_fits_the_window_at_large_text() {
    use egui::accesskit::Role;
    for (zoom, size) in [
        (2.0, egui::vec2(960.0, 660.0)),
        (1.5, egui::vec2(720.0, 460.0)),
    ] {
        let world = world();
        let mut harness = Harness::builder()
            .with_size(size)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(3);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        let points = size / zoom;
        for name in ["General", "OK"] {
            if name == "OK" {
                // In a window too short for the buttons beside the settings
                // the dialog scrolls as one page: its end is
                // brought into view as a person would, with the wheel.
                let general = harness
                    .get_by_role_and_label(Role::Button, "General")
                    .rect();
                harness.hover_at(general.center() + egui::vec2(0.0, 40.0));
                for _ in 0..10 {
                    harness.event(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -200.0),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Wheel,
                        modifiers: egui::Modifiers::NONE,
                    });
                    harness.run_steps(1);
                }
                harness.run_steps(3);
            }
            let at = harness
                .query_all_by_label(name)
                .find(|n| {
                    matches!(
                        n.accesskit_node().role(),
                        Role::Button | Role::Tab | Role::Label | Role::ListBoxOption
                    )
                })
                .unwrap_or_else(|| harness.get_by_label(name))
                .rect();
            assert!(
                at.left() >= 0.0
                    && at.right() <= points.x
                    && at.top() >= 0.0
                    && at.bottom() <= points.y,
                "{zoom}x {size:?}: {name} at {at:?} is outside the window"
            );
        }
    }
}

/// A queue's own daily start time is set in Options › Downloads; one that
/// is not a time is said beside it and refused.
#[test]
fn a_queues_start_time_is_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.tab = dm_desktop::dialogs::Tab::Downloads;
    }
    harness.run_steps(2);
    // Scrolled to, as a person scrolls to it.
    harness.get_by_label("Add a start time").scroll_to_me();
    harness.run();
    click(&mut harness, "Add a start time");
    harness.run_steps(2);
    harness
        .get_by_label_contains("Starts at (HH:MM)")
        .scroll_to_me();
    harness.run();
    type_role(&mut harness, "Starts at (HH:MM)", "x");
    assert!(harness.query_by_label("Not a time").is_some());
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.queue_starts[0].at = "23:45".into();
    }
    harness.run_steps(2);
    assert!(harness.query_by_label("Not a time").is_none());
    click(&mut harness, "OK");
    let settings = harness.state().backend().settings();
    assert_eq!(
        settings.queue_start_list(),
        [(dm_core::MAIN_QUEUE.to_string(), 23 * 60 + 45)]
    );
    // Put back for the other tests.
    let mut wanted = settings;
    wanted.queue_starts.clear();
    harness.state_mut().apply_settings(wanted).unwrap();
}

#[test]
fn a_queues_own_file_limit_and_the_parts_folder_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.tab = dm_desktop::dialogs::Tab::Downloads;
    }
    harness.run_steps(2);
    click(&mut harness, "Add a queue limit");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        if let Some(limit) = d.draft.queue_limits.first_mut() {
            limit.files = 2;
        }
        d.tab = dm_desktop::dialogs::Tab::SaveTo;
    }
    harness.run_steps(2);
    let staged = world.dir.path().join("staging");
    type_into(
        &mut harness,
        "Folder for unfinished downloads",
        &staged.display().to_string(),
    );
    click(&mut harness, "OK");
    let settings = harness.state().backend().settings();
    assert_eq!(
        settings.queue_limits,
        [dm_shell::settings::QueueLimit {
            queue: dm_core::MAIN_QUEUE.into(),
            files: 2
        }]
    );
    assert_eq!(settings.parts_dir(), staged);
    // In force on the queue.
    assert_eq!(
        harness.state().backend().queue.queue_limits(),
        std::collections::HashMap::from([(dm_core::MAIN_QUEUE.to_string(), 2)])
    );
    let options = world
        .runtime
        .block_on(harness.state().backend().queue.options());
    assert_eq!(options.parts_dir, staged);
    // Put back for the other tests.
    let mut wanted = harness.state().backend().settings();
    wanted.queue_limits.clear();
    wanted.parts_dir.clear();
    harness.state_mut().apply_settings(wanted).unwrap();
}

#[test]
fn an_option_that_cannot_be_applied_is_said_and_what_was_typed_is_kept() {
    /* A failed OK used to reopen Options on the settings in force, which
     * threw away every edit in the dialog to say what was wrong with one.
     * The edits stay, with the reason, and OK again once the cause is gone
     * puts them in force. */
    let _file = settings_file();
    let world = world();
    *world.fake.login_fails.lock().unwrap() = true;
    let mut harness = world.harness();
    click(&mut harness, "Options");
    harness
        .get_by_label_contains("Start dm when I sign in")
        .click();
    // Scrolled to, as a person scrolls to it.
    harness
        .get_by_label_contains("Play a sound when a download finishes")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_label_contains("Play a sound when a download finishes")
        .click();
    harness.run_steps(2);
    click(&mut harness, "OK");
    let held = harness.state().backend().settings();
    assert!(!held.start_on_login);
    assert!(
        !held.sound_on_complete,
        "half of the change was put in force"
    );
    assert!(harness
        .query_by_label_contains("could not change start-on-login")
        .is_some());
    for typed in [
        "Start dm when I sign in",
        "Play a sound when a download finishes",
    ] {
        assert_eq!(
            harness
                .get_by_label_contains(typed)
                .accesskit_node()
                .toggled(),
            Some(egui::accesskit::Toggled::True),
            "{typed}: the edit was thrown away"
        );
    }

    *world.fake.login_fails.lock().unwrap() = false;
    click(&mut harness, "OK");
    let held = harness.state().backend().settings();
    assert!(held.start_on_login && held.sound_on_complete);
    assert!(harness.state().dialogs.options.is_none());
}

#[test]
fn a_bad_category_options_opened_with_does_not_fail_an_unrelated_ok() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    let bad = dm_core::Custom {
        name: "Work/Docs".into(),
        extensions: vec!["pdf".into()],
        ..Default::default()
    };
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        // As if an older dm had saved it.
        d.opened.custom_categories.push(bad);
        d.custom
            .push(["Work/Docs".into(), "pdf".into(), String::new()]);
        d.draft.sound_on_failure = !d.draft.sound_on_failure;
    }
    let wanted = !harness.state().backend().settings().sound_on_failure;
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_none(),
        "the OK was refused"
    );
    assert_eq!(
        harness.state().backend().settings().sound_on_failure,
        wanted
    );
    let mut back = harness.state().backend().settings();
    back.sound_on_failure = !wanted;
    harness.state_mut().apply_settings(back).unwrap();
}

#[test]
fn options_refuses_a_category_that_is_not_one_folder_and_a_relative_download_folder() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let before = harness.state().backend().settings();
    click(&mut harness, "Options");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.custom
            .push(["Work/Docs".into(), "pdf".into(), String::new()]);
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("cannot be a category")
        .is_some());
    assert_eq!(
        harness.state().backend().settings().custom_categories,
        before.custom_categories
    );

    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.custom.clear();
        d.draft.download_dir = "Downloads/dm".into();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("must be a full path")
        .is_some());
    assert_eq!(
        harness.state().backend().settings().download_dir,
        before.download_dir
    );

    // From the home folder is a full path, and is expanded.
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.download_dir = "~/dm-test-downloads".into();
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert_eq!(
        harness.state().backend().settings().download_dir,
        dm_shell::settings::full_path("~/dm-test-downloads").unwrap()
    );
    let mut back = harness.state().backend().settings();
    back.download_dir = before.download_dir;
    harness.state_mut().apply_settings(back).unwrap();
}

#[test]
fn options_holds_only_new_or_renamed_folders_to_every_computers_rules() {
    let _file = settings_file();
    let world = world();
    // Saved before by a dm that took them, and usable on this computer: a
    // kind's folder and a category with a colon in it.
    let mut saved = Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: false,
        show_complete: false,
        language: "en".into(),
        ..Settings::default()
    };
    saved.category_folders[1] = "Audio: 2024".into();
    saved.custom_categories.push(dm_core::Custom {
        name: "Live: 2024".into(),
        extensions: vec!["mkv".into()],
        sites: Vec::new(),
    });
    let before = saved.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app_with(saved));
    // An unrelated change goes through.
    click(&mut harness, "Options");
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .connections = 4;
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_none(),
        "a name saved before blocked OK"
    );
    let now = harness.state().backend().settings();
    assert_eq!(now.connections, 4);
    assert_eq!(
        now.category_folders[1], "Audio: 2024",
        "a saved folder kept"
    );
    assert!(now.custom_categories.iter().any(|c| c.name == "Live: 2024"));
    // A kind's folder renamed to one Windows cannot make is refused.
    click(&mut harness, "Options");
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .category_folders[0] = "Clips|new".into();
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness
            .query_by_label_contains("not a folder name every computer can use")
            .is_some(),
        "{:?} {:?}",
        harness
            .state()
            .dialogs
            .options
            .as_ref()
            .map(|d| d.says.clone()),
        harness.state().backend().settings().category_folders
    );
    assert_eq!(
        harness.state().backend().settings().category_folders[0],
        before.category_folders[0]
    );
    // And a new category.
    {
        let d = harness.state_mut().dialogs.options.as_mut().unwrap();
        d.draft.category_folders[0] = before.category_folders[0].clone();
        d.custom
            .push(["Notes?".into(), "txt".into(), String::new()]);
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_some(),
        "a new bad name was taken"
    );
    harness.state_mut().dialogs.options = None;
}

/* ------------------------------------------- a human tester's session (6) */

/// The sort is remembered, a status line naming a row goes with
/// it, and Options keeps its place whichever tab shows.
#[test]
fn the_sort_survives_a_restart_and_options_keeps_its_place() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(2);
    click(&mut harness, "Size");
    click(&mut harness, "Size");
    let saved = harness.state().backend().settings();
    assert_eq!(
        (saved.sort_column.as_str(), saved.sort_ascending),
        ("size", false)
    );
    let again = world.app_with(saved);
    assert_eq!(again.sort, Some((Column::Size, false)), "not remembered");
    // A third click puts the list back in the queue's order.
    click(&mut harness, "Size");
    assert_eq!(harness.state().sort, None);
    assert_eq!(harness.state().backend().settings().sort_column, "");
    click(&mut harness, "Size");
    click(&mut harness, "Size");

    // "Found file.zip." does not outlive file.zip.
    harness.state_mut().dialogs.find_text = "file".into();
    harness.state_mut().run(Command::FindNext);
    assert!(harness.state().said.english().contains("file.zip"));
    let id = harness.state().rows()[0].id;
    harness.state_mut().remove_now(&[id]);
    assert!(
        harness.state().said.is_empty(),
        "{:?}",
        harness.state().said
    );

    click(&mut harness, "Options");
    let ok = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(egui::accesskit::Role::Button, "OK")
            .accesskit_node()
            .bounding_box()
    };
    let first = ok(&harness);
    for tab in ["Folders", "Network", "Sign-ins", "Downloads", "General"] {
        // The tab, not the menu of that name: Options is drawn after it.
        harness.query_all_by_label(tab).last().unwrap().click();
        harness.run_steps(3);
        assert_eq!(ok(&harness), first, "Options moved on {tab}");
    }
    let mut wanted = harness.state().applied.clone();
    wanted.sort_column = String::new();
    harness.state_mut().apply_settings(wanted).unwrap();
}

/// OK refuses a proxy turned on with no host, or with an address
/// where its host goes, and says why on the Proxy tab; nothing is put in force.
#[test]
fn options_refuses_a_proxy_with_no_host() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    click(&mut harness, "OK");
    assert!(
        harness.state().dialogs.options.is_some(),
        "OK took a proxy with no host"
    );
    assert!(harness
        .query_by_label_contains("Enter the proxy's host")
        .is_some());
    assert!(!harness.state().backend().settings().proxy.enabled);
    type_into(&mut harness, "Host", "http://proxy.example:3128");
    click(&mut harness, "OK");
    assert!(harness
        .query_by_label_contains("isn't a valid proxy host")
        .is_some());
    assert!(!harness.state().backend().settings().proxy.enabled);
    click(&mut harness, "Cancel");
}

/// At 200% text every tab of Options stays inside the window
/// and the dialog does not move from one tab to the next; Browser's long
/// rows pushed it off the window's left edge, OK past the right.
#[test]
fn every_options_tab_stays_put_inside_the_window_at_large_text() {
    use dm_desktop::dialogs::TABS;
    use egui_kittest::kittest::By;
    for (zoom, size) in [
        (2.0, egui::vec2(960.0, 660.0)),
        (1.5, egui::vec2(720.0, 460.0)),
        // Shorter still (OK and Cancel lay over the settings).
        (2.0, egui::vec2(720.0, 560.0)),
        (2.0, egui::vec2(720.0, 460.0)),
        (3.0, egui::vec2(1280.0, 800.0)),
    ] {
        let world = world();
        let mut harness = Harness::builder()
            .with_size(size)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(3);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        let points = size / zoom;
        // Where the buttons are in view: in a window too short for them
        // beside the settings, they come at the end of a page that scrolls.
        let ok_at = |h: &Harness<'_, DmApp>| {
            h.query_by_role_and_label(egui::accesskit::Role::Button, "OK")
                .map(|n| n.rect())
        };
        let first = ok_at(&harness);
        for (tab, label) in TABS {
            if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
                d.tab = tab;
                // A row in each list, so the widest rows are there.
                d.draft.queue_limits.push(Default::default());
                d.draft.host_limits.push(dm_core::HostLimit {
                    host: String::new(),
                    connections: 1,
                });
                d.draft.site_logins.push(Default::default());
            }
            harness.run_steps(6);
            let ok = ok_at(&harness);
            // Put, where the window has room for the buttons beside the
            // settings (the first two sizes); in a shorter one the page
            // scrolls and its end is where each tab's settings end.
            let roomy = size.y >= 660.0 && zoom <= 2.0;
            if let (Some(ok), Some(first), true) = (ok, first, roomy) {
                assert!(
                    (ok.center() - first.center()).length() < 1.0,
                    "{zoom}x {size:?}: OK moved from {first:?} to {ok:?} on {label}"
                );
            }
            // Options' own: a node with the dialog among its ancestors.
            let in_options = |n: &egui_kittest::kittest::AccessKitNode<'_>| {
                let mut up = n.parent();
                while let Some(p) = up {
                    if p.label().as_deref() == Some("Options") {
                        // The tabs scroll sideways by design.
                        let tab = TABS.iter().any(|(_, l)| n.label().as_deref() == Some(*l));
                        // A field's text runs on inside it, clipped there.
                        let inside_a_field = n.role() == egui::accesskit::Role::TextRun;
                        return n.bounding_box().is_some() && !tab && !inside_a_field;
                    }
                    up = p.parent();
                }
                false
            };
            let mut seen = 0;
            for node in harness.query_all(By::new().predicate(in_options)) {
                seen += 1;
                let at = node.rect();
                // Cut by a scroll area is fine; drawn past the window is not.
                assert!(
                    at.left() >= -0.5 && at.right() <= points.x + 0.5,
                    "{zoom}x {size:?}: on {label}, {:?} {:?} (in {:?}) at {at:?} runs past the window",
                    node.accesskit_node().role(),
                    node.accesskit_node().label(),
                    node.accesskit_node().parent().map(|p| (p.role(), p.label()))
                );
            }
            assert!(seen > 5, "{label}: Options' own controls were not found");
            // The first setting above the buttons, not under them (OK and Cancel were drawn over the Language row).
            if let (Some(ok), dm_desktop::dialogs::Tab::General) = (ok, tab) {
                let language = harness
                    .query_all_by_label("Language")
                    .map(|n| n.rect())
                    .next_back()
                    .expect("General's Language");
                assert!(
                    language.bottom() <= ok.top(),
                    "{zoom}x {size:?}: OK at {ok:?} lies over Language at {language:?}"
                );
            }
        }
    }
}

/// Options opens at its top however far down it was left,
/// in a window with room for its buttons and in one where it scrolls as a
/// page.
#[test]
fn options_opens_at_its_top_wherever_it_was_left() {
    use egui::accesskit::Role;
    for (zoom, size) in [
        (1.0, egui::vec2(800.0, 480.0)),
        (2.0, egui::vec2(720.0, 460.0)),
    ] {
        let world = world();
        let mut harness = Harness::builder()
            .with_size(size)
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(3);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        let general = harness
            .get_by_role_and_label(Role::Button, "General")
            .rect();
        // Over what scrolls: the settings below the tabs where they scroll
        // on their own, the tabs themselves where the page scrolls.
        harness.hover_at(if zoom > 1.0 {
            general.center() + egui::vec2(0.0, 12.0)
        } else {
            egui::pos2(general.center().x, general.bottom() + 60.0)
        });
        for _ in 0..15 {
            harness.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -300.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run_steps(1);
        }
        harness.run_steps(3);
        // Below the dialog's title bar, where its scrolled part is seen.
        let language_seen = |h: &Harness<'_, DmApp>| {
            let dialog = h.get_by_role_and_label(Role::Window, "Options").rect();
            // The first thing the page holds: Language where there is room
            // for a row of settings, the tabs where there is not.
            h.query_all_by_label(if zoom > 1.0 { "General" } else { "Language" })
                .map(|n| n.rect())
                .any(|at| at.top() >= dialog.top() + 40.0 && at.bottom() <= size.y / zoom)
        };
        assert!(
            !language_seen(&harness),
            "{zoom}x: the premise, scrolled past Language"
        );
        harness.state_mut().dialogs.options = None;
        harness.run_steps(2);
        harness.state_mut().run(Command::Options);
        harness.run_steps(6);
        assert!(
            language_seen(&harness),
            "{zoom}x {size:?}: reopened scrolled"
        );
    }
}

/// At 150% text in the window's first size, Options' tabs all fit
/// inside the dialog; Sign-ins stood 19 points past its edge.
#[test]
fn options_tabs_fit_at_150_percent() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 660.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    harness.ctx.set_zoom_factor(1.5);
    harness.run_steps(3);
    harness.state_mut().run(Command::Options);
    harness.run_steps(6);
    let edge = harness.get_by_label("Close window").rect().right();
    let first = harness
        .get_by_role_and_label(Role::Button, "General")
        .rect();
    let last = harness
        .get_by_role_and_label(Role::Button, "Sign-ins")
        .rect();
    assert!(
        last.right() <= edge + 0.5,
        "Sign-ins ends at {} past {edge}",
        last.right()
    );
    assert!(first.left() >= 40.0, "General moved: {first:?}");
}

#[test]
fn the_command_line_setting_is_in_options_off_by_default_and_saved() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    assert!(!harness.state().backend().settings().allow_command_line);
    click(&mut harness, "Options");
    assert!(harness
        .query_by_label_contains("Allow the command line to control dm")
        .is_some());
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        d.draft.allow_command_line = true;
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    assert!(harness.state().backend().settings().allow_command_line);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
    assert_eq!(saved["allow_command_line"], true);
}

/// Options › Network: a site's rule gives its downloads a folder and a speed
/// limit of their own, from the moment they are added by any way in.
#[test]
fn a_site_rule_set_in_options_sends_that_sites_downloads_to_its_folder_at_its_speed() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let folder = tempfile::tempdir().unwrap();
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    assert!(harness.query_by_label("Add a site rule").is_some());
    click(&mut harness, "Add a site rule");
    if let Some(d) = harness.state_mut().dialogs.options.as_mut() {
        let rule = d.draft.site_rules.last_mut().expect("the rule row");
        rule.host = "127.0.0.1".into();
        rule.folder = folder.path().display().to_string();
        rule.speed = 4096;
    }
    harness.run_steps(2);
    click(&mut harness, "OK");
    let kept = harness.state().backend().settings().site_rules;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].speed, 4096);

    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the file to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    let done = row(&harness, "file.zip");
    assert_eq!(done.speed_limit, 4096, "its own speed limit, from the rule");
    assert_eq!(done.save_dir.as_deref(), folder.path().to_str());
    assert!(
        std::path::Path::new(&done.path).starts_with(folder.path()),
        "it landed in the site's folder: {}",
        done.path
    );

    // The rule can be deleted again.
    click(&mut harness, "Options");
    click(&mut harness, "Network");
    click(&mut harness, "Delete site rule");
    click(&mut harness, "OK");
    assert!(harness.state().backend().settings().site_rules.is_empty());
}

#[test]
fn appearance_sets_the_theme_and_the_size_of_text_for_the_whole_window() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");
    harness.get_by_label("Appearance").scroll_to_me();
    harness.run();
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
    // Medium is the design; Large and Small scale everything.
    open_choice(&mut harness, "Text size");
    click(&mut harness, "Large");
    harness.run_steps(2);
    assert!((harness.ctx.zoom_factor() - 1.2).abs() < 0.001);
    open_choice(&mut harness, "Text size");
    click(&mut harness, "Small");
    harness.run_steps(2);
    assert!((harness.ctx.zoom_factor() - 0.85).abs() < 0.001);
    // The theme is a choice put in force on OK, as before.
    click(&mut harness, "Dark");
    // Shown as it is chosen, before OK.
    harness.run_steps(3);
    assert!(harness.state().dark, "the dark look is not shown");
    click(&mut harness, "Light");
    harness.run_steps(3);
    assert!(!harness.state().dark, "the light look is not shown");
    click(&mut harness, "Dark");
    click(&mut harness, "OK");
    assert_eq!(harness.state().backend().settings().theme, "dark");
    assert!(harness.state().dark);
}

/// Opens the drop-down a row names, as a click on it does.
fn open_choice(harness: &mut Harness<'_, DmApp>, name: &str) {
    harness
        .query_all_by_label(name)
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox)
        .unwrap_or_else(|| panic!("no drop-down named {name}"))
        .click();
    harness.run_steps(2);
}
