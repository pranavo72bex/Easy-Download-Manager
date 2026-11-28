// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::hls_text::{HlsTexts, Texts};
use super::in_place::tracks_in_place;
use super::*;
use crate::dash::{self, Medium, Track};

mod fetching;
mod joins;
mod sealed;
mod text;

use fetching::fetch_tracks;
#[cfg(test)]
use fetching::ONE_FILE_PIECE;

/// Fetches a DASH stream as one file.
///
/// The manifest is read by EDM (`dash`), one picture and one sound are chosen
/// by the quality asked for, each is fetched piece by piece into the parts
/// folder and gathered into a fragmented MP4 (or WebM) of its own, and the
/// two are joined by EDM's own muxer, copying the frames as they are. Like an
/// HLS stream there is no length and no position inside a piece to resume
/// from: pieces that arrived whole are kept and not fetched again.
pub(super) async fn download_dash(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let (text, landed) = hls::fetch_playlist(client, url, options).await?;
    // A live stream is recorded as it goes, its manifest read again and again.
    if dash::is_live(&text) {
        return super::dash_live::record(client, url, &landed, text, options, report).await;
    }
    let mut plan = dash::plan(
        &text,
        &landed,
        options.quality,
        options.container.as_deref(),
    )?;
    // Periods unlike the rest (an advert in another format) are left out, and the row says how
    // many (gaps row Y18).
    let left_out = plan.left_out + check_seams(client, &mut plan, options).await?;
    if left_out > 0 {
        if let Some(tx) = &report {
            let said = PARTS_LEFT_OUT.replace("{}", &left_out.to_string());
            let _ = tx.send(Update::Note(said)).await;
        }
    }

    // The best taken was below a quality passed over as copy-protected: the row says so.
    if plan.protected_above {
        if let Some(tx) = &report {
            let _ = tx.send(Update::Note(PROTECTED_ABOVE.into())).await;
        }
    }
    let tracks: Vec<&Track> = plan.video.iter().chain(plan.audio.iter()).collect();
    // WebM where it was asked for and every track is one; else an MP4,
    // which takes a WebM picture or sound too. Sound alone is an `.m4a`.
    let webm = options.container.as_deref() == Some("webm") && tracks.iter().all(|t| t.webm);
    let extension = match (&plan.video, webm) {
        (None, _) => "m4a",
        (Some(_), true) => "webm",
        (Some(_), false) => "mp4",
    };
    let filename = stream::titled(
        &stream::name_for(url, extension),
        options.title_hint.as_deref(),
        options.save_name.as_deref(),
    );
    let signature = format!("{url}\n{}", plan.signature());
    // Carried inside the file where Options says so (gaps row Y20); an `.m4a` has no picture to
    // show them over.
    let texts = if options.subtitles && plan.video.is_some() {
        text::fetch(client, &plan, options).await?
    } else {
        Texts::Timed(Vec::new())
    };
    join_tracks(
        client, &tracks, &filename, extension, webm, &signature, texts, options, report,
    )
    .await
}

