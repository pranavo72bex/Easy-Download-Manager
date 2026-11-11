// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the window asks of the app, as plain calls.
//!
//! The web window reached the queue across Tauri's IPC, which is why a test
//! had to hold the page's command names and the handler list to each other.
//! Here the window calls the queue directly, on the app's own runtime: a
//! command the window needs and the backend lacks is a compile error.

use std::sync::{Arc, Mutex};

use crate::{lists, shell, Settings};

/// How long a dm that is ending waits for its downloads to stop
/// (`Backend::shut_down`): each stops at its next piece, and a person
/// quitting is not kept waiting past this.
pub const SHUT_DOWN_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);
use dm_core::{AddOptions, Connection, Entry, Id, Queue};

pub use super::platform::{chooser_here, Platform};

/// The queue, the settings in force, and the problems no row can carry.
#[derive(Clone)]
pub struct Backend {
    /// The runtime the engine's work runs on.
    pub runtime: tokio::runtime::Handle,
    /// The download queue.
    pub queue: Queue,
    settings: Arc<Mutex<Settings>>,
    /// What went wrong at startup — a saved proxy that could not be used, a
    /// login item that could not be written — handed to the window once.
    notices: Arc<Mutex<Vec<String>>>,
    /// What the operating system is asked to do for dm: notifications, opening files, power.
    pub platform: Arc<dyn Platform>,
    /// Videos the browser's panel handed over to be asked about: the window
    /// opens "Download details" for each before it starts.
    pub handed: Arc<Mutex<std::collections::VecDeque<crate::host::Request>>>,
    /// Where slow work the window asks for is done, off its thread, in the
    /// order asked (`Self::later`).
    errands: Arc<crate::app::errands::Errands>,
}

impl Backend {
    /// Creates the backend over a queue, the settings in force, the start-up notices and the platform.
    pub fn new(
        runtime: tokio::runtime::Handle,
        queue: Queue,
        settings: Settings,
        notices: Vec<String>,
        platform: Arc<dyn Platform>,
    ) -> Self {
        Backend {
            runtime,
            queue,
            settings: Arc::new(Mutex::new(settings)),
            notices: Arc::new(Mutex::new(notices)),
            platform,
            handed: Arc::default(),
            errands: Arc::default(),
        }
    }

