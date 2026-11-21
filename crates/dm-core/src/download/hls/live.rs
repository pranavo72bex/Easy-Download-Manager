// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A live HLS stream recorded: its playlist read again and again, each track's new segments
//! fetched, and the tracks joined when it ends or is stopped.

use super::*;

use dm_media::sealed::{Lock, Locks};

/// A live stream recorded as it goes: its playlist asked for again at the
/// pace its segments come (half their length), each segment not yet
/// written fetched once, by its sequence number, decrypted where it is
/// scrambled, and added to one growing file, so what is recorded is never
/// more than a segment behind. Where the master keeps the sound in a
/// playlist of its own (`sound`), that playlist is recorded the same way
/// into a file of its own, asked for in the same rounds just after the
/// picture's, and the two are joined into one MP4 when the recording ends.
/// It ends when the picture's playlist says the stream has, or when the
/// person stops it, which keeps what was recorded rather than pausing: a
/// live stream cannot be resumed from where it was.
pub(super) async fn record_live(
    client: &reqwest::Client,
    playlist: &str,
    sound: Option<&str>,
    named_from: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    fs::create_dir_all(&options.parts_dir).await?;
    let sound_file = options.parts_dir.join("live.sound");
    let mut picture = LiveTrack::open(playlist, options.parts_dir.join("live.recording")).await?;
    let mut heard = match sound {
        Some(url) => Some(LiveTrack::open(url, sound_file.clone()).await?),
        None => None,
    };
    let downloaded = Arc::new(AtomicU64::new(0));
    let meter = report
        .clone()
        .map(|tx| watch(Arc::clone(&downloaded), None, tx));
    let mut filename = String::new();
    // Pieces of the picture the row has been told were missed.
    let mut told: u64 = 0;
    let ended = loop {
        let round = picture
            .round(client, named_from, options.quality, options, &downloaded)
            .await;
        // Pieces the playlist moved past before dm fetched them: the recording has a gap
        // there, which the row says (gaps row Y23).
        if picture.missed > told {
            told = picture.missed;
            if let Some(tx) = &report {
                let said = LIVE_PIECES_GONE.replace("{}", &told.to_string());
                let _ = tx.send(Update::Note(said)).await;
            }
        }
        let round = match round {
            Ok(round) => round,
            Err(e) => break Err(e),
        };
        if filename.is_empty() {
            if let Some(name) = &picture.named {
                filename = stream::titled(
                    name,
                    options.title_hint.as_deref(),
                    options.save_name.as_deref(),
                );
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
        /* The sound just after the picture, so it is taken at least as far
         * as the picture got, on the picture's last round too. A sound
         * playlist that fails five rounds running is left where it got to,
         * and the picture goes on without it. */
        if let Some(track) = heard.as_mut().filter(|t| !t.done) {
            match track
                .round(client, named_from, Quality::Best, options, &downloaded)
                .await
            {
                Ok(Round::Ended) => track.done = true,
                Ok(_) => {}
                Err(e @ (Error::Paused | Error::Cancelled)) => break Err(e),
                Err(_) => {
                    track.misses += 1;
                    track.done = track.misses > 5;
                }
            }
        }
        let pause = match round {
            Round::Ended => break Ok(()),
            // A playlist that did not answer is asked again shortly.
            Round::Missed => Duration::from_secs(2),
            Round::More(pause) => pause,
        };
        if let Err(stop) = or_stop(&options.control, async {
            tokio::time::sleep(pause).await;
            Ok(())
        })
        .await
        {
            break Err(stop);
        }
    };
    let flushed = picture.out.flush().await;
    let picture_at = picture.path.clone();
    let recorded = picture.written.is_some();
    let mut remapped = picture.remapped;
    let mut locks = [Locks(std::mem::take(&mut picture.locks)), Locks::default()];
    drop(picture);
    // The sound is joined only where some of it was written whole.
    let sound_at = match heard {
        Some(mut track) => {
            remapped |= track.remapped;
            locks[1] = Locks(std::mem::take(&mut track.locks));
            let whole = track.out.flush().await.is_ok();
            (whole && track.written.is_some()).then(|| track.path.clone())
        }
        None => None,
    };
    if let Some((handle, finished)) = meter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    steps::kept_or_cleared(ended, recorded, &picture_at, &sound_file).await?;
    flushed?;
    // Samples scrambled by SAMPLE-AES are put in the clear once the recording ends (gaps row Y16).
    if locks.iter().any(Locks::any) {
        let recording = live_sealed::Recorded {
            picture: picture_at,
            sound: sound_at,
            filename,
            remapped,
        };
        return live_sealed::place(recording, locks, options).await;
    }
    steps::place_live(picture_at, sound_at, filename, remapped, options).await
}

/// What one round of asking a live playlist came to.
enum Round {
    /// Taken as far as it goes; more is to come, worth asking for after
    /// this long.
    More(Duration),
    /// The playlist says the stream has ended: everything in it is taken.
    Ended,
    /// The playlist did not answer this time; it is asked again.
    Missed,
}

/// One playlist of a live stream being recorded: where it is asked for,
/// the file its segments are added to, and how far it has got.
struct LiveTrack {
    playlist: String,
    path: PathBuf,
    out: tokio::io::BufWriter<fs::File>,
    /// The sequence number of the last segment written.
    written: Option<u64>,
    /// The initialisation segment last written (its address and bytes), and whether one that
    /// changed was written after another: an advert break, made one MP4 when the recording
    /// ends (gaps row Y16).
    init_written: Option<(String, Option<(u64, u64)>)>,
    remapped: bool,
    /// Each key fetched once, by its address.
    keys: std::collections::HashMap<String, Vec<u8>>,
    /// Bytes written to the file so far, and the lock of each segment by where it starts in it:
    /// the key and vector of one whose samples are scrambled by SAMPLE-AES, `None` for the rest.
    at: u64,
    locks: Vec<(u64, Lock)>,
    /// Rounds in a row the playlist did not answer.
    misses: u32,
    /// Segments the playlist moved past before they were fetched.
    missed: u64,
    /// Asked for no more: its playlist ended, or stopped answering.
    done: bool,
    /// The name its first playlist gave the recording.
    named: Option<String>,
}

impl LiveTrack {
    async fn open(playlist: &str, path: PathBuf) -> Result<Self, Error> {
        let out = tokio::io::BufWriter::new(fs::File::create(&path).await?);
        Ok(LiveTrack {
            playlist: playlist.to_string(),
            path,
            out,
            written: None,
            init_written: None,
            remapped: false,
            keys: std::collections::HashMap::new(),
            at: 0,
            locks: Vec::new(),
            misses: 0,
            missed: 0,
            done: false,
            named: None,
        })
    }

    /// The playlist asked for once, and each segment in it not yet written
    /// fetched and added to the file. A playlist that does not answer once
    /// is a miss, asked again; one that stops answering (five misses), or
    /// fails before anything was recorded, is the error that ends it. A
    /// segment that cannot be had ends the round, and is asked for again
    /// with the playlist on the next.
    async fn round(
        &mut self,
        client: &reqwest::Client,
        named_from: &str,
        quality: Quality,
        options: &Options,
        downloaded: &AtomicU64,
    ) -> Result<Round, Error> {
        let (text, landed) = match fetch_playlist(client, &self.playlist, options).await {
            Ok(fetched) => fetched,
            Err(e @ (Error::Paused | Error::Cancelled)) => return Err(e),
            Err(_) if self.misses < 5 && self.written.is_some() => {
                self.misses += 1;
                return Ok(Round::Missed);
            }
            Err(e) => return Err(e),
        };
        self.misses = 0;
        let listed = stream::media_playlist(&text, &landed, named_from, quality)?;
        if self.named.is_none() {
            self.named = Some(listed.filename.clone());
        }
        // The initialisation segment of the segments that follow it, written before the first
        // of them that is, where it is not the one written last.
        let mut leading: Option<&stream::Part> = None;
        for part in &listed.parts {
            if part.init {
                leading = Some(part);
                continue;
            }
            if !self.written.is_none_or(|last| part.sequence > last) {
                continue;
            }
            if let Some(init) = leading
                .take()
                .filter(|init| self.init_written.as_ref() != Some(&(init.url.clone(), init.range)))
            {
                let Ok(body) = retry::run_up_to(&options.control, options.tries(), || {
                    fetch_segment_range(client, &init.url, init.range, options)
                })
                .await
                else {
                    break;
                };
                self.out.write_all(&body).await?;
                self.locks.push((self.at, None));
                self.at += body.len() as u64;
                downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
                self.remapped |= self.init_written.is_some();
                self.init_written = Some((init.url.clone(), init.range));
            }
            let Ok(body) = retry::run_up_to(&options.control, options.tries(), || {
                fetch_segment_range(client, &part.url, part.range, options)
            })
            .await
            else {
                break;
            };
            let mut lock = None;
            let bytes = match part.key.as_ref() {
                Some(declared) => {
                    if !self.keys.contains_key(&declared.uri) {
                        let key = fetch_capped(client, &declared.uri, 1024, options)
                            .await?
                            .body;
                        self.keys.insert(declared.uri.clone(), key);
                    }
                    let key = self
                        .keys
                        .get(&declared.uri)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let iv = stream::iv_for(declared, part.sequence);
                    if declared.sample {
                        // Written as it came; its samples are put in the clear when the
                        // recording ends, by where it starts in the file (gaps row Y16).
                        let key: [u8; 16] = key
                            .try_into()
                            .map_err(|_| Error::Internal("a stream key must be 16 bytes".into()))?;
                        lock = Some((key, iv));
                        body
                    } else {
                        stream::decrypt(key, &iv, &body)?
                    }
                }
                _ => body,
            };
            self.out.write_all(&bytes).await?;
            self.locks.push((self.at, lock));
            self.at += bytes.len() as u64;
            downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            if let Some(last) = self
                .written
                .filter(|last| part.sequence > last.saturating_add(1))
            {
                self.missed += part.sequence - last - 1;
            }
            self.written = Some(part.sequence);
        }
        self.out.flush().await?;
        if !stream::is_live(&text) {
            return Ok(Round::Ended);
        }
        // Half a segment's length, so a new one is not left waiting long.
        Ok(Round::More(Duration::from_millis(
            stream::target_duration(&text) * 500,
        )))
    }
}

/// A live picture and its sound, each recorded into a file of its own,
/// joined into one MP4 beside them: a transport stream with packed AAC by
/// the transport-stream muxer, fragmented MP4 by the MP4 muxer. With the
/// name the joined file is placed under. `None` where they cannot be
/// joined: the picture is then kept alone, as a recording cannot be asked
/// for again.
pub(super) async fn join_live(
    picture: &Path,
    sound: &Path,
    filename: &str,
    options: &Options,
) -> Option<(PathBuf, String)> {
    let joined = options.parts_dir.join("live.joined.mp4");
    let _ = fs::remove_file(&joined).await;
    let (picture_owned, sound_owned) = (picture.to_path_buf(), sound.to_path_buf());
    let made = if crate::tsmux::is_transport_stream(picture) && crate::tsmux::is_packed_aac(sound) {
        crate::tsmux::join_packed_async(picture_owned, sound_owned, joined.clone()).await
    } else if crate::tsmux::is_transport_stream(picture) && crate::tsmux::is_transport_stream(sound)
    {
        crate::tsmux::join_ts_async(picture_owned, sound_owned, joined.clone()).await
    } else {
        crate::mp4mux::join_async(vec![picture_owned, sound_owned], joined.clone()).await
    };
    let _ = fs::remove_file(sound).await;
    if made.is_ok() && fs::metadata(&joined).await.is_ok_and(|m| m.len() > 0) {
        let _ = fs::remove_file(picture).await;
        return Some((joined, format!("{}.mp4", media_stem(filename))));
    }
    let _ = fs::remove_file(&joined).await;
    None
}

/// A recording's name without its `.ts`, `.mp4` or `.m4a`, in any case.
pub(super) fn media_stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    for extension in [".ts", ".mp4", ".m4a"] {
        if lower.ends_with(extension) {
            return name.get(..name.len() - extension.len()).unwrap_or(name);
        }
    }
    name
}
