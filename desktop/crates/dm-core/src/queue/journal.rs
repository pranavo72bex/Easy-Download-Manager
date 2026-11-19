// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The queue file and its journal.
//!
//! A short list is written whole at every save, as it always was: indented,
//! for a person to read, and what the check scripts read. A long one (more
//! than [`PRETTY_UP_TO`] rows) was too, 9 MB at 20 000 rows, so a save cost
//! a tenth of a second and saves were spaced out to match (up to 5 s apart,
//! and a dm killed in between forgot what changed). Past that length a save
//! now appends to `queue.journal` only the rows that changed since the last
//! one, and the rows that went: a row that changed is one that is not the
//! same allocation as at the last write (the queue copies a row before it
//! changes it while anything still shares it, and the last write's list
//! always does). The whole list is written again when the journal grows
//! past half the file, when the order of the rows changed, and at every
//! start, which reads the journal back and folds it in.
//!
//! The journal's first line names the file it was written over (its length
//! and a hash of its bytes), so a journal left beside a newer file (a stop
//! between writing the file and removing the journal) is not replayed over
//! it. A line cut short by a stop mid-append ends the replay there: the
//! lines before it were each on disk before the next was written.
//!
//! The secrets (logins and sessions) are not in the journal. They stay in
//! `secrets.json`, written whole when one of them changed.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::persist::{secrets_file, Secret};
use super::{Entry, Id};

/// Rows up to which the queue file is indented, and written whole at
/// every save.
pub(super) const PRETTY_UP_TO: usize = 500;

/// What the queue file and its journal hold after a write.
pub(crate) struct Saved {
    /// The rows as written, shared with the queue: what tells the next
    /// write which rows changed.
    rows: Vec<Arc<Entry>>,
    /// The secrets as written, by row.
    secrets: HashMap<Id, Secret>,
    /// The queue file the journal is written over (`base_of`).
    base: String,
    file_bytes: u64,
    journal_bytes: u64,
}

/// One line of the journal after its first.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum Change {
    /// A row as it is now: one changed, or one added after the others.
    Row(Box<Entry>),
    /// A row that is no longer in the list.
    Gone(Id),
}

/// The queue's format number (`dm_core::saved`), in its journal's first line. The queue file
/// and `secrets.json` carry none: they stay bare lists, so an older dm still reads them (a
/// downgrade must never set the queue aside). 0, before the numbers: the line named the base
/// alone.
const JOURNAL_FORMAT: u32 = 1;

/// The journal's first line.
#[derive(serde::Serialize, serde::Deserialize)]
struct Header {
    base: String,
    /// The journal's format number; none (0) in one from before the numbers.
    #[serde(default)]
    format: u32,
}

pub(super) fn journal_file(queue_file: &Path) -> PathBuf {
    crate::paths::journal(queue_file)
}

/// What names a queue file's contents: its length and a hash of its bytes
/// (FNV-1a, which is the same on every build and every computer).
fn base_of(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{}:{hash:016x}", bytes.len())
}

/// Writes `rows` to `queue_file`: what changed since `previous` to the
/// journal where that will do, else the whole list.
pub(super) fn write(
    queue_file: &Path,
    rows: Vec<Arc<Entry>>,
    previous: Option<Saved>,
) -> std::io::Result<Saved> {
    match previous {
        Some(previous) => match changes(&previous, &rows) {
            Some((changed, gone)) => append(queue_file, rows, previous, &changed, gone),
            None => whole(queue_file, rows),
        },
        None => whole(queue_file, rows),
    }
}

/// The rows changed or added since `previous`, and the ids gone: `None`
/// where the whole list is to be written instead (a short list, or rows
/// in another order than appending and removing give; [`append`] also
/// writes it whole where the journal would grow past half the file).
fn changes(previous: &Saved, rows: &[Arc<Entry>]) -> Option<(Vec<Arc<Entry>>, Vec<Id>)> {
    if rows.len() <= PRETTY_UP_TO {
        return None;
    }
    let before: HashMap<Id, &Arc<Entry>> = previous.rows.iter().map(|r| (r.id, r)).collect();
    let now: std::collections::HashSet<Id> = rows.iter().map(|r| r.id).collect();
    // The rows kept, in the order they were written in, must come first and
    // in that order; anything after them is new.
    let mut kept = previous
        .rows
        .iter()
        .map(|r| r.id)
        .filter(|id| now.contains(id));
    let mut past_the_kept = false;
    let mut changed = Vec::new();
    for row in rows {
        match before.get(&row.id) {
            Some(was) => {
                if past_the_kept || kept.next() != Some(row.id) {
                    return None;
                }
                if !Arc::ptr_eq(was, row) {
                    changed.push(Arc::clone(row));
                }
            }
            None => {
                past_the_kept = true;
                changed.push(Arc::clone(row));
            }
        }
    }
    let gone = previous
        .rows
        .iter()
        .map(|r| r.id)
        .filter(|id| !now.contains(id))
        .collect();
    Some((changed, gone))
}

