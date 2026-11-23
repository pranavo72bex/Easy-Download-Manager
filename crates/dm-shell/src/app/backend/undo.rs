// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Undo: finished rows taken off the list, kept a while to be put back.

use dm_core::{Entry, Id};

use super::Backend;

/// How long after downloads were taken off the list Undo puts them back: long enough to see the
/// sentence and reach the button (gaps row U14), unless another command changes the list first.
pub const UNDO_FOR: std::time::Duration = std::time::Duration::from_secs(15);

/// Finished rows taken off the list, kept for [`UNDO_FOR`] so Undo can put
/// them back where they were.
#[derive(Debug, Default)]
pub struct Undo(Option<(std::time::Instant, Vec<(usize, Entry)>)>);

impl Undo {
    /// Kept since `at` (the window's tests set a time).
    pub fn since(at: std::time::Instant, rows: Vec<(usize, Entry)>) -> Undo {
        Undo(Some((at, rows)))
    }

    /// `rows` were just taken off (`Backend::finished_rows` before the
    /// removal): kept, in place of any before. How many; none keeps nothing.
    pub fn keep(&mut self, rows: Vec<(usize, Entry)>) -> usize {
        let n = rows.len();
        if n > 0 {
            self.0 = Some((std::time::Instant::now(), rows));
        }
        n
    }

    /// Whether Undo is still on offer; put away once its time is over.
    pub fn live(&mut self) -> bool {
        match &self.0 {
            Some((at, _)) if at.elapsed() < UNDO_FOR => true,
            Some(_) => {
                self.0 = None;
                false
            }
            None => false,
        }
    }

    /// How many downloads Undo would put back.
    pub fn count(&self) -> usize {
        self.0.as_ref().map_or(0, |(_, rows)| rows.len())
    }

    /// Undo is no longer on offer: another command changed the list.
    pub fn forget(&mut self) {
        self.0 = None;
    }

    /// Whether anything is kept, on offer or not yet put away.
    pub fn is_kept(&self) -> bool {
        self.0.is_some()
    }

    /// As if kept `by` earlier (the window's tests).
    pub fn backdate(&mut self, by: std::time::Duration) {
        if let Some((at, _)) = self.0.as_mut() {
            *at = at.checked_sub(by).unwrap_or(*at);
        }
    }

    /// Puts the rows back through `backend` while Undo is on offer: their
    /// ids, or `None` (nothing kept, or its time over). Once only.
    pub fn put_back(&mut self, backend: &Backend) -> Option<Vec<Id>> {
        let rows = self.take()?;
        let ids = rows.iter().map(|(_, e)| e.id).collect();
        backend.put_back(rows);
        Some(ids)
    }

    /// The rows kept, while Undo is on offer, for the caller to put back
    /// (`Backend::put_back`) where it may wait: the window hands that off
    /// its thread. `None` where nothing is kept or its time is over. Once
    /// only.
    pub fn take(&mut self) -> Option<Vec<(usize, Entry)>> {
        let (at, rows) = self.0.take()?;
        if at.elapsed() >= UNDO_FOR || rows.is_empty() {
            return None;
        }
        Some(rows)
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;
    use crate::app::backend::testing::backend;

    fn finished(id: Id) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": format!("http://127.0.0.1:9/{id}.bin"),
            "state": serde_json::to_value(dm_core::State::Completed).unwrap(),
            "progress": {"downloaded": 1, "total": 1, "speed": 0.0, "eta": null},
            "filename": format!("{id}.bin"),
        }))
        .unwrap()
    }

    /// Finished rows taken off are put back once, while Undo is on offer,
    /// and not after its time.
    #[test]
    fn undo_puts_the_rows_taken_off_back_once_and_only_in_its_time() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        backend.put_back(vec![(0, finished(7)), (1, finished(8))]);
        let ids: Vec<Id> = backend.snapshot().rows.iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), 2, "{ids:?}");
        let mut undo = Undo::default();
        assert!(
            !undo.live() && undo.put_back(&backend).is_none(),
            "nothing kept"
        );
        assert_eq!(undo.keep(backend.finished_rows(&ids)), 2);
        backend.remove_many(&ids);
        assert!(backend.snapshot().rows.is_empty());
        assert!(undo.live());
        let mut back = undo.put_back(&backend).unwrap();
        back.sort_unstable();
        let mut want = ids.clone();
        want.sort_unstable();
        assert_eq!(back, want);
        assert_eq!(backend.snapshot().rows.len(), 2);
        assert!(undo.put_back(&backend).is_none(), "once");
        // Its time over: put away, and nothing put back.
        assert_eq!(undo.keep(backend.finished_rows(&ids)), 2);
        backend.remove_many(&ids);
        undo.backdate(UNDO_FOR);
        assert!(!undo.live() && !undo.is_kept());
        assert!(undo.put_back(&backend).is_none());
        assert!(backend.snapshot().rows.is_empty());
        assert_eq!(undo.keep(Vec::new()), 0, "nothing to keep");
        assert!(!undo.is_kept());
    }

    /// Undo's rows are given once, while it is on offer, for a window to put
    /// back off its own thread; none after its time.
    #[test]
    fn undo_gives_its_rows_once_while_on_offer() {
        let mut undo = Undo::default();
        assert!(undo.take().is_none(), "nothing kept");
        assert_eq!(undo.keep(vec![(0, finished(7))]), 1);
        let rows = undo.take().unwrap();
        assert_eq!(rows.iter().map(|(_, e)| e.id).collect::<Vec<_>>(), [7]);
        assert!(undo.take().is_none() && !undo.is_kept(), "once");
        assert_eq!(undo.keep(vec![(0, finished(8))]), 1);
        undo.backdate(UNDO_FOR);
        assert!(undo.take().is_none(), "its time over");
    }
}