/// A plan joined from several periods, checked before its pieces are fetched:
/// each period's initialisation piece is fetched, and the periods are joined
/// only where theirs are alike (the same tracks, timescales and sample
/// descriptions, `mp4mux::init_shape`), or the pieces after one would be read
/// with another's set-up. Where they are not, every track is cut to the
/// longest group of alike periods (by playing time; the earliest among
/// equals). Returns how many periods were cut off.
async fn check_seams(
    client: &reqwest::Client,
    plan: &mut dash::Plan,
    options: &Options,
) -> Result<usize, Error> {
    let joined: Vec<&Track> = plan
        .video
        .iter()
        .chain(plan.audio.iter())
        .filter(|t| !t.seams.is_empty())
        .collect();
    let Some(reference) = joined.first().copied() else {
        return Ok(0);
    };
    let periods = reference.seams.len() + 1;
    // `keys[k]`: what period k's initialisation pieces say, track by track; `None` (alike to
    // nothing) where one cannot be read.
    let mut keys: Vec<Option<Option<Vec<Vec<u8>>>>> = vec![Some(Some(Vec::new())); periods];
    for track in &joined {
        let inits = track
            .pieces
            .first()
            .into_iter()
            .chain(track.seams.iter().map(|s| &s.init));
        for (k, init) in inits.enumerate() {
            let body = retry::run_up_to(&options.control, options.tries(), || {
                hls::fetch_segment_range(client, &init.url, init.range, options)
            })
            .await?;
            match (keys.get_mut(k), crate::mp4mux::init_shape(&body)) {
                (Some(Some(Some(key))), Some(shape)) => key.push(shape),
                (Some(slot), None) => *slot = Some(None),
                _ => {}
            }
        }
    }
    // Each period's playing time, from the first joined track's pieces (or how many it has).
    let starts: Vec<usize> = std::iter::once(1)
        .chain(reference.seams.iter().map(|s| s.at))
        .chain(std::iter::once(reference.pieces.len()))
        .collect();
    let weights: Vec<f64> = (0..periods)
        .map(|k| {
            let pieces = match (starts.get(k), starts.get(k + 1)) {
                (Some(&from), Some(&to)) => reference.pieces.get(from..to).unwrap_or_default(),
                _ => &[],
            };
            let millis: u64 = pieces.iter().map(|p| p.millis).sum();
            if millis > 0 {
                millis as f64
            } else {
                pieces.len() as f64
            }
        })
        .collect();
    let keep = dash::heaviest(&keys, &weights);
    if keep.len() == periods {
        return Ok(0);
    }
    let cut = |track: &mut Option<Track>| -> Result<(), Error> {
        if let Some(t) = track.as_mut().filter(|t| !t.seams.is_empty()) {
            *t = dash::keep_periods(t, &keep).ok_or_else(|| {
                Error::Internal("a manifest's periods could not be joined".into())
            })?;
        }
        Ok(())
    };
    cut(&mut plan.video)?;
    cut(&mut plan.audio)?;
    Ok(periods - keep.len())
}

/// An HLS picture and sound kept in playlists of their own (fragmented MP4 on
/// both sides, or a transport-stream picture), fetched and joined as the two tracks of a
/// DASH stream are. `texts` are the stream's subtitles, carried inside (gaps row Y20).
pub(super) async fn download_pair(
    client: &reqwest::Client,
    picture: &stream::Stream,
    sound: &stream::Stream,
    rendition: &str,
    texts: HlsTexts,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    /* Each key once, by its address, and each segment given its own with the vector its own
     * number makes: a key may change part-way, as may the initialisation segment, which leads
     * the segments after it (each run read with its own, `mp4mux`'s runs). Samples scrambled by
     * SAMPLE-AES are put in the clear track by track (`sealed`, gaps row Y16). */
    let (picture_keys, sound_keys) = tokio::try_join!(
        hls::fetch_keys(client, picture, options),
        hls::fetch_keys(client, sound, options)
    )?;
    let tracks = [
        sealed::hls_track(Medium::Video, picture, &picture_keys),
        sealed::hls_track(Medium::Audio, sound, &sound_keys),
    ];
    // Runs, each with its own set-up, are joined by the MP4 writer, which keeps every one's.
    let runs = |s: &stream::Stream| s.parts.iter().filter(|p| p.init).count() > 1;
    let mp4_only;
    let options = if runs(picture) || runs(sound) {
        mp4_only = Options {
            container: None,
            ..options.clone()
        };
        &mp4_only
    } else {
        options
    };
    let refs: Vec<&Track> = tracks.iter().collect();
    let signature = format!("pair {rendition}");
    // A transport-stream picture is joined into an MP4 all the same.
    let name = if picture.container == "ts" {
        format!("{}.mp4", hls::ts_stem(&picture.filename))
    } else {
        picture.filename.clone()
    };
    // Vimeo's stream keeps its sound apart, so it is joined here (gaps row 214).
    let name = stream::titled(
        &name,
        options.title_hint.as_deref(),
        options.save_name.as_deref(),
    );
    // A part starts at the picture's first piece that overlaps it, which subtitles with no map
    // of their own are timed from.
    let texts = match options.span {
        Some(span) => texts.skipping(
            stream::cover(picture, span.from, span.to).map_or(0.0, |(_, starts, _)| starts),
        ),
        None => texts,
    };
    join_tracks(
        client,
        &refs,
        &name,
        "mp4",
        false,
        &signature,
        Texts::of_hls(texts),
        options,
        report,
    )
    .await
}

