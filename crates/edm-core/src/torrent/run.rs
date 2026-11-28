// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! [`download_torrent`]'s phases: what the torrent is added from, its files checked, the
//! transfer sampled until it is whole, and what is left once it is.

use edm_torrent::Torrent;

use super::*;

/// What the torrent is added from: a magnet, a `.torrent` on this computer, or one fetched.
pub(super) async fn source_of(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    proxied: bool,
) -> Result<Source, Error> {
    let add = if is_magnet(url) {
        // In the spelling the engine looks for: it matches `magnet:` exactly.
        let rest = url.trim().get(7..).unwrap_or_default();
        let magnet = format!("magnet:{rest}");
        Source::Magnet(if proxied {
            tcp_trackers_of_magnet(&magnet)?
        } else {
            magnet
        })
    } else if let Some(path) = local_file(url) {
        // A .torrent on this computer, opened with EDM from a file manager.
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            Error::Internal(format!(
                "could not read the torrent file {}: {e}",
                path.display()
            ))
        })?;
        Source::File(
            if proxied {
                tcp_trackers_of(&bytes)?
            } else {
                bytes
            }
            .into(),
        )
    } else {
        // Fetched with EDM's client rather than librqbit's, so a .torrent
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
        Source::File(
            if proxied {
                tcp_trackers_of(&bytes)?
            } else {
                bytes
            }
            .into(),
        )
    };
    Ok(add)
}

