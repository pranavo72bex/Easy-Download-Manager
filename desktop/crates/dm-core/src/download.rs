//! Fetching a file over several connections at once, and putting it back
//! together.
//!
//! Each range streams into its own part file. The length of that file *is*
//! the resume position, so there is no separate ledger to fall out of step
//! with what is actually on disk — a crash costs the bytes that were in
//! flight and nothing else.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// free space (gap 94).
    pub room: Room,
    /// Whether finished files are sorted into folders by kind, and what
    /// those folders are called.
    pub categories: Categories,
    /// A digest the finished file must have. A Metalink supplies its own
    /// where the caller gave none.
    pub checksum: Option<Checksum>,
    /// Which rendition to take from an adaptive stream.
    pub quality: Quality,
    /// Media was asked for (a player's video, from the browser's panel or
    /// media menu): a page yt-dlp cannot read fails, rather than being
    /// fetched as the document its address ends like (UI round 9: a
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
    /// Ask yt-dlp for a video's subtitles and embed them, where an ffmpeg
    /// is there to embed with.
    pub subtitles: bool,
    /// The site's sign-in the browser handed over with this download, sent
    /// to that server only.
    pub sign_in: Option<crate::cookies::SignIn>,
    /// The name a person chose for the file, which it is placed under in
    /// the first place: placed under the server's name and renamed after,
    /// it held that name for a moment and pushed a download of the same
    /// file finishing then to "name (1)" (QA loop 2).
    pub save_name: Option<String>,
    /// The proxy is the computer's own settings. dm's HTTP client reads
    /// them, but where they name no address the environment carries (the
    /// macOS and Windows network settings), FTP and BitTorrent, which
    /// connect by themselves, have none to go through — and must not go
    /// around it.
    pub system_proxy: bool,
    /// Put a stream that arrives as a transport stream (`.ts`) into an MP4,
    /// losslessly, where an ffmpeg is there to do it. Players open an MP4;
    /// few open a `.ts`.
    pub remux: bool,
    /// Give a finished file the date the server says it last changed
    /// (`Last-Modified`) rather than when it was fetched.
    pub server_dates: bool,
    /// Seconds between attempts after a failure; 0 for dm's own pace.
    pub retry_wait: u64,
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

/// A range is halved only if each half would be at least this.
const MIN_SPLIT: u64 = 256 * 1024;

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

pub async fn download(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Finished, Error> {
    download_reporting(client, url, options, None).await
}

/// The same, with somewhere to send progress.
///
/// A separate entry point rather than a field on `Options`: a channel is not
/// configuration, and `Options` stays something a caller can clone and debug.
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
    },
}

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
    // Nor do RTMP, RTSP and MMS streams, which ffmpeg fetches: fetched
    // itself first where the computer has none.
    if crate::live::is_live(url) {
        let _ = crate::tools::ensure(client, crate::tools::Tool::Ffmpeg).await;
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
    let retrying = |attempt, of| {
        if let Some(tx) = &report {
            let _ = tx.try_send(Update::Retrying { attempt, of });
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
        ) if !names_a_document(url) && crate::tools::for_video(client).await.is_ok() => {
            return match crate::ytdlp::download_ytdlp(url, options, report.clone()).await {
                Ok(done) => Ok(done),
                Err(stopped @ (Error::Paused | Error::Cancelled)) => Err(stopped),
                Err(_) => Err(refused),
            };
        }
        other => other?,
    };

    /* A manifest is not a file. It lists segments that concatenate into one,
     * with no length to split into ranges and no position to resume from, so
     * it takes a path of its own rather than being fetched as the few
     * kilobytes of text it literally is. */
    /* A DASH manifest routinely carries picture and sound as separate
     * representations, and joining them needs a muxer. yt-dlp has one — it
     * reads MPDs natively and merges with ffmpeg — so where it is installed a
     * DASH stream is handed to it, the way a player page is. Without it the
     * manifest is refused plainly rather than fetched by half: only the
     * video, reported as a success, is a silent film. Decided once, here,
     * from the URL and the content type together — a manifest served as
     * `application/dash+xml` from a URL that does not end in `.mpd` is DASH
     * just the same. */
    let kind = stream::kind_of(url, info.content_type.as_deref());
    // yt-dlp found, or fetched by dm where the computer has none (`tools`).
    let dash_tools = match kind {
        Some(stream::Kind::Dash) => Some(crate::tools::for_video(client).await),
        _ => None,
    };
    match kind {
        Some(stream::Kind::Dash) if matches!(dash_tools, Some(Ok(()))) => {
            return crate::ytdlp::download_ytdlp(url, options, report).await;
        }
        Some(stream::Kind::Dash) => {
            return Err(Error::Internal(format!(
                "a DASH stream needs yt-dlp to join its picture and sound — install yt-dlp ({})",
                dash_tools.and_then(Result::err).unwrap_or_default()
            )));
        }
        Some(stream::Kind::Hls) => {
            return download_stream(client, url, options, report).await;
        }
        None => {}
    }

    /* A web page, not a file — a video site's watch URL, say. There is no
     * body to fetch: the media's address is minted by the site's own player.
     * Where a yt-dlp is installed to make sense of that, the page is handed
     * to it rather than saved as its own HTML, which is never the download
     * anyone meant. */
    /* Unless the address names a file. A `.zip` link answered as text/html
     * is either the zip with a careless type, which is fetched as the file
     * it is, or a page standing in for it — a sign-in, an expired link —
     * which is said for what it is rather than handed to yt-dlp, which
     * would save the page as a "video" or fail about an unknown site. The
     * first bytes tell the two apart. */
    let page = crate::ytdlp::is_page(info.content_type.as_deref());
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
            match crate::ytdlp::download_ytdlp(url, options, report.clone()).await {
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
                "that is a web page, not a file — install yt-dlp to download from video sites"
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
    let workers = options
        .connections
        .clamp(1, plan::MAX_CONNECTIONS)
        .min(planned);
    let outcome = run(client, &job, planned, workers, info.resumable).await;

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
    // copy reports what it moved, so the finished size needs no extra stat.
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

/// An HTTP date (`Sun, 06 Nov 1994 08:49:37 GMT`) as a time a file can
/// carry. `None` for anything else.
pub(crate) fn http_date(text: &str) -> Option<std::time::SystemTime> {
    let when = chrono::DateTime::parse_from_rfc2822(text.trim()).ok()?;
    let seconds = u64::try_from(when.timestamp()).ok()?;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
}

/// A playlist is text and small; anything claiming otherwise is not one.
const MANIFEST_CAP: usize = 8 * 1024 * 1024;

/// A request the way every request of this download is made.
///
/// One place for the `Referer`, because it was on the probe and the ranges
/// and on nothing else: a stream's playlists, key and segments, a Metalink's
/// body and a `.torrent` file all went out bare, so a download the browser
/// had just made with its page named failed in the app with a 403 one
/// request after the probe had passed.
/// Whether the address's last path segment names a file by a type dm knows
/// as one — an archive, a program, a document, media. Not any dotted name:
/// `/details/night.of.the.living.dead` and `/@some.handle` are pages.
fn names_a_file(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let name = parsed
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("");
    name.contains('.') && crate::category::classify(name, None) != crate::category::Category::Other
}

/// Whether the address's last segment names a document by its extension —
/// `.html`, `.xml`, `.txt` — rather than a script that makes pages
/// (`.php`, `.aspx`, `.cgi`), which is how a watch page is often reached.
fn names_a_document(url: &str) -> bool {
    const SCRIPTS: [&str; 12] = [
        "php", "php5", "asp", "aspx", "jsp", "jspx", "cgi", "pl", "py", "do", "action", "cfm",
    ];
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let name = parsed
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("");
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=5).contains(&extension.len())
        && extension.chars().all(|c| c.is_ascii_alphanumeric())
        && !SCRIPTS.iter().any(|s| s.eq_ignore_ascii_case(extension))
}

/// Whether the body at `url` begins as an HTML document, from its first
/// bytes: a few hundred are asked for, and a server that sends the whole
/// body anyway is read no further than that.
async fn starts_as_html(client: &reqwest::Client, url: &str, options: &Options) -> bool {
    let request = get(
        client,
        url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    )
    .header(reqwest::header::RANGE, "bytes=0-1023");
    let Ok(res) = request.send().await else {
        return false;
    };
    // An error page is not the file either, whatever it starts with.
    if !res.status().is_success() {
        return true;
    }
    let mut stream = res.bytes_stream();
    let mut head = Vec::new();
    while head.len() < 1024 {
        match futures_util::StreamExt::next(&mut stream).await {
            Some(Ok(chunk)) => head.extend_from_slice(&chunk),
            _ => break,
        }
    }
    looks_like_html(&head)
}

/// HTML by its opening: past any byte-order mark and white space, a
/// doctype, `<html`, `<head`, or a comment followed by one of them.
fn looks_like_html(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head.get(..1024).unwrap_or(head)).to_ascii_lowercase();
    let text = text.trim_start_matches('\u{feff}').trim_start();
    [
        "<!doctype html",
        "<html",
        "<head",
        "<!--",
        "<body",
        "<title",
        "<meta",
        "<script",
        "<link",
    ]
    .iter()
    .any(|tag| text.starts_with(tag))
}

/// The file as the server keeps it, never a compressed form of it. A
/// server that compresses text ranges over the compressed bytes (GitHub's
/// raw files, apple.com), so a client that asked for gzip and unpacked it
/// got each range longer than it asked for, and every such file failed as
/// a server that does not serve ranges honestly. Also what a download is:
/// the bytes of the file, which a probe's length and a digest are of.
pub(crate) const IDENTITY: &str = "identity";

pub(crate) fn get(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::agent_for(url))
        .header(reqwest::header::ACCEPT_ENCODING, IDENTITY);
    if let Some(cookies) = crate::cookies::header_with(sign_in, url) {
        request = request.header(reqwest::header::COOKIE, cookies);
    }
    if let Some(referrer) = referrer {
        request = request.header(reqwest::header::REFERER, referrer);
    }
    request
}

/// Fetches something small enough to hold: a manifest, a mirror list, a key.
///
/// Capped, because these are read into memory whole and the URL that produced
/// them is not necessarily one anybody vetted. The cap is enforced while
/// reading — a body that keeps coming is stopped, not buffered whole and
/// then trimmed — and the bytes count against the ceiling and can be paused
/// like any others.
///
/// With the address it was finally served from: a playlist's relative
/// entries are relative to that, which after a redirect is not where it was
/// asked for — a short link that sends a player to a CDN's directory has its
/// renditions and segments there. Joined against the asked address, every
/// one of them was a 404.
pub(crate) async fn fetch_capped(
    client: &reqwest::Client,
    url: &str,
    most: usize,
    options: &Options,
) -> Result<Fetched, Error> {
    let res = retry::run_up_to(&options.control, options.tries(), || async {
        or_stop(
            &options.control,
            crate::auth::send(
                client,
                get(
                    client,
                    url,
                    options.referrer.as_deref(),
                    options.sign_in.as_ref(),
                ),
                url,
            ),
        )
        .await
    })
    .await?;
    let res = Error::refused_or(res).await?;
    let landed = res.url().to_string();
    let mut body = Vec::new();
    let mut chunks = res.bytes_stream();
    while let Some(chunk) = next_or_stop(&mut chunks, &options.control).await? {
        let chunk = chunk?;
        if body.len() + chunk.len() > most {
            return Err(Error::Internal(format!(
                "{url} is larger than anything of its kind should be"
            )));
        }
        body.extend_from_slice(&chunk);
        or_stop(&options.control, async {
            options.limiter.take(chunk.len() as u64).await;
            Ok(())
        })
        .await?;
    }
    Ok(Fetched { body, landed })
}

