// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every action dm can take, by name: the application's command registry.
//!
//! The menu bar, the toolbar, the row menu, the keyboard and the control
//! protocol all run a `Command`, so a command that exists is reachable from
//! all of them, in any presentation. What is here is free of the window
//! toolkit: each command's stable name, its English label (the catalogs'
//! key), whether it asks for more first, the speed a preset holds, and its
//! keys as [`Chord`]s, which a presentation turns into its own shortcuts.
//! The words in the language in force, and matching a key press, are the
//! presentation's (dm-gpui's `menus.rs`). Which commands apply to the
//! rows chosen is decided here too ([`applies`]).

use std::sync::Arc;

use dm_core::{Entry, Id, State};

#[cfg(test)]
use crate::app::power::WhenDone;
use crate::Settings;

pub use verbs::{
    is_verb, limited, recheck, remembered_limit, verb, when_done, when_done_command, Ran,
    DEFAULT_LIMIT,
};

mod verbs;

/// The speed presets, in KB/s: one click from the tray or Downloads ›
/// Speed limiter, beside the exact limit Speed limiter settings keeps.
pub const SPEED_LOW: u64 = 256;
/// The middle speed preset, in KB/s.
pub const SPEED_MEDIUM: u64 = 1024;
/// The highest speed preset, in KB/s.
pub const SPEED_HIGH: u64 = 4096;

/// One action, as the menu, toolbar, keyboard or control protocol asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    // Program
    /// Opens the dialog to add a download by its address.
    AddUrl,
    /// Opens the dialog to add a numbered series of addresses.
    AddSeries,
    /// Adds the addresses on the clipboard.
    AddFromClipboard,
    /// Reads a list of downloads from a file.
    Import,
    /// Writes the list of downloads to a file.
    Export,
    /// Writes dm's settings, queues and list to one file, the sign-ins sealed with a passphrase
    /// where one is given (gaps row W16).
    BackUp,
    /// Puts back what such a file holds.
    Restore,
    /// Opens the dialog to download the files a web page links to.
    SiteFiles,
    /// Closes dm.
    Quit,
    // File (the selected rows)
    /// Starts the selected downloads at once.
    DownloadNow,
    /// Stops the selected downloads.
    StopDownload,
    /// Removes the selected rows from the list, keeping their files.
    Remove,
    /// Removes the selected rows and deletes their files.
    DeleteFile,
    /// Downloads the selected files again from the start.
    Redownload,
    /// Gives the selected download a new address.
    UpdateLink,
    /// Copies the selected download's address.
    CopyLink,
    /// Copies the selected download's details, for a support request.
    CopyDetails,
    /// Moves the selected downloads into a queue.
    MoveToQueue,
    /// Takes the selected downloads out of their queue.
    RemoveFromQueue,
    /// Moves the selected downloads one place up in their queue.
    MoveUp,
    /// Moves the selected downloads one place down in their queue.
    MoveDown,
    /// Opens the selected download's properties.
    Properties,
    /// Opens the selected download's progress window.
    ShowProgress,
    /// Opens the finished file.
    OpenFile,
    /// Opens the finished file with a program the person picks.
    OpenWith,
    /// Opens a preview of the file.
    Preview,
    /// Opens the folder the file is in.
    OpenFolder,
    /// Extracts an archive into its own folder.
    ExtractHere,
    /// Extracts an archive into a folder the person picks.
    ExtractTo,
    /// Moves or renames the finished file.
    MoveRename,
    /// Selects every row.
    SelectAll,
    // Downloads
    /// Resumes every stopped download.
    ResumeAll,
    /// Stops every running download.
    StopAll,
    /// Removes every finished download from the list.
    DeleteCompleted,
    /// Opens the search box.
    Find,
    /// Goes to the next row that matches the search.
    FindNext,
    /// Goes to the previous row that matches the search.
    FindPrevious,
    /// Opens the scheduler for queues.
    Scheduler,
    /// Starts the selected queue.
    StartQueue,
    /// Stops the selected queue.
    StopQueue,
    /// Turns the speed limit on or off.
    SpeedLimiterToggle,
    /// Opens the speed limiter's settings.
    SpeedLimiterSettings,
    /// Sets the speed limit to the low preset.
    SpeedLow,
    /// Sets the speed limit to the middle preset.
    SpeedMedium,
    /// Sets the speed limit to the high preset.
    SpeedHigh,
    /// Removes the speed limit.
    SpeedUnlimited,
    /// Does nothing when all downloads are finished.
    WhenDoneNothing,
    /// Closes dm when all downloads are finished.
    WhenDoneQuit,
    /// Puts the computer to sleep when all downloads are finished.
    WhenDoneSleep,
    /// Hibernates the computer when all downloads are finished.
    WhenDoneHibernate,
    /// Shuts the computer down when all downloads are finished.
    WhenDoneShutDown,
    /// Opens Options.
    Options,
    // View
    /// Shows or hides the category list.
    ToggleCategories,
    /// Shows or hides the toolbar.
    ToggleToolbar,
    /// Shows or hides the drop target.
    ToggleDropTarget,
    /// Sorts the list by name.
    SortByName,
    /// Sorts the list by size.
    SortBySize,
    /// Sorts the list by status.
    SortByStatus,
    /// Sorts the list by time left.
    SortByTimeLeft,
    /// Sorts the list by transfer rate.
    SortByRate,
    /// Sorts the list by date.
    SortByDate,
    /// Sorts the list by description.
    SortByDescription,
    // Help
    /// Opens the help.
    Help,
    /// Opens About.
    About,
    /// Checks whether a newer version exists.
    CheckForUpdates,
}

