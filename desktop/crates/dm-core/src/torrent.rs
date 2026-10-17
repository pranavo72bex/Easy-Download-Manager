//! BitTorrent, the one kind of download the browser side cannot do at all.
//!
//! An extension has no sockets, so a magnet link is the clearest case of
//! something only the desktop engine can take. This wraps `librqbit` in the
//! same shape as the HTTP downloader: given a URL and the shared `Options`,
//! it reports progress and returns a `Finished`, so the queue drives a
//! torrent exactly as it drives a file and nothing above this line has to
//! know the difference.
//!
//! One session per torrent, torn down by a cancellation token when the row is
//! paused or cancelled. That is heavier than a single shared session, but it
//! keeps a torrent's whole lifetime inside one call the queue already knows
//! how to start, pause and resume — a paused torrent's files stay on disk and
//! a fresh run resumes from them.

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions, TorrentStatsState};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::download::{Finished, Options, Update};
use crate::{category, Error, Progress};

/// Whether this is a download for the torrent engine rather than the HTTP one.
///
/// A `.torrent` served over HTTP is caught here too: fetched as a file it
/// would save the few kilobytes of metainfo rather than the gigabytes it
/// describes, which is never what anyone means by downloading a torrent.
pub fn is_torrent(url: &str) -> bool {
    is_magnet(url) || ends_with_ignoring_case(strip_query(url.trim()), ".torrent")
}

/// Whether this is a magnet link. A scheme is case-insensitive, so
/// `Magnet:` is one too, and would otherwise have been fetched over HTTP.
fn is_magnet(url: &str) -> bool {
    url.trim()
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("magnet:"))
}

fn ends_with_ignoring_case(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len()
        && s.get(s.len() - suffix.len()..)
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
}

fn strip_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// How often the row is redrawn while a torrent transfers.
const TICK: Duration = Duration::from_millis(500);

