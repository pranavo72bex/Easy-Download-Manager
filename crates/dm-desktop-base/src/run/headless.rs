// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `--headless`: dm with no window, for a server, a script or an agent. The
//! queue, the browser's hand-offs, the watched folder, the saved searches,
//! the playlists and feeds followed, the schedules, When done and the
//! control protocol go on as they do under
//! a window, with nothing drawn and no toolkit started, so it runs where
//! there is no display. Each rule is the application's (`dm_shell::app`);
//! what a window would ask a person is settled with no one asked: a
//! hand-off is added straight in, Exit when
//! done is at once, a row the schedule holds waits, and what needs a window
//! (a dialog, the clipboard, a file opened) is refused, said why.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use dm_core::{Entry, Id};
use dm_shell::app::backend::Backend;
use dm_shell::app::commands::{self, Command};
use dm_shell::app::handoff::{Asking, Route};
use dm_shell::app::places::{Place, Places};
use dm_shell::app::session::Carried;
#[cfg(test)]
use serde_json::{json, Value};

use crate::boot::Wake;
use crate::control::Request;
use crate::i18n::{fill, form_of, t};
#[cfg(test)]
use crate::probe;
use crate::session::{Driver, Effect, Session};

mod follows;
mod port;

/// How often dm looks at its clock when nothing wakes it: the schedules,
/// When done's countdown, the watched folder and the saved searches.
const CLOCK: Duration = Duration::from_secs(1);

/// How long dm waits after a wake that asks for soon, so the wakes within it are taken as one.
const SOON: Duration = Duration::from_millis(250);

/// How often the saved searches are looked at for one that is due.
const SEARCHES_EVERY: Duration = Duration::from_secs(60);

/// Hand-offs taken in one look: with nobody to ask, each goes straight in,
/// so a batch is not left waiting a second a link.
const HAND_OFFS_AT_ONCE: usize = 256;

/// Set by a phone's app when its window has closed and dm goes on without one (Options ›
/// Downloads, "Keep downloads and torrents going when dm is closed"): the headless loop then
/// ends by itself once nothing has been downloading or sharing for [`IDLE_FOR`], so the
/// process is let go. Never set on a computer, where `--headless` runs until it is told to end.
pub static QUIT_WHEN_IDLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// How long nothing must be downloading or sharing before [`QUIT_WHEN_IDLE`] ends the loop: a
/// queued download starting a moment after another ends is not taken for the end.
const IDLE_FOR: Duration = Duration::from_secs(10);

/// Whether `rows` keep dm busy: one downloading or putting its file together, or one waiting
/// that the queue lets start, or a finished torrent being shared (`sharing`).
fn busy(rows: &[Arc<Entry>], sharing: bool) -> bool {
    sharing
        || rows
            .iter()
            .any(|row| matches!(row.state, dm_core::State::Running | dm_core::State::Queued))
}

/// What a step over the port is refused with: there is nothing to click,
/// type into or scroll.
pub const NO_WINDOW: &str =
    "dm is running headless: there is no window to click, type or scroll in";

/// A sentence for the control port, which reports what dm says in English:
/// `template`'s English form for `n`, filled.
fn english(template: &str, n: u64, values: &[&dyn Display]) -> String {
    fill(form_of(template, "en", n), values)
}

/// dm with no window: what a window keeps of the list, without its looks.
pub struct Headless {
    backend: Backend,
    session: Session,
    rows: Vec<Arc<Entry>>,
    /// Each row by the name its file will get, as a window lists it.
    names: HashMap<Id, String>,
    places: Places,
    place: Place,
    selected: BTreeSet<Id>,
    /// What a window's status line would say now, in English.
    said: String,
    problems: Vec<String>,
    requests: Option<mpsc::Receiver<Request>>,
    /// The port was opened only for AI assistants.
    assistants_only: bool,
    quit_asked: bool,
    /// The speed limit before the limiter was turned off, for turning it on.
    last_limit: u64,
    /// What the saved searches found, as each run ends, by the search's name.
    #[allow(clippy::type_complexity)]
    found: Arc<Mutex<Vec<(String, Result<dm_shell::lists::Crawled, String>)>>>,
    /// When the saved searches were last looked at.
    searches_checked: Option<Instant>,
    /// The Recheck queue's counts, once a check of it is done.
    rechecked: Arc<Mutex<Option<(usize, usize)>>>,
    /// What hanging up the dial-up connection came to (Hang up modem when done), off this
    /// thread, until it is said.
    hung_up: Arc<Mutex<Option<dm_shell::dialup::HungUp>>>,
}

