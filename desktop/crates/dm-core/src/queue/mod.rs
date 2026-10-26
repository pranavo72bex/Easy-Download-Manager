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

mod actions;
mod edit;
mod inspect;
mod persist;
mod query;
mod queues;
mod recheck;
mod runner;
mod settings;

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
    /// The address of a separate sound track for the picture at `url`, from a
    /// player that fetched them apart; both are fetched and joined.
    #[serde(default)]
    pub sound: Option<String>,
    /// Where this row's parts are staged, recorded when it first started,
    /// so the folder in the settings can change without stranding them.
    /// Empty until then.
    #[serde(default)]
    pub parts_dir: String,
    /// The download folder in force when it first started, kept so a
    /// download begun goes on into the folder it began in: one running when
    /// the folder was changed in Options finished in the new one.
    /// Empty until then, and unused where `save_dir` names a folder.
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
    /// See `Entry::sound`.
    pub sound: Option<String>,
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
    /// This download's own speed limit, in KB/s; zero for none.
    pub speed_limit: u64,
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
    retrying: Option<(u32, u32, String, std::time::Instant)>,
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
                sound: None,
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

    /// Where the recheck queue's last run is kept: beside the queue file.
    async fn recheck_file(&self) -> Option<PathBuf> {
        let store = self.inner.store.lock().await.clone()?;
        Some(store.with_file_name("recheck-at"))
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
