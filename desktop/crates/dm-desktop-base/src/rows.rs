// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a row of the download list says, in words both windows show: its
//! status (with why it waits, retries or failed), its size, its folder.
//! What the last read of the list told beside the rows (ranges being tried
//! again, torrents' swarms, finished rows another has taken the file of,
//! the download limit) is kept here per thread, set by each read.

use std::collections::HashMap;

use dm_core::{Entry, Id, State};

use crate::format;
use crate::i18n::t;

/// A state's own word, as the wire spells it.
pub fn kind(state: &State) -> &'static str {
    match state {
        State::Queued => "queued",
        State::Running => "running",
        State::Paused => "paused",
        State::Completed => "completed",
        State::Failed(_) => "failed",
        State::Cancelled => "cancelled",
    }
}

pub use dm_shell::app::names::category_word;

pub use dm_shell::app::rules::link_expired;

/// The name a row is shown by, and its kind: the application's
/// (`dm_shell::app::names`).
pub use dm_shell::app::names::{kind_of, name_of};

/// Why a row paused by itself, in plain words: a full disk names its
/// folder, as a failed one does.
pub fn stalled_said(row: &Entry) -> String {
    let words = crate::i18n::t;
    match format::reason(&row.stalled) {
        Some("the disk is full") if !going_into(row).is_empty() => {
            crate::i18n::fill(words("the disk is full in {}"), &[&going_into(row)])
        }
        Some(plain) => words(plain).to_string(),
        None => row.stalled.clone(),
    }
}

/// What the Status column says.
pub fn status_of(row: &Entry) -> String {
    status_in(row, t)
}

thread_local! {
    /// The rows a range is being retried for, as the last refresh found
    /// them: the attempt and how many there may be (0 for no end). Kept
    /// beside the rows, which the engine hands over without it.
    static RETRYING: std::cell::RefCell<HashMap<Id, (u32, u32, String)>> =
        std::cell::RefCell::new(HashMap::new());
    /// When each retrying row tries again.
    static NEXT_TRY: std::cell::RefCell<HashMap<Id, std::time::Instant>> =
        std::cell::RefCell::new(HashMap::new());
}

/// What the engine now says about retries, for the statuses drawn next.
pub fn set_retrying(now: HashMap<Id, (u32, u32, String, std::time::Instant)>) {
    NEXT_TRY.with(|n| {
        *n.borrow_mut() = now.iter().map(|(id, r)| (*id, r.3)).collect();
    });
    RETRYING.with(|r| {
        *r.borrow_mut() = now
            .into_iter()
            .map(|(id, (attempt, of, why, _))| (id, (attempt, of, why)))
            .collect();
    });
}

/// How long until a retrying row tries again: nothing once it is due.
pub fn next_try(id: Id) -> Option<std::time::Duration> {
    let at = NEXT_TRY.with(|n| n.borrow().get(&id).copied())?;
    at.checked_duration_since(std::time::Instant::now())
        .filter(|left| !left.is_zero())
}

thread_local! {
    /// Whether the download limit, not the schedule, is what holds rows
    /// that wait by themselves, as the last refresh found it.
    static LIMIT_HOLDS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// Each running torrent's swarm, as the last refresh found it.
    static SWARMS: std::cell::RefCell<HashMap<Id, dm_core::Swarm>> =
        std::cell::RefCell::new(HashMap::new());
}

/// What the engine now says of each running torrent's swarm.
pub fn set_swarms(now: HashMap<Id, dm_core::Swarm>) {
    SWARMS.with(|s| *s.borrow_mut() = now);
}

/// A running torrent's swarm, as the last refresh found it.
pub fn swarm_of(id: Id) -> Option<dm_core::Swarm> {
    SWARMS.with(|s| s.borrow().get(&id).copied())
}

/// What holds waiting rows now, for the statuses drawn next.
pub fn set_limit_holds(now: bool) {
    LIMIT_HOLDS.with(|l| l.set(now));
}

/// The attempt, how many there may be (0 for no end) and why, for a row
/// a range is being tried again for, as the last read found it.
pub fn retrying(id: Id) -> Option<(u32, u32, String)> {
    RETRYING.with(|r| r.borrow().get(&id).cloned())
}