    /// Does `work` off the caller's thread, after the work asked for before
    /// it (`errands`): what a window's handler would otherwise wait for, a
    /// file copied to another disk or a command on ten thousand rows (gaps
    /// row 183). Its answer is awaited where it is to be shown.
    pub fn later<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Backend) -> R + Send + 'static,
    ) -> crate::app::errands::Answer<R> {
        let backend = self.clone();
        self.errands.run(move || work(&backend))
    }

    /// Runs `work` on the engine's runtime, beside the downloads rather
    /// than behind the window's errands: a read a window draws from (a
    /// download's pieces), which must not wait for a file being moved.
    pub fn on_runtime<R: Send + 'static>(
        &self,
        work: impl std::future::Future<Output = R> + Send + 'static,
    ) -> crate::app::errands::Answer<R> {
        let (tell, answer) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let _ = tell.send(work.await);
        });
        crate::app::errands::Answer(answer)
    }

    /// Places a download begun while Download details asked (`Queue::settle_early`).
    pub fn settle_early(
        &self,
        id: Id,
        dir: Option<std::path::PathBuf>,
        name: Option<String>,
        description: &str,
        queue: &str,
        later: bool,
    ) -> Result<(), String> {
        self.block_on(
            self.queue
                .settle_early(id, dir, name, description, queue, later),
        )
    }

    fn block_on<T>(&self, work: impl std::future::Future<Output = T>) -> T {
        self.runtime.block_on(work)
    }

    /// Every row, copied from the queue.
    #[must_use]
    pub fn list(&self) -> Vec<Entry> {
        self.block_on(self.queue.list())
    }

    /// What the window draws from, read at once (`crate::app::snapshot`).
    pub fn snapshot(&self) -> crate::app::snapshot::Snapshot {
        crate::app::snapshot::Snapshot {
            rows: self.list_shared(),
            retrying: self.retrying(),
            swarms: self.swarms(),
            allowance_spent: self.queue.allowance_spent().is_some(),
        }
    }

    /// The list shared with the queue, not copied: what the window reads.
    pub fn list_shared(&self) -> Vec<Arc<Entry>> {
        self.block_on(self.queue.list_shared())
    }

    /// One row as the queue holds it now, not as the window last read it.
    pub fn row(&self, id: Id) -> Option<Entry> {
        self.block_on(self.queue.get(id))
    }

    /// The rows being retried now, each with its attempt and how many
    /// there may be (0 for no end).
    pub fn retrying(
        &self,
    ) -> std::collections::HashMap<Id, (u32, u32, String, std::time::Instant)> {
        self.block_on(self.queue.retrying())
    }

    /// Which of a torrent's files are fetched, one tick a file.
    pub fn set_torrent_files(&self, id: Id, wanted: &[bool]) -> Result<(), String> {
        self.block_on(self.queue.set_torrent_files(id, wanted))
    }

    /// When each of a torrent's files is fetched, for a stopped torrent.
    pub fn set_torrent_priorities(
        &self,
        id: Id,
        priorities: &[dm_core::TorrentPriority],
    ) -> Result<(), String> {
        self.block_on(self.queue.set_torrent_priorities(id, priorities))
    }

    /// The speed limit every download shares, in bytes a second (0 for none).
    pub fn set_speed_limit(&self, bytes_per_second: u64) {
        self.block_on(self.queue.set_speed_limit(bytes_per_second));
    }

    /// Each running torrent's swarm, by row.
    pub fn swarms(&self) -> std::collections::HashMap<Id, dm_core::Swarm> {
        self.block_on(self.queue.swarms())
    }

    /// Adds the downloads that `text` names (addresses, one to a line).
    pub fn add(&self, text: &str) -> Result<shell::Added, String> {
        self.block_on(shell::add(&self.queue, text))
    }

    /// Pauses a download.
    pub fn pause(&self, id: Id) {
        self.block_on(self.queue.pause(id));
    }

    /// Puts a sign-in saved in Options since the row was added on its
    /// address: Retry of a 401 asked again without it, while a fresh copy
    /// of the address worked.
    fn login_now(&self, id: Id) {
        let Some(row) = self.block_on(self.queue.get(id)) else {
            return;
        };
        if row.state == dm_core::State::Running {
            return;
        }
        let signed = self.settings().with_login(&row.url);
        if signed != row.url {
            self.block_on(self.queue.set_url(id, &signed));
        }
    }

    /// Resumes a stopped download.
    pub fn resume(&self, id: Id) {
        self.login_now(id);
        self.block_on(async {
            self.queue.resume(id).await;
            shell::wake(&self.queue);
        });
    }

    /// Starts one row now, outside the schedule and the download limit.
    pub fn start_outside_schedule(&self, id: Id) -> bool {
        self.block_on(async {
            let started = self.queue.start_outside_schedule(id).await;
            if started {
                shell::wake(&self.queue);
            }
            started
        })
    }

    /// Starts a failed download again.
    pub fn retry(&self, id: Id) {
        self.login_now(id);
        self.block_on(async {
            if self.queue.retry(id).await {
                shell::wake(&self.queue);
            }
        });
    }

    /// A person said yes to saving the web page that answered for this file as
    /// an HTML page: the row is tried again as one.
    pub fn accept_page(&self, id: Id) {
        self.block_on(self.queue.accept_page(id));
        self.retry(id);
    }

    /// Cancels a download.
    pub fn cancel(&self, id: Id) {
        self.block_on(self.queue.cancel(id));
    }

    /// Removes a row from the list.
    pub fn remove(&self, id: Id) {
        self.block_on(self.queue.remove(id));
    }

    /// The finished rows among `ids` as they stand, to put back.
    pub fn finished_rows(&self, ids: &[Id]) -> Vec<(usize, Entry)> {
        self.block_on(self.queue.finished_rows(ids))
    }

    /// Rows taken off the list put back where they were.
    pub fn put_back(&self, rows: Vec<(usize, Entry)>) {
        self.block_on(self.queue.put_back(rows));
    }

    /// Several rows taken out with one save of the queue.
    pub fn remove_many(&self, ids: &[Id]) {
        self.block_on(self.queue.remove_many(ids));
    }

    /// Writes the queue file now if a change is still waiting to be written.
    pub fn flush(&self) {
        self.block_on(self.queue.flush());
    }

    /// What a dm that is ending does last (Exit, a logout, a signal): every
    /// running download stopped as a pause stops it, awaited for up to
    /// [`SHUT_DOWN_WITHIN`], and put back in line for the next start; then
    /// the queue file written (`Queue::shut_down`). Says how many runs were
    /// still going at the deadline.
    pub fn shut_down(&self) -> usize {
        self.block_on(self.queue.shut_down(SHUT_DOWN_WITHIN))
    }

    /// One command for the lot, not one per row: pausing forty downloads by
    /// asking forty times rewrites the queue file forty times.
    pub fn pause_all(&self) -> usize {
        self.block_on(self.queue.pause_all())
    }

    /// Resumes every stopped download and gives how many started.
    pub fn resume_all(&self) -> usize {
        self.block_on(async {
            let resumed = self.queue.resume_all().await;
            if resumed > 0 {
                shell::wake(&self.queue);
            }
            resumed
        })
    }

    /// Removes every finished download from the list and gives how many.
    pub fn clear_finished(&self) -> usize {
        self.block_on(self.queue.clear_finished())
    }

    /// A stopped download sent on to another folder, its unfinished parts
    /// moved there, and started again.
    pub fn relocate(&self, id: Id, dir: std::path::PathBuf) -> Result<(), String> {
        self.block_on(async {
            self.queue.relocate(id, dir).await?;
            shell::wake(&self.queue);
            Ok(())
        })
    }

    /// One download's own speed limit in KB/s, zero for none.
    pub fn set_own_limit(&self, id: Id, kb_per_second: u64) {
        self.block_on(self.queue.set_own_limit(id, kb_per_second));
    }

    /// Sets the checksum a finished download is checked against, from the text typed.
    pub fn set_checksum(&self, id: Id, text: &str) -> Result<(), String> {
        let checksum = shell::checksum(text)?;
        self.block_on(self.queue.set_checksum(id, checksum));
        Ok(())
    }

    /// The settings in force.
    #[must_use]
    pub fn settings(&self) -> Settings {
        dm_core::lock(&self.settings).clone()
    }

    /// A client for the application's own fetches, which are not downloads
    /// (a site's pages crawled, the update check, a ZIP's listing): through
    /// the proxy in force, with its read timeout.
    #[must_use]
    pub fn client(&self) -> dm_core::reqwest::Client {
        let settings = dm_core::lock(&self.settings);
        dm_core::client_with_timeout(&settings.proxy, settings.read_timeout())
            .unwrap_or_else(|_| dm_core::client())
    }

    /// How long downloads stay held where the download limit (the day's or
    /// the month's allowance) is spent; `None` while it is not.
    #[must_use]
    pub fn allowance_spent(&self) -> Option<std::time::Duration> {
        self.queue.allowance_spent()
    }

    /// Removes row `id` in the background, without waiting for it: what a
    /// dialog fetched early while it asked, once the person said no.
    pub fn remove_later(&self, id: Id) {
        let queue = self.queue.clone();
        self.runtime.spawn(async move { queue.remove(id).await });
    }

    /// The settings as they change, for a thread of its own to read.
    pub fn settings_handle(&self) -> Arc<Mutex<Settings>> {
        Arc::clone(&self.settings)
    }

    /// Applies a change asked for by a person or, `by_script`, by a script (the
    /// control port, an AI client): what makes dm run something (the command
    /// after each download, the scheduled hours' program, the proxy script) is
    /// a person's to set, as a settings file imported never sets it, so a
    /// script's change keeps the ones in force (`Settings::keep_programs_of`).
    /// Returns what was kept as well as the outcome, so the presentation can
    /// say so even when the change itself is then refused.
    pub fn set_settings_from(
        &self,
        mut wanted: Settings,
        by_script: bool,
    ) -> (Result<Settings, String>, crate::settings::KeptPrograms) {
        let kept = if by_script {
            wanted.keep_programs_of(&self.settings())
        } else {
            Default::default()
        };
        (self.set_settings(wanted), kept)
    }

    /// Applies a change and writes it down, returning what is now in force.
    ///
    /// Whole or not at all. What the change refuses (a relative folder, a
    /// time that is not one) and a proxy that cannot be built are found
    /// first, before anything is touched; then the login item, the other
    /// thing that can fail; then the queue, putting the login item back if
    /// that fails after all. Before, the login item changed and then a
    /// refused proxy left it so, beside settings that said otherwise. Then
    /// the record of what is held, before the save that might fail — this
    /// is what the queue is actually running, so it is what the next change
    /// is compared against.
    pub fn set_settings(&self, settings: Settings) -> Result<Settings, String> {
        let previous = self.settings();
        // Judged as typed, before `clamped` quietly mends what it can: a
        // relative download folder is refused, not swapped for the default.
        if let Some(why) = settings.refusal(&previous) {
            return Err(why);
        }
        let settings = settings.clamped();
        shell::check(&settings, Some(&previous))?;
        let login_changed = previous.start_on_login != settings.start_on_login;
        if login_changed {
            self.platform
                .set_login_item(settings.start_on_login)
                .map_err(|e| format!("could not change start-on-login: {e}"))?;
        }
        let problems = match self.block_on(shell::apply(&settings, Some(&previous), &self.queue)) {
            Ok(problems) => problems,
            Err(e) => {
                if login_changed {
                    if let Err(back) = self.platform.set_login_item(previous.start_on_login) {
                        self.note(format!(
                            "start-on-login could not be put back as it was: {back}"
                        ));
                    }
                }
                return Err(e);
            }
        };
        for problem in problems {
            self.note(problem);
        }
        *dm_core::lock(&self.settings) = settings.clone();
        settings
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        Ok(settings)
    }

    /// A change to how the window looks and nothing else (a column or a
    /// toolbar button dragged to another place): in force at once, and the
    /// settings file written off the caller's thread, after the errands
    /// asked before it (gaps row 183). Nothing reaches the queue, so
    /// nothing is applied to it. What is written is the settings in force
    /// when the errand runs, so a change made meanwhile is not written over
    /// by an older copy; a file that cannot be written is said in the
    /// banner.
    pub fn keep_look(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let now = {
            let mut settings = dm_core::lock(&self.settings);
            change(&mut settings);
            settings.clone()
        };
        // Not awaited: the errand runs whether or not its answer is.
        drop(self.later(|backend| {
            if let Err(e) = backend.settings().save() {
                backend.note(format!("settings could not be saved: {e}"));
            }
        }));
        now
    }

    /// Add URL's remembered addresses, kept and saved alone: nothing else
    /// changes, so nothing is applied to the queue — a full apply on every
    /// address added rebuilt its client and its cookies each time.
    pub fn remember_urls(&self, recent: Vec<String>) -> Result<Settings, String> {
        // Only this field changes on disk: the file is read, the list put
        // in, and written back, so nothing else saved there is overwritten.
        let (mut on_disk, _) = Settings::load();
        on_disk.recent_urls.clone_from(&recent);
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        settings.recent_urls = recent;
        settings.recent_urls.truncate(crate::settings::RECENT_URLS);
        Ok(settings.clone())
    }

    /// The folder Download details last saved a kind of file to, kept and
    /// saved alone as Add URL's addresses are; `None` forgets it, for the
    /// kind's own folder.
    pub fn remember_folder(&self, kind: &str, folder: Option<&str>) -> Result<Settings, String> {
        let (mut on_disk, _) = Settings::load();
        let set = |s: &mut Settings| match &folder {
            Some(f) => {
                s.last_folders.insert(kind.to_string(), (*f).to_string());
            }
            None => {
                s.last_folders.remove(kind);
            }
        };
        set(&mut on_disk);
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        set(&mut settings);
        Ok(settings.clone())
    }

    /// Whether a download the browser catches is asked about first
    /// (Options › Browser), as "Always save here without asking" switches
    /// it off from the dialog itself.
    pub fn set_ask_caught_downloads(&self, ask: bool) -> Result<Settings, String> {
        let (mut on_disk, _) = Settings::load();
        on_disk.ask_caught_downloads = ask;
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        settings.ask_caught_downloads = ask;
        Ok(settings.clone())
    }

    /// What went wrong that no row can carry, each handed over once.
    pub fn problems(&self) -> Vec<String> {
        let mut all = std::mem::take(&mut *dm_core::lock(&self.notices));
        all.extend(self.queue.problems());
        all
    }

    /// Where something running apart from the window leaves a problem for
    /// the banner.
    pub fn notices(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.notices)
    }

    /// Leaves a problem for the banner.
    pub fn note(&self, problem: String) {
        dm_core::lock(&self.notices).push(problem);
    }

    /* ------------------------------------------------------------- row verbs */

    /// Adds with what the "Download details" dialog chose. Site logins
    /// are put on the address here, so every way in gets them.
    pub fn add_with(&self, url: &str, with: AddOptions) -> Result<Id, String> {
        let settings = self.settings();
        let with = settings.with_site_rule(url, admit(url, with)?);
        let url = settings.with_login(url.trim());
        let later = with.later;
        let id = self
            .block_on(self.queue.add_with(&url, with))
            .ok_or_else(|| "that is not a URL dm can download".to_string())?;
        if !later {
            self.block_on(async { shell::wake(&self.queue) });
        }
        Ok(id)
    }

    /// Adds many at once, each with its options and a site's saved sign-in,
    /// writing the queue once. One result per address, in order.
    pub fn add_each(&self, items: Vec<(String, AddOptions)>) -> Vec<Result<Id, String>> {
        let settings = self.settings();
        // Each refused here is answered in its place; the rest are added in
        // one go, writing the queue once.
        let admitted: Vec<Result<(String, AddOptions), String>> = items
            .into_iter()
            .map(|(url, with)| {
                let with = settings.with_site_rule(&url, admit(&url, with)?);
                Ok((settings.with_login(url.trim()), with))
            })
            .collect();
        let starts = admitted
            .iter()
            .any(|item| item.as_ref().is_ok_and(|(_, with)| !with.later));
        let ids = self.block_on(
            self.queue.add_each(
                admitted
                    .iter()
                    .filter_map(|item| item.as_ref().ok().cloned())
                    .collect(),
            ),
        );
        if starts {
            self.block_on(async { shell::wake(&self.queue) });
        }
        let mut ids = ids.into_iter();
        admitted
            .into_iter()
            .map(|item| {
                item?;
                ids.next()
                    .flatten()
                    .ok_or_else(|| "that is not a URL dm can download".to_string())
            })
            .collect()
    }

    /// Sets a download's description.
    pub fn set_description(&self, id: Id, text: &str) {
        self.block_on(self.queue.set_description(id, text));
    }

    /// "Delete file after" for a row; `None` keeps its file.
    pub fn set_delete_after(&self, id: Id, seconds: Option<u64>) {
        self.block_on(self.queue.set_delete_after(id, seconds));
    }

    /// Puts a download in the queue of this name.
    pub fn set_queue(&self, id: Id, queue: &str) {
        self.block_on(self.queue.set_queue(id, queue));
    }

    /// Gives a download a new address.
    pub fn set_url(&self, id: Id, url: &str) -> Result<(), String> {
        let url = self.settings().with_login(url.trim());
        if !dm_core::accepted(&url) {
            return Err("it must be a link dm can download".into());
        }
        // A download retrying between attempts is the one most likely to
        // need a new address: stopped, given it, and started
        // again, rather than refused as running.
        let was_running = self
            .block_on(self.queue.get(id))
            .is_some_and(|row| row.state == dm_core::State::Running);
        if was_running {
            self.pause(id);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline
                && self
                    .block_on(self.queue.get(id))
                    .is_some_and(|row| row.state == dm_core::State::Running)
            {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
        let changed = self.block_on(self.queue.set_url(id, &url));
        if was_running {
            self.resume(id);
        }
        if changed {
            Ok(())
        } else {
            Err(
                "a running or finished download's address cannot be changed, \
                 and it must be a link dm can download"
                    .into(),
            )
        }
    }

    /// Downloads a file again from the start.
    pub fn redownload(&self, id: Id) {
        self.block_on(async {
            if self.queue.redownload(id).await {
                shell::wake(&self.queue);
            }
        });
    }

    /// Moves a download up or down its queue by this many places.
    pub fn shift(&self, id: Id, by: isize) {
        self.block_on(self.queue.shift(id, by));
    }

    /// Starts a queue and gives how many downloads it started.
    pub fn start_queue(&self, queue: &str) -> usize {
        self.block_on(async {
            let started = self.queue.start_queue(queue).await;
            if started > 0 {
                shell::wake(&self.queue);
            }
            started
        })
    }

    /// Stops a queue and gives how many downloads it stopped.
    pub fn stop_queue(&self, queue: &str) -> usize {
        self.block_on(self.queue.stop_queue(queue))
    }

    /// Renames a finished download's file.
    pub fn rename(&self, id: Id, name: &str) -> Result<(), String> {
        self.block_on(self.queue.rename(id, name))
    }

    /// Moves a finished file into `dir`; see `Queue::move_to`.
    /// A folder typed as a relative path is refused, as a new download's is
    /// (`admit`): it was taken against wherever dm was started, and the file
    /// went there.
    pub fn move_to(&self, id: Id, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let dir = full_folder(&dir.to_string_lossy())?;
        self.block_on(self.queue.move_to(id, &dir))
    }

    /// A playable file of what a download has fetched so far.
    pub fn preview(&self, id: Id) -> Result<std::path::PathBuf, String> {
        self.block_on(self.queue.preview(id))
    }

    /// A download's connections, each with its state.
    #[must_use]
    pub fn connections(&self, id: Id) -> Vec<Connection> {
        self.block_on(self.queue.connections(id))
    }

    /// Whether a download can resume, if that is known yet.
    #[must_use]
    pub fn resumable(&self, id: Id) -> Option<bool> {
        self.block_on(self.queue.resumable(id))
    }

    /// The names of the queues.
    #[must_use]
    pub fn queues(&self) -> Vec<String> {
        self.block_on(self.queue.queues())
    }

    /// Every download, as a `.dmlist` file.
    pub fn export(&self) -> String {
        let rows: Vec<lists::Listed> = self
            .list()
            .into_iter()
            // Without a login in the address: a list file is shared and
            // kept, and a password is not something to leave in it. Options
            // › Sign-ins puts the login back on import.
            // What a person gave the row goes with it: a list that came
            // back without its queues, notes, names and digests was a list
            // of addresses, and the rest had to be done again by hand.
            .map(|e| lists::Listed {
                url: without_login(&e.url),
                referrer: e.referrer,
                queue: e.queue,
                note: e.description,
                name: e.save_name.unwrap_or_default(),
                checksum: e
                    .checksum
                    .map(|c| format!("{}:{}", c.algorithm.name(), c.expected))
                    .unwrap_or_default(),
            })
            .collect();
        lists::to_list(&rows)
    }

    /// Adds what a `.dmlist` file or a plain list holds, saying how many.
    pub fn import(&self, text: &str) -> usize {
        self.import_where(text, |_| true)
    }

    /// The addresses a list holds, in its order.
    pub fn listed(text: &str) -> Vec<String> {
        lists::from_list(text)
            .into_iter()
            .map(|row| row.url)
            .collect()
    }

    /// [`Self::import`], of only the rows whose address `keep` keeps.
    pub fn import_where(&self, text: &str, keep: impl Fn(&str) -> bool) -> usize {
        let mut rows = lists::from_list(text);
        rows.retain(|row| keep(&row.url));
        let items = rows
            .iter()
            .map(|row| {
                let with = AddOptions {
                    referrer: row.referrer.clone(),
                    queue: row.queue.clone(),
                    description: row.note.clone(),
                    save_name: (!row.name.is_empty()).then(|| row.name.clone()),
                    // Held until its digest is on it: started at once, it
                    // could be fetched and finished before the check was
                    // there to be made.
                    later: !row.checksum.is_empty(),
                    ..AddOptions::default()
                };
                (row.url.clone(), with)
            })
            .collect();
        let added = self.add_each(items);
        let mut held = false;
        for (row, id) in rows.iter().zip(&added) {
            let Ok(id) = id else { continue };
            if row.checksum.is_empty() {
                continue;
            }
            // A digest that does not read is left off rather than failing
            // the row: the download is still the one listed.
            let _ = self.set_checksum(*id, &row.checksum);
            self.block_on(self.queue.resume(*id));
            held = true;
        }
        if held {
            self.block_on(async { shell::wake(&self.queue) });
        }
        added.iter().filter(|r| r.is_ok()).count()
    }
}

/// A download as the queue may keep it, or why not: an address with no
/// site in it, or a folder that is not a full path. `~` is the home folder;
/// a relative folder is refused, because a row keeps it and Open and Open
/// folder would read it later from wherever dm was started — `/` from the
/// Dock — and `~/x` made a folder called `~`.
fn admit(url: &str, mut with: AddOptions) -> Result<AddOptions, String> {
    if let Some(why) = shell::unusable(url) {
        return Err(why);
    }
    if let Some(typed) = with.save_dir.take().filter(|f| !f.trim().is_empty()) {
        with.save_dir = Some(full_folder(&typed)?.display().to_string());
    }
    Ok(with)
}

/// A folder typed for a row, as a full path (`~` is the home folder), or why not.
fn full_folder(typed: &str) -> Result<std::path::PathBuf, String> {
    crate::settings::full_folder(typed, None).ok_or_else(|| {
        format!("the folder must be a full path, such as ~/Downloads, not {typed:?}")
    })
}

/// An address with no user or password in it.
fn without_login(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() || parsed.password().is_some() => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod admit_tests {
    use super::*;

    #[test]
    fn a_download_is_refused_with_no_site_or_a_relative_folder() {
        let no_host = admit("http://", AddOptions::default()).unwrap_err();
        assert!(no_host.contains("names no site"), "{no_host}");
        let relative = AddOptions {
            save_dir: Some("out".into()),
            ..AddOptions::default()
        };
        assert!(admit("https://files.example/a.zip", relative).is_err());
        let home = AddOptions {
            save_dir: Some("~/x".into()),
            ..AddOptions::default()
        };
        let admitted = admit("https://files.example/a.zip", home).unwrap();
        let saved = admitted.save_dir.unwrap();
        assert!(!saved.contains('~') && std::path::Path::new(&saved).is_absolute());
        assert!(admit(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            AddOptions::default()
        )
        .is_ok());
    }
}

#[cfg(test)]
mod export_tests {
    use super::testing::backend;

    #[test]
    fn an_exported_list_brings_back_queue_note_name_and_digest() {
        /* Only the address and the page went out, so a list moved to
         * another computer came back without the rest. Held for later so
         * nothing is fetched; nothing listens on the port anyway. */
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let from = backend(&runtime, dir.path());
        let digest = "a".repeat(64);
        let id = from
            .add_with(
                "http://127.0.0.1:9/a.zip",
                dm_core::AddOptions {
                    referrer: "http://127.0.0.1:9/page".into(),
                    queue: "Nightly".into(),
                    description: "the big one".into(),
                    save_name: Some("mine.zip".into()),
                    later: true,
                    ..dm_core::AddOptions::default()
                },
            )
            .unwrap();
        from.set_checksum(id, &format!("sha256:{digest}")).unwrap();

        let to = backend(&runtime, dir.path());
        assert_eq!(to.import(&from.export()), 1);
        let row = to.list().pop().unwrap();
        assert_eq!(row.referrer, "http://127.0.0.1:9/page");
        assert_eq!(row.queue, "Nightly");
        assert_eq!(row.description, "the big one");
        assert_eq!(row.save_name.as_deref(), Some("mine.zip"));
        assert_eq!(row.checksum.map(|c| c.expected), Some(digest));
    }

    #[test]
    fn an_exported_address_carries_no_login() {
        assert_eq!(
            super::without_login(concat!("https://me:", "p%40ss", "@files.example/a.zip")),
            "https://files.example/a.zip"
        );
        assert_eq!(
            super::without_login("https://files.example/a.zip"),
            "https://files.example/a.zip"
        );
        assert_eq!(
            super::without_login("magnet:?xt=urn:btih:abc"),
            "magnet:?xt=urn:btih:abc"
        );
    }
}

#[cfg(test)]
mod chooser_tests {
    /// With no portal and no zenity there is no chooser, and
    /// Browse… says so rather than doing nothing.
    #[test]
    fn a_chooser_is_the_portal_or_zenity_on_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert!(!super::chooser_here(false, Some(&path)));
        assert!(!super::chooser_here(false, None));
        assert!(super::chooser_here(true, None));
        std::fs::write(dir.path().join("zenity"), "").unwrap();
        assert!(super::chooser_here(false, Some(&path)));
    }
}

#[cfg(test)]
mod keep_look_tests {
    use super::testing::backend;

    /// A column dragged is in force at once and reaches the settings file
    /// off the caller's thread; a change made meanwhile is not written over.
    #[test]
    fn a_look_kept_is_in_force_at_once_and_written_after() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        let order = vec!["size".to_string(), "status".to_string()];
        let now = backend.keep_look(|s| s.column_order.clone_from(&order));
        assert_eq!(now.column_order, order);
        assert_eq!(backend.settings().column_order, order);
        // An errand after it runs once it is written. (What the file holds
        // is not compared: this crate's other tests write it at the same
        // time, from their own backends.)
        let written = runtime.block_on(backend.later(|_| crate::Settings::path().is_file()));
        assert_eq!(written, Some(true));
    }
}

