// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options, set as a person sets it: its tabs, check boxes, pop-ups and fields, then OK, and
//! what that put in force on the queue and wrote down. Ported from the egui window's
//! `tests/window/options.rs`.

use dm_core::State;

use crate::harness::world::{payload, world};
use crate::harness::{saved_settings, settings_file, Harness};
use crate::options::Options;

/// Options opened, as Downloads › Options opens it.
fn open_options(h: &mut Harness) {
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.until_shown("General");
}

/// Options' tab `name` shown.
fn tab(h: &mut Harness, name: &str) {
    h.click(name);
    h.until(name, |h| {
        h.dialog(|o: &mut Options, _, _| {
            dm_desktop_base::options::TABS
                .iter()
                .any(|(tab, words)| *tab == o.tab && *words == name)
        })
    });
}

/// OK, and Options gone.
fn ok(h: &mut Harness) {
    h.person_click("OK");
    h.until("Options to close", |h| !h.dialog_open("Options"));
}

/// The settings in force.
fn kept(h: &mut Harness) -> dm_shell::Settings {
    h.view(|view| view.backend.settings())
}

/// The options the queue runs with.
fn in_force(h: &mut Harness, world: &crate::harness::world::World) -> dm_core::Options {
    let queue = h.view(|view| view.backend.queue.clone());
    world.runtime.block_on(queue.options())
}

/// A proxy of the person's own. A new dm has the proxy on, following the computer's settings
/// (docs/decisions.md, 2026-11-06), so its kind is picked from the pop-up, as a person does.
fn own_proxy(h: &mut Harness) {
    h.reveal("Proxy type");
    h.click("The computer's proxy settings");
    h.click("HTTP");
    h.until("the proxy's own kind", |h| {
        h.dialog(|o: &mut Options, _, _| o.draft.proxy.kind == dm_core::ProxyKind::Http)
    });
}

#[test]
fn options_are_put_into_force_on_ok_and_written_down() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.click("Sort finished files into folders by kind");
    ok(&mut h);
    assert!(kept(&mut h).categorize);
    assert_eq!(saved_settings()["categorize"], true);
}

#[test]
fn the_file_types_the_browser_hands_over_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Browser");
    h.type_into("File types", "ZIP .iso, mkv");
    h.type_into("Never take downloads from these sites", "intranet.example");
    // Sites whose links work once.
    h.type_into("Sites whose links work only once", "*.once.example");
    // The browsers dm takes from, each ticked at first (gaps row 86): one unticked is kept in
    // the rules the extension is sent.
    h.reveal("Brave");
    h.click("Brave");
    // A menu item left out of the browser's right-click menu.
    h.reveal("Download media with dm");
    h.click("Download media with dm");
    ok(&mut h);
    let catch = kept(&mut h).catch;
    assert_eq!(catch.extensions, ["zip", "iso", "mkv"]);
    assert_eq!(catch.skip_hosts, ["intranet.example"]);
    assert_eq!(catch.once_hosts, ["*.once.example"]);
    assert_eq!(catch.off_browsers, ["Brave"]);
    assert_eq!(catch.hidden_menus, ["dm-media"]);
    assert!(!catch.takes_from("Brave") && catch.takes_from("Microsoft Edge"));
    // What the extension is sent is what was typed.
    let rules = dm_protocol::CatchRules { ..catch };
    assert!(rules.enabled);
}

#[test]
fn site_limits_attempts_and_the_timeout_are_set_in_options_and_reach_the_queue() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    assert!(h.shows("Attempts per connection (0 to keep trying)"));
    assert!(h.shows("Time out a silent connection after (seconds)"));
    h.type_into("Attempts per connection (0 to keep trying)", "0");
    h.type_into("Time out a silent connection after (seconds)", "10");
    h.reveal("Add a site limit");
    h.click("Add a site limit");
    h.type_into("Site", "Slow.Example");
    h.type_into("Connections", "2");
    ok(&mut h);
    let settings = kept(&mut h);
    assert_eq!((settings.attempts, settings.read_timeout), (0, 10));
    assert_eq!(
        settings.host_limits,
        [dm_core::HostLimit {
            host: "slow.example".into(),
            connections: 2
        }]
    );
    // In force on the queue, not only written down.
    let options = in_force(&mut h, &world);
    assert_eq!(options.attempts, 0);
    assert_eq!(options.host_limits.len(), 1);
    assert_eq!(
        options.connections_for("https://dl.slow.example/f"),
        1,
        "these tests run on one connection, and a site's limit never raises that"
    );
    let saved = saved_settings();
    assert_eq!(saved["read_timeout"], 10);
    assert_eq!(saved["host_limits"][0]["host"], "slow.example");
    // The limit can be taken off again.
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Delete site limit");
    h.click("Delete site limit");
    ok(&mut h);
    assert!(kept(&mut h).host_limits.is_empty());
}

