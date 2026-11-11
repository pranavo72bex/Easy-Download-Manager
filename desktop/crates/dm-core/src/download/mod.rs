// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Fetching a file over several connections at once, and putting it back
//! together.
//!
//! Every range is written at its own offset in one staged file, renamed into
//! place when whole. Each range's ledger records how much of it is on disk,
//! written only after those bytes are flushed, so it is never ahead of them
//! — a crash costs the bytes that were in flight and nothing else.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::fs;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::category::{self, Categories};
use crate::checksum::{self, Checksum};
use crate::control::Control;
use crate::limit::Limiter;
use crate::metalink::{self, Metalink};
use crate::plan::{self, Segment};
use crate::probe::{probe_get, Probe, ProbeWithBody};
use crate::progress::{Meter, Progress};
use crate::retry;
use crate::stream::{self, Quality};
use crate::Error;

mod assemble;
mod dash;
mod files;
mod hls;
mod http;
mod mirrors;
mod plain;
mod ranged;
mod space;

pub(crate) use assemble::*;
pub use files::*;
pub(crate) use hls::*;
pub use http::*;
use mirrors::*;
pub(crate) use ranged::*;
pub use space::*;

/// How a download should be carried out.
#[derive(Debug, Clone)]
pub struct Options {
    /// How many connections one file may use at most.
    pub connections: usize,
    /// Somewhere to ask this download to stop. Shared, so a caller keeps its
    /// own clone; the default never stops anything.
    pub control: Control,
    /// Where finished files land.
    pub dest_dir: PathBuf,
    /// Where parts are staged. Kept apart from the destination so a
    /// half-finished download never looks like a finished one.
    pub parts_dir: PathBuf,
    /// The shared speed ceiling. Cloned from the queue's, so every download
    /// draws on one budget rather than each getting the whole of it.
    pub limiter: Limiter,
    /// Disk space promised to downloads under way, shared like the
    /// limiter, so downloads started together do not each count the same
    /// free space.
    pub room: Room,
    /// Whether finished files are sorted into folders by kind, and what
    /// those folders are called.
    pub categories: Categories,
    /// A digest the finished file must have. A Metalink supplies its own
    /// where the caller gave none.
    pub checksum: Option<Checksum>,
    /// What only a torrent's download reads: the files chosen, the peers
    /// blocked.
    pub torrent: crate::torrent::TorrentOptions,
    /// Which rendition to take from an adaptive stream.
    pub quality: Quality,
    /// The address of a separate sound track for the picture at the download's
    /// own address, which a player fetched apart from it: both are fetched at
    /// once and joined (`dash::download_media_pair`).
    pub sound: Option<String>,
    /// A part of the video rather than the whole: from one second to
    /// another, cut from the streams dm fetches itself (`crate::clip`).
    /// A video that cannot be cut that way fails and says so; nothing
    /// fetches the whole instead.
    pub span: Option<crate::clip::Span>,
    /// Vertical or square for a part of a video (`shape`); `None` as it is.
    pub shape: Option<crate::shape::Shape>,
    /// The finished sound saved as MP3 (`crate::mp3`).
    pub mp3: bool,
    /// The finished part saved as a GIF (`crate::gif`).
    pub gif: bool,
    /// Media was asked for (a player's video, from the browser's panel or
    /// media menu): a non-YouTube page fails, rather than being fetched as
    /// the document its address ends like.
    pub media: bool,
    /// The page a download came from, sent as `Referer`. Set for a download a
    /// browser handed over, where the file is served only to a request that
    /// names its page; `None` for one typed in, which needs no such thing.
    pub referrer: Option<String>,
    /// The proxy, as a URL, for a tool dm runs on its behalf. The client
    /// already carries it for every request of dm's own.
    pub proxy: Option<String>,
    /// The container a video from the browser's panel was listed in ("mp4",
    /// "webm"), preferred when choosing among the formats at its height,
    /// so what arrives is what the list said.
    pub container: Option<String>,
    /// The frame rate a video from the panel was listed at, where its height
    /// has two lines ("720p HD" and "720p HD 60 fps", as reference A): above
    /// 30 asks for the faster picture, 30 or less for the other.
    pub frame_rate: Option<u32>,
    /// The caption track a list line named (`Quality::caption_of`): its
    /// language and whether it is the speech-recognised one. The download is
    /// that caption file alone.
    pub caption: Option<(String, bool)>,
    /// How many times one connection tries a range (or the probe) before
    /// the download fails, where the failure is the kind waiting can fix.
    /// Zero keeps trying until it gets through, or is paused or cancelled.
    pub attempts: u32,
    /// Sites that take fewer connections at once than `connections`: a
    /// server that refuses or throttles a second connection is given one.
    pub host_limits: Vec<HostLimit>,
    /// Sites `proxy` is not used for; see `Proxy::bypass`. The client
    /// already knows them; the tools and the FTP client are told here.
    pub proxy_bypass: Vec<String>,
    /// Carry a video's subtitles inside it, put there by dm (`subtitles`).
    pub subtitles: bool,
    /// The site's sign-in the browser handed over with this download, sent
    /// to that server only.
    pub sign_in: Option<crate::cookies::SignIn>,
    /// The name a person chose for the file, which it is placed under in
    /// the first place: placed under the server's name and renamed after,
    /// it held that name for a moment and pushed a download of the same
    /// file finishing then to "name (1)".
    pub save_name: Option<String>,
    /// The title of the page the link came from: names the file when the name found says nothing (`naming`).
    pub title_hint: Option<String>,
    /// A page that answers for a file (a sign-in, an expired link) is saved as
    /// an HTML page, named `<name>.html`, rather than failing the download:
    /// what a person who was asked and said yes has chosen (reference A asks).
    pub accept_page: bool,
    /// The proxy is the computer's own settings. dm's HTTP client reads
    /// them, but where they name no address the environment carries (the
    /// macOS and Windows network settings), FTP and BitTorrent, which
    /// connect by themselves, have none to go through — and must not go
    /// around it.
    pub system_proxy: bool,
    /// Put a stream that arrives as a transport stream (`.ts`) into an MP4,
    /// losslessly, by dm itself (`tsmux`); a stream in a format it does not
    /// put into one stays a `.ts`. Players open an MP4; few open a `.ts`.
    pub remux: bool,
    /// Give a finished file the date the server says it last changed
    /// (`Last-Modified`) rather than when it was fetched.
    pub server_dates: bool,
    /// Seconds between attempts after a failure; 0 for dm's own pace.
    pub retry_wait: u64,
    /// How long a live stream (RTMP, RTSP, MMS) is recorded before dm ends
    /// it and keeps what it has, as it does when the server ends the stream;
    /// `None` records until the server ends it or the person stops it.
    pub record_limit: Option<std::time::Duration>,
}

/// Fewer connections for one site: its host, or a suffix of it
/// (`example.com` covers `dl.example.com`), and how many it takes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostLimit {
    /// The host, or a suffix of it, the limit is for.
    pub host: String,
    /// How many connections that site takes.
    pub connections: usize,
}

/// Whether a connection of dm's own is refused under the computer's proxy
/// settings: they are chosen, they gave no address, the site is not
/// bypassed, and the computer has a proxy in force (asked last, as it may
/// run `scutil`).
fn refuses_unrouted(
    system: bool,
    has_address: bool,
    bypassed: bool,
    in_force: impl FnOnce() -> bool,
) -> bool {
    system && !has_address && !bypassed && in_force()
}

impl Options {
    /// The attempts and the wait between them, as the retry loops take them.
    pub fn tries(&self) -> retry::Tries {
        retry::Tries {
            attempts: self.attempts,
            wait: (self.retry_wait > 0).then(|| std::time::Duration::from_secs(self.retry_wait)),
        }
    }

