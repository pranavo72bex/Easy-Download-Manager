//! Every action dm's window can take, by name.
//!
//! This is dm's registry: the menu bar, the toolbar, the row menu, the
//! keyboard and the control protocol all run a `Command`, so a command that
//! exists is reachable from all of them.

/// The speed presets, in KB/s: one click from the tray or Downloads ›
/// Speed limiter, beside the exact limit Speed limiter settings keeps.
pub const SPEED_LOW: u64 = 256;
pub const SPEED_MEDIUM: u64 = 1024;
pub const SPEED_HIGH: u64 = 4096;

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
    DeleteFile,
    Redownload,
    UpdateLink,
    CopyLink,
    MoveToQueue,
    RemoveFromQueue,
    MoveUp,
    MoveDown,
    Properties,
    ShowProgress,
    OpenFile,
    OpenWith,
    Preview,
    OpenFolder,
    ExtractHere,
    ExtractTo,
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
    SpeedLow,
    SpeedMedium,
    SpeedHigh,
    SpeedUnlimited,
    WhenDoneNothing,
    WhenDoneQuit,
    WhenDoneSleep,
    WhenDoneHibernate,
    WhenDoneShutDown,
    Options,
    // View
    ToggleCategories,
    ToggleToolbar,
    ToggleDropTarget,
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
        Command::DeleteFile,
        Command::Redownload,
        Command::UpdateLink,
        Command::CopyLink,
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
            Command::SiteFiles => "program.site_files",
            Command::Quit => "program.exit",
            Command::DownloadNow => "file.download_now",
            Command::StopDownload => "file.stop_download",
            Command::Remove => "file.remove",
            Command::DeleteFile => "file.delete_file",
            Command::Redownload => "file.redownload",
            Command::UpdateLink => "file.update_link",
            Command::CopyLink => "file.copy_link",
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
            Command::SiteFiles => "Site files",
            Command::Quit => "Exit",
            Command::DownloadNow => "Resume download",
            Command::StopDownload => "Stop download",
            Command::Remove => "Remove",
            Command::DeleteFile => "Delete file",
            Command::Redownload => "Redownload",
            Command::UpdateLink => "Update download link",
            Command::CopyLink => "Copy link",
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
            Command::StopAll => "Stop all",
            Command::DeleteCompleted => "Clear finished",
            Command::Find => "Find",
            Command::FindNext => "Find next",
            Command::Scheduler => "Scheduler",
            Command::StartQueue => "Start queue",
            Command::StopQueue => "Stop queue",
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

    /// The key that runs it, said the way the menu shows it.
    pub fn shortcut(self) -> Option<egui::KeyboardShortcut> {
        self.keys().first().copied()
    }

    /// Every key that runs it, the one the menu shows first. Delete
    /// removes a download where the keyboard has that key; a Mac's has
    /// none without Fn, so there it is ⌘⌫ as in the Finder, or ⌫ alone.
    pub fn keys(self) -> Vec<egui::KeyboardShortcut> {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let cmd = |key| vec![KeyboardShortcut::new(Modifiers::COMMAND, key)];
        let plain = |key| vec![KeyboardShortcut::new(Modifiers::NONE, key)];
        match self {
            Command::AddUrl => cmd(Key::N),
            Command::AddSeries => vec![KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::N,
            )],
            Command::Find => cmd(Key::F),
            Command::FindNext => plain(Key::F3),
            Command::SelectAll => cmd(Key::A),
            Command::Remove if cfg!(target_os = "macos") => vec![
                KeyboardShortcut::new(Modifiers::COMMAND, Key::Backspace),
                KeyboardShortcut::new(Modifiers::NONE, Key::Backspace),
                KeyboardShortcut::new(Modifiers::NONE, Key::Delete),
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
        )
    }

    /// The commands whose keys were pressed, each key taken by one command.
    ///
    /// egui matches a shortcut ignoring an extra Shift, so ⌘⇧N matched
    /// ⌘N first and opened Add URL rather than the numbered series: the
    /// keys with more modifiers are tried first, and the one that matches
    /// takes the key press.
    pub fn pressed(input: &mut egui::InputState) -> Vec<Command> {
        let mut keys: Vec<(Command, egui::KeyboardShortcut)> = Command::ALL
            .iter()
            .flat_map(|c| c.keys().into_iter().map(move |k| (*c, k)))
            .collect();
        let held = |k: &egui::KeyboardShortcut| {
            let m = k.modifiers;
            [m.alt, m.shift, m.ctrl || m.command || m.mac_cmd]
                .iter()
                .filter(|on| **on)
                .count()
        };
        keys.sort_by_key(|(_, k)| std::cmp::Reverse(held(k)));
        let mut ran = Vec::new();
        for (command, key) in keys {
            if input.consume_shortcut(&key) && !ran.contains(&command) {
                ran.push(command);
            }
        }
        ran
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
        let keys: Vec<_> = Command::ALL.iter().flat_map(|c| c.keys()).collect();
        for (i, a) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(a), "{a:?} runs two commands");
        }
    }

    /// Presses a key with its modifiers in a frame of its own, and says
    /// what the window would run.
    fn press(key: egui::KeyboardShortcut) -> Vec<Command> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: key.logical_key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: key.modifiers,
            }],
            ..Default::default()
        };
        let mut ran = Vec::new();
        let mut out = ctx.run_ui(input, |ui| {
            ran = ui.ctx().input_mut(Command::pressed);
        });
        out.textures_delta.clear();
        ran
    }

    #[test]
    fn each_key_runs_its_own_command_and_no_other() {
        let mut checked = 0;
        for command in Command::ALL {
            for key in command.keys() {
                assert_eq!(press(key), vec![*command], "{key:?}");
                checked += 1;
            }
        }
        assert!(checked >= 12, "only {checked} keys");
        // The one that went wrong: ⌘⇧N is the numbered series, not Add URL.
        let series = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::N,
        );
        assert_eq!(press(series), vec![Command::AddSeries]);
    }

    #[test]
    fn a_mac_removes_with_backspace_and_others_with_delete() {
        let backspace = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Backspace);
        let delete = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
        if cfg!(target_os = "macos") {
            assert_eq!(press(backspace), vec![Command::Remove]);
            assert_eq!(
                Command::Remove.shortcut().map(|k| k.logical_key),
                Some(egui::Key::Backspace),
                "the menu shows ⌘⌫"
            );
        } else {
            assert!(press(backspace).is_empty());
        }
        assert_eq!(press(delete), vec![Command::Remove]);
    }
}
