// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl Queue {
    /// Points a stopped download at another folder, its unfinished parts
    /// moved there too (they may be on the disk that filled), and puts it
    /// back in line. Parts on a drive no longer there are left: it starts
    /// afresh in the new folder.
    pub async fn relocate(&self, id: Id, dir: PathBuf) -> Result<(), String> {
        let old = {
            let slots = self.inner.slots.lock().await;
            let slot = slots.get(&id).ok_or("that download is no longer listed")?;
            if matches!(slot.entry.state, State::Running | State::Queued) || slot.still_stopping() {
                return Err("stop the download before giving it another folder".into());
            }
            if slot.entry.state.settled() {
                return Err("a finished download is moved with Move/Rename".into());
            }
            PathBuf::from(&slot.entry.parts_dir)
        };
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
        let parts = dir.join(UNFINISHED).join(id.to_string());
        if !old.as_os_str().is_empty() && old.is_dir() && old != parts {
            move_dir(&old, &parts).await.map_err(|e| {
                format!(
                    "the unfinished parts could not be moved to {}: {e}",
                    dir.display()
                )
            })?;
        }
        {
            let mut slots = self.inner.slots.lock().await;
            let Some(slot) = slots.get_mut(&id) else {
                return Err("that download is no longer listed".into());
            };
            let folder = dir.display().to_string();
            slot.entry_mut().parts_dir = parts.display().to_string();
            slot.entry_mut().save_dir = Some(folder.clone());
            slot.entry_mut().began_in = folder;
            slot.requeue();
        }
        self.kick();
        self.save().await;
        Ok(())
    }

    /// Where a download begun while its Download details was still open goes, once the person
    /// chose (reference A's "start downloading while the dialog is open"; gaps row 85): its
    /// folder and name, its description, and whether it waits in `queue` (`later`) or goes on.
    /// One already finished is renamed and moved as Move/Rename does; one still under way is
    /// stopped a moment, given its place, and put back in line keeping what it has (held, for
    /// later).
    pub async fn settle_early(
        &self,
        id: Id,
        dir: Option<PathBuf>,
        name: Option<String>,
        description: &str,
        queue: &str,
        later: bool,
    ) -> Result<(), String> {
        self.set_description(id, description).await;
        if !queue.is_empty() {
            self.set_queue(id, queue).await;
        }
        let name = name.filter(|n| !n.trim().is_empty());
        let finished = |e: &Entry| e.state == State::Completed;
        let entry = self
            .get(id)
            .await
            .ok_or("that download is no longer listed")?;
        if !finished(&entry) {
            self.pause(id).await;
            // Its run winds down before its place can change.
            for _ in 0..200 {
                let stopping = self
                    .inner
                    .slots
                    .lock()
                    .await
                    .get(&id)
                    .is_some_and(Slot::still_stopping);
                if !stopping {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }
        let entry = self
            .get(id)
            .await
            .ok_or("that download is no longer listed")?;
        if finished(&entry) {
            if let Some(name) = name.filter(|n| *n != entry.filename) {
                self.rename(id, &name).await?;
            }
            if let Some(dir) = dir {
                self.move_to(id, &dir).await?;
            }
            return Ok(());
        }
        self.edit(id, |slot| {
            slot.entry_mut().save_name = name.clone();
            true
        })
        .await;
        let elsewhere = dir
            .as_ref()
            .is_some_and(|d| entry.save_dir.as_deref().map(PathBuf::from).as_ref() != Some(d));
        match dir {
            Some(dir) if elsewhere => {
                self.relocate(id, dir).await?;
                if later {
                    self.pause(id).await;
                }
            }
            _ if !later => self.resume(id).await,
            _ => {}
        }
        Ok(())
    }

    /// Sets the hash the finished file is checked against, or clears it.
    pub async fn set_checksum(&self, id: Id, checksum: Option<Checksum>) {
        if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
            slot.entry_mut().checksum = checksum;
        }
        self.save().await;
    }

    /// Forgets every row that completed or was cancelled.
    ///
    /// The history is the queue: rows stay after they finish, which is what
    /// makes "where did that go" answerable. This is how it is emptied.
    /// Failed rows are kept with their staged parts (`State::clearable`):
    /// clearing them deleted what Retry would have resumed from, with no
    /// question asked.
    pub async fn clear_finished(&self) -> usize {
        let removed: Vec<Id> = {
            let slots = self.inner.slots.lock().await;
            slots
                .values()
                .filter(|s| s.entry.state.clearable())
                .map(|s| s.entry.id)
                .collect()
        };
        // Together rather than one after another: clearing a hundred rows is
        // a hundred recursive deletes, and they do not depend on each other.
        futures_util::future::join_all(removed.iter().map(|id| self.discard_parts(*id))).await;
        {
            let mut slots = self.inner.slots.lock().await;
            let mut order = self.inner.order.lock().await;
            for id in &removed {
                self.take_slot(&mut slots, *id);
            }
            // A set, because the history is the queue: clearing a thousand
            // finished rows out of a thousand is a million comparisons with
            // `Vec::contains` and a thousand without.
            let gone: std::collections::HashSet<Id> = removed.iter().copied().collect();
            order.retain(|id| !gone.contains(id));
        }
        self.save().await;
        removed.len()
    }

    /// Stops a running download, or holds a queued one back.
    pub async fn pause(&self, id: Id) {
        let mut slots = self.inner.slots.lock().await;
        let Some(slot) = slots.get_mut(&id) else {
            return;
        };
        if slot.entry.state.settled() {
            return;
        }
        // A deliberate pause, so the schedule must not undo it when the
        // window next opens.
        slot.hold(false);
        drop(slots);
        self.save().await;
    }

    /// What a dm that is ending does: every running download stopped as a
    /// pause stops it (what each connection holds written to its part
    /// file, its ledger brought up to date), its task awaited up to
    /// `within`, and the row put back in line, so the next start resumes it
    /// from its parts as one that was running always has. Nothing starts
    /// once this is called. The queue file is written last. Says how many
    /// runs were still going at the deadline: those are aborted, and resume
    /// from their ledgers as after a crash.
    ///
    /// Without it the runs went down with the runtime, mid-write, and the
    /// next start trusted ledgers a run had not finished bringing up to
    /// date (gaps row 186).
    pub async fn shut_down(&self, within: std::time::Duration) -> usize {
        let deadline = tokio::time::Instant::now() + within;
        self.inner.closing.store(true, Ordering::SeqCst);
        let stopped: Vec<Id> = {
            let slots = self.inner.slots.lock().await;
            slots
                .iter()
                .filter(|(_, slot)| slot.entry.state == State::Running)
                .map(|(id, slot)| {
                    // The control alone, not `hold`: the row is not paused
                    // by anyone, and goes back in line below.
                    slot.control.pause();
                    *id
                })
                .collect()
        };
        // Each run ends at its next piece, or at its read timeout's check;
        // looked at often, as the person is waiting for dm to go.
        let left = loop {
            let left = {
                let slots = self.inner.slots.lock().await;
                slots.values().filter(|slot| slot.still_stopping()).count()
            };
            if left == 0 || tokio::time::Instant::now() >= deadline {
                break left;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        {
            let mut slots = self.inner.slots.lock().await;
            for slot in slots.values_mut() {
                if slot.still_stopping() {
                    if let Some(task) = &slot.task {
                        task.abort();
                    }
                }
            }
            for id in &stopped {
                if let Some(slot) = slots.get_mut(id) {
                    if matches!(slot.entry.state, State::Paused | State::Running) {
                        slot.entry_mut().state = State::Queued;
                        slot.stopped_moving();
                    }
                }
            }
        }
        self.inner.unwritten.store(true, Ordering::SeqCst);
        self.save().await;
        self.flush().await;
        left
    }

    /// Puts a paused download back in line. It resumes from what is staged.
    pub async fn resume(&self, id: Id) {
        let mut slots = self.inner.slots.lock().await;
        let Some(slot) = slots.get_mut(&id) else {
            return;
        };
        if slot.entry.state != State::Paused {
            return;
        }
        slot.requeue();
        drop(slots);
        self.kick();
        self.save().await;
    }

    /// Starts one row now, whatever the schedule or the download limit
    /// says: a person asked for this one, and only this one. The schedule
    /// leaves its run alone; a pause, or the row going back in line, ends
    /// that. False for a row that is not waiting, failed or stopped, or
    /// whose last run is still winding down.
    pub async fn start_outside_schedule(&self, id: Id) -> bool {
        let (url, control) = {
            let mut slots = self.inner.slots.lock().await;
            let Some(slot) = slots.get_mut(&id) else {
                return false;
            };
            if !matches!(
                slot.entry.state,
                State::Paused | State::Queued | State::Failed(_)
            ) || slot.still_stopping()
            {
                return false;
            }
            slot.requeue();
            slot.entry_mut().state = State::Running;
            slot.outside_schedule = true;
            (slot.entry.url.clone(), slot.control.clone())
        };
        let task = self.start(id, url, control);
        if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
            slot.task = Some(task);
        }
        self.kick();
        self.save().await;
        true
    }

    /// Pauses everything unsettled, and says how many that was.
    ///
    /// One pass under one lock and one save, rather than a `pause` per row:
    /// a hundred rows would otherwise be a hundred rewrites of the queue
    /// file, ninety-nine of which describe a state nobody saw.
    pub async fn pause_all(&self) -> usize {
        let mut paused = 0;
        {
            let mut slots = self.inner.slots.lock().await;
            for slot in slots.values_mut() {
                if slot.entry.state.settled() {
                    continue;
                }
                // Already held by the schedule, it is now held by a person
                // too: left the schedule's, the window opening started it
                // again, undoing the "Pause all" that came after.
                if slot.entry.state == State::Paused {
                    if std::mem::take(&mut slot.entry_mut().paused_by_schedule) {
                        paused += 1;
                    }
                    continue;
                }
                slot.hold(false);
                paused += 1;
            }
        }
        if paused > 0 {
            self.save().await;
        }
        paused
    }

    /// Puts every paused download back in line.
    pub async fn resume_all(&self) -> usize {
        let mut resumed = 0;
        {
            let mut slots = self.inner.slots.lock().await;
            for slot in slots.values_mut() {
                if slot.entry.state != State::Paused {
                    continue;
                }
                // Not one added for later, waiting for its queue to be
                // started: Resume all is for what was stopped, and started
                // a file put aside on purpose.
                let for_later = !slot.entry.paused_by_schedule
                    && !slot.entry.queue.is_empty()
                    && slot.entry.progress.downloaded == 0;
                if for_later {
                    continue;
                }
                slot.requeue();
                resumed += 1;
            }
        }
        if resumed > 0 {
            self.kick();
            self.save().await;
        }
        resumed
    }

    /// Puts a failed download back in line to try again, and says whether
    /// there was one to put back.
    ///
    /// The parts it staged are kept, so a failure while fetching resumes
    /// from where it got to rather than from nothing. One that failed after
    /// the fetch — a digest that did not match, a disk that filled while the
    /// parts were being joined — has no parts left and fetches again; the
    /// row's progress is reset so it does not sit at 100% while it does.
    pub async fn retry(&self, id: Id) -> bool {
        let mut slots = self.inner.slots.lock().await;
        let Some(slot) = slots.get_mut(&id) else {
            return false;
        };
        if !matches!(slot.entry.state, State::Failed(_)) {
            return false;
        }
        slot.requeue();
        slot.entry_mut().progress = Progress {
            downloaded: 0,
            total: slot.entry.progress.total,
            speed: 0.0,
            eta: None,
        };
        drop(slots);
        self.kick();
        self.save().await;
        true
    }

    /// Cancels the download for good.
    pub async fn cancel(&self, id: Id) {
        let mut slots = self.inner.slots.lock().await;
        let Some(slot) = slots.get_mut(&id) else {
            return;
        };
        if slot.entry.state.settled() {
            return;
        }
        slot.control.cancel();
        slot.entry_mut().state = State::Cancelled;
        slot.stopped_moving();
        slot.entry_mut().finished_at = Some(now_seconds());
        drop(slots);
        self.discard_parts(id).await;
        self.save().await;
    }

    /// Takes a row out of the map, the one way a row leaves it: stopped if
    /// it was not already, and a run still winding down kept in `leaving`,
    /// where it counts against the limit until it lets go. `remove` did
    /// this and `clear_finished` did not, so clearing a cancelled row whose
    /// run still held a stalled read let a limit of one run two.
    pub(super) fn take_slot(&self, slots: &mut HashMap<Id, Slot>, id: Id) -> Option<Slot> {
        let mut slot = slots.remove(&id)?;
        if !slot.entry.state.settled() {
            slot.control.cancel();
        }
        if let Some(task) = slot.task.take().filter(|task| !task.is_finished()) {
            crate::lock(&self.inner.leaving).push((slot.entry.queue.clone(), task));
        }
        Some(slot)
    }

    /// The finished downloads among `ids` as they stand, each with its place in
    /// the list, for [`Queue::put_back`]: taking one off the list loses
    /// nothing, since its file stays where it was saved.
    pub async fn finished_rows(&self, ids: &[Id]) -> Vec<(usize, Entry)> {
        let slots = self.inner.slots.lock().await;
        let order = self.inner.order.lock().await;
        order
            .iter()
            .enumerate()
            .filter(|(_, id)| ids.contains(id))
            .filter_map(|(at, id)| {
                let entry = &slots.get(id)?.entry;
                (entry.state == State::Completed).then(|| (at, Entry::clone(entry)))
            })
            .collect()
    }

    /// Puts rows taken off the list back, each where it was (or at the end
    /// of a shorter list). A row whose number is in use again is left out.
    pub async fn put_back(&self, rows: Vec<(usize, Entry)>) {
        if rows.is_empty() {
            return;
        }
        {
            let mut slots = self.inner.slots.lock().await;
            let mut order = self.inner.order.lock().await;
            for (at, entry) in rows {
                if slots.contains_key(&entry.id) {
                    continue;
                }
                let id = entry.id;
                let own_limit = crate::limit::Limiter::new(entry.speed_limit.saturating_mul(1024));
                slots.insert(
                    id,
                    Slot {
                        entry: Arc::new(entry),
                        control: Control::new(),
                        task: None,
                        outside_schedule: false,
                        retrying: None,
                        swarm: None,
                        own_limit,
                    },
                );
                let at = at.min(order.len());
                order.insert(at, id);
            }
        }
        self.save().await;
    }

    /// Forgets a download entirely, stopping it first if it was running.
    ///
    /// Not `cancel` and then forget: that discarded the parts and wrote the
    /// queue twice, once for a Cancelled state nobody would ever see.
    pub async fn remove(&self, id: Id) {
        self.remove_many(&[id]).await;
    }

    /// Forgets several downloads with one save of the queue. Removing a
    /// selection one `remove` at a time wrote the whole queue file once per
    /// row: 10,000 rows of a 6.5 MB file at 32 ms a save held the window
    /// for minutes.
    pub async fn remove_many(&self, ids: &[Id]) {
        let mut taken = Vec::new();
        {
            let mut slots = self.inner.slots.lock().await;
            for &id in ids {
                if let Some(slot) = self.take_slot(&mut slots, id) {
                    taken.push((id, slot));
                }
            }
        }
        if taken.is_empty() {
            return;
        }
        let gone: HashSet<Id> = taken.iter().map(|(id, _)| *id).collect();
        self.inner.order.lock().await.retain(|x| !gone.contains(x));
        // The folder the row recorded, read from the row just taken: looked
        // up afresh it is no longer there, and the setting's folder is not
        // where a row begun before a change staged its parts.
        let default_parts = self.inner.options.lock().await.parts_dir.clone();
        for (id, slot) in taken {
            // A finished torrent still shared stops with its row (gaps row 147).
            if !slot.entry.path.is_empty() {
                crate::torrent::stop_sharing(std::path::Path::new(&slot.entry.path));
            }
            let parts = if slot.entry.parts_dir.is_empty() {
                default_parts.join(id.to_string())
            } else {
                PathBuf::from(&slot.entry.parts_dir)
            };
            self.discard_parts_at(&parts).await;
        }
        self.save().await;
    }
}