/// Something small fetched whole, and where it was finally served from.
pub(crate) struct Fetched {
    pub body: Vec<u8>,
    pub landed: String,
}

impl Fetched {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Reads the mirror list a Metalink URL points at.
pub(crate) async fn fetch_metalink(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Metalink, Error> {
    let text = fetch_capped(client, url, 4 * 1024 * 1024, options)
        .await?
        .text();
    metalink::parse(&text)
        .ok_or_else(|| Error::Internal("that Metalink names no file dm can fetch over HTTP".into()))
}

/// The ranges this download is fetched in: the plan its staged parts were
/// fetched under where there are any, a fresh one otherwise.
///
/// Parts staged under a plan that does not match — the file's length has
/// changed, or the parts predate the plan being written down — are not
/// adopted, because there is no offset to trust them at. Starting over costs
/// the bytes; adopting them costs the file.
async fn planned_segments(info: &Probe, options: &Options) -> Result<Vec<Segment>, Error> {
    let parts_dir = options.parts_dir.clone();
    let recorded = tokio::task::spawn_blocking(move || plan::recorded(&parts_dir))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

    match recorded {
        /* Only parts of this very file, fetched from a server that serves
         * ranges. A server that does not is asked for the whole file every
         * time, so nothing staged from it can be continued; and a file
         * whose validator moved is another file of the same length. */
        Some(recorded)
            if recorded.size == info.size
                && recorded.resumable
                && info.resumable
                && recorded.validator == info.validator =>
        {
            /* Ledgers (and an earlier dm's part files) past the plan's last
             * range belong to a split the plan does not remember — a power
             * cut can leave the plan from before it, which is safe only if
             * they go: a later split reusing the index would take its bytes
             * as done at another offset. */
            let ranges = recorded.segments().len();
            if let Ok(mut listing) = fs::read_dir(&options.parts_dir).await {
                while let Ok(Some(entry)) = listing.next_entry().await {
                    let name = entry.file_name();
                    let index = name.to_str().and_then(staged_index);
                    if index.is_some_and(|i| i >= ranges) {
                        let _ = fs::remove_file(entry.path()).await;
                    }
                }
            }
            return Ok(recorded.segments());
        }
        _ => {}
    }

    // Nothing here can be finished under any plan this run could write.
    // Only the parts go: the directory itself may be watched by a caller.
    let mut listing = fs::read_dir(&options.parts_dir).await?;
    while let Some(entry) = listing.next_entry().await? {
        let _ = fs::remove_file(entry.path()).await;
    }

    // One range a connection: as few requests as there are connections, so
    // a server that limits how fast requests may come (nginx's 429) is not
    // met with a burst; the halving below shares the work out as it goes.
    let segments = plan::plan(info.size, options.connections, info.resumable);
    let (dir, size, resumable, validator, written) = (
        options.parts_dir.clone(),
        info.size,
        info.resumable,
        info.validator.clone(),
        segments.clone(),
    );
    tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &written)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(segments)
}

/// Probes every mirror and keeps the ones describing the same file.
///
/// Mirrors that disagree about the length are not mirrors — they are a stale
/// copy and a current one, and splicing ranges from both produces a file that
/// is the right size and wrong throughout. That is the one failure this
/// engine must never produce, so disagreement costs the mirror rather than
/// the download.
async fn agreeing_mirrors(
    client: &reqwest::Client,
    listed: &Metalink,
) -> Result<(Vec<Source>, Probe), Error> {
    let probes = futures_util::future::join_all(
        listed
            .mirrors
            .iter()
            // A Metalink's mirrors are their own hosts; the page the
            // download came from is not the referer for any of them.
            .map(|url| probe(client, url, None, None)),
    )
    .await;

    let mut reference: Option<Probe> = None;
    let mut sources = Vec::new();
    let mut last_error = None;
    // A mirror that states no length, kept in case none states the right one.
    let mut lengthless: Option<Probe> = None;
    let mut disagreed = false;

    /* The document's length, where it states one, is the reference: it is
     * what the publisher signed off on. Taken from the best mirror instead,
     * a stale first mirror set the length and the current ones were dropped
     * as the ones that disagreed. */
    let expected = listed.size;

    // In the document's order, which is best first, so the reference is the
    // best mirror that answered rather than whichever answered first.
    for found in probes {
        let found = match found {
            Ok(found) => found,
            Err(e) => {
                last_error = Some(e);
                continue;
            }
        };
        let agrees = match (expected, &reference) {
            (Some(_), _) if found.size.is_none() => {
                if lengthless.is_none() {
                    lengthless = Some(found);
                }
                continue;
            }
            (Some(size), _) => found.size == Some(size),
            (None, None) => true,
            // An unstated length cannot be shown to agree, so it does not
            // get to join in.
            (None, Some(first)) => found.size.is_some() && found.size == first.size,
        };
        if !agrees {
            disagreed = true;
            continue;
        }
        sources.push(Source {
            url: found.final_url.clone(),
            validator: found.validator.clone(),
        });
        if reference.is_none() {
            reference = Some(found);
        }
    }

    /* No mirror stated the document's length, but one stated none: fetched
     * alone, held to the document's length, rather than refused. */
    if reference.is_none() {
        if let Some(only) = lengthless {
            sources.push(Source {
                url: only.final_url.clone(),
                validator: only.validator.clone(),
            });
            reference = Some(only);
        }
    }

    match reference {
        Some(mut agreed) => {
            if agreed.size.is_none() {
                agreed.size = listed.size;
            }
            Ok((sources, agreed))
        }
        None if disagreed => Err(Error::Internal(
            "no mirror in that Metalink has the file it describes: every one that answered \
             disagreed about its length"
                .into(),
        )),
        None => Err(last_error
            .unwrap_or_else(|| Error::Internal("not one mirror in that Metalink answered".into()))),
    }
}

/// Samples the byte count and reports a rate until the download ends.
///
/// A sampler rather than a report per chunk: chunks arrive thousands of times
/// a second and nothing can draw that fast, so the engine counts and this
/// decides how often anyone hears about it.
pub(crate) fn watch(
    downloaded: Arc<AtomicU64>,
    total: Option<u64>,
    tx: tokio::sync::mpsc::Sender<Update>,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(async move {
        let started = downloaded.load(Ordering::Relaxed);
        let mut meter = Meter::new(total, started);
        let mut last = Instant::now();

        // One immediately, so a caller has a row to draw before the first
        // interval has passed rather than an empty space.
        if tx
            .send(Update::Progress(meter.sample(started, Duration::ZERO)))
            .await
            .is_err()
        {
            return;
        }

        loop {
            /* Told when the download ends rather than merely cancelled.
             *
             * A download over a fast link finishes inside one interval, and a
             * watcher that is only aborted reports nothing at all — the bar
             * never moves and no final state ever arrives. Waiting on both
             * means the last sample is always sent. */
            let finished = tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(250)) => false,
                _ = &mut done_rx => true,
            };

            let now = Instant::now();
            let progress = meter.sample(downloaded.load(Ordering::Relaxed), now - last);
            last = now;

            // A receiver that has gone away ends the reporting, not the
            // download.
            if tx.send(Update::Progress(progress)).await.is_err() || finished {
                return;
            }
        }
    });

    (handle, done_tx)
}