    /// The proxy a download of `url` goes through, for the tools and the
    /// FTP client: `proxy`, unless the site is on the bypass list; where a
    /// configuration script chooses, what it says for the site.
    pub fn proxy_for(&self, url: &str) -> Option<String> {
        let host = category::host_of(url);
        let bypassed = crate::proxy::Proxy {
            bypass: self.proxy_bypass.clone(),
            ..Default::default()
        }
        .bypasses(&host);
        if bypassed {
            None
        } else {
            crate::proxy::route_for(self.proxy.as_deref(), url)
        }
    }

    /// Why a connection of dm's own (FTP, a torrent's swarm) to `url` may
    /// not be made: the computer's proxy is in force and names no address
    /// such a connection could use, and the site is not on the bypass list.
    /// A computer with no proxy at all goes direct (it refused every FTP
    /// download and torrent until 2026-11-06).
    pub fn no_route_through_system_proxy(&self, url: &str, what: &str) -> Option<String> {
        let bypassed = crate::proxy::Proxy {
            bypass: self.proxy_bypass.clone(),
            ..Default::default()
        }
        .bypasses(&category::host_of(url));
        refuses_unrouted(
            self.system_proxy,
            self.proxy.is_some(),
            bypassed,
            crate::proxy::system_proxy_in_force,
        )
        .then(|| {
            format!(
                "dm cannot send {what} through the computer's proxy settings, and does not go \
                 around them — name the proxy's address in Options › Advanced"
            )
        })
    }

    /// The connections a download of `url` opens: the setting, or the
    /// site's own limit where it has one.
    pub fn connections_for(&self, url: &str) -> usize {
        let host = category::host_of(url);
        let capped = self
            .host_limits
            .iter()
            .filter(|limit| {
                let want = limit
                    .host
                    .trim()
                    .trim_start_matches('.')
                    .to_ascii_lowercase();
                !want.is_empty() && (host == want || host.ends_with(&format!(".{want}")))
            })
            .map(|limit| limit.connections.max(1))
            .min();
        capped.map_or(self.connections, |cap| cap.min(self.connections))
    }
}

/// What happened, once it stopped happening.
#[derive(Debug)]
pub struct Finished {
    /// Where the finished file is.
    pub path: PathBuf,
    /// Its size in bytes.
    pub bytes: u64,
    /// How many connections it used.
    pub connections: usize,
    /// What it was sorted as, whether or not sorting was on.
    pub category: category::Category,
    /// Whether a digest was checked, and matched.
    pub verified: bool,
    /// How many mirrors the bytes came from. One unless a Metalink named
    /// others that agreed about the file.
    pub mirrors: usize,
    /// Something left behind that the person should hear of, though the
    /// download itself is whole: a torrent's staged copy that could not be
    /// removed after it was copied across disks.
    pub left_behind: Option<String>,
}

/// One place the same bytes can be fetched from.
#[derive(Debug, Clone)]
struct Source {
    url: String,
    /// This mirror's own ETag or Last-Modified. Per-source, because two
    /// mirrors of the same file will not have the same ETag, and sending one
    /// server's validator to another is how a resume gets refused.
    validator: Option<String>,
}

/// Shared state for one download.
struct Job {
    segments: Mutex<Vec<Segment>>,
    /// Set when the bytes either side of a seam, or at the file's end, asked
    /// for again were other bytes than the ones on disk: the connections did
    /// not get one file (`assemble::check_seams`).
    disagreed: std::sync::atomic::AtomicBool,
    /// Which seams have been asked for again, by the range that starts at the seam: each is
    /// asked once, by whichever of its two ranges is whole second.
    seams_checked: Vec<std::sync::atomic::AtomicBool>,
    /// Whether the file's last bytes have been asked for again.
    end_checked: std::sync::atomic::AtomicBool,
    /// The staged file, held open from its making until the finished file is placed or the
    /// download stops (`ranged::release_staged`). macOS writes out what a file still holds in
    /// memory when its last open handle is closed, before the close returns: in the end of a
    /// fast download, the range whose handle happened to close last held its connection 140 to
    /// 160 ms, and every other range opening the file waited with it (`bench` fast, 512 MiB,
    /// 2026-11-11). Held here, no range's close is the last one, and the one that is happens off
    /// the runtime's threads once nothing waits on it.
    holding: std::sync::Mutex<Option<std::fs::File>>,
    /// For each range after the first, the bytes just before it (up to 64)
    /// fetched as an overlap with the range before. `None` until fetched;
    /// compared at assembly against the same bytes on disk, which is the only
    /// way to catch a server that labels bytes from the wrong offset with the
    /// right Content-Range.
    overlaps: Vec<std::sync::Mutex<Option<Vec<u8>>>>,
    limiter: Limiter,
    /// Best first. More than one only when a Metalink named mirrors that
    /// agree about the file.
    sources: Vec<Source>,
    /// How many attempts each range has made, so a retry lands on a different
    /// mirror than the one that just failed it.
    attempts: Vec<AtomicU64>,
    /// Each range's live bounds, for halving one under the connection
    /// filling it. Sized for every range there can ever be, as `overlaps`
    /// and `attempts` are, so a split only fills a slot.
    live: Vec<std::sync::Mutex<Live>>,
    /// What the plan is recorded with, so a split can record it again.
    size: Option<u64>,
    resumable: bool,
    validator: Option<String>,
    parts: PathBuf,
    /// Each range is written into a plain file of its own (`range_path`) rather than at its
    /// offset in the one staged file: where the download folder is on another disk than `parts`
    /// (`stage_by_range`).
    by_range: bool,
    /// The most one request asks for; see [`StreamHints::chunk`].
    chunk: Option<u64>,
    /// This stream's own headers; see [`Headers`].
    headers: Headers,
    control: Control,
    /// The page a download came from, sent as `Referer` on every request.
    referrer: Option<String>,
    /// How many times a range is tried; see `Options::attempts`.
    tries: retry::Tries,
    /// The site's sign-in, sent with each range to its server.
    sign_in: Option<crate::cookies::SignIn>,
    /// Where a retry is said, if anyone is listening.
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    /// Where the free space is read, for a download of no known length.
    room: Room,
    /// Bytes on disk across every range.
    ///
    /// An atomic rather than a field under the segment lock: this is written
    /// on every chunk by every connection, and routing that through one mutex
    /// would have them queueing behind each other to publish a number.
    ///
    /// Shared rather than owned so the sampler can read it without holding
    /// the job — which is what lets streams, which have no job, use the same
    /// sampler instead of a second copy of it.
    downloaded: Arc<AtomicU64>,
    /// What the connections share: how many requests are bringing bytes in now, counted from a
    /// request's first byte to its end, so the connections added past [`OPENED_AT_ONCE`] are
    /// judged once they are flowing; and, while none is, their turns at asking a server they
    /// have all lost, ended for all of them by the first byte one gets (`retry::Together`).
    together: retry::Together,
    /// Told whenever a range's attempt ends or a range is given up for others to take, so a
    /// connection waiting for something worth halving looks again at once (`SPLIT_LOOK`).
    moved: tokio::sync::Notify,
    /// How long this download's requests have waited for their first byte, in microseconds,
    /// averaged as they come: what a halving's new request can expect to wait (`SPLIT_WORTH`).
    first_byte_us: AtomicU64,
    /// When a range last came whole: the ranges still under way take up its share for a moment,
    /// and none is judged for halving until they have (`SPLIT_SETTLE`).
    last_ended: std::sync::Mutex<Option<tokio::time::Instant>>,
}

/// The most connections a download opens at once, whatever more it is allowed. Past these, one
/// more is opened only while the last ones added raised the total rate, and given back when they
/// did not (`ranged::grow`): reference A treats its number as a ceiling in the same way. Allowed 16,
/// on a server that shares 8 MiB/s among all of them and cuts each, opening every one took 10.1
/// and 11.1 s where reference A's 8 took 8.4 and 8.2 s (gaps row 168, 2026-11-09).
pub(super) const OPENED_AT_ONCE: usize = 8;

