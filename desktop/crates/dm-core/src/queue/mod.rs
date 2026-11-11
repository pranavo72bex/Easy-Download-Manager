// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
mod family;
mod inspect;
mod journal;
mod persist;
mod query;
mod queues;
pub use queues::queue_names;
mod recheck;
mod runner;
mod settings;

/// Milliseconds since the epoch, for the recheck clock.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Seconds since the epoch, for the history a finished row becomes.
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
    /// Waiting for its turn.
    Queued,
    /// Being downloaded now.
    Running,
    /// Stopped by the person, to be resumed.
    Paused,
    /// Finished and in its place.
    Completed,
    /// Carries the reason, because a row saying only "failed" sends people
    /// looking in the wrong place.
    Failed(String),
    /// Stopped by the person for good.
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

/// What a row says when its address has expired and it waits for the browser to
/// hand over a fresh one (`Entry::fresh_link`); the window words it in plain
/// sentences.
pub const LINK_EXPIRED: &str =
    "the link has expired: dm is waiting for the browser to hand over a new one";

/// One row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    /// The row's number, never reused.
    pub id: Id,
    /// The address it is downloaded from.
    pub url: String,
    /// Where it has got to.
    pub state: State,
    /// How much of it is done, and how fast.
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
    /// The title of the page the link was found on, to name a file whose own name says nothing.
    #[serde(default)]
    pub title_hint: Option<String>,
    /// Whether a person said yes to saving a web page that answered for the
    /// file as an HTML page (`Options::accept_page`).
    #[serde(default)]
    pub accept_page: bool,
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
    /// A part of the video rather than the whole, from one second to
    /// another; `None` is the whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<crate::clip::Span>,
    /// The part made vertical or square (`crate::shape`); `None` as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<crate::shape::Shape>,
    /// Its sound saved as MP3 when it finishes (`crate::mp3`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mp3: bool,
    /// Its part saved as a GIF when it finishes (`crate::gif`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gif: bool,
    /// Seconds after it finishes that its file goes to the Trash and the
    /// row with it ("Delete file after"); `None` keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_after: Option<u64>,
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
    /// The request headers the browser sent (a bearer token and the like),
    /// sent to this row's own server on every request, held like `cookies`
    /// (outside the journal, dropped with a one-off sign-in).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
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
    /// A row from the browser whose address has expired (a signed link that
    /// ran out): failed, but waiting for the browser to hand over a fresh
    /// address for the same file, which takes its place and goes on from the
    /// parts already staged (`Queue::adopt_fresh_link`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh_link: bool,
    /// A torrent's files, once its metadata is known, each with whether it
    /// is fetched (Properties).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub torrent_files: Vec<crate::torrent::TorrentFile>,
    /// The form this download is the answer to, sent again to fetch it. Held
    /// in memory only: a form can hold what a person typed.
    #[serde(skip)]
    pub post: Option<crate::cookies::Posted>,
    /// Whether it is a form's answer, kept on disk: after a restart, with the
    /// form gone, it is said to be started again from its page rather than
    /// asked for in a way the server never answered with the file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub posted: bool,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// How one connection of a download is getting on: its range of the file
/// and how much of it is on disk. Read from the staged parts themselves, so
/// it costs the transfer nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    /// The first byte of the connection's range.
    pub start: u64,
    /// The last byte of the range, inclusive.
    pub end: u64,
    /// How many bytes of the range are on disk.
    pub done: u64,
    /// Its part file is there: the server has answered for this range, so
    /// it is under way though its first bytes may still be in a buffer.
    pub started: bool,
    /// Last fetched over the second network path (`second_path`), in this
    /// run of dm.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub second: bool,
}

impl Entry {
    /// Seconds until "Delete file after" is due for this row, as of `now`
    /// (Unix seconds): 0 once it is; `None` for a row with none, or one not
    /// finished well (a failed or cancelled row has no file to delete).
    pub fn delete_due_in(&self, now: u64) -> Option<u64> {
        let after = self.delete_after?;
        if self.state != State::Completed {
            return None;
        }
        let at = self.finished_at?.saturating_add(after);
        Some(at.saturating_sub(now))
    }