/// Runs dm with no window until it is asked to end (Exit over the port,
/// When done's Exit, a signal): woken by the engine and the control port
/// (`woken`, the window's own wake channel), and by its clock once a
/// second. The queue is written down and the port forgotten before it
/// returns.
pub fn run(
    backend: Backend,
    requests: Option<mpsc::Receiver<Request>>,
    assistants_only: bool,
    woken: &crate::wakes::Woken,
) {
    let mut dm = Headless::new(backend, requests, assistants_only);
    let mut idle_since: Option<Instant> = None;
    while !crate::signals::SIGNALLED.load(Ordering::SeqCst) {
        dm.tick();
        if dm.quit_asked {
            break;
        }
        // A phone's window closed with work left: done once that work is.
        if QUIT_WHEN_IDLE.load(Ordering::SeqCst) {
            if busy(&dm.rows, dm_core::sharing_any()) {
                idle_since = None;
            } else if idle_since.get_or_insert_with(Instant::now).elapsed() >= IDLE_FOR {
                break;
            }
        }
        // Waited for on the runtime, from this thread, which draws nothing
        // (`Backend`'s rule). The timeout is made inside the future: made out
        // here, on a thread with no runtime entered, its timer finds none and
        // panics, and `--headless` died as it opened its port.
        let next = dm
            .backend
            .runtime
            .block_on(async { tokio::time::timeout(CLOCK, woken.recv()).await });
        match next {
            // Wakes within the quarter are one.
            Ok(Ok(Wake::Soon)) => std::thread::sleep(SOON),
            // A wake to act on now, or the clock.
            Ok(Ok(Wake::Now)) | Err(_) => {}
            // Every sender gone: the clock alone.
            Ok(Err(_)) => std::thread::sleep(CLOCK),
        }
        while woken.try_recv().is_ok() {}
    }
    dm.backend.shut_down();
    crate::control::closing();
    crate::control::forget_port(&dm_shell::settings::home());
}

impl Headless {
    /// dm over `backend`, answering `requests` (only AI assistants', where
    /// `assistants_only`).
    pub fn new(
        backend: Backend,
        requests: Option<mpsc::Receiver<Request>>,
        assistants_only: bool,
    ) -> Headless {
        let mut dm = Headless {
            backend,
            session: Session::default(),
            rows: Vec::new(),
            names: HashMap::new(),
            places: Places::default(),
            place: Place::All,
            selected: BTreeSet::new(),
            said: String::new(),
            problems: Vec::new(),
            requests,
            assistants_only,
            quit_asked: false,
            last_limit: 0,
            found: Arc::default(),
            searches_checked: None,
            rechecked: Arc::default(),
            hung_up: Arc::default(),
        };
        dm.last_limit = commands::remembered_limit(&dm.backend.settings());
        dm.read();
        dm
    }

    /// One look: the list read, what it detected told, the schedules, When
    /// done, the hand-offs, the countdown, "Delete file after", the watched
    /// folder, the saved searches, and the port's requests answered.
    fn tick(&mut self) {
        self.read();
        // The application's tick (`Session::tick`): the inbox's orders, what
        // the read detected told, the schedules and When done, with nobody to
        // see a countdown for Exit.
        for effect in self.session.take_orders(Driver::HEADLESS) {
            self.carry(effect);
        }
        for effect in self
            .session
            .tick(&self.backend, &self.rows, Driver::HEADLESS)
        {
            if matches!(effect, Effect::Quit) {
                self.quit_asked = true;
                return;
            }
            self.carry(effect);
        }
        // What a platform held back while the person was busy elsewhere.
        self.backend.platform.tick();
        self.take_handed();
        self.countdown();
        self.delete_due_files();
        self.take_watched();
        self.take_searches();
        self.say_rechecked();
        self.answer_requests();
        let problems = self.backend.problems();
        self.problems
            .extend(problems.iter().map(|p| crate::format::worded_problem(p)));
    }