#[test]
fn a_category_made_in_options_takes_its_files_into_its_own_folder() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.click("Sort finished files into folders by kind");
    // At the foot of the page, scrolled to as a person would.
    h.reveal("Add a category");
    h.click("Add a category");
    h.type_into("Category", "Lectures");
    h.type_into("Types", "zip");
    h.type_into("Sites", "127.0.0.1");
    ok(&mut h);
    let made = kept(&mut h).custom_categories;
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].extensions, ["zip"]);

    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the download to complete", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    let row = h.rows()[0].clone();
    assert!(
        std::path::Path::new(&row.path)
            .parent()
            .unwrap()
            .ends_with("Lectures"),
        "{} is not in the category's folder",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap(), payload());
    // The side panel lists it, with its one download.
    h.click("Lectures");
    h.until("the category's one row", |h| {
        h.view(|view| {
            view.place == dm_shell::app::places::Place::Custom("Lectures".into())
                && view.shown.len() == 1
        })
    });

    // Deleted, it takes nothing more.
    open_options(&mut h);
    tab(&mut h, "Folders");
    // Under the page's fold, scrolled to as a person would.
    h.reveal("Delete category");
    h.click("Delete category");
    ok(&mut h);
    assert!(kept(&mut h).custom_categories.is_empty());
}

#[test]
fn the_recording_limit_is_set_in_options_and_reaches_the_queue() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.type_into("Stop recording after (minutes, 0 for no limit)", "90");
    ok(&mut h);
    assert_eq!(kept(&mut h).record_minutes, 90);
    assert_eq!(
        in_force(&mut h, &world).record_limit,
        Some(std::time::Duration::from_secs(90 * 60)),
        "in force on the queue, not only written down"
    );
    assert_eq!(saved_settings()["record_minutes"], 90);
}

#[test]
fn the_proxys_bypass_list_and_the_recheck_timer_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    h.type_into("Host", "proxy.example");
    h.type_into(
        "Not for these sites (fetched directly)",
        "Intranet.example 10.0.0.5",
    );
    // The recheck timer lives on the Downloads tab; what was typed on Advanced waits there.
    tab(&mut h, "Downloads");
    h.type_into("Recheck every (hours, 0 for manual)", "6");
    ok(&mut h);
    let settings = kept(&mut h);
    assert!(settings.proxy.enabled);
    assert_eq!(settings.proxy.host, "proxy.example");
    assert_eq!(settings.proxy.bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(settings.recheck_hours, 6);
    let options = in_force(&mut h, &world);
    assert_eq!(options.proxy_bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(options.proxy_for("http://files.intranet.example/a"), None);
    assert!(options.proxy_for("http://outside.example/a").is_some());
    // Turned off again from its switch.
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Connect through a proxy");
    h.click("Connect through a proxy");
    ok(&mut h);
    assert!(!kept(&mut h).proxy.enabled);
}

/// Reference A's per-protocol proxies and its automatic configuration script, in the proxy
/// section (gaps row 80).
#[test]
fn a_proxy_of_its_own_for_https_and_a_configuration_script_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    h.type_into("Host", "proxy.example");
    h.reveal("A proxy of its own for HTTPS downloads");
    h.click("A proxy of its own for HTTPS downloads");
    h.type_into("Its host", "secure-proxy.example");
    ok(&mut h);
    let settings = kept(&mut h);
    let https = &settings.proxy.per_scheme["https"];
    assert_eq!(https.host, "secure-proxy.example");
    assert_eq!(https.port, 8080);
    assert!(!settings.proxy.per_scheme.contains_key("http"));
    assert!(settings.proxy.script.is_empty());

    let script = "Automatic configuration script address (it chooses each site's proxy, in place of the above)";
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.type_into(script, "http://wpad.example/proxy.pac");
    ok(&mut h);
    assert_eq!(kept(&mut h).proxy.script, "http://wpad.example/proxy.pac");
    let saved = saved_settings();
    assert_eq!(saved["proxy"]["script"], "http://wpad.example/proxy.pac");
    assert_eq!(
        saved["proxy"]["per_scheme"]["https"]["host"],
        "secure-proxy.example"
    );
}