/// A picture and a sound a player fetched apart (two plain files, as a video
/// site serves them), fetched together and joined as the two tracks of a DASH
/// stream are. Each is read in pieces by byte range, so a server that does not
/// honour ranges is refused here rather than written in as the whole of a
/// piece.
pub(super) async fn download_media_pair(
    client: &reqwest::Client,
    picture: &str,
    sound: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    /// The size of one request. Large enough that setting a request up is
    /// small beside carrying it, small enough that a stop is heard promptly.
    const PIECE: u64 = 8 * 1024 * 1024;

    let track = |medium, url: &str, total: u64| Track {
        medium,
        id: String::new(),
        bandwidth: 0,
        height: 0,
        webm: query_of(url, "mime").is_some_and(|m| m.contains("webm")),
        indexed: None,
        seams: Vec::new(),
        pieces: pieces_of(url, total, PIECE),
    };
    let (picture_size, sound_size) = tokio::try_join!(
        total_of(client, picture, options),
        total_of(client, sound, options)
    )?;
    let tracks = [
        track(Medium::Video, picture, picture_size),
        track(Medium::Audio, sound, sound_size),
    ];
    let refs: Vec<&Track> = tracks.iter().collect();
    // The name a person gave, made an `.mp4` as what is joined is one.
    let filename = match options
        .save_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        Some(name) if name.to_ascii_lowercase().ends_with(".mp4") => name.to_string(),
        Some(name) => format!("{name}.mp4"),
        None => "video.mp4".to_string(),
    };
    let signature = format!("media {picture}\n{sound}");
    // `finish` places the file under the chosen name as it stands.
    let options = &Options {
        save_name: Some(filename.clone()),
        ..options.clone()
    };
    join_tracks(
        client,
        &refs,
        &filename,
        "mp4",
        false,
        &signature,
        Texts::Timed(Vec::new()),
        options,
        report,
    )
    .await
}