/// Picks up where a previous run stopped.
async fn adopt_staged(job: &Job) -> Result<(), Error> {
    let mut segments = job.segments.lock().await;
    let staged = job.staged();
    let ranges: Vec<(u64, u64)> = segments.iter().map(|s| (s.start, s.span())).collect();
    let (dir, had_staged) = (job.parts.clone(), staged.exists());
    // Part files an earlier dm left go into the staged file first. A ledger
    // with no staged file under it vouches for bytes that are not there.
    let adopted = tokio::task::spawn_blocking(move || {
        let mut adopted = Vec::new();
        for (index, &(start, span)) in ranges.iter().enumerate() {
            if part_path(&dir, index).exists() {
                adopted.push((index, adopt_part(&dir, index, start, span)));
            } else if !had_staged {
                let _ = std::fs::remove_file(ledger_path(&dir, index));
            }
        }
        adopted
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?;
    for (index, result) in adopted {
        result.map_err(|e| {
            Error::Internal(format!(
                "the part of range {index} could not be adopted: {e}"
            ))
        })?;
    }
    for index in 0..segments.len() {
        let segment = &mut segments[index];
        /* A ledger cannot say more than its range under the plan that was
         * recorded beside it; one that does was written by something that
         * did not keep to the plan. Held to the range, so what is adopted
         * is exactly what assembly will use. */
        segment.done = job.ledger(index).min(segment.span());
        if segment.done > 0 {
            job.downloaded.fetch_add(segment.done, Ordering::Relaxed);
            // Claimed, so a split never takes bytes already on disk.
            if let Some(live) = job.live.get(index) {
                crate::lock(live).reserved = segment.done;
            }
        }
    }
    Ok(())
}

/// The staged file, made as long as the file will be, so every range's
/// region is there to be written; on most file systems the room is only
/// taken as bytes land. Never cut: a longer staged file holds nothing a
/// ledger vouches for past its ranges.
async fn preallocate(job: &Job) -> Result<(), Error> {
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(job.staged())
        .await?;
    if let Some(size) = job.size {
        if file.metadata().await?.len() < size {
            file.set_len(size).await?;
        }
    }
    Ok(())
}

/// Runs every range, and keeps running until none are left unfinished.
///
/// `workers` connections take the pieces in file order, each the next one
/// not yet taken as soon as it is done with its own: the fast connections
/// fetch most of the file, and a slow one holds a single piece. Once none is
/// left untaken, a connection that is free halves the largest range still
/// under way and takes its back half (dynamic segmentation),
/// so no connection sits idle behind a slow one.
async fn run(
    client: &reqwest::Client,
    job: &Arc<Job>,
    planned: usize,
    workers: usize,
    resumable: bool,
) -> Result<(), Error> {
    let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Ranges a connection gave up on, for the others to take.
    let orphans = Arc::new(std::sync::Mutex::new(Vec::<usize>::new()));
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(workers.max(1)));
    let mut running = tokio::task::JoinSet::new();
    for _ in 0..workers.max(1) {
        let (client, job, next) = (client.clone(), Arc::clone(job), Arc::clone(&next));
        let (orphans, active) = (Arc::clone(&orphans), Arc::clone(&active));
        running.spawn(async move {
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let orphan = if index < planned {
                    None
                } else {
                    crate::lock(&orphans).pop()
                };
                let index = if index < planned {
                    index
                } else if let Some(index) = orphan {
                    index
                } else {
                    match split_largest(&job).await {
                        Some(index) => index,
                        None => break,
                    }
                };
                crate::lock(&job.live[index]).taken = true;
                match fetch_segment(&client, &job, index, resumable).await {
                    Ok(()) => {}
                    /* The server would not take this connection after every
                     * retry (429, 503, a refused connect) while others are
                     * working: this one steps aside and leaves its range to
                     * them, as a server that limits connections asks, rather
                     * than failing a download the rest are fetching. */
                    Err(e) if crate::retry::transient(&e) && active.load(Ordering::SeqCst) > 1 => {
                        crate::lock(&job.live[index]).taken = false;
                        crate::lock(&orphans).push(index);
                        break;
                    }
                    Err(e) => return Err(e),
                }
            }
            active.fetch_sub(1, Ordering::SeqCst);
            Ok::<(), Error>(())
        });
    }

    /* The first failure is the download's. The other connections are then
     * stopped and waited out before it is returned: dropping their handles
     * leaves them running, still writing parts and the plan beside them —
     * under a download that has already failed, or under the fresh attempt
     * a changed file starts, which would find the old version's parts
     * adopted again. A pause or a cancel stops every connection by itself,
     * each flushing what it has, so those get a moment to; one stalled
     * where the stop does not reach it is stopped after that. */
    let mut failed = None;
    // Once a pause or a cancel is in, how long the rest get to flush.
    let mut grace: Option<tokio::time::Instant> = None;
    // As each connection ends, in whatever order: a failure is seen when it
    // happens, not after the connections spawned before it are done.
    loop {
        let ended = match grace {
            Some(until) => match tokio::time::timeout_at(until, running.join_next()).await {
                Ok(ended) => ended,
                // One stalled where no stop reaches it: stopped here.
                Err(_) => {
                    running.abort_all();
                    grace = None;
                    continue;
                }
            },
            None => running.join_next().await,
        };
        let Some(ended) = ended else { break };
        // A worker that panicked is a bug, not a network condition, and is
        // worth surfacing as itself rather than as a download failure. One
        // stopped here on purpose is not news.
        let outcome = match ended {
            Ok(outcome) => outcome,
            Err(e) if e.is_cancelled() => continue,
            Err(e) => Err(Error::Internal(e.to_string())),
        };
        if let Err(e) = outcome {
            if failed.is_none() {
                if matches!(e, Error::Paused | Error::Cancelled) {
                    grace = Some(tokio::time::Instant::now() + STOP_GRACE);
                } else {
                    running.abort_all();
                }
                failed = Some(e);
            }
        }
    }
    if let Some(e) = failed {
        return Err(e);
    }
    // A range given up on after the others had finished: fetched now, on
    // one connection, its failure this download's.
    let left: Vec<usize> = std::mem::take(&mut *crate::lock(&orphans));
    for index in left {
        crate::lock(&job.live[index]).taken = true;
        fetch_segment(client, job, index, resumable).await?;
    }
    Ok(())
}

/// Halves the range with the most left to fetch, under the connection
/// fetching it, and returns the new range made of its back half; `None` when
/// nothing is worth halving.
///
/// The plan beside the parts is written again before the new range fetches
/// a byte, so a resume puts every part at the offset it was fetched for.
async fn split_largest(job: &Job) -> Option<usize> {
    let mut segments = job.segments.lock().await;
    let index = segments.len();
    if index >= job.live.len() || !job.resumable {
        return None;
    }
    let left = |l: &Live| {
        (l.end - l.start)
            .saturating_add(1)
            .saturating_sub(l.reserved)
    };
    let victim = (0..index)
        .filter(|&i| {
            let l = crate::lock(&job.live[i]);
            l.taken && left(&l) >= 2 * MIN_SPLIT
        })
        .max_by_key(|&i| left(&crate::lock(&job.live[i])))?;
    let (start, end, old_end) = {
        let mut l = crate::lock(&job.live[victim]);
        let rest = left(&l);
        if rest < 2 * MIN_SPLIT {
            return None;
        }
        let old_end = l.end;
        l.end = l.start + l.reserved + rest / 2 - 1;
        (l.end + 1, old_end, old_end)
    };
    let mut planned = segments.clone();
    planned[victim].end = start - 1;
    planned.push(Segment {
        start,
        end,
        done: 0,
    });
    let (dir, size, resumable, validator, written) = (
        job.parts.clone(),
        job.size,
        job.resumable,
        job.validator.clone(),
        planned.clone(),
    );
    let recorded = tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &written)
    })
    .await;
    if !matches!(recorded, Ok(Ok(()))) {
        // Unrecorded, the split is undone: the range keeps its whole length,
        // which nothing past the new end can have been claimed against.
        crate::lock(&job.live[victim]).end = old_end;
        return None;
    }
    segments[victim].end = start - 1;
    segments.push(Segment {
        start,
        end,
        done: 0,
    });
    // A new range starts with nothing written down for it.
    let _ = fs::remove_file(ledger_path(&job.parts, index)).await;
    *crate::lock(&job.live[index]) = Live {
        start,
        end,
        reserved: 0,
        taken: true,
    };
    Some(index)
}

/// Fetches one range into its part file, retrying what is worth retrying.
async fn fetch_segment(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
) -> Result<(), Error> {
    // A part found whole on a resume is still checked at its end where the
    // server can be asked for it, since a power cut can tear a whole part
    // as easily as a half one.
    if !resumable && job.segments.lock().await[index].finished() {
        return Ok(());
    }

    /* Each attempt reads the range's ledger rather than anything
     * remembered, so a stream that died halfway resumes from where it
     * actually stopped. Trusting an in-memory figure would seek past bytes
     * never written and leave a hole in the middle of the range. */
    // Progress counts only where an attempt continues the last: from a
    // server without ranges every attempt starts again from nothing, and
    // one that got further than the last is not progress toward the end.
    let on_disk = || if resumable { job.ledger(index) } else { 0 };
    // Said while it waits, so the row tells a retry from a stall.
    let retrying = |attempt, of| {
        if let Some(tx) = &job.report {
            let _ = tx.try_send(Update::Retrying { attempt, of });
        }
    };
    let outcome =
        retry::run_making_progress_telling(&job.control, job.tries, on_disk, retrying, || {
            // Counted before the attempt, so the next one picks a different
            // mirror where there is one to pick.
            job.attempts[index].fetch_add(1, Ordering::Relaxed);
            attempt_segment(client, job, index, resumable)
        })
        .await;

    match outcome {
        // Asked to stop, so stop — including not treating it as a short range
        // to be waited out.
        Err(Error::Paused) => Err(Error::Paused),
        Err(Error::Cancelled) => Err(Error::Cancelled),
        /* A range that keeps arriving short is not this function's to judge.
         * Assembly compares the whole against the length the server stated,
         * so one place decides whether a file is complete rather than two
         * disagreeing about it. */
        Err(Error::Incomplete { .. }) => Ok(()),
        other => other,
    }
}