/// The whole list, its secrets beside it, and no journal.
fn whole(queue_file: &Path, rows: Vec<Arc<Entry>>) -> std::io::Result<Saved> {
    let mut entries: Vec<Entry> = rows.iter().map(|r| Entry::clone(r)).collect();
    let secrets = Secret::taken_from(&mut entries);
    write_secrets(queue_file, &secrets)?;
    // Indented for a person reading a short list; a long one is written
    // compact, a third of the bytes.
    let bytes = if entries.len() <= PRETTY_UP_TO {
        serde_json::to_vec_pretty(&entries)
    } else {
        serde_json::to_vec(&entries)
    }
    .map_err(std::io::Error::from)?;
    crate::write_atomic_with(queue_file, |w| w.write_all(&bytes))?;
    // What it held is in the file now. Left behind by a stop here, it names
    // the file before this one and is not replayed.
    match std::fs::remove_file(journal_file(queue_file)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    Ok(Saved {
        rows,
        secrets: secrets.into_iter().map(|s| (s.id, s)).collect(),
        base: base_of(&bytes),
        file_bytes: bytes.len() as u64,
        journal_bytes: 0,
    })
}

/// What changed, added to the journal; the secrets written whole where one
/// of them changed.
fn append(
    queue_file: &Path,
    rows: Vec<Arc<Entry>>,
    previous: Saved,
    changed: &[Arc<Entry>],
    gone: Vec<Id>,
) -> std::io::Result<Saved> {
    let mut secrets = previous.secrets.clone();
    for id in &gone {
        secrets.remove(id);
    }
    let mut entries: Vec<Entry> = changed.iter().map(|r| Entry::clone(r)).collect();
    for entry in &entries {
        secrets.remove(&entry.id);
    }
    for secret in Secret::taken_from(&mut entries) {
        secrets.insert(secret.id, secret);
    }
    // The secrets first, as for the whole list: a stop between the two
    // leaves a login the journal does not carry rather than the other way.
    if secrets != previous.secrets {
        let mut listed: Vec<Secret> = secrets.values().cloned().collect();
        listed.sort_by_key(|s| s.id);
        write_secrets(queue_file, &listed)?;
    }
    if entries.is_empty() && gone.is_empty() {
        return Ok(Saved {
            rows,
            secrets,
            ..previous
        });
    }
    let mut lines = Vec::new();
    if previous.journal_bytes == 0 {
        serde_json::to_writer(
            &mut lines,
            &Header {
                base: previous.base.clone(),
                format: JOURNAL_FORMAT,
            },
        )?;
        lines.push(b'\n');
    }
    for id in gone {
        serde_json::to_writer(&mut lines, &Change::Gone(id))?;
        lines.push(b'\n');
    }
    for entry in entries {
        serde_json::to_writer(&mut lines, &Change::Row(Box::new(entry)))?;
        lines.push(b'\n');
    }
    // A journal that would hold more than half what the file does is
    // written as the whole list instead: it would take as long to replay.
    if previous.journal_bytes + lines.len() as u64 > previous.file_bytes / 2 {
        return whole(queue_file, rows);
    }
    let path = journal_file(queue_file);
    let mut open = std::fs::OpenOptions::new();
    open.create(true).append(true);
    // Readable by this account alone, as the queue file is.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    let mut file = open.open(&path)?;
    // A journal shorter than written (a stop mid-append, then a start that
    // did not fold it in) is not added to: the next write is whole.
    if file.metadata()?.len() != previous.journal_bytes {
        drop(file);
        return whole(queue_file, rows);
    }
    file.write_all(&lines)?;
    file.sync_data()?;
    Ok(Saved {
        rows,
        secrets,
        journal_bytes: previous.journal_bytes + lines.len() as u64,
        ..previous
    })
}

pub(super) fn write_secrets(queue_file: &Path, secrets: &[Secret]) -> std::io::Result<()> {
    let kept = secrets_file(queue_file);
    if secrets.is_empty() {
        if kept.exists() {
            std::fs::remove_file(&kept)?;
        }
        return Ok(());
    }
    // Sealed under a key the operating system's store keeps where there is one; the same
    // bytes as ever where there is none (`secretstore.rs`).
    let plain = serde_json::to_vec_pretty(secrets).map_err(std::io::Error::from)?;
    let bytes = crate::secretstore::sealed(plain);
    crate::write_atomic_with(&kept, |w| w.write_all(&bytes))
}

/// `entries`, read from `queue_file` (whose bytes are `text`), with the
/// journal written over that file replayed on them. A journal over another
/// file, or none, leaves them as they are; a line that cannot be read ends
/// the replay there.
pub(super) fn replayed(queue_file: &Path, text: &[u8], entries: Vec<Entry>) -> Vec<Entry> {
    let Ok(journal) = std::fs::read_to_string(journal_file(queue_file)) else {
        return entries;
    };
    let mut lines = journal.lines();
    let over_this = lines
        .next()
        .and_then(|first| serde_json::from_str::<Header>(first).ok())
        .is_some_and(|h| h.base == base_of(text));
    if !over_this {
        return entries;
    }
    let mut rows: Vec<Option<Entry>> = entries.into_iter().map(Some).collect();
    let mut at: HashMap<Id, usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.as_ref().map(|r| (r.id, i)))
        .collect();
    for line in lines {
        match serde_json::from_str::<Change>(line) {
            Ok(Change::Row(entry)) => match at.get(&entry.id) {
                Some(&i) => rows[i] = Some(*entry),
                None => {
                    at.insert(entry.id, rows.len());
                    rows.push(Some(*entry));
                }
            },
            Ok(Change::Gone(id)) => {
                if let Some(i) = at.remove(&id) {
                    rows[i] = None;
                }
            }
            Err(_) => break,
        }
    }
    rows.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests;
