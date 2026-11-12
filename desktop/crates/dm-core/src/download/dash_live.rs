// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A live DASH stream (`type="dynamic"`), recorded as it goes, as a live HLS
//! stream is (`hls::record_live`).
//!
//! The manifest is read again every `minimumUpdatePeriod` (kept between half
//! a second and half a minute), and each piece it lists that was not taken
//! yet is fetched once and added to its track's growing file: one for the
//! picture, one for the sound. A period whose initialisation piece is unlike
//! the first one's (an advert in another format) is passed over, and the row
//! says so. The recording ends when the manifest turns static (the stream
//! has ended: what it lists then is all there is) or when the person stops
//! it, which keeps what was recorded rather than pausing, as a live stream
//! cannot be resumed from where it was. The tracks are then joined into one
//! file by dm's own muxer.

use std::collections::{HashMap, HashSet, VecDeque};

use super::*;
use crate::dash::{self, LiveTrack, Medium};

/// One track of the stream as it is recorded.
struct Recorder {
    medium: Medium,
    /// The file it grows in.
    path: PathBuf,
    out: tokio::io::BufWriter<fs::File>,
    /// What the first initialisation piece said (`shape_of`); `None` before one.
    first: Option<Vec<u8>>,
    /// Each initialisation piece met, by address, and whether its period is recorded.
    inits: HashMap<String, bool>,
    /// The pieces taken or passed over, the newest [`SEEN_MOST`] of them: a manifest that lists
    /// one again (a stream that ends may list all it had) does not have it recorded twice.
    seen: HashSet<PieceKey>,
    /// `seen` in the order taken, the oldest forgotten first.
    seen_order: VecDeque<PieceKey>,
    /// The last piece taken or passed over: one listed before it is older, and is not taken
    /// after it (a stream that ends may list pieces from before dm began).
    last: Option<PieceKey>,
    /// Whether a piece of media was written.
    wrote: bool,
}

/// A piece by its address and bytes.
type PieceKey = (String, Option<(u64, u64)>);

/// The most pieces a track remembers taking: a day of two-second pieces, far more than a
/// manifest keeps listed, while a stream recorded for days does not keep every address.
const SEEN_MOST: usize = 50_000;

fn key_of(piece: &dash::Piece) -> PieceKey {
    (piece.url.clone(), piece.range)
}

/// What an initialisation piece says that a later one must say alike: an MP4's
/// tracks and sample descriptions, else (a WebM, or one dm cannot read) its bytes.
fn shape_of(body: &[u8]) -> Vec<u8> {
    crate::mp4mux::init_shape(body).unwrap_or_else(|| body.to_vec())
}

impl Recorder {
    fn track_in<'a>(&self, period: &'a dash::LivePeriod) -> Option<&'a LiveTrack> {
        match self.medium {
            Medium::Video => period.video.as_ref(),
            Medium::Audio => period.audio.as_ref(),
        }
    }

    /// Whether `track`'s period is recorded: its initialisation piece fetched the
    /// first time it is met, written where it is the first, and compared with the
    /// first otherwise. A track with none is taken as it comes.
    async fn admits(
        &mut self,
        client: &reqwest::Client,
        track: &LiveTrack,
        options: &Options,
        downloaded: &AtomicU64,
    ) -> Result<bool, Error> {
        let Some(init) = &track.init else {
            return Ok(true);
        };
        if let Some(known) = self.inits.get(&init.url) {
            return Ok(*known);
        }
        let body = retry::run_up_to(&options.control, options.tries(), || {
            fetch_segment_range(client, &init.url, init.range, options)
        })
        .await?;
        let shape = shape_of(&body);
        let admitted = match &self.first {
            None => {
                self.out.write_all(&body).await?;
                downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
                self.first = Some(shape);
                true
            }
            Some(first) => *first == shape,
        };
        self.inits.insert(init.url.clone(), admitted);
        Ok(admitted)
    }

    /// `track`'s pieces not taken yet: fetched and written in order where its
    /// period is recorded, marked as passed over where it is not.
    async fn take(
        &mut self,
        client: &reqwest::Client,
        track: &LiveTrack,
        recorded: bool,
        options: &Options,
        downloaded: &AtomicU64,
    ) -> Result<(), Error> {
        for piece in &track.pieces {
            let key = key_of(piece);
            if self.seen.contains(&key) {
                continue;
            }
            if recorded {
                let body = retry::run_up_to(&options.control, options.tries(), || {
                    fetch_segment_range(client, &piece.url, piece.range, options)
                })
                .await?;
                self.out.write_all(&body).await?;
                downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
                self.wrote = true;
            }
            self.last = Some(key.clone());
            self.remember(key);
        }
        Ok(())
    }

    /// Passes over the pieces `listing` lists before the last one taken: they are older than
    /// what is recorded, and would play out of order after it.
    fn pass_over_older(&mut self, listing: &dash::Live) {
        let listed: Vec<PieceKey> = listing
            .periods
            .iter()
            .filter_map(|p| self.track_in(p))
            .flat_map(|t| t.pieces.iter().map(key_of))
            .collect();
        let Some(at) = self
            .last
            .as_ref()
            .and_then(|last| listed.iter().position(|k| k == last))
        else {
            return;
        };
        for key in listed.into_iter().take(at) {
            self.remember(key);
        }
    }

    /// Notes a piece as taken, forgetting the oldest past [`SEEN_MOST`].
    fn remember(&mut self, key: PieceKey) {
        if self.seen.insert(key.clone()) {
            self.seen_order.push_back(key);
        }
        while self.seen_order.len() > SEEN_MOST {
            if let Some(oldest) = self.seen_order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
    }
}