impl Command {
    /// Every command, in menu order. Anything not here cannot be run.
    pub const ALL: &'static [Command] = &[
        Command::AddUrl,
        Command::AddSeries,
        Command::AddFromClipboard,
        Command::Import,
        Command::Export,
        Command::BackUp,
        Command::Restore,
        Command::SiteFiles,
        Command::Quit,
        Command::DownloadNow,
        Command::StopDownload,
        Command::Remove,
        Command::DeleteFile,
        Command::Redownload,
        Command::UpdateLink,
        Command::CopyLink,
        Command::CopyDetails,
        Command::MoveToQueue,
        Command::RemoveFromQueue,
        Command::MoveUp,
        Command::MoveDown,
        Command::Properties,
        Command::ShowProgress,
        Command::OpenFile,
        Command::OpenWith,
        Command::Preview,
        Command::OpenFolder,
        Command::ExtractHere,
        Command::ExtractTo,
        Command::MoveRename,
        Command::SelectAll,
        Command::ResumeAll,
        Command::StopAll,
        Command::DeleteCompleted,
        Command::Find,
        Command::FindNext,
        Command::FindPrevious,
        Command::Scheduler,
        Command::StartQueue,
        Command::StopQueue,
        Command::SpeedLimiterToggle,
        Command::SpeedLimiterSettings,
        Command::SpeedLow,
        Command::SpeedMedium,
        Command::SpeedHigh,
        Command::SpeedUnlimited,
        Command::WhenDoneNothing,
        Command::WhenDoneQuit,
        Command::WhenDoneSleep,
        Command::WhenDoneHibernate,
        Command::WhenDoneShutDown,
        Command::Options,
        Command::ToggleCategories,
        Command::ToggleToolbar,
        Command::ToggleDropTarget,
        Command::SortByName,
        Command::SortBySize,
        Command::SortByStatus,
        Command::SortByTimeLeft,
        Command::SortByRate,
        Command::SortByDate,
        Command::SortByDescription,
        Command::Help,
        Command::About,
        Command::CheckForUpdates,
    ];

    /// The stable name the control protocol and the catalog use.
    pub fn id(self) -> &'static str {
        match self {
            Command::AddUrl => "program.add_url",
            Command::AddSeries => "program.add_series",
            Command::AddFromClipboard => "program.add_from_clipboard",
            Command::Import => "program.import",
            Command::Export => "program.export",
            Command::BackUp => "program.back_up",
            Command::Restore => "program.restore",
            Command::SiteFiles => "program.site_files",
            Command::Quit => "program.exit",
            Command::DownloadNow => "file.download_now",
            Command::StopDownload => "file.stop_download",
            Command::Remove => "file.remove",
            Command::DeleteFile => "file.delete_file",
            Command::Redownload => "file.redownload",
            Command::UpdateLink => "file.update_link",
            Command::CopyLink => "file.copy_link",
            Command::CopyDetails => "file.copy_details",
            Command::MoveToQueue => "file.move_to_queue",
            Command::RemoveFromQueue => "file.remove_from_queue",
            Command::MoveUp => "file.move_up",
            Command::MoveDown => "file.move_down",
            Command::Properties => "file.properties",
            Command::ShowProgress => "file.show_progress",
            Command::OpenFile => "file.open",
            Command::OpenWith => "file.open_with",
            Command::Preview => "file.preview",
            Command::OpenFolder => "file.open_folder",
            Command::ExtractHere => "file.extract_here",
            Command::ExtractTo => "file.extract_to",
            Command::MoveRename => "file.move_rename",
            Command::SelectAll => "file.select_all",
            Command::ResumeAll => "downloads.resume_all",
            Command::StopAll => "downloads.stop_all",
            Command::DeleteCompleted => "downloads.clear_finished",
            Command::Find => "downloads.find",
            Command::FindNext => "downloads.find_next",
            Command::FindPrevious => "downloads.find_previous",
            Command::Scheduler => "downloads.scheduler",
            Command::StartQueue => "downloads.start_queue",
            Command::StopQueue => "downloads.stop_queue",
            Command::SpeedLimiterToggle => "downloads.speed_limiter.toggle",
            Command::SpeedLimiterSettings => "downloads.speed_limiter.settings",
            Command::SpeedLow => "downloads.speed.low",
            Command::SpeedMedium => "downloads.speed.medium",
            Command::SpeedHigh => "downloads.speed.high",
            Command::SpeedUnlimited => "downloads.speed.unlimited",
            Command::WhenDoneNothing => "downloads.when_done.nothing",
            Command::WhenDoneQuit => "downloads.when_done.exit",
            Command::WhenDoneSleep => "downloads.when_done.sleep",
            Command::WhenDoneHibernate => "downloads.when_done.hibernate",
            Command::WhenDoneShutDown => "downloads.when_done.turn_off",
            Command::Options => "downloads.options",
            Command::ToggleCategories => "view.categories",
            Command::ToggleToolbar => "view.toolbar",
            Command::ToggleDropTarget => "view.drop_target",
            Command::SortByName => "view.sort.name",
            Command::SortBySize => "view.sort.size",
            Command::SortByStatus => "view.sort.status",
            Command::SortByTimeLeft => "view.sort.time_left",
            Command::SortByRate => "view.sort.rate",
            Command::SortByDate => "view.sort.last_try",
            Command::SortByDescription => "view.sort.description",
            Command::Help => "help.help",
            Command::About => "help.about",
            Command::CheckForUpdates => "help.check_for_updates",
        }
    }

    /// Whether the command opens a dialog that asks for more before
    /// anything is done, which a menu marks with "…" (only
    /// Open with… had it).
    pub fn asks_more(self) -> bool {
        matches!(
            self,
            Command::AddUrl
                | Command::AddSeries
                | Command::AddFromClipboard
                | Command::Import
                | Command::Export
                | Command::BackUp
                | Command::Restore
                | Command::SiteFiles
                | Command::UpdateLink
                | Command::DeleteFile
                | Command::MoveToQueue
                | Command::Properties
                | Command::MoveRename
                | Command::Find
                | Command::Scheduler
                | Command::SpeedLimiterSettings
                | Command::Options
        )
    }

    /// The speed, in KB/s, a preset holds downloads to (0 for none).
    pub fn speed_preset(self) -> Option<u64> {
        match self {
            Command::SpeedLow => Some(SPEED_LOW),
            Command::SpeedMedium => Some(SPEED_MEDIUM),
            Command::SpeedHigh => Some(SPEED_HIGH),
            Command::SpeedUnlimited => Some(0),
            _ => None,
        }
    }

    /// The English words on its menu item or button, which the catalogs are
    /// keyed by and the control protocol lists.
    pub fn label(self) -> &'static str {
        match self {
            Command::AddUrl => "Add URL",
            Command::AddSeries => "Add numbered series",
            Command::AddFromClipboard => "Add links from clipboard",
            Command::Import => "Import",
            Command::Export => "Export",
            Command::BackUp => "Back up dm",
            Command::Restore => "Restore",
            Command::SiteFiles => "Grabber",
            Command::Quit => "Exit",
            Command::DownloadNow => "Resume download",
            Command::StopDownload => "Stop download",
            Command::Remove => "Delete",
            Command::DeleteFile => "Delete file",
            Command::Redownload => "Redownload",
            Command::UpdateLink => "Update download link",
            Command::CopyLink => "Copy link",
            Command::CopyDetails => "Copy details",
            Command::MoveToQueue => "Move to queue",
            Command::RemoveFromQueue => "Take out of queue",
            Command::MoveUp => "Move up",
            Command::MoveDown => "Move down",
            Command::Properties => "Properties",
            Command::ShowProgress => "Show progress",
            Command::OpenFile => "Open",
            Command::OpenWith => "Open with…",
            Command::Preview => "Preview",
            Command::OpenFolder => "Open folder",
            Command::ExtractHere => "Extract here",
            Command::ExtractTo => "Extract to…",
            Command::MoveRename => "Move/Rename",
            Command::SelectAll => "Select all",
            Command::ResumeAll => "Resume all",
            Command::StopAll => "Stop All",
            Command::DeleteCompleted => "Delete Completed",
            Command::Find => "Find",
            Command::FindNext => "Find next",
            Command::FindPrevious => "Find previous",
            Command::Scheduler => "Scheduler",
            Command::StartQueue => "Start Queue",
            Command::StopQueue => "Stop Queue",
            Command::SpeedLimiterToggle => "Limit speed",
            Command::SpeedLimiterSettings => "Speed limiter settings",
            Command::SpeedLow => "Low speed (256 KB/s)",
            Command::SpeedMedium => "Medium speed (1 MB/s)",
            Command::SpeedHigh => "High speed (4 MB/s)",
            Command::SpeedUnlimited => "Full speed",
            Command::WhenDoneNothing => "Do nothing",
            Command::WhenDoneQuit => "Exit dm",
            Command::WhenDoneSleep => "Sleep",
            Command::WhenDoneHibernate => "Hibernate",
            Command::WhenDoneShutDown => "Turn off computer",
            Command::Options => "Options",
            Command::ToggleCategories => "Categories",
            Command::ToggleToolbar => "Toolbar",
            Command::ToggleDropTarget => "Drop target",
            Command::SortByName => "Sort by name",
            Command::SortBySize => "Sort by size",
            Command::SortByStatus => "Sort by status",
            Command::SortByTimeLeft => "Sort by time left",
            Command::SortByRate => "Sort by speed",
            Command::SortByDate => "Sort by date",
            Command::SortByDescription => "Sort by note",
            Command::Help => "Help",
            Command::About => "About dm",
            Command::CheckForUpdates => "Check for updates",
        }
    }

    /// Every key that runs it, the one a menu shows first. Delete removes a
    /// download where the keyboard has that key; a Mac's has none without
    /// Fn, so there it is ⌘⌫ as in the Finder, or ⌫ alone.
    pub fn chords(self) -> Vec<Chord> {
        let cmd = |key| vec![Chord::with_command(key)];
        let plain = |key| vec![Chord::plain(key)];
        match self {
            Command::AddUrl => cmd(Key::N),
            Command::AddSeries => vec![Chord {
                shift: true,
                ..Chord::with_command(Key::N)
            }],
            Command::Find => cmd(Key::F),
            Command::FindNext => plain(Key::F3),
            Command::FindPrevious => vec![Chord {
                shift: true,
                ..Chord::plain(Key::F3)
            }],
            Command::SelectAll => cmd(Key::A),
            Command::Remove if cfg!(target_os = "macos") => vec![
                Chord::with_command(Key::Backspace),
                Chord::plain(Key::Backspace),
                Chord::plain(Key::Delete),
            ],
            Command::Remove => plain(Key::Delete),
            Command::Properties => cmd(Key::I),
            Command::Options => cmd(Key::Comma),
            Command::DownloadNow => cmd(Key::R),
            Command::StopDownload => cmd(Key::Period),
            Command::Quit => cmd(Key::Q),
            Command::Help => plain(Key::F1),
            _ => Vec::new(),
        }
    }

    /// Whether its key acts on the list's selection, and so must not reach
    /// the list from behind a dialog.
    pub fn acts_on_the_list(self) -> bool {
        matches!(
            self,
            Command::Remove
                | Command::SelectAll
                | Command::Properties
                | Command::DownloadNow
                | Command::StopDownload
                | Command::FindNext
                | Command::FindPrevious
        )
    }

    /// Looks a command up by the name the control protocol sends.
    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.iter().copied().find(|c| c.id() == id)
    }
}

