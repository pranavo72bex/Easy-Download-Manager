// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the window asks of the app, as plain calls.
//!
//! The window calls the queue directly, on the app's own runtime: a
//! command the window needs and the backend lacks is a compile error.
//!
//! # Which thread may wait on what
//!
//! Most calls here wait for the engine (`block_on`, on the engine's
//! runtime) on the caller's thread. Who may make them:
//!
//! * **The window's thread** (the one that draws: its frames, its
//!   listeners, its tick, its dialogs' answers) never waits on the disk,
//!   the network or a download stopping. It calls only the reads that are
//!   one of the queue's locks and a copy from memory: [`Backend::snapshot`]
//!   (each wake's read), [`Backend::row`], [`Backend::list_shared`],
//!   [`Backend::queues`], [`Backend::finished_rows`],
//!   [`Backend::clearable_rows`], [`Backend::retrying`],
//!   [`Backend::swarms`], and those with no wait at all
//!   ([`Backend::settings`], [`Backend::allowance_spent`],
//!   [`Backend::problems`]). They stay because the frame drawn next is drawn
//!   from their answer, and the queue lets go of its locks before it writes
//!   a file or asks a server, so the wait is as long as a copy of the rows.
//!   Anything else it hands off: to [`Backend::later`] (the errands' thread,
//!   in the order asked) or [`Backend::on_runtime`] (beside the downloads),
//!   and what came of it is said, and the list read again, when the
//!   [`crate::app::errands::Answer`] comes back to its own executor.
//!   Still made on it, each a known wait left for later: the settings
//!   changed ([`Backend::set_settings`] and its kin: in force or refused,
//!   said at once and read by the next line, with the settings file
//!   written); a download added ([`Backend::add`], [`Backend::add_with`],
//!   [`Backend::add_each`], [`Backend::settle_early`]: the new row chosen and
//!   what came of it said at once); a row dragged to another place
//!   ([`Backend::shift`]: it lands there in the next frame); the tick's own
//!   (the schedules' speed limit, "Delete file after"'s removal, a
//!   hand-off's add); and Exit's [`Backend::shut_down`], which waits for
//!   the downloads to stop, up to [`SHUT_DOWN_WITHIN`], as the window ends.
//!   A debug build counts the waits made while drawing
//!   (`crate::app::costs`).
//! * **The errands' thread** (`dm-errands`, [`Backend::later`]) may wait on
//!   anything the engine does: a file moved, the queue written, a running
//!   download stopped (a new address waits up to five seconds for it). One
//!   errand at a time, in the order asked, so two commands keep their order.
//! * **The runtime's workers** never block: no call here (a `block_on`
//!   inside the runtime would panic), and no file or socket but tokio's.
//! * **The runtime's blocking pool** (`spawn_blocking`) is where the engine
//!   reads and writes files it awaits: the queue file, a plan, a preview.
//! * **Threads of their own** (the control port's, the loop with no
//!   window, the login item's, the session's end) may wait on the engine
//!   through these calls; they draw nothing.
//!
//! # The one wake channel
//!
//! What tells the window (or the loop with no window) to look again is one
//! channel, `dm_desktop_base::wakes`: an `async_channel` holding at most
//! one wake of each kind, sent from any thread through the `wake` the app
//! is started with, and read on the window's own executor (or by that loop,
//! waiting on the runtime with its clock beside it). The answer to work
//! handed off is not a wake: it is the [`crate::app::errands::Answer`]
//! awaited where it is shown. The control port's requests are data, on a
//! channel of their own, which a wake tells the window to read.

use std::sync::{Arc, Mutex};

use crate::{shell, Settings};

/// How long a dm that is ending waits for its downloads to stop
/// (`Backend::shut_down`): each stops at its next piece, and a person
/// quitting is not kept waiting past this.
pub const SHUT_DOWN_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

use dm_core::{DownloadList, Entry, Id};

pub use super::platform::{chooser_here, Platform};
pub use undo::{Undo, UNDO_FOR};

mod rows;
mod settings;
mod undo;

/// The queue, the settings in force, and the problems no row can carry.
#[derive(Clone)]
pub struct Backend {
    /// The runtime the engine's work runs on.
    pub runtime: tokio::runtime::Handle,
    /// The download queue.
    pub queue: DownloadList,
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
    /// Held while the system's login item is changed and the setting that
    /// says it is put in force (`set_settings`), and while it is put in
    /// step at launch from a thread of its own (`login_item_follows_setting`,
    /// gaps row S16): one cannot slip between the other's two steps and
    /// leave the item on beside a setting saying off.
    login: Arc<Mutex<()>>,
}

impl Backend {
    /// Creates the backend over a queue, the settings in force, the start-up notices and the platform.
    pub fn new(
        runtime: tokio::runtime::Handle,
        queue: DownloadList,
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
            login: Arc::default(),
        }
    }

    /// Puts the system's login item in step with the setting in force: as
    /// the setting says when this runs, not as it said at launch, so a
    /// change made in Options meanwhile is the one that stands. dm's start
    /// does this off the window's thread once the window is up (gaps row
    /// S16); what the system refuses comes back in its words.
    pub fn login_item_follows_setting(&self) -> Result<(), String> {
        let _changing = dm_core::lock(&self.login);
        let wanted = self.settings().general.start_on_login;
        self.platform.set_login_item(wanted)
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

    /// Places a download begun while Download details asked (`DownloadList::settle_early`).
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
        crate::app::costs::waited();
        self.runtime.block_on(work)
    }

    /// Every row, copied from the queue.
    #[must_use]
    pub fn list(&self) -> Vec<Entry> {
        self.block_on(self.queue.list())
    }

    /// What the window draws from, read at once (`crate::app::snapshot`).
    /// The rows, the retries and the swarms come from one walk of the queue
    /// (`DownloadList::read_shared`): three walked it three times (gaps row 210).
    pub fn snapshot(&self) -> crate::app::snapshot::Snapshot {
        let read = self.block_on(self.queue.read_shared());
        crate::app::snapshot::Snapshot {
            rows: read.rows,
            retrying: read.retrying,
            swarms: read.swarms,
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
    pub fn add(&self, text: &str) -> Result<shell::Added, crate::Error> {
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

    /// A person said Download again to the same file as one already here (gaps row W18): the
    /// row is tried again, not asked about again.
    pub fn accept_same(&self, id: Id) {
        self.block_on(self.queue.accept_same(id));
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

    /// Every row Delete Completed would take off, with where it stands, for Undo.
    pub fn clearable_rows(&self) -> Vec<(usize, Entry)> {
        self.block_on(self.queue.clearable_rows())
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
    /// the queue file written (`DownloadList::shut_down`). Says how many runs were
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
        if let Some(why) =
            crate::app::control_api::folder_refused(&dir, &self.settings().folders.download_dir)
        {
            return Err(why);
        }
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
    pub fn set_checksum(&self, id: Id, text: &str) -> Result<(), crate::Error> {
        let checksum = shell::checksum(text)?;
        self.block_on(self.queue.set_checksum(id, checksum));
        Ok(())
    }

    /// Removes row `id` in the background, without waiting for it: what a
    /// dialog fetched early while it asked, once the person said no.
    pub fn remove_later(&self, id: Id) {
        let queue = self.queue.clone();
        self.runtime.spawn(async move { queue.remove(id).await });
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
mod login_tests;

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
        fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
            Ok(())
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
        let mut settings = settings;
        settings.folders.download_dir = dir.join("out");
        let queue = dm_core::DownloadList::new(
            dm_core::client(),
            settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
            settings.downloads.concurrency,
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
