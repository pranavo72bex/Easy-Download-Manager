// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the application carries out itself: the row verbs, When done, the speed
//! limit and the recheck.

use std::sync::Arc;

use dm_core::{Entry, Id};

use super::Command;
use crate::app::backend::Backend;
use crate::app::power::WhenDone;
use crate::app::rules::GoOn;
use crate::Settings;

/// What a command the application carries out did, for the presentation to
/// say or show: the words are its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// Done, with nothing to say.
    Done,
    /// Resume download: each chosen row went on (resumed, or retried after
    /// a failure) but these, which the schedule or the download limit
    /// holds, and which a person is asked about (start it now, outside the
    /// schedule, or let it wait).
    WentOn {
        /// The rows held.
        held: Vec<Id>,
    },
    /// Delete Completed: how many rows left the list.
    Cleared(usize),
    /// Start Queue: how many downloads in the queue started.
    QueueStarted(usize),
    /// Stop Queue: how many downloads in the queue stopped.
    QueueStopped(usize),
}

/// Whether [`verb`] carries `command` out (and answers `Some`): what the
/// window hands off its thread, since on ten thousand chosen rows it is
/// ten thousand changes to the queue (gaps row 183).
pub fn is_verb(command: Command) -> bool {
    matches!(
        command,
        Command::DownloadNow
            | Command::StopDownload
            | Command::Redownload
            | Command::RemoveFromQueue
            | Command::MoveUp
            | Command::MoveDown
            | Command::ResumeAll
            | Command::StopAll
            | Command::DeleteCompleted
            | Command::StartQueue
            | Command::StopQueue
    )
}

/// Carries out `command` where it is the application's alone, on the rows
/// chosen (`chosen`) or the queue chosen (`queue`, for Start and Stop
/// Queue): what it did, or `None` for a command that is the presentation's
/// (a dialog, the view, the clipboard, a file opened). The presentation has
/// checked [`applies`](super::applies) first.
pub fn verb(command: Command, backend: &Backend, chosen: &[Entry], queue: &str) -> Option<Ran> {
    let ran = match command {
        Command::DownloadNow => {
            let mut held = Vec::new();
            for row in chosen {
                match crate::app::rules::go_on(&row.state, row.paused_by_schedule) {
                    GoOn::Resume => backend.resume(row.id),
                    GoOn::Retry => backend.retry(row.id),
                    // Asked about: resuming would only requeue it to be held
                    // again.
                    GoOn::Held => held.push(row.id),
                    GoOn::Settled | GoOn::NotStopped => {}
                }
            }
            Ran::WentOn { held }
        }
        Command::StopDownload => {
            for row in chosen {
                backend.pause(row.id);
            }
            Ran::Done
        }
        Command::Redownload => {
            for row in chosen {
                backend.redownload(row.id);
            }
            Ran::Done
        }
        Command::RemoveFromQueue => {
            for row in chosen {
                backend.set_queue(row.id, "");
            }
            Ran::Done
        }
        Command::MoveUp | Command::MoveDown => {
            if let Some(row) = chosen.first() {
                backend.shift(row.id, if command == Command::MoveUp { -1 } else { 1 });
            }
            Ran::Done
        }
        Command::ResumeAll => {
            backend.resume_all();
            Ran::Done
        }
        Command::StopAll => {
            backend.pause_all();
            Ran::Done
        }
        Command::DeleteCompleted => Ran::Cleared(backend.clear_finished()),
        Command::StartQueue => Ran::QueueStarted(backend.start_queue(queue)),
        Command::StopQueue => Ran::QueueStopped(backend.stop_queue(queue)),
        _ => return None,
    };
    Some(ran)
}

/// What When done is set to by a When done command, or `None` for any other.
pub fn when_done(command: Command) -> Option<WhenDone> {
    Some(match command {
        Command::WhenDoneNothing => WhenDone::Nothing,
        Command::WhenDoneQuit => WhenDone::Quit,
        Command::WhenDoneSleep => WhenDone::Sleep,
        Command::WhenDoneHibernate => WhenDone::Hibernate,
        Command::WhenDoneShutDown => WhenDone::ShutDown,
        _ => return None,
    })
}

/// The command that chooses `wanted`, as the Downloads menu does: what an
/// order from the command line (`dm --when-done`) runs. [`when_done`] the
/// other way.
pub fn when_done_command(wanted: WhenDone) -> Command {
    match wanted {
        WhenDone::Nothing => Command::WhenDoneNothing,
        WhenDone::Quit => Command::WhenDoneQuit,
        WhenDone::Sleep => Command::WhenDoneSleep,
        WhenDone::Hibernate => Command::WhenDoneHibernate,
        WhenDone::ShutDown => Command::WhenDoneShutDown,
    }
}

/// The limit Limit speed turns back on at, as dm starts: the one in force,
/// else the one last turned off (kept in the settings, so it survives a
/// restart), else [`DEFAULT_LIMIT`]. [`limited`] keeps it from then on.
pub fn remembered_limit(settings: &Settings) -> u64 {
    [settings.advanced.speed_limit, settings.last_speed_limit]
        .into_iter()
        .find(|kb| *kb > 0)
        .unwrap_or(DEFAULT_LIMIT)
}

/// The limit, in KB/s, Limit speed turns on at when none was ever set.
pub const DEFAULT_LIMIT: u64 = 512;

/// The settings a speed command asks for, from those in force (`now`):
/// Limit speed (`SpeedLimiterToggle`) turns the limit off, or back on at the
/// last one (`last`, at least 1 KB/s); a preset holds downloads to its
/// speed, 0 for none. A limit turned off or replaced is kept in `last` and
/// in the settings' `last_speed_limit`. `None` for any other command.
pub fn limited(command: Command, now: &Settings, last: &mut u64) -> Option<Settings> {
    let mut wanted = now.clone();
    let to = match command {
        Command::SpeedLimiterToggle if now.advanced.speed_limit > 0 => 0,
        Command::SpeedLimiterToggle => (*last).max(1),
        _ => command.speed_preset()?,
    };
    if wanted.advanced.speed_limit > 0 {
        *last = wanted.advanced.speed_limit;
        wanted.last_speed_limit = wanted.advanced.speed_limit;
    }
    wanted.advanced.speed_limit = to;
    Some(wanted)
}

/// Started on the queue of downloads to check again (`dm_core::RECHECK_QUEUE`):
/// each row's server asked whether its file changed, in the background (a
/// server at a time, each up to its timeouts, held the window for minutes
/// when it was done in the window's turn). `done` is given (how many were
/// checked, how many changed) when it is over; the changed ones are
/// started.
pub fn recheck(backend: &Backend, done: Arc<std::sync::Mutex<Option<(usize, usize)>>>) {
    let queue = backend.queue.clone();
    backend.runtime.spawn(async move {
        let (checked, changed) = queue.synchronize(dm_core::RECHECK_QUEUE).await;
        if !changed.is_empty() {
            crate::shell::wake(&queue);
        }
        *dm_core::lock(&done) = Some((checked, changed.len()));
    });
}
