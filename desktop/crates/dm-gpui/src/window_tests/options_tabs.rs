// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! More of Options' pages, driven by the words on screen: a settings file that cannot be
//! written or read, the browsers' extension and the keys a click is held with, the sign-ins.
//! Ported from the egui window's `tests/dialogs_cover.rs`.

use dm_shell::app::commands::Command;

use crate::harness::world::world;
use crate::harness::Harness;
use crate::options::Options;

/// Options opened at the page `tab`.
fn options_at(h: &mut Harness, tab: &str) {
    h.run(Command::Options);
    h.until("Options", |h| h.dialog_open("Options"));
    h.until_shown("General");
    if tab != "General" {
        h.click(tab);
        h.until(tab, |h| {
            h.dialog(|o: &mut Options, _, _| {
                dm_desktop_base::options::TABS
                    .iter()
                    .any(|(page, words)| *page == o.tab && *words == tab)
            })
        });
    }
}

/// The settings file: a place that cannot be written, or a file that is not settings, is
/// said; the file saved is loaded back to look over before OK.
#[test]
fn a_settings_file_that_cannot_be_written_or_read_is_said() {
    let world = world();
    let mut h = world.harness();
    options_at(&mut h, "General");
    // A folder is no place to write a file.
    h.scroll_to("Save settings to a file…");
    crate::services::choose_ahead(world.dir.path().to_path_buf());
    h.click("Save settings to a file…");
    h.until("the refusal to write", |h| {
        h.shows_part("could not be written")
    });

    let junk = world.dir.path().join("junk.json");
    std::fs::write(&junk, "this is not settings").unwrap();
    crate::services::choose_ahead(junk.clone());
    h.scroll_to("Load settings from a file…");
    h.click("Load settings from a file…");
    let unread = format!("{} could not be read", junk.display());
    h.until("the refusal to read", |h| h.shows_part(&unread));

    let file = world.dir.path().join("carried.json");
    crate::services::choose_ahead(file.clone());
    h.scroll_to("Save settings to a file…");
    h.click("Save settings to a file…");
    h.until("the file", |_| file.is_file());
    crate::services::choose_ahead(file);
    h.scroll_to("Load settings from a file…");
    h.click("Load settings from a file…");
    h.until_shown("Loaded. Look them over, then OK to use them.");
    h.click("Cancel");
    h.until("Options to close", |h| !h.dialog_open("Options"));
}

/// Browser: a browser whose extension has talked to dm is Connected, the others offer Add,
/// which opens the extension's page; the same key held for both is warned of.
#[test]
fn the_browser_page_offers_the_extension_and_warns_of_one_key_for_both() {
    let world = world();
    let mut h = world.harness();
    let seen = dm_shell::settings::home().join("browser.seen");
    std::fs::write(&seen, "Firefox\n").unwrap();
    options_at(&mut h, "Browser");
    let connected = h.texts().iter().filter(|t| *t == "Connected").count();
    std::fs::remove_file(&seen).unwrap();
    assert_eq!(connected, 1, "{:?}", h.texts());
    h.click("Add");
    h.until("the extension's page", |_| {
        world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.starts_with("https://"))
    });

    let same = "The two keys are the same; holding it leaves the download with the browser.";
    h.dialog(|o: &mut Options, _, cx| {
        o.draft.catch.keep_key = "alt".into();
        o.draft.catch.take_key = "ctrl".into();
        cx.notify();
    });
    assert!(!h.shows(same));
    h.dialog(|o: &mut Options, _, cx| {
        o.draft.catch.take_key = "alt".into();
        cx.notify();
    });
    h.reveal(same);
    h.close_dialogs();
}

/// Sign-ins: a login is added, its host typed, and deleted again.
#[test]
fn sign_ins_add_and_delete_a_login() {
    let world = world();
    let mut h = world.harness();
    options_at(&mut h, "Sign-ins");
    h.click("Add a login");
    h.until("a login", |h| {
        h.dialog(|o: &mut Options, _, _| o.draft.site_logins.len() == 1)
    });
    h.fill("Host", "files.example.org");
    h.until("the host typed", |h| {
        h.dialog(|o: &mut Options, _, _| o.draft.site_logins[0].host == "files.example.org")
    });
    h.click("Delete");
    h.until("the login deleted", |h| {
        h.dialog(|o: &mut Options, _, _| o.draft.site_logins.is_empty())
    });
    h.close_dialogs();
}

/// Ctrl+Tab and Ctrl+Shift+Tab move through Options' pages, round the end, as a tab view's
/// keys do (gaps row U2).
#[test]
fn ctrl_tab_moves_from_page_to_page() {
    let world = world();
    let mut h = world.harness();
    options_at(&mut h, "General");
    let page = |h: &mut Harness| h.dialog(|o: &mut Options, _, _| o.tab);
    h.key("tab", &["ctrl"]);
    h.until("the next page", |h| {
        page(h) == dm_desktop_base::options::Tab::FileTypes
    });
    h.key("tab", &["ctrl", "shift"]);
    h.until("back", |h| {
        page(h) == dm_desktop_base::options::Tab::General
    });
    h.key("tab", &["ctrl", "shift"]);
    h.until("round to the last", |h| {
        page(h) == dm_desktop_base::options::Tab::SiteLogins
    });
}
