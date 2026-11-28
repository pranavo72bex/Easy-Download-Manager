// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A premiere, or a live stream not yet begun, waited for (gaps row Y5): YouTube's player names
//! the time it is set to start and no stream yet (`youtube::premiere`). The download waits, its
//! row saying for what ("Waiting: the premiere starts in 40 minutes"), until a moment past that
//! time; then it reads the video again, every half minute while it has not begun, for up to six
//! hours past its time. Once it has begun it goes on as a live stream, recorded, or as one just
//! ended. One set to start more than a week away is refused, saying when. A pause or a cancel
//! ends the wait at once.

use super::*;
use tokio::sync::mpsc::Sender;

/// The furthest ahead a premiere is waited for, in seconds: one further is refused, saying
/// when it starts.
const MOST_AHEAD: u64 = 7 * 24 * 3_600;

/// How long after its time a premiere is read again, in seconds: its stream comes up a moment
/// after it.
const AFTER_START: u64 = 3;

/// How often a premiere past its time is read again while it has not begun, in seconds.
const POLL: u64 = 30;

/// How often one not yet due is read again, in seconds, in case its time has been moved.
const RECHECK: u64 = 30 * 60;

/// How long past its time a premiere that has not begun is waited for, in seconds.
const LATE_MOST: u64 = 6 * 3_600;

/// How often the row's word on the time left is renewed, in seconds.
const SAID_EVERY: u64 = 60;

/// How many readings in a row may fail while it waits before the last failure is the outcome:
/// on a wait of hours the network drops now and then, while a video taken down fails each time.
const FAILED_READS_MOST: u32 = 3;

/// Whether `said` is the note of a premiere, or a live stream not begun, that its download
/// waits for: one of `edm_core::PREMIERE_NOTES`, a number in it where its template has
/// `{}`. The row says it after "Waiting: " rather than after its state (gaps row Y5).
#[must_use]
pub fn waits_for_premiere(said: &str) -> bool {
    crate::download::PREMIERE_NOTES
        .iter()
        .any(|&template| match template.split_once("{}") {
            None => said == template,
            Some((before, after)) => said
                .strip_prefix(before)
                .and_then(|rest| rest.strip_suffix(after))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())),
        })
}

/// The note of a premiere set to start at `at`, it being `now` (both seconds since 1970): in
/// hours from two hours ahead, in minutes from two minutes ahead, "in a moment" under that, and
/// "not begun yet" once its time has come. Each number is two or more, so said as many.
fn premiere_note(at: u64, now: u64) -> String {
    const HOUR: u64 = 3_600;
    let notes = &crate::download::PREMIERE_NOTES;
    let left = at.saturating_sub(now);
    if left >= 2 * HOUR {
        notes[0].replace("{}", &(left / HOUR).to_string())
    } else if left >= 120 {
        notes[1].replace("{}", &left.div_ceil(60).to_string())
    } else if left > 0 {
        notes[2].into()
    } else {
        notes[3].into()
    }
}

/// The time now, in seconds since 1970.
fn unix_now() -> u64 {
    u64::try_from(crate::clock::unix_now()).unwrap_or(0)
}

/// `read`, or, where it is a premiere's, the reading of the video once the premiere has begun.
pub(super) async fn waited(
    url: &str,
    options: &Options,
    report: Option<&Sender<Update>>,
    read: Read,
) -> Result<Read, Error> {
    let Some(at) = read.info.premiere_at else {
        return Ok(read);
    };
    let outcome = waiting(url, options, report, at).await;
    // The row's word on the wait goes with it, whatever the outcome.
    say(report, String::new()).await;
    outcome
}