/// Runs one torrent to completion, reporting as it goes.
///
/// `client` is dm's own, so a `.torrent` fetched over HTTP goes through
/// whatever proxy the file downloads use. The swarm is librqbit's to reach,
/// and it is told the proxy too where it can use one — SOCKS5 — and refused
/// where it cannot, rather than announcing to trackers and peers directly on
/// a machine configured to make no direct connection.
pub async fn download_torrent(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<Sender<Update>>,
) -> Result<Finished, Error> {
    if let Some(why) = options.no_route_through_system_proxy(url, "a torrent") {
        return Err(Error::Internal(why));
    }
    let connect = swarm_proxy(options.proxy_for(url))?;

    /* Staged like every other download, under its own parts directory, and
     * moved into the download folder only when whole. Written straight into
     * the download folder, a paused torrent's half-fetched files sat there
     * looking finished, and removing the row left them: the queue clears a
     * row's parts directory, and a torrent had nothing in it. */
    let output = options.parts_dir.clone();
    tokio::fs::create_dir_all(&output)
        .await
        .map_err(Error::Disk)?;

    // The whole session shuts down when this is cancelled, which is how a
    // pause or a cancel stops every socket at once rather than leaving peers
    // connected to a download nobody is watching.
    let token = CancellationToken::new();
    // This torrent's claim on the shared ceiling, given back on every exit.
    let _share = options.limiter.share();
    let session = Session::new_with_opts(
        output.clone(),
        SessionOptions {
            // Off: dm's queue is the record of what to resume, and a second
            // one in librqbit's own state file would be a thing to keep in
            // step for no gain.
            persistence: None,
            // The DHT on, for magnets with no tracker, but without its state
            // file: that file is one per user, so a second session (two
            // torrents at once, or a resume while the paused one is still
            // closing) failed to start with "error initializing persistent
            // DHT". Each session finds peers afresh on a port of its own.
            dht: Some(librqbit::DhtSessionConfig {
                persistence: None,
                ..Default::default()
            }),
            cancellation_token: Some(token.clone()),
            ratelimits: librqbit::limits::LimitsConfig {
                download_bps: download_cap(options).await,
                upload_bps: None,
            },
            connect,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| Error::Internal(format!("the torrent engine would not start: {e}")))?;

    // Dropped on every exit, so a paused or failed torrent does not leave a
    // session listening on a port for the life of the process.
    let _guard = CancelOnDrop(token.clone());

    let add = if is_magnet(url) {
        // In the spelling librqbit looks for: it matches `magnet:` exactly.
        let rest = url.trim().get(7..).unwrap_or_default();
        AddTorrent::Url(format!("magnet:{rest}").into())
    } else {
        // Fetched with dm's client rather than librqbit's, so a .torrent
        // behind a proxy is reachable exactly as a file would be, and with
        // the page's referrer, so one served only to its own page is too.
        let bytes = crate::download::or_stop(
            &options.control,
            fetch_metainfo(
                client,
                url,
                options.referrer.as_deref(),
                options.sign_in.as_ref(),
            ),
        )
        .await?;
        AddTorrent::TorrentFileBytes(bytes.into())
    };

    /* Raced against the row's control: a magnet's metadata comes from its
     * peers, and with no peer answering the add waits with no end. Awaited
     * on its own, a pause or a cancel of such a magnet never took effect.
     * Stopped here, the guard below cancels the token, which shuts the
     * session and its half-made lookup down with it. */
    let added = crate::download::or_stop(&options.control, async {
        session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    // Resume from and seed the files already on disk: a
                    // paused torrent that could not overwrite its own parts
                    // would start from nothing every time it resumed.
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| Error::Internal(format!("the torrent could not be added: {e}")))
    })
    .await?;
    let handle = added
        .into_handle()
        .ok_or_else(|| Error::Internal("the torrent produced no handle".into()))?;

    let mut named = false;
    let mut last = Instant::now();
    /* From the first sample, not from zero: a resumed torrent's first report
     * already counts the bytes on disk, and measured from zero they showed
     * as a burst of speed nobody downloaded. */
    let mut last_done: Option<u64> = None;

    loop {
        // A pause or cancel is honoured here, between samples, the same place
        // the HTTP engine checks it. Cancelling deletes the partial files;
        // pausing leaves them for a resume to pick up.
        match options.control.check() {
            Ok(()) => {}
            Err(stop) => {
                token.cancel();
                if matches!(stop, Error::Cancelled) {
                    let _ = session.delete(handle.id().into(), true).await;
                }
                return Err(stop);
            }
        }

        // The shared ceiling can change while this runs — someone drags the
        // limit slider — and an HTTP download picks that up on its next chunk.
        // A torrent must too, or "one ceiling governs files and torrents
        // together" would hold only at the moment it started.
        session
            .ratelimits
            .set_download_bps(download_cap(options).await);

        let stats = handle.stats();

        // The name is known only once the metadata is, which for a magnet
        // means after the first peers answer. Sent as soon as it is, so the
        // row stops showing the info-hash and starts showing the name.
        if !named {
            if let Some(name) = handle.name() {
                if let Some(tx) = &report {
                    let _ = tx
                        .send(Update::Named {
                            category: category::classify(&name, None),
                            name,
                        })
                        .await;
                }
                named = true;
            }
        }

        if let Some(tx) = &report {
            let now = Instant::now();
            let dt = (now - last).as_secs_f64();
            let done = stats.progress_bytes;
            let total = (stats.total_bytes > 0).then_some(stats.total_bytes);
            // Measured rather than read off librqbit's own estimator, so the
            // number is in the same units and computed the same way as every
            // other row.
            let speed = match last_done {
                Some(before) if dt > 0.0 => done.saturating_sub(before) as f64 / dt,
                _ => 0.0,
            };
            let eta = match total {
                Some(total) if speed > 1.0 && total > done => {
                    Some(Duration::from_secs_f64((total - done) as f64 / speed))
                }
                _ => None,
            };
            let _ = tx
                .send(Update::Progress(Progress {
                    downloaded: done,
                    total,
                    speed,
                    eta,
                }))
                .await;
            last = now;
            last_done = Some(done);
        }

        if stats.finished {
            break;
        }
        if let TorrentStatsState::Error = stats.state {
            token.cancel();
            return Err(Error::Internal(
                stats.error.unwrap_or_else(|| "the torrent failed".into()),
            ));
        }

        tokio::time::sleep(TICK).await;
    }

    let name = handle
        .name()
        .unwrap_or_else(|| handle.info_hash().as_string());
    let bytes = handle.stats().total_bytes;

    /* Where librqbit put it, read from the handle rather than guessed from
     * the name: one file is written straight into the output folder under
     * its own name, and only a tree gets a folder named for the torrent. */
    let staged = handle
        .with_metadata(|metadata| match metadata.file_infos.as_slice() {
            [only] => handle.output_folder().join(&only.relative_filename),
            _ => handle.output_folder().to_path_buf(),
        })
        .map_err(|e| Error::Internal(format!("the finished torrent has no metadata: {e}")))?;

    // Forgotten by the session, files kept, so nothing holds them open
    // while they are moved into place.
    let id = handle.id();
    drop(handle);
    session
        .delete(id.into(), false)
        .await
        .map_err(|e| Error::Internal(format!("the finished torrent could not be released: {e}")))?;
    drop(session);

    let (path, category, verified) = place(&staged, &name, options).await?;

    Ok(Finished {
        path,
        bytes,
        // A torrent has no fixed connection count; it is however many peers
        // happen to be in the swarm. Zero rather than a number that means
        // something different from what the column says everywhere else.
        connections: 0,
        category,
        verified,
        mirrors: 0,
    })
}

