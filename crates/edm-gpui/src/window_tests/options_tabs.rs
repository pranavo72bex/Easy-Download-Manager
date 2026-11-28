// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! More of Options' pages, driven by the words on screen: a settings file that cannot be
//! written or read, the browsers' extension and the keys a click is held with, the sign-ins.

use edm_shell::app::commands::Command;

use crate::harness::world::world;
use crate::harness::Harness;
use crate::options::Options;

use super::options::{open_options, tab};

/// Options opened at the page `tab`.
fn options_at(h: &mut Harness, tab: &str) {
    h.run(Command::Options);
    h.until("Options", |h| h.dialog_open("Options"));
    h.until_shown("General");
    if tab != "General" {
        h.click(tab);
        h.until(tab, |h| {
            h.dialog(|o: &mut Options, _, _| {
                edm_desktop_base::options::TABS
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
    crate::platform::services::choose_ahead(world.dir.path().to_path_buf());
    h.click("Save settings to a file…");
    h.until("the refusal to write", |h| {
        h.shows_part("couldn’t be written")
    });

    let junk = world.dir.path().join("junk.json");
    std::fs::write(&junk, "this is not settings").unwrap();
    crate::platform::services::choose_ahead(junk.clone());
    h.scroll_to("Load settings from a file…");
    h.click("Load settings from a file…");
    let unread = format!("{} couldn’t be read", junk.display());
    h.until("the refusal to read", |h| h.shows_part(&unread));

    let file = world.dir.path().join("carried.json");
    crate::platform::services::choose_ahead(file.clone());
    h.scroll_to("Save settings to a file…");
    h.click("Save settings to a file…");
    h.until("the file", |_| file.is_file());
    crate::platform::services::choose_ahead(file);
    h.scroll_to("Load settings from a file…");
    h.click("Load settings from a file…");
    h.until_shown("Loaded. Look them over, then OK to use them.");
    h.click("Cancel");
    h.until("Options to close", |h| !h.dialog_open("Options"));
}

/// Browser: a browser whose extension has talked to EDM is Connected, the others offer Add,
/// which opens the extension's page; the same key held for both is warned of.
#[test]
fn the_browser_page_offers_the_extension_and_warns_of_one_key_for_both() {
    let world = world();
    let mut h = world.harness();
    let seen = edm_shell::settings::home().join("browser.seen");
    std::fs::write(&seen, "Firefox\n").unwrap();
    options_at(&mut h, "Browser");
    let connected = h.texts().iter().filter(|t| *t == "Connected").count();
    std::fs::remove_file(&seen).unwrap();
    assert_eq!(connected, 1, "{:?}", h.texts());
    h.click("Add");
    h.until("the extension’s page", |_| {
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
        o.draft.browser.catch.keep_key = "alt".into();
        o.draft.browser.catch.take_key = "ctrl".into();
        cx.notify();
    });
    assert!(!h.shows(same));
    h.dialog(|o: &mut Options, _, cx| {
        o.draft.browser.catch.take_key = "alt".into();
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
    h.click("Add a sign-in");
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
        page(h) == edm_desktop_base::options::Tab::FileTypes
    });
    h.key("tab", &["ctrl", "shift"]);
    h.until("back", |h| {
        page(h) == edm_desktop_base::options::Tab::General
    });
    h.key("tab", &["ctrl", "shift"]);
    h.until("round to the last", |h| {
        page(h) == edm_desktop_base::options::Tab::SiteLogins
    });
}

/// Every setting a tab's Restore defaults names exists, and putting a tab back changes what
/// the tab edits and nothing else.
#[test]
fn restore_defaults_puts_a_tab_back_and_only_that_tab() {
    use edm_desktop_base::options::{tab_settings, TABS};
    let fresh = serde_json::to_value(edm_shell::Settings::default()).unwrap();
    for (tab, _) in TABS {
        for key in tab_settings(tab) {
            assert!(fresh.get(*key).is_some(), "{key} is no setting");
        }
    }
    let world = world();
    let mut changed = world.settings();
    changed.downloads.connections = 9;
    changed.general.theme = "dark".into();
    changed.browser.catch.extensions = vec!["zzz".into()];
    let mut h = world.harness_with(changed);
    open_options(&mut h);
    tab(&mut h, "Downloads");
    h.click("Restore defaults");
    let defaults = edm_shell::Settings::default();
    let draft = h.dialog(|o: &mut Options, _, _| o.draft.clone());
    let (general, catch) = (&draft.general, &draft.browser.catch);
    assert_eq!(draft.downloads.connections, defaults.downloads.connections);
    assert_eq!(general.theme, "dark", "General’s setting was put back");
    assert_eq!(catch.extensions, ["zzz"], "Browser’s was put back");
    tab(&mut h, "Browser");
    h.click("Restore defaults");
    let (draft, types) = h.dialog(|o: &mut Options, _, _| (o.draft.clone(), o.typed.types.clone()));
    let restored = defaults.browser.catch.extensions;
    assert_eq!(draft.browser.catch.extensions, restored);
    assert_eq!(types, restored.join(" "));
    h.click("Cancel");
}

/// Every page in the list of pages has a line of what it holds (contract B43), Sign-ins
/// included, from its heading (gaps row E43): with words and no heading its line was empty.
#[test]
fn every_page_says_what_it_holds() {
    use edm_desktop_base::options::Tab;
    let _file = crate::harness::settings_file();
    let world = crate::harness::world::world();
    let mut h = world.harness();
    super::options::open_options(&mut h);
    let said = h.dialog::<crate::options::Options, _>(|o, window, cx| {
        let theme = crate::window::theme::Theme::of(window.appearance());
        crate::options::pages::summary(o, Tab::SiteLogins, theme, window, cx)
    });
    assert_eq!(said, "Sites that need a sign-in");
}

/// Every page in the list of pages is in view at the smallest size Options opens at,
/// Windows' 820 by 520: with two-line summaries Sign-ins fell below the list's edge there and
/// could only be reached by scrolling the list (gaps row E35's two lines, main's Windows run).
#[test]
fn every_page_is_in_view_at_the_smallest_options() {
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    let (w, hgt) = crate::widgets::native::WindowSize::Options.each().windows;
    let main = h.window;
    h.cx.update(|cx| {
        for window in cx.windows() {
            if window != main {
                let _ = window.update(cx, |_, window, _| {
                    window.resize(gpui::size(gpui::px(w), gpui::px(hgt)))
                });
            }
        }
    });
    h.until("Options at Windows' size", |h| {
        h.dialog(|_: &mut Options, window, _| f32::from(window.viewport_size().height) <= hgt + 1.0)
    });
    for (_, words) in edm_desktop_base::options::TABS {
        assert!(h.shows(words), "{words} out of view: {:?}", h.texts());
    }
    h.click("Cancel");
}
