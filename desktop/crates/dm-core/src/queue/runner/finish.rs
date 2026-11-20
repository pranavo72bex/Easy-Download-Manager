// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A run ended: its outcome recorded, the finished file put in place, and the row told.

use super::*;

impl DownloadList {
    /// What a run that ended leaves on its row: its files placed or cleared away, and its
    /// outcome recorded. Whether the row was written and a save is owed: not for a row
    /// deleted or cancelled while it ran, or put back in line since.
    pub(super) async fn finish_run(
        &self,
        id: Id,
        url: &str,
        options: &Options,
        ran_with: &Control,
        outcome: Result<crate::download::Finished, Error>,
        reported: Option<u64>,
    ) -> bool {
        if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
            slot.swarm = None;
        }
        /* Reports are samples: a file whole before the next one was
         * sent had bytes no report carried, and a small download could
         * go uncounted by the download limit altogether. What a
         * finished file holds past the last report counts now. */
        if let (Ok(finished), Some(reported)) = (&outcome, reported) {
            let unreported = finished.bytes.saturating_sub(reported);
            if unreported > 0 {
                crate::lock(&self.inner.quota).add(unreported, std::time::SystemTime::now());
            }
        }

        let mut slots = self.inner.slots.lock().await;
        let Some(slot) = slots.get_mut(&id) else {
            /* Deleted while it ran. Its parts were removed while this
             * run still held some (on Windows an open file cannot be
             * deleted), and a file it finished meanwhile is one nobody
             * asked to keep: both go, as for a cancelled row. */
            drop(slots);
            if let Ok(finished) = &outcome {
                let _ = tokio::fs::remove_file(&finished.path).await;
            }
            let _ = tokio::fs::remove_dir_all(&options.parts_dir).await;
            return false;
        };

        /* Cancelled while it ran: the row already says so and its parts
         * are gone. Whatever the task made of that — a join that failed
         * on the missing parts, or a file it had finished a moment
         * before — must not turn the row back into Failed or Completed,
         * and a file it did produce is not one anybody asked to keep. */
        if slot.entry.state == State::Cancelled {
            // The cancel removed the parts while this run still held
            // some open — a helper's — and on Windows an
            // open file cannot be deleted. Now the run has let go,
            // whatever is left goes.
            let leftover = PathBuf::from(&slot.entry.parts_dir);
            // Let go of before the disk is asked anything: every read of
            // the list waits for this lock (gaps row 184).
            drop(slots);
            if let Ok(finished) = outcome {
                if let Err(e) = tokio::fs::remove_file(&finished.path).await {
                    self.note(format!(
                        "a cancelled download finished first and its file could not be \
                         removed from {}: {e}",
                        finished.path.display()
                    ));
                }
            }
            if !leftover.as_os_str().is_empty() {
                let _ = tokio::fs::remove_dir_all(&leftover).await;
            }
            return false;
        }

        /* Stopped and already put back in line: the row carries a fresh
         * control, and this task's word on how its run ended — Paused,
         * usually, seen at the read timeout — is about a run the row has
         * moved past. Writing it would turn a Queued row back into a
         * Paused one that nothing restarts. A run that finished is still
         * finished, whoever asked for it. */
        if !slot.control.is(ran_with) && outcome.is_err() {
            return false;
        }

