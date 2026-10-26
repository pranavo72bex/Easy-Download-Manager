use super::*;

impl Queue {
    /// Starts every row waiting in a named queue, and says how many.
    pub async fn start_queue(&self, queue: &str) -> usize {
        let mut started = 0;
        {
            let mut slots = self.inner.slots.lock().await;
            for slot in slots.values_mut() {
                if slot.entry.queue == queue && slot.entry.state == State::Paused {
                    slot.requeue();
                    started += 1;
                }
            }
        }
        if started > 0 {
            self.kick();
            self.save().await;
        }
        started
    }

    /// Stops every row of a named queue that is running or waiting.
    pub async fn stop_queue(&self, queue: &str) -> usize {
        let mut stopped = 0;
        {
            let mut slots = self.inner.slots.lock().await;
            for slot in slots.values_mut() {
                if slot.entry.queue != queue {
                    continue;
                }
                if matches!(slot.entry.state, State::Queued | State::Running) {
                    slot.hold(false);
                    stopped += 1;
                } else if slot.entry.state == State::Paused
                    && std::mem::take(&mut slot.entry.paused_by_schedule)
                {
                    // Held by the schedule, now stopped by a person: not
                    // started again when the window opens.
                    stopped += 1;
                }
            }
        }
        if stopped > 0 {
            self.save().await;
        }
        stopped
    }

    /// "Move/Rename" for a finished file: renamed where it lies, never
    /// over another file.
    pub async fn rename(&self, id: Id, name: &str) -> Result<(), String> {
        let path = match self.get(id).await {
            Some(entry) if entry.state == State::Completed && !entry.path.is_empty() => {
                PathBuf::from(entry.path)
            }
            Some(_) => return Err("only a finished download can be renamed".into()),
            None => return Err("there is no such download".into()),
        };
        let to = rename_free(&path, name).await.map_err(|e| e.to_string())?;
        self.edit(id, |slot| {
            slot.entry.filename = to
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            slot.entry.path = to.to_string_lossy().into_owned();
            true
        })
        .await;
        Ok(())
    }

    /// Moves a finished download's file into `dir`, under the same name or
    /// a numbered one where that is taken, across disks too, and keeps the
    /// row's path to where it now is.
    pub async fn move_to(&self, id: Id, dir: &std::path::Path) -> Result<PathBuf, String> {
        let from = match self.get(id).await {
            Some(entry) if entry.state == State::Completed && !entry.path.is_empty() => {
                PathBuf::from(entry.path)
            }
            Some(_) => return Err("only a finished download can be moved".into()),
            None => return Err("there is no such download".into()),
        };
        if from.parent() == Some(dir) {
            return Ok(from);
        }
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
        let name = from
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Claimed first, as a rename claims its name, then moved over.
        let to = crate::download::claim_path(dir, &name)
            .await
            .map_err(|e| e.to_string())?;
        if tokio::fs::rename(&from, &to).await.is_err() {
            // Another disk: copied, then the original removed once whole.
            if let Err(e) = tokio::fs::copy(&from, &to).await {
                let _ = tokio::fs::remove_file(&to).await;
                return Err(format!("{} could not be moved: {e}", from.display()));
            }
            let _ = tokio::fs::remove_file(&from).await;
        }
        self.edit(id, |slot| {
            slot.entry.filename = to
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            slot.entry.path = to.to_string_lossy().into_owned();
            true
        })
        .await;
        Ok(to)
    }

    /// The names of the queues rows are in, in the order first met.
    pub async fn queues(&self) -> Vec<String> {
        let mut names = vec![MAIN_QUEUE.to_string(), RECHECK_QUEUE.to_string()];
        for entry in self.list().await {
            if !entry.queue.is_empty() && !names.contains(&entry.queue) {
                names.push(entry.queue);
            }
        }
        names
    }
}
