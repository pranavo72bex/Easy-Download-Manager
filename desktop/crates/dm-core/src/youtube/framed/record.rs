// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What one format's file of the framed stream holds, written down beside it after each whole
//! piece (`name.f137.mp4.framed.held`), so a paused or failed fetch goes on from its last
//! whole piece, as a ranged download goes on from its ledger.
//!
//! Written after the piece's bytes are on the disk itself (synced), so it never says more
//! than the file holds; one cut short or lagging reads as less, or as nothing, never as more.
//! Always as many bytes, written over the last in one write, as a range's ledger is, with a
//! check of its words at its end, so a record torn by a crash reads as nothing.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use tokio::io::{AsyncSeekExt as _, AsyncWriteExt as _};

use super::Pick;
use crate::Error;

/// The first word of a record, naming its layout.
const TAG: &str = "dm-framed-1";

/// What a format's file holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Record {
    /// The format's number.
    pub itag: u64,
    /// Its last-modified time, where the answer gave one: another means another file.
    pub lmt: Option<u64>,
    /// Its first word was heard.
    pub started: bool,
    /// Its first boxes are written.
    pub init: bool,
    /// The first piece held.
    pub first: Option<u64>,
    /// The last piece held.
    pub last: Option<u64>,
    /// Where what it holds ends, in milliseconds.
    pub held_ms: u64,
    /// The bytes of the file it speaks for.
    pub written: u64,
    /// Its last piece's number, as its first word said.
    pub end_segment: Option<u64>,
    /// Where it ends, in milliseconds, as its first word said.
    pub end_ms: Option<u64>,
}

impl Record {
    /// Nothing held yet of format `pick`.
    pub(super) fn of(pick: &Pick) -> Record {
        Record {
            itag: u64::from(pick.itag),
            lmt: pick.lmt,
            ..Record::default()
        }
    }

    /// The record's words: its tag, each field in twenty places (a number, or dashes for
    /// none), and the check of all before it.
    pub(super) fn text(&self) -> String {
        let mut out = String::from(TAG);
        for field in self.fields() {
            match field {
                Some(n) => out.push_str(&format!(" {n:020}")),
                None => out.push_str(&format!(" {:-<20}", "")),
            }
        }
        let check = check(&out);
        out.push_str(&format!(" {check:020}\n"));
        out
    }

    /// The record `text` holds, where it reads whole and its check agrees.
    pub(super) fn parse(text: &str) -> Option<Record> {
        let (body, check_word) = text.strip_suffix('\n')?.rsplit_once(' ')?;
        if check_word.parse::<u64>().ok()? != check(body) {
            return None;
        }
        let mut words = body.split(' ');
        if words.next()? != TAG {
            return None;
        }
        let field = |word: &str| -> Option<Option<u64>> {
            if word.len() == 20 && word.bytes().all(|b| b == b'-') {
                return Some(None);
            }
            word.parse().ok().map(Some)
        };
        let fields = words.map(field).collect::<Option<Vec<_>>>()?;
        let flag = |v: Option<u64>| match v {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        };
        let &[itag, lmt, started, init, first, last, held_ms, written, end_segment, end_ms] =
            fields.as_slice()
        else {
            return None;
        };
        Some(Record {
            itag: itag?,
            lmt,
            started: flag(started)?,
            init: flag(init)?,
            first,
            last,
            held_ms: held_ms?,
            written: written?,
            end_segment,
            end_ms,
        })
    }

    fn fields(&self) -> [Option<u64>; 10] {
        [
            Some(self.itag),
            self.lmt,
            Some(u64::from(self.started)),
            Some(u64::from(self.init)),
            self.first,
            self.last,
            Some(self.held_ms),
            Some(self.written),
            self.end_segment,
            self.end_ms,
        ]
    }
}

