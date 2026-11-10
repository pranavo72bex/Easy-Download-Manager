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
use dm_shell::app::power::WhenDone;
use gpui::{Action, App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};

/// What the menus show ticked, and the queues Start Queue and Stop Queue
/// list, as the egui window's menus show them: the menu bar is built again
/// when any of it changes (`Downloads::refresh_menus`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ticks {
    /// The queues, for Start Queue's and Stop Queue's submenus.
    pub queues: Vec<String>,
    /// Downloads › When done's choice.
    pub when_done: WhenDone,
    /// The speed limit in force, in KB/s (0: none): Limit speed is ticked
    /// while there is one, and so is the preset it is.
    pub speed_limit: u64,
    /// View › Categories.
    pub sidebar: bool,
    /// View › Toolbar.
    pub toolbar: bool,
    /// View › Drop target.
    pub drop_target: bool,
    /// The columns' order and those hidden, by key (View › Columns).
    pub column_order: Vec<String>,
    /// The columns hidden, by key.
    pub columns_hidden: Vec<String>,
    /// The toolbar's style, its buttons' order and those hidden (View ›
    /// Customize toolbar).
    pub toolbar_style: String,
    /// The toolbar's buttons in the person's order, by command id.
    pub toolbar_order: Vec<String>,
    /// The toolbar's buttons hidden, by command id.
    pub toolbar_hidden: Vec<String>,
    /// The language setting (View › Language).
    pub language: String,
    /// The sort in force, as the command that sorts so (View › Sort by).
    pub sorted_by: Option<Command>,
    /// Resume's words for the rows chosen, in English (Retry, Start now:
    /// `rows::resume_word`); none for Resume's own.
    pub resume: &'static str,
}

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

/// File › Close (⌘W), as every macOS program has it: the main window's
/// close button, whatever Options says it does.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct CloseWindow;

/// Window › Minimize (⌘M), as every macOS program's Window menu has it.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct Minimize;

/// Window › Zoom.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct Zoom;

/// Window › Bring All to Front.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = dm, no_json)]
pub struct BringAllToFront;

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
            | C::Options
            | C::ToggleDropTarget
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
    // Showing and hiding the categories: macOS's own keys for a sidebar.
    bindings.push(KeyBinding::new(
        "ctrl-cmd-s",
        Run(Command::ToggleCategories),
        None,
    ));
    bindings.push(KeyBinding::new("cmd-w", CloseWindow, None));
    bindings.push(KeyBinding::new("cmd-m", Minimize, None));
    bindings.push(KeyBinding::new("up", SelectPrevious, None));
    bindings.push(KeyBinding::new("down", SelectNext, None));
    cx.bind_keys(bindings);
    // The application's own actions, heard from any window: registered
    // once, here, as the menus are built again whenever the queues change.
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|page: &OpenPage, cx| cx.open_url(&page.address));
    // ⌘W in a dialog: its own close (the main window hears it first).
    cx.on_action(|_: &CloseWindow, cx| crate::native::close_front(cx));
    // The Window menu, on the window in front.
    cx.on_action(|_: &Minimize, cx| {
        if let Some(front) = cx.active_window() {
            cx.defer(move |cx| {
                let _ = front.update(cx, |_, window, _| window.minimize_window());
            });
        }
    });
    cx.on_action(|_: &Zoom, cx| {
        if let Some(front) = cx.active_window() {
            cx.defer(move |cx| {
                let _ = front.update(cx, |_, window, _| window.zoom_window());
            });
        }
    });
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
}

fn item(command: Command) -> MenuItem {
    if command == Command::Quit {
        return MenuItem::action(t(command.label()), Quit);
    }
    MenuItem::action(shown(command), Run(command)).disabled(!carried_out(command))
}