/// Where a range under way has got to, and where it now ends.
///
/// One small lock per range, taken once per chunk by that range's own
/// connection and only otherwise by a split: `reserved` is claimed before the
/// bytes are written, so a split never moves the end below a byte that is
/// about to land.
#[derive(Debug, Default, Clone, Copy)]
struct Live {
    start: u64,
    end: u64,
    reserved: u64,
    /// A connection has taken it, so it is not waiting for one.
    taken: bool,
    /// Every byte its ledger vouches for was written by this run, or its end has been checked
    /// against the server once in it: a reconnect then goes on from the ledger without asking
    /// for that end again (`ranged::attempt_segment`). The check is for what a power cut or a
    /// crash leaves, which cannot happen to a run that is still going; asked again on every
    /// reconnect, it was a sixteenth of each request on a server that cuts connections every
    /// megabyte or so (gaps row 167).
    trusted: bool,
    /// How its attempt is going, for whether halving it is worth a request (`Going::time_left`).
    going: Going,
}

/// How a range's current attempt is going: when it asked, the pace its bytes came at over the
/// last [`PACE_WINDOW`], and when its latest bytes came. Kept beside `Live::reserved` and set under
/// the same lock, once an attempt and once a chunk, so the pace a split is judged by costs no lock
/// of its own.
#[derive(Debug, Default, Clone, Copy)]
struct Going {
    /// When the attempt asked for its bytes.
    since: Option<tokio::time::Instant>,
    /// Where the window being measured began: when, and what the range held then.
    window: Option<(tokio::time::Instant, u64)>,
    /// Bytes a second over the last whole window. Over the last one, not since the attempt
    /// began: a connection that shared the line with seven others is judged by the pace it has
    /// once they are done, not by the one it had among them.
    pace: Option<f64>,
    /// When its latest bytes came.
    latest: Option<tokio::time::Instant>,
}

impl Going {
    /// Counts bytes that came at `arrived`, the range then holding `held`: a window is closed and
    /// its pace taken once it is [`PACE_WINDOW`] long. Whether these were the attempt's first.
    fn came(&mut self, arrived: tokio::time::Instant, held: u64) -> bool {
        self.latest = Some(arrived);
        let Some((opened, had)) = self.window else {
            // From the first bytes on: what came with them came before the window opened.
            self.window = Some((arrived, held));
            return true;
        };
        let lasted = arrived.saturating_duration_since(opened);
        if lasted >= PACE_WINDOW {
            self.pace = Some(held.saturating_sub(had) as f64 / lasted.as_secs_f64());
            self.window = Some((arrived, held));
        }
        false
    }
}

impl Live {
    /// The bytes it still has to fetch.
    fn left(&self) -> u64 {
        (self.end - self.start)
            .saturating_add(1)
            .saturating_sub(self.reserved)
    }

    /// How long its own connection would take to fetch what it has left at the pace of its last
    /// window: endless where the attempt has waited [`STUCK_AFTER`] for an answer or for its next
    /// bytes, `None` where it is too soon to tell (an answer still on its way, or no window yet).
    fn time_left(&self, now: tokio::time::Instant) -> Option<f64> {
        let quiet = |since: Option<tokio::time::Instant>| {
            since.is_some_and(|at| now.saturating_duration_since(at) >= STUCK_AFTER)
        };
        if quiet(self.going.latest.or(self.going.since)) {
            return Some(f64::INFINITY);
        }
        let pace = self.going.pace?;
        Some(self.left() as f64 / pace)
    }
}

/// The least free space a download of no known length may leave. Such a body
/// has no end to plan the room for: a server that sends on for ever, or a
/// small compressed one that unpacks to gigabytes (a response whose
/// `Content-Encoding` the engine undoes), wrote until the disk was full.
const DISK_FLOOR: u64 = 256 * 1024 * 1024;

/// How much a download of no known length writes between looks at the free
/// space.
const DISK_LOOK_EVERY: u64 = 32 * 1024 * 1024;

/// The most ranges a download can end up with, splits included.
const MAX_RANGES: usize = 256;

/// A range is halved only if each half would be at least this. 16 KB:
/// reference A halved a 100 KB file from a slow server into four, pieces
/// down to about 14 KB, where 128 KB left dm on one connection for it
/// (`docs/reference-behavior.md`, measured 2026-10-31); 256 KB had left a
/// 1.5 MB file on a connection-capped server a third slower. A fast server
/// never sees the halving of a small file: the helpers that do it wait
/// first (`HELP_AFTER`).
const MIN_SPLIT: u64 = 16 * 1024;

/// The least time a range must have left at its own pace for a connection with nothing to do to
/// halve it, or twice what a request waited for its first byte in this download, if more. Below
/// that the new request saves less than it costs: it waits for its answer while the range's own
/// connection carries on, and the halving drops that connection's answer short, so whatever the
/// server had sent past the new end comes again. Halved however soon it would end, the end of a
/// fast file was a cascade: nine halvings of its last range in 5 ms, 38 to 60 requests for 8
/// ranges, and 4 to 9% of 512 MiB fetched twice (`bench` fast, macOS, 2026-11-11; gaps rows 171
/// and 177).
const SPLIT_WORTH: Duration = Duration::from_millis(100);

/// How long a range may wait for its answer, or go without bytes, before it counts as stuck and
/// is halved however little it has left: a request the server has not answered in that time is
/// not to be waited on, where one answered a few milliseconds late (a busy server) is.
const STUCK_AFTER: Duration = Duration::from_secs(1);

/// The window a range's pace is measured over (`Going::pace`).
const PACE_WINDOW: Duration = Duration::from_millis(20);

/// How long after a range comes whole the others are first judged for halving: what it was
/// fetching goes to them, on a link or a server they share, and they are judged at the pace they
/// have once they have it. Judged at once by the connection just freed, a fast file's last ranges
/// looked three to four times slower than they were and were halved for nothing; judged 40 ms
/// after its own range alone, while the ranges beside it were still ending, one in two fast runs
/// still halved one (`bench` fast, macOS, 2026-11-11).
const SPLIT_SETTLE: Duration = Duration::from_millis(40);

/// How soon a connection that found nothing worth halving looks again, unless a range ends first:
/// a range just asked for is judged once its bytes have come for a window, and one can slow down
/// or stop, and is then worth helping after all.
const SPLIT_LOOK: Duration = Duration::from_millis(25);

impl Job {
    /// The one file every range of this download is written into, each at
    /// its own offset; renamed into place when whole.
    fn staged(&self) -> PathBuf {
        staged_path(&self.parts)
    }

    /// The file range `index`, starting at `start`, is written into, and where in it the range's
    /// first byte is: its own file from the front, or the staged file at its offset.
    fn held(&self, index: usize, start: u64) -> (PathBuf, u64) {
        if self.by_range {
            (range_path(&self.parts, index), 0)
        } else {
            (self.staged(), start)
        }
    }

    /// Counts how long a request waited for its first byte into the average.
    fn note_first_byte(&self, waited: Duration) {
        let sample = u64::try_from(waited.as_micros()).unwrap_or(u64::MAX).max(1);
        let _ = self
            .first_byte_us
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |was| {
                Some(if was == 0 {
                    sample
                } else {
                    (was.saturating_mul(3).saturating_add(sample)) / 4
                })
            });
    }

    /// The least time left at a range's own pace that is worth a new request for its back half.
    fn split_worth(&self) -> Duration {
        let waited = Duration::from_micros(self.first_byte_us.load(Ordering::Relaxed));
        SPLIT_WORTH.max(waited.saturating_mul(2))
    }

    /// How much of range `index` is on disk, as its ledger last said.
    fn ledger(&self, index: usize) -> u64 {
        read_ledger(&self.parts, index).unwrap_or(0)
    }

    /// Range `index`'s ledger, open for an attempt to write. Held open, as
    /// a part file was: a connection still winding down after a pause
    /// writes through it into a file already removed with the row, rather
    /// than making one again in a folder being deleted.
    async fn open_ledger(&self, index: usize) -> Result<fs::File, Error> {
        Ok(fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(ledger_path(&self.parts, index))
            .await?)
    }

    /// Which mirror this range should use on this attempt.
    ///
    /// Ranges are spread across mirrors by their own index, so a download
    /// with eight connections and three mirrors draws from all three at once
    /// — the point of having them. Adding the attempt count moves a range off
    /// a mirror that has just failed it rather than retrying the same one.
    fn source(&self, index: usize) -> &Source {
        let attempt = self.attempts[index].load(Ordering::Relaxed) as usize;
        &self.sources[(index + attempt) % self.sources.len()]
    }
}