/// The check of a record's words (FNV-1a).
fn check(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Where the record of the file `partial` is kept: beside it.
pub(super) fn path_of(partial: &Path) -> PathBuf {
    let mut name = partial.as_os_str().to_owned();
    name.push(".held");
    PathBuf::from(name)
}

/// Writes `record` over the one before it, in one write, and hands it to the system.
pub(super) async fn put(file: &mut tokio::fs::File, record: &Record) -> Result<(), Error> {
    file.seek(SeekFrom::Start(0)).await?;
    file.write_all(record.text().as_bytes()).await?;
    file.flush().await?;
    Ok(())
}

/// The file `partial` of format `pick` and its record as an earlier run left them, where they
/// agree: the file open at the end of what the record says it holds. A file that holds more
/// (the bytes of a piece its record never spoke for) is cut back to that. `None` where there is
/// no record, it does not read, it is another format's, nothing whole is held, or the file
/// holds less than it says: that format is fetched again from its start.
pub(super) async fn taken_up(partial: &Path, pick: &Pick) -> Option<(tokio::fs::File, Record)> {
    let text = tokio::fs::read_to_string(path_of(partial)).await.ok()?;
    let record = Record::parse(&text)?;
    if record.itag != u64::from(pick.itag)
        || record.lmt != pick.lmt
        || !record.init
        || record.written == 0
    {
        return None;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(partial)
        .await
        .ok()?;
    let length = file.metadata().await.ok()?.len();
    if length < record.written {
        return None;
    }
    if length > record.written {
        file.set_len(record.written).await.ok()?;
    }
    file.seek(SeekFrom::Start(record.written)).await.ok()?;
    Some((file, record))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick() -> Pick {
        Pick {
            itag: 137,
            lmt: Some(1_700_000_000_000_137),
            ..Pick::default()
        }
    }

    fn held() -> Record {
        Record {
            started: true,
            init: true,
            first: Some(1),
            last: Some(2),
            held_ms: 4_000,
            written: 10,
            end_segment: Some(3),
            ..Record::of(&pick())
        }
    }

    #[test]
    fn a_record_reads_back_as_written_and_is_always_as_long() {
        let record = held();
        assert_eq!(Record::parse(&record.text()), Some(record));
        let none = Record::of(&Pick::default());
        assert_eq!(Record::parse(&none.text()), Some(none));
        assert_eq!(none.text().len(), record.text().len());
        // A word changed, torn or cut short reads as nothing.
        let text = record.text();
        let changed = text.replace(" 00000000000000004000", " 00000000000000009000");
        assert_ne!(changed, text);
        assert_eq!(Record::parse(&changed), None);
        assert_eq!(Record::parse(&text[..text.len() / 2]), None);
        assert_eq!(Record::parse(""), None);
    }

    #[tokio::test]
    async fn a_file_is_taken_up_only_where_its_record_agrees() {
        let dir = tempfile::tempdir().unwrap();
        let partial = dir.path().join("v.f137.mp4.framed");
        let write = |bytes: &[u8], record: &Record| {
            std::fs::write(&partial, bytes).unwrap();
            std::fs::write(path_of(&partial), record.text()).unwrap();
        };
        // As long as it says: taken up at its end.
        write(b"0123456789", &held());
        let (_, record) = taken_up(&partial, &pick()).await.expect("taken up");
        assert_eq!(record, held());
        // Longer: cut back to what it says.
        write(b"0123456789half a piece", &held());
        assert!(taken_up(&partial, &pick()).await.is_some());
        assert_eq!(std::fs::read(&partial).unwrap(), b"0123456789");
        // Shorter, another format's, or no record: started again.
        write(b"01234", &held());
        assert!(taken_up(&partial, &pick()).await.is_none());
        let other = Pick {
            lmt: Some(1),
            ..pick()
        };
        write(b"0123456789", &held());
        assert!(taken_up(&partial, &other).await.is_none());
        std::fs::remove_file(path_of(&partial)).unwrap();
        assert!(taken_up(&partial, &pick()).await.is_none());
    }
}