/// What the engine said of a failure, word for word, where the status
/// says it in plainer words: shown on hover.
pub fn failure_as_said(row: &Entry) -> Option<&str> {
    match &row.state {
        State::Failed(raw) if format::reason(raw).is_some() => Some(raw),
        _ => None,
    }
}

/// Row rules the status bar and the list share: the application's.
pub use dm_shell::app::rules::{fetched_all, waits_for_its_queue};

/// A fraction as a whole percent, rounded down.
pub fn whole_percent(fraction: f64) -> u32 {
    (fraction * 100.0).clamp(0.0, 100.0).floor() as u32
}

/// The folder a row's file is in, once finished, or the one it was asked
/// to go to; empty where that is the download folder's own choice.
pub fn folder_of(row: &Entry) -> String {
    if !row.path.is_empty() {
        return std::path::Path::new(&row.path)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
    }
    row.save_dir.clone().unwrap_or_default()
}

/// Whether Windows may be asking about dm and the network (`dm_shell::firewall`); never in the
/// unit tests, which say a row's words without a firewall.
fn firewall_may_ask() -> bool {
    !cfg!(test) && dm_shell::firewall::may_ask()
}

/// What the Status column says, in English whatever the window's language: what the control
/// protocol and MCP report, so a script or an agent reads the same words on
/// every machine.
pub fn status_in_english(row: &Entry) -> String {
    status_in(row, |english| english)
}