/// Moves what a torrent produced into the download folder.
///
/// One file is placed exactly as any other download is — sorted into its
/// category folder and checked against the digest it was given, by the same
/// code. A tree of files keeps the structure the torrent names, under the
/// download folder itself: category sorting is for one file with one kind.
/// A digest cannot be checked over a tree, and a row that carries one is
/// failed rather than completed with the digest quietly unchecked.
async fn place(
    staged: &Path,
    name: &str,
    options: &Options,
) -> Result<(PathBuf, category::Category, bool), Error> {
    if staged.is_file() {
        // Under the file's own name, not the torrent's: the two differ when
        // a torrent of one file is named for its release.
        let file_name = staged
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string());
        let (path, _, category) =
            crate::download::finish(&file_name, None, &[staged.to_path_buf()], options).await?;
        return Ok((path, category, options.checksum.is_some()));
    }
    if options.checksum.is_some() {
        return Err(Error::Internal(
            "a checksum cannot be checked over a torrent of several files".into(),
        ));
    }
    options.control.check()?;
    tokio::fs::create_dir_all(&options.dest_dir)
        .await
        .map_err(Error::Disk)?;
    let folder = folder_name(name);
    let dest = crate::download::claim_dir(&options.dest_dir, &folder).await?;
    if let Err(e) = move_tree(staged, &dest).await {
        // Only the empty folder this claimed; never anything with files in.
        let _ = tokio::fs::remove_dir(&dest).await;
        return Err(e);
    }
    Ok((dest, category::classify(&folder, None), false))
}

/// The folder a torrent of several files lands in, from the name the
/// torrent gives itself — which is the torrent's to say, and so cleaned as
/// any remote name is: `../.config/autostart` must not leave the download
/// folder, nor arrive hidden.
fn folder_name(name: &str) -> String {
    let cleaned = crate::probe::sanitize(name);
    if cleaned.is_empty() {
        "torrent".into()
    } else {
        cleaned
    }
}

/// Moves a directory: a rename where the two are on one filesystem, a copy
/// and a removal where they are not.
async fn move_tree(from: &Path, to: &Path) -> Result<(), Error> {
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(());
    }
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        copy_tree(&from, &to)?;
        std::fs::remove_dir_all(&from)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
    .map_err(Error::Disk)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// How the swarm is reached when a proxy is configured.
///
/// librqbit can carry trackers and peers over SOCKS5 and nothing else. An
/// HTTP proxy cannot carry a peer connection, so a torrent on a machine with
/// one configured is refused with the reason rather than sent out around it.
fn swarm_proxy(proxy: Option<&str>) -> Result<Option<librqbit::ConnectionOptions>, Error> {
    let Some(proxy) = proxy else {
        return Ok(None);
    };
    /* librqbit names SOCKS5 `socks5://` and refuses `socks5h://`, which is
     * what dm's settings hand out so names resolve at the proxy. Peers are
     * addresses already; only a tracker's name differs between the two. */
    let proxy = match proxy.strip_prefix("socks5h://") {
        Some(rest) => format!("socks5://{rest}"),
        None => proxy.to_string(),
    };
    if !proxy.starts_with("socks5://") {
        return Err(Error::Internal(
            "a torrent can only go through a SOCKS5 proxy; the configured proxy is not one, \
             and BitTorrent will not go around it"
                .into(),
        ));
    }
    Ok(Some(librqbit::ConnectionOptions {
        proxy_url: Some(proxy),
        ..Default::default()
    }))
}

