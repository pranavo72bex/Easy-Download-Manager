// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The item called `name` in `menus`, looked for in every submenu.
fn find<'a>(menus: &'a [Menu], name: &str) -> Option<&'a MenuItem> {
    fn within<'a>(items: &'a [MenuItem], name: &str) -> Option<&'a MenuItem> {
        items.iter().find_map(|item| match item {
            MenuItem::Action { name: n, .. } if n.as_ref() == name => Some(item),
            MenuItem::Submenu(menu) => within(&menu.items, name),
            _ => None,
        })
    }
    menus.iter().find_map(|menu| within(&menu.items, name))
}

/// Whether the item called `name` is ticked, and whether it is greyed.
fn state(menus: &[Menu], name: &str) -> (bool, bool) {
    match find(menus, name) {
        Some(MenuItem::Action {
            checked, disabled, ..
        }) => (*checked, *disabled),
        _ => panic!("no menu item {name}"),
    }
}

#[test]
fn when_done_ticks_its_choice_and_greys_what_the_computer_cannot_do() {
    let ticks = Ticks {
        when_done: WhenDone::Quit,
        ..Default::default()
    };
    let menus = build(&ticks);
    assert_eq!(state(&menus, "Exit dm"), (true, false));
    assert!(!state(&menus, "Do nothing").0);
    let hibernate = WhenDone::Hibernate.possible(dm_desktop_base::power::available);
    assert_eq!(state(&menus, "Hibernate"), (false, !hibernate));
}

#[test]
fn the_speed_limiter_ticks_while_a_limit_holds_and_the_preset_it_is() {
    let off = build(&Ticks::default());
    assert!(!state(&off, "Limit speed").0);
    let low = Command::SpeedLow.speed_preset().unwrap_or(0);
    let on = build(&Ticks {
        speed_limit: low,
        ..Default::default()
    });
    assert!(state(&on, "Limit speed").0);
    assert!(state(&on, Command::SpeedLow.label()).0);
    assert!(!state(&on, Command::SpeedHigh.label()).0);
}

/// The status bar's Speed limiter pops up Downloads › Speed limiter's
/// items, ticked alike: Limit speed and the preset in force, then
/// Settings… last (gaps row U29).
#[test]
fn the_status_bars_speed_limiter_offers_the_menus_items() {
    let ticked_at = |limit| -> Vec<Command> {
        speed_limiter(limit)
            .into_iter()
            .flatten()
            .filter(|(_, ticked)| *ticked)
            .map(|(command, _)| command)
            .collect()
    };
    assert_eq!(ticked_at(0), vec![Command::SpeedUnlimited]);
    let low = Command::SpeedLow.speed_preset().unwrap_or(0);
    assert_eq!(
        ticked_at(low),
        vec![Command::SpeedLimiterToggle, Command::SpeedLow]
    );
    assert_eq!(ticked_at(low + 1), vec![Command::SpeedLimiterToggle]);
    let choices = speed_limiter(low);
    assert_eq!(
        choices.first(),
        Some(&Some((Command::SpeedLimiterToggle, true)))
    );
    assert_eq!(
        choices.last(),
        Some(&Some((Command::SpeedLimiterSettings, false)))
    );
    // The menu bar's submenu is built from the same list.
    let menus = build(&Ticks {
        speed_limit: low,
        ..Default::default()
    });
    for (command, ticked) in choices.into_iter().flatten() {
        assert_eq!(state(&menus, &shown(command)).0, ticked, "{command:?}");
    }
}

/// An item is greyed exactly while its command does not apply (gaps row U5).
#[test]
fn an_item_that_does_not_apply_now_is_greyed() {
    let none_off = build(&Ticks::default());
    assert!(!state(&none_off, Command::CopyLink.label()).1);
    let off = build(&Ticks {
        off: vec![Command::CopyLink, Command::StopDownload],
        ..Default::default()
    });
    assert!(state(&off, Command::CopyLink.label()).1);
    assert!(state(&off, Command::StopDownload.label()).1);
    assert!(!state(&off, "Add URL…").1);
}

/// The Dock's menu offers adding a download, resuming or stopping them
/// all, and the download folder (gaps row U16).
#[test]
fn the_dock_menu_adds_resumes_and_stops() {
    let names: Vec<String> = dock_menu()
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(names.len(), 4);
    assert!(names[0].starts_with("Add URL"), "{names:?}");
    assert_eq!(names[3], "Open download folder");
}

#[test]
fn an_item_that_opens_a_dialog_says_so_with_an_ellipsis() {
    let menus = build(&Ticks::default());
    assert!(find(&menus, "Add numbered series…").is_some());
    assert!(find(&menus, "Options…").is_some());
    // One that acts at once has none.
    assert!(find(&menus, Command::StopDownload.label()).is_some());
    assert!(find(&menus, "Add numbered series").is_none());
}