/// A queue's own daily start time is set in Options › Downloads; one that is not a time is
/// said beside it and refused.
#[test]
fn a_queues_start_time_is_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Downloads");
    // Scrolled to, as a person scrolls to it.
    h.reveal("Add a start time");
    h.click("Add a start time");
    h.type_into("Starts at (HH:MM)", "x");
    h.until_shown("Not a time");
    h.type_into("Starts at (HH:MM)", "23:45");
    h.until("the time taken", |h| !h.shows("Not a time"));
    ok(&mut h);
    assert_eq!(
        kept(&mut h).queue_start_list(),
        [(dm_core::MAIN_QUEUE.to_string(), 23 * 60 + 45)]
    );
}

#[test]
fn a_queues_own_file_limit_and_the_parts_folder_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.reveal("Add a queue limit");
    h.click("Add a queue limit");
    // The queue's own "Files at once", under the one for every download.
    h.type_into_after("Queues with their own limit:", "Files at once", "2");
    tab(&mut h, "Folders");
    let staged = world.dir.path().join("staging");
    h.type_into(
        "Folder for unfinished downloads",
        &staged.display().to_string(),
    );
    ok(&mut h);
    let settings = kept(&mut h);
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
        h.view(|view| view.backend.queue.queue_limits()),
        std::collections::HashMap::from([(dm_core::MAIN_QUEUE.to_string(), 2)])
    );
    assert_eq!(in_force(&mut h, &world).parts_dir, staged);
}

#[test]
fn an_option_that_cannot_be_applied_is_said_and_what_was_typed_is_kept() {
    /* A failed OK used to reopen Options on the settings in force, which threw away every
     * edit in the dialog to say what was wrong with one. The edits stay, with the reason,
     * and OK again once the cause is gone puts them in force. */
    let _file = settings_file();
    let world = world();
    *world.fake.login_fails.lock().unwrap() = true;
    let mut h = world.harness();
    open_options(&mut h);
    h.reveal("Start dm when I sign in");
    h.click("Start dm when I sign in");
    h.reveal("Play a sound when a download finishes");
    h.click("Play a sound when a download finishes");
    h.person_click("OK");
    h.until("the reason", |h| {
        h.shows_part("could not change start-on-login")
    });
    let held = kept(&mut h);
    assert!(!held.start_on_login);
    assert!(
        !held.sound_on_complete,
        "half of the change was put in force"
    );
    assert!(
        h.dialog(|o: &mut Options, _, _| o.draft.start_on_login && o.draft.sound_on_complete),
        "the edits were thrown away"
    );

    *world.fake.login_fails.lock().unwrap() = false;
    ok(&mut h);
    let held = kept(&mut h);
    assert!(held.start_on_login && held.sound_on_complete);
}

#[test]
fn a_bad_category_options_opened_with_does_not_fail_an_unrelated_ok() {
    let _file = settings_file();
    let world = world();
    // As if an older dm had saved it.
    let mut saved = world.settings();
    saved.custom_categories.push(dm_core::Custom {
        name: "Work/Docs".into(),
        extensions: vec!["pdf".into()],
        ..Default::default()
    });
    let wanted = !saved.sound_on_failure;
    let mut h = world.harness_with(saved);
    open_options(&mut h);
    h.reveal("Play a sound when a download fails");
    h.click("Play a sound when a download fails");
    ok(&mut h);
    assert_eq!(kept(&mut h).sound_on_failure, wanted);
}