    /// The list read again, as a window reads it.
    fn read(&mut self) {
        dm_shell::app::changes::take();
        let settings = self.backend.settings();
        let read = self.session.read(&self.backend, &settings);
        self.session.note_read(read.detected);
        self.problems.extend(read.problems);
        crate::rows::set_retrying(read.snapshot.retrying);
        crate::rows::set_swarms(read.snapshot.swarms);
        crate::rows::set_limit_holds(read.snapshot.allowance_spent);
        crate::rows::set_hold_times(self.backend.allowance_spent(), &settings.schedule);
        // What of the computer's state holds the rows: their reason (gaps row W11).
        crate::rows::set_held_by(self.backend.held_by());
        self.rows = read.snapshot.rows;
        // Once per read, as the window notes it: a read between ticks (a command over the port,
        // a hand-off) that took the change no longer leaves When done unfired (gaps row A7).
        self.session.note_dry(&self.rows);
        let mut categories = settings.categories();
        let before = dm_shell::app::names::earlier_of_the_same_name(&self.rows);
        self.names = self
            .rows
            .iter()
            .map(|row| {
                let name = dm_shell::app::names::name_it_will_get_after(
                    &self.rows,
                    row,
                    &settings.folders.download_dir,
                    &mut categories,
                    Some(before.get(&row.id).copied().unwrap_or(0)),
                );
                (row.id, name)
            })
            .collect();
        crate::rows::set_superseded(&self.rows);
        let ids: std::collections::HashSet<Id> = self.rows.iter().map(|r| r.id).collect();
        self.selected.retain(|id| ids.contains(id));
        self.places.place_rows(&self.rows, &settings.categories());
    }

    /// The rows the place shows, in the list's order.
    fn shown(&self) -> Vec<&Entry> {
        let categories = self.backend.settings().categories();
        self.rows
            .iter()
            .map(|r| &**r)
            .filter(|r| self.places.within(&self.place, r, &categories))
            .collect()
    }

    /// The rows chosen: selected, of those the place shows.
    fn chosen(&self) -> Vec<&Entry> {
        self.shown()
            .into_iter()
            .filter(|r| self.selected.contains(&r.id))
            .collect()
    }

    /// Whether `command` applies now, as a window greys it.
    fn enabled(&self, command: Command) -> bool {
        commands::applies(
            command,
            &self.chosen(),
            &self.rows,
            false,
            &crate::rows::Looks,
        )
    }

    /// The queue Start Queue and Stop Queue act on: the place's, else the
    /// main one.
    fn current_queue(&self) -> String {
        match &self.place {
            Place::Queue(name) => name.clone(),
            _ => dm_core::MAIN_QUEUE.to_string(),
        }
    }

    /// The sound file chosen for `event`, if one was.
    fn sound(&self, event: &str) -> Option<String> {
        self.backend
            .settings()
            .sounds
            .get(event)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
    }

    /// One thing the application's tick asks (`Session::tick`), settled with
    /// no one asked: what a window would open is not, what it would say is
    /// said in English, and a failure is notified without a Retry.
    fn carry(&mut self, effect: Effect) {
        if let Some(done) = effect.on_platform(&self.backend) {
            if let Err(why) = done {
                self.problems.push(why);
            }
            return;
        }
        match effect {
            Effect::WhenDone(wanted) => self.session.when_done = wanted,
            Effect::CannotWhenDone => {
                self.said =
                    "This computer has no way to do that when the downloads are done.".into();
            }
            Effect::Failed { name, why, .. } => self
                .backend
                .platform
                .notify(t("Download failed"), &format!("{name} — {why}")),
            Effect::AddedAgain { name } => {
                self.said = english(
                    "Added {} again: that address was already in the list.",
                    1,
                    &[&name],
                );
            }
            Effect::Problem(why) => self.problems.push(why),
            Effect::Armed(wanted) => {
                self.said = english(
                    "The schedule has set what dm does when the downloads are done: {}.",
                    1,
                    &[&wanted.verb()],
                );
            }
            Effect::Quit => self.quit_asked = true,
            // Scheduler › Hang up modem when done, as the window does it (gaps row A7).
            Effect::Scheduled { hang_up } => self.hang_up(hang_up),
            // A window's: Add URL (`--new-download` asks for a window to add
            // in: there is none, and the order is not left for a later
            // launch), the progress and Download complete windows, a fresh
            // link asked for, what a screen reader hears.
            Effect::NewDownload
            | Effect::Telling { .. }
            | Effect::StartedElsewhere(_)
            | Effect::Finished(_)
            | Effect::Told { .. } => {}
            // Never asked with no window: "ask" waits, its reason said on the rows (gaps row
            // W11).
            Effect::AskMetered => {}
            // A followed playlist's or feed's reading came back: what is new added, as the
            // window adds it (gaps row W10).
            Effect::Followed { address, read } => self.take_followed(&address, read),
            // Carried on the platform, above.
            Effect::Notify { .. }
            | Effect::NotifyFile { .. }
            | Effect::Play(_)
            | Effect::Open(_)
            | Effect::Reveal(_) => {}
        }
    }