/// One attempt at one range.
async fn attempt_segment(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    resumable: bool,
) -> Result<(), Error> {
    // Read at each attempt: a split may have shortened the range since.
    let (start, end) = {
        let l = crate::lock(&job.live[index]);
        (l.start, l.end)
    };
    let span = (end - start).saturating_add(1);
    // The ledger, read fresh: what it says is on disk is what has arrived.
    let already = job.ledger(index).min(span);
    let mut ledger = job.open_ledger(index).await?;
    {
        let mut l = crate::lock(&job.live[index]);
        l.reserved = l.reserved.max(already);
    }
    // A whole part needs nothing more, unless its tail is to be checked
    // against the server below.
    if already >= span && !resumable {
        job.segments.lock().await[index].done = span;
        return Ok(());
    }

    /* A server that does not serve ranges is asked for the whole file, so
     * the whole file is what arrives — from byte 0. Seeking past what an
     * earlier attempt left wrote the body at that offset: the part only grew
     * and every retry after the first drop was corrupt. Start again. */
    let already = if !resumable && already > 0 {
        put(&mut ledger, 0).await?;
        job.downloaded.fetch_sub(already, Ordering::Relaxed);
        0
    } else {
        already
    };

    // A range after the first, starting fresh, asks for one byte more at
    // its front: the last byte of the range before, checked at assembly.
    let lead = resumable && index > 0 && already == 0 && start > 0;
    /* What a part already holds is asked for again at its end, and compared.
     * Its length is the resume ledger, and after a power cut the length can
     * be on disk while the last bytes are not — zeros, on NTFS, exFAT and
     * others — which a resume would have kept and built on, and called the
     * file finished. Where the two differ the part is cut back to the last
     * byte they agree on and fetched from there: repaired, not restarted. */
    let check = if resumable {
        already.min(RESUME_CHECK)
    } else {
        0
    };
    let from = if lead {
        start - 1
    } else {
        start + already - check
    };

    let source = job.source(index);
    let mut request = get(
        client,
        &source.url,
        job.referrer.as_deref(),
        job.sign_in.as_ref(),
    );
    if resumable {
        request = request.header(reqwest::header::RANGE, format!("bytes={from}-{end}"));
        // Only ever alongside a range: it is a condition on the range, and a
        // republished file answers 200 instead, which is how a changed file
        // is noticed rather than spliced into the old one. The validator is
        // this mirror's own — another server's would simply not match.
        // Never a weak ETag (`W/"…"`): `If-Range` takes only a strong one
        // (RFC 9110), and a strict server answers a weak one with the whole
        // file, every range, so the download fell back to one connection
        // and a resume failed as though ranges had stopped. Without the
        // condition, a changed file is still caught: by the validator the
        // answer carries, and by the bytes the resume compares.
        if let Some(validator) = source.validator.as_ref().filter(|v| !v.starts_with("W/")) {
            request = request.header(reqwest::header::IF_RANGE, validator);
        }
    }

    let res = Error::refused_or(
        or_stop(
            &job.control,
            crate::auth::send(client, request, &source.url),
        )
        .await?,
    )
    .await?;
    let status = res.status();
    let stated = res.content_length();
    if resumable && status != reqwest::StatusCode::PARTIAL_CONTENT {
        // The server answered the whole file to a request for part of it.
        // Under the validator the ranges were planned under, it is the same
        // file served whole; under another, everything staged belongs to a
        // version that is gone.
        let header = |name| {
            res.headers()
                .get(name)
                .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
        };
        let answered =
            header(reqwest::header::ETAG).or_else(|| header(reqwest::header::LAST_MODIFIED));
        let same = source.validator.is_some() && answered == source.validator.as_deref();
        // Asked for every byte, it is the range: Wikimedia answers a range
        // of the whole file that way, and that is the file.
        let all_of_it = from == 0 && stated == Some(end + 1);
        if !(same && all_of_it) {
            if !same {
                forget_plan(job).await;
            }
            return Err(if same {
                Error::RangesIgnored
            } else {
                Error::Changed
            });
        }
    }
    /* A 206 for another range than the one asked — a server rounding to its
     * own blocks — said so in its Content-Range, and its bytes were written
     * at the asked offset regardless. Refused, from the zoo. */
    if resumable {
        let said_start = res
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().strip_prefix("bytes "))
            .and_then(|v| v.split(['-', '/']).next())
            .and_then(|v| v.trim().parse::<u64>().ok());
        if said_start.is_some_and(|s| s != from) {
            return Err(Error::DishonestRanges);
        }
    }

    /* Created if absent, never truncated.
     *
     * clippy suggests `.truncate(true)` here and it would be exactly wrong:
     * the staged file holds every range, so emptying it on any attempt
     * would throw away the bytes of all of them. Stated explicitly so the
     * next person to read the lint does not take its advice. */
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(job.staged())
        .await?;

    /* Buffered, because chunks arrive far smaller than a useful write.
     *
     * Safe against the part-length-as-ledger design precisely because `done`
     * is never persisted: a retry re-derives it from whatever actually
     * reached the disk, so a buffer lost to a crash costs the bytes it held
     * and nothing else. */
    let mut stream = res.bytes_stream();
    // The part of a chunk past the bytes compared, still to be written.
    let mut carry = None;
    let already = if check > 0 {
        let mut tail = vec![0u8; check as usize];
        let mut on_disk = tokio::fs::File::open(job.staged()).await?;
        on_disk
            .seek(std::io::SeekFrom::Start(start + already - check))
            .await?;
        tokio::io::AsyncReadExt::read_exact(&mut on_disk, &mut tail).await?;
        drop(on_disk);
        let (agree, rest) = compare_tail(&mut stream, &tail, &job.control).await?;
        /* What a power cut leaves is blocks never written: zeros, a whole
         * disk block at a time, and not always the last ones — a disk puts
         * blocks down in its own order. So the block the first difference
         * falls in decides. All zeros, it never reached the disk, and the
         * part is cut back to there and fetched again. Written and still
         * different, it is another version of the file — republished by a
         * server that names no version, so nothing else noticed — and
         * cutting back would splice the two into one file and call it
         * finished. Everything staged belongs to the old one. */
        let differs = (agree as usize) < tail.len() && rest.is_some();
        if differs {
            let base = already - check;
            let at = base + agree;
            // From the first difference to the end of its block: the bytes
            // before it agree, so they say nothing either way.
            let block_end = (at - at % DISK_BLOCK + DISK_BLOCK).min(already);
            let written = tail
                .get(agree as usize..(block_end - base) as usize)
                .unwrap_or(&[]);
            if written.iter().any(|&b| b != 0) {
                drop(file);
                forget_plan(job).await;
                return Err(Error::Changed);
            }
            /* Zeros from the front of the window to the difference: what
             * was lost may reach further back than the window does. A power
             * cut that lost a quarter of a megabyte had its zeros before the
             * window trusted, and the file came out the right length, wrong,
             * and Completed. The part is cut back to where the zeros begin,
             * to a block, and the next attempt checks the window there.
             *
             * Only where the disk's zeros run unbroken from the window's
             * front through the byte that differed — a zero observed where
             * the server has something else, not merely an empty stretch
             * before it. And no further than `ZERO_RUN_STEP` back: a file
             * can be zeros for gigabytes (a disk image, a sparse archive),
             * and following them all to their start fetched every one of
             * them again for a torn last block. A loss longer than a step
             * is still found, a step at a time: the window at the cut opens
             * on zeros the server does not have, and is cut again. */
            let zeros_through_difference = tail
                .get(..=agree as usize)
                .is_some_and(|run| run.iter().all(|&b| b == 0));
            if base > 0 && zeros_through_difference {
                let floor = base.saturating_sub(ZERO_RUN_STEP);
                // In the staged file, at this range's offset.
                let zeros_from =
                    zeros_ending_at(&job.staged(), start + base, start + floor).await? - start;
                let cut = zeros_from - zeros_from % DISK_BLOCK;
                if cut < base {
                    put(&mut ledger, cut).await?;
                    drop(file);
                    // Its connection goes before the next is opened: a site
                    // held to one connection would refuse the repair's.
                    drop(stream);
                    job.downloaded.fetch_sub(already - cut, Ordering::Relaxed);
                    crate::lock(&job.live[index]).reserved = cut;
                    /* Straight on to the window at the cut, rather than
                     * as a failed attempt: a repair stepping back through a
                     * long loss is getting somewhere, and spending the
                     * retry budget on it failed the range part-way. Each
                     * step leaves the part shorter, so it ends. */
                    return Box::pin(attempt_segment(client, job, index, resumable)).await;
                }
            }
        }
        carry = rest;
        let keep = already - check + agree;
        if keep < already {
            put(&mut ledger, keep).await?;
            job.downloaded.fetch_sub(already - keep, Ordering::Relaxed);
            crate::lock(&job.live[index]).reserved = keep;
        }
        keep
    } else {
        already
    };

    /* Small: tokio's file keeps a copy of every write it hands to the
     * disk, so each connection holds this twice. 256 KiB cost 20–30 MB more
     * over ten downloads of eight connections than 16 KiB, and fetched 2 GB
     * over loopback no faster nor with less CPU (qa/after.md). */
    let mut file = tokio::io::BufWriter::with_capacity(16 * 1024, file);
    // At this range's own place in the staged file.
    file.seek(std::io::SeekFrom::Start(start + already)).await?;

    let mut written = already;
    // What the ledger says and when it said it, so it is written down
    // again every few megabytes or every second rather than every chunk.
    let mut recorded = already;
    let mut recorded_at = std::time::Instant::now();
    // Where this range ends now: sooner than asked, once a split has taken
    // its back half.
    let mut span_now = span;
    let mut want_lead = lead;

    loop {
        let chunk = match carry.take() {
            Some(rest) => Ok(rest),
            None => match next_or_stop(&mut stream, &job.control).await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                // Stopped while waiting: what is in the write buffer is on
                // disk before the range lets go, or a resume fetches it again.
                Err(stopped) => {
                    file.flush().await?;
                    put(&mut ledger, written).await?;
                    if matches!(stopped, Error::Paused) {
                        job.segments.lock().await[index].done = written.min(span_now);
                    }
                    return Err(stopped);
                }
            },
        };
        // A stream that dies mid-range leaves what it wrote; flushing before
        // giving up is what lets the retry pick up from there instead of
        // fetching those bytes again.
        let mut chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                file.flush().await?;
                put(&mut ledger, written).await?;
                return Err(Error::Network(e));
            }
        };
        // The overlap byte is kept aside, not written into this range.
        if want_lead && !chunk.is_empty() {
            if let (Some(first), Some(slot)) = (chunk.first(), job.overlaps.get(index)) {
                slot.store(u16::from(*first), Ordering::Relaxed);
            }
            chunk = chunk.slice(1..);
            want_lead = false;
        }

        /* Checked between chunks rather than mid-write: a partial chunk on
         * disk would still be consistent — the part length is the ledger —
         * but stopping on a boundary keeps the two obviously in step. */
        if let Err(stopped) = job.control.check() {
            // Flushed first either way: the bytes already written are what a
            // resume picks up from, and a pause also records where it got to.
            file.flush().await?;
            put(&mut ledger, written).await?;
            if matches!(stopped, Error::Paused) {
                job.segments.lock().await[index].done = written.min(span_now);
            }
            return Err(stopped);
        }

        if written + chunk.len() as u64 > span {
            /* More bytes than the range asked for.
             *
             * A server holding a compressed representation ranges over the
             * *encoded* bytes and hands back the decoded ones, and nothing in
             * the response says the two do not correspond. Trimming to length
             * here is what makes that silent: every part ends up exactly as
             * long as planned, assembled in order, and wrong all the way
             * through. A file of the right size and the wrong contents is the
             * one thing a download manager must never produce. */
            file.flush().await?;
            put(&mut ledger, written).await?;
            return Err(Error::DishonestRanges);
        }

        // Claimed before it is written, against where the range ends now:
        // a split under this connection stops it at the new end.
        let fits = {
            let mut l = crate::lock(&job.live[index]);
            span_now = (l.end - l.start).saturating_add(1);
            let fits = (chunk.len() as u64).min(span_now.saturating_sub(written));
            l.reserved = written + fits;
            fits
        };
        let cut = fits < chunk.len() as u64;
        if cut {
            chunk = chunk.slice(..fits as usize);
        }
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        // Written down every few megabytes or every second: a crash then
        // costs no more than that of this range, fetched again.
        if written > recorded
            && (written - recorded >= LEDGER_EVERY || recorded_at.elapsed() >= LEDGER_EVERY_SECOND)
        {
            file.flush().await?;
            put(&mut ledger, written).await?;
            recorded = written;
            recorded_at = std::time::Instant::now();
        }
        if cut {
            job.downloaded
                .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            // The rest belongs to the range split off; this one is whole.
            break;
        }

        /* Applied after the bytes have arrived, because there is nowhere
         * earlier to stand: the only lever on an HTTP body is how fast it is
         * read, and a reader that pauses is a sender that stops.
         *
         * Counted as the ceiling lets it through rather than as it arrived:
         * counted on arrival, every connection's chunk was in the count a
         * whole wait early, and the shown speed and ETA ran at twice the
         * limit for the first seconds (gap 112). */
        let mut through = 0;
        let stopping = or_stop(&job.control, async {
            job.limiter
                .take_counting(chunk.len() as u64, &job.downloaded, &mut through)
                .await;
            Ok(())
        })
        .await;
        if let Err(stopped) = stopping {
            // It is on disk: the count is a ledger of what is, and a retry
            // of this range starts from the part's length.
            job.downloaded
                .fetch_add(chunk.len() as u64 - through, Ordering::Relaxed);
            file.flush().await?;
            put(&mut ledger, written).await?;
            if matches!(stopped, Error::Paused) {
                job.segments.lock().await[index].done = written.min(span_now);
            }
            return Err(stopped);
        }
    }

    file.flush().await?;
    put(&mut ledger, written).await?;

    /* The shared plan is touched once, at the end, rather than per chunk.
     *
     * One mutex guards every range, so a lock per chunk is every connection
     * queueing behind every other a hundred thousand times over a large
     * download — to publish a number nothing reads while it runs. */
    {
        // Read under the segment lock, which a split holds from shortening
        // this range to recording the plan: a split whose plan could not be
        // written gives the range its end back before letting go, and the
        // end read here is the one that stands.
        let mut segments = job.segments.lock().await;
        let l = crate::lock(&job.live[index]);
        span_now = (l.end - l.start).saturating_add(1);
        drop(l);
        if let Some(segment) = segments.get_mut(index) {
            segment.done = written.min(span_now);
        }
    }

    /* The whole file, asked for without a range, ended cleanly: that is its
     * end, whatever the plan guessed. An unknown length is planned to
     * `u64::MAX` and an empty file as one byte, and both sat through every
     * retry as "short" before this. A length the response stated is still
     * held to, and assembly holds the total to the probed one. */
    if !resumable && stated.is_none_or(|stated| stated == written) {
        return Ok(());
    }

    // A stream that ended early without erroring is not a finished range.
    if written < span_now {
        return Err(Error::Incomplete {
            got: written,
            expected: span_now,
        });
    }
    Ok(())
}