/// What the Status column says, each word through `words` (the language in
/// force, or English for the control port).
pub fn status_in(row: &Entry, words: fn(&'static str) -> &'static str) -> String {
    match &row.state {
        // Said while a range waits to be tried again: the bar drops back
        // and climbs, which read as a fault.
        // With why, where dm can say it plainly: "Retrying (attempt 5 of
        // 10)" alone read as dm stuck on nothing.
        State::Running if retrying(row.id).is_some() => match retrying(row.id) {
            Some((attempt, of, why)) => {
                let cause = format::retry_cause(&why).map(words);
                match (of > 0, cause) {
                    (true, Some(cause)) => {
                        crate::i18n::fill(words("{} (retry {} of {})"), &[&cause, &attempt, &of])
                    }
                    (true, None) => {
                        crate::i18n::fill(words("Retrying (attempt {} of {})"), &[&attempt, &of])
                    }
                    (false, Some(cause)) => {
                        crate::i18n::fill(words("{} (retry {})"), &[&cause, &attempt])
                    }
                    (false, None) => crate::i18n::fill(words("Retrying (attempt {})"), &[&attempt]),
                }
            }
            None => String::new(),
        },
        State::Running => match row.progress.fraction() {
            // A torrent with nothing arriving is waiting for peers, said:
            // "0%" stood there with nothing to say why.
            Some(f)
                if dm_core::is_torrent(&row.url)
                    && row.progress.speed < 1.0
                    && !fetched_all(row) =>
            {
                format!("{}% · {}", whole_percent(f), words("waiting for peers"))
            }
            // Every byte in, its parts being put together: said, not "100%"
            // for as long as that takes, tens of seconds for a large file
            // on a slow disk.
            Some(_) if fetched_all(row) => words("Joining parts…").into(),
            // Whole percents everywhere, rounded down so 99.9 is not 100:
            // the list said 28.88% beside a bar saying 28%.
            Some(f) => format!("{}%", whole_percent(f)),
            // A length the server did not say: what has arrived, and how
            // fast, rather than "Starting…" for the whole run.
            None if row.progress.downloaded > 0 => {
                if row.progress.speed > 1.0 {
                    format!(
                        "{} · {}",
                        format::bytes(row.progress.downloaded),
                        format::rate(row.progress.speed)
                    )
                } else {
                    format::bytes(row.progress.downloaded)
                }
            }
            // A torrent with nothing yet is looking for peers, for as long
            // as that takes; "Starting…" read as stuck.
            // The first torrent makes Windows ask, in a window of its own that
            // can sit behind others, whether dm may use the network: said
            // until Windows has an answer for dm (gaps row 101).
            None if dm_core::is_torrent(&row.url) && firewall_may_ask() => {
                words("Finding peers… Windows may be asking whether dm may use the network: allow it in that window").into()
            }
            None if dm_core::is_torrent(&row.url) => words("Finding peers…").into(),
            // The first video waits while dm fetches what it needs for
            // videos, half a minute or so: said, so it does not read as
            // stuck.
            None if dm_core::tools::preparing() => {
                let said = words("Getting ready for videos (first time only)…");
                // With the bytes so far, in units alone, so half a minute
                // of nothing moving is seen to move.
                match dm_core::tools::preparing_progress() {
                    Some((got, Some(total))) => {
                        format!("{said} {} / {}", format::bytes(got), format::bytes(total))
                    }
                    Some((got, None)) => format!("{said} {}", format::bytes(got)),
                    None => said.into(),
                }
            }
            None => words("Starting…").into(),
        },
        State::Queued => words("Queued").into(),
        // Held by the schedule or the download limit, not by a person: it
        // goes on by itself, which "Stopped" would not say.
        // Saying what for: "Waiting" alone read as stuck.
        State::Paused if row.paused_by_schedule => {
            if LIMIT_HOLDS.with(std::cell::Cell::get) {
                words("Waiting for the download limit").into()
            } else {
                words("Waiting for the schedule").into()
            }
        }
        // Added for later: it waits for its queue to be started, and does
        // not start by itself, which "Queued" did not say.
        // Stopped by itself for want of room or of its folder's drive.
        State::Paused if !row.stalled.is_empty() => {
            crate::i18n::fill(words("Paused: {}"), &[&stalled_said(row)])
        }
        State::Paused if waits_for_its_queue(row) => words("Waiting for Start queue").into(),
        // How far it got, so a stopped row is not mistaken for one that
        // never started.
        State::Paused => match row.progress.fraction() {
            Some(f) if f > 0.0 => format!("{} · {}%", words("Stopped"), whole_percent(f)),
            _ => words("Stopped").into(),
        },
        // Its file moved or deleted since: said so in Properties and to the
        // control protocol too, as the list says it (Properties
        // said "Complete").
        State::Completed if gone(row) => words("File missing").into(),
        // "Delete file after": how long the file has left before it goes
        // to the Trash, the row with it.
        State::Completed if row.delete_after.is_some() => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            match row.delete_due_in(now) {
                Some(left) if left > 0 => crate::i18n::fill(
                    words("Complete · to the Trash in {}"),
                    &[&format::span(std::time::Duration::from_secs(left))],
                ),
                _ => words("Complete").into(),
            }
        }
        // A finished torrent kept shared says so, and how much it has sent (gaps row 147).
        State::Completed if !row.path.is_empty() && dm_core::sharing(std::path::Path::new(&row.path)).is_some() => {
            let sent = dm_core::sharing(std::path::Path::new(&row.path)).map_or(0, |s| s.sent);
            crate::i18n::fill(words("Complete · sharing, {} sent"), &[&format::bytes(sent)])
        }
        State::Completed => words("Complete").into(),
        // A server that wants a sign-in says where to give one: the plain
        // sentences for 401 and 403 do (`format::REASONS`); anything else
        // with that status is said as the engine said it, with where. The
        // address and the server's own page were shown in the row.
        State::Failed(reason) if needs_sign_in(reason) && format::reason(reason).is_none() => {
            format!(
                "{} — {}",
                crate::i18n::fill(words("Error: {}"), &[reason]),
                words("it needs a sign-in: Properties › Login")
            )
        }
        // In plain words where dm knows them; the engine's own on hover.
        // A full disk says which folder's: "the disk is full" left a person
        // to guess which disk.
        State::Failed(reason)
            if format::reason(reason) == Some("the disk is full")
                && !going_into(row).is_empty() =>
        {
            crate::i18n::fill(words("Error: the disk is full in {}"), &[&going_into(row)])
        }
        // A video page that failed, with nothing more specific known: the
        // reader's own words follow the sentence.
        State::Failed(reason)
            if format::reason(reason) == Some(format::VIDEO_NOT_GOT)
                && format::video_said(reason).is_some() =>
        {
            let said = format::video_said(reason).unwrap_or_default();
            crate::i18n::fill(
                words("Error: {}"),
                &[&format!("{} ({said})", words(format::VIDEO_NOT_GOT))],
            )
        }
        State::Failed(reason) => crate::i18n::fill(
            words("Error: {}"),
            &[&format::reason(reason).map_or(reason.as_str(), words)],
        ),
        State::Cancelled => words("Cancelled").into(),
    }
}

