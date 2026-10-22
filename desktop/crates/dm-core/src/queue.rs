//! Several downloads, a limit on how many run at once, and the controls to
//! move them between those states.
//!
//! The limit is the point. A machine asked to fetch forty files at once
//! finishes none of them sooner and every one of them later, because the link
//! is shared and the seeks are not; holding most of them back is what makes
//! the first ones arrive.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::category::Category;
use crate::checksum::Checksum;
use crate::control::Control;
use crate::download::{download_reporting, Options};
use crate::progress::Progress;
use crate::schedule::Schedule;
use crate::Error;

/// Seconds since the epoch, for the history a finished row becomes.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Identifies one download for as long as the queue remembers it.
pub type Id = u64;

/// Where a download has got to.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "reason", rename_all = "lowercase")]
pub enum State {
    Queued,
    Running,
    Paused,
    Completed,
    /// Carries the reason, because a row saying only "failed" sends people
    /// looking in the wrong place.
    Failed(String),
    Cancelled,
}

impl State {
    /// Whether this download still wants the queue's attention.
    pub fn settled(&self) -> bool {
        matches!(self, State::Completed | State::Failed(_) | State::Cancelled)
    }

    /// Whether "Clear finished" takes this row. A failed row is not
    /// finished: its staged parts are what Retry resumes from, so it stays,
    /// parts and all, until a person removes it on purpose.
    pub fn clearable(&self) -> bool {
        matches!(self, State::Completed | State::Cancelled)
    }
}

/// One row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub id: Id,
    pub url: String,
    pub state: State,
    pub progress: Progress,
    /// Empty until the probe has learned one.
    pub filename: String,
    /// When it finished, succeeded or failed — this is what turns a queue
    /// into a history.
    #[serde(default)]
    pub finished_at: Option<u64>,
    /// What it was sorted as. Known only once it has been assembled.
    #[serde(default)]
    pub category: Option<Category>,
    /// A digest it is expected to have, if one was given.
    #[serde(default)]
    pub checksum: Option<Checksum>,
    /// Whether that digest was checked and matched.
    #[serde(default)]
    pub verified: bool,
    /// How many mirrors the bytes came from. More than one only where a
    /// Metalink named others that agreed about the file.
    #[serde(default)]
    pub mirrors: usize,
    /// Where the finished file was written.
    #[serde(default)]
    pub path: String,

    /// The page a browser said this came from, kept so the engine can send it
    /// as `Referer` — a hotlink-protected file is served only to a request
    /// that names its page. Empty for a download typed in by hand.
    #[serde(default)]
    pub referrer: String,
    /// Whether the schedule (or a spent download limit) stopped this one
    /// rather than a person. Only
    /// these are started again when the window reopens: a download somebody
    /// paused deliberately should stay paused through the night.
    ///
    /// On the row, so it is saved with it: kept beside it, it was lost to a
    /// restart, and a queue the schedule paused overnight came back as one
    /// a person had paused — never started again by the window opening.
    #[serde(default)]
    pub paused_by_schedule: bool,

    /// A person's note on the row: the Note column.
    #[serde(default)]
    pub description: String,
    /// The named queue this row waits in — "Main queue" and
    /// the rest. Empty is none: a download started now rather than later.
    #[serde(default)]
    pub queue: String,
    /// Where to save this one, if not the download folder: the folder chosen
    /// in the "Download details" dialog. Overrides category sorting.
    #[serde(default)]
    pub save_dir: Option<String>,
    /// The name to save it under, if a person chose one over the server's.
    #[serde(default)]
    pub save_name: Option<String>,
    /// The server's ETag or Last-Modified for the file fetched, which the
    /// recheck queue compares against to tell whether it changed.
    #[serde(default)]
    pub validator: Option<String>,
    /// The quality picked for this one in the browser's video panel, in
    /// `Quality::named`'s words; `None` is the setting's.
    #[serde(default)]
    pub quality: Option<String>,
    /// Where this row's parts are staged, recorded when it first started,
    /// so the folder in the settings can change without stranding them.
    /// Empty until then.
    #[serde(default)]
    pub parts_dir: String,
    /// The download folder in force when it first started, kept so a
    /// download begun goes on into the folder it began in: one running when
    /// the folder was changed in Options finished in the new one (UI round
    /// 18). Empty until then, and unused where `save_dir` names a folder.
    #[serde(default)]
    pub began_in: String,
    /// The `Cookie` header the browser handed over with this download, for
    /// a site a person switched that on for; sent on every request of this
    /// row's, so a resume reaches the server signed in. Empty for none.
    #[serde(default)]
    pub cookies: String,
    /// The cookies were for this download alone (a video page's session
    /// the browser sends with every video): dropped once it completes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cookies_once: bool,
    /// The finished file a recheck found changed on its server, kept until
    /// the new version is whole and then replaced by it. Fetched beside it
    /// rather than after deleting it: a server that fails halfway through
    /// the new version left a person with neither.
    #[serde(default)]
    pub replaces: Option<String>,
    /// This download's own speed limit in KB/s, beside the one every
    /// download shares; zero for none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub speed_limit: u64,
    /// Why a paused row stopped by itself: its disk filled, or its folder's
    /// drive went. Empty for any other pause. It goes on, from the parts it
    /// has, once there is room, or from another folder (`relocate`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stalled: String,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// How one connection of a download is getting on: its range of the file
/// and how much of it is on disk. Read from the staged parts themselves, so
/// it costs the transfer nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub start: u64,
    pub end: u64,
    pub done: u64,
    /// Its part file is there: the server has answered for this range, so
    /// it is under way though its first bytes may still be in a buffer.
    pub started: bool,
}

/// What a row is added with, beyond its address: what the "Download details"
/// dialog asks before a download starts.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub referrer: String,
    pub description: String,
    pub queue: String,
    pub save_dir: Option<String>,
    pub save_name: Option<String>,
    /// "Add for later": added paused, waiting for its queue to start.
    pub later: bool,
    /// The quality picked in the browser's video panel, in
    /// `Quality::named`'s words; `None` is the setting's.
    pub quality: Option<String>,
    /// The browser's cookies for this download's site, as a `Cookie`
    /// header, where the person switched that on for the site.
    pub cookies: String,
    /// Those cookies are for this download alone, not kept once it is done.
    pub cookies_once: bool,
    /// A file this download takes the place of once it is whole, as a
    /// recheck's new version does: kept until then, so a failed download
    /// leaves it.
    pub replaces: Option<String>,
    /// The digest the file must have, set before it can start.
    pub checksum: Option<crate::checksum::Checksum>,
}

