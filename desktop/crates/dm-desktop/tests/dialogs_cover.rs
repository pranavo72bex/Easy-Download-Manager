// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Playlist dialog, Properties, the Grabber and the tabs of Options,
//! driven as a person drives them, by the words on screen, against a real
//! queue fetching from a loopback server: what each shows, and what its
//! buttons, ticks and drop-downs do.

#[path = "window/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use egui::accesskit::Role;
use support::*;

/// The world every test here runs in, with the bar that offers the browser
/// extension put away for good: one test says a browser has talked to dm,
/// which would take the bar away under the others' clicks.
fn world() -> World {
    let world = support::world();
    let hidden = dm_shell::settings::home().join("browser-hint.hidden");
    if !hidden.exists() {
        std::fs::write(hidden, "").unwrap();
    }
    world
}

/// Replaces what a text field holds with `text`, as a person selects all
/// and types over it.
fn retype(harness: &mut Harness<'_, DmApp>, label: &str, text: &str) {
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .scroll_to_me();
    harness.run_steps(3);
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .click();
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(1);
    harness
        .get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    harness.run_steps(2);
}

/// Opens the drop-down of that name and picks `choice` in it.
fn choose(harness: &mut Harness<'_, DmApp>, combo: &str, choice: &str) {
    harness
        .get_by_role_and_label(Role::ComboBox, combo)
        .scroll_to_me();
    harness.run_steps(3);
    harness.get_by_role_and_label(Role::ComboBox, combo).click();
    harness.run_steps(3);
    assert!(harness.ctx.any_popup_open(), "{combo} did not open");
    harness
        .query_all_by_label(choice)
        .last()
        .unwrap_or_else(|| panic!("{choice} is not in {combo}"))
        .click();
    harness.run_steps(3);
}

/// What the status bar says.
fn said(harness: &Harness<'_, DmApp>) -> String {
    harness.state().said.shown()
}

// --------------------------------------------------------------- Playlist

// ------------------------------------------------------------- Properties

// ---------------------------------------------------------------- Grabber

fn grabber<'h>(harness: &'h mut Harness<'_, DmApp>) -> &'h mut dm_desktop::dialogs::SiteFiles {
    harness
        .state_mut()
        .dialogs
        .site_files
        .as_mut()
        .expect("the Grabber")
}

/// A start page whose site Options › Sign-ins keeps a login for says the
/// search signs in with it; a template fills in the file types.
#[test]
fn the_grabber_says_it_signs_in_and_a_template_fills_the_types() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    let mut settings = harness.state().backend().settings();
    settings.site_logins = vec![dm_shell::settings::SiteLogin {
        host: "127.0.0.1".into(),
        username: "reader".into(),
        password: ["made", "at", "run", "time"].join("-"),
    }];
    harness.state_mut().apply_settings(settings).unwrap();
    harness.state_mut().dialogs.open_site_files();
    harness.run_steps(2);
    assert!(harness
        .query_by_label("Signed in with the login Options › Sign-ins keeps for this site.")
        .is_none());
    type_into(&mut harness, "Start page", &world.url("page.html"));
    assert!(harness
        .query_by_label("Signed in with the login Options › Sign-ins keeps for this site.")
        .is_some());
    choose(&mut harness, "Template", "Documents");
    assert_eq!(
        grabber(&mut harness).types,
        "pdf doc docx xls xlsx ppt pptx odt ods odp rtf txt epub"
    );
    choose(&mut harness, "Template", "Every file");
    assert_eq!(grabber(&mut harness).types, "");
    let mut back = harness.state().backend().settings();
    back.site_logins.clear();
    harness.state_mut().apply_settings(back).unwrap();
}

