// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every action of the window, declared once (gaps row N20): where each shows, and, where a
//! phone leaves it out, why. The menu bar's menus (`menus.rs`, `menus/bar_menus.rs`), the row
//! menu and a card's menu (`rowmenu.rs`) are built from the lists here; the toolbar's buttons
//! are `commands::TOOLBAR` and a place's menu is `places::place_menu`, each one list of its
//! own. The phone's layout draws the same menus (the ⋮ sheet holds the toolbar's buttons and
//! the window's menus, a card's sheet the row menu, a place's sheet its menu), leaving out what
//! [`on_phone`] says is left out. [`on_phone`] matches every action by name, so a new one does
//! not build until it says whether a phone has it; the test `every_action_is_reached_by_touch`
//! (`window_tests/reach.rs`) walks the phone's sheets and fails on an action they cannot reach,
//! or one they show that the phone leaves out.

use dm_shell::app::commands::Command;
use dm_shell::app::power::WhenDone;

use Command as C;

/// Tasks, its groups in order (Exit last, in the window's own menus).
pub(crate) const TASKS: &[&[Command]] = &[
    &[C::AddUrl, C::AddSeries, C::AddFromClipboard],
    &[C::Export, C::Import],
    &[C::SiteFiles],
];

/// File, its groups after Resume (first, in the words it has for the rows chosen).
pub(crate) const FILE: &[&[Command]] = &[
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
];

/// File's group before Close: Back up dm… and Restore… (gaps row W16).
pub(crate) const FILE_LAST: &[Command] = &[C::BackUp, C::Restore];

/// Downloads, its groups before the queues' submenus (Options last, in the window's own menus).
pub(crate) const DOWNLOADS: &[&[Command]] = &[
    &[C::ResumeAll, C::StopAll, C::DeleteCompleted],
    &[C::Find, C::FindNext, C::FindPrevious],
    &[C::Scheduler],
];

/// Downloads' submenus that list the queues.
pub(crate) const QUEUES: [Command; 2] = [C::StartQueue, C::StopQueue];

/// Downloads › When done, each with the choice it makes.
pub(crate) const WHEN_DONE: [(Command, WhenDone); 5] = [
    (C::WhenDoneNothing, WhenDone::Nothing),
    (C::WhenDoneQuit, WhenDone::Quit),
    (C::WhenDoneSleep, WhenDone::Sleep),
    (C::WhenDoneHibernate, WhenDone::Hibernate),
    (C::WhenDoneShutDown, WhenDone::ShutDown),
];

/// View's first items, each ticked while what it shows is shown.
pub(crate) const VIEW_SHOWN: [Command; 3] =
    [C::ToggleCategories, C::ToggleToolbar, C::ToggleDropTarget];

/// View › Sort by.
pub(crate) const SORT: [Command; 7] = [
    C::SortByName,
    C::SortBySize,
    C::SortByStatus,
    C::SortByTimeLeft,
    C::SortByRate,
    C::SortByDate,
    C::SortByDescription,
];

/// The row menu's groups (and so a card's menu's), in order; its words are `rowmenu.rs`'s.
pub(crate) const ROW: &[&[Command]] = &[
    &[
        C::OpenFile,
        C::OpenWith,
        C::Preview,
        C::OpenFolder,
        C::ExtractHere,
        C::ExtractTo,
        C::MoveRename,
        C::Redownload,
    ],
    &[
        C::DownloadNow,
        C::StopDownload,
        C::UpdateLink,
        C::CopyLink,
        C::CopyDetails,
        C::Remove,
        C::DeleteFile,
    ],
    &[C::MoveToQueue, C::RemoveFromQueue, C::MoveUp, C::MoveDown],
    &[C::Properties],
];

/// The Dock icon's menu on macOS, before its Open download folder.
pub(crate) const DOCK: [Command; 3] = [C::AddUrl, C::ResumeAll, C::StopAll];

/// One action of the window: a command of the application's, or one of the window's own.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Act {
    /// A command of the application's registry.
    Run(Command),
    /// File › Close.
    Close,
    /// View › Columns.
    Columns,
    /// View › Customize toolbar.
    CustomizeToolbar,
    /// View › Text size.
    TextSize,
    /// View › Language.
    Language,
    /// View › Statistics….
    Statistics,
    /// Help's guide topics, Home page, Contact support and Tell a friend: pages opened.
    HelpPages,
    /// Help › Tip of the day.
    TipOfTheDay,
    /// Help › Copy a report.
    CopyReport,
    /// The row menu's Delete file after.
    DeleteAfter,
    /// A card menu's Share.
    Share,
    /// A card menu's Select: several cards chosen with taps.
    ChooseSeveral,
    /// A category's Open folder, in its place menu.
    PlaceFolder,
    /// A category's Add or edit categories…, in its place menu.
    EditCategories,
    /// macOS's Edit menu: Undo, Redo, Cut, Copy and Paste.
    Edit,
    /// macOS's Window menu: Minimize, Zoom and Bring All to Front.
    WindowMenu,
    /// macOS's app menu's own items: Services, Hide dm, Hide Others and Show All.
    AppMenu,
    /// The Dock menu's Open download folder.
    DockFolder,
}