    /// This row as it may be shown or sent anywhere but its own server: its
    /// sign-in (`cookies`), the browser's request headers (a bearer token
    /// rides in them) and a form's body (`post`) taken out, and a password
    /// in its address masked ([`shown_url`]). Whether it was signed in is
    /// still `!self.cookies.is_empty()` on the row itself.
    #[must_use]
    pub fn redacted(&self) -> Entry {
        Entry {
            url: shown_url(&self.url),
            cookies: String::new(),
            headers: Vec::new(),
            post: None,
            ..self.clone()
        }
    }
}

/// A download's address as a window shows it: a password in it is replaced
/// by dots, the rest kept.
pub fn shown_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if parsed.password().is_some() => {
            // Masked after formatting: put into the URL, the dots were
            // percent-encoded and read "user:%E2%80%A2…@". The first
            // ":mask@" is the user's, whose own `:` and `@` are encoded.
            let _ = parsed.set_password(Some("dmmasked"));
            parsed.to_string().replacen(":dmmasked@", ":•••@", 1)
        }
        _ => url.to_string(),
    }
}

/// What a caller may say when it adds a download: beyond its address, what
/// the "Download details" dialog asks before a download starts.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    /// The page the link was found on, where known.
    pub referrer: String,
    /// The person's note on the download.
    pub description: String,
    /// The queue to add it to; empty is the main queue.
    pub queue: String,
    /// The folder to save in, in place of the default.
    pub save_dir: Option<String>,
    /// The name to save under, in place of the one found.
    pub save_name: Option<String>,
    /// The title of the page the link was found on, where known.
    pub title_hint: Option<String>,
    /// "Add for later": added paused, waiting for its queue to start.
    pub later: bool,
    /// The quality picked in the browser's video panel, in
    /// `Quality::named`'s words; `None` is the setting's.
    pub quality: Option<String>,
    /// See `Entry::sound`.
    pub sound: Option<String>,
    /// A part of the video rather than the whole, from one second to
    /// another (`crate::clip::Span`).
    pub span: Option<crate::clip::Span>,
    /// The part made vertical or square (`crate::shape`).
    pub shape: Option<crate::shape::Shape>,
    /// Its sound saved as MP3 when it finishes.
    pub mp3: bool,
    /// Its part saved as a GIF when it finishes.
    pub gif: bool,
    /// "Delete file after", for the row it adds.
    pub delete_after: Option<u64>,
    /// The browser's cookies for this download's site, as a `Cookie`
    /// header, where the person switched that on for the site.
    pub cookies: String,
    /// Those cookies are for this download alone, not kept once it is done.
    pub cookies_once: bool,
    /// The headers the browser sent with the request (see `Entry::headers`).
    pub headers: Vec<(String, String)>,
    /// A file this download takes the place of once it is whole, as a
    /// recheck's new version does: kept until then, so a failed download
    /// leaves it.
    pub replaces: Option<String>,
    /// The digest the file must have, set before it can start.
    pub checksum: Option<crate::checksum::Checksum>,
    /// This download's own speed limit, in KB/s; zero for none.
    pub speed_limit: u64,
    /// Handed over by the browser: it may take the place of a row waiting
    /// for a fresh address for the same file (`Queue::adopt_fresh_link`)
    /// instead of being added as another row.
    pub fresh_link_ok: bool,
    /// The form the download is the answer to, where the browser sent one
    /// (its body, kept in memory with the row only).
    pub post: Option<crate::cookies::Posted>,
}

/// Named queues that start on their own each day: each with its minute of
/// the local day, and the day each last started.
#[derive(Default)]
struct QueueStarts {
    at: Vec<(String, u32)>,
    fired: HashMap<String, i64>,
}

