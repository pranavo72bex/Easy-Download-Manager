// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `--headless`: dm with no window, for a server, a script or an agent. The
//! queue, the browser's hand-offs, the watched folder, the saved searches,
//! the schedules, When done and the control protocol go on as they do under
//! a window, with nothing drawn and no toolkit started, so it runs where
//! there is no display. Each rule is the application's (`dm_shell::app`);
//! what a window would ask a person is settled as the egui window's
//! headless mode settles it: a hand-off is added straight in, Exit when
//! done is at once, a row the schedule holds waits, and what needs a window
//! (a dialog, the clipboard, a file opened) is refused, said why.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use dm_core::{Entry, Id};
use dm_shell::app::backend::Backend;
use dm_shell::app::commands::{self, Command, Ran};
use dm_shell::app::control_api;
use dm_shell::app::detect::Detected;
use dm_shell::app::handoff::{Asking, Route};
use dm_shell::app::names::name_of;
use dm_shell::app::places::{Place, Places};
use dm_shell::app::session::{Carried, Fired, Session};
use serde_json::{json, Value};

use crate::boot::Wake;
use crate::control::Request;
use crate::i18n::{fill, form_of, t, tf};
use crate::probe::{self, WindowProbe};

/// How often dm looks at its clock when nothing wakes it: the schedules,
/// When done's countdown, the watched folder and the saved searches.
const CLOCK: Duration = Duration::from_secs(1);

/// Hand-offs taken in one look: with nobody to ask, each goes straight in,
/// so a batch is not left waiting a second a link.
const HAND_OFFS_AT_ONCE: usize = 256;

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
    /// What the reads detected, for the next look (notifications, sounds).
    detected: Vec<Detected>,
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
}