    /// Scheduler › Hang up modem when done, where the schedules asked for it (`hang_up`): off
    /// this thread, as Windows takes a moment to end a connection, as the window does it; what
    /// it came to is said when it is known.
    fn hang_up(&mut self, hang_up: bool) {
        if hang_up && crate::platform::dialup::CAN_HANG_UP {
            if let Some(hung) = dm_shell::dialup::hang_up_later(Arc::clone(&self.hung_up)) {
                self.say_hung_up(hung);
            }
        }
        let hung = dm_core::lock(&self.hung_up).take();
        if let Some(hung) = hung {
            self.say_hung_up(hung);
        }
    }

    /// Says what hanging up came to, in English: a connection that was not up needs no word,
    /// one Windows would not end is a problem.
    fn say_hung_up(&mut self, hung: dm_shell::dialup::HungUp) {
        use dm_shell::dialup::HungUp;
        match hung {
            HungUp::Done(name) => {
                self.said = english(
                    "The scheduled downloads are done: {} is hung up.",
                    1,
                    &[&name],
                );
            }
            HungUp::WasDown(_) => {}
            HungUp::NoneSet => {
                self.said = "Hang up modem when done: no connection is set in Options › \
                             Advanced › Dial Up / VPN."
                    .into();
            }
            HungUp::Failed(name, why) => {
                self.problems
                    .push(english("{} was not hung up: {}", 1, &[&name, &why]))
            }
        }
    }

    /// What the browser handed over, each added straight in: there is
    /// nobody to ask (`Asking::Nobody`).
    fn take_handed(&mut self) {
        for _ in 0..HAND_OFFS_AT_ONCE {
            let settings = self.backend.settings();
            let route = self.session.hands.next(
                &self.backend,
                &settings,
                &self.rows,
                Asking::Nobody,
                Instant::now(),
            );
            match route {
                Route::Nothing => return,
                Route::Again => {}
                Route::FreshLink { id, url } => self.take_fresh_link(id, &url),
                Route::AddLinks(items) => {
                    let added = self.session.add(&self.backend, items);
                    if let Some((_, e)) = added.partly() {
                        self.said = e.to_string();
                    }
                    self.read();
                }
                Route::Added(Ok(_)) => self.read(),
                Route::Added(Err(e)) => self.said = e,
                // Only where there is somebody to ask.
                Route::AskLinks(_) | Route::Ask(_) | Route::Listed { .. } => {}
            }
        }
    }

    /// The download caught in the browser becomes row `id`'s link, and the
    /// row goes on from where it stopped.
    fn take_fresh_link(&mut self, id: Id, url: &str) {
        self.session.hands.stop_awaiting_link();
        let failed = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .is_some_and(|r| matches!(r.state, dm_core::State::Failed(_)));
        match self.backend.set_url(id, url) {
            Ok(()) => {
                if failed {
                    self.backend.retry(id);
                }
                self.said = "The new link was taken from the browser; the download goes on.".into();
            }
            Err(e) => self.said = e,
        }
        self.read();
    }

    /// When done's countdown (sleep, shut down): its action carried out
    /// once its time is up.
    fn countdown(&mut self) {
        match self.session.countdown_due(&self.backend, Instant::now()) {
            Some(Ok(Carried::Quit)) => self.quit_asked = true,
            Some(Err(e)) => {
                self.problems.push(e.clone());
                self.said = e;
            }
            _ => {}
        }
    }

    /// "Delete file after": the files due go to the Trash, their rows with
    /// them.
    fn delete_due_files(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let Some(done) = dm_shell::app::session::delete_due(&self.backend, &self.rows, now) else {
            return;
        };
        for id in &done.gone {
            self.selected.remove(id);
        }
        if !done.failed.is_empty() {
            self.said = format!("Couldn’t delete {}", done.failed.join("; "));
        }
        self.read();
    }

    /// The watched folder: each file's links added.
    fn take_watched(&mut self) {
        let folder = self.backend.settings().downloads.watch_folder.clone();
        for links in self
            .session
            .look_at_watched(&folder, &self.rows, &self.backend)
        {
            let items = links
                .into_iter()
                .map(|url| (url, dm_core::AddOptions::default()))
                .collect();
            let _ = self.add_items(items);
        }
    }