tokio::task_local! {
    /// Set while one stream of a video page is fetched (`videopage::fetch`): the
    /// address is the file, so a refusal or a page in its place is the
    /// stream's failure, not a page to try reading.
    static STREAM: ();

    /// How one stream of a video page is served, from the reading, set
    /// beside [`STREAM`]. Kept here rather than on `Options`, which callers
    /// build field by field.
    static HINTS: StreamHints;
}

/// How the server of a stream wants to be asked, as the reading of the page
/// gave it. Data only: nothing here names a site.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StreamHints {
    /// The rest of the headers its reading says the server is asked with.
    pub headers: Headers,
    /// The most bytes one request may ask for (`downloader_options`'
    /// `http_chunk_size`): a server that slows or cuts any one response
    /// after that is asked for the next slice on a fresh one.
    pub chunk: Option<u64>,
}

/// Headers a video site's reading gives for one stream (`http_headers`),
/// kept for that stream's requests alone.
///
/// Only what dm does not own: the agent goes by dm's table of agents, and
/// `Range`, `If-Range`, `Accept-Encoding` (the engine needs the file as it
/// is) and the connection's own are dm's. A `Cookie` can be a sign-in, so
/// the values are never printed, and it is sent only where dm has none of
/// its own for the site, as a `Referer` only where it has none.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct Headers(Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>);

impl std::fmt::Debug for Headers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(n, _)| n))
            .finish()
    }
}

impl Headers {
    /// The sendable ones of `(name, value)` pairs; the rest are dropped.
    pub fn of<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Headers {
        const DM_OWN: [&str; 14] = [
            "user-agent",
            "range",
            "if-range",
            "accept-encoding",
            "host",
            "content-length",
            "content-type",
            "transfer-encoding",
            "connection",
            "te",
            "upgrade",
            "expect",
            "proxy-authorization",
            "authorization",
        ];
        Headers(
            pairs
                .into_iter()
                .filter(|(name, _)| !DM_OWN.iter().any(|own| own.eq_ignore_ascii_case(name)))
                .filter_map(|(name, value)| {
                    Some((
                        reqwest::header::HeaderName::from_bytes(name.as_bytes()).ok()?,
                        reqwest::header::HeaderValue::from_str(value).ok()?,
                    ))
                })
                .collect(),
        )
    }

    /// `request` with these added, below what dm sets itself.
    pub fn onto(
        &self,
        mut request: reqwest::RequestBuilder,
        own_cookie: bool,
        own_referrer: bool,
    ) -> reqwest::RequestBuilder {
        for (name, value) in &self.0 {
            let held = (*name == reqwest::header::COOKIE && own_cookie)
                || (*name == reqwest::header::REFERER && own_referrer);
            if !held {
                request = request.header(name.clone(), value.clone());
            }
        }
        request
    }
}

/// The headers of the stream being fetched, if one is.
pub(crate) fn stream_headers() -> Headers {
    HINTS
        .try_with(|hints| hints.headers.clone())
        .unwrap_or_default()
}

/// [`download_reporting`] for one stream of a video page (`videopage::fetch`),
/// asked for as its reading says: the address is the file, so what the
/// server refuses, or answers with a page, is that stream's failure.
pub(crate) async fn download_video_stream_with(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    hints: StreamHints,
) -> Result<Finished, Error> {
    // Boxed: a video page's streams are downloads that may themselves meet a
    // page (`videopage::download_page`), which an async fn cannot be recursive in.
    Box::pin(HINTS.scope(
        hints,
        STREAM.scope((), download_reporting(client, url, options, report)),
    ))
    .await
}

/// Whether the download in hand is one stream of a video page.
fn is_stream() -> bool {
    STREAM.try_with(|()| ()).is_ok()
}

/// Downloads `url` into the folder in `options` and returns what came of it;
/// [`download_reporting`] also reports progress as it goes.
pub async fn download(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Finished, Error> {
    let finished = download_reporting(client, url, options, None).await?;
    // Nothing more is done to it here; the queue's runner starts it after its own renames and
    // the mark of where the file came from instead.
    sync_after(finished.path.clone());
    Ok(finished)
}

/// What the engine tells a caller while a download is in flight.
///
/// The name is sent as well as the byte counts because it is learned by the
/// probe, before a single byte has arrived, and a list of long URLs is not a
/// list anyone can read. Waiting for the download to finish to name its own
/// row shows the URL for exactly as long as the name is useful.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// The filename the probe settled on, and what kind of file it is.
    ///
    /// The category comes with the name rather than at assembly because a
    /// list that can be filtered by kind has to know the kind of every row,
    /// not only the finished ones.
    Named {
        /// The filename the server or URL gives to the file.
        name: String,
        /// Which category the file falls into.
        category: category::Category,
    },
    /// How far the download has got.
    Progress(Progress),
    /// A torrent's swarm at this sample: its peers and what was sent to them.
    Swarm(crate::torrent::Swarm),
    /// A torrent's files, once its metadata is known.
    Files(Vec<crate::torrent::TorrentFile>),
    /// The server's ETag or Last-Modified for the file being fetched.
    Validator(String),
    /// A range failed and is about to be tried again: attempt `attempt`
    /// (the first try is 1) of `of`, 0 being "keep trying". Over once bytes
    /// arrive again.
    Retrying {
        /// Which attempt this is; the first try is 1.
        attempt: u32,
        /// Total attempts allowed; 0 means keep trying indefinitely.
        of: u32,
        /// What failed, in the engine's words, so the row can say why.
        why: String,
        /// How long it waits before the next attempt.
        wait: std::time::Duration,
    },
}

/// `download`, with somewhere to send progress.
///
/// A separate entry point rather than a field on `Options`: a channel is not
/// configuration, and `Options` stays something a caller can clone and debug.
pub async fn download_reporting(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let mut done = fetched(client, url, options, report).await?;
    // Its sound as an MP3, where that was asked for: an M4A (or an MP4's
    // sound) read by the system's decoder and encoded by dm (`mp3`).
    let ext = done
        .path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if options.mp3 && matches!(ext.as_str(), "m4a" | "mp4" | "aac") {
        let target = done.path.with_extension("mp3");
        let bytes = crate::mp3::convert_async(done.path.clone(), target.clone())
            .await
            .map_err(|why| Error::Internal(format!("the MP3 could not be made: {why}")))?;
        let _ = tokio::fs::remove_file(&done.path).await;
        done.path = target;
        done.bytes = bytes;
    }
    // A part as a GIF: its pictures decoded by the system, written by dm.
    if options.gif && options.span.is_some() && ext == "mp4" {
        let target = done.path.with_extension("gif");
        let bytes = crate::gif::from_video_async(done.path.clone(), target.clone())
            .await
            .map_err(|why| Error::Internal(format!("the GIF could not be made: {why}")))?;
        let _ = tokio::fs::remove_file(&done.path).await;
        done.path = target;
        done.bytes = bytes;
    }
    Ok(done)
}

/// [`download_reporting`] before any conversion.
async fn fetched(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let counted = Arc::new(AtomicUsize::new(0));
    let outcome = MIRRORS
        .scope(
            counted.clone(),
            Box::pin(fetched_once_more_if_changed(
                client,
                url,
                options,
                report.clone(),
            )),
        )
        .await;
    /* A Metalink whose mirrors give files that disagree (the same length,
     * other bytes) fails as a whole, though one of them may be the file.
     * Each is then tried alone, in the document's order, and what
     * the document's digest accepts is the download; it fails only when
     * none is left. */
    let mirrors = counted.load(Ordering::SeqCst);
    if mirrors > 1
        && matches!(
            outcome,
            Err(Error::DishonestRanges | Error::Checksum { .. } | Error::Changed)
        )
    {
        let mut last = outcome;
        for only in 0..mirrors {
            discard_staged(&options.parts_dir).await;
            let again = ONLY_MIRROR
                .scope(
                    only,
                    MIRRORS.scope(
                        counted.clone(),
                        Box::pin(fetched_once_more_if_changed(
                            client,
                            url,
                            options,
                            report.clone(),
                        )),
                    ),
                )
                .await;
            match again {
                Ok(done) => return Ok(done),
                Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
                Err(e) => last = Err(e),
            }
        }
        return last;
    }
    outcome
}