/// A search is saved only with a name and a start page; saved again under
/// its name it keeps when it last ran; one that cannot be saved or deleted
/// says why in the dialog.
#[test]
fn a_grabber_search_is_saved_only_whole_and_refusals_are_said() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.open_site_files();
    harness.run_steps(2);
    click(&mut harness, "Save search");
    assert!(harness
        .query_by_label("A saved search needs a name.")
        .is_some());
    type_into(&mut harness, "Save this search as", "Weekly");
    click(&mut harness, "Save search");
    assert!(harness
        .query_by_label("A saved search needs a start page.")
        .is_some());
    type_into(&mut harness, "Start page", &world.url("page.html"));
    click(&mut harness, "Save search");
    let saved = harness.state().backend().settings().site_searches;
    assert_eq!(saved.len(), 1);
    assert!(grabber(&mut harness).says.is_empty());
    // As if it ran a day ago: saved again, it keeps that.
    let mut settings = harness.state().backend().settings();
    settings.site_searches[0].last_run -= 86_400;
    let ran = settings.site_searches[0].last_run;
    harness.state_mut().apply_settings(settings).unwrap();
    click(&mut harness, "Save search");
    let saved = harness.state().backend().settings().site_searches;
    assert_eq!(saved.len(), 1, "saved twice under one name");
    assert_eq!(saved[0].last_run, ran);

    // Settings that cannot be saved (a download folder that is not a full
    // path): the dialog says so, for Save and for Delete.
    let before = harness.state().applied.download_dir.clone();
    harness.state_mut().applied.download_dir = "relative/folder".into();
    click(&mut harness, "Save search");
    let says = grabber(&mut harness).says.clone();
    assert!(says.contains("full path"), "{says}");
    assert!(harness.query_by_label(&says).is_some());
    assert_eq!(harness.state().applied.download_dir, before, "put back");
    grabber(&mut harness).says.clear();
    harness.state_mut().applied.download_dir = "relative/folder".into();
    click(&mut harness, "Delete search");
    assert!(grabber(&mut harness).says.contains("full path"));
    assert_eq!(harness.state().backend().settings().site_searches.len(), 1);
    click(&mut harness, "Delete search");
    assert!(harness
        .state()
        .backend()
        .settings()
        .site_searches
        .is_empty());
}

/// What a search found is said: nothing of those types, a page that could
/// not be read, a run that ran out of time; while it reads, it says so;
/// Close closes the dialog.
#[test]
fn the_grabber_says_what_a_search_found() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.open_site_files();
    harness.run_steps(2);
    type_into(&mut harness, "Start page", &world.url("page.html"));
    retype(&mut harness, "File types", "xyz");
    click(&mut harness, "Find files");
    until(&mut harness, "the search to end", |app| {
        app.dialogs.site_files.as_ref().is_some_and(|d| {
            matches!(
                *dm_core::lock(&d.state),
                dm_desktop::dialogs::Grab::Found(..)
            )
        })
    });
    harness.run_steps(2);
    assert!(harness
        .query_by_label("No files of those types are linked from that page.")
        .is_some());

    // A page no server answers for.
    retype(
        &mut harness,
        "Start page",
        "http://127.0.0.1:9/nothing.html",
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    until(&mut harness, "the search to fail", |app| {
        app.dialogs.site_files.as_ref().is_some_and(|d| {
            matches!(
                *dm_core::lock(&d.state),
                dm_desktop::dialogs::Grab::Failed(_)
            )
        })
    });
    harness.run_steps(2);
    let why = match &*dm_core::lock(&grabber(&mut harness).state) {
        dm_desktop::dialogs::Grab::Failed(why) => why.clone(),
        _ => unreachable!(),
    };
    assert!(harness.query_by_label(&why).is_some());

    // A run that ran out of time says the files it found are not all.
    *dm_core::lock(&grabber(&mut harness).state) =
        dm_desktop::dialogs::Grab::Found(vec![(world.url("file.zip"), true)], 4, true);
    harness.run_steps(2);
    assert!(harness
        .query_by_label(
            "It ran out of time before reading every page. These are the files it found."
        )
        .is_some());
    assert!(harness
        .query_by_label_contains("Files found: 1 · pages read: 4")
        .is_some());

    // A page that never answers: it reads on.
    retype(&mut harness, "Start page", &world.url("stalled.bin"));
    click(&mut harness, "Find files");
    assert!(harness.query_by_label("Reading the page…").is_some());
    click(&mut harness, "Close");
    assert!(harness.state().dialogs.site_files.is_none());
}

// ---------------------------------------------------------------- Options

fn open_options_at(harness: &mut Harness<'_, DmApp>, tab: dm_desktop::dialogs::Tab) {
    let settings = harness.state().applied.clone();
    harness.state_mut().dialogs.open_options(&settings);
    harness.state_mut().dialogs.options.as_mut().unwrap().tab = tab;
    harness.run_steps(4);
    assert!(
        harness.query_by_label("Restore defaults").is_some()
            || tab == dm_desktop::dialogs::Tab::SiteLogins
    );
}

fn draft<'h>(harness: &'h Harness<'_, DmApp>) -> &'h Settings {
    &harness
        .state()
        .dialogs
        .options
        .as_ref()
        .expect("Options")
        .draft
}

fn options_says(harness: &Harness<'_, DmApp>) -> String {
    harness
        .state()
        .dialogs
        .options
        .as_ref()
        .expect("Options")
        .says
        .clone()
}