/// How often a range's ledger is written down while it downloads: every
/// this many bytes, or every second, whichever comes first.
const LEDGER_EVERY: u64 = 4 * 1024 * 1024;
const LEDGER_EVERY_SECOND: Duration = Duration::from_secs(1);

/// How much of a part's end is fetched again and compared on a resume: more
/// than a filesystem leaves unwritten at a power cut, little enough to cost
/// nothing beside the rest.
const RESUME_CHECK: u64 = 64 * 1024;

/// How long the other connections get to put what they hold on disk after a
/// pause or a cancel, before the ones still going are stopped.
const STOP_GRACE: Duration = Duration::from_secs(1);

/// The unit a disk writes in, and so the unit a power cut loses.
const DISK_BLOCK: u64 = 4096;

/// Removes the plan and the part files staged under `dir`.
async fn discard_staged(dir: &Path) {
    let _ = fs::remove_file(crate::plan::record_path(dir)).await;
    if let Ok(mut listing) = fs::read_dir(dir).await {
        while let Ok(Some(entry)) = listing.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("part-") || name.starts_with("done-") || name == STAGED {
                let _ = fs::remove_file(entry.path()).await;
            }
        }
    }
}

/// After the file is found changed: the plan its parts were fetched under
/// goes, so the next attempt adopts none of them and starts afresh, rather
/// than resuming into a file that no longer exists on the server.
async fn forget_plan(job: &Job) {
    let _ = fs::remove_file(crate::plan::record_path(&job.parts)).await;
}

/// How far back past the checked window one repair follows a run of zeros
/// in a part. Far more than a disk block or the write buffer a power cut
/// loses at once, and still little to fetch again where the zeros were the
/// file's own; a longer loss is followed a step of this at a time.
const ZERO_RUN_STEP: u64 = 4 * 1024 * 1024;

/// Where the run of zeros that ends at `end` in this file begins, looking
/// no further back than `floor`: `end` itself when the byte before it is not
/// zero, `floor` when the run reaches it. Read backwards a block of reads at
/// a time, so a long run costs reads, not memory.
async fn zeros_ending_at(path: &Path, end: u64, floor: u64) -> Result<u64, Error> {
    const STEP: u64 = 64 * 1024;
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; STEP as usize];
    let mut at = end;
    while at > floor {
        let from = at.saturating_sub(STEP).max(floor);
        let len = (at - from) as usize;
        let chunk = buf.get_mut(..len).unwrap_or(&mut []);
        file.seek(std::io::SeekFrom::Start(from)).await?;
        tokio::io::AsyncReadExt::read_exact(&mut file, chunk).await?;
        if let Some(last) = chunk.iter().rposition(|&b| b != 0) {
            return Ok(from + last as u64 + 1);
        }
        at = from;
    }
    Ok(floor)
}

/// Reads `tail.len()` bytes off the stream against `tail`: how many agree
/// from the front, and whatever the stream gave past them (from the first
/// byte that differed, or past the end of `tail`), still to be written.
async fn compare_tail<S>(
    stream: &mut S,
    tail: &[u8],
    control: &Control,
) -> Result<(u64, Option<bytes::Bytes>), Error>
where
    S: futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let mut seen = 0usize;
    while seen < tail.len() {
        let Some(chunk) = next_or_stop(stream, control).await? else {
            // Ended early: what agreed so far is kept, and the rest of the
            // range is short, which the caller reports as such.
            break;
        };
        let chunk = chunk.map_err(Error::Network)?;
        let theirs = chunk
            .get(..chunk.len().min(tail.len() - seen))
            .unwrap_or(&[]);
        let ours = tail.get(seen..seen + theirs.len()).unwrap_or(&[]);
        if let Some(at) = theirs.iter().zip(ours).position(|(a, b)| a != b) {
            return Ok(((seen + at) as u64, Some(chunk.slice(at..))));
        }
        seen += theirs.len();
        if theirs.len() < chunk.len() {
            return Ok((seen as u64, Some(chunk.slice(theirs.len()..))));
        }
    }
    Ok((seen as u64, None))
}

/* ------------------------------------------------------------- streams */

/// Fetches an adaptive stream as one file.
///
/// Segments rather than ranges, so almost none of the machinery above
/// applies: there is no length to plan against, no offset to resume from
/// within a segment, and nothing to ask a server for a second copy of. What
/// carries over is the retry policy, the speed ceiling, and being able to
/// stop.
async fn download_stream(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let (mut listed, rendition) = match resolve_stream(client, url, options).await? {
        Resolved::Own(listed, rendition) => (listed, rendition),
        // As a DASH stream is: yt-dlp reads these shapes properly; without
        // it the stream is refused rather than saved wrong and called done.
        Resolved::Beyond(_) if crate::tools::for_video(client).await.is_ok() => {
            return crate::ytdlp::download_ytdlp(url, options, report).await;
        }
        Resolved::Beyond(why) => {
            return Err(Error::Internal(format!(
                "{why}, which dm does not assemble itself — install yt-dlp to download it"
            )));
        }
    };

    fs::create_dir_all(&options.parts_dir).await?;
    keep_only_parts_of(&options.parts_dir, &rendition).await?;
    if let Some(found) = first_segment_is(client, &listed, options).await {
        listed.found_to_be(found, url, options.quality);
    }

    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: listed.filename.clone(),
                category: category::classify(&listed.filename, None),
            })
            .await;
    }

    let key = match &listed.key {
        // Once, not once per segment: a playlist of a thousand segments
        // would otherwise be a thousand requests for the same sixteen bytes.
        Some(key) => Some(fetch_capped(client, &key.uri, 1024, options).await?.body),
        None => None,
    };

    fs::create_dir_all(&options.dest_dir).await?;

    /* Stopped after every segment had arrived — paused during the remux, or
     * dm quit or killed then — the segments are in the gathered stream, and
     * once remuxed, in the MP4: neither is fetched again. The gathered
     * stream is named only once whole (it is joined under `.partial`), and
     * removed only once the MP4 is, so whichever is here is complete. */
    let gathered = options.parts_dir.join(GATHERED);
    let remuxed = options.parts_dir.join(REMUXED);
    // Only where the remux would run now: turned off since, or with a
    // digest that names the `.ts` bytes, an MP4 from before is not the file
    // asked for, and the stream is fetched again from its segments.
    let remuxing = remux_wanted(&listed.filename, options);
    if !remuxing {
        let _ = fs::remove_file(&remuxed).await;
    }
    if remuxing
        && fs::metadata(&gathered).await.is_err()
        && fs::metadata(&remuxed).await.is_ok_and(|m| m.len() > 0)
    {
        let stem = ts_stem(&listed.filename);
        let (path, bytes, category) =
            finish(&format!("{stem}.mp4"), None, &[remuxed], options).await?;
        return Ok(Finished {
            left_behind: None,
            path,
            bytes,
            connections: 1,
            category,
            verified: options.checksum.is_some(),
            mirrors: 1,
        });
    }

    let parts: Vec<PathBuf> = if fs::metadata(&gathered).await.is_ok() {
        vec![gathered]
    } else {
        // No total: a playlist states none, and a bar that guessed one would
        // be a bar that lies.
        let downloaded = Arc::new(AtomicU64::new(0));
        let meter = report.map(|tx| watch(Arc::clone(&downloaded), None, tx));
        let outcome = fetch_segments(client, &listed, key.as_deref(), options, &downloaded).await;
        if let Some((handle, finished)) = meter {
            let _ = finished.send(());
            let _ = handle.await;
        }
        outcome?;
        (0..listed.parts.len())
            .map(|i| part_path(&options.parts_dir, i))
            .collect()
    };
    // A transport stream into an MP4 where asked and able; the `.ts` where
    // not, which is still the stream, whole.
    let (parts, filename) = match remux_ts(&listed.filename, &parts, options).await {
        Some((mp4, name)) => (vec![mp4], name),
        None => (parts, listed.filename.clone()),
    };
    let (path, bytes, category) = finish(&filename, None, &parts, options).await?;

    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// What the stream's first segment is, from its first few kilobytes; see
/// `stream::container_of_head`. `None` where it cannot be read or says
/// nothing, and the playlist's words stand: an initialisation segment
/// already says fMP4, and an encrypted segment's first bytes are
/// ciphertext. Read from what is staged where a resume has it, so a
/// resume asks the server for nothing it already has.
async fn first_segment_is(
    client: &reqwest::Client,
    listed: &stream::Stream,
    options: &Options,
) -> Option<&'static str> {
    const HEAD: usize = 4096;
    if listed.key.is_some() || listed.parts.iter().any(|p| p.init) {
        return None;
    }
    for staged in [
        part_path(&options.parts_dir, 0),
        options.parts_dir.join(GATHERED),
    ] {
        if let Ok(mut file) = fs::File::open(&staged).await {
            let mut head = Vec::with_capacity(HEAD);
            let read = tokio::io::AsyncReadExt::read_buf(
                &mut tokio::io::AsyncReadExt::take(&mut file, HEAD as u64),
                &mut head,
            )
            .await;
            if read.is_ok_and(|n| n > 0) {
                return stream::container_of_head(&head);
            }
        }
    }
    let first = listed.parts.first()?;
    let request = get(
        client,
        &first.url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    )
    .header(reqwest::header::RANGE, format!("bytes=0-{}", HEAD - 1));
    let res = or_stop(
        &options.control,
        crate::auth::send(client, request, &first.url),
    )
    .await
    .ok()?;
    if !res.status().is_success() {
        return None;
    }
    // Read no further than the head, whether or not the range was honoured.
    let mut chunks = res.bytes_stream();
    let mut head = Vec::new();
    while head.len() < HEAD {
        match next_or_stop(&mut chunks, &options.control).await {
            Ok(Some(Ok(chunk))) => head.extend_from_slice(&chunk),
            _ => break,
        }
    }
    stream::container_of_head(&head)
}