    /// Adds downloads, each with its options: how many went in. Those that
    /// did not are said, not lost.
    fn add_items(&mut self, items: Vec<(String, dm_core::AddOptions)>) -> Result<usize, String> {
        let added = self.session.add(&self.backend, items);
        if let Some((failed, e)) = added.partly() {
            self.said = match failed {
                1 => format!("One of the links was not added: {e}"),
                n => format!("{n} of the links were not added: {e}"),
            };
        }
        self.read();
        added.outcome()
    }

    /// Takes in what finished saved searches found (their new files added
    /// for later; what they have seen kept), and starts any saved search
    /// whose time has come, looked at once a minute.
    fn take_searches(&mut self) {
        let done: Vec<_> = std::mem::take(&mut *dm_core::lock(&self.found));
        for (name, found) in done {
            match found {
                Ok(found) => self.take_found(&name, &found),
                Err(e) => self.problems.push(format!("the search {name} failed: {e}")),
            }
        }
        let now = Instant::now();
        if self
            .searches_checked
            .is_some_and(|at| now.duration_since(at) < SEARCHES_EVERY)
        {
            return;
        }
        self.searches_checked = Some(now);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let settings = self.backend.settings();
        let Some((due, wanted)) = dm_shell::app::search::due_now(&settings, epoch) else {
            return;
        };
        if let Err(e) = self.backend.set_settings(wanted) {
            self.problems.push(format!(
                "when the saved searches last ran could not be saved: {e}"
            ));
        }
        for mut search in due {
            // The login Options › Sign-ins keeps for the site, where it
            // keeps one.
            search.page = settings.with_login(&search.page);
            let found = Arc::clone(&self.found);
            let name = search.name.clone();
            dm_shell::app::search::crawl_later(&self.backend, search, move |crawled| {
                dm_core::lock(&found).push((name, crawled));
                dm_shell::app::changes::changed();
            });
        }
    }

    /// What the saved search `name` found: the new files added to the main
    /// queue for later (each in a folder that mirrors its address, where
    /// the search keeps the site's layout), and what it has seen kept.
    fn take_found(&mut self, name: &str, found: &dm_shell::lists::Crawled) {
        let settings = self.backend.settings();
        let saved = settings.site_searches.iter().find(|s| s.name == name);
        let seen: Vec<String> = saved.map(|s| s.seen.clone()).unwrap_or_default();
        let keep_layout = saved.is_some_and(|s| s.keep_layout);
        let finds = dm_shell::app::search::sort_finds(&found.files, &self.rows, &seen);
        let new = finds.new;
        let mut remember = finds.listed;
        if new.is_empty() {
            self.said = english("The search {} found nothing new.", 1, &[&name]);
        } else {
            let with = dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            };
            let items: Vec<(String, dm_core::AddOptions)> = if keep_layout {
                dm_shell::app::search::by_site_folder(&new, &settings.folders.download_dir)
                    .into_iter()
                    .flat_map(|(folder, group)| {
                        let mut here = with.clone();
                        if folder.is_some() {
                            here.save_dir = folder;
                        }
                        group.into_iter().map(move |u| (u, here.clone()))
                    })
                    .collect()
            } else {
                new.iter().map(|u| (u.clone(), with.clone())).collect()
            };
            let added = self.add_items(items);
            let now = dm_shell::app::search::listed(&self.rows);
            remember.extend(
                new.iter()
                    .map(|f| dm_shell::lists::bare(f))
                    .filter(|f| now.contains(f)),
            );
            match added {
                Ok(n) => {
                    self.said = english(
                        "Added {} download to {}.||Added {} downloads to {}.",
                        n as u64,
                        &[&n, &dm_core::MAIN_QUEUE],
                    )
                }
                Err(e) => self.problems.push(format!(
                    "the search {name} found files that could not be added: {e}"
                )),
            }
        }
        let settings = self.backend.settings();
        if let Some(wanted) = dm_shell::app::search::seen_saved(&settings, name, remember) {
            if let Err(e) = self.backend.set_settings(wanted) {
                self.problems.push(format!(
                    "what the search {name} has seen could not be saved: {e}"
                ));
            }
        }
    }

    /// The Recheck queue's counts, once its check is done.
    fn say_rechecked(&mut self) {
        if let Some((checked, changed)) = dm_core::lock(&self.rechecked).take() {
            self.said = english(
                "Checked {} finished file with its server. Changed since, and downloaded again: {}.||Checked {} finished files with their servers. Changed since, and downloaded again: {}.",
                checked as u64,
                &[&checked, &changed],
            );
        }
    }

    /// Whether dm has been asked to end.
    pub fn quit_asked(&self) -> bool {
        self.quit_asked
    }
}

#[cfg(test)]
mod tests;