#[test]
fn options_refuses_a_category_that_is_not_one_folder_and_a_relative_download_folder() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let before = kept(&mut h);
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.click("Sort finished files into folders by kind");
    h.reveal("Add a category");
    h.click("Add a category");
    h.type_into("Category", "Work/Docs");
    h.type_into("Types", "pdf");
    h.person_click("OK");
    h.until("the refusal", |h| h.shows_part("can't be a category"));
    assert_eq!(kept(&mut h).custom_categories, before.custom_categories);

    h.reveal("Delete category");
    h.click("Delete category");
    h.type_into("Download folder", "Downloads/dm");
    h.person_click("OK");
    h.until("the refusal", |h| h.shows_part("must be a full path"));
    assert_eq!(kept(&mut h).download_dir, before.download_dir);

    // From the home folder is a full path, and is expanded.
    h.type_into("Download folder", "~/dm-test-downloads");
    ok(&mut h);
    assert_eq!(
        kept(&mut h).download_dir,
        dm_shell::settings::full_path("~/dm-test-downloads").unwrap()
    );
}

#[test]
fn options_holds_only_new_or_renamed_folders_to_every_computers_rules() {
    let _file = settings_file();
    let world = world();
    // Saved before by a dm that took them, and usable on this computer: a kind's folder and
    // a category with a colon in it.
    let mut saved = world.settings();
    saved.categorize = true;
    saved.category_folders[1] = "Audio: 2024".into();
    saved.custom_categories.push(dm_core::Custom {
        name: "Live: 2024".into(),
        extensions: vec!["mkv".into()],
        sites: Vec::new(),
    });
    let before = saved.clone();
    let mut h = world.harness_with(saved);
    // An unrelated change goes through.
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.type_into("Connections per file", "4");
    ok(&mut h);
    let now = kept(&mut h);
    assert_eq!(now.connections, 4);
    assert_eq!(
        now.category_folders[1], "Audio: 2024",
        "a saved folder kept"
    );
    assert!(now.custom_categories.iter().any(|c| c.name == "Live: 2024"));
    // A kind's folder renamed to one Windows cannot make is refused.
    open_options(&mut h);
    tab(&mut h, "Folders");
    h.type_into("Videos", "Clips|new");
    h.person_click("OK");
    h.until("the refusal", |h| {
        h.shows_part("because its name is also its folder")
    });
    assert_eq!(kept(&mut h).category_folders[0], before.category_folders[0]);
    // And a new category.
    h.type_into("Videos", &before.category_folders[0]);
    h.reveal("Add a category");
    h.click("Add a category");
    // The new category's row, after the saved one's.
    h.type_into_after("Delete category", "Category", "Notes?");
    h.type_into_after("Delete category", "Types", "txt");
    h.person_click("OK");
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert!(h.dialog_open("Options"), "a new bad name was taken");
}

/// Options keeps its place whichever tab shows: OK stands where it stood. (The other half of
/// the egui window's test, the sort remembered across a restart and a third click back to the
/// queue's order, is gaps row U29.)
#[test]
fn the_sort_survives_a_restart_and_options_keeps_its_place() {
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    let ok_at = |h: &mut Harness| h.dialog_words("OK").expect("OK on screen");
    let first = ok_at(&mut h);
    for name in [
        "Folders",
        "Advanced",
        "Sign-ins",
        "Downloads",
        "Browser",
        "General",
    ] {
        tab(&mut h, name);
        assert_eq!(ok_at(&mut h), first, "Options moved on {name}");
    }
    h.click("Cancel");
}

/// OK refuses a proxy turned on with no host, or with an address where its host goes, and
/// says why on the Advanced page, at its proxy section; nothing is put in force.
#[test]
fn options_refuses_a_proxy_with_no_host() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    h.person_click("OK");
    h.until("the refusal", |h| h.shows_part("Enter the proxy's host"));
    assert!(h.dialog_open("Options"), "OK took a proxy with no host");
    // Nothing was put in force: the proxy still follows the computer's settings.
    let untouched = |h: &mut Harness| kept(h).proxy.kind == dm_core::ProxyKind::System;
    assert!(untouched(&mut h));
    h.type_into("Host", "http://proxy.example:3128");
    h.person_click("OK");
    h.until("the refusal", |h| h.shows_part("isn't a valid proxy host"));
    assert!(untouched(&mut h));
    h.click("Cancel");
}