/// What only the presentation knows of a row, for [`applies`]: kept where
/// it is read for drawing, and asked here rather than copied.
pub trait Looks {
    /// Whether `id` is waiting to try again after a failure.
    fn retrying(&self, id: Id) -> bool;
    /// Whether a finished row's file is no longer where dm saved it
    /// (`rules::file_gone`).
    fn gone(&self, row: &Entry) -> bool;
    /// Whether a player can show it: a video or a sound.
    fn plays(&self, row: &Entry) -> bool;
}

/// Whether `command` applies to the rows chosen now (`chosen`: selected and
/// shown, in any order), with every row of the list in its order (`rows`)
/// and whether the find box holds words (`finding`). A presentation greys
/// out, or hides, what does not apply; running it does nothing.
pub fn applies(
    command: Command,
    chosen: &[&Entry],
    rows: &[Arc<Entry>],
    finding: bool,
    looks: &impl Looks,
) -> bool {
    let any = |f: &dyn Fn(&Entry) -> bool| chosen.iter().any(|r| f(r));
    let settled = |r: &Entry| !matches!(r.state, State::Running | State::Queued);
    let one = chosen.len() == 1;
    match command {
        Command::DownloadNow => any(&|r| matches!(r.state, State::Paused | State::Failed(_))),
        Command::StopDownload => any(&|r| matches!(r.state, State::Running | State::Queued)),
        // Enabled only for downloads neither running nor in line: a running
        // one is stopped first, so nothing is removed from under its own
        // writer by a slip of the Delete key.
        Command::Remove => !chosen.is_empty() && chosen.iter().all(|r| settled(r)),
        // A finished download's file, which only a settled row has.
        Command::DeleteFile => {
            any(&|r| r.state == State::Completed) && chosen.iter().all(|r| settled(r))
        }
        Command::MoveToQueue | Command::Properties | Command::CopyLink => !chosen.is_empty(),
        // Only where there is something to fetch again: a row that has
        // fetched nothing yet would only start. A failure with nothing
        // fetched is Retry's.
        Command::Redownload => any(&|r| {
            matches!(r.state, State::Completed | State::Cancelled) || r.progress.downloaded > 0
        }),
        Command::CopyDetails => any(&|r| matches!(r.state, State::Failed(_))),
        Command::UpdateLink => {
            one && (!matches!(chosen[0].state, State::Running | State::Completed)
                || looks.retrying(chosen[0].id))
        }
        Command::RemoveFromQueue => any(&|r| !r.queue.is_empty()),
        // Not where it would do nothing: at the top or the bottom of the
        // order.
        Command::MoveUp => one && rows.first().is_some_and(|r| r.id != chosen[0].id),
        Command::MoveDown => one && rows.last().is_some_and(|r| r.id != chosen[0].id),
        Command::ShowProgress => any(&|r| r.state != State::Completed),
        // Open folder stays: it says the file is gone, which is an answer.
        Command::OpenFolder => {
            one && chosen[0].state == State::Completed && !chosen[0].path.is_empty()
        }
        // Something a player can show, with some of it fetched.
        Command::Preview => {
            one && chosen[0].state != State::Completed
                && chosen[0].progress.downloaded > 0
                && looks.plays(chosen[0])
        }
        // A finished archive dm unpacks (ZIP, tar, 7z) that is still on disk.
        Command::ExtractHere | Command::ExtractTo => {
            one && chosen[0].state == State::Completed
                && dm_core::extract::is_archive(&chosen[0].path)
                && !looks.gone(chosen[0])
        }
        Command::OpenFile | Command::OpenWith => {
            one && chosen[0].state == State::Completed
                && !chosen[0].path.is_empty()
                && !looks.gone(chosen[0])
        }
        // One to move and rename, or several to move together; or one that failed or is
        // stopped, given another name or folder (the button beside Retry, B54, and a stalled
        // row's words name it).
        Command::MoveRename => {
            (one && matches!(chosen[0].state, State::Failed(_) | State::Paused))
                || !chosen.is_empty()
                    && chosen.iter().all(|r| {
                        r.state == State::Completed && !r.path.is_empty() && !looks.gone(r)
                    })
        }
        Command::ResumeAll => rows.iter().any(|r| r.state == State::Paused),
        Command::StopAll => rows
            .iter()
            .any(|r| matches!(r.state, State::Running | State::Queued)),
        Command::DeleteCompleted => rows.iter().any(|r| r.state.clearable()),
        Command::FindNext | Command::FindPrevious => finding,
        _ => true,
    }
}

