use super::*;

/// The least time between two writes of the queue file.
const SAVE_EVERY: Duration = Duration::from_millis(150);

impl Queue {
    /// Writes the queue to `path` from now on, and reads back what is there.
    ///
    /// A download manager that forgets its queue when it closes is one that
    /// loses a half-finished four gigabyte transfer to a reboot — the part
    /// files survive on disk, so losing the row that points at them is the
    /// only reason the bytes would be wasted.
    pub async fn persist_to(&self, path: PathBuf) {
        // Nothing is known of what is on disk there until it is written.
        *crate::lock(&self.inner.saved) = None;
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
        let secrets = self.read_secrets(path);
        let entries = match serde_json::from_str::<Vec<Entry>>(&text) {
            // With what changed since, where the journal was written over
            // this very file (`journal`).
            Ok(entries) => super::journal::replayed(path, text.as_bytes(), entries),
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
            if let Some(secret) = secrets.get(&entry.id) {
                secret.given_back(&mut entry);
            }
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
                    entry: Arc::new(entry),
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
            slot.entry_mut().id = highest;
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
    ///
    /// The first change after a quiet moment is written before this returns.
    /// Changes that follow within [`SAVE_EVERY`] of a write are folded into
    /// one more write, made when that time is up, so the cost of a long list
    /// is paid once per interval and not once per change: every state change
    /// used to rewrite and sync the whole list, which held 20 000 links to
    /// 13 small files a second and took a minute to cancel 2 000 of them.
    /// [`Queue::flush`] writes what is waiting at once, for a program that
    /// is about to end.
    pub(super) async fn save(&self) {
        let told = crate::lock(&self.inner.changed).clone();
        if let Some(tell) = told {
            tell();
        }
        let Some(path) = self.inner.store.lock().await.clone() else {
            return;
        };
        let due =
            crate::lock(&self.inner.written).is_none_or(|at| at.elapsed() >= self.save_every());
        if due {
            self.write_to(&path).await;
            return;
        }
        self.inner.unwritten.store(true, Ordering::SeqCst);
        if self.inner.trailing.swap(true, Ordering::SeqCst) {
            return;
        }
        let queue = self.clone();
        tokio::spawn(async move {
            loop {
                let wait = crate::lock(&queue.inner.written).map_or(Duration::ZERO, |at| {
                    queue.save_every().saturating_sub(at.elapsed())
                });
                tokio::time::sleep(wait).await;
                if crate::lock(&queue.inner.written)
                    .is_none_or(|at| at.elapsed() >= queue.save_every())
                {
                    break;
                }
            }
            queue.inner.trailing.store(false, Ordering::SeqCst);
            queue.flush().await;
        });
    }

    /// The least time between two writes: [`SAVE_EVERY`], or ten times what
    /// the last write took, so a long list spends a tenth of its time
    /// rewriting itself and the rest on the downloads.
    fn save_every(&self) -> Duration {
        let took = Duration::from_millis(self.inner.last_write_ms.load(Ordering::Relaxed));
        SAVE_EVERY.max(took * 10).min(Duration::from_secs(5))
    }

    /// Writes the queue now if a change is waiting for its turn, and returns
    /// when what was asked for is on disk (a write already under way is waited
    /// for).
    pub async fn flush(&self) {
        if self.inner.unwritten.load(Ordering::SeqCst) {
            if let Some(path) = self.inner.store.lock().await.clone() {
                let saving = self.inner.saving.lock().await;
                /* Asked again now that this is the one writing: two flushes
                 * (the one waiting out the write gap and one made by a
                 * caller) both saw a save owed, and each wrote the list, one
                 * after the other, for a single change. */
                if self.inner.unwritten.swap(false, Ordering::SeqCst) {
                    self.write_held(&path).await;
                }
                drop(saving);
                return;
            }
        }
        let _waited = self.inner.saving.lock().await;
    }

    /// One write of the whole list: serialized and synced off the runtime's
    /// threads (a long list takes a tenth of a second or more), one at a
    /// time so an older snapshot is never written after a newer one.
    async fn write_to(&self, path: &std::path::Path) {
        let _saving = self.inner.saving.lock().await;
        self.inner.unwritten.store(false, Ordering::SeqCst);
        self.write_held(path).await;
    }

    /// [`Self::write_to`] by whoever holds `saving` and has cleared what was
    /// owed.
    async fn write_held(&self, path: &std::path::Path) {
        self.inner.saves.fetch_add(1, Ordering::Relaxed);
        // Shared, not copied: what changed since the last write is the rows
        // that are not the same allocation as then (`journal`).
        let rows = self.list_shared().await;
        let previous = crate::lock(&self.inner.saved).take();
        let began = std::time::Instant::now();
        let target = path.to_path_buf();
        let wrote =
            tokio::task::spawn_blocking(move || super::journal::write(&target, rows, previous))
                .await;
        self.inner.last_write_ms.store(
            began.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        *crate::lock(&self.inner.written) = Some(std::time::Instant::now());
        // What is on disk now, for the next write to add to; after a failed
        // write nothing is known, and the next one writes the whole list.
        let wrote = wrote.map(|w| {
            w.map(|saved| {
                *crate::lock(&self.inner.saved) = Some(saved);
            })
        });
        let failed = match wrote {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(e) => Some(e.to_string()),
        };
        if let Some(e) = failed {
            self.note(format!(
                "the queue could not be written ({e}); changes since will not survive a restart"
            ));
        }
        self.save_quota(path);
    }

    /// The secrets saved beside the queue file, by row.
    fn read_secrets(&self, queue_file: &std::path::Path) -> HashMap<Id, Secret> {
        let kept = secrets_file(queue_file);
        let Ok(text) = std::fs::read_to_string(&kept) else {
            return HashMap::new();
        };
        match serde_json::from_str::<Vec<Secret>>(&text) {
            Ok(secrets) => secrets.into_iter().map(|s| (s.id, s)).collect(),
            Err(e) => {
                self.note(format!(
                    "the saved logins and cookies could not be read ({e}): the downloads \
                     that had them will ask for them again"
                ));
                HashMap::new()
            }
        }
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

/// What a row carries that is a secret: the login written into its address
/// and the cookies the browser handed over. Kept out of the queue file, in
/// `secrets.json` beside it (readable by this account alone, as the queue
/// file is), so a queue file shown, sent with a report or backed up on its
/// own carries no password and no session.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Secret {
    pub(super) id: Id,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    cookies: String,
}

impl Secret {
    /// Takes the secrets out of `entries`: each address is left without its
    /// login and each row without its cookies. Rows with neither give none.
    pub(super) fn taken_from(entries: &mut [Entry]) -> Vec<Secret> {
        entries
            .iter_mut()
            .filter_map(|entry| {
                let mut secret = Secret {
                    id: entry.id,
                    cookies: std::mem::take(&mut entry.cookies),
                    ..Secret::default()
                };
                if let Ok(mut url) = url::Url::parse(&entry.url) {
                    if !url.username().is_empty() || url.password().is_some() {
                        secret.username = url.username().to_string();
                        secret.password = url.password().map(str::to_string);
                        let _ = url.set_username("");
                        let _ = url.set_password(None);
                        entry.url = url.to_string();
                    }
                }
                let any = !secret.username.is_empty()
                    || secret.password.is_some()
                    || !secret.cookies.is_empty();
                any.then_some(secret)
            })
            .collect()
    }

    /// Puts this row's secrets back where they were.
    fn given_back(&self, entry: &mut Entry) {
        if !self.cookies.is_empty() {
            entry.cookies = self.cookies.clone();
        }
        if !self.username.is_empty() || self.password.is_some() {
            if let Ok(mut url) = url::Url::parse(&entry.url) {
                let _ = url.set_username(&self.username);
                let _ = url.set_password(self.password.as_deref());
                entry.url = url.to_string();
            }
        }
    }
}

/// Where the secrets of the queue at `queue_file` are kept.
pub(super) fn secrets_file(queue_file: &std::path::Path) -> PathBuf {
    queue_file.with_file_name("secrets.json")
}