impl Act {
    /// Every action of the window's own, beside the registry's commands.
    #[cfg(test)]
    pub(crate) const OWN: [Act; 18] = [
        Act::Close,
        Act::Columns,
        Act::CustomizeToolbar,
        Act::TextSize,
        Act::Language,
        Act::Statistics,
        Act::HelpPages,
        Act::TipOfTheDay,
        Act::CopyReport,
        Act::DeleteAfter,
        Act::Share,
        Act::ChooseSeveral,
        Act::PlaceFolder,
        Act::EditCategories,
        Act::Edit,
        Act::WindowMenu,
        Act::AppMenu,
        Act::DockFolder,
    ];

    /// Every action: the registry's commands, then the window's own.
    #[cfg(test)]
    pub(crate) fn all() -> Vec<Act> {
        Command::ALL
            .iter()
            .map(|c| Act::Run(*c))
            .chain(Act::OWN)
            .collect()
    }
}

/// Whether a phone has an action.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phone {
    /// Drawn there, and does there what it does on a computer.
    Shown,
    /// Drawn there greyed, as on a computer that cannot do it: why.
    Greyed(&'static str),
    /// Left out there: why (gaps row N24, `docs/decisions.md` 2026-11-22).
    Left(&'static str),
    /// Drawn on a phone alone: why a computer has it otherwise.
    PhoneOnly(&'static str),
}

/// Whether a phone has `act`, and why not where it does not.
pub(crate) const fn on_phone(act: Act) -> Phone {
    use Phone::*;
    match act {
        Act::Run(command) => command_on_phone(command),
        Act::Columns => Left("the table's columns: the cards have none"),
        Act::CustomizeToolbar => Left("the toolbar's look and buttons: the phone has no toolbar"),
        Act::Edit => Left(
            "macOS's menu bar: a text field's own menu edits its text, and Undo after Remove \
             is the bar over the list's foot",
        ),
        Act::WindowMenu => Left("a computer's windows: a phone shows the app full screen"),
        Act::AppMenu => Left("macOS's app menu"),
        Act::DockFolder => Left("the Dock's menu: a phone has no Dock"),
        Act::Share => PhoneOnly("the phone's share sheet: a computer drags the file out"),
        Act::Close
        | Act::TextSize
        | Act::Language
        | Act::Statistics
        | Act::HelpPages
        | Act::TipOfTheDay
        | Act::CopyReport
        | Act::DeleteAfter
        | Act::ChooseSeveral
        | Act::PlaceFolder
        | Act::EditCategories => Shown,
    }
}

/// [`on_phone`] for the registry's commands, each by name.
const fn command_on_phone(command: Command) -> Phone {
    use Phone::*;
    match command {
        C::ToggleCategories => Left("a computer's side panel: the phone's places are a drawer"),
        C::ToggleToolbar => Left("a computer's toolbar: the phone's top bar and ⋮ hold it"),
        C::ToggleDropTarget => Left("a floating window over a computer's desktop"),
        C::CheckForUpdates => Left(
            "it knows only a computer's installers: the store or the APK updates the phone's app",
        ),
        C::WhenDoneSleep | C::WhenDoneHibernate | C::WhenDoneShutDown => {
            Greyed("Android lets no app put the phone to sleep or turn it off")
        }
        C::AddUrl
        | C::AddSeries
        | C::AddFromClipboard
        | C::Import
        | C::Export
        | C::BackUp
        | C::Restore
        | C::SiteFiles
        | C::Quit
        | C::DownloadNow
        | C::StopDownload
        | C::Remove
        | C::DeleteFile
        | C::Redownload
        | C::UpdateLink
        | C::CopyLink
        | C::CopyDetails
        | C::MoveToQueue
        | C::RemoveFromQueue
        | C::MoveUp
        | C::MoveDown
        | C::Properties
        | C::ShowProgress
        | C::OpenFile
        | C::OpenWith
        | C::Preview
        | C::OpenFolder
        | C::ExtractHere
        | C::ExtractTo
        | C::MoveRename
        | C::SelectAll
        | C::ResumeAll
        | C::StopAll
        | C::DeleteCompleted
        | C::Find
        | C::FindNext
        | C::FindPrevious
        | C::Scheduler
        | C::StartQueue
        | C::StopQueue
        | C::SpeedLimiterToggle
        | C::SpeedLimiterSettings
        | C::SpeedLow
        | C::SpeedMedium
        | C::SpeedHigh
        | C::SpeedUnlimited
        | C::WhenDoneNothing
        | C::WhenDoneQuit
        | C::Options
        | C::SortByName
        | C::SortBySize
        | C::SortByStatus
        | C::SortByTimeLeft
        | C::SortByRate
        | C::SortByDate
        | C::SortByDescription
        | C::Help
        | C::About => Shown,
    }
}

/// Whether `act` is drawn, on a phone (`phone`) or a computer.
pub(crate) const fn drawn(act: Act, phone: bool) -> bool {
    match on_phone(act) {
        Phone::Left(_) => !phone,
        Phone::PhoneOnly(_) => phone,
        Phone::Shown | Phone::Greyed(_) => true,
    }
}

#[cfg(test)]
/// The action a menu item runs, as the list names it; `None` for one the list does not know.
pub(crate) fn act_of(action: &dyn gpui::Action) -> Option<Act> {
    use crate::window::{menus, placemenu, rowmenu, viewmenu};
    use dm_shell::app::places::PlaceAction;
    let any = action.as_any();
    if let Some(run) = any.downcast_ref::<menus::Run>() {
        return Some(Act::Run(run.0));
    }
    if let Some(on) = any.downcast_ref::<menus::RunOnQueue>() {
        return Some(Act::Run(on.command));
    }
    if let Some(on) = any.downcast_ref::<placemenu::OnPlace>() {
        return Some(match on.1 {
            PlaceAction::Run(command) => Act::Run(command),
            PlaceAction::OpenFolder => Act::PlaceFolder,
            PlaceAction::EditCategories => Act::EditCategories,
        });
    }
    let is = |of: &[std::any::TypeId]| of.contains(&any.type_id());
    use std::any::TypeId as T;
    Some(if any.is::<menus::Quit>() {
        Act::Run(C::Quit)
    } else if any.is::<menus::CloseWindow>() {
        Act::Close
    } else if any.is::<viewmenu::ToggleColumn>() {
        Act::Columns
    } else if is(&[
        T::of::<viewmenu::ToolbarStyle>(),
        T::of::<viewmenu::ToggleToolbarButton>(),
    ]) {
        Act::CustomizeToolbar
    } else if is(&[
        T::of::<viewmenu::TextLarger>(),
        T::of::<viewmenu::TextSmaller>(),
        T::of::<viewmenu::TextNormal>(),
    ]) {
        Act::TextSize
    } else if any.is::<viewmenu::SetLanguage>() {
        Act::Language
    } else if any.is::<crate::dialogs::statistics::ShowStatistics>() {
        Act::Statistics
    } else if any.is::<menus::OpenPage>() {
        Act::HelpPages
    } else if any.is::<menus::TipOfTheDay>() {
        Act::TipOfTheDay
    } else if any.is::<menus::CopyReport>() {
        Act::CopyReport
    } else if any.is::<rowmenu::DeleteAfter>() {
        Act::DeleteAfter
    } else if any.is::<rowmenu::ShareFile>() {
        Act::Share
    } else if any.is::<rowmenu::ChooseSeveral>() {
        Act::ChooseSeveral
    } else if is(&[
        T::of::<gpui_base::input::Undo>(),
        T::of::<gpui_base::input::Redo>(),
        T::of::<gpui_base::input::Cut>(),
        T::of::<gpui_base::input::Copy>(),
        T::of::<gpui_base::input::Paste>(),
    ]) {
        Act::Edit
    } else if is(&[
        T::of::<menus::Minimize>(),
        T::of::<menus::Zoom>(),
        T::of::<menus::BringAllToFront>(),
    ]) {
        Act::WindowMenu
    } else if is(&[
        T::of::<menus::Hide>(),
        T::of::<menus::HideOthers>(),
        T::of::<menus::ShowAll>(),
    ]) {
        Act::AppMenu
    } else if any.is::<menus::OpenDownloadFolder>() {
        Act::DockFolder
    } else {
        return None;
    })
}

#[cfg(test)]
/// Every action `items` reach, in their submenus too; an item the list does not know is put
/// in `unknown` by its words. A system's own submenu (macOS's Services) is the app menu's.
pub(crate) fn walk(
    items: &[gpui::OwnedMenuItem],
    reached: &mut Vec<Act>,
    unknown: &mut Vec<String>,
) {
    for item in items {
        match item {
            gpui::OwnedMenuItem::Action { name, action, .. } => match act_of(action.as_ref()) {
                Some(act) => reached.push(act),
                None => unknown.push(name.clone()),
            },
            gpui::OwnedMenuItem::Submenu(menu) => walk(&menu.items, reached, unknown),
            gpui::OwnedMenuItem::SystemMenu(_) => reached.push(Act::AppMenu),
            gpui::OwnedMenuItem::Separator => {}
        }
    }
}
