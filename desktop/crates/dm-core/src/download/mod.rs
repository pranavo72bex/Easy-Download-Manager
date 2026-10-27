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
use crate::probe::{probe, Probe};
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
    /// Which rendition to take from an adaptive stream.
    pub quality: Quality,
    /// The address of a separate sound track for the picture at the download's
    /// own address, which a player fetched apart from it: both are fetched at
    /// once and joined (`dash::download_media_pair`).
    pub sound: Option<String>,
    /// Media was asked for (a player's video, from the browser's panel or
    /// media menu): a page yt-dlp cannot read fails, rather than being
    /// fetched as the document its address ends like (a
    /// `player.html` was saved as its HTML and called complete).
    pub media: bool,
    /// The page a download came from, sent as `Referer`. Set for a download a
    /// browser handed over, where the file is served only to a request that
    /// names its page; `None` for one typed in, which needs no such thing.
    pub referrer: Option<String>,
    /// The proxy, as a URL, for a tool dm runs on its behalf. The client
    /// already carries it for every request of dm's own; yt-dlp makes its own
    /// connections and has to be told, or a page it fetches goes out directly
    /// on a machine where nothing may.
    pub proxy: Option<String>,
    /// The container a video from the browser's panel was listed in ("mp4",
    /// "webm"), preferred when yt-dlp chooses among the formats at its
    /// height, so what arrives is what the list said.
    pub container: Option<String>,
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
    /// Ask yt-dlp for a video's subtitles and carry them inside the video,
    /// put there by dm (`subtitles`).
    pub subtitles: bool,
    /// The site's sign-in the browser handed over with this download, sent
    /// to that server only.
    pub sign_in: Option<crate::cookies::SignIn>,
    /// The name a person chose for the file, which it is placed under in
    /// the first place: placed under the server's name and renamed after,
    /// it held that name for a moment and pushed a download of the same
    /// file finishing then to "name (1)".
    pub save_name: Option<String>,
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
    pub host: String,
    pub connections: usize,
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
    /// FTP client: `proxy`, unless the site is on the bypass list.
    pub fn proxy_for(&self, url: &str) -> Option<&str> {
        let host = category::host_of(url);
        let bypassed = crate::proxy::Proxy {
            bypass: self.proxy_bypass.clone(),
            ..Default::default()
        }
        .bypasses(&host);
        if bypassed {
            None
        } else {
            self.proxy.as_deref()
        }
    }

    /// Why a connection of dm's own (FTP, a torrent's swarm) to `url` may
    /// not be made: the computer's proxy is in force and names no address
    /// such a connection could use, and the site is not on the bypass list.
    pub fn no_route_through_system_proxy(&self, url: &str, what: &str) -> Option<String> {
        let bypassed = crate::proxy::Proxy {
            bypass: self.proxy_bypass.clone(),
            ..Default::default()
        }
        .bypasses(&category::host_of(url));
        (self.system_proxy && self.proxy.is_none() && !bypassed).then(|| {
            format!(
                "dm cannot send {what} through the computer's proxy settings, and does not go \
                 around them — name the proxy's address in Options › Proxy"
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
    pub path: PathBuf,
    pub bytes: u64,
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
    /// For each range after the first, the byte just before it, fetched as
    /// a one-byte overlap with the range before; `NO_OVERLAP` until fetched.
    /// Checked at assembly against the previous range's last byte, which is
    /// the only way to see a server that labels bytes from the wrong offset
    /// with the right Content-Range.
    overlaps: Vec<std::sync::atomic::AtomicU16>,
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
    control: Control,
    /// The page a download came from, sent as `Referer` on every request.
    referrer: Option<String>,
    /// How many times a range is tried; see `Options::attempts`.
    tries: retry::Tries,
    /// The site's sign-in, sent with each range to its server.
    sign_in: Option<crate::cookies::SignIn>,
    /// Where a retry is said, if anyone is listening.
    report: Option<tokio::sync::mpsc::Sender<Update>>,
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
}

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
}

/// The most ranges a download can end up with, splits included.
const MAX_RANGES: usize = 256;

/// A range is halved only if each half would be at least this (128 KB: 256 KB
/// left a 1.5 MB file on a connection-capped server a third slower).
const MIN_SPLIT: u64 = 128 * 1024;

impl Job {
    /// The one file every range of this download is written into, each at
    /// its own offset; renamed into place when whole.
    fn staged(&self) -> PathBuf {
        staged_path(&self.parts)
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
    /// Set while one stream of a video page is fetched (`ytdlp::fetch`): the
    /// address is the file, so a refusal or a page in its place is the
    /// stream's failure, not a page for yt-dlp to try. Handed to it as one,
    /// each of a video's streams cost two more yt-dlp runs (about 1.4 s on
    /// YouTube, which refuses dm's own requests for its streams) before the
    /// helper downloaded the video from its reading as it was always going to.
    static STREAM: ();
}

/// [`download_reporting`] for one stream of a video page (`ytdlp::fetch`):
/// the address is the file, so what the server refuses, or answers with a
/// page, is that stream's failure and nothing is handed to yt-dlp.
pub async fn download_video_stream(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    // Boxed: a video page's streams are downloads that may themselves meet a
    // page (`ytdlp::download_page`), which an async fn cannot be recursive in.
    Box::pin(STREAM.scope((), download_reporting(client, url, options, report))).await
}

/// Whether the download in hand is one stream of a video page.
fn is_stream() -> bool {
    STREAM.try_with(|()| ()).is_ok()
}

pub async fn download(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Finished, Error> {
    download_reporting(client, url, options, None).await
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
        name: String,
        category: category::Category,
    },
    Progress(Progress),
    /// The server's ETag or Last-Modified for the file being fetched.
    Validator(String),
    /// A range failed and is about to be tried again: attempt `attempt`
    /// (the first try is 1) of `of`, 0 being "keep trying". Over once bytes
    /// arrive again.
    Retrying {
        attempt: u32,
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
    /* A file found changed on the server has had its staged parts disowned
     * (their plan is gone), so a second go starts from nothing and fetches
     * the version that is there now — what anyone asking for the file
     * wants, rather than a failed row and a Retry. Once: a server whose
     * bytes differ on every request fails the second time and says so. */
    match fetch_reporting(client, url, options, report.clone()).await {
        Err(Error::Changed) => {
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
         * megabyte): the page they came from is read by the video helper
         * instead, and its streams fetched by dm all the same, rather than
         * the row failing on what the browser could play. */
        let page = options
            .referrer
            .as_deref()
            .filter(|page| page.starts_with("https://") || page.starts_with("http://"));
        return match (paired, page) {
            (Err(why), Some(page))
                if !matches!(why, Error::Paused | Error::Cancelled)
                    && crate::tools::for_video(client).await.is_ok() =>
            {
                eprintln!("dm: the player's streams were refused ({why}); reading {page} instead");
                discard_staged(&options.parts_dir).await;
                let page_options = Options {
                    sound: None,
                    ..options.clone()
                };
                crate::ytdlp::download_page(client, page, &page_options, report).await
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
            probe(
                client,
                url,
                options.referrer.as_deref(),
                options.sign_in.as_ref(),
            ),
        )
    })
    .await;
    /* A page refused to dm's own request (a 429 bot check, a 403 to a
     * client that is not a browser) is yt-dlp's to try: it gets through
     * where a plain request does not, and a video site's watch page failed
     * at the probe though yt-dlp could fetch it (seen with YouTube). Only an
     * address that names no file; if yt-dlp makes nothing of it either, the
     * refusal is what is said. */
    let mut info = match probed {
        Err(
            refused @ Error::Refused {
                status: 403 | 429, ..
            },
        ) if !is_stream()
            && !names_a_document(url)
            && crate::tools::for_video(client).await.is_ok() =>
        {
            return match crate::ytdlp::download_page(client, url, options, report.clone()).await {
                Ok(done) => Ok(done),
                Err(stopped @ (Error::Paused | Error::Cancelled)) => Err(stopped),
                // A site yt-dlp does not know: the page's own refusal says
                // more. Anything else is why the video did not come, which
                // the refusal hid (an old yt-dlp, a bot check), and which
                // is not tried ten times over as the refusal was.
                Err(e) if e.to_string().contains(crate::ytdlp::UNKNOWN_SITE) => Err(refused),
                Err(e) => Err(e),
            };
        }
        other => other?,
    };

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

    /* A web page, not a file — a video site's watch URL, say. There is no
     * body to fetch: the media's address is minted by the site's own player.
     * Where yt-dlp is there to make sense of that (installed, or fetched by
     * dm), the page is handed to it rather than saved as its own HTML, which
     * is never the download
     * anyone meant. */
    /* Unless the address names a file. A `.zip` link answered as text/html
     * is either the zip with a careless type, which is fetched as the file
     * it is, or a page standing in for it — a sign-in, an expired link —
     * which is said for what it is rather than handed to yt-dlp, which
     * would save the page as a "video" or fail about an unknown site. The
     * first bytes tell the two apart. */
    let page = crate::ytdlp::is_page(info.content_type.as_deref());
    if page && is_stream() {
        return Err(Error::Internal(
            "the stream answered with a web page rather than the file".into(),
        ));
    }
    if page && names_a_file(url) {
        if starts_as_html(client, url, options).await {
            return Err(Error::Internal(
                "that link answered with a web page rather than the file — it may have expired, \
                 or need a sign-in"
                    .into(),
            ));
        }
    } else if page {
        /* An address that names a document — `notes.html`, `feed.xml` —
         * may be the document itself, or a video site's watch page that
         * happens to end in `.html`. yt-dlp is asked first, and if it
         * cannot make a video of it, for whatever reason (an unknown site,
         * a 403 to a request without the page's sign-in, a 429), dm's own
         * client fetches the document with the row's cookies and referrer.
         * A watch address with no extension, or a script's (`.php?v=`), is
         * yt-dlp's alone: its HTML is never the download anyone meant. */
        let document = names_a_document(url);
        if crate::tools::for_video(client).await.is_ok() {
            match crate::ytdlp::download_page(client, url, options, report.clone()).await {
                Err(e)
                    if document
                        && !options.media
                        && !matches!(e, Error::Paused | Error::Cancelled) => {}
                outcome => return outcome,
            }
        } else if !document {
            /* No yt-dlp, so nothing can turn this page into a file. Saving
             * the page's own HTML and calling it the download is how
             * "YouTube does not download" looks like success — refused
             * plainly instead, with the one thing that fixes it. */
            return Err(Error::Internal(
                "that is a web page, not a file, and dm's video helper, which finds the video in a page, could not be got ready"
                    .into(),
            ));
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
        let listed = fetch_metalink(client, &info.final_url, options).await?;
        let (mirrors, agreed) =
            or_stop(&options.control, agreeing_mirrors(client, &listed)).await?;

        sources = mirrors;
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

    let job = Arc::new(Job {
        segments: Mutex::new(segments),
        limiter: options.limiter.clone(),
        sources,
        attempts: (0..capacity).map(|_| AtomicU64::new(0)).collect(),
        overlaps: (0..capacity)
            .map(|_| std::sync::atomic::AtomicU16::new(NO_OVERLAP))
            .collect(),
        live: (0..capacity)
            .map(|i| {
                std::sync::Mutex::new(live.get(i).map_or(Live::default(), |s| Live {
                    start: s.start,
                    end: s.end,
                    reserved: 0,
                    taken: false,
                }))
            })
            .collect(),
        size: info.size,
        resumable: info.resumable,
        validator: info.validator.clone(),
        parts: options.parts_dir.clone(),
        downloaded: Arc::new(AtomicU64::new(0)),
        control: options.control.clone(),
        referrer: options.referrer.clone(),
        tries: options.tries(),
        sign_in: options.sign_in.clone(),
        report: report.clone(),
    });

    // Anything already staged from a previous run counts as done. Read from
    // the files rather than remembered, because the files are the truth.
    adopt_staged(&job).await?;

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
    let workers = wanted.min(planned);
    // Connections beyond the ranges planned, for a file small enough to be one
    // or two: they join late and only if the server is slow (`ranged::run`).
    let helpers = if info.resumable && info.size.is_some() {
        wanted - workers
    } else {
        0
    };
    let outcome = run(client, &job, planned, workers, helpers, info.resumable).await;

    if let Some((handle, finished)) = reporter {
        // Ask for a closing sample, then wait for it to be sent.
        let _ = finished.send(());
        let _ = handle.await;
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

    /* A Metalink supplies its own digest where the caller gave none, so the
     * options handed to assembly carry whichever applies — one place decides
     * whether a file is the file. */
    let verified = checksum.is_some();
    let options = &Options {
        checksum,
        ..options.clone()
    };
    let (path, bytes, category) = assemble(&job, &info, options).await?;
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

#[cfg(test)]
mod tests {
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
            sign_in: None,
            save_name: None,
            system_proxy: false,
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
        let outcome = join_parts(&parts, &into).await;
        assert!(outcome.is_err(), "a missing part was skipped");
        assert!(!into.exists(), "a short file was left under the name");
    }
}
