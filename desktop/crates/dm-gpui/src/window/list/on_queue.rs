// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Commands carried out on the queue, what each did said, and the speed shown.

use super::*;

impl Downloads {
    pub(super) fn carry_out(
        &mut self,
        command: dm_shell::app::commands::Command,
        cx: &mut Context<Self>,
    ) {
        use dm_shell::app::commands::Command;
        if !self.enabled(command) {
            return;
        }
        // Held by Options (B42): Quit and the dialogs' own still go. An open
        // question (Linux, `holder`) holds the window's own clicks and keys
        // only, not what the tray, `--when-done` or the control port asks.
        let options_open = self
            .windows
            .options_window
            .is_some_and(|w| cx.windows().contains(&w));
        if command != Command::Quit
            && command != Command::Options
            && options_open
            && self.bring_holder(cx)
        {
            return;
        }
        if command == Command::Quit {
            self.ask_quit(cx);
            return;
        }
        let chosen: Vec<Entry> = self.chosen().into_iter().cloned().collect();
        if command == Command::MoveRename {
            crate::dialogs::rowdialogs::move_rename(cx.entity().downgrade(), chosen, cx);
            return;
        }
        if self.row_command(command, &chosen, cx)
            || self.view_command(command, cx)
            || self.window_command(command, cx)
        {
            return;
        }
        if dm_shell::app::commands::is_verb(command) {
            // Undo stands until the next command that changes the list (gaps row U14).
            self.status.undo.forget();
            // What Delete Completed takes off, kept for its Undo (B24).
            let kept = (command == Command::DeleteCompleted).then(|| self.backend.clearable_rows());
            // Off the window's thread: Stop on ten thousand chosen rows is
            // ten thousand changes to the queue (gaps row 183).
            let queue = self.current_queue();
            let ids: Vec<dm_core::Id> = chosen.iter().map(|r| r.id).collect();
            self.later(
                move |backend| dm_shell::app::commands::verb(command, backend, &chosen, &queue),
                move |this, ran, cx| this.verb_done(command, &ids, ran, kept, cx),
                cx,
            );
            return;
        }
        self.read();
        cx.notify();
    }

    /// What a command `commands::verb` carried out on the rows `chosen`
    /// did, said and shown, once it is done.
    fn verb_done(
        &mut self,
        command: dm_shell::app::commands::Command,
        chosen: &[dm_core::Id],
        ran: Option<dm_shell::app::commands::Ran>,
        kept: Option<Vec<(usize, Entry)>>,
        cx: &mut Context<Self>,
    ) {
        use dm_shell::app::commands::Command;
        self.read();
        if let Some(dm_shell::app::commands::Ran::Cleared(n)) = ran {
            // Undo puts them back, as it does what Remove took off (B24, gaps row U14).
            if n > 0 && self.status.undo.keep(kept.unwrap_or_default()) > 0 {
                self.undo_expires(cx);
            }
            self.say(dm_desktop_base::i18n::tn(
                "Deleted {} completed or cancelled download from the list.||Deleted {} completed or cancelled downloads from the list.",
                n as u64,
                &[&n],
            ));
        }
        // Resume opens each row's progress window where Options says, but
        // not one its person closed: fifteen Resumes opened it fifteen
        // times over the list. Show progress still opens it.
        if command == Command::DownloadNow && self.backend.settings().general.show_progress {
            for id in chosen {
                if !self.windows.progress_closed.contains(id) {
                    self.show_progress(*id, cx);
                }
            }
        }
        // Resume on rows the schedule or the download limit holds: each of
        // the others went on, and these are asked about.
        if let Some(dm_shell::app::commands::Ran::WentOn { held }) = ran {
            self.ask_outside(held, cx);
        }
        cx.notify();
    }

    /// Start Queue or Stop Queue for `queue`, and what came of it said:
    /// how many started (or that the schedule holds them, or that nothing
    /// was waiting) or stopped. Checking the Recheck queue is said when it
    /// is done.
    pub fn run_on_queue(
        &mut self,
        command: dm_shell::app::commands::Command,
        queue: &str,
        cx: &mut Context<Self>,
    ) {
        // Off the window's thread, as the commands on rows are (gaps row
        // 183), and said once done.
        let asked = queue.to_string();
        let queue = queue.to_string();
        self.later(
            move |backend| dm_shell::app::commands::verb(command, backend, &[], &asked),
            move |this, ran, cx| this.ran_on_queue(command, &queue, ran, cx),
            cx,
        );
    }

    /// What Start Queue or Stop Queue on `queue` did, said.
    fn ran_on_queue(
        &mut self,
        command: dm_shell::app::commands::Command,
        queue: &str,
        ran: Option<dm_shell::app::commands::Ran>,
        cx: &mut Context<Self>,
    ) {
        use dm_desktop_base::i18n::{tf, tn};
        use dm_shell::app::commands::{Command, Ran};
        self.read();
        let name = dm_desktop_base::i18n::name(queue);
        match (command, ran) {
            (Command::StartQueue, ran) => {
                let n = match ran {
                    Some(Ran::QueueStarted(n)) => n,
                    _ => 0,
                };
                let settings = self.backend.settings();
                // Its own sound, only where the person chose a file.
                if n > 0 {
                    if let Some(path) = settings
                        .sounds
                        .get("queue_started")
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                    {
                        self.backend.platform.play_file(&path);
                    }
                }
                let schedule = settings.schedule_for(queue);
                let lets = !schedule.enabled || schedule.open();
                self.say(if n == 0 {
                    tf("Nothing in {} was waiting to start.", &[&name])
                } else if !lets {
                    tf(
                        "{} is waiting for the schedule; it starts by itself when the schedule opens.",
                        &[&name],
                    )
                } else {
                    tn(
                        "Started {} download in {}.||Started {} downloads in {}.",
                        n as u64,
                        &[&n, &name],
                    )
                });
                self.status.said_schedule_wait = n > 0 && !lets;
                if queue == dm_core::RECHECK_QUEUE {
                    dm_shell::app::commands::recheck(&self.backend, self.threads.rechecked.clone());
                }
            }
            (_, ran) => {
                let n = match ran {
                    Some(Ran::QueueStopped(n)) => n,
                    _ => 0,
                };
                self.say(tn(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &name],
                ));
            }
        }
        cx.notify();
    }

    /// The Speed limiter on or off, or a preset (Low, Medium, High,
    /// Unlimited), as `commands::limited` says, and said.
    pub(super) fn speed(&mut self, command: dm_shell::app::commands::Command) {
        use dm_desktop_base::i18n::{t, tf};
        let now = self.backend.settings();
        let wanted =
            dm_shell::app::commands::limited(command, &now, &mut self.last_limit).unwrap_or(now);
        let set = self.backend.set_settings(wanted);
        match set.map(|after| after.advanced.speed_limit) {
            Ok(kb) if kb > 0 => self.say(tf("Speed limiter on: {} KB/s.", &[&kb])),
            Ok(_) => self.say(t("Speed limiter off.").into()),
            Err(e) => self.say(e),
        }
        self.session.forget_limit();
    }
}