/// Options opens at its top however far down it was left.
#[test]
fn options_opens_at_its_top_wherever_it_was_left() {
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    h.reveal("Save settings to a file…");
    assert!(!h.shows("Language"), "the premise, scrolled past Language");
    h.click("Cancel");
    h.until("Options to close", |h| !h.dialog_open("Options"));
    open_options(&mut h);
    assert!(h.shows("Language"), "reopened scrolled");
}

#[test]
fn the_command_line_setting_is_in_options_off_by_default_and_saved() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert!(!kept(&mut h).allow_command_line);
    open_options(&mut h);
    h.reveal("Allow the command line to control dm");
    h.click("Allow the command line to control dm");
    ok(&mut h);
    assert!(kept(&mut h).allow_command_line);
    assert_eq!(saved_settings()["allow_command_line"], true);
}

/// Options › Advanced: a site's rule gives its downloads a folder and a speed limit of their
/// own, from the moment they are added by any way in.
#[test]
fn a_site_rule_set_in_options_sends_that_sites_downloads_to_its_folder_at_its_speed() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    // Under the download folder: a folder elsewhere is refused to a download a script adds
    // (`control_api::folder_refused`), and this window is driven as a script drives it.
    let folder = world.dir.path().join("out").join("from-the-site");
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Add a site rule");
    h.click("Add a site rule");
    h.type_into("Site", "127.0.0.1");
    h.type_into("Folder", &folder.display().to_string());
    // The rule's own speed, under the page's of the same name.
    h.type_into_after(
        "Sites with their own folder and speed:",
        "Speed limit, KB/s (0 for none)",
        "4096",
    );
    ok(&mut h);
    let rules = kept(&mut h).site_rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].speed, 4096);

    h.add_url(&world.url("file.zip"), "Start now");
    h.until("the file to complete", |h| {
        h.rows().iter().any(|r| r.state == State::Completed)
    });
    let done = h.rows()[0].clone();
    assert_eq!(done.speed_limit, 4096, "its own speed limit, from the rule");
    assert_eq!(done.save_dir.as_deref(), folder.to_str());
    assert!(
        std::path::Path::new(&done.path).starts_with(&folder),
        "it landed in the site's folder: {}",
        done.path
    );

    // The rule can be deleted again.
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Delete site rule");
    h.click("Delete site rule");
    ok(&mut h);
    assert!(kept(&mut h).site_rules.is_empty());
}

/// Appearance's text size (the slider's stops) is put in force at once and kept, as View ›
/// Larger and Smaller are; the theme is a choice kept on OK. (The egui window also showed the
/// look as it was chosen, before OK; GPUI's test platform draws no look of its own to read, so
/// that is not checked here.)
#[test]
fn appearance_sets_the_theme_and_the_size_of_text_for_the_whole_window() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    h.reveal("Appearance");
    assert_eq!(kept(&mut h).text_size, 1.0);
    // 100% is the design; the stops either side scale everything.
    h.reveal("120%");
    h.click("120%");
    h.until("120%", |h| (kept(h).text_size - 1.2).abs() < 0.001);
    h.reveal("80%");
    h.click("80%");
    h.until("80%", |h| (kept(h).text_size - 0.8).abs() < 0.001);
    h.reveal("100%");
    h.click("100%");
    h.until("100%", |h| (kept(h).text_size - 1.0).abs() < 0.001);
    // The theme, picked from its pop-up, shown on it, and kept on OK.
    h.reveal("As the computer is");
    h.click("As the computer is");
    h.click("Light");
    h.until_shown("Light");
    h.click("Light");
    h.click("Dark");
    h.until_shown("Dark");
    assert_eq!(kept(&mut h).theme, "system", "put in force before OK");
    ok(&mut h);
    assert_eq!(kept(&mut h).theme, "dark");
}