/// General: what closing the window does and what an address already in the
/// list does are picked from their lists and kept on OK; the language list
/// picks a language; a sound of one's own is played with Play.
#[test]
fn general_picks_what_closing_and_a_repeated_address_do_and_plays_a_sound() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    open_options_at(&mut harness, dm_desktop::dialogs::Tab::General);
    choose(&mut harness, "Language", "English");
    assert_eq!(draft(&harness).language, "en");
    choose(&mut harness, "When I close the window", "Exit dm");
    assert_eq!(draft(&harness).on_close, "exit");
    choose(
        &mut harness,
        "When I close the window",
        "Keep running in the tray",
    );
    assert_eq!(draft(&harness).on_close, "tray");
    choose(
        &mut harness,
        "When an address is already in the list",
        "Show it",
    );
    assert_eq!(draft(&harness).duplicate_choice, "show");
    choose(
        &mut harness,
        "When an address is already in the list",
        "Replace the old file",
    );
    assert_eq!(draft(&harness).duplicate_choice, "replace");

    // A sound of one's own: Play only once there is a file named.
    let play = |h: &Harness<'_, DmApp>| {
        h.query_all_by_role_and_label(Role::Button, "Play")
            .next()
            .unwrap()
            .accesskit_node()
            .is_disabled()
    };
    assert!(play(&harness), "Play with no sound named");
    let sound = world.dir.path().join("done.wav");
    type_role(
        &mut harness,
        "Download complete",
        &sound.display().to_string(),
    );
    assert_eq!(
        draft(&harness).sounds.get("complete"),
        Some(&sound.display().to_string())
    );
    assert!(!play(&harness));
    harness
        .query_all_by_role_and_label(Role::Button, "Play")
        .next()
        .unwrap()
        .click();
    harness.run_steps(2);
    assert_eq!(*world.fake.opened.lock().unwrap(), ["chime"]);

    click(&mut harness, "OK");
    let kept = harness.state().backend().settings();
    assert_eq!(kept.on_close, "tray");
    assert_eq!(kept.duplicate_choice, "replace");
    assert_eq!(
        kept.sounds.get("complete"),
        Some(&sound.display().to_string())
    );
    let mut back = kept;
    back.on_close = world.settings().on_close;
    back.duplicate_choice = String::new();
    back.sounds.clear();
    harness.state_mut().apply_settings(back).unwrap();
}

/// The settings file: saved where the chooser says, without a password, and
/// loaded back to look over before OK; a place that cannot be written or a
/// file that is not settings is said; a chooser cancelled says nothing;
/// with no chooser the download folder's dm-settings.json is used.
#[test]
fn general_saves_and_loads_the_settings_file() {
    let world = world();
    let mut harness = world.harness();
    open_options_at(&mut harness, dm_desktop::dialogs::Tab::General);
    let file = world.dir.path().join("carried.json");
    *world.fake.chosen.lock().unwrap() = Some(file.clone());
    click(&mut harness, "Save settings to a file…");
    assert_eq!(
        options_says(&harness),
        format!("Saved to {}, without any password.", file.display())
    );
    assert!(file.exists());

    // A folder is no place to write a file.
    *world.fake.chosen.lock().unwrap() = Some(world.dir.path().to_path_buf());
    click(&mut harness, "Save settings to a file…");
    assert!(
        options_says(&harness).contains("could not be written"),
        "{}",
        options_says(&harness)
    );
    // Cancelled: nothing said.
    *world.fake.chosen.lock().unwrap() = None;
    click(&mut harness, "Save settings to a file…");
    assert_eq!(options_says(&harness), "");

    // Cancelled: nothing loaded, nothing said.
    click(&mut harness, "Load settings from a file…");
    assert_eq!(options_says(&harness), "");

    // Not settings.
    let junk = world.dir.path().join("junk.json");
    std::fs::write(&junk, "this is not settings").unwrap();
    *world.fake.chosen.lock().unwrap() = Some(junk.clone());
    click(&mut harness, "Load settings from a file…");
    assert!(
        options_says(&harness).starts_with(&format!("{} could not be read", junk.display())),
        "{}",
        options_says(&harness)
    );
    // The file saved: loaded, to be looked over.
    *world.fake.chosen.lock().unwrap() = Some(file);
    click(&mut harness, "Load settings from a file…");
    assert_eq!(
        options_says(&harness),
        "Loaded. Look them over, then OK to use them."
    );

    // No chooser here: the download folder's dm-settings.json.
    *world.fake.no_chooser.lock().unwrap() = true;
    let folder = draft(&harness).download_dir.clone();
    std::fs::create_dir_all(&folder).unwrap();
    click(&mut harness, "Save settings to a file…");
    let fallback = folder.join("dm-settings.json");
    assert!(fallback.exists(), "{}", options_says(&harness));
    click(&mut harness, "Load settings from a file…");
    assert_eq!(
        options_says(&harness),
        "Loaded. Look them over, then OK to use them."
    );
    click(&mut harness, "Cancel");
}