/// A stream served only as numbered pieces, as YouTube serves some of a video's formats
/// (`FORMAT_STREAM_TYPE_OTF`, gaps row Y14): `&sq=0` is its first boxes and says how many
/// pieces follow it (`Segment-Count: N`), and `&sq=1` to `&sq=N` are those pieces. Every piece
/// is fetched as a DASH track's are, on as many connections as are set, kept when whole across
/// a stop, and gathered in order into one file, placed under `options.save_name` (its
/// extension says picture or sound, MP4 or WebM).
pub(super) async fn download_numbered(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let name = options
        .save_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("video.mp4")
        .to_string();
    let ext = Path::new(&name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    // Where the first piece cannot be had or does not say how many follow, the caller falls
    // back to the video's other formats (`NUMBERED_UNREAD`); a stop is a stop.
    let first =
        match fetch_capped(client, &numbered_address(url, 0), FIRST_PIECE_MOST, options).await {
            Ok(first) => first,
            Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
            Err(why) => return Err(numbered_unread(&why.to_string())),
        };
    let Some(count) = segment_count(&first.body) else {
        return Err(numbered_unread(
            "its first piece does not say how many follow",
        ));
    };
    let track = Track {
        medium: if ext == "m4a" {
            Medium::Audio
        } else {
            Medium::Video
        },
        id: String::new(),
        bandwidth: 0,
        height: 0,
        webm: ext == "webm",
        indexed: None,
        seams: Vec::new(),
        pieces: (0..=count)
            .map(|n| dash::Piece {
                url: numbered_address(url, n),
                range: None,
                lock: None,
                millis: 0,
            })
            .collect(),
    };
    // The pieces are kept for this stream's count alone; its address changes when it is read
    // again, its pieces do not.
    let signature = format!("numbered {count}");
    fs::create_dir_all(&options.parts_dir).await?;
    hls::keep_only_parts_of(&options.parts_dir, &signature).await?;
    fs::create_dir_all(&options.dest_dir).await?;
    let downloaded = Arc::new(AtomicU64::new(0));
    let meter = report.map(|tx| {
        crate::download::ranged::watch_estimating(Arc::clone(&downloaded), None, None, tx)
    });
    let fetched = fetch_tracks(client, &[&track], options, &downloaded).await;
    if let Some((handle, finished)) = meter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    let files = match fetched {
        Ok(files) => files,
        Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
        // Nothing after the first piece was whole: the other formats are fetched instead.
        Err(why) if !any_numbered_piece(&options.parts_dir).await => {
            return Err(numbered_unread(&why.to_string()))
        }
        Err(why) => return Err(why),
    };
    options.control.check()?;
    let (path, bytes, category) = finish(&name, None, &files, options).await?;
    Ok(Finished {
        left_behind: None,
        disguised: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// [`NUMBERED_UNREAD`], with why noted in the log.
fn numbered_unread(why: &str) -> Error {
    crate::note!(
        warn,
        "EDM: a stream's numbered pieces could not be read ({why}); its other formats are fetched"
    );
    Error::Internal(NUMBERED_UNREAD.into())
}

/// Whether any numbered piece after the first is whole in `parts_dir` (its part file, named
/// when whole).
async fn any_numbered_piece(parts_dir: &Path) -> bool {
    let first = part_path(parts_dir, 0);
    let Ok(mut entries) = fs::read_dir(parts_dir).await else {
        return false;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("part-") && !name.ends_with(".partial") && entry.path() != first {
            return true;
        }
    }
    false
}

/// The most the first of a stream's numbered pieces is believed to be: its first boxes and
/// the words on how many pieces follow are kilobytes.
const FIRST_PIECE_MOST: usize = 4 * 1024 * 1024;

/// The most numbered pieces a stream is believed to have: a piece is seconds long, and this
/// is days of video.
const NUMBERED_MOST: u64 = 200_000;

/// `url` asking for its piece `n` (`&sq=n`), the rest of the address as it came.
pub(crate) fn numbered_address(url: &str, n: u64) -> String {
    let joint = if url.contains('?') { '&' } else { '?' };
    format!("{url}{joint}sq={n}")
}

/// How many numbered pieces follow the first, as it says (`Segment-Count: N`); `None` where it
/// does not say, or names none or more than [`NUMBERED_MOST`].
pub(crate) fn segment_count(first: &[u8]) -> Option<u64> {
    const SAID: &[u8] = b"Segment-Count:";
    let at = first.windows(SAID.len()).position(|w| w == SAID)? + SAID.len();
    let rest = &first[at..];
    let digits: String = rest
        .iter()
        .skip_while(|b| **b == b' ')
        .take_while(|b| b.is_ascii_digit())
        .take(10)
        .map(|b| char::from(*b))
        .collect();
    digits
        .parse::<u64>()
        .ok()
        .filter(|n| (1..=NUMBERED_MOST).contains(n))
}

/// The value of one query parameter of a URL, where it has it.
pub(crate) fn query_of(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.replace("%2F", "/").replace("%2f", "/"))
    })
}

/// `total` bytes cut into requests of at most `piece` bytes each. A file of
/// no known length (0) is one request with no range.
pub(crate) fn pieces_of(url: &str, total: u64, piece: u64) -> Vec<dash::Piece> {
    if total == 0 {
        return vec![dash::Piece {
            url: url.to_string(),
            range: None,
            lock: None,
            millis: 0,
        }];
    }
    let mut pieces = Vec::new();
    let mut first = 0;
    while first < total {
        let last = (first + piece).min(total) - 1;
        pieces.push(dash::Piece {
            url: url.to_string(),
            range: Some((first, last)),
            lock: None,
            millis: 0,
        });
        first = last + 1;
    }
    pieces
}

/// How long a file is, asked of the server by a one-byte range: the length
/// after the slash of `Content-Range`. A server that answers the whole file
/// does not do ranges, and is refused rather than fetched twice as often.
async fn total_of(client: &reqwest::Client, url: &str, options: &Options) -> Result<u64, Error> {
    let request = get(
        client,
        url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    )
    .header(reqwest::header::RANGE, "bytes=0-0");
    let res = or_stop(&options.control, crate::auth::send(client, request, url)).await?;
    let res = Error::refused_or(res).await?;
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(Error::Internal(format!(
            "{url} does not serve byte ranges, which a picture and sound fetched apart need"
        )));
    }
    res.headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|n| n.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| Error::Internal(format!("{url} did not say how long it is")))
        .and_then(|n| {
            // The file is cut into a list of requests by this number before
            // a byte arrives: one past any real picture or sound is refused.
            if n > TRACK_MOST {
                return Err(Error::Internal(format!(
                    "{url} says it is {n} bytes long, more than a video's picture or sound is"
                )));
            }
            Ok(n)
        })
}

