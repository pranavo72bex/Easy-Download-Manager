// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

mod finish;

use super::*;

/// How often the scheduler looks at the queue when nothing has woken it.
const LOOK_EVERY: Duration = Duration::from_secs(1);

impl DownloadList {
    /// Wakes a scheduler that is waiting for its next tick.
    pub(super) fn kick(&self) {
        self.inner.kick.notify_one();
    }

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
            // Woken by what changes the queue (`kick`: an add, a download
            // ending, a resume, an edit), and looked at once a second besides
            // for what nothing announces: a schedule's window opening, a
            // retrying row lending its place. Looked at every 50 ms, a queue
            // of 10,000 rows was passed over twenty times a second while one
            // download ran (gaps row S10).
            tokio::select! {
                _ = tokio::time::sleep(LOOK_EVERY) => {}
                _ = self.inner.kick.notified() => {}
            }
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
        // dm is ending: what runs is being put down, and nothing starts.
        if self.inner.closing.load(Ordering::SeqCst) {
            return;
        }
        // Outside the window, nothing new starts and what is running is put
        // down. Pausing rather than letting it run to the end is the point:
        // the reason to set a window is a link that must be free outside it.
        // The same for a download limit whose allowance is spent: held until
        // the period rolls over, then put back as the schedule's are.
        let spent = self.allowance_spent().is_some();
        // Every queue on the one schedule: closed, nothing more to look at. With queues on
        // schedules of their own each row is held or put back by its own queue's window.
        if spent || (!self.has_queue_schedules() && !self.window_open()) {
            self.hold_for_schedule().await;
            return;
        }
        if self.has_queue_schedules() {
            self.hold_for_schedule().await;
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
                    slot.entry_mut().state = State::Running;
                    slot.entry_mut().paused_by_schedule = false;
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
        // All of it where the one schedule (or the download limit) says stop; only the rows of
        // queues whose own window is closed where queues have schedules of their own.
        let all = self.allowance_spent().is_some() || !self.has_queue_schedules();
        let held = {
            let mut slots = self.inner.slots.lock().await;
            let mut held = false;
            for slot in slots.values_mut() {
                // One that already has every byte is only putting its file
                // together: held there, it would be paused at 100 % and left
                // for the next window, for bytes it does not need to fetch.
                let fetched = slot
                    .entry
                    .progress
                    .total
                    .is_some_and(|total| slot.entry.progress.downloaded >= total);
                if matches!(slot.entry.state, State::Running | State::Queued)
                    && !fetched
                    && !slot.outside_schedule
                    && (all || !self.window_open_for(&slot.entry.queue))
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
                if slot.entry.paused_by_schedule
                    && slot.entry.state == State::Paused
                    && self.window_open_for(&slot.entry.queue)
                {
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
            let mut options = crate::lock(&inner.options).clone();
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
            // A form's answer whose form is gone (dm was closed since).
            let mut form_lost = false;
            let own_parts = options.parts_dir.join(id.to_string());
            // Its own folder even if the row is gone before it starts: the
            // shared parts folder is never anybody's staging.
            options.parts_dir = own_parts.clone();
            {
                let mut slots = inner.slots.lock().await;
                if let Some(slot) = slots.get_mut(&id) {
                    form_lost = options_from_row(slot, &mut options, &own_parts);
                }
            }

            // What an earlier run already staged, which the download limit
            // counted then. Read off the disk rather than taken as the run's
            // first report: a small file can be whole by its first report,
            // and then nothing it fetched was counted at all.
            let staged = staged_bytes(&options.parts_dir).await;
            let (tx, rx) = tokio::sync::mpsc::channel(32);
            let watcher = tokio::spawn(watch_run(queue.clone(), id, ran_with.clone(), staged, rx));

            let client = crate::lock(&inner.client).clone();
            // A dial-up or VPN connection the person chose is up before the first request, and
            // the download waits for it: never sent around it (`connect_first.rs`).
            let connected = if form_lost {
                Ok(())
            } else {
                let told = tx.clone();
                crate::connect_first::before_download(&options.control, move |redial| {
                    let _ = told.try_send(crate::download::Update::Retrying {
                        attempt: redial.attempt,
                        of: redial.of,
                        why: redial.why,
                        wait: redial.wait,
                    });
                })
                .await
            };
            let outcome = if form_lost {
                drop(tx);
                Err(crate::Error::Internal(
                    "this download is a form's answer, and the form is gone since dm was \
                     closed: start it again from the page it came from"
                        .into(),
                ))
            } else if let Err(not_connected) = connected {
                drop(tx);
                Err(not_connected)
            } else {
                download_reporting(&client, &url, &options, Some(tx)).await
            };
            let reported = watcher.await.ok().flatten();
            if !queue
                .finish_run(id, &url, &options, &ran_with, outcome, reported)
                .await
            {
                return;
            }
            // This download's place is free: the next in line starts now,
            // not at the next tick. A small file is over in a few
            // milliseconds, and its place sat empty for the rest of the
            // fifty, which was all that held a queue of them to about 130
            // files a second however fast they were.
            //
            // The save is owed before the scheduler is woken: it may find
            // nothing left to run and return, and a `flush` right after
            // would otherwise see nothing waiting and write nothing, the
            // last state lost.
            queue.inner.unwritten.store(true, Ordering::SeqCst);
            queue.kick();
            queue.save().await;
        })
    }
}

/// Fills a run's options from its row: where it stages and lands, its cookies and form,
/// its digest, files, name, limit, referrer and media choices. Whether the row is a form's
/// answer whose form is gone (dm was closed since).
fn options_from_row(slot: &mut Slot, options: &mut Options, own_parts: &std::path::Path) -> bool {
    let mut form_lost = false;
    if slot.entry.parts_dir.is_empty() {
        slot.entry_mut().parts_dir = own_parts.display().to_string();
    }
    options.parts_dir = PathBuf::from(&slot.entry.parts_dir);
    if slot.entry.began_in.is_empty() {
        slot.entry_mut().began_in = options.dest_dir.display().to_string();
    }
    options.dest_dir = PathBuf::from(&slot.entry.began_in);
    // The browser's cookies for this site, where handed
    // over: this download's alone, for its own server.
    options.sign_in = crate::cookies::SignIn::with_headers(
        &slot.entry.url,
        &slot.entry.cookies,
        &slot.entry.headers,
    );
    // A form's answer goes with its form, sent again.
    if let Some(post) = slot.entry.post.clone() {
        options.sign_in = crate::cookies::SignIn::posting(options.sign_in.take(), post);
    } else if slot.entry.posted {
        form_lost = true;
    }
    options.checksum = slot.entry.checksum.clone();
    /* Whether a checksum file found beside the file passed is said by the run
     * that checked it (`Update::Note`): a run fetching the file again, which may
     * check a typed digest instead or fail its check, does not carry an earlier
     * run's word. */
    if slot.entry.said == crate::download::CHECKED_BESIDE {
        slot.entry_mut().said.clear();
    }
    options.torrent.only = crate::torrent::only_files(&slot.entry.torrent_files);
    // Files of different priorities: the first tier asked for alone.
    let tiers = crate::torrent::tiers(&slot.entry.torrent_files);
    if tiers.len() > 1 {
        options.torrent.only = tiers.first().cloned();
        options.torrent.tiers = tiers;
    } else {
        options.torrent.tiers = Vec::new();
    }
    options.save_name = slot.entry.save_name.clone();
    options.title_hint = slot.entry.title_hint.clone();
    options.accept_page = slot.entry.accept_page;
    // Its own limit as well as the one every download shares.
    options.limiter = options.limiter.chained(&slot.own_limit);
    // Sent as Referer where the row carries one, which is the
    // downloads a browser handed over.
    options.referrer = (!slot.entry.referrer.is_empty()).then(|| slot.entry.referrer.clone());
    // For categories a person limited to sites.
    options.categories.site = crate::category::host_of(&slot.entry.url);
    options.sound = slot.entry.sound.clone();
    options.span = slot.entry.span;
    options.shape = slot.entry.shape;
    options.mp3 = slot.entry.mp3;
    options.gif = slot.entry.gif;
    // The quality picked in the browser's panel, over the setting.
    if let Some(quality) = &slot.entry.quality {
        options.quality = crate::stream::Quality::named(quality);
        options.container = crate::stream::Quality::container_of(quality);
        options.frame_rate = crate::stream::Quality::frame_rate_of(quality);
        // The picture the line was listed from, by its format number (gaps row Y9).
        options.format = crate::stream::Quality::format_of(quality);
        // A caption line: the caption file alone, in no container.
        options.caption = crate::stream::Quality::caption_of(quality);
        if options.caption.is_some() {
            options.container = None;
        }
        // MP3: the M4A sound fetched, then made an MP3 by dm.
        if options.container.as_deref() == Some("mp3") {
            options.container = Some("m4a".into());
            options.mp3 = true;
        }
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
    form_lost
}

/// Writes what a run reports on its row as it comes, until the run lets go of its sender:
/// what it reported last, counted from what an earlier run had already `staged`.
async fn watch_run(
    queue: DownloadList,
    id: Id,
    this_run: Control,
    staged: Option<u64>,
    mut rx: tokio::sync::mpsc::Receiver<crate::download::Update>,
) -> Option<u64> {
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
                slot.entry_mut().filename = name;
                slot.entry_mut().category = Some(category);
            }
            crate::download::Update::Progress(progress) => {
                // What arrived since the last sample counts
                // against the download limit.
                let arrived =
                    reported.map_or(0, |before| progress.downloaded.saturating_sub(before));
                // Bytes again: the retry is over.
                if progress.downloaded > slot.entry.progress.downloaded {
                    slot.retrying = None;
                }
                reported = Some(progress.downloaded);
                crate::lock(&queue.inner.quota).add(arrived, std::time::SystemTime::now());
                slot.entry_mut().progress = progress;
                // A report that arrives after a pause or a
                // cancel (what was in flight) counts its bytes
                // but is not a speed.
                if slot.entry.state != State::Running {
                    slot.stopped_moving();
                }
                drop(slots);
                queue.save_quota_now_and_then().await;
                continue;
            }
            crate::download::Update::Validator(validator) => {
                slot.entry_mut().validator = Some(validator);
            }
            crate::download::Update::Note(said) => {
                if slot.entry.said != said {
                    slot.entry_mut().said = said;
                }
            }
            crate::download::Update::Files(files) => {
                let merged = crate::torrent::merged_files(&slot.entry.torrent_files, files);
                if merged != *slot.entry.torrent_files {
                    slot.entry_mut().torrent_files = Arc::new(merged);
                }
            }
            // Kept for the window, not saved: a stopped
            // torrent has no swarm.
            crate::download::Update::Swarm(swarm) => {
                slot.swarm = Some(swarm);
                drop(slots);
                continue;
            }
            // The highest attempt of any range is the row's.
            crate::download::Update::Retrying {
                attempt,
                of,
                why,
                wait,
            } => {
                let now = slot.retrying.as_ref().map_or(0, |(a, _, _, _)| *a);
                // Not a run the row has been put back in
                // line since: its retry is not the row's.
                if attempt >= now && slot.control.is(&this_run) {
                    slot.retrying = Some((attempt, of, why, std::time::Instant::now() + wait));
                }
            }
        }
    }
    reported
}