/// Browser: a browser whose extension has talked to dm is Connected, the
/// others offer Add, which opens the extension's page; the two keys are
/// picked from their lists, and the same key for both is warned of.
#[test]
fn the_browser_tab_offers_the_extension_and_warns_of_one_key_for_both() {
    let world = world();
    let mut harness = world.harness();
    let seen = dm_shell::settings::home().join("browser.seen");
    std::fs::write(&seen, "Firefox\n").unwrap();
    open_options_at(&mut harness, dm_desktop::dialogs::Tab::FileTypes);
    assert_eq!(harness.query_all_by_label("Connected").count(), 1);
    let add = harness
        .query_all_by_role_and_label(Role::Button, "Add")
        .count();
    assert_eq!(add, 2, "Add for each browser not yet connected");
    harness
        .query_all_by_role_and_label(Role::Button, "Add")
        .next()
        .unwrap()
        .click();
    harness.run_steps(3);
    let opened = world.fake.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].starts_with("https://"), "{opened:?}");
    std::fs::remove_file(&seen).unwrap();

    let keep = "Hold while clicking to leave it with the browser";
    let take = "Hold while clicking to send it to dm whatever its type or size";
    let same = "The two keys are the same; holding it leaves the download with the browser.";
    choose(&mut harness, keep, "Alt");
    assert_eq!(draft(&harness).catch.keep_key, "alt");
    choose(&mut harness, take, "Ctrl or Cmd");
    assert_eq!(draft(&harness).catch.take_key, "ctrl");
    assert!(harness.query_by_label(same).is_none());
    choose(&mut harness, take, "Alt");
    assert!(harness.query_by_label(same).is_some());
    choose(&mut harness, take, "None");
    assert_eq!(draft(&harness).catch.take_key, "");
    assert!(harness.query_by_label(same).is_none());
    click(&mut harness, "Cancel");
}

/// Folders: a download folder typed in is the draft's.
#[test]
fn the_download_folder_typed_in_folders_is_the_drafts() {
    let world = world();
    let mut harness = world.harness();
    open_options_at(&mut harness, dm_desktop::dialogs::Tab::SaveTo);
    let elsewhere = world.dir.path().join("elsewhere");
    retype(
        &mut harness,
        "Download folder",
        &elsewhere.display().to_string(),
    );
    assert_eq!(draft(&harness).download_dir, elsewhere);
    click(&mut harness, "Cancel");
}

/// Downloads: a preset sets the connections; the video quality is picked;
/// a queue's own limit and a start time are added, their queue picked from
/// the list, a start time that is not one warned of, and both deleted.
#[test]
fn downloads_sets_presets_quality_queue_limits_and_start_times() {
    let world = world();
    let mut harness = world.harness();
    open_options_at(&mut harness, dm_desktop::dialogs::Tab::Downloads);
    click(&mut harness, "Fast (16)");
    assert_eq!(draft(&harness).connections, 16);
    choose(&mut harness, "Video quality", "Audio only");
    assert_eq!(draft(&harness).stream_quality, "audio");

    let limits = draft(&harness).queue_limits.len();
    click(&mut harness, "Add a queue limit");
    assert_eq!(draft(&harness).queue_limits.len(), limits + 1);
    let queue = draft(&harness).queue_limits[limits].queue.clone();
    let shown = dm_desktop::i18n::name(&queue).to_string();
    choose(&mut harness, "Queue", &shown);
    assert_eq!(draft(&harness).queue_limits[limits].queue, queue);
    click(&mut harness, "Delete queue limit");
    assert_eq!(draft(&harness).queue_limits.len(), limits);
    // A limit for a queue since deleted still offers it, beside the others.
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .queue_limits
        .push(dm_shell::settings::QueueLimit {
            queue: "Gone".into(),
            files: 2,
        });
    harness.run_steps(2);
    choose(&mut harness, "Queue", "Gone");
    assert_eq!(draft(&harness).queue_limits[limits].queue, "Gone");
    click(&mut harness, "Delete queue limit");
    assert_eq!(draft(&harness).queue_limits.len(), limits);

    click(&mut harness, "Add a start time");
    let starts = draft(&harness).queue_starts.len();
    assert_eq!(draft(&harness).queue_starts[starts - 1].at, "02:00");
    let queue = draft(&harness).queue_starts[starts - 1].queue.clone();
    choose(&mut harness, "Queue", &dm_desktop::i18n::name(&queue));
    assert!(harness.query_by_label("Not a time").is_none());
    retype(&mut harness, "Starts at (HH:MM)", "25:99");
    assert!(harness.query_by_label("Not a time").is_some());
    click(&mut harness, "Delete start time");
    assert_eq!(draft(&harness).queue_starts.len(), starts - 1);
    click(&mut harness, "Cancel");
}