#[cfg(test)]
mod settings_from_tests {
    use super::testing::backend;

    /// What makes dm run something is a person's to set: a script's change
    /// keeps the command in force and says it did; a person's sets it.
    #[test]
    fn a_scripts_settings_change_keeps_the_command_a_persons_sets_it() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        let mut wanted = backend.settings();
        wanted.on_complete = "echo done".into();
        let (now, kept) = backend.set_settings_from(wanted.clone(), true);
        assert!(kept.command);
        assert_eq!(now.unwrap().on_complete, "");
        let (now, kept) = backend.set_settings_from(wanted, false);
        assert!(!kept.command);
        assert_eq!(now.unwrap().on_complete, "echo done");
    }
}

#[cfg(test)]
mod undo_tests {
    use super::testing::backend;
    use super::*;

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
}

#[cfg(test)]
mod set_url_tests {
    use super::testing::backend;
    use std::time::Duration;

    /// A download running when it is given a new address is stopped, given
    /// it and started again; one that is no link is refused.
    #[test]
    fn a_running_download_given_a_new_address_is_stopped_changed_and_started_again() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        // A server that takes each request and never answers it.
        let addr = runtime.block_on(async {
            let app = axum::Router::new().fallback(|| async {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                "never"
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            addr
        });
        let id = backend
            .add_with(
                &format!("http://{addr}/old.bin"),
                dm_core::AddOptions::default(),
            )
            .unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while backend.row(id).unwrap().state != dm_core::State::Running {
            assert!(std::time::Instant::now() < until, "never ran");
            std::thread::sleep(Duration::from_millis(10));
        }
        let new = format!("http://{addr}/new.bin");
        backend.set_url(id, &new).unwrap();
        let row = backend.row(id).unwrap();
        assert_eq!(row.url, new);
        assert!(
            matches!(row.state, dm_core::State::Running | dm_core::State::Queued),
            "started again: {:?}",
            row.state
        );
        assert!(backend.set_url(id, "not a link").is_err());
        backend.cancel(id);
    }
}

/// How long after downloads were taken off the list Undo puts them back.
pub const UNDO_FOR: std::time::Duration = std::time::Duration::from_secs(5);

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
        let (at, rows) = self.0.take()?;
        if at.elapsed() >= UNDO_FOR || rows.is_empty() {
            return None;
        }
        let ids = rows.iter().map(|(_, e)| e.id).collect();
        backend.put_back(rows);
        Some(ids)
    }
}