/// The shared download ceiling, in the shape librqbit wants it.
///
/// This torrent's part of the limiter every HTTP download shares, re-read
/// every tick, so one ceiling governs files, torrents and yt-dlp runs
/// together as they start and stop. Zero
/// — no limit — becomes `None`, which is librqbit's word for the same thing.
async fn download_cap(options: &Options) -> Option<NonZeroU32> {
    let bps = options.limiter.part().await;
    u32::try_from(bps).ok().and_then(NonZeroU32::new)
}

/// Fetches a `.torrent` file, bounded so a link that is not one cannot ask
/// for an unbounded allocation.
async fn fetch_metainfo(
    client: &reqwest::Client,
    url: &str,
    referrer: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Vec<u8>, Error> {
    // A metainfo file is kilobytes; ten megabytes is far past any real one
    // and small enough that a mislabelled link cannot exhaust memory.
    const CAP: usize = 10 * 1024 * 1024;
    let too_large = || Error::Internal("that .torrent file is implausibly large".into());

    let res = crate::auth::send(
        client,
        crate::download::get(client, url, referrer, sign_in),
        url,
    )
    .await?;
    let res = Error::refused_or(res).await?;
    // A stated length past the cap is rejected before a byte is read.
    if res.content_length().is_some_and(|len| len > CAP as u64) {
        return Err(too_large());
    }

    // And enforced while reading, because Content-Length is a claim: a body
    // that keeps coming past the cap is stopped rather than buffered whole.
    let mut body = Vec::new();
    let mut chunks = res.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > CAP {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Cancels the session's token however the download ends, including a panic
/// or an early return, so no path can leave a torrent's sockets open.
struct CancelOnDrop(CancellationToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnets_and_torrent_files_are_for_the_torrent_engine() {
        assert!(is_torrent("magnet:?xt=urn:btih:0123456789abcdef"));
        assert!(is_torrent("  magnet:?xt=urn:btih:x  ")); // pasted with spaces
        assert!(is_torrent("https://example.com/ubuntu.torrent"));
        // A query string is where a signed link keeps its token; the path is
        // still what says .torrent.
        assert!(is_torrent("https://example.com/get.torrent?token=abc"));
        // A scheme and an extension in capitals are the same thing.
        assert!(is_torrent("Magnet:?xt=urn:btih:0123456789abcdef"));
        assert!(is_torrent("https://example.com/X.TORRENT"));
    }

    #[test]
    fn only_a_socks5_proxy_can_carry_a_swarm() {
        assert!(matches!(swarm_proxy(None), Ok(None)));
        let Ok(Some(socks)) = swarm_proxy(Some("socks5://h:1080")) else {
            panic!("a SOCKS5 proxy was not handed to the swarm");
        };
        assert_eq!(socks.proxy_url.as_deref(), Some("socks5://h:1080"));
        // What dm's settings hand out, in the spelling librqbit reads.
        let Ok(Some(remote)) = swarm_proxy(Some("socks5h://h:1080")) else {
            panic!("a SOCKS5 proxy that resolves names was not handed to the swarm");
        };
        assert_eq!(remote.proxy_url.as_deref(), Some("socks5://h:1080"));
        // An HTTP proxy cannot carry a peer connection, and going around it
        // is the one thing a configured proxy must never see.
        let Err(refused) = swarm_proxy(Some("http://h:8080")) else {
            panic!("an HTTP proxy was accepted for a swarm");
        };
        assert!(refused.to_string().contains("SOCKS5"), "{refused}");
    }

    #[tokio::test]
    async fn a_torrents_own_name_cannot_lead_out_of_the_download_folder() {
        assert_eq!(folder_name("../.config/autostart"), "autostart");
        assert_eq!(folder_name(".."), "torrent");
        assert_eq!(folder_name(""), "torrent");
        assert_eq!(folder_name("Album (2024)"), "Album (2024)");
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("Downloads");
        tokio::fs::create_dir_all(&downloads).await.unwrap();
        let claimed = crate::download::claim_dir(&downloads, &folder_name("../.config/autostart"))
            .await
            .unwrap();
        assert_eq!(claimed.parent(), Some(downloads.as_path()));
    }

    #[test]
    fn ordinary_files_are_not() {
        assert!(!is_torrent("https://example.com/ubuntu.iso"));
        assert!(!is_torrent("https://example.com/video.mp4"));
        // The word appears, but not as the extension: this is a page, not a
        // metainfo file, and fetching it as a torrent would be wrong.
        assert!(!is_torrent("https://example.com/torrent/list"));
        assert!(!is_torrent("https://example.com/a.torrent.html"));
    }
}