/// Advanced: the second network is picked from the computer's own (None
/// turns it off); a cookies file is cleared, and what was said of it with
/// it; a proxy's kind brings its usual port in place of the last kind's,
/// and leaves a port typed in; a proxy of its own for HTTP is
/// turned on and off again.
#[test]
fn advanced_picks_the_second_network_clears_cookies_and_sets_the_proxys_port() {
    let world = world();
    let mut harness = world.harness();
    open_options_at(&mut harness, dm_desktop::dialogs::Tab::Advanced);
    {
        let d = harness.state_mut().dialogs.options.as_mut().unwrap();
        d.draft.second_path = "en-test".into();
        d.draft.cookies_file = "/nowhere/cookies.txt".into();
        d.says = "/nowhere/cookies.txt could not be read".into();
        d.draft.proxy.enabled = true;
        d.draft.proxy.kind = dm_core::ProxyKind::Http;
        d.draft.proxy.port = 8080;
    }
    harness.run_steps(2);
    assert!(harness
        .query_by_role_and_label(Role::ComboBox, "Second network")
        .is_some());
    choose(&mut harness, "Second network", "None");
    assert_eq!(draft(&harness).second_path, "");
    // One of the computer's own networks, named as the list names it.
    if let Some(found) = dm_core::interfaces().into_iter().next() {
        let words = match dm_core::system_name(&found.name) {
            Some(named) => format!("{named} ({})", found.name),
            None => format!("{} ({})", found.name, found.address),
        };
        choose(&mut harness, "Second network", &words);
        assert_eq!(draft(&harness).second_path, found.name);
    }

    click(&mut harness, "Clear");
    assert_eq!(draft(&harness).cookies_file, "");
    assert_eq!(
        options_says(&harness),
        "",
        "what was said of the file stayed"
    );

    choose(&mut harness, "Proxy type", "SOCKS5");
    assert_eq!(draft(&harness).proxy.kind, dm_core::ProxyKind::Socks5);
    assert_eq!(draft(&harness).proxy.port, 1080);
    choose(&mut harness, "Proxy type", "HTTP");
    assert_eq!(draft(&harness).proxy.port, 8080);
    // The computer's own settings have no usual port: the port stays.
    choose(&mut harness, "Proxy type", "The computer's proxy settings");
    assert_eq!(draft(&harness).proxy.kind, dm_core::ProxyKind::System);
    assert_eq!(draft(&harness).proxy.port, 8080);
    choose(&mut harness, "Proxy type", "HTTP");
    // A port typed in stays whatever the kind.
    harness
        .state_mut()
        .dialogs
        .options
        .as_mut()
        .unwrap()
        .draft
        .proxy
        .port = 3128;
    harness.run_steps(2);
    choose(&mut harness, "Proxy type", "SOCKS4");
    assert_eq!(draft(&harness).proxy.port, 3128);

    let own = "A proxy of its own for HTTP downloads";
    click(&mut harness, own);
    assert!(draft(&harness).proxy.per_scheme.contains_key("http"));
    assert!(harness.query_by_label("Its host").is_some());
    click(&mut harness, own);
    assert!(draft(&harness).proxy.per_scheme.is_empty());
    click(&mut harness, "Cancel");
}

/// Sign-ins: a login is added, its host field given the keyboard, and
/// deleted again.
#[test]
fn sign_ins_add_and_delete_a_login() {
    let world = world();
    let mut harness = world.harness();
    open_options_at(&mut harness, dm_desktop::dialogs::Tab::SiteLogins);
    click(&mut harness, "Add a login");
    harness.run_steps(2);
    assert_eq!(draft(&harness).site_logins.len(), 1);
    harness
        .get_by_role_and_label(Role::TextInput, "Host")
        .type_text("files.example.org");
    harness.run_steps(2);
    assert_eq!(draft(&harness).site_logins[0].host, "files.example.org");
    harness
        .query_all_by_role_and_label(Role::Button, "Delete")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    assert!(draft(&harness).site_logins.is_empty());
    click(&mut harness, "Cancel");
}
