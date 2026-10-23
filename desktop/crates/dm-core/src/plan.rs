//! Cutting a file into the ranges that will be fetched at once.
//!
//! This is the decision download managers are really named for: one connection is limited by
//! the server's willingness to serve one connection, and several are not. The
//! arithmetic is small, so it lives on its own where it can be reasoned about
//! without a socket in the way.

use std::path::Path;

/// One piece of the file, as a byte range that includes both ends.
///
/// `done` is how much of it is already on disk. A ranged download writes
/// every range into one staged file at its own offset, so each range's
/// `done` is written down in a ledger of its own (`done-NNNN` beside the
/// plan), always after the bytes it speaks for and never ahead of them; the
/// end of what it vouches for is fetched again and compared on a resume.
/// Streams and FTP still stage a part file per piece, whose length is this
/// number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub done: u64,
}

impl Segment {
    /// How many bytes this range covers in total.
    ///
    /// Saturating, because a file of unknown length is planned as one range
    /// ending at `u64::MAX`, and `MAX - 0 + 1` panicked in a debug build and
    /// wrapped to an empty range — "finished" before a byte was fetched — in
    /// a release one.
    pub fn span(&self) -> u64 {
        (self.end - self.start).saturating_add(1)
    }

    /// Where the next request for this range should begin.
    pub fn cursor(&self) -> u64 {
        self.start + self.done
    }

    pub fn finished(&self) -> bool {
        self.done >= self.span()
    }
}

/// Below this, splitting costs more in round trips than it wins in
/// parallelism — the connections spend their time being set up rather than
/// carrying bytes.
pub const MIN_SEGMENT: u64 = 1024 * 1024;

/// Browsers cap connections per host at six and no amount of asking changes
/// it. A program has no such ceiling, which is most of the reason this is a
/// program — but a server that is asked for sixty-four ranges at once will
/// often answer by refusing all of them.
pub const MAX_CONNECTIONS: usize = 32;

/// Cuts a file of `size` bytes into at most `connections` ranges.
///
/// A file that cannot be resumed is one range whatever the setting says: a
/// server that will not serve ranges cannot serve several of them either, and
/// asking anyway turns one working download into eight failing ones.
pub fn plan(size: Option<u64>, connections: usize, resumable: bool) -> Vec<Segment> {
    // An unknown length cannot be divided. It is streamed to its end instead,
    // which is what a browser would have done unaided.
    let Some(size) = size else {
        return vec![Segment {
            start: 0,
            end: u64::MAX,
            done: 0,
        }];
    };
    if size == 0 {
        return vec![Segment {
            start: 0,
            end: 0,
            done: 0,
        }];
    }
    if !resumable {
        return vec![Segment {
            start: 0,
            end: size - 1,
            done: 0,
        }];
    }

    let wanted = connections.clamp(1, MAX_CONNECTIONS) as u64;
    // Never so many that the pieces stop being worth fetching separately.
    let count = wanted.min((size / MIN_SEGMENT).max(1));

    let each = size / count;
    (0..count)
        .map(|i| {
            let start = i * each;
            // The last range takes the remainder, so rounding never leaves a
            // few bytes at the end belonging to nobody.
            let end = if i == count - 1 {
                size - 1
            } else {
                start + each - 1
            };
            Segment {
                start,
                end,
                done: 0,
            }
        })
        .collect()
}

/// How many pieces each connection's share of a file is cut into.
///
/// One fixed range per connection let one slow connection hold a whole
/// share of the file while the others sat finished: 8.4s for 32 MB over
/// eight connections with one at 512 KB/s (`tests/speed.rs`). Cut finer, a
/// connection that finishes takes the next piece, so the slow one only ever
/// holds a small one. This is the start of dynamic segmentation; the rest
/// is in `download.rs`, where a connection with no piece left halves the
/// largest range still being fetched.
pub const PIECES_PER_CONNECTION: usize = 4;

/// The most pieces a file is cut into, whatever the connections.
pub const MAX_PIECES: u64 = 128;

/// Cuts a file into the pieces `connections` connections take in turn: about
/// [`PIECES_PER_CONNECTION`] each, none smaller than [`MIN_SEGMENT`].
pub fn pieces(size: Option<u64>, connections: usize, resumable: bool) -> Vec<Segment> {
    let base = plan(size, connections, resumable);
    let (Some(size), true, true) = (size, resumable, base.len() > 1) else {
        return base;
    };
    let wanted = (connections.clamp(1, MAX_CONNECTIONS) * PIECES_PER_CONNECTION) as u64;
    let count = wanted.min(MAX_PIECES).min((size / MIN_SEGMENT).max(1));
    let each = size / count;
    (0..count)
        .map(|i| {
            let start = i * each;
            let end = if i == count - 1 {
                size - 1
            } else {
                start + each - 1
            };
            Segment {
                start,
                end,
                done: 0,
            }
        })
        .collect()
}