/// Its files, once known: refused where one would land outside its folder, or where a
/// digest is asked of several; told to the row for a person to choose from.
pub(super) async fn check_files(
    session: &Session,
    handle: &Torrent,
    token: &CancellationToken,
    options: &Options,
    report: Option<&Sender<Update>>,
) -> Result<(), Error> {
    /* A file name inside the torrent is the swarm's to choose: one with a
     * drive, a stream suffix or a device name would land outside the folder
     * the files go in. Refused as soon as the list is known. The torrent is
     * removed without deleting files: a delete would act on that path. */
    let escaping = handle
        .files()
        .map(|files| files.iter().any(|file| !plain_torrent_path(&file.relative)))
        .unwrap_or(false);
    if escaping {
        let _ = session.forget(handle.id(), false).await;
        token.cancel();
        return Err(Error::Internal(
            "the torrent names a file outside its own folder, so it was not downloaded".into(),
        ));
    }

    // Its files, once known, for a person to choose from.
    if let Some(tx) = report {
        let files = handle
            .files()
            .map(|files| {
                files
                    .iter()
                    .map(|file| TorrentFile {
                        name: file.relative.to_string_lossy().replace('\\', "/"),
                        size: file.len,
                        wanted: true,
                        priority: Priority::Normal,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !files.is_empty() {
            let _ = tx.send(Update::Files(files)).await;
        }
    }

    // A digest is for one file: refused as soon as the metadata shows more,
    // rather than after the whole tree was fetched for nothing.
    let several = handle.files().map(|files| files.len() > 1).unwrap_or(false);
    if several && options.checksum.is_some() {
        let _ = session.forget(handle.id(), true).await;
        token.cancel();
        return Err(Error::Internal(
            "a checksum cannot be checked over a torrent of several files".into(),
        ));
    }
    Ok(())
}

/// Every wanted file's bytes, where the files' priorities differ: the row's total.
pub(super) fn tiered_total(handle: &Torrent, tiers: &[Vec<usize>]) -> Option<u64> {
    (tiers.len() > 1)
        .then(|| {
            handle
                .files()
                .map(|files| {
                    tiers
                        .iter()
                        .flatten()
                        .filter_map(|&i| files.get(i).map(|f| f.len))
                        .sum::<u64>()
                })
                .ok()
        })
        .flatten()
}

/// Samples the transfer until every tier is whole, reporting as it goes: a pause or a
/// cancel honoured between samples, and the shared ceiling followed as it changes.
pub(super) async fn transfer(
    session: &Session,
    handle: &Torrent,
    token: &CancellationToken,
    options: &Options,
    report: Option<&Sender<Update>>,
    tiers: &[Vec<usize>],
    tiered_total: Option<u64>,
) -> Result<(), Error> {
    let mut tier = 0;
    let mut named = false;
    let mut last = Instant::now();
    /* From the first sample, not from zero: a resumed torrent's first report
     * already counts the bytes on disk, and measured from zero they showed
     * as a burst of speed nobody downloaded. */
    let mut last_done: Option<u64> = None;
    let mut last_sent: Option<u64> = None;
    // The network it started on: where the computer moves to another, it announces itself
    // again there (gaps row N8).
    let mut follow = Follow::new();

    loop {
        // A pause or cancel is honoured here, between samples, the same place
        // the HTTP engine checks it. Cancelling deletes the partial files;
        // pausing leaves them for a resume to pick up.
        match options.control.check() {
            Ok(()) => {}
            Err(stop) => {
                token.cancel();
                if matches!(stop, Error::Cancelled) {
                    let _ = session.forget(handle.id(), true).await;
                }
                return Err(stop);
            }
        }

        // The shared ceiling can change while this runs — someone drags the
        // limit slider — and an HTTP download picks that up on its next chunk.
        // A torrent must too, or "one ceiling governs files and torrents
        // together" would hold only at the moment it started.
        // Only when it changed: each call makes a fresh limiter with a full
        // second's allowance, and set twice a second it let a torrent run at
        // three times its share.
        let cap = download_cap(options).await;
        if session.download_bps() != cap {
            session.set_download_bps(cap);
        }

        let stats = handle.stats();

        // The name is known only once the metadata is, which for a magnet
        // means after the first peers answer. Sent as soon as it is, so the
        // row stops showing the info-hash and starts showing the name.
        if !named {
            if let Some(name) = handle.name() {
                if let Some(tx) = report {
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

        if let Some(tx) = report {
            let now = Instant::now();
            let dt = (now - last).as_secs_f64();
            let total = tiered_total.or((stats.total_bytes > 0).then_some(stats.total_bytes));
            /* Never less than already reported. The engine's count
             * drops when a piece fails its hash and is fetched again, and
             * while it checks what is on disk; a row whose bar went back
             * read as a download losing data. The high-water mark, held to
             * the total, catches up once the piece arrives again. */
            // While it initialises, the engine's count is how much of what is
            // on disk it has checked, not what it has: a fresh torrent showed
            // a piece "downloaded" with no peer connected, now and then.
            let have = match stats.state {
                State::Initializing => last_done.unwrap_or(0),
                _ => stats.progress_bytes,
            };
            let done = high_water(last_done, have, total);
            // Measured rather than read off the engine's own estimator, so the
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
                    estimated: false,
                }))
                .await;
            let swarm = Swarm::of(&stats, last_sent, dt);
            let _ = tx.send(Update::Swarm(swarm)).await;
            last = now;
            last_done = Some(done);
            last_sent = Some(swarm.uploaded);
        }

        if stats.finished {
            if tier + 1 >= tiers.len() {
                break;
            }
            // This tier is whole: the next one joins the files asked for.
            tier += 1;
            let asked: std::collections::HashSet<usize> =
                tiers[..=tier].iter().flatten().copied().collect();
            if let Err(e) = session.ask_for(handle, &asked).await {
                token.cancel();
                return Err(Error::Internal(format!(
                    "the torrent's next files could not be asked for: {e}"
                )));
            }
        }
        if let State::Error = stats.state {
            token.cancel();
            return Err(Error::Internal(
                stats.error.unwrap_or_else(|| "the torrent failed".into()),
            ));
        }

        follow.follow(session, handle).await;
        tokio::time::sleep(TICK).await;
    }
    Ok(())
}

/// The finished torrent's name, its size and where its engine put it, the files a person
/// left out removed.
pub(super) fn finished_files(
    handle: &Torrent,
    options: &Options,
) -> Result<(String, u64, PathBuf), Error> {
    let name = handle.name().unwrap_or_else(|| handle.info_hash());
    let bytes = handle.stats().total_bytes;

    /* Where the engine put it, read from the handle rather than guessed from
     * the name: one file is written straight into the output folder under
     * its own name, and only a tree gets a folder named for the torrent. */
    let staged = handle
        .files()
        .map(|files| match files.as_slice() {
            [only] => handle.output_folder().join(&only.relative),
            _ => handle.output_folder().to_path_buf(),
        })
        .map_err(|e| Error::Internal(format!("the finished torrent has no metadata: {e}")))?;

    /* The files left out are not the person's: pieces they share with a
     * wanted file leave them partly written, and moved with the rest they
     * would look like files that were fetched. */
    // Every tier's files: `only` is the first tier's alone where there are several.
    if let Some(only) = &options.torrent.wanted() {
        if let Ok(files) = handle.files() {
            for (i, file) in files.iter().enumerate() {
                if !only.contains(&i) {
                    let _ = std::fs::remove_file(handle.output_folder().join(&file.relative));
                }
            }
        }
    }
    Ok((name, bytes, staged))
}

/// Shared from where it now is, where the person keeps finished torrents shared.
pub(super) fn share_finished(
    options: &Options,
    url: &str,
    again: Option<Source>,
    staged: &Path,
    path: &Path,
    bytes: u64,
) {
    // Shared from where it now is, where the person keeps finished torrents shared. Not
    // where the file was renamed on the way (a name already taken): the torrent names it
    // otherwise, and its engine would start a fresh file under that name.
    if let (Some(share), Some(again)) = (options.torrent.share, again) {
        let folder = if staged.is_dir() || path.is_dir() {
            Some(path.to_path_buf())
        } else if path.file_name() == staged.file_name() {
            path.parent().map(Path::to_path_buf)
        } else {
            None
        };
        if let Some(folder) = folder {
            keep_sharing(
                path.to_path_buf(),
                folder,
                again,
                options.torrent.wanted(),
                bytes,
                share,
                options.proxy_for(url),
                options.torrent.no_public_discovery,
            );
        }
    }
}
