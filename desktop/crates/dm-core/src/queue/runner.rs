use super::*;

impl Queue {
    /// Whether anything is still running or waiting to.
    pub async fn busy(&self) -> bool {
        self.inner
            .slots
            .lock()
            .await
            .values()
            .any(|s| matches!(s.entry.state, State::Queued | State::Running))
    }

    /// Serves the queue until nothing is left running or waiting.
    ///
    /// Polled rather than event-driven: the states that make a download
    /// eligible change from several places — a pause here, a finished
    /// download there — and a scheduler that has to be woken by each of them
    /// is a scheduler with a path that forgets to.
    /// One at a time. A scheduler is started on every add and by whatever
    /// watches for work arriving from a browser, so without this they pile
    /// up — each looping until the queue drains, all doing the same job.
    ///
    /// A caller that finds one already running returns immediately rather
    /// than waiting: the scheduler in progress will pick up whatever was just
    /// added on its next tick.
    pub async fn run(&self) {
        if self.inner.scheduling.swap(true, Ordering::SeqCst) {
            return;
        }
        loop {
            self.tick().await;
            if !self.busy().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.inner.scheduling.store(false, Ordering::SeqCst);
    }

    /// Starts whatever may start.
    ///
    /// Claiming and marking happen under one lock. Releasing it between
    /// "these are startable" and "these are now running" lets a second
    /// scheduler see the same downloads as still queued and start them
    /// again — and the window spawns a scheduler on every add, so overlapping
    /// ticks are the normal case rather than a rare one. That would not
    /// merely oversell the limit; it would put two writers on one set of
    /// part files.
    pub(super) async fn tick(&self) {
        // Outside the window, nothing new starts and what is running is put
        // down. Pausing rather than letting it run to the end is the point:
        // the reason to set a window is a link that must be free outside it.
        // The same for a download limit whose allowance is spent: held until
        // the period rolls over, then put back as the schedule's are.
        if !self.window_open() || self.allowance_spent().is_some() {
            self.hold_for_schedule().await;
            return;
        }
        self.release_from_schedule().await;

        let claimed: Vec<(Id, String, Control)> = {
            let mut slots = self.inner.slots.lock().await;
            let order = self.inner.order.lock().await;

            // A row paused or cancelled whose task has not yet let go still
            // holds its connections; counting only `Running` let a limit of
            // one run two for as long as a read timeout. So does a deleted
            // row's, which no slot is left to count.
            let leaving: Vec<String> = {
                let mut leaving = crate::lock(&self.inner.leaving);
                leaving.retain(|(_, task)| !task.is_finished());
                leaving.iter().map(|(queue, _)| queue.clone()).collect()
            };
            let running = slots
                .values()
                .filter(|s| s.entry.state == State::Running || s.still_stopping())
                .count()
                + leaving.len();
            /* A row waiting out a failure between attempts lends its place
             * to the next in line: a server answering 500 held one for all
             * ten attempts while the queue stood still behind it.
             * Lent, not given: with the retrying rows, no more than
             * twice the setting run, so a dead server cannot pull the whole
             * queue in behind it. */
            let retrying = slots
                .values()
                .filter(|s| s.entry.state == State::Running && s.retrying.is_some())
                .count();
            let concurrency = self.inner.concurrency.load(Ordering::Relaxed);
            let room = concurrency
                .saturating_sub(running.saturating_sub(retrying))
                .min(concurrency.saturating_mul(2).saturating_sub(running));

            // A named queue with a limit of its own fills only up to it,
            // counting what it already has running; the rest of the room
            // goes to the next queued row of another queue.
            let limits = crate::lock(&self.inner.queue_limits).clone();
            let mut per_queue: HashMap<String, usize> = HashMap::new();
            for queue in leaving {
                *per_queue.entry(queue).or_insert(0) += 1;
            }
            for s in slots.values() {
                if s.entry.state == State::Running || s.still_stopping() {
                    *per_queue.entry(s.entry.queue.clone()).or_insert(0) += 1;
                }
            }
            let picked: Vec<Id> = order
                .iter()
                .filter(|id| {
                    slots
                        .get(id)
                        .is_some_and(|s| s.entry.state == State::Queued && !s.still_stopping())
                })
                .filter(|id| {
                    let Some(s) = slots.get(id) else {
                        return false;
                    };
                    let Some(&cap) = limits.get(&s.entry.queue) else {
                        return true;
                    };
                    let running = per_queue.entry(s.entry.queue.clone()).or_insert(0);
                    if *running >= cap {
                        return false;
                    }
                    *running += 1;
                    true
                })
                .take(room)
                .copied()
                .collect();

            picked
                .into_iter()
                .filter_map(|id| {
                    let slot = slots.get_mut(&id)?;
                    slot.entry.state = State::Running;
                    slot.entry.paused_by_schedule = false;
                    Some((id, slot.entry.url.clone(), slot.control.clone()))
                })
                .collect()
        };

        if claimed.is_empty() {
            return;
        }
        for (id, url, control) in claimed {
            let task = self.start(id, url, control);
            if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
                slot.task = Some(task);
            }
        }
        self.save().await;
    }