/// The folder a row's file was going into: the one chosen for it, else the
/// download folder it began in; empty where neither is known yet.
pub fn going_into(row: &Entry) -> String {
    row.save_dir
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| row.began_in.clone())
}

/// Whether a failure is a server refusing without a sign-in (401, 403).
pub fn needs_sign_in(reason: &str) -> bool {
    [" 401 ", " 403 "]
        .iter()
        .any(|code| format!(" {reason} ").contains(code))
}

/// What the Size column says.
pub fn size_of(entry: &Entry) -> String {
    shown_size(entry).map(format::bytes).unwrap_or_default()
}

/// The size the Size column shows: the file's, or what has arrived of one
/// whose length the server did not say. The column sorts by it.
pub fn shown_size(entry: &Entry) -> Option<u64> {
    entry
        .progress
        .total
        .or(Some(entry.progress.downloaded).filter(|n| *n > 0))
}

/// A finished row whose file is no longer where dm saved it.
pub fn gone(entry: &Entry) -> bool {
    SUPERSEDED.with(|s| dm_shell::app::rules::file_gone(entry, &s.borrow()))
}

/// How a row stands for the commands' rules (`commands::applies`), as
/// every presentation and the headless dm see it: waiting to try again
/// after a failure, its file gone, or something a player can show.
pub struct Looks;

impl dm_shell::app::commands::Looks for Looks {
    fn retrying(&self, id: Id) -> bool {
        retrying(id).is_some()
    }

    fn gone(&self, row: &Entry) -> bool {
        gone(row)
    }

    fn plays(&self, row: &Entry) -> bool {
        matches!(
            kind_of(row),
            Some(dm_core::Category::Videos | dm_core::Category::Audio)
        )
    }
}

thread_local! {
    /// Finished rows whose path a row finished since also wrote: the
    /// file there is that one's. A row whose file was deleted turned
    /// "Complete" again when the same address was downloaded anew under
    /// its old name.
    static SUPERSEDED: std::cell::RefCell<std::collections::HashSet<dm_core::Id>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Which finished rows another, later one has taken the file of, as the
/// rows now stand.
pub fn set_superseded<R: std::borrow::Borrow<dm_core::Entry>>(rows: &[R]) {
    let superseded = dm_shell::app::rules::superseded(rows);
    SUPERSEDED.with(|s| *s.borrow_mut() = superseded);
}

/// Resume's words for the rows chosen, in English, as every window says
/// them on its menus: Retry where each one failed, Start now where the
/// schedule or the download limit holds each one, else Resume's own.
#[must_use]
pub fn resume_word(chosen: &[&Entry]) -> &'static str {
    if chosen.is_empty() {
        return dm_shell::app::commands::Command::DownloadNow.label();
    }
    if chosen.iter().all(|r| matches!(r.state, State::Failed(_))) {
        return "Retry";
    }
    if chosen
        .iter()
        .all(|r| r.state == State::Paused && r.paused_by_schedule)
    {
        return "Start now";
    }
    dm_shell::app::commands::Command::DownloadNow.label()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: State, held: bool) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "http://e.com/f",
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": 10, "total": 100, "speed": 0.0, "eta": null},
            "filename": "f",
            "paused_by_schedule": held,
        }))
        .unwrap()
    }

    #[test]
    fn resume_says_retry_for_failures_and_start_now_for_held_rows() {
        let resume = dm_shell::app::commands::Command::DownloadNow.label();
        let failed = row(State::Failed("gone".into()), false);
        let held = row(State::Paused, true);
        let paused = row(State::Paused, false);
        assert_eq!(resume_word(&[]), resume);
        assert_eq!(resume_word(&[&failed]), "Retry");
        assert_eq!(resume_word(&[&held]), "Start now");
        assert_eq!(resume_word(&[&paused]), resume);
        assert_eq!(resume_word(&[&failed, &held]), resume, "a mix is resumed");
    }
}
