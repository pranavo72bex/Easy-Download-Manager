// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the ranges' attempts read and did not write (gaps row 186): the bytes before a range set
//! aside for the check of its seam, a resume's tail compared with what is on the disk, what came
//! past a range's end and was not read on into the next, and what was in hand when an attempt
//! stopped. Each was fetched before or is fetched again: what dm itself received twice, apart
//! from what a server had sent and dm never read when it let go, which only the server can count.
//!
//! Counted for every download in the process, for the tests that measure what a stop costs
//! (`tests/shutdown.rs`); each attempt that counts any writes it, with its range, to the finish
//! trace (`DM_FINISH_TRACE`).

use std::sync::atomic::{AtomicU64, Ordering};

/// Every byte counted since dm started.
static UNWRITTEN: AtomicU64 = AtomicU64::new(0);

/// Counts `bytes` that range `index`'s attempt read and did not write.
pub(super) fn count(index: usize, bytes: u64) {
    if bytes == 0 {
        return;
    }
    UNWRITTEN.fetch_add(bytes, Ordering::Relaxed);
    crate::finish_trace::mark(&format!(
        "range {index}: {bytes} bytes read and not written"
    ));
}

/// What an attempt read and did not write: what it took off its answer, with what it was handed
/// to write first, less what it wrote and what it handed on to the range after it.
pub(super) fn left_unwritten(received: u64, written: u64, handed_on: u64) -> u64 {
    received.saturating_sub(written.saturating_add(handed_on))
}

/// Every byte the downloads' ranges have read and not written since dm started: what dm itself
/// received twice (gaps row 186). For tests.
#[doc(hidden)]
pub fn read_not_written() -> u64 {
    UNWRITTEN.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An attempt that wrote all it read counts nothing; one that set bytes aside, or dropped
    /// what came past its range's end, counts them; what it read on into the next range is that
    /// range's, and is not counted.
    #[test]
    fn an_attempt_counts_what_it_read_and_did_not_write() {
        assert_eq!(left_unwritten(1 << 20, 1 << 20, 0), 0);
        // The 64 bytes before the range, set aside for the check of its seam.
        assert_eq!(left_unwritten(64 + 4096, 4096, 0), 64);
        // 3 KiB past the range's end: dropped, or read on into the range after it.
        assert_eq!(left_unwritten(4096 + 3072, 4096, 0), 3072);
        assert_eq!(left_unwritten(4096 + 3072, 4096, 3072), 0);
        // A chunk in hand when the attempt stopped, written nowhere.
        assert_eq!(left_unwritten(16 << 10, 0, 0), 16 << 10);
        assert_eq!(left_unwritten(0, 10, u64::MAX), 0);
    }

    /// What is counted adds to the total; nothing counts nothing. Other tests' downloads count at
    /// the same time, so the total is held to at least what this test counted.
    #[test]
    fn counted_bytes_add_to_the_total() {
        let before = read_not_written();
        count(3, 1000);
        count(3, 0);
        assert!(read_not_written() - before >= 1000);
    }
}