/// A `Backend` on a queue of its own, with a platform that does nothing, for
/// the application's tests.
#[cfg(test)]
pub(crate) mod testing {
    pub(crate) struct Nothing;

    impl super::Platform for Nothing {
        fn set_login_item(&self, _: bool) -> Result<(), String> {
            Ok(())
        }
        fn notify(&self, _: &str, _: &str) {}
        fn clipboard(&self) -> Option<String> {
            None
        }
        fn reveal(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn open(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn chime(&self) {}
        fn power(
            &self,
            _: crate::app::power::WhenDone,
            _: crate::app::power::Report,
        ) -> Result<(), String> {
            Ok(())
        }
        fn choose_app(&self) -> Option<std::path::PathBuf> {
            None
        }
        fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
            Ok(())
        }
        fn choose_file(&self, _: bool, _: &str) -> Option<std::path::PathBuf> {
            None
        }
    }

    pub(crate) fn backend(
        runtime: &tokio::runtime::Runtime,
        dir: &std::path::Path,
    ) -> super::Backend {
        backend_with(runtime, dir, crate::Settings::default())
    }

    /// A backend over `settings`, its downloads under `dir`.
    pub(crate) fn backend_with(
        runtime: &tokio::runtime::Runtime,
        dir: &std::path::Path,
        settings: crate::Settings,
    ) -> super::Backend {
        let settings = crate::Settings {
            download_dir: dir.join("out"),
            ..settings
        };
        let queue = dm_core::Queue::new(
            dm_core::client(),
            settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
            settings.concurrency,
        );
        super::Backend::new(
            runtime.handle().clone(),
            queue,
            settings,
            Vec::new(),
            std::sync::Arc::new(Nothing),
        )
    }
}