/// The file's extension for the tracks the stream has: a WebM where it was asked for and every
/// track is one, sound alone an `.m4a`, otherwise an MP4.
fn extension_for(period: &dash::LivePeriod, options: &Options) -> &'static str {
    let tracks: Vec<&LiveTrack> = period.video.iter().chain(period.audio.iter()).collect();
    if options.container.as_deref() == Some("webm") && tracks.iter().all(|t| t.webm) {
        "webm"
    } else if period.video.is_none() {
        "m4a"
    } else {
        "mp4"
    }
}

/// How many times a stream that has ended is read again for pieces that failed to come.
const LAST_TRIES: u32 = 3;

/// Records the live stream whose manifest `first_text` was read from `manifest`, named after
/// `named_from`. See the module's words.
pub(super) async fn record(
    client: &reqwest::Client,
    named_from: &str,
    manifest: &str,
    first_text: String,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    fs::create_dir_all(&options.parts_dir).await?;
    let downloaded = Arc::new(AtomicU64::new(0));
    let meter = report
        .clone()
        .map(|tx| watch(Arc::clone(&downloaded), None, tx));
    let mut recorders: Vec<Recorder> = Vec::new();
    let mut filename = String::new();
    let mut extension = "mp4";
    // The periods passed over, by their initialisation pieces' addresses.
    let mut passed_over: HashSet<Vec<String>> = HashSet::new();
    let mut text = Some(first_text);
    let mut landed = manifest.to_string();
    let (mut misses, mut last_tries) = (0u32, 0u32);
    let ended: Result<(), Error> = 'rounds: loop {
        let body = match text.take() {
            Some(body) => body,
            None => match fetch_playlist(client, manifest, options).await {
                Ok((body, at)) => {
                    misses = 0;
                    landed = at;
                    body
                }
                Err(e @ (Error::Paused | Error::Cancelled)) => break Err(e),
                // A manifest that does not answer once is asked again; one that stops
                // answering ends the recording with what it has.
                Err(_) if misses < 5 && recorders.iter().any(|r| r.wrote) => {
                    misses += 1;
                    if let Err(stop) = or_stop(&options.control, async {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        Ok(())
                    })
                    .await
                    {
                        break Err(stop);
                    }
                    continue;
                }
                Err(e) => break Err(e),
            },
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let listing = match dash::live(
            &body,
            &landed,
            options.quality,
            options.container.as_deref(),
            now,
        ) {
            Ok(listing) => listing,
            Err(e) => break Err(e),
        };
        // The tracks and the name, from the first period that has any.
        if recorders.is_empty() {
            if let Some(period) = listing
                .periods
                .iter()
                .find(|p| p.video.is_some() || p.audio.is_some())
            {
                extension = extension_for(period, options);
                for (medium, present, name) in [
                    (
                        Medium::Video,
                        period.video.is_some(),
                        "live-picture.recording",
                    ),
                    (
                        Medium::Audio,
                        period.audio.is_some(),
                        "live-sound.recording",
                    ),
                ] {
                    if !present {
                        continue;
                    }
                    let path = options.parts_dir.join(name);
                    recorders.push(Recorder {
                        medium,
                        out: tokio::io::BufWriter::new(fs::File::create(&path).await?),
                        path,
                        first: None,
                        inits: HashMap::new(),
                        seen: HashSet::new(),
                        seen_order: VecDeque::new(),
                        last: None,
                        wrote: false,
                    });
                }
                filename = stream::name_for(named_from, extension);
                if let Some(tx) = &report {
                    let _ = tx
                        .send(Update::Named {
                            name: filename.clone(),
                            category: category::classify(&filename, None),
                        })
                        .await;
                }
            }
        }
        // Each period's new pieces, in order. A failure leaves the rest for the next reading.
        for recorder in recorders.iter_mut() {
            recorder.pass_over_older(&listing);
        }
        let mut failed = false;
        'periods: for period in &listing.periods {
            let mut recorded = true;
            for recorder in recorders.iter_mut() {
                let Some(track) = recorder.track_in(period) else {
                    continue;
                };
                match recorder.admits(client, track, options, &downloaded).await {
                    Ok(admitted) => recorded &= admitted,
                    Err(e @ (Error::Paused | Error::Cancelled)) => break 'rounds Err(e),
                    Err(_) => {
                        failed = true;
                        break 'periods;
                    }
                }
            }
            if !recorded {
                let inits: Vec<String> = recorders
                    .iter()
                    .filter_map(|r| r.track_in(period))
                    .filter_map(|t| t.init.as_ref().map(|i| i.url.clone()))
                    .collect();
                if passed_over.insert(inits) {
                    if let Some(tx) = &report {
                        let said =
                            crate::download::NOTES[2].replace("{}", &passed_over.len().to_string());
                        let _ = tx.send(Update::Note(said)).await;
                    }
                }
            }
            for recorder in recorders.iter_mut() {
                let Some(track) = recorder.track_in(period) else {
                    continue;
                };
                match recorder
                    .take(client, track, recorded, options, &downloaded)
                    .await
                {
                    Ok(()) => {}
                    Err(e @ (Error::Paused | Error::Cancelled)) => break 'rounds Err(e),
                    Err(_) => {
                        failed = true;
                        break 'periods;
                    }
                }
            }
        }
        for recorder in recorders.iter_mut() {
            recorder.out.flush().await?;
        }
        if !listing.live {
            // Ended: what it lists is all there is, the pieces that failed asked for again a
            // few times.
            if !failed || last_tries >= LAST_TRIES {
                break Ok(());
            }
            last_tries += 1;
        }
        if let Err(stop) = or_stop(&options.control, async {
            tokio::time::sleep(Duration::from_secs_f64(listing.every)).await;
            Ok(())
        })
        .await
        {
            break Err(stop);
        }
    };
    for recorder in recorders.iter_mut() {
        recorder.out.flush().await?;
    }
    if let Some((handle, finished)) = meter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    let wrote = recorders.iter().any(|r| r.wrote);
    match ended {
        Ok(()) | Err(Error::Paused) => {}
        Err(Error::Cancelled) => {
            for recorder in &recorders {
                let _ = fs::remove_file(&recorder.path).await;
            }
            return Err(Error::Cancelled);
        }
        // Whatever stopped it, what was recorded is kept where there is any.
        Err(e) if !wrote => return Err(e),
        Err(_) => {}
    }
    if !wrote {
        return Err(Error::Internal(
            "the live stream sent nothing to record".into(),
        ));
    }
    let files: Vec<PathBuf> = recorders
        .iter()
        .filter(|r| r.wrote)
        .map(|r| r.path.clone())
        .collect();
    drop(recorders);
    let joined = options.parts_dir.join(format!("joined.{extension}"));
    let partial = options
        .parts_dir
        .join(format!("joined.partial.{extension}"));
    let _ = fs::remove_file(&partial).await;
    let made = if extension == "webm" {
        crate::mkvmux::join_async(files.clone(), partial.clone()).await
    } else {
        crate::mp4mux::join_async(files.clone(), partial.clone()).await
    };
    if let Err(why) = made {
        let _ = fs::remove_file(&partial).await;
        return Err(Error::Internal(format!(
            "the live recording could not be joined: {why}"
        )));
    }
    fs::rename(&partial, &joined).await?;
    for file in files {
        let _ = fs::remove_file(file).await;
    }
    // Stopped by the person: the recording is finished, not paused, so placing it must not wait
    // on the stop that ended it.
    let placing = Options {
        control: crate::control::Control::new(),
        ..options.clone()
    };
    let (path, bytes, category) = finish(&filename, None, &[joined], &placing).await?;
    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: false,
        mirrors: 1,
    })
}

#[cfg(test)]
mod tests;
