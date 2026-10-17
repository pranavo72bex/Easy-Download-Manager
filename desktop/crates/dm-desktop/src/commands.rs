//! Every action dm's window can take, by name.
//!
//! This is dm's registry: the menu bar, the toolbar, the row menu, the
//! keyboard and the control protocol all run a `Command`, so a command that
//! exists is reachable from all of them.

/// One action, as the menu, toolbar, keyboard or control protocol asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    // Program
    AddUrl,
    AddSeries,
    AddFromClipboard,
    Import,
    Export,
    SiteFiles,
    Quit,
    // File (the selected rows)
    DownloadNow,
    StopDownload,
    Remove,
    Redownload,
    UpdateLink,
    MoveToQueue,
    RemoveFromQueue,
    MoveUp,
    MoveDown,
    Properties,
    ShowProgress,
    OpenFile,
    OpenWith,
    OpenFolder,
    MoveRename,
    SelectAll,
    // Downloads
    ResumeAll,
    StopAll,
    DeleteCompleted,
    Find,
    FindNext,
    Scheduler,
    StartQueue,
    StopQueue,
    SpeedLimiterToggle,
    SpeedLimiterSettings,
    WhenDoneNothing,
    WhenDoneQuit,
    WhenDoneSleep,
    WhenDoneHibernate,
    WhenDoneShutDown,
    Options,
    // View
    ToggleCategories,
    ToggleToolbar,
    SortByName,
    SortBySize,
    SortByStatus,
    SortByTimeLeft,
    SortByRate,
    SortByDate,
    SortByDescription,
    // Help
    Help,
    About,
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
        Command::SiteFiles,
        Command::Quit,
        Command::DownloadNow,
        Command::StopDownload,
        Command::Remove,
        Command::Redownload,
        Command::UpdateLink,
        Command::MoveToQueue,
        Command::RemoveFromQueue,
        Command::MoveUp,
        Command::MoveDown,
        Command::Properties,
        Command::ShowProgress,
        Command::OpenFile,
        Command::OpenWith,
        Command::OpenFolder,
        Command::MoveRename,
        Command::SelectAll,
        Command::ResumeAll,
        Command::StopAll,
        Command::DeleteCompleted,
        Command::Find,
        Command::FindNext,
        Command::Scheduler,
        Command::StartQueue,
        Command::StopQueue,
        Command::SpeedLimiterToggle,
        Command::SpeedLimiterSettings,
        Command::WhenDoneNothing,
        Command::WhenDoneQuit,
        Command::WhenDoneSleep,
        Command::WhenDoneHibernate,
        Command::WhenDoneShutDown,
        Command::Options,
        Command::ToggleCategories,
        Command::ToggleToolbar,
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
            Command::SiteFiles => "program.site_files",
            Command::Quit => "program.exit",
            Command::DownloadNow => "file.download_now",
            Command::StopDownload => "file.stop_download",
            Command::Remove => "file.remove",
            Command::Redownload => "file.redownload",
            Command::UpdateLink => "file.update_link",
            Command::MoveToQueue => "file.move_to_queue",
            Command::RemoveFromQueue => "file.remove_from_queue",
            Command::MoveUp => "file.move_up",
            Command::MoveDown => "file.move_down",
            Command::Properties => "file.properties",
            Command::ShowProgress => "file.show_progress",
            Command::OpenFile => "file.open",
            Command::OpenWith => "file.open_with",
            Command::OpenFolder => "file.open_folder",
            Command::MoveRename => "file.move_rename",
            Command::SelectAll => "file.select_all",
            Command::ResumeAll => "downloads.resume_all",
            Command::StopAll => "downloads.stop_all",
            Command::DeleteCompleted => "downloads.clear_finished",
            Command::Find => "downloads.find",
            Command::FindNext => "downloads.find_next",
            Command::Scheduler => "downloads.scheduler",
            Command::StartQueue => "downloads.start_queue",
            Command::StopQueue => "downloads.stop_queue",
            Command::SpeedLimiterToggle => "downloads.speed_limiter.toggle",
            Command::SpeedLimiterSettings => "downloads.speed_limiter.settings",
            Command::WhenDoneNothing => "downloads.when_done.nothing",
            Command::WhenDoneQuit => "downloads.when_done.exit",
            Command::WhenDoneSleep => "downloads.when_done.sleep",
            Command::WhenDoneHibernate => "downloads.when_done.hibernate",
            Command::WhenDoneShutDown => "downloads.when_done.turn_off",
            Command::Options => "downloads.options",
            Command::ToggleCategories => "view.categories",
            Command::ToggleToolbar => "view.toolbar",
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

    /// What a person reads on the menu item or button, in the language in
    /// force.
    pub fn title(self) -> &'static str {
        crate::i18n::t(self.label())
    }

    /// The same in English, which the catalogs are keyed by and the control
    /// protocol lists.
    pub fn label(self) -> &'static str {
        match self {
            Command::AddUrl => "Add URL",
            Command::AddSeries => "Add numbered series",
            Command::AddFromClipboard => "Add links from clipboard",
            Command::Import => "Import",
            Command::Export => "Export",
            Command::SiteFiles => "Find files on a site",
            Command::Quit => "Exit",
            Command::DownloadNow => "Resume download",
            Command::StopDownload => "Stop download",
            Command::Remove => "Remove",
            Command::Redownload => "Redownload",
            Command::UpdateLink => "Update download link",
            Command::MoveToQueue => "Move to queue",
            Command::RemoveFromQueue => "Take out of queue",
            Command::MoveUp => "Move up",
            Command::MoveDown => "Move down",
            Command::Properties => "Properties",
            Command::ShowProgress => "Show progress",
            Command::OpenFile => "Open",
            Command::OpenWith => "Open with…",
            Command::OpenFolder => "Open folder",
            Command::MoveRename => "Move/Rename",
            Command::SelectAll => "Select all",
            Command::ResumeAll => "Resume all",
            Command::StopAll => "Stop all",
            Command::DeleteCompleted => "Clear all finished",
            Command::Find => "Find",
            Command::FindNext => "Find next",
            Command::Scheduler => "Scheduler",
            Command::StartQueue => "Start queue",
            Command::StopQueue => "Stop queue",
            Command::SpeedLimiterToggle => "Turn speed limiter on or off",
            Command::SpeedLimiterSettings => "Speed limiter settings",
            Command::WhenDoneNothing => "Do nothing",
            Command::WhenDoneQuit => "Exit dm",
            Command::WhenDoneSleep => "Sleep",
            Command::WhenDoneHibernate => "Hibernate",
            Command::WhenDoneShutDown => "Turn off computer",
            Command::Options => "Options",
            Command::ToggleCategories => "Categories",
            Command::ToggleToolbar => "Toolbar",
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

    /// The key that runs it, said the way the menu shows it.
    pub fn shortcut(self) -> Option<egui::KeyboardShortcut> {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let cmd = |key| Some(KeyboardShortcut::new(Modifiers::COMMAND, key));
        let shift = |key| {
            Some(KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                key,
            ))
        };
        match self {
            Command::AddUrl => cmd(Key::N),
            Command::AddSeries => shift(Key::N),
            Command::Find => cmd(Key::F),
            Command::FindNext => Some(KeyboardShortcut::new(Modifiers::NONE, Key::F3)),
            Command::SelectAll => cmd(Key::A),
            Command::Remove => Some(KeyboardShortcut::new(Modifiers::NONE, Key::Delete)),
            Command::Properties => cmd(Key::I),
            Command::Options => cmd(Key::Comma),
            Command::DownloadNow => cmd(Key::R),
            Command::StopDownload => cmd(Key::Period),
            Command::Quit => cmd(Key::Q),
            Command::Help => Some(KeyboardShortcut::new(Modifiers::NONE, Key::F1)),
            _ => None,
        }
    }

    /// Looks a command up by the name the control protocol sends.
    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.iter().copied().find(|c| c.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_its_own_name_and_label() {
        let mut ids: Vec<_> = Command::ALL.iter().map(|c| c.id()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "two commands share a name");
        for c in Command::ALL {
            assert_eq!(Command::from_id(c.id()), Some(*c));
            assert!(!c.label().is_empty());
        }
    }

    #[test]
    fn no_two_commands_share_a_key() {
        let keys: Vec<_> = Command::ALL.iter().filter_map(|c| c.shortcut()).collect();
        for (i, a) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(a), "{a:?} runs two commands");
        }
    }
}
