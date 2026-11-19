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
mod files;
mod forget;
mod inspect;
mod journal;
mod list;
mod persist;
mod query;
pub use query::SharedRead;
mod queues;
pub use queues::queue_names;
mod recheck;
mod runner;
mod settings;

#[cfg(test)]
use files::{copy_dir, rename_case_only};
pub use files::{differs, UNFINISHED};
use files::{move_dir, out_of_room, preview_of, rename_free, staged_bytes};

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
    /// parts already staged (`DownloadList::adopt_fresh_link`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh_link: bool,
    /// A torrent's files, once its metadata is known, each with whether it
    /// is fetched (Properties). Shared: a row is copied each time it changes while a list read
    /// earlier still holds it, four times a second while it downloads, and a torrent's list of
    /// thousands of files was copied with it every time (gaps row S15).
    #[serde(default, skip_serializing_if = "no_files")]
    pub torrent_files: std::sync::Arc<Vec<crate::torrent::TorrentFile>>,
    /// What the download said of itself beside its state (`download::Update::Note`), in
    /// English; kept with the row once it finishes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub said: String,
    /// The form this download is the answer to, sent again to fetch it. Held
    /// in memory only: a form can hold what a person typed.
    #[serde(skip)]
    pub post: Option<crate::cookies::Posted>,
    /// Whether it is a form's answer, kept on disk: after a restart, with the
    /// form gone, it is said to be started again from its page rather than
    /// asked for in a way the server never answered with the file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub posted: bool,
    /// What kind of failure a failed row's is ([`State::Failed`] keeps its sentence), from the
    /// error that failed it; read through [`Entry::failed_as`]. Kept beside the state rather
    /// than in it, so a queue saved by this dm is still read by an older one, which passes the
    /// field over; one saved before it is worked out from the sentence once, as it is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_kind: Option<crate::FailKind>,
    /// "Keep no record" (gaps row W17): once the file is whole the row goes, and its address,
    /// page and sign-in with it, the queue file written whole so no journal line keeps them; the
    /// file stays. Ticked in Download details, and by itself for a hand-over from a private
    /// browser window. Left out of the queue file when false, so an older dm reads it as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_no_record: bool,
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
    /// What kind of failure this row's is, where it failed: the kind its error carried, or
    /// for a row with none (saved by a dm from before kinds, or made by hand) the one its
    /// sentence gives ([`crate::FailKind::of_old_text`]). `None` for a row that has not failed.
    pub fn failed_as(&self) -> Option<crate::FailKind> {
        match &self.state {
            State::Failed(said) => Some(
                self.fail_kind
                    .unwrap_or_else(|| crate::FailKind::of_old_text(said)),
            ),
            _ => None,
        }
    }

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
    /// for a fresh address for the same file (`DownloadList::adopt_fresh_link`)
    /// instead of being added as another row.
    pub fresh_link_ok: bool,
    /// The form the download is the answer to, where the browser sent one
    /// (its body, kept in memory with the row only).
    pub post: Option<crate::cookies::Posted>,
    /// "Keep no record": the row goes once the file is whole (`Entry::keep_no_record`).
    pub keep_no_record: bool,
}

/// Named queues that start on their own each day: each with its minute of
/// the local day, and the day each last started.
#[derive(Default)]
struct QueueStarts {
    at: Vec<(String, u32)>,
    fired: HashMap<String, i64>,
}

struct Slot {
    /// Shared with the lists handed out ([`DownloadList::list_shared`]): a change
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

/// Whether a row lists no torrent files (`Entry::torrent_files`), left out of the queue file.
fn no_files(files: &std::sync::Arc<Vec<crate::torrent::TorrentFile>>) -> bool {
    files.is_empty()
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

/// The queue's state, shared by every clone of it.
///
/// The order its locks are taken in, where one is taken while another is
/// held (gaps row 184): `saving`, then `slots`, then `order`. Every other
/// lock here is a leaf, taken last: its value is copied out (`options`,
/// `store`, `client`) or changed in place (`quota`, `leaving`, the
/// schedules), and the guard dropped before anything else is locked or
/// awaited. Nothing awaits while it holds `slots` or `order`, nor does any
/// file or network work under them; `note`, which calls the change
/// callback, is called with neither held, nor `saving`. `saving` alone is
/// held across I/O, the queue file's write, on purpose: it is what keeps an
/// older list from being written after a newer one, and nothing waits for
/// it but the next write and a `flush` asked to return once the list is on
/// disk (a save made meanwhile goes into the next write: `save`). The std
/// locks cannot be held across an `.await` at all: clippy's
/// `await_holding_lock` refuses it, and CI refuses its warnings.
struct Inner {
    slots: Mutex<HashMap<Id, Slot>>,
    /// Insertion order, so the queue is served in the order it was given.
    order: Mutex<Vec<Id>>,
    next_id: AtomicU64,
    /// Changeable while downloads are running: a setting that only applies
    /// after a restart is one people assume is broken. Running transfers keep
    /// the options they started with; the next one to start picks these up.
    /// A plain mutex: it is only ever copied out or replaced, never held
    /// while anything is awaited.
    options: std::sync::Mutex<Options>,
    /// Where the queue is written, if anywhere. Tests leave it unset. A plain
    /// mutex, read by copying the path out: a tokio guard made in an `if let`
    /// lived to the end of its block, and held this across the queue file's
    /// whole write, so every save (each pause, each finished download) and
    /// the progress's own writes waited for it (gaps row 184).
    store: std::sync::Mutex<Option<PathBuf>>,
    /// Held from reading the rows to renaming the file, so an older snapshot
    /// cannot be written after a newer one.
    saving: Mutex<()>,
    /// How long each write of the queue file is made to take, in
    /// milliseconds, by a test standing in for a slow disk (zero: none).
    slow_writes_ms: AtomicU64,
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
    /// Set as dm ends (`DownloadList::shut_down`): nothing starts after it.
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
    /// Running downloads keep the client they started with. A plain mutex:
    /// it is only ever copied out or replaced.
    client: std::sync::Mutex<reqwest::Client>,
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
    /// The next write is of the whole list, the journal gone with it: a row that keeps no
    /// record went since, and the journal's lines still name it (`forget`).
    whole_next: AtomicBool,
}

/// A queue of downloads.
///
/// Cheap to clone; every clone talks to the same queue.
#[derive(Clone)]
pub struct DownloadList {
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

/// What a preview's file is called, before its extension.
pub(crate) const PREVIEW: &str = "preview";

/// The queue dm keeps finished files in to check them again for changes on
/// their servers.
pub const RECHECK_QUEUE: &str = "Recheck queue";

/// The queue dm has from the start, and that "Add for later" puts rows in.
pub const MAIN_QUEUE: &str = "Main queue";

#[cfg(test)]
mod preview_tests;

#[cfg(test)]
mod redacted_tests;

#[cfg(test)]
mod folder_tests;
