// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options, set as a person sets it: its tabs, check boxes, pop-ups and fields, then OK, and
//! what that put in force on the queue and wrote down.

use dm_core::State;

use crate::harness::world::{payload, world};
use crate::harness::{saved_settings, settings_file, Harness};
use crate::options::Options;

mod folders;
mod proxy_and_advanced;

/// Options opened, as Downloads › Options opens it.
pub(super) fn open_options(h: &mut Harness) {
    h.command("downloads.options");
    h.until("Options", |h| h.dialog_open("Options"));
    h.until_shown("General");
}

/// Options' tab `name` shown.
pub(super) fn tab(h: &mut Harness, name: &str) {
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
    h.click_as_person("OK", 0);
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
        h.dialog(|o: &mut Options, _, _| o.draft.advanced.proxy.kind == dm_core::ProxyKind::Http)
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
    assert!(kept(&mut h).folders.categorize);
    assert_eq!(saved_settings()["categorize"], true);
}

#[test]
fn the_file_types_the_browser_hands_over_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Browser");
    h.fill("File types", "ZIP .iso, mkv");
    h.fill("Never take downloads from these sites", "intranet.example");
    // Sites whose links work once.
    h.fill("Sites whose links work only once", "*.once.example");
    // The browsers dm takes from, each ticked at first (gaps row 86): one unticked is kept in
    // the rules the extension is sent.
    h.reveal("Brave");
    h.click("Brave");
    // A menu item left out of the browser's right-click menu.
    h.reveal("Download media with dm");
    h.click("Download media with dm");
    ok(&mut h);
    let catch = kept(&mut h).browser.catch;
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
    h.fill("Attempts per connection (0 to keep trying)", "0");
    h.fill("Time out a silent connection after (seconds)", "10");
    h.reveal("Add a site limit");
    h.click("Add a site limit");
    h.fill("Site", "Slow.Example");
    h.fill("Connections", "2");
    ok(&mut h);
    let settings = kept(&mut h).advanced;
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
    assert!(kept(&mut h).advanced.host_limits.is_empty());
}

#[test]
fn the_recording_limit_is_set_in_options_and_reaches_the_queue() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.fill("Stop recording after (minutes, 0 for no limit)", "90");
    ok(&mut h);
    assert_eq!(kept(&mut h).downloads.record_minutes, 90);
    assert_eq!(
        in_force(&mut h, &world).record_limit,
        Some(std::time::Duration::from_secs(90 * 60)),
        "in force on the queue, not only written down"
    );
    assert_eq!(saved_settings()["record_minutes"], 90);
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
    // Chosen from its hour and minute pop-ups, which hold only times (gaps row U32).
    for (field, index) in [("start-hour", 23), ("start-minute", 45)] {
        h.dialog(|o: &mut Options, _, cx| {
            crate::options::pages::pick(
                o,
                &crate::widgets::popup::Pick {
                    field,
                    item: 0,
                    index,
                },
            );
            cx.notify();
        });
    }
    ok(&mut h);
    assert_eq!(
        kept(&mut h).queue_start_list(),
        [(dm_core::MAIN_QUEUE.to_string(), 23 * 60 + 45)]
    );
}