/// A command's words on its menu item: with "…" where it opens a dialog
/// that asks for more first, as the egui window's menus and macOS's own
/// say it.
fn shown(command: Command) -> String {
    let title = t(command.label());
    if command.asks_more() && !title.ends_with('…') {
        format!("{title}…")
    } else {
        title.to_string()
    }
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
pub fn set(cx: &mut App, ticks: &Ticks) {
    cx.set_menus(build(ticks));
}

/// The menu bar's menus, in their order.
fn build(ticks: &Ticks) -> Vec<Menu> {
    let queues = &ticks.queues;
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
            &[C::StopDownload, C::ShowProgress],
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
    let mut file = file;
    // Resume first, in the words it has for the rows chosen (Retry, Start
    // now), as the egui window's File menu says it.
    let resume = if ticks.resume.is_empty() {
        Command::DownloadNow.label()
    } else {
        ticks.resume
    };
    file.items.insert(
        0,
        MenuItem::action(t(resume), Run(C::DownloadNow)).disabled(!carried_out(C::DownloadNow)),
    );
    file.items.extend([
        MenuItem::separator(),
        MenuItem::action(t("Close"), CloseWindow),
    ]);
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
            // Limit speed ticked while a limit is in force, and the preset
            // that is.
            Menu::new(t("Speed limiter")).items(
                [
                    item(C::SpeedLimiterToggle).checked(ticks.speed_limit > 0),
                    MenuItem::separator(),
                ]
                .into_iter()
                .chain(
                    [C::SpeedLow, C::SpeedMedium, C::SpeedHigh, C::SpeedUnlimited]
                        .into_iter()
                        .map(|c| item(c).checked(c.speed_preset() == Some(ticks.speed_limit))),
                )
                .chain([MenuItem::separator(), item(C::SpeedLimiterSettings)]),
            ),
        ),
        // The choice ticked; greyed where this computer gives dm no way to
        // do it (macOS does not hibernate), rather than failing when the
        // queue runs dry.
        MenuItem::submenu(
            Menu::new(t("When done")).items(
                [
                    (C::WhenDoneNothing, WhenDone::Nothing),
                    (C::WhenDoneQuit, WhenDone::Quit),
                    (C::WhenDoneSleep, WhenDone::Sleep),
                    (C::WhenDoneHibernate, WhenDone::Hibernate),
                    (C::WhenDoneShutDown, WhenDone::ShutDown),
                ]
                .into_iter()
                .map(|(c, w)| {
                    item(c)
                        .checked(ticks.when_done == w)
                        .disabled(!w.possible(dm_desktop_base::power::available))
                }),
            ),
        ),
    ]);
    // View, as the egui window's: Categories, Toolbar and Drop target
    // ticked while shown; then Columns, Customize toolbar, Language and Sort
    // by, each ticked as it stands.
    let columns = Menu::new(t("Columns")).items(
        dm_desktop_base::columns::Column::ordered(&ticks.column_order)
            .into_iter()
            // The file name always stays.
            .skip(1)
            .map(|c| {
                MenuItem::action(
                    c.title(),
                    crate::viewmenu::ToggleColumn {
                        key: c.key().to_string(),
                    },
                )
                .checked(!ticks.columns_hidden.iter().any(|h| h == c.key()))
            }),
    );
    let style = dm_desktop_base::toolbar::ToolStyle::named(&ticks.toolbar_style);
    let customize = Menu::new(t("Customize toolbar")).items(
        dm_desktop_base::toolbar::TOOL_STYLES
            .iter()
            .map(|(s, words)| {
                MenuItem::action(
                    t(words),
                    crate::viewmenu::ToolbarStyle {
                        style: s.name().to_string(),
                    },
                )
                .checked(*s == style)
            })
            .chain([MenuItem::separator()])
            .chain(
                dm_shell::app::commands::toolbar_in(&ticks.toolbar_order)
                    .into_iter()
                    .map(|(c, words)| {
                        MenuItem::action(
                            t(words),
                            crate::viewmenu::ToggleToolbarButton {
                                id: c.id().to_string(),
                            },
                        )
                        .checked(!ticks.toolbar_hidden.iter().any(|h| h == c.id()))
                    }),
            ),
    );
    let speaks = dm_desktop_base::i18n::chosen(&ticks.language);
    let language = Menu::new(t("Language")).items(dm_desktop_base::i18n::LANGUAGES.iter().map(
        |(code, name)| {
            MenuItem::action(
                *name,
                crate::viewmenu::SetLanguage {
                    code: code.to_string(),
                },
            )
            .checked(*code == speaks)
        },
    ));
    let sort = Menu::new(t("Sort by")).items(
        [
            C::SortByName,
            C::SortBySize,
            C::SortByStatus,
            C::SortByTimeLeft,
            C::SortByRate,
            C::SortByDate,
            C::SortByDescription,
        ]
        .into_iter()
        .map(|c| item(c).checked(ticks.sorted_by == Some(c))),
    );
    let view = Menu::new(t("View")).items([
        item(C::ToggleCategories).checked(ticks.sidebar),
        item(C::ToggleToolbar).checked(ticks.toolbar),
        item(C::ToggleDropTarget).checked(ticks.drop_target),
        MenuItem::separator(),
        MenuItem::submenu(columns),
        MenuItem::submenu(customize),
        MenuItem::submenu(language),
        MenuItem::submenu(sort),
    ]);
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
    // Edit, as every macOS program has it: the text fields' own Undo, Redo,
    // Cut, Copy and Paste (the system's actions, so its services and
    // dictation work too). The list's Select all stays in File.
    let edit = Menu::new(t("Edit")).items([
        MenuItem::os_action(t("Undo"), gpui_base::input::Undo, OsAction::Undo),
        MenuItem::os_action(t("Redo"), gpui_base::input::Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action(t("Cut"), gpui_base::input::Cut, OsAction::Cut),
        MenuItem::os_action(t("Copy"), gpui_base::input::Copy, OsAction::Copy),
        MenuItem::os_action(t("Paste"), gpui_base::input::Paste, OsAction::Paste),
    ]);
    // Window, as every macOS program has it.
    let window = Menu::new(t("Window")).items([
        MenuItem::action(t("Minimize"), Minimize),
        MenuItem::action(t("Zoom"), Zoom),
        MenuItem::separator(),
        MenuItem::action(t("Bring All to Front"), BringAllToFront),
    ]);
    vec![app, tasks, file, edit, downloads, view, window, help]
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn an_item_that_opens_a_dialog_says_so_with_an_ellipsis() {
        let menus = build(&Ticks::default());
        assert!(find(&menus, "Add numbered series…").is_some());
        assert!(find(&menus, "Options…").is_some());
        // One that acts at once has none.
        assert!(find(&menus, Command::StopDownload.label()).is_some());
        assert!(find(&menus, "Add numbered series").is_none());
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
}