/// Every setting a tab's Restore defaults names exists, and putting a tab back changes what
/// the tab edits and nothing else.
#[test]
fn restore_defaults_puts_a_tab_back_and_only_that_tab() {
    use dm_desktop_base::options::{tab_settings, TABS};
    let fresh = serde_json::to_value(dm_shell::Settings::default()).unwrap();
    for (tab, _) in TABS {
        for key in tab_settings(tab) {
            assert!(fresh.get(*key).is_some(), "{key} is no setting");
        }
    }
    let world = world();
    let mut changed = world.settings();
    changed.connections = 9;
    changed.theme = "dark".into();
    changed.catch.extensions = vec!["zzz".into()];
    let mut h = world.harness_with(changed);
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.click("Restore defaults");
    let defaults = dm_shell::Settings::default();
    let draft = h.dialog(|o: &mut Options, _, _| o.draft.clone());
    assert_eq!(draft.connections, defaults.connections);
    assert_eq!(draft.theme, "dark", "General's setting was put back");
    assert_eq!(draft.catch.extensions, ["zzz"], "Browser's was put back");
    tab(&mut h, "Browser");
    h.click("Restore defaults");
    let (draft, types) = h.dialog(|o: &mut Options, _, _| (o.draft.clone(), o.typed.types.clone()));
    assert_eq!(draft.catch.extensions, defaults.catch.extensions);
    assert_eq!(types, defaults.catch.extensions.join(" "));
    h.click("Cancel");
}

/// Options › Advanced says how to have a phone as the second network, in steps folded away
/// until asked for.
#[test]
fn the_second_network_says_how_to_use_a_phone_step_by_step() {
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("How to use your phone");
    assert!(!h.shows_part("Turn on its hotspot"));
    h.click("How to use your phone");
    for step in [
        "1. Connect your phone",
        "2. Turn on its hotspot",
        "3. Choose it above",
    ] {
        h.until(step, |h| h.shows_part(step));
    }
    h.click("Cancel");
}

/// Options has one Advanced page for what were the Connection and Proxy tabs: both groups of
/// settings are on it, and neither tab is in the list any more.
#[test]
fn advanced_holds_the_connection_and_the_proxy_settings_on_one_page() {
    use dm_desktop_base::options::{Tab, TABS};
    assert!(TABS.iter().any(|(tab, _)| *tab == Tab::Advanced));
    assert!(
        !TABS
            .iter()
            .any(|(_, name)| *name == "Network" || *name == "Proxy"),
        "the Connection and Proxy tabs are still in the list"
    );
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    for name in [
        "Speed limit, KB/s (0 for none)",
        "Attempts per connection (0 to keep trying)",
        "Wait between attempts (seconds, 0 for automatic)",
        "Time out a silent connection after (seconds)",
        "Second network",
        "Connect through a proxy",
    ] {
        h.reveal(name);
    }
    h.click("Cancel");
}

/// A proxy that cannot be one, found by OK on another tab, is shown where it is said:
/// Advanced opens at its proxy section.
#[test]
fn a_proxy_that_is_wrong_opens_advanced_at_the_proxy_section() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    // Another tab, as a person leaves the proxy for another page and presses OK.
    tab(&mut h, "Downloads");
    h.person_click("OK");
    h.until("Advanced at its proxy section", |h| {
        h.dialog(|o: &mut Options, _, _| o.tab == dm_desktop_base::options::Tab::Advanced)
            && h.shows("Connect through a proxy")
    });
    assert!(h.dialog_open("Options"), "OK took a proxy with no host");
    assert!(h.shows_part("Enter the proxy's host"));
    h.click("Cancel");
}

/// OK keeps every setting the Advanced page holds, as the two tabs did.
#[test]
fn ok_keeps_every_setting_of_the_advanced_page() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.type_into("Speed limit, KB/s (0 for none)", "321");
    h.type_into("Attempts per connection (0 to keep trying)", "7");
    h.type_into("Wait between attempts (seconds, 0 for automatic)", "9");
    h.type_into("Time out a silent connection after (seconds)", "45");
    own_proxy(&mut h);
    h.type_into("Host", "proxy.example");
    h.type_into("Port", "3128");
    h.type_into("Username, if it needs one", "someone");
    ok(&mut h);
    let s = kept(&mut h);
    assert_eq!(
        (s.speed_limit, s.attempts, s.retry_wait, s.read_timeout),
        (321, 7, 9, 45)
    );
    assert!(s.proxy.enabled);
    assert_eq!(
        (
            s.proxy.host.as_str(),
            s.proxy.port,
            s.proxy.username.as_str()
        ),
        ("proxy.example", 3128, "someone")
    );
    let saved = saved_settings();
    assert_eq!(saved["speed_limit"], 321);
    assert_eq!(saved["proxy"]["host"], "proxy.example");
}

