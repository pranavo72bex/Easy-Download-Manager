use super::*;

impl Queue {
    /// Writes the queue to `path` from now on, and reads back what is there.
    ///
    /// A download manager that forgets its queue when it closes is one that
    /// loses a half-finished four gigabyte transfer to a reboot — the part
    /// files survive on disk, so losing the row that points at them is the
    /// only reason the bytes would be wasted.
    pub async fn persist_to(&self, path: PathBuf) {
        if let Err(problem) = self.restore(&path).await {
            self.note(problem);
        }
        *self.inner.store.lock().await = Some(path);
        // What the download limit had used, from before this launch.
        if let Some(store) = self.inner.store.lock().await.clone() {
            if let Ok(text) = std::fs::read_to_string(store.with_file_name("quota")) {
                crate::lock(&self.inner.quota).restore(&text);
            }
        }
        // When the recheck queue last ran, from before this launch.
        if let Some(at) = self.recheck_file().await {
            if let Some(last) = std::fs::read_to_string(&at)
                .ok()
                .and_then(|t| t.trim().parse::<u64>().ok())
            {
                crate::lock(&self.inner.recheck).1 = last;
            }
        }
        self.save().await;
    }

    /// Reads a stored queue, if one is readable.
    ///
    /// A queue file that cannot be parsed is not fatal — the app opening
    /// with an empty list is recoverable, and refusing to start over a bad
    /// file is not — but it is not overwritten either. The first save used
    /// to write the empty list over it, so a release that changed a field
    /// destroyed every queue on first launch, and the parts those rows
    /// pointed at were orphaned for good. It is moved aside and said.
    pub(super) async fn restore(&self, path: &std::path::Path) -> Result<(), String> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(());
        };
        let entries = match serde_json::from_str::<Vec<Entry>>(&text) {
            Ok(entries) => entries,
            Err(e) => {
                let aside = path.with_extension("json.unreadable");
                return match std::fs::rename(path, &aside) {
                    Ok(()) => Err(format!(
                        "the saved queue could not be read ({e}); it was kept as {} and the \
                         queue starts empty",
                        aside.display()
                    )),
                    Err(io) => Err(format!(
                        "the saved queue could not be read ({e}) or moved aside ({io}); it \
                         will be overwritten by the next change"
                    )),
                };
            }
        };

        // Read before the rows are locked, in the order `start` takes them.
        let parts_base = self.inner.options.lock().await.parts_dir.clone();
        let mut slots = self.inner.slots.lock().await;
        let mut order = self.inner.order.lock().await;
        let mut highest = 0;

        // Rows added before the store was read — a browser hand-off on a
        // cold start — were numbered from 1 too, and inserting the restored
        // rows wrote over them. Restored rows keep their ids, because their
        // part files are named by them; the early arrivals have none yet and
        // are renumbered after them.
        let early: Vec<Slot> = order.drain(..).filter_map(|id| slots.remove(&id)).collect();

        for mut entry in entries {
            highest = highest.max(entry.id);
            // Anything that was running when the app closed is put back in
            // line rather than left claiming to run. It resumes from its
            // part files, so this costs only what was in flight.
            if entry.state == State::Running {
                entry.state = State::Queued;
            }
            // Nothing restored is moving yet; a rate and a time remaining
            // from before the restart are figures about a transfer that is
            // not happening.
            entry.progress.speed = 0.0;
            entry.progress.eta = None;
            // A row staged by a dm from before rows recorded their folder:
            // its parts are under the folder in force now, and are recorded
            // there, so a later change of the setting does not strand them.
            if entry.parts_dir.is_empty() {
                let staged = parts_base.join(entry.id.to_string());
                if staged.is_dir() {
                    entry.parts_dir = staged.display().to_string();
                }
            }
            order.push(entry.id);
            let own_limit = crate::limit::Limiter::new(entry.speed_limit.saturating_mul(1024));
            slots.insert(
                entry.id,
                Slot {
                    entry,
                    control: Control::new(),
                    task: None,
                    outside_schedule: false,
                    retrying: None,
                    own_limit,
                },
            );
        }

        for mut slot in early {
            highest += 1;
            slot.entry.id = highest;
            order.push(highest);
            slots.insert(highest, slot);
        }

        // Ids must not be reissued over restored ones.
        self.inner.next_id.store(highest + 1, Ordering::SeqCst);
        Ok(())
    }

    /// Writes the queue out, if it is being persisted.
    ///
    /// Called on state changes rather than on progress: a byte count moves
    /// thousands of times a download and says nothing a restart needs.
    pub(super) async fn save(&self) {
        let told = crate::lock(&self.inner.changed).clone();
        if let Some(tell) = told {
            tell();
        }
        let Some(path) = self.inner.store.lock().await.clone() else {
            return;
        };
        let _saving = self.inner.saving.lock().await;
        self.inner.saves.fetch_add(1, Ordering::Relaxed);
        let entries = self.list().await;

        let write = || -> std::io::Result<()> {
            crate::write_atomic(&path, serde_json::to_string_pretty(&entries)?.as_bytes())
        };
        if let Err(e) = write() {
            self.note(format!(
                "the queue could not be written ({e}); changes since will not survive a restart"
            ));
        }
        self.save_quota(&path);
    }

    /// Writes what the download limit has used beside the queue file, so a
    /// restart does not hand the allowance back.
    pub(super) fn save_quota(&self, queue_file: &std::path::Path) {
        let kept = crate::lock(&self.inner.quota).kept();
        *crate::lock(&self.inner.quota_saved) = Some(std::time::Instant::now());
        let _ = crate::write_atomic_unsynced(&queue_file.with_file_name("quota"), kept.as_bytes());
    }

    /// The same, at most every ten seconds, from the progress of a transfer.
    pub(super) async fn save_quota_now_and_then(&self) {
        let due = crate::lock(&self.inner.quota_saved)
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(10));
        if !due {
            return;
        }
        if let Some(path) = self.inner.store.lock().await.clone() {
            self.save_quota(&path);
        }
    }

    /// Records something that went wrong and belongs to no row.
    pub(super) fn note(&self, problem: String) {
        {
            let mut problems = crate::lock(&self.inner.problems);
            // The same failure on every save is one problem, not a hundred.
            if problems.last() == Some(&problem) {
                return;
            }
            problems.push(problem);
        }
        // Shown now, not at the window's next look.
        let told = crate::lock(&self.inner.changed).clone();
        if let Some(tell) = told {
            tell();
        }
    }

    /// What has gone wrong since anyone last asked, and then nothing until
    /// something else does.
    pub fn problems(&self) -> Vec<String> {
        std::mem::take(&mut *crate::lock(&self.inner.problems))
    }

    /// Throws away the part files one download staged.
    ///
    /// Cancelling or removing is the only moment anyone says those bytes are
    /// unwanted. Without this they sit under the app's data directory for
    /// ever, and a cancelled four gigabyte download costs four gigabytes
    /// indefinitely.
    pub(super) async fn discard_parts(&self, id: Id) {
        let parts = self.parts_dir_of(id).await;
        self.discard_parts_at(&parts).await;
    }

    /// Removes a row's staging folder, where it is known.
    pub(super) async fn discard_parts_at(&self, parts: &std::path::Path) {
        // tokio's: a cancelled multi-gigabyte download unlinks a lot of
        // files, and doing it inline holds a worker that has downloads to run.
        if let Err(e) = tokio::fs::remove_dir_all(&parts).await {
            // Nothing staged is the usual case, not a problem.
            if e.kind() != std::io::ErrorKind::NotFound {
                self.note(format!(
                    "the staged parts in {} could not be removed: {e}",
                    parts.display()
                ));
            }
        }
    }

    /// How many times the queue file has been written since this queue was
    /// made: what a test holds a batch operation to.
    pub fn saves_written(&self) -> u64 {
        self.inner.saves.load(Ordering::Relaxed)
    }
}