tokio::task_local! {
    /// How many mirrors the Metalink being fetched named that agree about
    /// the file, told back to `download_reporting`.
    static MIRRORS: Arc<AtomicUsize>;
    /// The one mirror to fetch from, where each is being tried alone.
    static ONLY_MIRROR: usize;
}

async fn fetched_once_more_if_changed(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    /* A file found changed on the server has had its staged parts disowned
     * (their plan is gone), so a second go starts from nothing and fetches
     * the version that is there now — what anyone asking for the file
     * wants, rather than a failed row and a Retry. Once: a server whose
     * bytes differ on every request fails the second time and says so. */
    match fetch_reporting(client, url, options, report.clone()).await {
        // Ranges refused on what was already one connection (a resume, or a server that stopped
        // serving ranges): there is no fewer to fall back to, so the file is fetched again from
        // the start, as a changed one is.
        Err(Error::Changed) | Err(Error::RangesIgnored) => {
            // The parts go as well as their plan: a connection that was
            // writing the plan down as the change was found can put it back,
            // and a plan with no parts to point at adopts nothing.
            discard_staged(&options.parts_dir).await;
            fetch_reporting(client, url, options, report).await
        }
        outcome => outcome,
    }
}

async fn fetch_reporting(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    // Before anything is fetched, by every route below.
    writable(&options.dest_dir).await?;

    // A picture and a sound a player fetched apart: both at once, then joined.
    if let Some(sound) = options.sound.as_deref() {
        let paired = dash::download_media_pair(client, url, sound, options, report.clone()).await;
        /* Addresses taken from a player can stop serving anyone but that
         * player (YouTube's ask for a proof-of-origin token after about a
         * megabyte): a YouTube page is read by dm itself, and its streams
         * fetched by dm all the same. */
        let page = options
            .referrer
            .as_deref()
            .filter(|page| page.starts_with("https://") || page.starts_with("http://"));
        return match (paired, page) {
            (Err(why), Some(page))
                if !matches!(why, Error::Paused | Error::Cancelled)
                    && crate::youtube::video_id(page).is_some() =>
            {
                eprintln!("dm: the player's streams were refused ({why}); reading {page} instead");
                discard_staged(&options.parts_dir).await;
                let page_options = Options {
                    sound: None,
                    ..options.clone()
                };
                crate::videopage::download_page(client, page, &page_options, report).await
            }
            (outcome, _) => outcome,
        };
    }

    /* A torrent is not fetched from a server at all — there is nothing to
     * probe, no length to split into ranges, no address to resume from. It
     * takes a path of its own, before the probe a URL over HTTP would get. */
    if crate::torrent::is_torrent(url) {
        return crate::torrent::download_torrent(client, url, options, report).await;
    }
    // FTP has no HTTP probe either: it asks the server itself.
    if crate::ftp::is_ftp(url) {
        return crate::ftp::download_ftp(url, options, report).await;
    }
    // Nor do RTMP, RTSP and MMS streams, which dm records itself.
    if crate::live::is_live(url) {
        return crate::live::download_live(url, options, report).await;
    }

    // A site with a limit of its own is fetched on that many connections,
    // whatever the setting says; decided once, here, for every path below.
    let options = &Options {
        connections: options.connections_for(url),
        ..options.clone()
    };

    // The probe is as worth retrying as anything after it: a server that is
    // briefly busy should not fail a download before it has begun. Each
    // retry is said, as a range's is.
    let retrying = |attempt, of, error: &Error, wait| {
        if let Some(tx) = &report {
            let why = error.to_string();
            let _ = tx.try_send(Update::Retrying {
                attempt,
                of,
                why,
                wait,
            });
        }
    };
    let probed = retry::run_up_to_telling(&options.control, options.tries(), retrying, || {
        or_stop(
            &options.control,
            probe_get(
                client,
                url,
                options.referrer.as_deref(),
                options.sign_in.as_ref(),
            ),
        )
    })
    .await;
    /* A YouTube page refused to dm's own request (a 429 bot check, a 403
     * to a client that is not a browser) is read by dm's YouTube reader,
     * which asks as a different client. Only YouTube; other sites' videos
     * come from the browser extension's caught requests. */
    let (mut info, mut first_response) = match probed {
        Err(
            refused @ Error::Refused {
                status: 403 | 429, ..
            },
        ) if !is_stream() && !names_a_document(url) && crate::youtube::video_id(url).is_some() => {
            return match crate::videopage::download_page(client, url, options, report.clone()).await
            {
                Ok(done) => Ok(done),
                Err(stopped @ (Error::Paused | Error::Cancelled)) => Err(stopped),
                Err(_) => Err(refused),
            };
        }
        other => {
            let ProbeWithBody { probe, response } = other?;
            (probe, Some(response))
        }
    };

    // A part of a video is cut from a video page's streams (`clip`), from an
    // HLS or DASH stream's segments, or from a plain MP4 by its sample
    // tables (`plain`); any other file is not fetched whole in its place
    // without a word.
    if options.span.is_some()
        && !is_stream()
        && !crate::videopage::is_page(info.content_type.as_deref())
        && crate::youtube::video_id(url).is_none()
        && stream::kind_of(url, info.content_type.as_deref()).is_none()
    {
        if plain::is_mp4(&info.filename, info.content_type.as_deref()) {
            return plain::download_mp4_part(client, url, &info, options, report).await;
        }
        return Err(Error::Internal(
            "a part can be cut only from a video page, an HLS or DASH stream or an MP4 file;              this address is another kind of file"
                .into(),
        ));
    }
    /* A manifest is not a file. It lists segments that concatenate into one,
     * with no length to split into ranges and no position to resume from, so
     * it takes a path of its own rather than being fetched as the few
     * kilobytes of text it literally is. */
    /* A DASH manifest routinely carries picture and sound as separate
     * representations; dm reads it, fetches both and joins them itself
     * (`dash`, `mp4mux`, `mkvmux`). Decided once, here, from the URL and the
     * content type together — a manifest served as `application/dash+xml`
     * from a URL that does not end in `.mpd` is DASH just the same. */
    let kind = stream::kind_of(url, info.content_type.as_deref());
    match kind {
        Some(stream::Kind::Dash) => {
            return dash::download_dash(client, url, options, report).await;
        }
        Some(stream::Kind::Hls) => {
            return download_stream(client, url, options, report).await;
        }
        None => {}
    }

    /* A web page, not a file — a video site's watch URL, say. YouTube
     * pages are read by dm itself; every other site's video comes from the
     * browser extension's caught player requests, not from reading the
     * page. */
    /* A `.zip` link answered as text/html is either the zip with a careless
     * type, which is fetched as the file it is, or a page standing in for
     * it — a sign-in, an expired link. The first bytes tell the two
     * apart. */
    let page = crate::videopage::is_page(info.content_type.as_deref());
    if page && is_stream() {
        return Err(Error::Internal(
            "the stream answered with a web page rather than the file".into(),
        ));
    }
    if page && names_a_file(url) {
        if starts_as_html(client, url, options).await {
            if options.accept_page {
                info.filename = html_name(&info.filename);
            } else {
                return Err(Error::Internal(
                    "that link answered with a web page rather than the file — it may have \
                     expired, or need a sign-in"
                        .into(),
                ));
            }
        }
    } else if page {
        /* A YouTube page is read by dm itself; every other site's video
         * comes from the browser extension's caught player requests. An
         * address that names a document (`.html`, `.xml`) may be the
         * document itself, so a non-YouTube page that looks like one falls
         * through to be fetched as the file it literally is. */
        let document = names_a_document(url);
        let youtube = crate::youtube::video_id(url).is_some();
        if youtube {
            match crate::videopage::download_page(client, url, options, report.clone()).await {
                Err(e)
                    if document
                        && !options.media
                        && !matches!(e, Error::Paused | Error::Cancelled) => {}
                outcome => return outcome,
            }
        } else if !document && options.media {
            /* A page the browser's panel handed over for its video (gaps row 155): dm reads
             * YouTube's pages only, and this site's video comes from its player's own files,
             * which the browser had not seen. Not an expired link, and not a page to save. */
            return Err(Error::Internal(crate::PAGE_NOT_READ.into()));
        } else if !document {
            /* Asked about in the window as an address that ends like a file is (contract row
             * B40, gaps row 136): a person who typed a page may want it, and Yes saves it as
             * HTML. The words say the asking's phrase, and where a video comes from. */
            if options.accept_page {
                info.filename = html_name(&info.filename);
            } else {
                return Err(Error::Internal(
                    "that address answered with a web page rather than the file; its video, if \
                     any, comes from the browser's Download video button when the page plays"
                        .into(),
                ));
            }
        }
    }

    let mut checksum = options.checksum.clone();
    let mut sources = vec![Source {
        url: info.final_url.clone(),
        validator: info.validator.clone(),
    }];

    /* A mirror list rather than a file. What was probed is a few hundred
     * bytes of XML; the download this call is really about has not started
     * yet, so everything gathered so far is replaced by what the document
     * names. */
    if metalink::is_metalink(url, info.content_type.as_deref()) {
        first_response = None;
        let listed = fetch_metalink(client, &info.final_url, options).await?;
        let (mirrors, agreed) =
            or_stop(&options.control, agreeing_mirrors(client, &listed)).await?;

        let _ = MIRRORS.try_with(|n| n.store(mirrors.len(), Ordering::SeqCst));
        sources = match ONLY_MIRROR.try_with(|only| *only) {
            // The mirror counted on the first try may have stopped answering,
            // or stopped agreeing, since: that try fails as any other.
            Ok(only) => match mirrors.get(only) {
                Some(mirror) => vec![mirror.clone()],
                None => {
                    return Err(Error::Connection(
                        "that mirror no longer answers, or no longer agrees about the file".into(),
                    ))
                }
            },
            Err(_) => mirrors,
        };
        info = agreed;
        if !listed.filename.is_empty() {
            info.filename = listed.filename.clone();
        }
        // The caller's digest wins: it is the one a person typed in, and a
        // Metalink is a document from the same place as the file.
        checksum = checksum.or(listed.checksum);
    }

    fs::create_dir_all(&options.parts_dir).await?;
    fs::create_dir_all(&options.dest_dir).await?;

    let segments = planned_segments(&info, options).await?;
    let planned = segments.len();
    // Room for the ranges splitting may add, where the length is known and
    // the server serves ranges; none otherwise.
    let capacity = if info.resumable && info.size.is_some() {
        planned.max(MAX_RANGES)
    } else {
        planned
    };
    let live = segments.clone();
    // A plain file a range where the download folder is on another disk (gaps row 166).
    let by_range = {
        let (parts, dest) = (options.parts_dir.clone(), options.dest_dir.clone());
        let by_ranges = info.resumable && info.size.is_some_and(|size| size > 0);
        tokio::task::spawn_blocking(move || stage_by_range(&parts, &dest, by_ranges))
            .await
            .unwrap_or(false)
    };

    let job = Arc::new(Job {
        disagreed: std::sync::atomic::AtomicBool::new(false),
        seams_checked: (0..capacity)
            .map(|_| std::sync::atomic::AtomicBool::new(false))
            .collect(),
        end_checked: std::sync::atomic::AtomicBool::new(false),
        holding: std::sync::Mutex::new(None),
        segments: Mutex::new(segments),
        limiter: options.limiter.clone(),
        sources,
        attempts: (0..capacity).map(|_| AtomicU64::new(0)).collect(),
        overlaps: (0..capacity).map(|_| std::sync::Mutex::new(None)).collect(),
        live: (0..capacity)
            .map(|i| {
                std::sync::Mutex::new(live.get(i).map_or(Live::default(), |s| Live {
                    start: s.start,
                    end: s.end,
                    reserved: 0,
                    taken: false,
                    trusted: false,
                    going: Going::default(),
                }))
            })
            .collect(),
        size: info.size,
        resumable: info.resumable,
        validator: info.validator.clone(),
        parts: options.parts_dir.clone(),
        by_range,
        chunk: HINTS.try_with(|hints| hints.chunk).ok().flatten(),
        headers: stream_headers(),
        downloaded: Arc::new(AtomicU64::new(0)),
        // Spread over the connections asked for: those of a small file, fewer, still ask no
        // more than once a round each.
        together: retry::Together::new(options.connections.clamp(1, plan::MAX_CONNECTIONS)),
        moved: tokio::sync::Notify::new(),
        first_byte_us: AtomicU64::new(0),
        last_ended: std::sync::Mutex::new(None),
        control: options.control.clone(),
        // The ranges go straight to where the probe landed: past a redirect
        // to another host they carry what the hop carried, the page's origin.
        referrer: crate::redirects::referrer_for(options.referrer.as_deref(), url, &info.final_url),
        tries: options.tries(),
        sign_in: options.sign_in.clone(),
        report: report.clone(),
        room: options.room.clone(),
    });

    // Anything already staged from a previous run counts as done. Read from
    // the files rather than remembered, because the files are the truth.
    adopt_staged(&job).await?;

    // A resume that already has bytes for segment 0 cannot use the probe's
    // response: it streams from byte 0, and the bytes on disk started there.
    // A capped stream (chunk) makes many small requests on purpose — each
    // under the cap — so the probe's full-file body defeats it. Several ranges under a speed
    // limit would have the body read ahead of the pacing, and the shown speed
    // overshoots; one range keeps it.
    if first_response.is_some()
        && (job.ledger(0) > 0 || job.chunk.is_some() || (job.limiter.is_limited() && planned > 1))
    {
        first_response = None;
    }

    let claimed = match info.size {
        Some(size) => Some(room_for(size, &job.downloaded, options)?),
        None => None,
    };
    // Once there is known to be room for it.
    preallocate(&job).await?;

    if let Some(tx) = &report {
        // Before the first byte: this is the whole point of sending it.
        let _ = tx
            .send(Update::Named {
                name: info.filename.clone(),
                category: category::classify(&info.filename, info.content_type.as_deref()),
            })
            .await;
        if let Some(validator) = &info.validator {
            let _ = tx.send(Update::Validator(validator.clone())).await;
        }
    }
    let again = report.clone();
    let reporter = report.map(|tx| watch(Arc::clone(&job.downloaded), info.size, tx));
    let wanted = options.connections.clamp(1, plan::MAX_CONNECTIONS);
    // No more at once than `OPENED_AT_ONCE`, even on a plan an earlier run recorded with more.
    let workers = wanted.min(planned).min(OPENED_AT_ONCE);
    // Connections beyond the ranges planned, for a file small enough to be one
    // or two: they join late and only if the server is slow (`ranged::run`);
    // and those past `OPENED_AT_ONCE`, only while they raise the rate (`ranged::grow`).
    let helpers = if info.resumable && info.size.is_some() {
        wanted - workers
    } else {
        0
    };
    let outcome = run(
        client,
        &job,
        planned,
        workers,
        helpers,
        info.resumable,
        first_response,
    )
    .await;

    if let Some((handle, finished)) = reporter {
        // Ask for a closing sample, then wait for it to be sent.
        let _ = finished.send(());
        let _ = handle.await;
    }
    // Not assembled: whatever comes next removes the parts, makes them again or leaves them.
    if outcome.is_err() || job.disagreed.load(std::sync::atomic::Ordering::SeqCst) {
        release_staged(&job, true).await;
    }
    /* A server that stopped serving ranges to several connections serves
     * one: the file again on a single connection, from the start. The parts
     * go first, or the recorded plan of many ranges would be resumed. */
    if matches!(outcome, Err(Error::RangesIgnored)) && options.connections > 1 {
        let mut listing = fs::read_dir(&options.parts_dir).await?;
        while let Some(entry) = listing.next_entry().await? {
            let _ = fs::remove_file(entry.path()).await;
        }
        let one = Options {
            connections: 1,
            ..options.clone()
        };
        // Its own claim is made again by the run on one connection.
        drop(claimed);
        return Box::pin(download_reporting(client, url, &one, again)).await;
    }
    outcome?;

    /* A range's last bytes, asked for again, were not the bytes on disk: the
     * connections did not all get the same file. Fetched again from the
     * start on one connection, where there is no seam, and the row says why. */
    if job.disagreed.load(std::sync::atomic::Ordering::SeqCst)
        && options.connections > 1
        && workers > 1
    {
        let mut listing = fs::read_dir(&options.parts_dir).await?;
        while let Some(entry) = listing.next_entry().await? {
            let _ = fs::remove_file(entry.path()).await;
        }
        if let Some(to) = &again {
            let _ = to
                .send(Update::Retrying {
                    attempt: 1,
                    of: 1,
                    why: "the pieces fetched over several connections did not agree when \
                          checked again, so the file is fetched again over one"
                        .into(),
                    wait: std::time::Duration::ZERO,
                })
                .await;
        }
        let one = Options {
            connections: 1,
            ..options.clone()
        };
        drop(claimed);
        return Box::pin(download_reporting(client, url, &one, again)).await;
    }

    /* A Metalink supplies its own digest where the caller gave none, so the
     * options handed to assembly carry whichever applies — one place decides
     * whether a file is the file. */
    let verified = checksum.is_some();
    let options = &Options {
        checksum,
        ..options.clone()
    };
    let assembled = assemble(&job, &info, options).await;
    // Placed, or never to be: the staged file is let go of, in the background once it is placed.
    release_staged(&job, assembled.is_err()).await;
    /* Bytes that differ across a seam when asked for a second time: the
     * connections did not all get the same file, or a server answers ranges
     * with other bytes. The parts are gone (`assemble` removed them); the
     * file is fetched again from the start on one connection, where there
     * is no seam, and the row says why. */
    if matches!(assembled, Err(Error::DishonestRanges)) && options.connections > 1 && workers > 1 {
        if let Some(to) = &again {
            let _ = to
                .send(Update::Retrying {
                    attempt: 1,
                    of: 1,
                    why: "the pieces fetched over several connections did not agree when \
                          checked again, so the file is fetched again over one"
                        .into(),
                    wait: std::time::Duration::ZERO,
                })
                .await;
        }
        let one = Options {
            connections: 1,
            ..options.clone()
        };
        drop(claimed);
        return Box::pin(download_reporting(client, url, &one, again)).await;
    }
    let (path, bytes, category) = assembled?;
    // The server's date on the file, where asked: a file a person sorts by
    // date then sorts by when it was made, not when it was fetched.
    if options.server_dates {
        if let Some(when) = info.modified.as_deref().and_then(http_date) {
            // A file system that keeps no such date leaves the download's
            // own, which is no reason to fail a whole file.
            let _ = std::fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(when));
        }
    }

    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: workers,
        category,
        verified,
        mirrors: job.sources.len(),
    })
}