/// Options › General › Sounds names a sound file for each event, kept with the settings, and
/// an empty one is the usual sound again.
#[test]
fn a_sound_file_is_chosen_for_each_event_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    for words in [
        "Download complete",
        "Download failed",
        "Queue started",
        "Queue finished",
    ] {
        h.reveal(words);
    }
    h.type_into("Queue finished", r"C:\Windows\Media\tada.wav");
    ok(&mut h);
    assert_eq!(
        kept(&mut h).sounds.get("queue_done").map(String::as_str),
        Some(r"C:\Windows\Media\tada.wav")
    );
    // Emptied, the event's entry goes, and the usual sound is back.
    open_options(&mut h);
    h.type_into("Queue finished", "");
    ok(&mut h);
    assert!(!kept(&mut h).sounds.contains_key("queue_done"));
}

/// Options › Browser says who opens magnet links and .torrent files on this computer, read
/// from Windows' keys (read only: the test clicks no button that would write them).
#[cfg(target_os = "windows")]
#[test]
fn file_types_says_who_opens_magnet_links_and_torrent_files() {
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Browser");
    h.reveal("Magnet links open with:");
    h.reveal(".torrent files open with:");
    h.until_within(
        "the keys to be read",
        std::time::Duration::from_secs(20),
        &mut |h| !h.shows("Looking…"),
    );
    h.click("Cancel");
}

/// Options › Advanced › BitTorrent turns on keeping a finished torrent shared, with its ratio,
/// time and upload limit (gaps row 147); off in a new dm.
#[test]
fn keeping_a_finished_torrent_shared_is_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert!(!kept(&mut h).torrent_share);
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Keep sharing a finished torrent");
    h.click("Keep sharing a finished torrent");
    h.type_into("Until it has sent (times its size)", "2");
    h.type_into("For at most (minutes)", "30");
    h.type_into("Send at most (KB/s, 0 for no limit)", "100");
    ok(&mut h);
    let settings = kept(&mut h);
    assert!(settings.torrent_share);
    assert_eq!(settings.torrent_share_ratio, 2.0);
    assert_eq!(settings.torrent_share_minutes, 30);
    assert_eq!(settings.torrent_upload_limit, 100);
    // Turned off again.
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Keep sharing a finished torrent");
    h.click("Keep sharing a finished torrent");
    ok(&mut h);
    assert!(!kept(&mut h).torrent_share);
}

/// Advanced's Dial Up / VPN section (Windows), reference A's last tab in its words and order:
/// off as a new dm starts, the sign-in and the redials once a connection is chosen, and OK
/// keeping them (gaps row 93, G84). Saved with the switch off: on, the settings in force would
/// hold every download in this program for the connection.
#[cfg(windows)]
#[test]
fn advanced_has_the_dial_up_section_of_reference_a_on_windows() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    for name in [
        "Dial Up / VPN",
        "Use Windows Dial Up / VPN Networking",
        "User name",
        "Save password",
        "Redial attempts (zero if endlessly)",
        "Time between redial attempts (seconds)",
    ] {
        h.reveal(name);
    }
    // A connection Windows knows, as its pop-up gives it (this computer may know none).
    h.dialog(|o: &mut Options, _, _| o.draft.dialup.connection = "Office VPN".into());
    h.type_into("User name", "me");
    h.type_into("Redial attempts (zero if endlessly)", "3");
    h.type_into("Time between redial attempts (seconds)", "10");
    ok(&mut h);
    let s = kept(&mut h);
    assert!(!s.dialup.enabled, "a new dm dials nothing");
    assert_eq!(
        (
            s.dialup.connection.as_str(),
            s.dialup.username.as_str(),
            s.dialup.attempts,
            s.dialup.wait_seconds
        ),
        ("Office VPN", "me", 3, 10)
    );
    let saved = saved_settings();
    assert_eq!(saved["dialup"]["connection"], "Office VPN");
    assert_eq!(saved["dialup"]["attempts"], 3);
}