    /// Stops everything the schedule is not letting run.
    ///
    /// Paused rather than cancelled, so the bytes already staged are kept
    /// and the next window resumes from them.
    pub(super) async fn hold_for_schedule(&self) {
        let held = {
            let mut slots = self.inner.slots.lock().await;
            let mut held = false;
            for slot in slots.values_mut() {
                // One that already has every byte is only putting its file
                // together: held there, it was paused at 100 % and left for
                // the next window, for bytes it no longer needed to fetch.
                let fetched = slot
                    .entry
                    .progress
                    .total
                    .is_some_and(|total| slot.entry.progress.downloaded >= total);
                if matches!(slot.entry.state, State::Running | State::Queued)
                    && !fetched
                    && !slot.outside_schedule
                {
                    slot.hold(true);
                    held = true;
                }
            }
            held
        };
        if held {
            self.save().await;
        }
    }

    /// Puts back whatever the schedule stopped, and only that.
    ///
    /// A download somebody paused on purpose stays paused: obeying a
    /// schedule should not quietly undo a decision a person made.
    pub(super) async fn release_from_schedule(&self) {
        let released = {
            let mut slots = self.inner.slots.lock().await;
            let mut released = false;
            for slot in slots.values_mut() {
                if slot.entry.paused_by_schedule && slot.entry.state == State::Paused {
                    slot.requeue();
                    released = true;
                }
            }
            released
        };
        if released {
            self.save().await;
        }
    }