/// A request's answer, or the stop asked for meanwhile. A server that takes
/// the connection and never answers holds a request until the read timeout,
/// which is a setting and can be ten minutes; a pause does not wait for it.
pub(crate) async fn or_stop<T>(
    control: &Control,
    work: impl std::future::Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::pin!(work);
    loop {
        control.check()?;
        tokio::select! {
            done = &mut work => return done,
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
}

/// The next item of a body, or the stop asked for meanwhile.
///
/// A body that has gone quiet is waited on in short spells, with the control
/// looked at between them: checked only between chunks, a pause waited for
/// the read timeout, which is a setting now and can be ten minutes. Waiting
/// out a spell drops nothing — the stream yields a chunk only once it has
/// one, and the next spell picks it up.
pub(crate) async fn next_or_stop<S>(
    stream: &mut S,
    control: &Control,
) -> Result<Option<S::Item>, Error>
where
    S: futures_util::Stream + Unpin,
{
    loop {
        control.check()?;
        if let Ok(item) = tokio::time::timeout(Duration::from_millis(250), stream.next()).await {
            return Ok(item);
        }
    }
}

/// The most of one stream segment held in memory.
const SEGMENT_CAP: usize = 512 * 1024 * 1024;

/// A `.ts` stream's segments, gathered into one, under its parts folder.
const GATHERED: &str = "joined.ts";
/// That stream put into an MP4.
const REMUXED: &str = "joined.mp4";

/// Puts the segments of a `.ts` stream into one MP4 with ffmpeg, copying
/// the picture and sound as they are. `None` where that is not asked for,
/// not possible (no ffmpeg, a digest that names the `.ts` bytes, another
/// container) or did not work, in which case the parts are left for the
/// ordinary join: a stream a person can play as `.ts` beats no stream.
async fn remux_ts(name: &str, parts: &[PathBuf], options: &Options) -> Option<(PathBuf, String)> {
    if !remux_wanted(name, options) {
        return None;
    }
    let ffmpeg = crate::ytdlp::ffmpeg()?;
    options.control.check().ok()?;
    let joined = options.parts_dir.join(GATHERED);
    let mp4 = options.parts_dir.join(REMUXED);
    /* The segments are gathered into one stream with each freed as it is
     * consumed, as the ordinary join does: segments, a joined copy and the
     * MP4 side by side needed three times the file at the end. Now the peak
     * is the stream and its MP4. Joined under `.partial` and named only once
     * whole, so a resume that finds the name finds every segment in it; a
     * join cut short leaves the segments it had not reached, and the ones it
     * had are fetched again. Already gathered (a resume), it is used as is. */
    if parts != [joined.clone()] {
        let partial = options.parts_dir.join(format!("{GATHERED}.partial"));
        let gathered = join_parts(parts, &partial).await;
        if gathered.is_err() {
            let _ = fs::remove_file(&partial).await;
            return None;
        }
        if fs::rename(&partial, &joined).await.is_err() {
            return None;
        }
    }
    let ts_name = name.to_string();
    let keep_ts = || async {
        // The stream, whole, as the one part to place: what not remuxing
        // would have saved.
        Some((joined.clone(), ts_name.clone()))
    };
    if options.control.check().is_err() {
        return keep_ts().await;
    }
    let child = crate::no_window_async(&mut tokio::process::Command::new(&ffmpeg))
        .args(["-y", "-nostdin", "-loglevel", "error", "-i"])
        .arg(&joined)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(&mp4)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn();
    // Raced against the control: a pause or a cancel during a long remux
    // ends ffmpeg now (dropping the child kills it), and the gathered
    // stream stays for the resume to remux again.
    let ran = match child {
        Ok(mut child) => {
            let waited = or_stop(&options.control, async {
                child.wait().await.map_err(Error::Disk)
            })
            .await;
            match waited {
                Ok(status) => Ok(status),
                Err(_) => {
                    drop(child);
                    let _ = fs::remove_file(&mp4).await;
                    return keep_ts().await;
                }
            }
        }
        Err(e) => Err(e),
    };
    let made =
        ran.is_ok_and(|s| s.success()) && fs::metadata(&mp4).await.is_ok_and(|m| m.len() > 0);
    if !made {
        let _ = fs::remove_file(&mp4).await;
        return keep_ts().await;
    }
    let _ = fs::remove_file(&joined).await;
    Some((mp4, format!("{}.mp4", ts_stem(name))))
}

/// Whether a stream of this name is put into an MP4: asked for, a `.ts`,
/// and no digest of the `.ts` bytes to check. ffmpeg is looked for later.
fn remux_wanted(name: &str, options: &Options) -> bool {
    options.remux && options.checksum.is_none() && name.to_ascii_lowercase().ends_with(".ts")
}

/// A `.ts` name without its extension, in any case.
fn ts_stem(name: &str) -> &str {
    if name.to_ascii_lowercase().ends_with(".ts") {
        name.get(..name.len() - 3).unwrap_or(name)
    } else {
        name
    }
}

/// Reads the manifest, following a master playlist to the rendition wanted.
///
/// Returns the media playlist's URL alongside, which is what names the
/// rendition the segments belong to.
/// A stream as resolved: dm's to assemble, or a shape it hands on.
enum Resolved {
    Own(stream::Stream, String),
    /// Why dm does not assemble it itself (`stream::beyond_dm`).
    Beyond(&'static str),
}

async fn resolve_stream(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Resolved, Error> {
    let quality = options.quality;
    let (text, landed) = fetch_playlist(client, url, options).await?;
    if !text.contains("#EXTM3U") {
        return Err(Error::Internal("that is not an HLS playlist".into()));
    }

    // A master playlist lists renditions; a media playlist lists segments.
    // Either way the name comes from the URL the person reached for.
    if text.to_ascii_uppercase().contains("#EXT-X-STREAM-INF") {
        let chosen = stream::pick(stream::variants(&text, &landed), quality)
            .ok_or_else(|| Error::Internal("no playable rendition in that stream".into()))?;
        let (media, media_landed) = fetch_playlist(client, &chosen.url, options).await?;
        if let Some(why) =
            stream::beyond_dm(Some(&text), chosen.audio.as_deref(), &media, &media_landed)
        {
            return Ok(Resolved::Beyond(why));
        }
        let listed = stream::media_playlist(&media, &media_landed, url, quality)?;
        return Ok(Resolved::Own(listed, chosen.url));
    }
    if let Some(why) = stream::beyond_dm(None, None, &text, &landed) {
        return Ok(Resolved::Beyond(why));
    }
    let listed = stream::media_playlist(&text, &landed, url, quality)?;
    Ok(Resolved::Own(listed, url.to_string()))
}

/// A playlist's text and the address it was finally served from, which
/// its relative entries are joined against.
async fn fetch_playlist(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<(String, String), Error> {
    let fetched = fetch_capped(client, url, MANIFEST_CAP, options).await?;
    Ok((fetched.text(), fetched.landed))
}

/// Discards staged segments that belong to a different rendition.
///
/// Segments are matched to the playlist by index, and an index says nothing
/// about which rendition it came from: with the quality setting changed
/// between a pause and a resume, the first segments were 1080p and the rest
/// 480p, joined as one file and reported Completed. The rendition is written
/// beside the parts, and parts of another are not adopted.
async fn keep_only_parts_of(parts_dir: &Path, rendition: &str) -> Result<(), Error> {
    let marker = parts_dir.join("rendition");
    let staged = fs::read_to_string(&marker).await.ok();
    if staged.as_deref() != Some(rendition) {
        let mut listing = fs::read_dir(parts_dir).await?;
        while let Some(entry) = listing.next_entry().await? {
            let _ = fs::remove_file(entry.path()).await;
        }
        fs::write(&marker, rendition).await?;
    }
    Ok(())
}

/// Fetches every segment, in order, into its own part file.
///
/// Sequential on purpose. Segments arrive already split, so there is nothing
/// to gain from asking one server for eight at once beyond what its own
/// per-connection limit allows, and a stream fetched out of order is a file
/// that has to be held in memory to be put back in order.
async fn fetch_segments(
    client: &reqwest::Client,
    listed: &stream::Stream,
    key: Option<&[u8]>,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<(), Error> {
    for (index, part) in listed.parts.iter().enumerate() {
        let done = part_path(&options.parts_dir, index);

        /* Written under a temporary name and moved when whole.
         *
         * A segment has no declared length, so a part file that is present
         * says nothing about whether it finished — half of one looks exactly
         * like all of one. Only a completed segment is ever given its real
         * name, which is what makes resuming a stream possible at all. */
        if let Ok(meta) = fs::metadata(&done).await {
            downloaded.fetch_add(meta.len(), Ordering::Relaxed);
            continue;
        }

        options.control.check()?;

        let staging = done.with_extension("partial");
        let fetched = retry::run_up_to(&options.control, options.tries(), || {
            fetch_segment_body(client, &part.url, options)
        })
        .await?;

        let bytes = match (key, &listed.key) {
            (Some(key), Some(declared)) if !part.init => {
                // The initialisation segment is never encrypted.
                stream::decrypt(key, &stream::iv_for(declared, part.sequence), &fetched)?
            }
            _ => fetched,
        };

        downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        fs::write(&staging, &bytes).await?;
        fs::rename(&staging, &done).await?;
    }
    Ok(())
}

/// One segment, whole, because it has to be decrypted before it can be kept.
async fn fetch_segment_body(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
) -> Result<Vec<u8>, Error> {
    /// The largest a segment is sized for up front. A segment grown by
    /// doubling from empty is copied about fourteen times on the way, so the
    /// hint is worth taking — but only as far as a real segment goes.
    /// Content-Length is the server's claim, and a claim of a terabyte would
    /// otherwise be an allocation of a terabyte.
    const HINT_CAP: u64 = 32 * 1024 * 1024;

    let res = or_stop(
        &options.control,
        crate::auth::send(
            client,
            get(
                client,
                url,
                options.referrer.as_deref(),
                options.sign_in.as_ref(),
            ),
            url,
        ),
    )
    .await?;
    let res = Error::refused_or(res).await?;
    let hint = res.content_length().unwrap_or(0).min(HINT_CAP);
    let mut body = Vec::with_capacity(hint as usize);
    let mut chunks = res.bytes_stream();
    while let Some(chunk) = next_or_stop(&mut chunks, &options.control).await? {
        let chunk = chunk?;
        // A segment is seconds of video. One that keeps coming is not one,
        // and is not held in memory until the process runs out.
        if body.len() + chunk.len() > SEGMENT_CAP {
            return Err(Error::Internal(format!(
                "{url} is far larger than a stream's segment should be"
            )));
        }
        body.extend_from_slice(&chunk);
        or_stop(&options.control, async {
            options.limiter.take(chunk.len() as u64).await;
            Ok(())
        })
        .await?;
    }
    Ok(body)
}

/// Concatenates part files into one, freeing each as it is consumed.
///
/// Copied through rather than read whole: `fs::read` allocates an entire part
/// before a byte of it is written, so peak memory tracked file size — a four
/// gigabyte download over eight connections held half a gigabyte at a time
/// for no reason.
///
/// Each part is removed as soon as it has been copied, which is the whole
/// difference between needing the file's size on disk and needing twice it.
/// Deleting them all at the end leaves every part in place while the finished
/// file grows alongside them; deleting each as it is consumed keeps the two
/// adding up to about one copy, since what the destination gains the parts
/// have just given up.
///
/// The cost is that a failure part-way through loses the parts already
/// copied, so the download starts again rather than resuming. That failure is
/// nearly always running out of room — which is the thing this stops
/// happening.
async fn join_parts(parts: &[PathBuf], into: &Path) -> Result<u64, Error> {
    /* One part is not concatenated — it is moved.
     *
     * A single-connection download (a server that will not range, a file
     * under one segment, a stream joined into one part) has nothing to join:
     * the one part IS the file. Copying it doubled both the time and the peak
     * disk — the part and the growing copy both existed in full for the
     * length of the copy — for a rename that costs neither, when the two are
     * on one filesystem. Across filesystems a rename cannot move bytes, so it
     * falls back to the copy below. */
    if let [only] = parts {
        if fs::rename(only, into).await.is_ok() {
            return Ok(fs::metadata(into).await?.len());
        }
    }

    /* The file is dm's from here — `claim_path` took a name nothing had —
     * so a copy that fails part-way (the disk filled, nearly always) removes
     * what it had written rather than leaving a truncated file that looks
     * finished and, worse, holds the name the retry would otherwise take. */
    let copied = copy_parts(parts, into).await;
    if copied.is_err() {
        let _ = fs::remove_file(into).await;
    }
    copied
}

/* The first part becomes the file by a rename, and the rest are appended to
 * it on one blocking thread. The join used to copy every byte again through
 * async I/O 8 KiB at a time, each read and write a hop to the blocking pool:
 * on a fast link it took longer than the download did. `std::io::copy` into
 * a large `BufWriter` reads in large blocks, and on Linux becomes
 * `copy_file_range`, which the kernel does without copying through dm (and
 * Btrfs or XFS without copying at all). */
async fn copy_parts(parts: &[PathBuf], into: &Path) -> Result<u64, Error> {
    let (parts, into) = (parts.to_vec(), into.to_path_buf());
    tokio::task::spawn_blocking(move || copy_parts_blocking(&parts, &into))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
}

fn copy_parts_blocking(parts: &[PathBuf], into: &Path) -> Result<u64, Error> {
    use std::io::Write;
    /* Every planned part exists by the time this runs — each range creates
     * its file before its first byte — so one that does not is one that was
     * taken away, which is what a cancel does. Skipping it wrote the rest as
     * a shorter file and reported it Completed. */
    let missing =
        || Error::Internal("a staged part went missing while the file was being joined".into());
    let rest = match parts.split_first() {
        Some((first, rest)) if std::fs::rename(first, into).is_ok() => rest,
        Some((first, _)) if !first.exists() => return Err(missing()),
        // Across filesystems a rename cannot move bytes: every part is copied.
        _ => parts,
    };
    let out = if rest.len() == parts.len() {
        std::fs::File::create(into)?
    } else {
        std::fs::OpenOptions::new().append(true).open(into)?
    };
    let mut out = std::io::BufWriter::with_capacity(4 << 20, out);
    for part in rest {
        let mut source = std::fs::File::open(part).map_err(|_| missing())?;
        std::io::copy(&mut source, &mut out)?;
        drop(source);
        let _ = std::fs::remove_file(part);
    }
    out.flush()?;
    Ok(out.get_ref().metadata()?.len())
}

/// No overlap byte fetched for a range (one fetched is 0–255).
const NO_OVERLAP: u16 = u16::MAX;

/// The byte at `at` in a file, if it has one there.
async fn byte_at(path: &Path, at: u64) -> Option<u8> {
    let mut file = fs::File::open(path).await.ok()?;
    if file.metadata().await.ok()?.len() <= at {
        return None;
    }
    file.seek(std::io::SeekFrom::Start(at)).await.ok()?;
    let mut byte = [0u8; 1];
    tokio::io::AsyncReadExt::read_exact(&mut file, &mut byte)
        .await
        .ok()?;
    Some(byte[0])
}

/// Joins the parts into the finished file.
async fn assemble(
    job: &Job,
    info: &Probe,
    options: &Options,
) -> Result<(PathBuf, u64, category::Category), Error> {
    let segments = job.segments.lock().await;

    // A length the server stated has to be the length that arrived. Segments
    // are written at the offsets they were handed out, so a server that
    // answers a range with other bytes leaves gaps that nothing above this
    // line would notice — the parts are all present and the result is short.
    if let Some(expected) = info.size {
        let got: u64 = segments.iter().map(|s| s.done).sum();
        if got != expected {
            return Err(Error::ShortFile { got, expected });
        }
    }

    // Each overlap byte against the last byte of the range before it. A
    // mismatch is a server that sent bytes from the wrong place under the
    // right label; the parts are thrown away so a Retry fetches them afresh
    // rather than reassembling the same wrong bytes.
    // In file order, which a split leaves different from part order.
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&i| segments[i].start);
    for pair in order.windows(2) {
        let (previous, index) = (pair[0], pair[1]);
        let byte = job.overlaps[index].load(Ordering::Relaxed);
        if byte == NO_OVERLAP {
            continue;
        }
        let before = {
            let p = &segments[previous];
            match p.done {
                0 => None,
                done => byte_at(&job.staged(), p.start + done - 1).await,
            }
        };
        if before.is_some_and(|b| u16::from(b) != byte) {
            drop(segments);
            let mut listing = fs::read_dir(&options.parts_dir).await?;
            while let Some(entry) = listing.next_entry().await? {
                let _ = fs::remove_file(entry.path()).await;
            }
            return Err(Error::DishonestRanges);
        }
    }

    let name = if info.filename.is_empty() {
        "download".into()
    } else {
        info.filename.clone()
    };

    /* Every range is in place in the staged file, so it is the file: held
     * to the length that arrived (an unknown length was never known to
     * preallocate) and renamed into place, with nothing copied. */
    let length: u64 = info
        .size
        .unwrap_or_else(|| segments.iter().map(|s| s.done).sum());
    drop(segments);
    let staged = job.staged();
    fs::OpenOptions::new()
        .write(true)
        .open(&staged)
        .await?
        .set_len(length)
        .await?;
    finish(&name, info.content_type.as_deref(), &[staged], options).await
}

/// Everything that happens once the bytes are all on disk.
///
/// Shared by ranged downloads, streams and a torrent of one file. The first
/// two used to carry their own copy of this, including the checksum block —
/// directly under a comment claiming there was one place that decided
/// whether a file is the file.
pub(crate) async fn finish(
    name: &str,
    content_type: Option<&str>,
    parts: &[PathBuf],
    options: &Options,
) -> Result<(PathBuf, u64, category::Category), Error> {
    // Sorted by what it turned out to be, which is only knowable now: the
    // name can come from a Content-Disposition the URL never hinted at.
    let category = category::classify(name, content_type);
    /* Nothing is joined for a download that was stopped after its last byte
     * arrived: the parts are being removed underneath the join, and a file
     * assembled from whichever were still there is short and looks finished.
     * A pause is honoured here as well, so a paused row does not become a
     * completed one a moment later. */
    options.control.check()?;
    let dir = options.categories.place(&options.dest_dir, category, name);
    fs::create_dir_all(&dir).await?;
    // Cleaned as any name from outside is: a chosen "../x" stays in the
    // folder, as the rename it replaces kept it.
    let chosen = options
        .save_name
        .as_deref()
        .map(crate::probe::sanitize)
        .filter(|n| !n.trim().is_empty());
    let path = claim_path(&dir, chosen.as_deref().unwrap_or(name)).await?;

    let bytes = join_parts(parts, &path).await?;

    // Cancelled during the join: what it produced is not wanted either.
    if options.control.wanted() == crate::control::Wanted::Cancel {
        let _ = fs::remove_file(&path).await;
        return Err(Error::Cancelled);
    }

    /* A file that fails its digest is removed rather than kept beside a
     * warning: one known to be wrong, sitting in the downloads folder looking
     * like every other one, is how it comes to be opened. */
    if let Some(expected) = &options.checksum {
        if let Err(e) = checksum::verify(&path, expected).await {
            // The row must not say the file is gone while it is still there.
            if let Err(io) = fs::remove_file(&path).await {
                return Err(Error::Internal(format!(
                    "{e}; and the file could not be removed from {}: {io}",
                    path.display()
                )));
            }
            return Err(e);
        }
    }

    // The parts are consumed; what was written down about them goes too,
    // so a finished download leaves its staging directory as empty as it
    // found it.
    // A gather or a remux cut short by a crash, on a run that placed the
    // stream without them, goes too.
    for note in ["plan.json", "rendition", "joined.ts.partial", REMUXED] {
        let _ = fs::remove_file(options.parts_dir.join(note)).await;
    }
    // And each range's ledger, which the bytes they spoke for have left.
    if let Ok(mut listing) = fs::read_dir(&options.parts_dir).await {
        while let Ok(Some(entry)) = listing.next_entry().await {
            if entry.file_name().to_string_lossy().starts_with("done-") {
                let _ = fs::remove_file(entry.path()).await;
            }
        }
    }
    Ok((path, bytes, category))
}

/// Disk space promised to the downloads under way. Each download that
/// passes the free-space check claims what it still needs; the next one is
/// checked against the free space less what the others still need on the
/// same disk. Without it, downloads started together each saw the whole
/// free space and together could still fill the disk (gap 94).
///
/// A part's claim shrinks as its bytes arrive (they are then counted by the
/// file system's own free space instead); the finished file's claim on
/// another disk is whole until the download ends.
#[derive(Clone, Default)]
pub struct Room {
    inner: Arc<std::sync::Mutex<Claims>>,
    /// Where the free space is read; `free_space` unless a test says.
    free: Option<Arc<FreeSpace>>,
}

/// A reader of a folder's free space.
type FreeSpace = dyn Fn(&Path) -> Option<u64> + Send + Sync;

#[derive(Default)]
struct Claims {
    next: u64,
    held: std::collections::HashMap<u64, Claim>,
}

struct Claim {
    dir: PathBuf,
    size: u64,
    /// The download's bytes so far, for a part's claim; `None` for a
    /// finished file's, which is whole until the end.
    done: Option<Arc<AtomicU64>>,
}

impl Claim {
    fn outstanding(&self) -> u64 {
        match &self.done {
            Some(done) => self.size.saturating_sub(done.load(Ordering::Relaxed)),
            None => self.size,
        }
    }
}

impl std::fmt::Debug for Room {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Room").finish_non_exhaustive()
    }
}

impl Room {
    /// A ledger that reads free space through `free`: for tests, a disk of
    /// any size.
    pub fn with_free_space(free: impl Fn(&Path) -> Option<u64> + Send + Sync + 'static) -> Self {
        Room {
            inner: Arc::default(),
            free: Some(Arc::new(free)),
        }
    }

    fn free_space(&self, dir: &Path) -> Option<u64> {
        match &self.free {
            Some(free) => free(dir),
            None => free_space(dir),
        }
    }
}

/// What one download has claimed, given back when it ends however it ends.
pub struct Claimed {
    room: Room,
    ids: Vec<u64>,
}

impl Drop for Claimed {
    fn drop(&mut self) {
        let mut claims = crate::lock(&self.room.inner);
        for id in &self.ids {
            claims.held.remove(id);
        }
    }
}

/// Refuses a download the disk cannot hold, before a byte of it is fetched
/// (gap 60). Without it a stated 100 TB ran until the disk was full,
/// taking every other program's room with it. The parts need what is not
/// yet staged; the finished file needs its whole length where it lands,
/// when that is another disk (on the same one, assembly frees each part as
/// it goes). What other downloads under way still need on the same disk is
/// taken from the free space first (`Room`). Where the free space cannot be
/// read, nothing is refused. The claim lasts as long as what is returned.
fn room_for(size: u64, done: &Arc<AtomicU64>, options: &Options) -> Result<Claimed, Error> {
    let room = &options.room;
    let parts = &options.parts_dir;
    let dest = &options.dest_dir;
    let mut needs = vec![(parts, Some(Arc::clone(done)))];
    if !same_disk(parts, dest) {
        needs.push((dest, None));
    }
    // Held across the check and the claim, so two downloads starting at
    // once cannot both pass against the same space.
    let mut claims = crate::lock(&room.inner);
    for (dir, counts) in &needs {
        let need = match counts {
            Some(done) => size.saturating_sub(done.load(Ordering::Relaxed)),
            None => size,
        };
        let Some(free) = room.free_space(dir) else {
            continue;
        };
        let promised: u64 = claims
            .held
            .values()
            .filter(|c| same_disk(&c.dir, dir))
            .map(Claim::outstanding)
            .sum();
        if need > free.saturating_sub(promised) {
            let others = if promised > 0 {
                format!(
                    ", {} of it already needed by downloads under way",
                    amount(promised.min(free))
                )
            } else {
                String::new()
            };
            // The folder the person chose, not the row's own staging
            // folder inside dm's (QA loop 1, bug 4): the download folder
            // where the two share a disk, the chosen parts folder where not.
            let named = if same_disk(parts, dest) {
                dest.as_path()
            } else if dir == &parts {
                parts.parent().unwrap_or(parts)
            } else {
                dir.as_path()
            };
            return Err(Error::Internal(format!(
                "not enough room: the file needs {}, and {} has {} free{others}",
                amount(need),
                named.display(),
                amount(free)
            )));
        }
    }
    let mut ids = Vec::new();
    for (dir, counts) in needs {
        claims.next += 1;
        let id = claims.next;
        claims.held.insert(
            id,
            Claim {
                dir: dir.clone(),
                size,
                done: counts,
            },
        );
        ids.push(id);
    }
    drop(claims);
    Ok(Claimed {
        room: room.clone(),
        ids,
    })
}

/// Bytes as a person reads them.
fn amount(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{value:.1} {}", UNITS.get(unit).unwrap_or(&"bytes"))
    }
}

/// Whether two folders are on one disk. Unix by device number; Windows by
/// drive. Unreadable is taken as one disk, the check that asks less.
fn same_disk(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev(),
            _ => true,
        }
    }
    #[cfg(not(unix))]
    {
        let drive = |p: &Path| {
            std::path::absolute(p).ok().and_then(|p| {
                p.components()
                    .next()
                    .map(|c| c.as_os_str().to_ascii_lowercase())
            })
        };
        drive(a) == drive(b)
    }
}

/// The space this account may still write in the folder's file system.
#[cfg(unix)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `stat` is a buffer of the type statvfs fills; it is read only when the
    // call says it succeeded.
    let done = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if done != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it filled the buffer.
    let stat = unsafe { stat.assume_init() };
    // The field types differ between systems; u64 holds each.
    #[allow(clippy::unnecessary_cast)]
    reported_free(
        (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64),
        (stat.f_blocks as u64).saturating_mul(stat.f_frsize as u64),
    )
}

