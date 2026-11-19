// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The commands `carry_out` carries out itself, by what they act on: the rows chosen, how the
//! list is shown, and the windows and dialogs they open.

use super::*;

impl Downloads {
    /// Carries out `command` where it acts on the rows `chosen`; `false` for any other.
    pub(super) fn row_command(
        &mut self,
        command: dm_shell::app::commands::Command,
        chosen: &[Entry],
        cx: &mut Context<Self>,
    ) -> bool {
        use dm_shell::app::commands::Command;
        match command {
            // Each address on a line of its own, without a sign-in that may
            // be in it: what is pasted elsewhere is the link.
            Command::CopyLink => {
                let links: Vec<String> = chosen
                    .iter()
                    .map(|r| dm_desktop_base::format::copyable_url(&r.url))
                    .collect();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(links.join("\n")));
                self.say(dm_desktop_base::i18n::tn(
                    "Copied {} link.||Copied {} links.",
                    links.len() as u64,
                    &[&links.len()],
                ));
                cx.notify();
                true
            }
            Command::Remove | Command::DeleteFile => {
                self.remove(chosen, command == Command::DeleteFile, cx);
                cx.notify();
                true
            }
            Command::OpenFile
            | Command::OpenWith
            | Command::Preview
            | Command::OpenFolder
            | Command::ExtractHere
            | Command::ExtractTo => {
                self.act_on_file(command, chosen, cx);
                true
            }
            Command::ShowProgress => {
                let ids: Vec<dm_core::Id> = chosen
                    .iter()
                    .filter(|r| r.state != State::Completed)
                    .map(|r| r.id)
                    .collect();
                for id in ids {
                    self.windows.progress_closed.remove(&id);
                    self.show_progress(id, cx);
                }
                true
            }
            Command::UpdateLink => {
                crate::dialogs::rowdialogs::update_link(cx.entity().downgrade(), chosen[0].id, cx);
                true
            }
            Command::MoveToQueue => {
                let ids = chosen.iter().map(|r| r.id).collect();
                crate::dialogs::rowdialogs::move_to_queue(cx.entity().downgrade(), ids, cx);
                true
            }
            Command::Properties => {
                for row in chosen {
                    crate::dialogs::properties::open(cx.entity().downgrade(), row.id, cx);
                }
                true
            }
            Command::CopyDetails => {
                let settings = self.backend.settings();
                if let Some((text, count)) = dm_desktop_base::details::blocks_for(
                    chosen,
                    settings.downloads.connections,
                    &settings.folders.download_dir,
                ) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.say(dm_desktop_base::i18n::tn(
                        "Copied the details of {} download.||Copied the details of {} downloads.",
                        count as u64,
                        &[&count],
                    ));
                }
                cx.notify();
                true
            }
            _ => false,
        }
    }

    /// Carries out `command` where it changes how the list is shown, or what dm does: `false`
    /// for any other.
    pub(super) fn view_command(
        &mut self,
        command: dm_shell::app::commands::Command,
        cx: &mut Context<Self>,
    ) -> bool {
        use dm_shell::app::commands::Command;
        match command {
            Command::SelectAll => {
                self.select_all(cx);
                true
            }
            Command::Import => {
                self.import(cx);
                true
            }
            Command::Export => {
                self.export(cx);
                true
            }
            // File › Back up dm… and Restore… (gaps row W16).
            Command::BackUp => {
                self.back_up_dm(cx);
                true
            }
            Command::Restore => {
                self.restore_dm(cx);
                true
            }
            Command::Help => {
                self.help();
                cx.notify();
                true
            }
            Command::ToggleCategories => {
                self.layout.show_sidebar = !self.layout.show_sidebar;
                self.keep_shown(cx);
                cx.notify();
                true
            }
            Command::ToggleToolbar => {
                self.layout.show_toolbar = !self.layout.show_toolbar;
                self.keep_shown(cx);
                cx.notify();
                true
            }
            Command::SortByName
            | Command::SortBySize
            | Command::SortByStatus
            | Command::SortByTimeLeft
            | Command::SortByRate
            | Command::SortByDate
            | Command::SortByDescription => {
                let column = match command {
                    Command::SortByName => Column::Name,
                    Command::SortBySize => Column::Size,
                    Command::SortByStatus => Column::Status,
                    Command::SortByTimeLeft => Column::TimeLeft,
                    Command::SortByRate => Column::Rate,
                    Command::SortByDate => Column::LastTry,
                    _ => Column::Description,
                };
                self.sort_by(column, cx);
                cx.notify();
                true
            }
            Command::SpeedLimiterToggle
            | Command::SpeedLow
            | Command::SpeedMedium
            | Command::SpeedHigh
            | Command::SpeedUnlimited => {
                self.speed(command);
                cx.notify();
                true
            }
            // Downloads › When done: what dm does once the queue has run
            // dry, for this session.
            Command::WhenDoneNothing
            | Command::WhenDoneQuit
            | Command::WhenDoneSleep
            | Command::WhenDoneHibernate
            | Command::WhenDoneShutDown => {
                if let Some(chosen) = dm_shell::app::commands::when_done(command) {
                    self.session.when_done = chosen;
                }
                cx.notify();
                true
            }
            // View › Drop target: kept in Options, and the card shown or put
            // away.
            Command::ToggleDropTarget => {
                let mut wanted = self.backend.settings();
                wanted.drop_target = !wanted.drop_target;
                if let Err(e) = self.apply_settings(wanted, cx) {
                    self.say(e);
                }
                self.follow_drop_target(cx);
                cx.notify();
                true
            }
            _ => false,
        }
    }

    /// Carries out `command` where it opens a window or a dialog, or runs a queue: `false` for
    /// any other.
    pub(super) fn window_command(
        &mut self,
        command: dm_shell::app::commands::Command,
        cx: &mut Context<Self>,
    ) -> bool {
        use dm_shell::app::commands::Command;
        match command {
            Command::SiteFiles => {
                crate::dialogs::grabber::open(cx.entity().downgrade(), cx);
                true
            }
            Command::Scheduler => {
                crate::dialogs::scheduler::open(cx.entity().downgrade(), self.current_queue(), cx);
                true
            }
            Command::SpeedLimiterSettings => {
                crate::dialogs::limiter::open(cx.entity().downgrade(), cx);
                true
            }
            Command::StartQueue | Command::StopQueue => {
                let queue = self.current_queue();
                self.run_on_queue(command, &queue, cx);
                true
            }
            Command::AddUrl => {
                crate::dialogs::addurl::open(cx.entity().downgrade(), cx);
                true
            }
            Command::AddSeries => {
                crate::dialogs::series::open(cx.entity().downgrade(), cx);
                true
            }
            Command::AddFromClipboard => {
                self.add_from_clipboard(cx);
                true
            }
            Command::Options => {
                self.open_options(cx);
                true
            }
            Command::About | Command::CheckForUpdates => {
                self.open_about(command == Command::CheckForUpdates, cx);
                true
            }
            _ => false,
        }
    }
}