#[test]
fn an_option_that_cannot_be_applied_is_said_and_what_was_typed_is_kept() {
    /* A failed OK does not reopen Options on the settings in force, which would throw away
     * every edit in the dialog to say what was wrong with one. The edits stay, with the reason,
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
    h.click_as_person("OK", 0);
    h.until("the reason", |h| {
        h.shows_part("could not change start-on-login")
    });
    let held = kept(&mut h);
    assert!(!held.general.start_on_login);
    assert!(
        !held.general.sound_on_complete,
        "half of the change was put in force"
    );
    assert!(
        h.dialog(|o: &mut Options, _, _| o.draft.general.start_on_login
            && o.draft.general.sound_on_complete),
        "the edits were thrown away"
    );

    *world.fake.login_fails.lock().unwrap() = false;
    ok(&mut h);
    let held = kept(&mut h);
    assert!(held.general.start_on_login && held.general.sound_on_complete);
}

/// The sort is remembered, and a third click on its heading puts the list back in the queue's
/// order; Options keeps its place whichever tab shows: OK stands where it stood.
#[test]
fn the_sort_survives_a_restart_and_options_keeps_its_place() {
    use dm_desktop_base::columns::Column;
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("file.zip"), "Add for later");
    h.until("the row", |h| !h.rows().is_empty());
    h.click("Size");
    h.click("Size");
    h.until("the sort kept", |h| {
        let saved = kept(h);
        saved.sort_column == "size" && !saved.sort_ascending
    });
    // The next launch, from what was saved.
    let mut again = world.harness_with(kept(&mut h));
    assert_eq!(
        again.view(|view| view.list.sort),
        Some((Column::Size, false)),
        "not remembered"
    );
    drop(again);
    // A third click puts the list back in the queue's order.
    h.click("Size");
    h.until("no sort", |h| h.view(|view| view.list.sort).is_none());
    assert_eq!(kept(&mut h).sort_column, "");

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
    assert!(!kept(&mut h).general.allow_command_line);
    open_options(&mut h);
    h.reveal("Allow the command line to control dm");
    h.click("Allow the command line to control dm");
    ok(&mut h);
    assert!(kept(&mut h).general.allow_command_line);
    assert_eq!(saved_settings()["allow_command_line"], true);
}

/// Appearance's text size (the slider's stops) is put in force at once and kept, as View ›
/// Larger and Smaller are; the theme is a choice kept on OK. (GPUI's test platform draws no
/// look of its own to read, so the look shown before OK is not checked here.)
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
    assert_eq!(
        kept(&mut h).general.theme,
        "system",
        "put in force before OK"
    );
    ok(&mut h);
    assert_eq!(kept(&mut h).general.theme, "dark");
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
    // Each scrolled to where the page runs past the window (Windows' longer lines).
    for step in [
        "1. Connect your phone to this computer with its USB cable. If it asks whether to trust this computer, say yes.",
        "2. Turn on its hotspot: on an iPhone, Settings › Personal Hotspot; on Android, USB tethering.",
        "3. Choose it above. Keep Wi-Fi as this computer's own connection, so only downloads use the phone's data.",
    ] {
        h.reveal(step);
    }
    h.click("Cancel");
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
    h.fill("Queue finished", r"C:\Windows\Media\tada.wav");
    ok(&mut h);
    assert_eq!(
        kept(&mut h).sounds.get("queue_done").map(String::as_str),
        Some(r"C:\Windows\Media\tada.wav")
    );
    // Emptied, the event's entry goes, and the usual sound is back.
    open_options(&mut h);
    h.fill("Queue finished", "");
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
    h.fill("Until it has sent (times its size)", "2");
    h.fill("For at most (minutes)", "30");
    h.fill("Send at most (KB/s, 0 for no limit)", "100");
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

/// A browser whose extension has spoken to dm is said to be connected with when it was last
/// heard from, read as Options opens and again by Check now (gaps row U24).
#[test]
fn options_says_when_each_browser_was_last_heard_from() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    let seen = dm_shell::settings::home().join("browser.seen");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(&seen, format!("Firefox\t{}\n", now - 125)).unwrap();
    open_options(&mut h);
    tab(&mut h, "Browser");
    // Two minutes and some seconds: a slow runner takes seconds to open Options (2m 17s on
    // Windows' runner, 2026-11-14), where a fast one says 2m 5s.
    h.until("Firefox heard two minutes ago", |h| {
        h.texts().iter().any(|t| {
            t.strip_prefix("Connected · last heard 2m ")
                .and_then(|rest| rest.strip_suffix("s ago"))
                .and_then(|seconds| seconds.parse::<u32>().ok())
                .is_some_and(|seconds| (5..60).contains(&seconds))
        })
    });
    // Heard again since: Check now reads it again.
    std::fs::write(&seen, format!("Firefox\t{}\n", now - 3)).unwrap();
    h.click("Check now");
    h.until("Firefox heard just now", |h| {
        h.texts().iter().any(|t| {
            t.starts_with("Connected · last heard ") && t.ends_with("s ago") && !t.contains('m')
        })
    });
    let _ = std::fs::remove_file(&seen);
}

/// Options' search finds a setting on whatever page it is, under its page's name, which a click
/// opens; a word nothing says is said so (gaps row U24).
#[test]
fn options_search_finds_a_setting_on_any_page() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    assert!(!h.shows("Proxy"), "the proxy on General");
    let set = |h: &mut Harness, words: &'static str| {
        h.dialog(|o: &mut Options, window, cx| {
            o.search.update(cx, |s, cx| s.set_value(words, window, cx));
            cx.notify();
        });
    };
    set(&mut h, "proxy");
    h.until("the proxy found", |h| h.shows("Proxy"));
    h.click("Advanced");
    h.until("Advanced open", |h| {
        h.dialog(|o: &mut Options, _, _| o.tab == dm_desktop_base::options::Tab::Advanced)
    });
    set(&mut h, "zzzz");
    h.until_shown("No setting says “zzzz”.");
}

/// "Forget finished downloads after … days" is set on Options › Downloads, put in force and
/// written down, and Options opened again holds it; 0, the default, forgets nothing (contract row
/// B70, gaps row W17).
#[test]
fn forgetting_finished_downloads_after_some_days_is_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    assert_eq!(
        kept(&mut h).downloads.forget_after_days,
        0,
        "never, at first"
    );
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.fill("Forget finished downloads after (days, 0 for never)", "30");
    ok(&mut h);
    assert_eq!(kept(&mut h).downloads.forget_after_days, 30);
    assert_eq!(saved_settings()["forget_after_days"], 30);
    open_options(&mut h);
    tab(&mut h, "Downloads");
    let held = h.dialog(|o: &mut Options, _, _| o.draft.downloads.forget_after_days);
    assert_eq!(held, 30, "Options opened again holds what was kept");
    h.click("Cancel");
    h.until("Options to close", |h| !h.dialog_open("Options"));
}