/// The toolbar's buttons, in their default order: each command and the
/// English word under its picture.
pub const TOOLBAR: [(Command, &str); 11] = [
    (Command::AddUrl, "Add URL"),
    (Command::DownloadNow, "Resume"),
    (Command::StopDownload, "Stop"),
    (Command::StopAll, "Stop All"),
    (Command::Remove, "Delete"),
    (Command::DeleteCompleted, "Delete Completed"),
    (Command::Options, "Options"),
    (Command::Scheduler, "Scheduler"),
    (Command::StartQueue, "Start Queue"),
    (Command::StopQueue, "Stop Queue"),
    (Command::SiteFiles, "Grabber"),
];

/// The toolbar's buttons in the order a person chose (by command id), with
/// any not named after them in the default order. Unknown ids are skipped,
/// and one named twice (a hand-edited settings file) is drawn once.
pub fn toolbar_in(order: &[String]) -> Vec<(Command, &'static str)> {
    let mut buttons: Vec<(Command, &'static str)> = Vec::new();
    for id in order {
        if let Some(button) = TOOLBAR.iter().find(|(c, _)| c.id() == id) {
            if !buttons.iter().any(|(c, _)| *c == button.0) {
                buttons.push(*button);
            }
        }
    }
    for button in TOOLBAR {
        if !buttons.iter().any(|(c, _)| *c == button.0) {
            buttons.push(button);
        }
    }
    buttons
}

/// The buttons the toolbar shows: [`toolbar_in`] the settings' order, less
/// those the person hid.
pub fn toolbar_shown(settings: &Settings) -> Vec<(Command, &'static str)> {
    toolbar_in(&settings.toolbar_order)
        .into_iter()
        .filter(|(c, _)| !settings.toolbar_hidden.iter().any(|h| h == c.id()))
        .collect()
}

/// A key a command runs on, as any presentation can read it: the key and
/// the modifiers held with it. `command` is ⌘ on a Mac and Ctrl elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    /// ⌘ on a Mac, Ctrl elsewhere.
    pub command: bool,
    /// Shift.
    pub shift: bool,
    /// The key itself.
    pub key: Key,
}

impl Chord {
    /// `key` alone.
    pub const fn plain(key: Key) -> Self {
        Chord {
            command: false,
            shift: false,
            key,
        }
    }

    /// `key` with ⌘ (Ctrl elsewhere).
    pub const fn with_command(key: Key) -> Self {
        Chord {
            command: true,
            shift: false,
            key,
        }
    }

    /// How many modifiers it holds: a presentation that matches a press
    /// ignoring an extra modifier tries the chords holding more first.
    pub fn held(self) -> usize {
        usize::from(self.command) + usize::from(self.shift)
    }
}

/// The keys dm's commands run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A.
    A,
    /// F.
    F,
    /// I.
    I,
    /// N.
    N,
    /// Q.
    Q,
    /// R.
    R,
    /// The comma.
    Comma,
    /// The full stop.
    Period,
    /// F1.
    F1,
    /// F3.
    F3,
    /// Backspace (⌫).
    Backspace,
    /// Delete (⌦).
    Delete,
}

#[cfg(test)]
mod tests;