/// Waits for the premiere set to start at `at`, reading the video again as the module says.
async fn waiting(
    url: &str,
    options: &Options,
    report: Option<&Sender<Update>>,
    mut at: u64,
) -> Result<Read, Error> {
    let mut read_at = unix_now();
    let mut failed = 0;
    loop {
        let now = unix_now();
        if at > now.saturating_add(MOST_AHEAD) {
            return Err(too_far(at));
        }
        if now > at.saturating_add(LATE_MOST) {
            return Err(Error::Internal(
                "the premiere had not begun six hours after the time it was set for".into(),
            ));
        }
        say(report, premiere_note(at, now)).await;
        let due = if now < at.saturating_add(AFTER_START) {
            at.saturating_add(AFTER_START)
                .min(read_at.saturating_add(RECHECK))
        } else {
            read_at.saturating_add(POLL)
        };
        let wake = due.min(now.saturating_add(SAID_EVERY));
        let wait = Duration::from_secs(wake.saturating_sub(now));
        crate::download::or_stop(&options.control, async {
            tokio::time::sleep(wait).await;
            Ok(())
        })
        .await?;
        if unix_now() < due {
            continue;
        }
        read_at = unix_now();
        match resolve(url, options, false).await {
            Ok(again) => match again.info.premiere_at {
                Some(next) => {
                    at = next;
                    failed = 0;
                }
                None => return Ok(again),
            },
            Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
            Err(why) => {
                failed += 1;
                if failed >= FAILED_READS_MOST {
                    return Err(why);
                }
                crate::note!(
                    warn,
                    "EDM: {url}: the premiere could not be read again: {why}"
                );
            }
        }
    }
}

/// `said` put on the row, where there is one to say it on.
async fn say(report: Option<&Sender<Update>>, said: String) {
    if let Some(tx) = report {
        let _ = tx.send(Update::Note(said)).await;
    }
}

/// The refusal of a premiere set to start at `at`, more than a week away: when it starts, on
/// this computer's clock.
fn too_far(at: u64) -> Error {
    let local = i64::try_from(at).ok().and_then(crate::clock::local_at);
    let when = match local {
        Some(t) => format!(
            "{}-{:02}-{:02} {:02}:{:02}",
            t.date.year(),
            t.date.month(),
            t.date.day(),
            t.hour,
            t.minute
        ),
        None => at.to_string(),
    };
    Error::Internal(format!(
        "the premiere starts at {when}, more than a week from now"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_left_is_said_in_hours_minutes_or_a_moment() {
        let at = 1_795_186_800;
        assert_eq!(
            premiere_note(at, at - 5 * 3_600 - 59),
            "the premiere starts in 5 hours"
        );
        assert_eq!(
            premiere_note(at, at - 2 * 3_600),
            "the premiere starts in 2 hours"
        );
        assert_eq!(
            premiere_note(at, at - 2 * 3_600 + 1),
            "the premiere starts in 120 minutes"
        );
        assert_eq!(
            premiere_note(at, at - 150),
            "the premiere starts in 3 minutes"
        );
        assert_eq!(
            premiere_note(at, at - 120),
            "the premiere starts in 2 minutes"
        );
        assert_eq!(
            premiere_note(at, at - 119),
            "the premiere starts in a moment"
        );
        assert_eq!(premiere_note(at, at), "the premiere has not begun yet");
        assert_eq!(
            premiere_note(at, at + 600),
            "the premiere has not begun yet"
        );
    }

    #[test]
    fn a_premieres_notes_are_known_as_its_and_no_others_are() {
        for left in [0, 30, 150, 4_000, 90_000] {
            assert!(waits_for_premiere(&premiere_note(
                5_000_000,
                5_000_000 - left
            )));
        }
        assert!(!waits_for_premiere("the premiere starts in many hours"));
        assert!(!waits_for_premiere("the premiere starts in  hours"));
        assert!(!waits_for_premiere(""));
        assert!(!waits_for_premiere(crate::download::LIVE_PIECES_GONE));
        assert!(!waits_for_premiere(crate::download::CHECKED_BESIDE));
    }

    #[test]
    fn one_more_than_a_week_away_is_refused_saying_when() {
        let Error::Internal(why) = too_far(1_795_186_800) else {
            panic!("not an internal failure");
        };
        assert!(why.starts_with("the premiere starts at 2026-11-2"), "{why}");
        assert!(why.ends_with(", more than a week from now"), "{why}");
    }
}