/// `name` as an HTML page's: its extension replaced by `.html`, so a `.zip`
/// that was a page is saved as the page it is.
pub(crate) fn html_name(name: &str) -> String {
    let path = std::path::Path::new(name);
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") => {
            name.to_string()
        }
        _ => {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
            format!("{stem}.html")
        }
    }
}

#[cfg(test)]
mod capped_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn under_the_computers_proxy_only_a_proxy_in_force_with_no_address_refuses() {
        // A proxy in force (a configuration script alone) and no address: refused.
        assert!(super::refuses_unrouted(true, false, false, || true));
        // No proxy on the computer: direct.
        assert!(!super::refuses_unrouted(true, false, false, || false));
        // An address to go through, a bypassed site, or settings of dm's own:
        // the computer is not asked.
        for (system, address, bypassed) in [
            (true, true, false),
            (true, false, true),
            (false, false, false),
        ] {
            assert!(!super::refuses_unrouted(
                system,
                address,
                bypassed,
                || panic!("asked")
            ));
        }
    }

    /// A range of 1 MiB from `start`, `held` of it fetched, its attempt asked at `asked`.
    fn live(held: u64, asked: tokio::time::Instant) -> super::Live {
        super::Live {
            start: 0,
            end: 1024 * 1024 - 1,
            reserved: held,
            taken: true,
            trusted: true,
            going: super::Going {
                since: Some(asked),
                ..Default::default()
            },
        }
    }

    #[test]
    fn a_range_is_judged_at_the_pace_of_its_last_window() {
        use std::time::Duration;
        let t0 = tokio::time::Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut l = live(0, t0);
        // Asked, nothing yet: too soon to tell.
        assert_eq!(l.time_left(ms(5)), None);
        // 64 KiB every 10 ms: the first bytes open the window, the next close it once it is
        // 20 ms long.
        for (at, held) in [(10, 64), (20, 128), (30, 192)] {
            l.reserved = held * 1024;
            let first = l.going.came(ms(at), l.reserved);
            assert_eq!(first, at == 10);
        }
        // 128 KiB over the 20 ms window: 6.5 MB/s, with 832 KiB left.
        let left = l.time_left(ms(31)).expect("a whole window was measured");
        assert!((left - 832.0 / 128.0 * 0.020).abs() < 1e-9, "{left}");
        // A connection that shared the line and then had it to itself: judged by the new pace.
        for (at, held) in [(40, 448), (50, 704)] {
            l.reserved = held * 1024;
            l.going.came(ms(at), l.reserved);
        }
        let left = l.time_left(ms(51)).expect("judged");
        assert!((left - 320.0 / 512.0 * 0.020).abs() < 1e-9, "{left}");
    }

    #[test]
    fn a_range_that_waited_a_second_for_its_answer_or_its_next_bytes_is_stuck() {
        use std::time::Duration;
        let t0 = tokio::time::Instant::now();
        let later = |d: Duration| t0 + d;
        // No answer: too soon to tell, then stuck.
        let l = live(0, t0);
        assert_eq!(l.time_left(later(Duration::from_millis(900))), None);
        assert_eq!(l.time_left(later(super::STUCK_AFTER)), Some(f64::INFINITY));
        // Bytes, then none for a second: stuck, whatever its pace was.
        let mut l = live(0, t0);
        l.going.came(later(Duration::from_millis(10)), 1000);
        l.going.came(later(Duration::from_millis(40)), 900_000);
        assert!(l
            .time_left(later(Duration::from_millis(41)))
            .is_some_and(f64::is_finite));
        assert_eq!(
            l.time_left(later(Duration::from_millis(40) + super::STUCK_AFTER)),
            Some(f64::INFINITY)
        );
    }

    #[test]
    fn a_ts_name_loses_its_extension_in_any_case() {
        assert_eq!(super::ts_stem("show.ts"), "show");
        assert_eq!(super::ts_stem("SHOW.TS"), "SHOW");
        assert_eq!(super::ts_stem("show.mp4"), "show.mp4");
        assert_eq!(super::ts_stem("ts"), "ts");
    }

    use super::*;

    #[test]
    fn only_a_known_file_type_makes_an_address_a_file() {
        assert!(names_a_file("https://e.com/files/report.zip"));
        assert!(names_a_file("https://e.com/a/b/Setup.EXE?x=1"));
        assert!(!names_a_file(
            "https://archive.org/details/night.of.the.living.dead"
        ));
        assert!(!names_a_file("https://video.example/@some.handle"));
        assert!(!names_a_file("https://e.com/view.php?id=4"));
        assert!(!names_a_file("https://e.com/watch"));
    }

    #[test]
    fn a_page_is_known_by_its_opening() {
        assert!(looks_like_html(b"\xef\xbb\xbf  <!DOCTYPE html><html>"));
        assert!(looks_like_html(b"\n<script>location='/login'</script>"));
        assert!(!looks_like_html(b"PK\x03\x04 zip bytes"));
        assert!(!looks_like_html(b"%PDF-1.7"));
    }

    #[tokio::test]
    async fn two_files_finishing_at_once_never_share_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let claims: Vec<_> = (0..16)
            .map(|_| {
                let dir = dir.path().to_path_buf();
                tokio::spawn(async move { claim_path(&dir, "f.bin").await.unwrap() })
            })
            .collect();
        let mut taken = std::collections::HashSet::new();
        for claim in claims {
            assert!(
                taken.insert(claim.await.unwrap()),
                "one name was given twice"
            );
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 16);
    }

    /// The name shown for a file still downloading is the one it will be
    /// placed under (`big.bin` until it finished as `big (1).bin`).
    #[tokio::test]
    async fn the_free_name_is_the_one_a_claim_takes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_name(dir.path(), "big.bin"), "big.bin");
        std::fs::write(dir.path().join("big.bin"), b"old").unwrap();
        assert_eq!(free_name(dir.path(), "big.bin"), "big (1).bin");
        let claimed = claim_path(dir.path(), "big.bin").await.unwrap();
        assert_eq!(claimed, dir.path().join("big (1).bin"));
        assert_eq!(free_name(dir.path(), "big.bin"), "big (2).bin");
        assert_eq!(free_name(dir.path(), "README"), "README");
    }

    #[tokio::test]
    async fn a_folder_already_there_is_not_moved_onto() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("album")).unwrap();
        std::fs::write(dir.path().join("album/mine.txt"), b"keep").unwrap();
        let claimed = claim_dir(dir.path(), "album").await.unwrap();
        assert_eq!(claimed, dir.path().join("album (1)"));
        assert_eq!(
            std::fs::read(dir.path().join("album/mine.txt")).unwrap(),
            b"keep"
        );
    }

    fn options_in(dir: &Path) -> Options {
        Options {
            connections: 1,
            control: Control::new(),
            dest_dir: dir.join("out"),
            parts_dir: dir.join("parts"),
            limiter: Limiter::unlimited(),
            categories: Categories::default(),
            checksum: None,
            quality: Quality::Best,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            media: false,
            referrer: None,
            proxy: None,
            container: None,
            attempts: 5,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            room: Default::default(),
            remux: false,
            server_dates: false,
            retry_wait: 0,
            record_limit: None,
            frame_rate: None,
            caption: None,
            sign_in: None,
            save_name: None,
            accept_page: false,
            torrent: Default::default(),
            system_proxy: false,
            title_hint: None,
        }
    }

    /// A mount reporting zero free of zero total is unknown, not
    /// full; every download of known length was refused on one.
    #[test]
    fn a_file_system_that_reports_no_size_is_not_taken_as_full() {
        assert_eq!(reported_free(0, 0), None);
        assert_eq!(reported_free(0, 1 << 30), Some(0));
        assert_eq!(reported_free(5, 10), Some(5));
    }

    #[tokio::test]
    async fn a_cancelled_download_is_not_assembled() {
        // The bytes all arrived and then the row was cancelled. The join
        // must not run: the parts are being deleted while it would, and a
        // file made of whichever survived is short and looks finished.
        let dir = tempfile::tempdir().unwrap();
        let options = options_in(dir.path());
        std::fs::create_dir_all(&options.parts_dir).unwrap();
        let parts: Vec<PathBuf> = (0..2).map(|i| part_path(&options.parts_dir, i)).collect();
        for part in &parts {
            std::fs::write(part, b"1234").unwrap();
        }
        options.control.cancel();

        let outcome = finish("f.bin", None, &parts, &options).await;
        assert!(matches!(outcome, Err(Error::Cancelled)), "got {outcome:?}");
        assert!(
            !options.dest_dir.join("f.bin").exists(),
            "a cancelled download was assembled"
        );
    }

    #[tokio::test]
    async fn a_part_taken_away_fails_the_join_rather_than_shortening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let options = options_in(dir.path());
        std::fs::create_dir_all(&options.parts_dir).unwrap();
        std::fs::create_dir_all(&options.dest_dir).unwrap();
        let parts: Vec<PathBuf> = (0..3).map(|i| part_path(&options.parts_dir, i)).collect();
        std::fs::write(&parts[0], b"1234").unwrap();
        // parts[1] never written: it was removed under the join.
        std::fs::write(&parts[2], b"5678").unwrap();

        let into = options.dest_dir.join("f.bin");
        let outcome = join_parts(&parts, &into, &options.control).await;
        assert!(outcome.is_err(), "a missing part was skipped");
        assert!(!into.exists(), "a short file was left under the name");
    }

    /// A Cancel stops the join of a file's parts part-way, within a second, and leaves no file
    /// under the name (gaps row 185). The parts are sparse: only what the join wrote before it
    /// stopped is on the disk.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancel_stops_a_join_part_way() {
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let options = options_in(dir.path());
        std::fs::create_dir_all(&options.parts_dir).unwrap();
        std::fs::create_dir_all(&options.dest_dir).unwrap();
        let parts: Vec<PathBuf> = (0..8).map(|i| part_path(&options.parts_dir, i)).collect();
        for part in &parts {
            std::fs::File::create(part)
                .unwrap()
                .set_len(256 << 20)
                .unwrap();
        }
        let into = options.dest_dir.join("f.bin");
        let joining = {
            let (parts, into, control) = (parts.clone(), into.clone(), options.control.clone());
            tokio::spawn(async move { join_parts(&parts, &into, &control).await })
        };
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !joining.is_finished(),
            "the join was over before it could be cancelled"
        );
        options.control.cancel();
        let outcome = tokio::time::timeout(Duration::from_secs(1), joining)
            .await
            .expect("the join went on for a second after the cancel")
            .unwrap();
        assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
        assert!(!into.exists(), "a part-joined file was left under the name");
    }

    #[test]
    fn a_streams_headers_are_printed_by_name_never_by_value() {
        let headers = Headers::of([("cookie", "sid=hidden-value"), ("x-token", "abc123")]);
        let printed = format!("{headers:?}");
        assert!(
            printed.contains("cookie") && printed.contains("x-token"),
            "{printed}"
        );
        assert!(
            !printed.contains("hidden-value") && !printed.contains("abc123"),
            "{printed}"
        );
    }

    /// A server on 127.0.0.1 answering every path with `body` as `kind`.
    async fn serve_one(kind: &'static str, body: &'static [u8]) -> std::net::SocketAddr {
        let app = axum::Router::new().fallback(move || async move {
            axum::response::Response::builder()
                .header("content-type", kind)
                .header("content-length", body.len())
                .body(axum::body::Body::from(body))
                .unwrap()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    #[tokio::test]
    async fn a_part_of_a_file_that_is_no_video_is_refused_and_nothing_is_fetched_into_place() {
        let addr = serve_one("application/zip", b"PK\x03\x04 not really").await;
        let dir = tempfile::tempdir().unwrap();
        let mut options = options_in(dir.path());
        options.span = crate::clip::Span::new(1.0, 2.0);
        let why = download(
            &reqwest::Client::new(),
            &format!("http://{addr}/files/pack.zip"),
            &options,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(why.contains("a part can be cut only"), "{why}");
        assert!(!options.dest_dir.join("pack.zip").exists());
    }

    #[tokio::test]
    async fn a_page_where_a_videos_stream_should_be_is_that_streams_failure() {
        let addr = serve_one(
            "text/html; charset=utf-8",
            b"<!DOCTYPE html><html><body>sign in</body></html>",
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let options = options_in(dir.path());
        let why = download_video_stream_with(
            &reqwest::Client::new(),
            &format!("http://{addr}/v/stream"),
            &options,
            None,
            StreamHints::default(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(why.contains("answered with a web page"), "{why}");
    }
}