struct Slot {
    /// Shared with the lists handed out ([`Queue::list_shared`]): a change
    /// copies the row first if a list still holds it ([`Slot::entry_mut`]).
    entry: Arc<Entry>,
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
    /// A running torrent's swarm at its last sample; none once it stops.
    swarm: Option<crate::torrent::Swarm>,
    /// This row's own speed limit (`Entry::speed_limit`), chained to the
    /// shared one while it runs, so a change reaches it at once.
    own_limit: crate::limit::Limiter,
}

impl Slot {
    /// The row, to change: copied first where a list handed out still
    /// shares it, so a list already read never changes under its reader.
    fn entry_mut(&mut self) -> &mut Entry {
        Arc::make_mut(&mut self.entry)
    }

    /// A row that is not downloading has no speed and no time left: the
    /// figures of its last run stayed on it (a finished download read as
    /// moving at 1.1 MB/s to `queue.list`, the command line and an AI
    /// client), as `persist` already says of a restored one.
    fn stopped_moving(&mut self) {
        self.entry_mut().progress.speed = 0.0;
        self.entry_mut().progress.eta = None;
    }

    /// Whether a previous run is still winding down.
    fn still_stopping(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    /// Stopped, keeping what it has. The twin of `requeue`, and the one
    /// transition behind pause, pause-all and the schedule closing — three
    /// copies of the same three lines, until one of them drifted.
    fn hold(&mut self, by_schedule: bool) {
        self.control.pause();
        self.entry_mut().state = State::Paused;
        self.stopped_moving();
        self.entry_mut().paused_by_schedule = by_schedule;
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
        self.entry_mut().state = State::Queued;
        self.stopped_moving();
        self.entry_mut().finished_at = None;
        self.entry_mut().paused_by_schedule = false;
        self.entry_mut().stalled.clear();
        self.entry_mut().fresh_link = false;
        self.outside_schedule = false;
        // About the run this one replaces.
        self.retrying = None;
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
    /// When the queue file was last written, and whether a change is
    /// waiting for the next write (`save`, `flush`).
    written: std::sync::Mutex<Option<std::time::Instant>>,
    unwritten: AtomicBool,
    trailing: AtomicBool,
    /// How long the last write of the queue file took, in milliseconds.
    last_write_ms: std::sync::atomic::AtomicU64,
    concurrency: AtomicUsize,
    /// Fewer files at once for a named queue than `concurrency` allows in
    /// all: a queue of one for a site that takes one download at a time.
    queue_limits: std::sync::Mutex<HashMap<String, usize>>,
    /// Whether a scheduler is already serving this queue.
    scheduling: AtomicBool,
    /// Set as dm ends (`Queue::shut_down`): nothing starts after it.
    closing: AtomicBool,
    /// Wakes a scheduler between its ticks, so a download added while
    /// others run starts now rather than at the next tick.
    kick: tokio::sync::Notify,
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
    /// A schedule of its own for a named queue; a queue with none follows `schedule`.
    queue_schedules: std::sync::Mutex<std::collections::BTreeMap<String, Schedule>>,
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
    /// What the queue file and its journal hold, for the next write to add
    /// only what changed (`journal`); `None` until a write has been made.
    saved: std::sync::Mutex<Option<journal::Saved>>,
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
    /// A queue that downloads with `client` and `options`, `concurrency` files at a time.
    pub fn new(client: reqwest::Client, options: Options, concurrency: usize) -> Self {
        Queue {
            inner: Arc::new(Inner {
                slots: Mutex::new(HashMap::new()),
                order: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                options: Mutex::new(options),
                store: Mutex::new(None),
                saving: Mutex::new(()),
                written: std::sync::Mutex::new(None),
                unwritten: AtomicBool::new(false),
                trailing: AtomicBool::new(false),
                last_write_ms: std::sync::atomic::AtomicU64::new(0),
                problems: std::sync::Mutex::new(Vec::new()),
                leaving: std::sync::Mutex::new(Vec::new()),
                scheduling: AtomicBool::new(false),
                closing: AtomicBool::new(false),
                kick: tokio::sync::Notify::new(),
                rechecking: AtomicBool::new(false),
                concurrency: AtomicUsize::new(concurrency.max(1)),
                queue_limits: std::sync::Mutex::new(HashMap::new()),
                schedule: std::sync::Mutex::new(Schedule::default()),
                queue_schedules: std::sync::Mutex::new(Default::default()),
                quota: std::sync::Mutex::new(crate::quota::Meter::default()),
                quota_saved: std::sync::Mutex::new(None),
                recheck: std::sync::Mutex::new((None, now_millis())),
                starts: std::sync::Mutex::new(QueueStarts::default()),
                client: Mutex::new(client),
                saves: AtomicU64::new(0),
                saved: std::sync::Mutex::new(None),
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
            entry: Arc::new(Entry {
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
                accept_page: false,
                validator: None,
                quality: None,
                sound: None,
                span: None,
                shape: None,
                mp3: false,
                gif: false,
                delete_after: None,
                parts_dir: String::new(),
                began_in: String::new(),
                cookies: String::new(),
                cookies_once: false,
                headers: Vec::new(),
                replaces: None,
                speed_limit: 0,
                stalled: String::new(),
                fresh_link: false,
                title_hint: None,
                torrent_files: Vec::new(),
                post: None,
                posted: false,
            }),
            control: Control::new(),
            task: None,
            outside_schedule: false,
            retrying: None,
            swarm: None,
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
/// torrent's pieces), whose own first report says how
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
            // A range's own file: its ledger above says what of it is on disk.
            && !name.starts_with(crate::download::RANGE)
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
    // The staged file of a ranged download, the first range's own file of
    // one going to another disk (which holds the file's beginning), a
    // stream's recording, or the one part of a download over one connection.
    let first_range = crate::download::range_path(parts, 0);
    let source = [
        parts.join(crate::download::STAGED),
        first_range,
        parts.join("live.recording"),
        parts.join("part-0000"),
    ]
    .into_iter()
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

    /// A download going to another disk has a file a range (gaps row 166): the first holds the
    /// file's beginning, which is what a player needs; the others are not the file's start.
    #[test]
    fn the_preview_of_a_download_with_a_file_a_range_is_its_first_range() {
        let dir = tempfile::tempdir().unwrap();
        let parts = dir.path();
        std::fs::write(crate::download::range_path(parts, 1), b"the middle").unwrap();
        assert!(
            preview_of(parts, "talk.mp4").is_err(),
            "not the file's start"
        );
        std::fs::write(crate::download::range_path(parts, 0), b"the start").unwrap();
        let preview = preview_of(parts, "talk.mp4").unwrap();
        assert_eq!(std::fs::read(preview).unwrap(), b"the start");
    }

    /// What an earlier run left is counted once: by each range's ledger, not again by the length
    /// of the range's own file.
    #[tokio::test]
    async fn a_range_file_is_counted_by_its_ledger_alone() {
        let dir = tempfile::tempdir().unwrap();
        let parts = dir.path();
        std::fs::write(crate::download::range_path(parts, 0), vec![1u8; 5000]).unwrap();
        std::fs::write(parts.join("done-0000"), format!("{:020}", 4096)).unwrap();
        std::fs::write(parts.join("plan.json"), b"{}").unwrap();
        assert_eq!(staged_bytes(parts).await, Some(4096));
    }
}

#[cfg(test)]
mod redacted_tests {
    use super::*;

    #[test]
    fn a_redacted_row_carries_no_secret_and_keeps_the_rest() {
        let mut row: Entry = serde_json::from_value(serde_json::json!({
            "id": 7,
            "url": concat!("https://me:", "pa55", "@files.example/a.zip"),
            "state": serde_json::to_value(State::Paused).unwrap(),
            "progress": {"downloaded": 5, "total": null, "speed": 0.0, "eta": null},
            "filename": "a.zip",
            "description": "the big one",
        }))
        .unwrap();
        row.cookies = "sid=1".into();
        row.headers = vec![("Authorization".into(), "Bearer t".into())];
        row.post = Some(crate::cookies::Posted {
            url: "https://files.example/form".into(),
            body: b"user=me".to_vec(),
            content_type: "application/x-www-form-urlencoded".into(),
        });
        let shown = row.redacted();
        assert!(shown.cookies.is_empty() && shown.headers.is_empty() && shown.post.is_none());
        assert_eq!(
            shown.url,
            concat!("https://me:", "•••", "@files.example/a.zip")
        );
        assert_eq!(
            (
                shown.id,
                shown.filename.as_str(),
                shown.description.as_str()
            ),
            (7, "a.zip", "the big one")
        );
        let said = serde_json::to_string(&shown).unwrap();
        assert!(!said.contains("pa55") && !said.contains("sid=1") && !said.contains("Bearer"));
        // An address with no password is kept as it is.
        assert_eq!(
            shown_url("https://files.example/a.zip"),
            "https://files.example/a.zip"
        );
    }
}

#[cfg(test)]
mod folder_tests {
    use super::*;

    #[test]
    fn a_full_disk_or_a_folder_that_cannot_be_reached_is_said_and_nothing_else_is() {
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path();
        let full = Error::Disk(std::io::Error::from(std::io::ErrorKind::StorageFull));
        assert!(out_of_room(&full, [here, here]).is_some());
        let enospc = Error::Disk(std::io::Error::from_raw_os_error(28));
        assert!(out_of_room(&enospc, [here, here]).is_some());
        let said = Error::Internal("the download folder cannot be written to".into());
        assert_eq!(
            out_of_room(&said, [here, here]).as_deref(),
            Some("the download folder cannot be written to")
        );
        // Another failure, on folders that are there: not a matter of room.
        assert_eq!(
            out_of_room(&Error::Internal("the server said no".into()), [here, here]),
            None
        );
        let other = Error::Disk(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(out_of_room(&other, [here, here]), None);
        // A folder under a file can never be made: its drive is said to be gone.
        let file = here.join("a file");
        std::fs::write(&file, b"x").unwrap();
        let under = file.join("downloads");
        let why = out_of_room(&other, [std::path::Path::new(""), &under]).unwrap();
        assert!(
            why.contains("cannot be reached") && why.contains("downloads"),
            "{why}"
        );
    }

    #[tokio::test]
    async fn a_folder_is_copied_whole_with_what_is_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        std::fs::create_dir_all(from.join("a/b")).unwrap();
        std::fs::write(from.join("one"), b"1").unwrap();
        std::fs::write(from.join("a/b/two"), b"2").unwrap();
        let to = dir.path().join("elsewhere/to");
        copy_dir(&from, &to).await.unwrap();
        assert_eq!(std::fs::read(to.join("one")).unwrap(), b"1");
        assert_eq!(std::fs::read(to.join("a/b/two")).unwrap(), b"2");
        assert!(from.join("one").exists(), "a copy leaves the original");
        // A move takes it away from where it was.
        let moved = dir.path().join("moved");
        move_dir(&from, &moved).await.unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(moved.join("a/b/two")).unwrap(), b"2");
        assert!(copy_dir(&dir.path().join("missing"), &dir.path().join("x"))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_rename_to_no_name_is_refused_and_one_of_case_only_is_done_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Report.pdf");
        std::fs::write(&file, b"r").unwrap();
        let why = rename_free(&file, "").await.unwrap_err();
        assert_eq!(why.kind(), std::io::ErrorKind::InvalidInput);
        // Only the case changes: the same file under the new spelling, never "report (1).pdf".
        let to = rename_free(&file, "report.pdf").await.unwrap();
        assert_eq!(to, dir.path().join("report.pdf"));
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["report.pdf"]);
        assert_eq!(std::fs::read(&to).unwrap(), b"r");
        // Another name entirely: not a case-only rename.
        assert_eq!(
            rename_case_only(&to, dir.path(), "other.pdf")
                .await
                .unwrap(),
            None
        );
    }
}