/// Named queues that start on their own each day: each with its minute of
/// the local day, and the day each last started.
#[derive(Default)]
struct QueueStarts {
    at: Vec<(String, u32)>,
    fired: HashMap<String, i64>,
}

struct Slot {
    entry: Entry,
    control: Control,
    /// The task running this download's last start, until it has written
    /// its outcome. A row is not started again while it is still there: a
    /// pause is recorded the moment it is asked for, but the task honours it
    /// at its next chunk, and a resume before then would put a second writer
    /// on the same part files — and the first task's outcome, landing late,
    /// would overwrite the second's state.
    task: Option<tokio::task::JoinHandle<()>>,
    /// Started by a person outside the schedule (`start_outside_schedule`):
    /// the schedule and the download limit leave this run alone.
    outside_schedule: bool,
    /// The attempt a range of this run is on after a failure, and how many
    /// there may be (0 for no end), and what failed, until bytes arrive
    /// again. Not saved: it is about this run.
    retrying: Option<(u32, u32, String)>,
    /// This row's own speed limit (`Entry::speed_limit`), chained to the
    /// shared one while it runs, so a change reaches it at once.
    own_limit: crate::limit::Limiter,
}

impl Slot {
    /// Whether a previous run is still winding down.
    fn still_stopping(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    /// Stopped, keeping what it has. The twin of `requeue`, and the one
    /// transition behind pause, pause-all and the schedule closing — three
    /// copies of the same three lines, until one of them drifted.
    fn hold(&mut self, by_schedule: bool) {
        self.control.pause();
        self.entry.state = State::Paused;
        self.entry.paused_by_schedule = by_schedule;
        self.outside_schedule = false;
    }
}

impl Slot {
    /// Back in line, whatever stopped it.
    ///
    /// The one transition behind resume, resume-all, retry and the schedule
    /// lifting: a fresh control, because the old one is still held by the
    /// task that stopped and clearing its flag would tell that task to carry
    /// on; and no longer the schedule's, because a person asked.
    fn requeue(&mut self) {
        self.control = Control::new();
        self.entry.state = State::Queued;
        self.entry.finished_at = None;
        self.entry.paused_by_schedule = false;
        self.entry.stalled.clear();
        self.outside_schedule = false;
    }
}

struct Inner {
    slots: Mutex<HashMap<Id, Slot>>,
    /// Insertion order, so the queue is served in the order it was given.
    order: Mutex<Vec<Id>>,
    next_id: AtomicU64,
    /// Changeable while downloads are running: a setting that only applies
    /// after a restart is one people assume is broken. Running transfers keep
    /// the options they started with; the next one to start picks these up.
    options: Mutex<Options>,
    /// Where the queue is written, if anywhere. Tests leave it unset.
    store: Mutex<Option<PathBuf>>,
    /// Held from reading the rows to renaming the file, so an older snapshot
    /// cannot be written after a newer one.
    saving: Mutex<()>,
    concurrency: AtomicUsize,
    /// Fewer files at once for a named queue than `concurrency` allows in
    /// all: a queue of one for a site that takes one download at a time.
    queue_limits: std::sync::Mutex<HashMap<String, usize>>,
    /// Whether a scheduler is already serving this queue.
    scheduling: AtomicBool,
    /// Whether the recheck queue's servers are being asked now.
    rechecking: AtomicBool,
    /// When the queue may run.
    ///
    /// A plain mutex, not tokio's: deciding whether a window is open touches
    /// no I/O and awaits nothing, and being able to set this without an
    /// async context is what lets the shell put the saved schedule in place
    /// before the queue starts serving anything. Spawning that as a task
    /// raced the scheduler, and a scheduled queue would run at launch.
    schedule: std::sync::Mutex<Schedule>,
    /// How much may be downloaded per period, and how much has been.
    quota: std::sync::Mutex<crate::quota::Meter>,
    /// When the download limit's use was last written beside the queue.
    quota_saved: std::sync::Mutex<Option<std::time::Instant>>,
    /// How often the recheck queue asks its servers again on its own, and
    /// when it last did (or was told to start counting from).
    /// The last time is milliseconds since 1970, kept beside the queue
    /// file: a clock that started again at every launch never reached a
    /// day for anyone who quits dm at night, and a monotonic clock stops
    /// while the computer sleeps.
    recheck: std::sync::Mutex<(Option<Duration>, u64)>,
    /// Named queues that start on their own each day at a minute of the
    /// local day, and the day each last did.
    starts: std::sync::Mutex<QueueStarts>,
    /// Replaceable, because changing the proxy means a different client and
    /// a preference that needed a restart is one people assume is broken.
    /// Running downloads keep the client they started with.
    client: Mutex<reqwest::Client>,
    /// What went wrong that no row can carry: the queue file that could not
    /// be written, the parts that could not be removed. Printed to stderr
    /// these reached nobody — a Dock-launched app has no stderr — while the
    /// window showed the state the failure had just failed to record. Held
    /// until something asks, then handed over once.
    problems: std::sync::Mutex<Vec<String>>,
    /// The runs of rows deleted while they ran, with the queue each was in,
    /// until they have let go: still holding a connection, they count
    /// against the limit as a paused row's run does. Dropped with the row,
    /// they let a limit of one run two.
    leaving: std::sync::Mutex<Vec<(String, tokio::task::JoinHandle<()>)>>,
    /// How many times the queue file has been written, for a test to hold
    /// a batch to one save.
    saves: AtomicU64,
    /// Told whenever the queue changes, so a window that is not polling
    /// draws the change: a hand-off arriving, the schedule starting a row, a
    /// row finishing.
    changed: std::sync::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// A queue of downloads.
///
/// Cheap to clone; every clone talks to the same queue.
#[derive(Clone)]
pub struct Queue {
    inner: Arc<Inner>,
}

/// Whether the queue will take this URL: something the HTTP engine can fetch,
/// or the FTP, torrent or live-stream one can. Anything else is refused before it
/// becomes a row that could only ever fail.
///
/// Public so the native host can answer a browser with the same rule: it
/// told the browser "started" for a `javascript:` or `ftp:` link the queue
/// then dropped without a word, and the row sat at "handed" for good.
pub fn accepted(url: &str) -> bool {
    // A scheme is case-insensitive, as the extension reads it: an
    // `HTTP://` link it hands over is not then refused here.
    let scheme = |prefix: &str| {
        url.get(..prefix.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
    };
    scheme("http://")
        || scheme("https://")
        || crate::ftp::is_ftp(url)
        || crate::torrent::is_torrent(url)
        || crate::live::is_live(url)
}

impl Queue {
    pub fn new(client: reqwest::Client, options: Options, concurrency: usize) -> Self {
        Queue {
            inner: Arc::new(Inner {
                slots: Mutex::new(HashMap::new()),
                order: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                options: Mutex::new(options),
                store: Mutex::new(None),
                saving: Mutex::new(()),
                problems: std::sync::Mutex::new(Vec::new()),
                leaving: std::sync::Mutex::new(Vec::new()),
                scheduling: AtomicBool::new(false),
                rechecking: AtomicBool::new(false),
                concurrency: AtomicUsize::new(concurrency.max(1)),
                queue_limits: std::sync::Mutex::new(HashMap::new()),
                schedule: std::sync::Mutex::new(Schedule::default()),
                quota: std::sync::Mutex::new(crate::quota::Meter::default()),
                quota_saved: std::sync::Mutex::new(None),
                recheck: std::sync::Mutex::new((None, now_millis())),
                starts: std::sync::Mutex::new(QueueStarts::default()),
                client: Mutex::new(client),
                saves: AtomicU64::new(0),
                changed: std::sync::Mutex::new(None),
            }),
        }
    }

    /// Where a row's parts are staged: the folder it was first started
    /// under, so a parts folder changed in the settings afterwards strands
    /// nothing; the current setting's for a row not yet started.
    async fn parts_dir_of(&self, id: Id) -> PathBuf {
        let recorded = self
            .inner
            .slots
            .lock()
            .await
            .get(&id)
            .map(|s| s.entry.parts_dir.clone())
            .filter(|p| !p.is_empty());
        match recorded {
            Some(dir) => PathBuf::from(dir),
            None => self
                .inner
                .options
                .lock()
                .await
                .parts_dir
                .join(id.to_string()),
        }
    }

    /// Whether a scheduler is already serving this queue.
    ///
    /// Only worth asking to avoid starting one that would immediately find
    /// it had nothing to do.
    pub fn scheduling(&self) -> bool {
        self.inner.scheduling.load(Ordering::SeqCst)
    }

    /// Adds a URL and returns the id it will be known by, or `None` when that
    /// is not something dm can fetch.
    ///
    /// Checked here because this is the one door. The window checked and the
    /// browser hand-off did not, so anything a browser sent became a row that
    /// could only ever fail.
    pub async fn add(&self, url: &str) -> Option<Id> {
        let url = url.trim();
        if !accepted(url) {
            return None;
        }
        let id = self.insert(url, String::new()).await;
        self.save().await;
        Some(id)
    }

    /// Adds a download a browser handed over, keeping the page it came from.
    ///
    /// The referrer is why this is separate from `add`: a hotlink-protected
    /// file is served only to a request that names its page, and a browser
    /// knows the page where dm cannot. An empty referrer is the same as none.
    pub async fn add_from(&self, url: &str, referrer: &str) -> Option<Id> {
        let url = url.trim();
        if !accepted(url) {
            return None;
        }
        let id = self.insert(url, referrer.trim().to_string()).await;
        self.save().await;
        Some(id)
    }

    /// Adds several at once, writing the queue once rather than once each.
    ///
    /// Pasting a list is the ordinary way people add more than one, and
    /// `save` serialises every row — so twenty URLs meant twenty writes of a
    /// file that grew with each.
    pub async fn add_all(&self, urls: &[&str]) -> Vec<Id> {
        let mut ids = Vec::new();
        for url in urls {
            let url = url.trim();
            if accepted(url) {
                ids.push(self.insert(url, String::new()).await);
            }
        }
        if !ids.is_empty() {
            self.save().await;
        }
        ids
    }

    async fn insert(&self, url: &str, referrer: String) -> Id {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let slot = Slot {
            entry: Entry {
                id,
                url: url.to_string(),
                state: State::Queued,
                progress: Progress {
                    downloaded: 0,
                    total: None,
                    speed: 0.0,
                    eta: None,
                },
                filename: String::new(),
                finished_at: None,
                category: None,
                checksum: None,
                verified: false,
                mirrors: 0,
                path: String::new(),
                referrer,
                paused_by_schedule: false,
                description: String::new(),
                queue: String::new(),
                save_dir: None,
                save_name: None,
                validator: None,
                quality: None,
                parts_dir: String::new(),
                began_in: String::new(),
                cookies: String::new(),
                cookies_once: false,
                replaces: None,
                speed_limit: 0,
                stalled: String::new(),
            },
            control: Control::new(),
            task: None,
            outside_schedule: false,
            retrying: None,
            own_limit: crate::limit::Limiter::unlimited(),
        };

        self.inner.slots.lock().await.insert(id, slot);
        self.inner.order.lock().await.push(id);
        id
    }

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

    /// Where the recheck queue's last run is kept: beside the queue file.
    async fn recheck_file(&self) -> Option<PathBuf> {
        let store = self.inner.store.lock().await.clone()?;
        Some(store.with_file_name("recheck-at"))
    }

    /// Reads a stored queue, if one is readable.
    ///
    /// A queue file that cannot be parsed is not fatal — the app opening
    /// with an empty list is recoverable, and refusing to start over a bad
    /// file is not — but it is not overwritten either. The first save used
    /// to write the empty list over it, so a release that changed a field
    /// destroyed every queue on first launch, and the parts those rows
    /// pointed at were orphaned for good. It is moved aside and said.
    async fn restore(&self, path: &std::path::Path) -> Result<(), String> {
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
    async fn save(&self) {
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
    fn save_quota(&self, queue_file: &std::path::Path) {
        let kept = crate::lock(&self.inner.quota).kept();
        *crate::lock(&self.inner.quota_saved) = Some(std::time::Instant::now());
        let _ = crate::write_atomic_unsynced(&queue_file.with_file_name("quota"), kept.as_bytes());
    }

    /// The same, at most every ten seconds, from the progress of a transfer.
    async fn save_quota_now_and_then(&self) {
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
    fn note(&self, problem: String) {
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
    async fn discard_parts(&self, id: Id) {
        let parts = self.parts_dir_of(id).await;
        self.discard_parts_at(&parts).await;
    }

    /// Removes a row's staging folder, where it is known.
    async fn discard_parts_at(&self, parts: &std::path::Path) {
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

    /// What to call whenever the queue changes. One at a time: a second
    /// replaces the first.
    pub fn on_change(&self, tell: impl Fn() + Send + Sync + 'static) {
        *crate::lock(&self.inner.changed) = Some(Arc::new(tell));
    }

    /// How many may run at once.
    pub fn concurrency(&self) -> usize {
        self.inner.concurrency.load(Ordering::Relaxed)
    }

    /// Changes how many may run at once.
    ///
    /// Lowering it does not stop anything already running — a setting that
    /// killed transfers to obey itself would lose bytes to a preference.
    /// The excess drains as those finish.
    /// How many files each named queue runs at once, where fewer than the
    /// whole queue's limit; unnamed queues run up to that limit.
    /// Named queues to start on their own each day, at a minute past local
    /// midnight. Which already started today is kept across a change.
    pub fn set_queue_starts(&self, starts: Vec<(String, u32)>) {
        let mut held = crate::lock(&self.inner.starts);
        held.at = starts
            .into_iter()
            .filter(|(name, at)| !name.trim().is_empty() && *at < 24 * 60)
            .collect();
    }

    /// The queues whose start time has come on `day` (any count of days)
    /// at `minute` past midnight, each once a day: within five minutes of
    /// its time, so a loop that slept a little late still starts it, and
    /// a computer that slept through it does not start it hours after.
    pub fn starts_due(&self, day: i64, minute: u32) -> Vec<String> {
        let mut held = crate::lock(&self.inner.starts);
        let QueueStarts { at: starts, fired } = &mut *held;
        let mut due = Vec::new();
        for (queue, at) in starts.iter() {
            let near = minute >= *at && minute < at + 5;
            if near && fired.get(queue) != Some(&day) {
                fired.insert(queue.clone(), day);
                due.push(queue.clone());
            }
        }
        due
    }

    pub fn set_queue_limits(&self, limits: HashMap<String, usize>) {
        *crate::lock(&self.inner.queue_limits) = limits
            .into_iter()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, n)| (name, n.max(1)))
            .collect();
    }

    pub fn queue_limits(&self) -> HashMap<String, usize> {
        crate::lock(&self.inner.queue_limits).clone()
    }

    pub fn set_concurrency(&self, limit: usize) {
        self.inner
            .concurrency
            .store(limit.max(1), Ordering::Relaxed);
    }

    /// The options the next download to start will use.
    pub async fn options(&self) -> Options {
        self.inner.options.lock().await.clone()
    }

    /// Replaces the options the next download will start with.
    ///
    /// The limiter is deliberately not replaced: it is shared with every
    /// download already running, and swapping the handle would leave those
    /// drawing on a budget nothing else could see. Use `set_speed_limit`.
    pub async fn set_options(&self, options: Options) {
        let mut current = self.inner.options.lock().await;
        let limiter = current.limiter.clone();
        // Nor is the ledger of disk space promised to downloads under way.
        let room = current.room.clone();
        *current = Options {
            limiter,
            room,
            ..options
        };
    }

    /// The shared ceiling, in bytes per second. Zero means none.
    pub async fn speed_limit(&self) -> u64 {
        self.inner.options.lock().await.limiter.rate().await
    }

    /// Applies a ceiling to everything, including transfers already running.
    pub async fn set_speed_limit(&self, bytes_per_second: u64) {
        let limiter = self.inner.options.lock().await.limiter.clone();
        limiter.set_rate(bytes_per_second).await;
    }

    pub fn schedule(&self) -> Schedule {
        crate::lock(&self.inner.schedule).clone()
    }

    /// Whether the queue may run now.
    ///
    /// Asked on every tick, so it reads the window in place rather than
    /// cloning one — the clone was two heap strings and a vector, twenty
    /// times a second, to answer a question about the clock.
    fn window_open(&self) -> bool {
        crate::lock(&self.inner.schedule).open()
    }

    pub fn set_schedule(&self, schedule: Schedule) {
        *crate::lock(&self.inner.schedule) = schedule;
    }

    /// Caps how much is downloaded per period; zero bytes is no cap.
    pub fn set_allowance(&self, allowance: crate::quota::Allowance) {
        crate::lock(&self.inner.quota).set(allowance);
    }

    /// Whether the schedule and the download limit both let downloads run.
    pub fn may_run(&self) -> bool {
        self.window_open() && self.allowance_spent().is_none()
    }

    /// Whether any row is held by the schedule or the download limit, and
    /// so waits for one of them to let it go rather than for a person.
    pub async fn held(&self) -> bool {
        self.inner
            .slots
            .lock()
            .await
            .values()
            .any(|s| s.entry.paused_by_schedule && s.entry.state == State::Paused)
    }

    /// How long until downloading goes on, while the allowance is spent.
    pub fn allowance_spent(&self) -> Option<Duration> {
        crate::lock(&self.inner.quota).spent(std::time::SystemTime::now())
    }

    /// Sends downloads through this proxy from now on.
    ///
    /// Reported rather than silently ignored when it cannot be built:
    /// downloading directly is the one outcome someone who configured a
    /// proxy would most want to hear about.
    pub async fn set_proxy(&self, proxy: &crate::proxy::Proxy) -> Result<(), String> {
        self.set_client(proxy, crate::READ_TIMEOUT).await
    }

    /// Sends downloads through this proxy, giving up on a connection that
    /// sends nothing for `read_timeout`, from now on. Running downloads keep
    /// the client they started with.
    pub async fn set_client(
        &self,
        proxy: &crate::proxy::Proxy,
        read_timeout: Duration,
    ) -> Result<(), String> {
        let client = crate::proxy::client_with_timeout(proxy, read_timeout)?;
        *self.inner.client.lock().await = client;
        Ok(())
    }

    /// Attaches a digest for a download to be checked against.
    /// One download's own speed limit, in KB/s (zero for none), in force at
    /// once if it is running.
    pub async fn set_own_limit(&self, id: Id, kb_per_second: u64) {
        let limiter = {
            let mut slots = self.inner.slots.lock().await;
            let Some(slot) = slots.get_mut(&id) else {
                return;
            };
            slot.entry.speed_limit = kb_per_second;
            slot.own_limit.clone()
        };
        limiter.set_rate(kb_per_second.saturating_mul(1024)).await;
        self.save().await;
    }

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
            slot.entry.parts_dir = parts.display().to_string();
            slot.entry.save_dir = Some(folder.clone());
            slot.entry.began_in = folder;
            slot.requeue();
        }
        self.save().await;
        Ok(())
    }

    pub async fn set_checksum(&self, id: Id, checksum: Option<Checksum>) {
        if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
            slot.entry.checksum = checksum;
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

    /// The rows a range is being retried for, now: each with the attempt
    /// it is on (the first try is 1) and how many there may be, 0 for no
    /// end, and what failed, in the engine's words. Over once bytes arrive
    /// again.
    pub async fn retrying(&self) -> HashMap<Id, (u32, u32, String)> {
        self.inner
            .slots
            .lock()
            .await
            .iter()
            .filter(|(_, s)| s.entry.state == State::Running)
            .filter_map(|(id, s)| s.retrying.clone().map(|r| (*id, r)))
            .collect()
    }

    /// Every row, in the order they were added.
    pub async fn list(&self) -> Vec<Entry> {
        let slots = self.inner.slots.lock().await;
        let order = self.inner.order.lock().await;
        order
            .iter()
            .filter_map(|id| slots.get(id).map(|s| s.entry.clone()))
            .collect()
    }

    pub async fn get(&self, id: Id) -> Option<Entry> {
        self.inner
            .slots
            .lock()
            .await
            .get(&id)
            .map(|s| s.entry.clone())
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
            slot.entry.state = State::Running;
            slot.outside_schedule = true;
            (slot.entry.url.clone(), slot.control.clone())
        };
        let task = self.start(id, url, control);
        if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
            slot.task = Some(task);
        }
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
                    if std::mem::take(&mut slot.entry.paused_by_schedule) {
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
                // a file put aside on purpose (UI round 16).
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
        slot.entry.progress = Progress {
            downloaded: 0,
            total: slot.entry.progress.total,
            speed: 0.0,
            eta: None,
        };
        drop(slots);
        self.save().await;
        true
    }

    pub async fn cancel(&self, id: Id) {
        let mut slots = self.inner.slots.lock().await;
        let Some(slot) = slots.get_mut(&id) else {
            return;
        };
        if slot.entry.state.settled() {
            return;
        }
        slot.control.cancel();
        slot.entry.state = State::Cancelled;
        slot.entry.finished_at = Some(now_seconds());
        drop(slots);
        self.discard_parts(id).await;
        self.save().await;
    }

    /// Takes a row out of the map, the one way a row leaves it: stopped if
    /// it was not already, and a run still winding down kept in `leaving`,
    /// where it counts against the limit until it lets go. `remove` did
    /// this and `clear_finished` did not, so clearing a cancelled row whose
    /// run still held a stalled read let a limit of one run two.
    fn take_slot(&self, slots: &mut HashMap<Id, Slot>, id: Id) -> Option<Slot> {
        let mut slot = slots.remove(&id)?;
        if !slot.entry.state.settled() {
            slot.control.cancel();
        }
        if let Some(task) = slot.task.take().filter(|task| !task.is_finished()) {
            crate::lock(&self.inner.leaving).push((slot.entry.queue.clone(), task));
        }
        Some(slot)
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
            let parts = if slot.entry.parts_dir.is_empty() {
                default_parts.join(id.to_string())
            } else {
                PathBuf::from(&slot.entry.parts_dir)
            };
            self.discard_parts_at(&parts).await;
        }
        self.save().await;
    }

    /// How many times the queue file has been written since this queue was
    /// made: what a test holds a batch operation to.
    pub fn saves_written(&self) -> u64 {
        self.inner.saves.load(Ordering::Relaxed)
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
    async fn tick(&self) {
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
             * ten attempts while the queue stood still behind it (QA pass,
             * gap 81). Lent, not given: with the retrying rows, no more than
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
    async fn hold_for_schedule(&self) {
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
    async fn release_from_schedule(&self) {
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
    fn start(&self, id: Id, url: String, control: Control) -> tokio::task::JoinHandle<()> {
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

/// Why a download stopped for want of room or of its folder, where that is
/// why: a full disk (as the system says it, or dm's own check before it
/// started), a folder it may not write to, or one that can no longer be
/// made, as when its drive is taken out. `None` for any other failure.
fn out_of_room(error: &Error, dirs: [&std::path::Path; 2]) -> Option<String> {
    let text = error.to_string();
    let lower = text.to_ascii_lowercase();
    let full = match error {
        Error::Disk(io) => {
            io.kind() == std::io::ErrorKind::StorageFull
                // ENOSPC; Windows' ERROR_DISK_FULL and ERROR_HANDLE_DISK_FULL.
                || matches!(io.raw_os_error(), Some(28 | 112 | 39))
        }
        _ => false,
    } || [
        "no space left",
        "not enough room",
        "disk full",
        "disk is full",
        // dm's own check of the download folder, before a byte is fetched.
        "cannot be written to",
    ]
    .iter()
    .any(|w| lower.contains(w));
    if full {
        return Some(text);
    }
    if !matches!(error, Error::Disk(_)) {
        return None;
    }
    dirs.iter()
        .find(|dir| !dir.as_os_str().is_empty() && std::fs::create_dir_all(dir).is_err())
        .map(|dir| {
            format!(
                "the folder {} cannot be reached (was its drive taken out?)",
                dir.display()
            )
        })
}

/// Where a row's unfinished parts go in a folder a person chose for it.
pub const UNFINISHED: &str = ".dm-unfinished";

/// Moves a folder, across disks if it has to: copied whole, then the
/// original removed.
async fn move_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(());
    }
    copy_dir(from, to).await?;
    let _ = tokio::fs::remove_dir_all(from).await;
    Ok(())
}

fn copy_dir<'a>(
    from: &'a std::path::Path,
    to: &'a std::path::Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(to).await?;
        let mut listing = tokio::fs::read_dir(from).await?;
        while let Some(entry) = listing.next_entry().await? {
            let target = to.join(entry.file_name());
            if entry.file_type().await?.is_dir() {
                copy_dir(&entry.path(), &target).await?;
            } else {
                tokio::fs::copy(entry.path(), &target).await?;
            }
        }
        Ok(())
    })
}

/// Moves `from` to `name` beside it, under a free name if that one is taken.
async fn rename_free(from: &std::path::Path, name: &str) -> std::io::Result<PathBuf> {
    let name = crate::probe::sanitize(name);
    if name.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "that is not a name a file can have",
        ));
    }
    let dir = from.parent().unwrap_or(std::path::Path::new("."));
    // Placed under the chosen name already, or the free one beside it
    // where that was taken: renamed again, "name (1)" became "name (2)".
    if crate::download::candidates(dir, &name).any(|c| c == from) {
        return Ok(from.to_path_buf());
    }
    if let Some(to) = rename_case_only(from, dir, &name).await? {
        return Ok(to);
    }
    // Claimed by creating it, so nothing else can take the name between
    // the choice and the move; the move then replaces that empty claim.
    let to = crate::download::claim_path(dir, &name)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    if let Err(e) = tokio::fs::rename(from, &to).await {
        let _ = tokio::fs::remove_file(&to).await;
        return Err(e);
    }
    Ok(to)
}

/// A rename that changes only the case of a name ("Report.pdf" to
/// "report.pdf"), done in place. On a case-insensitive disk (macOS's and
/// Windows' by default) the new name is "taken" by the file itself, so the
/// free-name rule gave "report (1).pdf". `None` when this is not such a
/// rename, or the disk holds the new name as another file.
async fn rename_case_only(
    from: &std::path::Path,
    dir: &std::path::Path,
    name: &str,
) -> std::io::Result<Option<PathBuf>> {
    let Some(old) = from.file_name().map(|f| f.to_string_lossy().into_owned()) else {
        return Ok(None);
    };
    if old.to_lowercase() != name.to_lowercase() {
        return Ok(None);
    }
    let to = dir.join(name);
    if tokio::fs::symlink_metadata(&to).await.is_err() {
        // Nothing answers to the new name: an ordinary rename will do.
        return Ok(None);
    }
    // Something answers to it. If no entry is spelt that way, it is this
    // file found by a case-insensitive disk; otherwise it is another file.
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name().to_string_lossy() == name {
            return Ok(None);
        }
    }
    // Through a name of its own: a disk that ignores case may also ignore
    // a rename to what it takes for the same name.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let between = dir.join(format!(".dm-rename-{}-{nanos}", std::process::id()));
    tokio::fs::rename(from, &between).await?;
    if let Err(e) = tokio::fs::rename(&between, &to).await {
        let _ = tokio::fs::rename(&between, from).await;
        return Err(e);
    }
    Ok(Some(to))
}