/// The free space a file system reported, or unknown when it reported no
/// size at all. Some FUSE and network mounts answer zero free of zero
/// total, and taken at their word every download of known length was
/// refused (gap 78).
fn reported_free(free: u64, total: u64) -> Option<u64> {
    (total > 0).then_some(free)
}

/// The space this account may still write in the folder's volume.
#[cfg(windows)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the two
    // out-pointers are live u64s and the third may be null.
    let done = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    if done == 0 {
        return None;
    }
    reported_free(free, total)
}

#[cfg(not(any(unix, windows)))]
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}

/// Where one range of a download is staged.
///
/// The name is a contract across runs — a part file the next run cannot find
/// is a range silently fetched again — so it is derived in one place rather
/// than re-typed wherever a part is opened.
fn part_path(parts_dir: &Path, index: usize) -> PathBuf {
    parts_dir.join(format!("part-{index:04}"))
}

/// The file a ranged download is written into, every range at its own
/// offset.
///
/// Ranges used to go into a part file each, and the parts were copied into
/// the finished file at the end: every byte written twice, and on a fast
/// link the copy took longer than the fetch (2 GB: 4.4 s fetched, then 7 to
/// 9 s joining, `qa/baseline.md`). Written in place, finishing is a rename.
pub(crate) const STAGED: &str = "staged";

