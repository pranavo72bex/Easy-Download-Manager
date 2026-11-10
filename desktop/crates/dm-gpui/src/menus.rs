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

/// Start Queue or Stop Queue for the queue named, whatever the list shows
/// (B64): Downloads › Start Queue's and Stop Queue's submenus, and the
/// toolbar's arrows beside them.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct RunOnQueue {
    /// Start Queue or Stop Queue.
    pub command: Command,
    /// The queue, by its name in dm's list.
    pub queue: String,
}

/// Ends dm (Exit, ⌘Q): the application's, so it is heard from any window
/// in front, as Quit is in every macOS program; the list's commands
/// (`Run`) are the main window's.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct Quit;

/// Chooses the row above, as ↑ does in a macOS list.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct SelectPrevious;

/// Chooses the row below, as ↓ does.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct SelectNext;

/// A page of the project's, or the email that tells a friend: Help's
/// topics, home page, support and Tell a friend, opened by the system.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct OpenPage {
    /// The page's address (or the email's `mailto:`).
    pub address: String,
}

/// Help › Tip of the day.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct TipOfTheDay;

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

/// What the shell can carry out today: Exit, and the commands the
/// application carries out by itself (`commands::verb`); the rest open a
/// dialog, which comes with its slice.
pub fn carried_out(command: Command) -> bool {
    use Command as C;
    matches!(
        command,
        C::Quit
            | C::AddUrl
            | C::DownloadNow
            | C::StopDownload
            | C::Redownload
            | C::RemoveFromQueue
            | C::MoveUp
            | C::MoveDown
            | C::ResumeAll
            | C::StopAll
            | C::DeleteCompleted
            | C::CopyLink
            | C::Remove
            | C::DeleteFile
            | C::CopyDetails
            | C::Find
            | C::FindNext
            | C::FindPrevious
            | C::Scheduler
            | C::SiteFiles
            | C::ShowProgress
            | C::AddSeries
            | C::AddFromClipboard
            | C::About
            | C::CheckForUpdates
            | C::UpdateLink
            | C::MoveToQueue
            | C::MoveRename
            | C::Properties
            | C::OpenFile
            | C::OpenWith
            | C::Preview
            | C::OpenFolder
            | C::ExtractHere
            | C::ExtractTo
            | C::SelectAll
            | C::Import
            | C::Export
            | C::Help
            | C::ToggleCategories
            | C::ToggleToolbar
            | C::SortByName
            | C::SortBySize
            | C::SortByStatus
            | C::SortByTimeLeft
            | C::SortByRate
            | C::SortByDate
            | C::SortByDescription
            | C::SpeedLimiterToggle
            | C::SpeedLimiterSettings
            | C::SpeedLow
            | C::SpeedMedium
            | C::SpeedHigh
            | C::SpeedUnlimited
            | C::StartQueue
            | C::StopQueue
            | C::WhenDoneNothing
            | C::WhenDoneQuit
            | C::WhenDoneSleep
            | C::WhenDoneHibernate
            | C::WhenDoneShutDown
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
            if *command == Command::Quit {
                bindings.push(KeyBinding::new(keys, Quit, None));
            } else {
                bindings.push(KeyBinding::new(keys, Run(*command), None));
            }
        }
    }
    bindings.push(KeyBinding::new("up", SelectPrevious, None));
    bindings.push(KeyBinding::new("down", SelectNext, None));
    cx.bind_keys(bindings);
    // The application's own actions, heard from any window: registered
    // once, here, as the menus are built again whenever the queues change.
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|page: &OpenPage, cx| cx.open_url(&page.address));
}

fn item(command: Command) -> MenuItem {
    if command == Command::Quit {
        return MenuItem::action(t(command.label()), Quit);
    }
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
pub fn set(cx: &mut App, queues: &[String]) {
    use Command as C;
    // Start Queue and Stop Queue list the queues; the one clicked is started
    // or stopped, whatever the list shows (B64).
    let on_queues = |command: Command| {
        MenuItem::submenu(
            Menu::new(t(command.label())).items(queues.iter().map(|queue| {
                MenuItem::action(
                    dm_desktop_base::i18n::name(queue),
                    RunOnQueue {
                        command,
                        queue: queue.clone(),
                    },
                )
            })),
        )
    };
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
            &[C::Scheduler],
        ],
    );
    downloads.items.extend([
        on_queues(C::StartQueue),
        on_queues(C::StopQueue),
        MenuItem::separator(),
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
    // Help: dm's guide and its topics, Tip of the day, the project's home
    // page, support and telling a friend (gaps row 91), as the egui
    // window's Help menu has them.
    let page =
        |words: &str, address: String| MenuItem::action(words.to_string(), OpenPage { address });
    let mut help = menu("Help", &[&[C::Help]]);
    help.items.extend(
        dm_desktop_base::project::guide_topics()
            .into_iter()
            .map(|(words, address)| page(words, address)),
    );
    help.items.extend([
        MenuItem::action(t("Tip of the day"), TipOfTheDay),
        MenuItem::separator(),
        page(t("Home page"), dm_desktop_base::project::SOURCE.to_string()),
        page(
            t("Contact support"),
            dm_desktop_base::project::SUPPORT.to_string(),
        ),
        page(
            t("Tell a friend"),
            dm_desktop_base::project::tell_a_friend(),
        ),
        MenuItem::separator(),
        item(C::CheckForUpdates),
    ]);
    cx.set_menus([app, tasks, file, downloads, view, help]);
}