/* --------------------------------------------------------------------- row verbs
 *
 * What the File and Downloads menus do to a row, beyond pause and resume:
 * a description, named queues, a folder and name chosen before the start,
 * a fresh address for an expired link, a download started again from
 * nothing, the order rows wait in, and how each connection is getting on.
 */
impl Queue {
    /// Adds a download with what the "Download details" dialog chose.
    pub async fn add_with(&self, url: &str, with: AddOptions) -> Option<Id> {
        let id = self.add_unsaved(url, with).await;
        if id.is_some() {
            self.save().await;
        }
        id
    }

    /// Adds every one of `items`, each with its own options, and writes the
    /// queue once. One at a time it was written, and synced to disk, once a
    /// link: two thousand links from a page were two thousand rewrites of a
    /// growing file, with the window waiting.
    pub async fn add_each(&self, items: Vec<(String, AddOptions)>) -> Vec<Option<Id>> {
        let mut ids = Vec::with_capacity(items.len());
        for (url, with) in items {
            ids.push(self.add_unsaved(&url, with).await);
        }
        if ids.iter().any(Option::is_some) {
            self.save().await;
        }
        ids
    }

    async fn add_unsaved(&self, url: &str, with: AddOptions) -> Option<Id> {
        let url = url.trim();
        if !accepted(url) {
            return None;
        }
        let id = self.insert(url, with.referrer.trim().to_string()).await;
        if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
            slot.entry.description = with.description;
            slot.entry.queue = with.queue;
            slot.entry.save_dir = with.save_dir.filter(|d| !d.trim().is_empty());
            slot.entry.save_name = with.save_name.filter(|n| !n.trim().is_empty());
            slot.entry.quality = with.quality.filter(|q| !q.trim().is_empty());
            slot.entry.cookies = with.cookies.trim().to_string();
            slot.entry.cookies_once = with.cookies_once && !slot.entry.cookies.is_empty();
            slot.entry.replaces = with.replaces.filter(|p| !p.trim().is_empty());
            if with.checksum.is_some() {
                slot.entry.checksum = with.checksum;
            }
            if with.later {
                slot.hold(false);
            }
        }
        Some(id)
    }

    /// Changes something about a row and writes the queue, if the row exists.
    async fn edit(&self, id: Id, change: impl FnOnce(&mut Slot) -> bool) -> bool {
        let changed = match self.inner.slots.lock().await.get_mut(&id) {
            Some(slot) => change(slot),
            None => false,
        };
        if changed {
            self.save().await;
        }
        changed
    }

    pub async fn set_description(&self, id: Id, text: &str) -> bool {
        let text = text.to_string();
        self.edit(id, |slot| {
            slot.entry.description = text;
            true
        })
        .await
    }

    /// Puts a row in a named queue, or takes it out with an empty name.
    pub async fn set_queue(&self, id: Id, queue: &str) -> bool {
        let queue = queue.trim().to_string();
        self.edit(id, |slot| {
            slot.entry.queue = queue;
            true
        })
        .await
    }

    /// "Update download link": a new link for a download whose
    /// old one expired. Only while it is not running, and only to something
    /// dm can fetch. The parts stay; the plan recorded beside them, with the
    /// file's validator, decides whether they belong to the new link's file.
    pub async fn set_url(&self, id: Id, url: &str) -> bool {
        let url = url.trim().to_string();
        if !accepted(&url) {
            return false;
        }
        self.edit(id, |slot| {
            if slot.entry.state == State::Running || slot.entry.state == State::Completed {
                return false;
            }
            // A sign-in is for the server it came from: an address on
            // another one does not get it.
            let same_server = crate::cookies::SignIn::new(&slot.entry.url, "x=1")
                .and_then(|s| s.for_url(&url).map(|_| ()))
                .is_some();
            if !same_server {
                slot.entry.cookies.clear();
            }
            slot.entry.url = url;
            true
        })
        .await
    }

    /// "Redownload": the same address fetched again from nothing.
    pub async fn redownload(&self, id: Id) -> bool {
        self.fetch_again(id, None).await
    }

    /// Fetched again from nothing; with `replacing`, the finished file the
    /// row keeps pointing at until the new version is whole and takes its
    /// place.
    async fn fetch_again(&self, id: Id, replacing: Option<String>) -> bool {
        let reset = {
            let mut slots = self.inner.slots.lock().await;
            match slots.get_mut(&id) {
                Some(slot) => {
                    slot.control.cancel();
                    slot.requeue();
                    slot.entry.progress = Progress {
                        downloaded: 0,
                        total: None,
                        speed: 0.0,
                        eta: None,
                    };
                    slot.entry.path = replacing.clone().unwrap_or_default();
                    slot.entry.replaces = replacing;
                    slot.entry.verified = false;
                    true
                }
                None => false,
            }
        };
        if reset {
            self.discard_parts(id).await;
            self.save().await;
        }
        reset
    }

    /// Moves a row up (negative) or down the order the queue is served in.
    pub async fn shift(&self, id: Id, by: isize) -> bool {
        let moved = {
            let mut order = self.inner.order.lock().await;
            match order.iter().position(|x| *x == id) {
                Some(at) => {
                    let to = (at as isize + by).clamp(0, order.len() as isize - 1) as usize;
                    let row = order.remove(at);
                    order.insert(to, row);
                    to != at
                }
                None => false,
            }
        };
        if moved {
            self.save().await;
        }
        moved
    }

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
            self.save().await;
        }
        started
    }

    /// The recheck queue: asks the server about every finished row
    /// of `queue` again and downloads afresh each one whose file changed
    /// there: a different ETag or Last-Modified, or a different length.
    /// Returns how many were checked and which are being fetched again. A
    /// server that cannot be reached, or says nothing to compare, leaves its
    /// row alone; a row whose address is a page yt-dlp read or a stream's
    /// playlist has nothing to compare and is not counted as checked.
    pub async fn synchronize(&self, queue: &str) -> (usize, Vec<Id>) {
        // One at a time: a second check over the same rows, from a click
        // while the timer's runs, held the old validators and fetched again
        // what the first had just fetched.
        if self.inner.rechecking.swap(true, Ordering::SeqCst) {
            return (0, Vec::new());
        }
        let found = self.synchronize_now(queue).await;
        self.inner.rechecking.store(false, Ordering::SeqCst);
        found
    }

    async fn synchronize_now(&self, queue: &str) -> (usize, Vec<Id>) {
        // A scheme is case-insensitive: an `HTTP://` row is as much a
        // server's file as an `http://` one, and was never asked about.
        let web = |url: &str| {
            ["http://", "https://"].iter().any(|scheme| {
                url.get(..scheme.len())
                    .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
            })
        };
        let rows: Vec<Entry> = self
            .list()
            .await
            .into_iter()
            .filter(|e| e.queue == queue && e.state == State::Completed)
            .filter(|e| web(&e.url))
            .collect();
        let client = self.inner.client.lock().await.clone();
        let (mut checked, mut changed) = (0, Vec::new());
        for row in &rows {
            let referrer = (!row.referrer.is_empty()).then_some(row.referrer.as_str());
            let sign_in = crate::cookies::SignIn::new(&row.url, &row.cookies);
            let Ok(now) = crate::probe::probe(&client, &row.url, referrer, sign_in.as_ref()).await
            else {
                continue;
            };
            let Some(different) = self
                .compare(&client, row, &now, referrer, sign_in.as_ref())
                .await
            else {
                continue;
            };
            checked += 1;
            if !different {
                continue;
            }
            // Asked a moment ago, from a list taken at the start: the row
            // must still be there, finished, with the same file, or the
            // file is not the one this check is about (removed from the
            // list and kept, or fetched again since).
            let same = self.get(row.id).await.is_some_and(|now| {
                now.state == State::Completed
                    && now.path == row.path
                    && now.validator == row.validator
            });
            if !same {
                continue;
            }
            // The outdated copy stays until the new one is whole, then the
            // new one takes its name rather than arriving beside it as
            // "name (1)". Deleted first, a failed fetch left neither.
            let keep = (!row.path.is_empty()).then(|| row.path.clone());
            if self.fetch_again(row.id, keep).await {
                changed.push(row.id);
            }
        }
        (checked, changed)
    }

    /// Whether a finished row's file differs from what its address serves
    /// now; `None` where there is nothing comparable to ask, which is not
    /// counted as checked.
    ///
    /// What the row recorded is of the file, and the address is not always
    /// the file's: a Metalink row's address is the mirror list, a yt-dlp
    /// row's is a web page, a stream's is its playlist. Compared with the
    /// file's length and ETag, the document always differed, and every such
    /// row was fetched again on every check.
    async fn compare(
        &self,
        client: &reqwest::Client,
        row: &Entry,
        now: &crate::Probe,
        referrer: Option<&str>,
        sign_in: Option<&crate::cookies::SignIn>,
    ) -> Option<bool> {
        let kind = now.content_type.as_deref();
        // A page yt-dlp made a video of, or a playlist of segments: what the
        // address serves says nothing about the file made from it.
        if crate::ytdlp::is_page(kind) || crate::stream::kind_of(&row.url, kind).is_some() {
            return None;
        }
        if !crate::metalink::is_metalink(&row.url, kind) {
            return Some(differs(row, now));
        }
        // The file a Metalink names: its length, as the document states it
        // or as a mirror reports it. Not its ETag: the row's is the one
        // mirror's it was probed from, and every mirror has its own.
        // Fetched as a download fetches it: retried, under the speed
        // ceiling, and capped. Retried a bounded number of times even where
        // the setting says to keep trying: nobody is waiting on a check.
        let mut options = crate::Options {
            control: Control::new(),
            referrer: referrer.map(str::to_string),
            sign_in: sign_in.cloned(),
            ..self.inner.options.lock().await.clone()
        };
        if options.attempts == 0 {
            options.attempts = crate::retry::MAX_ATTEMPTS;
        }
        let listed = crate::download::fetch_metalink(client, &now.final_url, &options)
            .await
            .ok()?;
        let size = match listed.size {
            Some(size) => Some(size),
            None => {
                let mut size = None;
                for mirror in &listed.mirrors {
                    if let Ok(probed) = crate::probe::probe(client, mirror, None, None).await {
                        size = probed.size;
                        break;
                    }
                }
                size
            }
        };
        let file = crate::Probe {
            size,
            validator: None,
            ..now.clone()
        };
        Some(differs(row, &file))
    }

    /// Has the recheck queue ask its servers again every `every` on its
    /// own, counted from now; `None` only when asked. Running the check
    /// by hand does not move the clock: a person who asked now still gets
    /// the next one on time.
    pub fn set_recheck_every(&self, every: Option<Duration>) {
        let mut recheck = crate::lock(&self.inner.recheck);
        if recheck.0 != every {
            *recheck = (every, now_millis());
        }
    }

    /// Runs the recheck queue's check if its timer says so, and says what
    /// it found; `None` when there is no timer or it is not yet time.
    pub async fn recheck_if_due(&self) -> Option<(usize, Vec<Id>)> {
        let now = now_millis();
        let due = {
            let mut recheck = crate::lock(&self.inner.recheck);
            let every = recheck.0?;
            if recheck.1 > now {
                // A last run later than now is a clock that was once set
                // ahead. Waited out, it held the recheck until that date;
                // it is stale, and the count starts again from now.
                recheck.1 = now;
                false
            } else if u128::from(now - recheck.1) < every.as_millis() {
                return None;
            } else {
                recheck.1 = now;
                true
            }
        };
        if let Some(at) = self.recheck_file().await {
            if let Err(e) = tokio::fs::write(&at, now.to_string()).await {
                self.note(format!(
                    "when the recheck queue last ran could not be kept in {}: {e}",
                    at.display()
                ));
            }
        }
        if !due {
            return None;
        }
        Some(self.synchronize(RECHECK_QUEUE).await)
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

    /// How each connection of a download is getting on, from the plan and
    /// the part files on disk. Empty for one with nothing staged.
    pub async fn connections(&self, id: Id) -> Vec<Connection> {
        let parts = self.parts_dir_of(id).await;
        tokio::task::spawn_blocking(move || {
            let Some(plan) = crate::plan::recorded(&parts) else {
                return Vec::new();
            };
            plan.ranges
                .iter()
                .enumerate()
                .map(|(i, &(start, end))| {
                    // Its ledger, or the part file an earlier dm wrote.
                    let ledger = crate::download::read_ledger(&parts, i);
                    let part = std::fs::metadata(parts.join(format!("part-{i:04}"))).ok();
                    let on_disk = ledger.or(part.as_ref().map(|m| m.len())).unwrap_or(0);
                    let span = (end - start).saturating_add(1);
                    Connection {
                        start,
                        end,
                        done: on_disk.min(span),
                        started: ledger.is_some() || part.is_some(),
                    }
                })
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    /// What a download has fetched so far, as a file a player can open
    /// while it goes on: `preview.<its extension>` beside its parts, a hard
    /// link to the staged file where the disk allows one (so it grows as the
    /// download does), a copy otherwise. A player plays from the start for
    /// as far as the bytes are there; a file whose index is at its end will
    /// not play until it is. Replaced at each call; removed with the parts.
    pub async fn preview(&self, id: Id) -> Result<PathBuf, String> {
        let entry = self.get(id).await.ok_or("there is no such download")?;
        if entry.state == State::Completed {
            return Err("it has finished: open it instead".into());
        }
        let parts = self.parts_dir_of(id).await;
        tokio::task::spawn_blocking(move || preview_of(&parts, &entry.filename))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Whether the server said it serves ranges — "Can resume".
    /// `None` until the download has been planned.
    pub async fn resumable(&self, id: Id) -> Option<bool> {
        let parts = self.parts_dir_of(id).await;
        tokio::task::spawn_blocking(move || crate::plan::recorded(&parts).map(|p| p.resumable))
            .await
            .ok()
            .flatten()
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

/// Whether the server's file is no longer the one a finished row fetched.
/// Only what both sides know is compared; nothing known is no change.
pub fn differs(row: &Entry, now: &crate::Probe) -> bool {
    let validator = matches!((&row.validator, &now.validator), (Some(a), Some(b)) if a != b);
    let length = matches!((row.progress.total, now.size), (Some(a), Some(b)) if a != b);
    validator || length
}

/// What an earlier run left under `dir` that the download limit counted
/// then: the bytes of its part files; none for a folder that is empty or
/// missing; and `None` for one holding only another route's files (a
/// torrent's pieces, yt-dlp's `.part`), whose own first report says how
/// much of them there is.
async fn staged_bytes(dir: &std::path::Path) -> Option<u64> {
    let Ok(mut listing) = tokio::fs::read_dir(dir).await else {
        return Some(0);
    };
    let (mut parts, mut total, mut others) = (0, 0, 0);
    while let Ok(Some(entry)) = listing.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("part-") {
            parts += 1;
            total += entry.metadata().await.map(|m| m.len()).unwrap_or(0);
        } else if let Some(done) = name.strip_prefix("done-") {
            // A range's ledger: what is on disk of it in the staged file.
            parts += 1;
            if let Ok(index) = done.parse::<usize>() {
                total += crate::download::read_ledger(dir, index).unwrap_or(0);
            }
        } else if name != "plan.json"
            && name != crate::download::STAGED
            && !name.starts_with(PREVIEW)
        {
            others += 1;
        }
    }
    (parts > 0 || others == 0).then_some(total)
}

/// The file a player is given for [`Queue::preview`]: what is fetched of
/// `filename` under `parts`, under a name that ends as `filename` does.
fn preview_of(parts: &std::path::Path, filename: &str) -> Result<PathBuf, String> {
    // The staged file of a ranged download, a stream's recording, or the
    // one part of a download over one connection.
    let source = [crate::download::STAGED, "live.recording", "part-0000"]
        .iter()
        .map(|name| parts.join(name))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
        .ok_or("nothing of it is on disk to preview yet")?;
    let extension = std::path::Path::new(filename)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let to = parts.join(if extension.is_empty() {
        "preview".to_string()
    } else {
        format!("{PREVIEW}.{extension}")
    });
    let _ = std::fs::remove_file(&to);
    if std::fs::hard_link(&source, &to).is_err() {
        std::fs::copy(&source, &to).map_err(|e| format!("could not make a preview: {e}"))?;
    }
    Ok(to)
}

/// What a preview's file is called, before its extension.
pub(crate) const PREVIEW: &str = "preview";

/// The queue dm keeps finished files in to check them again for changes on
/// their servers.
pub const RECHECK_QUEUE: &str = "Recheck queue";

/// The queue dm has from the start, and that "Add for later" puts rows in.
pub const MAIN_QUEUE: &str = "Main queue";

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn a_preview_is_what_is_fetched_under_the_files_own_ending() {
        let dir = tempfile::tempdir().unwrap();
        let parts = dir.path();
        assert!(preview_of(parts, "talk.mp4").is_err(), "nothing yet");

        std::fs::write(parts.join(crate::download::STAGED), b"first bytes").unwrap();
        let preview = preview_of(parts, "talk.mp4").unwrap();
        assert_eq!(preview, parts.join("preview.mp4"));
        assert_eq!(std::fs::read(&preview).unwrap(), b"first bytes");

        // A link grows with the download; asked again, it is made afresh.
        std::fs::write(parts.join(crate::download::STAGED), b"first bytes, more").unwrap();
        let again = preview_of(parts, "talk.mp4").unwrap();
        assert_eq!(std::fs::read(again).unwrap(), b"first bytes, more");
    }
}