/// The longest picture or sound fetched apart that is believed: 1 TiB.
const TRACK_MOST: u64 = 1 << 40;

/// The tracks fetched, gathered one file each, and joined into the file
/// named `filename`, placed where downloads go. `signature` says what is
/// being fetched: parts staged for something else are not adopted. `texts`
/// are carried as subtitle tracks where an MP4 or a Matroska file is made;
/// an HLS stream's (gaps row Y20) by the joins of transport streams too.
#[allow(clippy::too_many_arguments)]
async fn join_tracks(
    client: &reqwest::Client,
    tracks: &[&Track],
    filename: &str,
    extension: &str,
    webm: bool,
    signature: &str,
    texts: Texts,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let mut filename = filename.to_string();
    let mut signature = signature.to_string();
    /* A part of the stream: each track reduced to the pieces that overlap the
     * span (a one-file track is cut by its own index when it is fetched), and
     * the joined file trimmed exactly by the muxer's edit lists. */
    let cut_tracks: Vec<Track>;
    let tracks: Vec<&Track> = match options.span {
        Some(span) => {
            let mut reduced = Vec::new();
            for track in tracks {
                if track.indexed.is_some() {
                    reduced.push((*track).clone());
                } else {
                    let (cut, _, _) = dash::cut(track, span.from, span.to).ok_or_else(|| {
                        Error::Internal(
                            "that part is outside the stream, or the stream does not say how                              long its pieces are"
                                .to_string(),
                        )
                    })?;
                    reduced.push(cut);
                }
            }
            cut_tracks = reduced;
            signature = format!(
                "{signature}
{}",
                span.words()
            );
            filename = hls::named_for_span(&filename, span);
            cut_tracks.iter().collect()
        }
        None => tracks.to_vec(),
    };
    let tracks = tracks.as_slice();
    let signature = signature.as_str();
    fs::create_dir_all(&options.parts_dir).await?;
    hls::keep_only_parts_of(&options.parts_dir, signature).await?;

    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: filename.clone(),
                category: category::classify(&filename, None),
            })
            .await;
    }
    fs::create_dir_all(&options.dest_dir).await?;

    // Joined before a stop: that file is whole (it is named only when it is),
    // so it is placed and nothing is fetched again.
    let mut joined = options.parts_dir.join(format!("joined.{extension}"));
    // A picture and sound joined into a Matroska file instead of an MP4
    // (below) are found under that name.
    let matroska = options.parts_dir.join("joined.mkv");
    if extension == "mp4" && fs::metadata(&matroska).await.is_ok_and(|m| m.len() > 0) {
        joined = matroska.clone();
    }
    if !fs::metadata(&joined).await.is_ok_and(|m| m.len() > 0) {
        let files = joins::fetch_tracks_shown(client, tracks, options, report).await?;

        // Matroska where it was asked for, or where the picture and sound
        // are a pair no MP4 holds (VP8 or Vorbis beside an MP4's samples),
        // rather than refused. A part or a reshaped picture is made by the
        // MP4 writer alone, as before, and so is an HLS transport stream
        // with its sound kept apart.
        let streams = files
            .iter()
            .any(|f| crate::tsmux::is_transport_stream(f) || crate::tsmux::is_packed_aac(f));
        let mkv = extension == "mp4"
            && options.span.is_none()
            && options.shape.is_none()
            && !streams
            && (options.container.as_deref() == Some("mkv")
                || crate::mkvmux::needs_matroska(&files));
        if mkv {
            joined = matroska.clone();
        }
        // No room for the join beside its tracks: an MP4 is written into the largest of them
        // (gaps row Y21); anything else is refused in the words of the check before fetching,
        // the tracks kept for when there is room.
        if let Err(no_room) = room_to_join(&files, &options.parts_dir, options) {
            if webm || mkv || streams || !tracks_in_place(&files, texts, &joined, options).await? {
                return Err(no_room);
            }
        } else {
            joins::join_beside(&files, texts, &joined, webm || mkv, options).await?;
        }
    }
    // Saved under the name of what it is.
    if joined == matroska {
        filename = Path::new(&filename)
            .with_extension("mkv")
            .to_string_lossy()
            .into_owned();
    }

    let (path, bytes, category) = finish(&filename, None, &[joined], options).await?;
    Ok(Finished {
        left_behind: None,
        disguised: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// The tracks joined into one MP4 with `texts` as its subtitles, cut to the span where there is
/// one. An HLS stream's are timed from where the tracks start (`tfdt`, gaps row Y20).
async fn join_mp4(
    files: Vec<PathBuf>,
    texts: Texts,
    out: PathBuf,
    options: &Options,
) -> Result<u64, String> {
    let texts = match texts {
        Texts::Timed(texts) => texts,
        Texts::Hls(texts) => {
            let (cut, shape) = match options.span {
                Some(span) => (Some(span.millis()), options.shape),
                None => (None, None),
            };
            let place = move |zero| texts.timed(zero);
            return crate::mp4mux::join_cut_placed_async(files, place, out, cut, shape).await;
        }
    };
    match options.span {
        Some(span) => {
            crate::mp4mux::join_cut_async(files, texts, out, Some(span.millis()), options.shape)
                .await
        }
        None => crate::mp4mux::join_with_text_async(files, texts, out).await,
    }
}

/// The tracks joined into one Matroska or WebM file with `texts` as its subtitles; an HLS
/// stream's timed from where the tracks start (`tfdt`), as [`join_mp4`] times them (gaps row
/// Y20).
async fn join_mkv(files: Vec<PathBuf>, texts: Texts, out: PathBuf) -> Result<u64, String> {
    match texts {
        Texts::Timed(texts) => crate::mkvmux::join_with_text_async(files, texts, out).await,
        Texts::Hls(texts) => {
            let place = move |zero| texts.timed(zero);
            crate::mkvmux::join_placed_async(files, place, out).await
        }
    }
}

/// The tracks' size: exact where every piece names its bytes, else about, from each track's
/// bitrate and the pieces' playing time; `(None, None)` where neither is said.
fn size_of_tracks(tracks: &[&crate::dash::Track]) -> (Option<u64>, Option<u64>) {
    let ranged = tracks
        .iter()
        .all(|t| t.pieces.iter().all(|p| p.range.is_some()));
    if ranged {
        let bytes = tracks
            .iter()
            .flat_map(|t| &t.pieces)
            .filter_map(|p| p.range)
            .map(|(first, last)| last.saturating_sub(first) + 1)
            .sum::<u64>();
        return ((bytes > 0).then_some(bytes), None);
    }
    let about: u64 = tracks
        .iter()
        .map(|t| {
            let millis: u64 = t.pieces.iter().map(|p| p.millis).sum();
            t.bandwidth / 8 * millis / 1000
        })
        .sum();
    (None, (about > 0).then_some(about))
}

#[cfg(test)]
pub(super) mod tests;