/// The plan a set of staged parts was fetched under.
///
/// Part files and ledgers are matched to ranges by index, and an index says nothing
/// about the offset it was written at: the same `part-0001` is bytes
/// 512M–1G under an eight-way plan and 256M–512M under a sixteen-way one.
/// Every start used to re-plan from the connection setting of the moment,
/// so pausing, changing the setting and resuming spliced one plan's bytes
/// into another's offsets and reported the result Completed. The plan is
/// written down when the parts are first staged and read back before they
/// are adopted, so a resume finishes the plan it started rather than the
/// one the settings now suggest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Recorded {
    pub size: Option<u64>,
    pub resumable: bool,
    /// `(start, end)` of each range, in part-file order.
    pub ranges: Vec<(u64, u64)>,
    /// The ETag or Last-Modified of the file the parts are bytes of.
    ///
    /// `If-Range` guards one run only. Across a pause the next run probes
    /// the file as it is now and sends *that* validator as the condition, so
    /// a file republished at the same length was served as a range of the
    /// new version and appended to parts of the old one — reported Completed
    /// and matching neither. Parts are adopted only under the validator they
    /// were fetched under. Plans written before this have none and are
    /// adopted only from a server that offers none.
    #[serde(default)]
    pub validator: Option<String>,
}

/// Where the plan is written, beside the parts it describes.
pub fn record_path(parts_dir: &Path) -> std::path::PathBuf {
    parts_dir.join("plan.json")
}

/// Writes the plan the parts in `parts_dir` are being fetched under.
pub fn record(
    parts_dir: &Path,
    size: Option<u64>,
    resumable: bool,
    validator: Option<&str>,
    segments: &[Segment],
) -> std::io::Result<()> {
    let recorded = Recorded {
        size,
        resumable,
        ranges: segments.iter().map(|s| (s.start, s.end)).collect(),
        validator: validator.map(str::to_owned),
    };
    let json = serde_json::to_vec(&recorded).map_err(std::io::Error::other)?;
    /* Rewritten at every split, so not waited onto the disk: a flush each
     * time held every connection behind it and halved a fast download. A
     * power cut can leave the plan before the last split, which still covers
     * every byte (a split only shortens a range), or an empty one, whose
     * parts are then fetched again — slower, never wrong. */
    crate::write_atomic_unsynced(&record_path(parts_dir), &json)
}

/// The plan the parts in `parts_dir` were staged under, if one was written.
pub fn recorded(parts_dir: &Path) -> Option<Recorded> {
    let bytes = std::fs::read(record_path(parts_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

impl Recorded {
    /// The segments this plan describes, none of them started: what is on
    /// disk is adopted afterwards, from the files themselves.
    pub fn segments(&self) -> Vec<Segment> {
        self.ranges
            .iter()
            .map(|&(start, end)| Segment {
                start,
                end,
                done: 0,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_plan_reads_back_as_it_was_written() {
        let dir = tempfile::tempdir().unwrap();
        let planned = plan(Some(80 * MIN_SEGMENT), 8, true);
        record(
            dir.path(),
            Some(80 * MIN_SEGMENT),
            true,
            Some("\"v1\""),
            &planned,
        )
        .unwrap();
        let back = recorded(dir.path()).expect("the plan was not recorded");
        assert_eq!(back.segments(), planned);
        assert_eq!(back.size, Some(80 * MIN_SEGMENT));
        assert!(back.resumable);
    }

    #[test]
    fn splits_into_the_asked_for_number() {
        let s = plan(Some(80 * MIN_SEGMENT), 8, true);
        assert_eq!(s.len(), 8);
        assert_eq!(s[0].start, 0);
        // Contiguous, with no byte in two ranges and none in none.
        for pair in s.windows(2) {
            assert_eq!(pair[1].start, pair[0].end + 1);
        }
        assert_eq!(s.last().unwrap().end, 80 * MIN_SEGMENT - 1);
    }

    #[test]
    fn covers_every_byte_when_the_size_does_not_divide_evenly() {
        let size = 10 * MIN_SEGMENT + 7;
        let s = plan(Some(size), 8, true);
        let covered: u64 = s.iter().map(|x| x.span()).sum();
        assert_eq!(covered, size, "the plan lost or duplicated bytes");
    }

    #[test]
    fn a_small_file_is_not_split_into_slivers() {
        // Eight connections over half a megabyte is eight setups and no gain.
        assert_eq!(plan(Some(MIN_SEGMENT / 2), 8, true).len(), 1);
    }

    #[test]
    fn a_server_without_ranges_gets_one_connection() {
        assert_eq!(plan(Some(100 * MIN_SEGMENT), 8, false).len(), 1);
    }

    #[test]
    fn an_unknown_length_is_streamed_whole() {
        let s = plan(None, 8, true);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].end, u64::MAX);
    }

    #[test]
    fn an_empty_file_still_has_something_to_do() {
        assert_eq!(plan(Some(0), 8, true).len(), 1);
    }
}