/// View › Text size: how large it is, then
/// Larger, Smaller and Normal size, between Customize toolbar and
/// Language.
#[test]
fn the_view_menu_offers_the_text_size() {
    let menus = build(&Ticks {
        text_size: 1.2,
        ..Default::default()
    });
    assert!(find(&menus, "120%").is_some());
    for words in ["Larger", "Smaller", "Normal size"] {
        assert!(find(&menus, words).is_some(), "{words}");
    }
    let view = menus
        .iter()
        .find(|m| m.name.as_ref() == "View")
        .expect("a View menu");
    let names: Vec<String> = view
        .items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Submenu(menu) => Some(menu.name.to_string()),
            _ => None,
        })
        .collect();
    let at = |name: &str| names.iter().position(|n| n == name);
    assert_eq!(at("Text size"), at("Customize toolbar").map(|i| i + 1));
    assert_eq!(at("Language"), at("Text size").map(|i| i + 1));
}

#[test]
fn the_view_menu_ticks_the_columns_the_toolbar_the_language_and_the_sort() {
    let menus = build(&Ticks {
        columns_hidden: vec!["size".into()],
        toolbar_style: "icons".into(),
        toolbar_hidden: vec![Command::StopAll.id().into()],
        language: "en".into(),
        sorted_by: Some(Command::SortBySize),
        ..Default::default()
    });
    use dm_desktop_base::columns::Column;
    assert!(!state(&menus, Column::Size.title()).0, "Size hidden");
    assert!(state(&menus, Column::Status.title()).0, "Status shown");
    assert!(
        find(&menus, Column::Name.title()).is_none(),
        "the name stays"
    );
    assert!(state(&menus, "Small buttons").0);
    assert!(!state(&menus, "Icons and text").0);
    assert!(!state(&menus, "Stop All").0, "a hidden button unticked");
    assert!(state(&menus, "English").0);
    assert!(!state(&menus, "Deutsch").0);
    assert!(state(&menus, Command::SortBySize.label()).0);
    assert!(!state(&menus, Command::SortByName.label()).0);
}

#[test]
fn resume_says_what_it_will_do_for_the_rows_chosen() {
    assert!(find(&build(&Ticks::default()), Command::DownloadNow.label()).is_some());
    let retry = build(&Ticks {
        resume: "Retry",
        ..Default::default()
    });
    assert!(find(&retry, "Retry").is_some());
    assert!(find(&retry, Command::DownloadNow.label()).is_none());
}

#[test]
fn the_window_has_five_menus_with_exit_options_and_about() {
    let menus = window_menus(&Ticks::default());
    let names: Vec<String> = menus.iter().map(|m| m.name.to_string()).collect();
    assert_eq!(names, WINDOW_MENUS.map(t));
    let last = |menu: &Menu| match menu.items.last() {
        Some(MenuItem::Action { name, .. }) => name.to_string(),
        _ => String::new(),
    };
    assert_eq!(last(&menus[0]), Command::Quit.label());
    assert_eq!(last(&menus[2]), "Options…");
    assert_eq!(last(&menus[4]), Command::About.label());
}

#[test]
fn the_view_menu_ticks_what_is_shown() {
    let menus = build(&Ticks {
        sidebar: true,
        toolbar: false,
        drop_target: true,
        ..Default::default()
    });
    assert!(state(&menus, Command::ToggleCategories.label()).0);
    assert!(!state(&menus, Command::ToggleToolbar.label()).0);
    assert!(state(&menus, Command::ToggleDropTarget.label()).0);
}

/// Every command's keys, as bound, and the command each one runs.
fn bound() -> Vec<(gpui::Keystroke, Command)> {
    Command::ALL
        .iter()
        .flat_map(|c| c.chords().into_iter().map(move |k| (k, *c)))
        .map(|(chord, command)| {
            // As `bind_keys` reads it: GPUI keeps a keystroke's words.
            let keys: &'static str = keystroke(&chord).leak();
            let parsed = gpui::Keystroke::parse(keys)
                .unwrap_or_else(|e| panic!("{keys} cannot be bound: {e:?}"));
            (parsed, command)
        })
        .collect()
}

/// What the key `keys` (as GPUI writes one) runs.
fn runs(keys: String) -> Vec<Command> {
    let pressed = gpui::Keystroke::parse(keys.leak()).unwrap();
    bound()
        .into_iter()
        .filter(|(k, _)| *k == pressed)
        .map(|(_, c)| c)
        .collect()
}

#[test]
fn each_key_runs_its_own_command_and_no_other() {
    let keys = bound();
    assert!(keys.len() >= 12, "only {} keys", keys.len());
    for (i, (a, command)) in keys.iter().enumerate() {
        assert!(
            !keys[i + 1..].iter().any(|(b, _)| b == a),
            "{a:?} runs {command:?} and another"
        );
    }
    // ⌘⇧N is the numbered
    // series, not Add URL.
    let command = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    assert_eq!(runs(format!("{command}-shift-n")), vec![Command::AddSeries]);
    assert_eq!(runs(format!("{command}-n")), vec![Command::AddUrl]);
}

#[test]
fn a_mac_removes_with_backspace_and_others_with_delete() {
    if cfg!(target_os = "macos") {
        assert_eq!(runs("backspace".into()), vec![Command::Remove]);
        assert_eq!(
            Command::Remove.chords().first().map(keystroke).as_deref(),
            Some("cmd-backspace"),
            "the menu shows ⌘⌫"
        );
    } else {
        assert!(runs("backspace".into()).is_empty());
    }
    assert_eq!(runs("delete".into()), vec![Command::Remove]);
}
