// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's menus where the system keeps them: on macOS the menu bar, with the
//! app menu macOS programs have (About, Options, Services, Hide, Quit) and
//! then the egui window's own menus (Tasks, File, Downloads, View, Help),
//! built from the application's command registry (`Command`: each one's
//! label, in the language in force, and its keys), in the same order. A
//! command the shell cannot carry out yet is shown greyed, not run.

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::{Chord, Command, Key};
use gpui::{Action, App, KeyBinding, Menu, MenuItem, SystemMenuType};

/// A command of dm's, as a menu item or a key runs it.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct Run(pub Command);

/// Hides dm, as every macOS program's app menu does.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct Hide;

/// Hides the other programs.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct HideOthers;

/// Shows the other programs again.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct ShowAll;

/// What the shell can carry out today.
pub fn carried_out(command: Command) -> bool {
    matches!(
        command,
        Command::Quit | Command::ResumeAll | Command::StopAll | Command::DeleteCompleted
    )
}

/// A key as GPUI names it in a binding.
fn key_name(key: Key) -> &'static str {
    match key {
        Key::A => "a",
        Key::F => "f",
        Key::I => "i",
        Key::N => "n",
        Key::Q => "q",
        Key::R => "r",
        Key::Comma => ",",
        Key::Period => ".",
        Key::F1 => "f1",
        Key::F3 => "f3",
        Key::Backspace => "backspace",
        Key::Delete => "delete",
    }
}

/// A chord as GPUI writes a keystroke: ⌘ on macOS and Ctrl elsewhere, as
/// dm's chords mean.
pub fn keystroke(chord: &Chord) -> String {
    let mut words = Vec::new();
    if chord.command {
        words.push(if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        });
    }
    if chord.shift {
        words.push("shift");
    }
    words.push(key_name(chord.key));
    words.join("-")
}

/// Every command's keys, bound to running it.
pub fn bind_keys(cx: &mut App) {
    let mut bindings = Vec::new();
    for command in Command::ALL {
        for chord in command.chords() {
            // Bound once, at start: GPUI keeps a binding's keystroke for the
            // life of the program.
            let keys: &'static str = keystroke(&chord).leak();
            // Checked first: GPUI's binding panics on a keystroke it cannot
            // read, and a key dm cannot bind is said, not a crash.
            if let Err(e) = gpui::Keystroke::parse(keys) {
                eprintln!("dm-gpui: {} cannot be bound to {keys}: {e:?}", command.id());
                continue;
            }
            bindings.push(KeyBinding::new(keys, Run(*command), None));
        }
    }
    cx.bind_keys(bindings);
}

fn item(command: Command) -> MenuItem {
    MenuItem::action(t(command.label()), Run(command)).disabled(!carried_out(command))
}

fn items(commands: &[Command]) -> Vec<MenuItem> {
    commands.iter().copied().map(item).collect()
}

fn menu(title: &'static str, groups: &[&[Command]]) -> Menu {
    let mut all = Vec::new();
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            all.push(MenuItem::separator());
        }
        all.extend(items(group));
    }
    Menu::new(t(title)).items(all)
}

/// The menu bar.
pub fn set(cx: &mut App) {
    use Command as C;
    let app = Menu::new("dm").items([
        item(C::About),
        MenuItem::separator(),
        item(C::Options),
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Hide dm", Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        item(C::Quit),
    ]);
    let tasks = menu(
        "Tasks",
        &[
            &[C::AddUrl, C::AddSeries, C::AddFromClipboard],
            &[C::Export, C::Import],
            &[C::SiteFiles],
        ],
    );
    let file = menu(
        "File",
        &[
            &[C::DownloadNow, C::StopDownload, C::ShowProgress],
            &[
                C::Remove,
                C::DeleteFile,
                C::Redownload,
                C::UpdateLink,
                C::CopyLink,
                C::CopyDetails,
                C::MoveRename,
            ],
            &[
                C::OpenFile,
                C::OpenWith,
                C::Preview,
                C::OpenFolder,
                C::ExtractHere,
                C::ExtractTo,
                C::Properties,
            ],
            &[
                C::MoveToQueue,
                C::RemoveFromQueue,
                C::MoveUp,
                C::MoveDown,
                C::SelectAll,
            ],
        ],
    );
    let mut downloads = menu(
        "Downloads",
        &[
            &[C::ResumeAll, C::StopAll, C::DeleteCompleted],
            &[C::Find, C::FindNext, C::FindPrevious],
            &[C::Scheduler, C::StartQueue, C::StopQueue],
        ],
    );
    downloads.items.extend([
        MenuItem::submenu(
            Menu::new(t("Speed limiter")).items(
                [item(C::SpeedLimiterToggle), MenuItem::separator()]
                    .into_iter()
                    .chain(items(&[
                        C::SpeedLow,
                        C::SpeedMedium,
                        C::SpeedHigh,
                        C::SpeedUnlimited,
                    ]))
                    .chain([MenuItem::separator(), item(C::SpeedLimiterSettings)]),
            ),
        ),
        MenuItem::submenu(Menu::new(t("When done")).items(items(&[
            C::WhenDoneNothing,
            C::WhenDoneQuit,
            C::WhenDoneSleep,
            C::WhenDoneHibernate,
            C::WhenDoneShutDown,
        ]))),
    ]);
    let view = menu(
        "View",
        &[
            &[C::ToggleCategories, C::ToggleToolbar, C::ToggleDropTarget],
            &[
                C::SortByName,
                C::SortBySize,
                C::SortByStatus,
                C::SortByTimeLeft,
                C::SortByRate,
                C::SortByDate,
                C::SortByDescription,
            ],
        ],
    );
    let help = menu("Help", &[&[C::Help], &[C::CheckForUpdates]]);
    cx.set_menus([app, tasks, file, downloads, view, help]);
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
}