pub(crate) fn staged_path(parts_dir: &Path) -> PathBuf {
    parts_dir.join(STAGED)
}

/// The ledger of range `index`: how many of its bytes are on disk, in
/// decimal. A part file's length used to be this number; in one shared
/// file, where every range's region exists from the start, it is written
/// down instead.
pub(crate) fn ledger_path(parts_dir: &Path, index: usize) -> PathBuf {
    parts_dir.join(format!("done-{index:04}"))
}

/// Writes down how much of a range is on disk. Behind the bytes, never ahead
/// of them: written after they are handed to the disk, and one that lags
/// costs only the bytes fetched again. Always as many digits, written over
/// the last in one write, so a shorter number never leaves a longer one's
/// tail behind it (which would read as more than is there).
async fn put(ledger: &mut fs::File, done: u64) -> Result<(), Error> {
    use tokio::io::AsyncWriteExt;
    ledger.seek(std::io::SeekFrom::Start(0)).await?;
    ledger.write_all(format!("{done:020}").as_bytes()).await?;
    ledger.flush().await?;
    Ok(())
}

/// What range `index`'s ledger says, if it has one that reads. One cut
/// short by a crash reads as nothing, which is safe: a ledger behind the
/// bytes costs a fetch, never a wrong file.
pub(crate) fn read_ledger(parts_dir: &Path, index: usize) -> Option<u64> {
    std::fs::read_to_string(ledger_path(parts_dir, index))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The index a staged file's name gives it: `part-0003` or `done-0003`.
fn staged_index(name: &str) -> Option<usize> {
    name.strip_prefix("part-")
        .or_else(|| name.strip_prefix("done-"))
        .and_then(|n| n.parse().ok())
}

/// A part file an earlier dm left, copied into the staged file at its
/// range's offset and written down in the range's ledger: a download begun
/// before ranges were written in place resumes rather than starting again.
fn adopt_part(parts_dir: &Path, index: usize, start: u64, span: u64) -> std::io::Result<u64> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let part = part_path(parts_dir, index);
    let mut source = std::fs::File::open(&part)?;
    let mut staged = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(staged_path(parts_dir))?;
    staged.seek(SeekFrom::Start(start))?;
    let copied = std::io::copy(&mut (&mut source).take(span), &mut staged)?;
    staged.flush()?;
    std::fs::write(ledger_path(parts_dir, index), format!("{copied:020}"))?;
    drop(source);
    std::fs::remove_file(&part)?;
    Ok(copied)
}

/// Whether files can be made in the download folder, said with its path.
///
/// The folder was first written to when the finished file was placed, so a
/// read-only folder — a removed disk, a permission changed, a sandbox — cost
/// the whole transfer and then said "disk: Permission denied", naming
/// neither the folder nor what to do about it.
async fn writable(dir: &Path) -> Result<(), Error> {
    let said = |e: std::io::Error| {
        Error::Internal(format!(
            "the folder {} cannot be written to ({e}); choose another for this download \
             (Move/Rename) or for every download (Options › Folders)",
            dir.display()
        ))
    };
    fs::create_dir_all(dir).await.map_err(said)?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let probe = dir.join(format!(".dm-write-check-{}-{n}", std::process::id()));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .await
        .map_err(said)?;
    let _ = fs::remove_file(&probe).await;
    Ok(())
}

/// `name`, then `name (1)`, `name (2)`… — the paths a file may be saved at.
pub(crate) fn candidates<'a>(dir: &'a Path, name: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    std::iter::once(dir.join(name))
        .chain((1..10_000).map(move |n| dir.join(format!("{stem} ({n}){ext}"))))
}

/// The name a file called `name` would be saved under in `dir` as things
/// stand: `name` if nothing has it, else the first `name (n)` free. A
/// guess for showing — [`claim_path`] decides when the file is placed.
pub fn free_name(dir: &Path, name: &str) -> String {
    candidates(dir, name)
        .find(|path| std::fs::symlink_metadata(path).is_err())
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| name.to_string())
}

/// The name the `nth` download of `name` into `dir` will get, counting from
/// 0, where `nth` others of that name are already waiting to be placed
/// there: each takes the next free one in turn.
pub fn nth_free_name(dir: &Path, name: &str, nth: usize) -> String {
    candidates(dir, name)
        .filter(|path| std::fs::symlink_metadata(path).is_err())
        .nth(nth)
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| name.to_string())
}

fn all_taken(dir: &Path, name: &str) -> Error {
    Error::Internal(format!(
        "ten thousand files named {name} already in {}; nothing will be overwritten",
        dir.display()
    ))
}

/// A folder nothing is using, taken by creating it empty — for a torrent of
/// several files, which arrives as a tree. The same race as [`claim_path`]:
/// a look and then a move could land on a folder made in between.
///
/// An existing file is somebody's, and a download manager that overwrites one
/// is worse than one that fails — so past ten thousand of the same name it
/// fails, rather than falling back to the one path it knows is taken.
pub(crate) async fn claim_dir(dir: &Path, name: &str) -> Result<PathBuf, Error> {
    for candidate in candidates(dir, name) {
        match fs::create_dir(&candidate).await {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(all_taken(dir, name))
}

/// A path nothing is using, taken by creating it empty.
///
/// Looking and then writing let two downloads finishing at once with the
/// same name both choose `f.bin`, and the second rename replaced the first;
/// a file of the person's own made in that moment went the same way.
/// `create_new` asks the filesystem, which answers one of them only.
pub(crate) async fn claim_path(dir: &Path, name: &str) -> Result<PathBuf, Error> {
    for candidate in candidates(dir, name) {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .await
        {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(all_taken(dir, name))
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
    /// placed under (gap 47: `big.bin` until it finished as `big (1).bin`).
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
            sign_in: None,
            save_name: None,
            system_proxy: false,
        }
    }

    /// Gap 78: a mount reporting zero free of zero total is unknown, not
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