/// Runs dm with no window until it is asked to end (Exit over the port,
/// When done's Exit, a signal): woken by the engine and the control port
/// (`woken`), and by its clock once a second. The queue is written down
/// and the port forgotten before it returns.
pub fn run(
    backend: Backend,
    requests: Option<mpsc::Receiver<Request>>,
    assistants_only: bool,
    woken: &mpsc::Receiver<Wake>,
) {
    let mut dm = Headless::new(backend, requests, assistants_only);
    while !crate::signals::SIGNALLED.load(Ordering::SeqCst) {
        dm.tick();
        if dm.quit_asked {
            break;
        }
        match woken.recv_timeout(CLOCK) {
            // Wakes within the quarter are one.
            Ok(Wake::Soon) => std::thread::sleep(Duration::from_millis(250)),
            Ok(Wake::Now) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(CLOCK),
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
            detected: Vec::new(),
            requests,
            assistants_only,
            quit_asked: false,
            last_limit: 0,
            found: Arc::default(),
            searches_checked: None,
            rechecked: Arc::default(),
        };
        dm.last_limit = commands::remembered_limit(&dm.backend.settings());
        dm.read();
        dm
    }

    /// One look: the list read, what it detected told, the schedules, When
    /// done, the hand-offs, the countdown, "Delete file after", the watched
    /// folder, the saved searches, and the port's requests answered.
    fn tick(&mut self) {
        let home = dm_shell::settings::home();
        self.read();
        // `--new-download` asks for a window to add in: there is none, and
        // the order is not left for a later launch.
        let _ = dm_shell::app::session::new_download_asked(&home);
        if let Some(wanted) = dm_shell::app::session::when_done_asked(&home) {
            if wanted.possible(crate::power::available) {
                self.session.when_done = wanted;
            } else {
                self.said =
                    "This computer has no way to do that when the downloads are done.".into();
            }
        }
        self.told();
        self.schedule_hooks();
        if self.when_queue_dry() {
            self.quit_asked = true;
            return;
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
        self.detected.push(read.detected);
        self.problems.extend(read.problems);
        crate::rows::set_retrying(read.snapshot.retrying);
        crate::rows::set_swarms(read.snapshot.swarms);
        crate::rows::set_limit_holds(read.snapshot.allowance_spent);
        self.rows = read.snapshot.rows;
        let mut categories = settings.categories();
        let before = dm_shell::app::names::earlier_of_the_same_name(&self.rows);
        self.names = self
            .rows
            .iter()
            .map(|row| {
                let name = dm_shell::app::names::name_it_will_get_after(
                    &self.rows,
                    row,
                    &settings.download_dir,
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

    /// What the reads detected: a notification for each download that
    /// failed and one for what finished (a batch is one), the sounds
    /// chosen, and a hand-off's address already listed said.
    fn told(&mut self) {
        let detected = std::mem::take(&mut self.detected);
        let settings = self.backend.settings();
        let mut finished: Vec<(String, String)> = Vec::new();
        let mut failed = false;
        for read in &detected {
            for row in &self.rows {
                if read.failed.contains(&row.id) {
                    failed = true;
                    self.backend.platform.notify(
                        t("Download failed"),
                        &format!("{} — {}", name_of(row), crate::rows::status_of(row)),
                    );
                }
                if read.finished.contains(&row.id) {
                    finished.push((name_of(row), row.path.clone()));
                }
            }
            if let Some(row) = read
                .added_again
                .and_then(|id| self.rows.iter().find(|r| r.id == id))
            {
                self.said = english(
                    "Added {} again: that address was already in the list.",
                    1,
                    &[&name_of(row)],
                );
            }
        }
        match finished.as_slice() {
            [] => {}
            [(name, path)] => self
                .backend
                .platform
                .notify_file(t("Download finished"), name, path),
            many => self.backend.platform.notify(
                t("Download finished"),
                &tf("{} downloads finished.", &[&many.len()]),
            ),
        }
        if settings.sound_on_complete && !finished.is_empty() {
            match self.sound("complete") {
                Some(path) => self.backend.platform.play_file(&path),
                None => self.backend.platform.chime(),
            }
        }
        if settings.sound_on_failure && failed {
            match self.sound("failed") {
                Some(path) => self.backend.platform.play_file(&path),
                None => self.backend.platform.alert(),
            }
        }
    }

    /// The schedules (`Session::schedule_tick`): a once-only date that has
    /// passed is put away, and what the schedule armed for When done said.
    fn schedule_hooks(&mut self) {
        let settings = self.backend.settings();
        if let Some(wanted) = crate::held::spent_once_now(&settings) {
            if let Err(why) = self.backend.set_settings(wanted) {
                self.problems.push(why);
            }
        }
        let settings = self.backend.settings();
        let tick = self.session.schedule_tick(
            &self.backend,
            &settings,
            &self.rows,
            crate::power::available,
        );
        if let Some(wanted) = tick.armed {
            self.said = english(
                "The schedule has set what dm does when the downloads are done: {}.",
                1,
                &[&wanted.verb()],
            );
        }
        self.problems.extend(tick.problems);
    }

    /// A queue that has run dry has finished: its own sound, where one was
    /// chosen, and When done, with nobody to see a countdown for Exit.
    /// Whether dm is to end now.
    fn when_queue_dry(&mut self) -> bool {
        if !self.session.ran_dry(&self.rows) {
            return false;
        }
        if let Some(path) = self.sound("queue_done") {
            self.backend.platform.play_file(&path);
        }
        self.session.when_done_fires(true, Instant::now()) == Fired::QuitNow
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
            self.said = format!("Could not delete {}", done.failed.join("; "));
        }
        self.read();
    }

    /// The watched folder: each file's links added.
    fn take_watched(&mut self) {
        let folder = self.backend.settings().watch_folder.clone();
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
            .is_some_and(|at| now.duration_since(at) < Duration::from_secs(60))
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
                dm_shell::app::search::by_site_folder(&new, &settings.download_dir)
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

    /// Answers what the control port asked, in order: who may ask as the
    /// application says, then [`Self::answer`].
    fn answer_requests(&mut self) {
        let asked: Vec<Request> = match &self.requests {
            Some(requests) => requests.try_iter().collect(),
            None => return,
        };
        let allow_mcp = self.backend.settings().allow_mcp;
        for request in asked {
            let answer =
                match control_api::refused(request.via_mcp, self.assistants_only, allow_mcp) {
                    Some(why) => Err(why),
                    None => self.answer(&request.method, &request.params),
                };
            let _ = request.reply.send(answer);
        }
    }

    /// One control request: a screenshot is refused (there is no window to
    /// picture), a step says there is no window, and the view, the
    /// commands and the application answer as under a window.
    pub fn answer(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        if method == "ui.screenshot" {
            return Err(probe::NO_WINDOW_TO_PICTURE.into());
        }
        if let Some(answer) = probe::input(&mut Probe(self), method, params) {
            return answer;
        }
        self.answer_inside(method, params)
    }

    /// What a `batch` reaches as well: the view and the application.
    fn answer_inside(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        if let Some(answer) = probe::view(&mut Probe(self), method, params) {
            return answer;
        }
        if let Some(answer) = control_api::answer(&self.backend, &self.rows, method, params) {
            self.read();
            return answer;
        }
        match method {
            "downloads.add" => {
                let settings = self.backend.settings();
                let asked = control_api::add_asked(&self.rows, &settings, params)?;
                let added = if asked.fresh.is_empty() {
                    0
                } else {
                    let items = asked
                        .fresh
                        .iter()
                        .map(|url| (url.clone(), asked.with.clone()))
                        .collect();
                    self.add_items(items)?
                };
                Ok(json!({ "added": added, "already_listed": asked.listed }))
            }
            // An exact ceiling in KB/s; 0 is no limit. A script's change
            // keeps what makes dm run something.
            "downloads.speed_limit" => {
                let (wanted, replaced) =
                    control_api::speed_limit(&self.backend.settings(), params)?;
                if let Some(limit) = replaced {
                    self.last_limit = limit;
                }
                let (now, _) = self.backend.set_settings_from(wanted, true);
                self.session.forget_limit();
                Ok(json!({ "speed_limit_kb": now?.speed_limit }))
            }
            "batch" => {
                control_api::batch(params, |method, params| self.answer_inside(method, params))
            }
            other => Err(format!("no method is called {other}")),
        }
    }

    /// Carries out `command` where there is no window: the application's
    /// own (a row's verbs, the queues, the speed limiter, When done, Exit);
    /// what needs a window is refused, said why.
    fn run(&mut self, command: Command) -> Result<(), String> {
        let chosen: Vec<Entry> = self.chosen().into_iter().cloned().collect();
        if let Some(wanted) = commands::when_done(command) {
            self.session.when_done = wanted;
            return Ok(());
        }
        match command {
            // Nobody to ask "Exit while downloading?": over the port, dm
            // ends.
            Command::Quit => self.quit_asked = true,
            Command::SelectAll => {
                self.selected = self.shown().iter().map(|r| r.id).collect();
            }
            Command::SpeedLimiterToggle
            | Command::SpeedLow
            | Command::SpeedMedium
            | Command::SpeedHigh
            | Command::SpeedUnlimited => {
                let now = self.backend.settings();
                let wanted = commands::limited(command, &now, &mut self.last_limit).unwrap_or(now);
                let after = self.backend.set_settings(wanted)?;
                self.said = if after.speed_limit > 0 {
                    english("Speed limiter on: {} KB/s.", 1, &[&after.speed_limit])
                } else {
                    "Speed limiter off.".into()
                };
                self.session.forget_limit();
            }
            Command::StartQueue | Command::StopQueue => self.run_on_queue(command),
            _ => {
                let queue = self.current_queue();
                let ran =
                    commands::verb(command, &self.backend, &chosen, &queue).ok_or_else(|| {
                        format!("{} needs dm's window: dm is running headless", command.id())
                    })?;
                self.read();
                match ran {
                    Ran::Cleared(n) => {
                        self.said = english(
                            "Deleted {} completed or cancelled download from the list.||Deleted {} completed or cancelled downloads from the list.",
                            n as u64,
                            &[&n],
                        );
                    }
                    // Resume on rows the schedule or the download limit
                    // holds: each of the others went on; these wait, with
                    // nobody to ask whether to start them outside it.
                    Ran::WentOn { held } if !held.is_empty() => {
                        self.said = "Waiting for the schedule or speed limit.".into();
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Start Queue or Stop Queue on the place's queue, and said.
    fn run_on_queue(&mut self, command: Command) {
        let queue = self.current_queue();
        let ran = commands::verb(command, &self.backend, &[], &queue);
        self.read();
        match (command, ran) {
            (Command::StartQueue, ran) => {
                let n = match ran {
                    Some(Ran::QueueStarted(n)) => n,
                    _ => 0,
                };
                if n > 0 {
                    if let Some(path) = self.sound("queue_started") {
                        self.backend.platform.play_file(&path);
                    }
                }
                let settings = self.backend.settings();
                let schedule = settings.schedule_for(&queue);
                let lets = !schedule.enabled || schedule.open();
                self.said = if n == 0 {
                    english("Nothing in {} was waiting to start.", 1, &[&queue])
                } else if !lets {
                    english(
                        "{} is waiting for the schedule; it starts by itself when the schedule opens.",
                        1,
                        &[&queue],
                    )
                } else {
                    english(
                        "Started {} download in {}.||Started {} downloads in {}.",
                        n as u64,
                        &[&n, &queue],
                    )
                };
                if queue == dm_core::RECHECK_QUEUE {
                    commands::recheck(&self.backend, Arc::clone(&self.rechecked));
                }
            }
            (_, ran) => {
                let n = match ran {
                    Some(Ran::QueueStopped(n)) => n,
                    _ => 0,
                };
                self.said = english(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &queue],
                );
            }
        }
    }

    /// Whether dm has been asked to end.
    pub fn quit_asked(&self) -> bool {
        self.quit_asked
    }
}

/// dm with no window as the control port's [`WindowProbe`]: the view and
/// its commands answer; there is nothing to click, picture or move.
struct Probe<'a>(&'a mut Headless);

impl WindowProbe for Probe<'_> {
    fn may_step(&mut self) -> Result<(), String> {
        Err(NO_WINDOW.into())
    }

    fn has_window(&self) -> bool {
        false
    }

    /// Nothing is on a screen.
    fn texts(&mut self) -> Result<probe::Texts, String> {
        Ok(probe::Texts::new(Vec::new()))
    }

    fn click(&mut self, _: probe::Click) -> Result<probe::Clicked, String> {
        Err(NO_WINDOW.into())
    }

    fn scroll(&mut self, _: probe::Scroll) -> Result<probe::Scrolled, String> {
        Err(NO_WINDOW.into())
    }

    fn type_text(&mut self, _: &str) -> Result<usize, String> {
        Err(NO_WINDOW.into())
    }

    fn key(&mut self, _: probe::KeyPress) -> Result<(), String> {
        Err(NO_WINDOW.into())
    }

    fn accessibility(&mut self) -> Result<Value, String> {
        Ok(json!([]))
    }

    fn windows(&mut self) -> Result<probe::Windows, String> {
        Ok(probe::Windows {
            pixels_per_point: None,
            main: None,
            dialogs: Vec::new(),
        })
    }

    fn window_to(&mut self, _: probe::WindowTo) -> Result<(), String> {
        Err("there is no window".into())
    }

    fn move_window(&mut self, _: probe::MoveWindow) -> Result<probe::Moved, String> {
        Err("there is no window to move".into())
    }

    fn select(&mut self, ids: Vec<u64>) -> Result<Vec<u64>, String> {
        let listed: std::collections::HashSet<u64> = self.0.rows.iter().map(|r| r.id).collect();
        self.0.selected = ids.into_iter().filter(|id| listed.contains(id)).collect();
        Ok(self.0.selected.iter().copied().collect())
    }

    fn place(&mut self, place: &str) -> Result<(), String> {
        let customs: Vec<String> = self
            .0
            .backend
            .settings()
            .custom_categories
            .iter()
            .map(|c| c.name.clone())
            .collect();
        self.0.place = Place::from_word(place, &self.0.backend.queues(), &customs)?;
        Ok(())
    }

    fn inspect(&mut self) -> Result<probe::Inspect, String> {
        let dm = &*self.0;
        let rows = dm
            .shown()
            .into_iter()
            .map(|row| probe::InspectRow {
                id: row.id,
                name: dm
                    .names
                    .get(&row.id)
                    .cloned()
                    .unwrap_or_else(|| name_of(row)),
                status: crate::rows::status_in_english(row),
                queue: row.queue.clone(),
                description: row.description.clone(),
                url: crate::format::shown_url(&row.url),
                rect: None,
            })
            .collect();
        Ok(probe::Inspect {
            place: format!("{:?}", dm.place),
            selected: dm.selected.iter().copied().collect(),
            sort: None,
            categories: false,
            toolbar: false,
            dark: false,
            hidden: true,
            said: dm.said.clone(),
            problems: dm.problems.clone(),
            dialogs: Vec::new(),
            rows,
        })
    }

    fn commands(&mut self) -> Vec<probe::CommandState> {
        Command::ALL
            .iter()
            .map(|c| probe::CommandState {
                id: c.id(),
                label: c.label(),
                enabled: self.0.enabled(*c),
            })
            .collect()
    }

    fn run_command(&mut self, id: &str) -> Result<probe::Ran, String> {
        let command = Command::from_id(id).ok_or_else(|| format!("no command is called {id}"))?;
        // Opening a downloaded file is a person's click in the window.
        if let Some(why) = control_api::person_only(command.id()) {
            return Err(why);
        }
        if !self.0.enabled(command) {
            return Err(format!("{id} has nothing to act on"));
        }
        self.0.run(command)?;
        Ok(probe::Ran {
            ran: id.to_string(),
            said: self.0.said.clone(),
        })
    }
}

#[cfg(test)]
mod tests;