        // A name a person chose replaces the server's once the file is
        // whole, the same way any other name is placed: never over a
        // file already there.
        let wanted = slot
            .entry
            .save_name
            .clone()
            .filter(|n| !n.trim().is_empty());
        let replaces = slot.entry.replaces.clone().filter(|p| !p.is_empty());
        drop(slots);
        let outcome = self
            .place_finished(outcome, wanted, replaces.as_deref(), url, options)
            .await;
        // Out of room, or the folder's drive gone: paused, its parts
        // kept, rather than failed, to go on once there is room or
        // from another folder. Worked out before the queue's lock is
        // taken, off the runtime's threads: the folder is looked for on
        // the disk, and on a drive just taken out that can take seconds,
        // which every read of the list waited out (gaps row 184).
        let (outcome, stalled) = match outcome {
            Err(error) if replaces.is_none() => {
                let dirs = [options.parts_dir.clone(), options.dest_dir.clone()];
                let looked = tokio::task::spawn_blocking(move || {
                    let why = out_of_room(&error, [&dirs[0], &dirs[1]]);
                    (error, why)
                })
                .await;
                match looked {
                    Ok((error, why)) => (Err(error), why),
                    Err(e) => (Err(Error::Internal(e.to_string())), None),
                }
            }
            other => (other, None),
        };
        self.record_outcome(id, outcome, stalled, replaces.as_deref())
            .await
    }

    /// A finished file under the name a person chose, in place of the old version it
    /// replaces, marked as from the internet; its parts' folders gone.
    async fn place_finished(
        &self,
        outcome: Result<crate::download::Finished, Error>,
        wanted: Option<String>,
        replaces: Option<&str>,
        url: &str,
        options: &Options,
    ) -> Result<crate::download::Finished, Error> {
        let outcome = match (outcome, wanted) {
            (Ok(mut finished), Some(name)) => {
                match rename_free(&finished.path, &name).await {
                    Ok(path) => finished.path = path,
                    Err(e) => self.note(format!(
                        "{} could not be renamed to {name}: {e}",
                        finished.path.display()
                    )),
                }
                Ok(finished)
            }
            (outcome, _) => outcome,
        };
        // A recheck's new version, whole now: it takes the old one's
        // place, which was kept until this moment so that a fetch that
        // failed left the old file rather than nothing.
        let outcome = match (outcome, replaces) {
            (Ok(mut finished), Some(old)) => {
                let old = PathBuf::from(old);
                if finished.path != old {
                    match tokio::fs::rename(&finished.path, &old).await {
                        Ok(()) => finished.path = old,
                        Err(e) => self.note(format!(
                            "the new version of {} could not replace it, and is at {}: {e}",
                            old.display(),
                            finished.path.display()
                        )),
                    }
                }
                Ok(finished)
            }
            (outcome, _) => outcome,
        };
        if let Ok(crate::download::Finished {
            left_behind: Some(note),
            ..
        }) = &outcome
        {
            self.note(note.clone());
        }
        // Finished, its parts are spent: the folder they were staged in
        // goes too, when nothing else is left in it.
        if outcome.is_ok() {
            let _ = tokio::fs::remove_dir(&options.parts_dir).await;
            // And the folder that held it in a place a person chose,
            // once nothing else is staged there.
            if let Some(holder) = options
                .parts_dir
                .parent()
                .filter(|p| p.file_name().is_some_and(|n| n == UNFINISHED))
            {
                let _ = tokio::fs::remove_dir(holder).await;
            }
        }
        // From the internet, as a browser's download says: marked here,
        // before the queue's lock and on a thread of its own. On Windows
        // the mark is a stream of the file's own, which the virus checker
        // reads as it is written: about 16 ms a file on a Windows 11
        // computer (2026-11-03), during which the lock would hold every
        // other look at the queue, the window's included. Still before the row
        // says Completed, so a file is never opened unmarked.
        if let Ok(finished) = &outcome {
            let path = finished.path.clone();
            let from = url.to_owned();
            let referrer = options.referrer.clone().unwrap_or_default();
            let _ = tokio::task::spawn_blocking(move || {
                crate::motw::mark(&path, &from, &referrer);
            })
            .await;
        }
        // On a phone, a file finished in the app's own folder is put in the shared Download
        // folder (gaps row N9), before the row says Completed, so the row names it there.
        #[cfg(target_os = "android")]
        let outcome = self.publish_finished(outcome, url).await;
        if let Ok(finished) = &outcome {
            // Put on the disk from here, not waited for, once the renames and the mark are
            // done (`download::sync_after`).
            crate::download::sync_after(finished.path.clone());
        }
        outcome
    }

    /// A finished file handed to the app's publisher (`folders/publish.rs`), off the runtime's
    /// threads: a large file is a long copy. A torrent's files stay where its seeding reads
    /// them, and a folder where it is; a file that could not be published stays, whole, in
    /// dm's own folder, and the person is told why.
    #[cfg(target_os = "android")]
    async fn publish_finished(
        &self,
        outcome: Result<crate::download::Finished, Error>,
        url: &str,
    ) -> Result<crate::download::Finished, Error> {
        let mut finished = match outcome {
            Ok(finished) => finished,
            Err(error) => return Err(error),
        };
        let Some(publisher) = crate::folders::publisher() else {
            return Ok(finished);
        };
        if crate::torrent::is_torrent(url) {
            return Ok(finished);
        }
        let path = finished.path.clone();
        let asked = tokio::task::spawn_blocking(move || {
            if path.is_file() {
                crate::folders::published(&path, &*publisher)
            } else {
                (path, None)
            }
        })
        .await;
        match asked {
            Ok((path, why)) => {
                finished.path = path;
                if let Some(why) = why {
                    self.note(why);
                }
            }
            Err(e) => self.note(format!(
                "{} stays in dm's own folder: {e}",
                finished.path.display()
            )),
        }
        Ok(finished)
    }

    /// The row's state, as the run ended: Completed with its file, Paused (asked for, or out
    /// of room), Cancelled, or Failed with why. Whether the row was there to write.
    async fn record_outcome(
        &self,
        id: Id,
        outcome: Result<crate::download::Finished, Error>,
        stalled: Option<String>,
        replaces: Option<&str>,
    ) -> bool {
        let mut slots = self.inner.slots.lock().await;
        let Some(slot) = slots.get_mut(&id) else {
            return false;
        };

        slot.retrying = None;
        slot.entry_mut().stalled.clear();
        let outcome = match (outcome, stalled) {
            (Err(_), Some(why)) => {
                slot.entry_mut().stalled = why;
                Err(Error::Paused)
            }
            (outcome, _) => outcome,
        };
        // The failure's kind, kept beside its sentence (a row that is about to replace an old
        // version, or whose link expired, fails of the kind its error is).
        let kind = outcome.as_ref().err().map(Error::fail_kind);
        slot.entry_mut().state = match outcome {
            Ok(finished) => {
                // Usually the name the probe already reported, but
                // assembly has the last word: it is what avoided a
                // collision with a file already on disk.
                if let Some(name) = finished.path.file_name() {
                    slot.entry_mut().filename = name.to_string_lossy().into_owned();
                }
                slot.entry_mut().path = finished.path.to_string_lossy().into_owned();
                slot.entry_mut().category = Some(finished.category);
                slot.entry_mut().verified = finished.verified;
                slot.entry_mut().mirrors = finished.mirrors;
                slot.entry_mut().replaces = None;
                if std::mem::take(&mut slot.entry_mut().cookies_once) {
                    slot.entry_mut().cookies.clear();
                    slot.entry_mut().headers.clear();
                }
                State::Completed
            }
            // Both were asked for, and the queue has already recorded
            // them; overwriting would undo a pause the moment it landed.
            Err(Error::Paused) => State::Paused,
            Err(Error::Cancelled) => State::Cancelled,
            // The row still points at the old file, which is still
            // there; said, so a failed recheck does not read as a lost
            // file. Retry fetches the new version again.
            Err(error) => match replaces {
                Some(old) => State::Failed(format!(
                    "the new version could not be fetched, so the old one is kept at {old}: \
                     {error}"
                )),
                None if error.link_expired()
                    && (!slot.entry.referrer.is_empty()
                        || !slot.entry.cookies.is_empty()
                        || !slot.entry.headers.is_empty()) =>
                {
                    slot.entry_mut().fresh_link = true;
                    State::Failed(format!("{LINK_EXPIRED} ({error})"))
                }
                None => State::Failed(error.to_string()),
            },
        };
        let kind = kind.filter(|_| matches!(slot.entry.state, State::Failed(_)));
        if slot.entry.fail_kind != kind {
            slot.entry_mut().fail_kind = kind;
        }
        // What turns a settled row into history.
        if slot.entry.state.settled() {
            slot.entry_mut().finished_at = Some(now_seconds());
        }
        // However the run ended, it is not running now.
        slot.stopped_moving();
        // Dropped before saving: save() reads the queue back through
        // this same lock, so holding it here would deadlock the task
        // that just finished a download.
        drop(slots);
        true
    }
}
