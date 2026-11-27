// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A connection with no range of its own: the range it would take longest to finish halved, and its
//! back half taken.

use super::*;

/// What a connection with no range of its own found to do.
pub(super) enum Halving {
    /// The new range made of a range's back half, to fetch.
    Range(usize),
    /// Ranges are still being fetched, none of them yet worth halving: looked at again soon.
    Later,
    /// Nothing will be worth halving again: the ranges under way are all too short.
    Nothing,
}

/// Whether `candidate` (a range's index, its start and the time it has left) is halved before
/// `chosen`: the slower, or in file order the nearer the file's start.
fn first_to_halve(
    chosen: (usize, u64, f64),
    candidate: (usize, u64, f64),
    start_first: bool,
) -> bool {
    if start_first {
        candidate.1 < chosen.1
    } else {
        candidate.2 > chosen.2
    }
}

/// Halves the range its own connection would take longest to finish (in file order, the one
/// nearest the file's start: [`Control::fetch_in_file_order`](crate::Control)), under that connection,
/// where that is longer than a new request is worth (`Job::split_worth`), and returns the new
/// range made of its back half. Only says whether anything is left to halve when not `judging`.
///
/// The plan beside the parts is written again before the new range fetches
/// a byte, so a resume puts every part at the offset it was fetched for.
pub(super) async fn split_largest(job: &Job, judging: bool) -> Halving {
    /* Held across the plan's write below, on purpose (gaps row 184): from the
     * moment the range is shortened until the plan is on disk. A range that
     * reaches the end it had been given reads that end under this lock, and
     * a split whose plan could not be written gives the range its end back
     * before letting go: read in between, the range would stop at an end the
     * plan never recorded, and its last bytes would be fetched by no one.
     * This download's connections alone wait, for one small write, at most
     * once per halving. */
    let mut segments = job.segments.lock().await;
    let index = segments.len();
    if index >= job.live.len() || !job.resumable {
        return Halving::Nothing;
    }
    let (now, worth) = (tokio::time::Instant::now(), job.split_worth().as_secs_f64());
    // Preview asked for the file's start first (gaps row W13): the range worth halving nearest
    // the file's start, not the slowest, so the free connections fill the start a player reads.
    let start_first = job.control.in_file_order();
    let mut halvable = false;
    let mut slowest: Option<(usize, u64, f64)> = None;
    for i in 0..index {
        let l = crate::lock(&job.live[i]);
        if !l.taken || l.left() < 2 * MIN_SPLIT {
            continue;
        }
        halvable = true;
        if !judging {
            break;
        }
        if let Some(time) = l.time_left(now).filter(|&time| time > worth) {
            let candidate = (i, l.start, time);
            if slowest.is_none_or(|chosen| first_to_halve(chosen, candidate, start_first)) {
                slowest = Some(candidate);
            }
        }
    }
    let Some((victim, _, _)) = slowest else {
        return if halvable {
            Halving::Later
        } else {
            Halving::Nothing
        };
    };
    let (start, end, old_end) = {
        let mut l = crate::lock(&job.live[victim]);
        let rest = l.left();
        if rest < 2 * MIN_SPLIT {
            return Halving::Later;
        }
        let old_end = l.end;
        l.end = l.start + l.reserved + rest / 2 - 1;
        (l.end + 1, old_end, old_end)
    };
    let mut planned = segments.clone();
    planned[victim].end = start - 1;
    planned.push(Segment {
        start,
        end,
        done: 0,
    });
    let (dir, size, resumable, validator, written) = (
        job.parts.clone(),
        job.size,
        job.resumable,
        job.validator.clone(),
        planned.clone(),
    );
    let recorded = tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &written)
    })
    .await;
    if !matches!(recorded, Ok(Ok(()))) {
        // Unrecorded, the split is undone: the range keeps its whole length,
        // which nothing past the new end can have been claimed against.
        crate::lock(&job.live[victim]).end = old_end;
        return Halving::Nothing;
    }
    segments[victim].end = start - 1;
    segments.push(Segment {
        start,
        end,
        done: 0,
    });
    // A new range starts with nothing written down for it.
    let _ = fs::remove_file(ledger_path(&job.parts, index)).await;
    forget_synced(&job.parts, index).await;
    // Every byte of it will be this run's.
    *crate::lock(&job.live[index]) = Live {
        start,
        end,
        reserved: 0,
        taken: true,
        trusted: true,
        reach: 0,
        going: Going {
            since: Some(tokio::time::Instant::now()),
            ..Going::default()
        },
    };
    Halving::Range(index)
}

#[cfg(test)]
mod tests {
    use super::first_to_halve;

    /// The slower of two ranges is halved first; in file order (Preview, gaps row W13), the one
    /// nearer the file's start, however fast it goes.
    #[test]
    fn the_slowest_is_halved_first_or_in_file_order_the_nearest_the_start() {
        let near_and_quick = (1, 2 << 20, 0.8);
        let far_and_slow = (7, 14 << 20, 9.0);
        assert!(first_to_halve(near_and_quick, far_and_slow, false));
        assert!(!first_to_halve(far_and_slow, near_and_quick, false));
        assert!(first_to_halve(far_and_slow, near_and_quick, true));
        assert!(!first_to_halve(near_and_quick, far_and_slow, true));
    }
}