    /// Runs one download that `tick` has already claimed.
    ///
    /// Takes what it needs rather than looking it up again: the claim
    /// happened under the lock, and re-reading here would reopen the gap it
    /// closed.
    pub(super) fn start(
        &self,
        id: Id,
        url: String,
        control: Control,
    ) -> tokio::task::JoinHandle<()> {
        let inner = Arc::clone(&self.inner);
        let queue = self.clone();

        tokio::spawn(async move {
            let mut options = inner.options.lock().await.clone();
            let ran_with = control.clone();
            options.control = control;
            /* Handed to the engine rather than checked here, so one place
             * decides whether a file is the file: a digest typed by a person
             * and a digest read out of a Metalink must not be checked by
             * different code. */
            // Each download stages into its own directory, or two of them
            // would overwrite each other's part files: the one it was first
            // started under, recorded on the row, so a parts folder changed
            // in the settings since does not strand what it has.
            let own_parts = options.parts_dir.join(id.to_string());
            // Its own folder even if the row is gone before it starts: the
            // shared parts folder is never anybody's staging.
            options.parts_dir = own_parts.clone();
            {
                let mut slots = inner.slots.lock().await;
                if let Some(slot) = slots.get_mut(&id) {
                    if slot.entry.parts_dir.is_empty() {
                        slot.entry.parts_dir = own_parts.display().to_string();
                    }
                    options.parts_dir = PathBuf::from(&slot.entry.parts_dir);
                    if slot.entry.began_in.is_empty() {
                        slot.entry.began_in = options.dest_dir.display().to_string();
                    }
                    options.dest_dir = PathBuf::from(&slot.entry.began_in);
                    // The browser's cookies for this site, where handed
                    // over: this download's alone, for its own server.
                    options.sign_in =
                        crate::cookies::SignIn::new(&slot.entry.url, &slot.entry.cookies);
                    options.checksum = slot.entry.checksum.clone();
                    options.save_name = slot.entry.save_name.clone();
                    // Its own limit as well as the one every download shares.
                    options.limiter = options.limiter.chained(&slot.own_limit);
                    // Sent as Referer where the row carries one, which is the
                    // downloads a browser handed over.
                    options.referrer =
                        (!slot.entry.referrer.is_empty()).then(|| slot.entry.referrer.clone());
                    // For categories a person limited to sites.
                    options.categories.site = crate::category::host_of(&slot.entry.url);
                    // The quality picked in the browser's panel, over the setting.
                    if let Some(quality) = &slot.entry.quality {
                        options.quality = crate::stream::Quality::named(quality);
                        options.container = crate::stream::Quality::container_of(quality);
                        // Media was asked for, from the panel or the media
                        // menu: never the page's own HTML instead.
                        options.media = true;
                    }
                    // A folder a person chose is where it goes, unsorted:
                    // they picked the place, not a category.
                    if let Some(dir) = slot.entry.save_dir.as_ref().filter(|d| !d.is_empty()) {
                        options.dest_dir = PathBuf::from(dir);
                        options.categories.enabled = false;
                    }
                }
            }

            // What an earlier run already staged, which the download limit
            // counted then. Read off the disk rather than taken as the run's
            // first report: a small file can be whole by its first report,
            // and then nothing it fetched was counted at all.
            let staged = staged_bytes(&options.parts_dir).await;
            let (tx, mut rx) = tokio::sync::mpsc::channel(32);
            let watcher = {
                let queue = queue.clone();
                tokio::spawn(async move {
                    // What this run has reported so far, from what it found.
                    let mut reported: Option<u64> = staged;
                    while let Some(update) = rx.recv().await {
                        let mut slots = queue.inner.slots.lock().await;
                        let Some(slot) = slots.get_mut(&id) else {
                            continue;
                        };
                        match update {
                            // Named once, as soon as the probe knows, so the
                            // row stops showing a URL people cannot read.
                            crate::download::Update::Named { name, category } => {
                                slot.entry.filename = name;
                                slot.entry.category = Some(category);
                            }
                            crate::download::Update::Progress(progress) => {
                                // What arrived since the last sample counts
                                // against the download limit.
                                let arrived = reported
                                    .map_or(0, |before| progress.downloaded.saturating_sub(before));
                                // Bytes again: the retry is over.
                                if progress.downloaded > slot.entry.progress.downloaded {
                                    slot.retrying = None;
                                }
                                reported = Some(progress.downloaded);
                                crate::lock(&queue.inner.quota)
                                    .add(arrived, std::time::SystemTime::now());
                                slot.entry.progress = progress;
                                drop(slots);
                                queue.save_quota_now_and_then().await;
                                continue;
                            }
                            crate::download::Update::Validator(validator) => {
                                slot.entry.validator = Some(validator);
                            }
                            // The highest attempt of any range is the row's.
                            crate::download::Update::Retrying { attempt, of, why } => {
                                let now = slot.retrying.as_ref().map_or(0, |(a, _, _)| *a);
                                if attempt >= now {
                                    slot.retrying = Some((attempt, of, why));
                                }
                            }
                        }
                    }
                    reported
                })
            };

            let client = inner.client.lock().await.clone();
            let outcome = download_reporting(&client, &url, &options, Some(tx)).await;
            let reported = watcher.await.ok().flatten();
            /* Reports are samples: a file whole before the next one was
             * sent had bytes no report carried, and a small download could
             * go uncounted by the download limit altogether. What a
             * finished file holds past the last report counts now. */
            if let (Ok(finished), Some(reported)) = (&outcome, reported) {
                let unreported = finished.bytes.saturating_sub(reported);
                if unreported > 0 {
                    crate::lock(&inner.quota).add(unreported, std::time::SystemTime::now());
                }
            }

            let mut slots = inner.slots.lock().await;
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
                return;
            };

            /* Cancelled while it ran: the row already says so and its parts
             * are gone. Whatever the task made of that — a join that failed
             * on the missing parts, or a file it had finished a moment
             * before — must not turn the row back into Failed or Completed,
             * and a file it did produce is not one anybody asked to keep. */
            if slot.entry.state == State::Cancelled {
                if let Ok(finished) = outcome {
                    if let Err(e) = tokio::fs::remove_file(&finished.path).await {
                        queue.note(format!(
                            "a cancelled download finished first and its file could not be \
                             removed from {}: {e}",
                            finished.path.display()
                        ));
                    }
                }
                // The cancel removed the parts while this run still held
                // some open — a helper's, such as yt-dlp's — and on Windows an
                // open file cannot be deleted. Now the run has let go,
                // whatever is left goes.
                let leftover = PathBuf::from(&slot.entry.parts_dir);
                drop(slots);
                if !leftover.as_os_str().is_empty() {
                    let _ = tokio::fs::remove_dir_all(&leftover).await;
                }
                return;
            }

            /* Stopped and already put back in line: the row carries a fresh
             * control, and this task's word on how its run ended — Paused,
             * usually, seen at the read timeout — is about a run the row has
             * moved past. Writing it would turn a Queued row back into a
             * Paused one that nothing restarts. A run that finished is still
             * finished, whoever asked for it. */
            if !slot.control.is(&ran_with) && outcome.is_err() {
                return;
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
            let outcome = match (outcome, wanted) {
                (Ok(mut finished), Some(name)) => {
                    match rename_free(&finished.path, &name).await {
                        Ok(path) => finished.path = path,
                        Err(e) => queue.note(format!(
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
            let outcome = match (outcome, &replaces) {
                (Ok(mut finished), Some(old)) => {
                    let old = PathBuf::from(old);
                    if finished.path != old {
                        match tokio::fs::rename(&finished.path, &old).await {
                            Ok(()) => finished.path = old,
                            Err(e) => queue.note(format!(
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
                queue.note(note.clone());
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
            let mut slots = inner.slots.lock().await;
            let Some(slot) = slots.get_mut(&id) else {
                return;
            };

            slot.retrying = None;
            slot.entry.stalled.clear();
            // Out of room, or the folder's drive gone: paused, its parts
            // kept, rather than failed, to go on once there is room or
            // from another folder.
            let outcome = match outcome {
                Err(error) if replaces.is_none() => {
                    match out_of_room(&error, [&options.parts_dir, &options.dest_dir]) {
                        Some(why) => {
                            slot.entry.stalled = why;
                            Err(Error::Paused)
                        }
                        None => Err(error),
                    }
                }
                other => other,
            };
            slot.entry.state = match outcome {
                Ok(finished) => {
                    // Usually the name the probe already reported, but
                    // assembly has the last word: it is what avoided a
                    // collision with a file already on disk.
                    if let Some(name) = finished.path.file_name() {
                        slot.entry.filename = name.to_string_lossy().into_owned();
                    }
                    slot.entry.path = finished.path.to_string_lossy().into_owned();
                    // From the internet, as a browser's download says.
                    crate::motw::mark(&finished.path, &slot.entry.url, &slot.entry.referrer);
                    slot.entry.category = Some(finished.category);
                    slot.entry.verified = finished.verified;
                    slot.entry.mirrors = finished.mirrors;
                    slot.entry.replaces = None;
                    if std::mem::take(&mut slot.entry.cookies_once) {
                        slot.entry.cookies.clear();
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
                Err(error) => match &replaces {
                    Some(old) => State::Failed(format!(
                        "the new version could not be fetched, so the old one is kept at {old}: \
                         {error}"
                    )),
                    None => State::Failed(error.to_string()),
                },
            };
            // What turns a settled row into history.
            if slot.entry.state.settled() {
                slot.entry.finished_at = Some(now_seconds());
            }
            // Dropped before saving: save() reads the queue back through
            // this same lock, so holding it here would deadlock the task
            // that just finished a download.
            drop(slots);
            queue.save().await;
        })
    }
}
