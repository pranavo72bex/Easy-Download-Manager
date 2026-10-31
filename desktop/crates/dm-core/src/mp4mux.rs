//! Joins a video's picture and sound into one MP4, by dm itself.
//!
//! A video site serves its better qualities as two files, the picture alone
//! and the sound alone, each an MP4 — fragmented, as DASH serves them, or
//! plain. yt-dlp fetches both and leaves the joining to a separate tool,
//! which a person would have to install (or dm fetch, 40–80 MB). This
//! does the joining itself: each input's samples are read from its own index
//! (the `moov` sample tables, or the `moof` fragments after an empty one),
//! and written as one plain MP4, the index first so it plays while it is
//! still being read, and picture and sound interleaved half a second at a
//! time. The samples themselves are copied byte for byte: nothing is
//! decoded or encoded, so it runs at the disk's speed.
//!
//! The inputs are read from files this computer was handed by a server, so
//! nothing here indexes past what it checked, and a file whose index points
//! outside it is refused rather than copied short.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The output's movie timescale: milliseconds, which every player reads.
const MOVIE_TIMESCALE: u32 = 1000;
/// How much of a track goes in one chunk before the other track's turn.
const CHUNK_SECONDS: u64 = 1;
/// The most read at once while copying samples.
const COPY_BUFFER: usize = 1 << 20;

/// One sample: where its bytes are in its input, and when it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub(crate) offset: u64,
    pub(crate) size: u32,
    pub(crate) duration: u32,
    /// Composition offset: when it shows, after it is decoded.
    pub(crate) cts: i32,
    pub(crate) sync: bool,
}

/// One track read out of an input, with the boxes copied to the output.
#[derive(Debug, Default)]
pub(crate) struct Track {
    pub(crate) input: usize,
    pub(crate) id: u32,
    /// The input's `tkhd` from its layer field on: volume, matrix, size.
    pub(crate) tkhd_tail: Vec<u8>,
    pub(crate) timescale: u32,
    pub(crate) language: u16,
    pub(crate) hdlr: Vec<u8>,
    /// `vmhd` for a picture, `smhd` for sound.
    pub(crate) media_header: Vec<u8>,
    pub(crate) stsd: Vec<u8>,
    /// The input's edit list and its movie timescale: what an AAC track
    /// uses to skip its encoder's priming samples.
    pub(crate) edits: Vec<Edit>,
    pub(crate) edits_timescale: u32,
    pub(crate) samples: Vec<Sample>,
    /// When its first sample plays, in its timescale, where a fragmented
    /// file says (`tfdt`): a live stream's sound may start after its
    /// picture.
    pub(crate) start: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Edit {
    pub(crate) duration: u64,
    pub(crate) media_time: i64,
}

/// A track's defaults from `trex`, for its fragments.
#[derive(Debug, Clone, Copy, Default)]
struct Defaults {
    description: u32,
    duration: u32,
    size: u32,
    flags: u32,
}

/// Joins `inputs`' tracks into one MP4 at `out`. Returns its length.
pub fn join(inputs: &[PathBuf], out: &Path) -> Result<u64, String> {
    join_with_text(inputs, &[], out)
}

/// [`join`], with `texts` carried as subtitle tracks, one per language.
pub fn join_with_text(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
) -> Result<u64, String> {
    join_cut(inputs, texts, out, None, None)
}

/// [`join_with_text`], cut to a part: `cut` is where the part starts and
/// ends, in milliseconds of the inputs' own timeline (a fragmented
/// stream's pieces say when they play, `tfdt`). The output holds the
/// pieces given and plays the part alone, by each track's edit list.
pub fn join_cut(
    inputs: &[PathBuf],
    texts: &[crate::subtitles::Subtitle],
    out: &Path,
    cut: Option<(u64, u64)>,
    shape: Option<crate::shape::Shape>,
) -> Result<u64, String> {
    let mut tracks = Vec::new();
    let mut files = Vec::new();
    for (index, path) in inputs.iter().enumerate() {
        let mut file =
            File::open(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        let length = file
            .metadata()
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?
            .len();
        // A WebM picture or sound (VP9, AV1, Opus) goes into the MP4 as is,
        // the way players expect: a video's tallest picture can come only
        // as WebM.
        let read = if crate::mkvmux::is_webm(path) {
            crate::mkvmux::mp4_tracks(path, index)
        } else {
            read_input(&mut file, length, index)
        }
        .map_err(|e| format!("{} is not an MP4 dm can join: {e}", path.display()))?;
        for track in &read {
            if let Some(end) = track
                .samples
                .iter()
                .map(|s| s.offset + u64::from(s.size))
                .max()
            {
                if end > length {
                    return Err(format!("{} is cut short", path.display()));
                }
            }
        }
        tracks.extend(read.into_iter().filter(|t| !t.samples.is_empty()));
        files.push(file);
    }
    if tracks.is_empty() {
        return Err("there is nothing to join".into());
    }
    // Vertical or square: the picture's crop, rewritten (`shape`).
    if let Some(shape) = shape {
        for track in &mut tracks {
            crate::shape::apply(track, shape)?;
        }
    }
    if texts.is_empty() {
        return write_output(&tracks, &mut files, out, cut);
    }
    // The subtitles' samples are written to a scratch file the writer copies
    // from, as it copies the inputs'.
    let mut scratch_name = out.as_os_str().to_owned();
    scratch_name.push(".text");
    let scratch = PathBuf::from(scratch_name);
    let written = (|| {
        let mut data = Vec::new();
        for text in texts {
            tracks.push(text_track(text, files.len(), &mut data));
        }
        std::fs::write(&scratch, &data).map_err(|e| format!("subtitles: {e}"))?;
        files.push(File::open(&scratch).map_err(|e| format!("subtitles: {e}"))?);
        write_output(&tracks, &mut files, out, cut)
    })();
    let _ = std::fs::remove_file(&scratch);
    written
}

/// [`join_cut`], off the async runtime.
pub async fn join_cut_async(
    inputs: Vec<PathBuf>,
    texts: Vec<crate::subtitles::Subtitle>,
    out: PathBuf,
    cut: Option<(u64, u64)>,
    shape: Option<crate::shape::Shape>,
) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_cut(&inputs, &texts, &out, cut, shape))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}

/// [`join`], off the async runtime.
pub async fn join_async(inputs: Vec<PathBuf>, out: PathBuf) -> Result<u64, String> {
    join_with_text_async(inputs, Vec::new(), out).await
}

/// [`join_with_text`], off the async runtime.
pub async fn join_with_text_async(
    inputs: Vec<PathBuf>,
    texts: Vec<crate::subtitles::Subtitle>,
    out: PathBuf,
) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || join_with_text(&inputs, &texts, &out))
        .await
        .map_err(|e| format!("joining stopped: {e}"))?
}

mod read;
mod text;
mod write;

use read::*;
use text::*;
pub(crate) use write::*;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal track: one sample description of `codec`, `samples` of
    /// `size` bytes each lasting `duration`.
    fn moov_of(id: u32, handler: &[u8; 4], timescale: u32, stbl: &[u8], mvex: bool) -> Vec<u8> {
        let mut tkhd = vec![0u8; 8];
        tkhd.extend_from_slice(&id.to_be_bytes());
        tkhd.extend_from_slice(&[0u8; 8]);
        tkhd.extend_from_slice(&[0u8; 60]);
        let mut mdhd = vec![0u8; 8];
        mdhd.extend_from_slice(&timescale.to_be_bytes());
        mdhd.extend_from_slice(&[0u8; 4]);
        mdhd.extend_from_slice(&0x55c4u16.to_be_bytes());
        mdhd.extend_from_slice(&[0u8; 2]);
        let mut hdlr = vec![0u8; 4];
        hdlr.extend_from_slice(handler);
        hdlr.extend_from_slice(&[0u8; 13]);
        let minf = boxed(b"minf", &[&full(b"smhd", 0, 0, &[0u8; 4]), stbl]);
        let mdia = boxed(
            b"mdia",
            &[
                &full(b"mdhd", 0, 0, &mdhd),
                &full(b"hdlr", 0, 0, &hdlr),
                &minf,
            ],
        );
        let trak = boxed(b"trak", &[&full(b"tkhd", 0, 3, &tkhd), &mdia]);
        let mut mvhd = vec![0u8; 8];
        mvhd.extend_from_slice(&1000u32.to_be_bytes());
        mvhd.extend_from_slice(&[0u8; 84]);
        let mut parts = vec![full(b"mvhd", 0, 0, &mvhd), trak];
        if mvex {
            let mut trex = id.to_be_bytes().to_vec();
            trex.extend_from_slice(&1u32.to_be_bytes());
            trex.extend_from_slice(&1024u32.to_be_bytes());
            trex.extend_from_slice(&0u32.to_be_bytes());
            trex.extend_from_slice(&0x0101_0000u32.to_be_bytes());
            parts.push(boxed(b"mvex", &[&full(b"trex", 0, 0, &trex)]));
        }
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        boxed(b"moov", &refs)
    }

    fn stsd() -> Vec<u8> {
        full(
            b"stsd",
            0,
            0,
            &[&1u32.to_be_bytes()[..], &boxed(b"test", &[&[0u8; 8]])].concat(),
        )
    }

    fn empty_stbl() -> Vec<u8> {
        let none = 0u32.to_be_bytes();
        boxed(
            b"stbl",
            &[
                &stsd(),
                &full(b"stts", 0, 0, &none),
                &full(b"stsc", 0, 0, &none),
                &full(b"stsz", 0, 0, &[0u8; 8]),
                &full(b"stco", 0, 0, &none),
            ],
        )
    }

    /// A fragmented file: an empty index, then fragments of three samples,
    /// the second sample of each not a sync sample, data counted from its
    /// moof. Sample n of the file is n bytes of the byte n.
    pub(crate) fn fragmented(dir: &Path, name: &str, fragments: u32) -> (PathBuf, Vec<Vec<u8>>) {
        fragmented_from(dir, name, fragments, 0)
    }

    /// [`fragmented`], its first fragment starting at `start` (`tfdt`,
    /// written only where that is not zero).
    pub(crate) fn fragmented_from(
        dir: &Path,
        name: &str,
        fragments: u32,
        start: u64,
    ) -> (PathBuf, Vec<Vec<u8>>) {
        let mut file = boxed(b"ftyp", &[b"iso5", &[0u8; 4]]);
        file.extend(moov_of(1, b"vide", 90000, &empty_stbl(), true));
        let mut samples = Vec::new();
        let mut n = 1u8;
        for f in 0..fragments {
            let sizes: Vec<u32> = (0..3).map(|i| u32::from(n) + i).collect();
            let mut trun_entries = Vec::new();
            for (i, size) in sizes.iter().enumerate() {
                trun_entries.extend_from_slice(&3000u32.to_be_bytes());
                trun_entries.extend_from_slice(&size.to_be_bytes());
                let flags: u32 = if i == 1 { 0x0101_0000 } else { 0x0200_0000 };
                trun_entries.extend_from_slice(&flags.to_be_bytes());
            }
            let build = |offset: u32| {
                let mfhd = full(b"mfhd", 0, 0, &(f + 1).to_be_bytes());
                let tfhd = full(b"tfhd", 0, 0x2_0000, &1u32.to_be_bytes());
                let decode_time = start + u64::from(f) * 9000;
                let tfdt = full(b"tfdt", 1, 0, &decode_time.to_be_bytes());
                let mut trun = 3u32.to_be_bytes().to_vec();
                trun.extend_from_slice(&offset.to_be_bytes());
                trun.extend_from_slice(&trun_entries);
                let traf = if start > 0 {
                    boxed(b"traf", &[&tfhd, &tfdt, &full(b"trun", 0, 0x701, &trun)])
                } else {
                    boxed(b"traf", &[&tfhd, &full(b"trun", 0, 0x701, &trun)])
                };
                boxed(b"moof", &[&mfhd, &traf])
            };
            let moof_len = build(0).len() as u32;
            file.extend(build(moof_len + 8));
            let mut data = Vec::new();
            for size in &sizes {
                let sample = vec![n; *size as usize];
                data.extend_from_slice(&sample);
                samples.push(sample);
                n += 1;
            }
            file.extend(boxed(b"mdat", &[&data]));
        }
        let path = dir.join(name);
        std::fs::write(&path, file).unwrap();
        (path, samples)
    }

    /// A plain file of `count` samples of 10 bytes, two per chunk.
    pub(crate) fn plain(dir: &Path, name: &str, count: u32, byte: u8) -> (PathBuf, Vec<Vec<u8>>) {
        let ftyp = boxed(b"ftyp", &[b"M4A ", &[0u8; 4]]);
        let samples: Vec<Vec<u8>> = (0..count)
            .map(|i| vec![byte.wrapping_add(i as u8); 10])
            .collect();
        let build = |data_at: u32| {
            let chunks = count.div_ceil(2);
            let mut stco = chunks.to_be_bytes().to_vec();
            for c in 0..chunks {
                stco.extend_from_slice(&(data_at + c * 20).to_be_bytes());
            }
            let mut stsz = 10u32.to_be_bytes().to_vec();
            stsz.extend_from_slice(&count.to_be_bytes());
            let mut stsc = 1u32.to_be_bytes().to_vec();
            stsc.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1]);
            let mut stts = 1u32.to_be_bytes().to_vec();
            stts.extend_from_slice(&count.to_be_bytes());
            stts.extend_from_slice(&1024u32.to_be_bytes());
            let stbl = boxed(
                b"stbl",
                &[
                    &stsd(),
                    &full(b"stts", 0, 0, &stts),
                    &full(b"stsc", 0, 0, &stsc),
                    &full(b"stsz", 0, 0, &stsz),
                    &full(b"stco", 0, 0, &stco),
                ],
            );
            moov_of(1, b"soun", 48000, &stbl, false)
        };
        let moov_len = build(0).len() as u32;
        let mut file = ftyp.clone();
        file.extend(build(ftyp.len() as u32 + moov_len + 8));
        file.extend(boxed(b"mdat", &[&samples.concat()]));
        let path = dir.join(name);
        std::fs::write(&path, file).unwrap();
        (path, samples)
    }

    fn read_back(path: &Path) -> Vec<Track> {
        let mut file = File::open(path).unwrap();
        let length = file.metadata().unwrap().len();
        read_input(&mut file, length, 0).unwrap()
    }

    fn bytes_of(path: &Path, track: &Track) -> Vec<Vec<u8>> {
        let data = std::fs::read(path).unwrap();
        track
            .samples
            .iter()
            .map(|s| data[s.offset as usize..(s.offset + u64::from(s.size)) as usize].to_vec())
            .collect()
    }

    #[test]
    fn a_fragmented_picture_and_a_plain_sound_join_sample_for_sample() {
        let dir = tempfile::tempdir().unwrap();
        let (video, pictures) = fragmented(dir.path(), "v.mp4", 4);
        let (audio, sounds) = plain(dir.path(), "a.m4a", 7, 200);
        let out = dir.path().join("joined.mp4");
        let length = join(&[video, audio], &out).unwrap();
        assert_eq!(length, std::fs::metadata(&out).unwrap().len());

        let data = std::fs::read(&out).unwrap();
        let kinds: Vec<[u8; 4]> = boxes(&data).map(|(k, _, _)| k).collect();
        assert_eq!(
            kinds,
            [*b"ftyp", *b"moov", *b"mdat"],
            "the index comes first"
        );

        let tracks = read_back(&out);
        assert_eq!(tracks.len(), 2);
        assert_eq!((tracks[0].id, tracks[1].id), (1, 2));
        assert_eq!(tracks[0].timescale, 90000);
        assert_eq!(tracks[1].timescale, 48000);
        assert_eq!(bytes_of(&out, &tracks[0]), pictures);
        assert_eq!(bytes_of(&out, &tracks[1]), sounds);
        // Timing and the sync samples survive: every second picture of a
        // fragment of three is not one.
        assert!(tracks[0].samples.iter().all(|s| s.duration == 3000));
        let syncs: Vec<bool> = tracks[0].samples.iter().map(|s| s.sync).collect();
        assert_eq!(syncs, [true, false, true].repeat(4));
        assert!(tracks[1]
            .samples
            .iter()
            .all(|s| s.duration == 1024 && s.sync));
    }

    #[test]
    fn a_cut_plays_the_part_alone_by_each_tracks_edit_list() {
        // Four fragments of three samples, each sample 3000 of 90000 a
        // second: 0.4 s in all. Cut to 0.15-0.35 s, the output holds every
        // sample and its edit list starts 150 ms in for 200 ms. The plain
        // sound (7 samples of 1024 at 48000: 0.149 s) ends before the cut
        // does, so its edit is what it has left of it.
        let dir = tempfile::tempdir().unwrap();
        let (video, pictures) = fragmented(dir.path(), "v.mp4", 4);
        let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
        let out = dir.path().join("part.mp4");
        join_cut(&[video, audio], &[], &out, Some((150, 350)), None).unwrap();
        let data = std::fs::read(&out).unwrap();
        let moov = read::child(&data, b"moov").unwrap();
        let edits: Vec<Vec<Edit>> = read::boxes(moov)
            .filter(|(k, _, _)| k == b"trak")
            .map(|(_, trak, _)| {
                let elst = read::child(read::child(trak, b"edts").unwrap(), b"elst").unwrap();
                read::read_edits(elst)
            })
            .collect();
        assert_eq!(edits.len(), 2);
        // The picture: 150 ms at 90000 is 13500; 200 ms at the movie's
        // 1000 is 200.
        assert_eq!(edits[0][0].media_time, 13500);
        assert_eq!(edits[0][0].duration, 200);
        // The sound: 150 ms at 48000 is 7200, and what it has after that
        // is nothing, which is written as the least an edit can be.
        assert_eq!(edits[1][0].media_time, 7200);
        assert!(edits[1][0].duration <= 1);
        let back = read_back(&out);
        assert_eq!(
            back[0].samples.len(),
            pictures.len(),
            "every sample is kept"
        );
        // A cut starting before a track that begins late waits for it:
        // the picture's fragments say they start at 0.2 s (tfdt).
        let (late, _) = fragmented_from(dir.path(), "late.mp4", 4, 18000);
        let out = dir.path().join("late.mp4.part");
        join_cut(&[late], &[], &out, Some((100, 400)), None).unwrap();
        let data = std::fs::read(&out).unwrap();
        let moov = read::child(&data, b"moov").unwrap();
        let trak = read::child(moov, b"trak").unwrap();
        let elst = read::child(read::child(trak, b"edts").unwrap(), b"elst").unwrap();
        let edits = read::read_edits(elst);
        assert_eq!(
            edits[0],
            Edit {
                duration: 100,
                media_time: -1
            },
            "waits 100 ms"
        );
        assert_eq!(edits[1].media_time, 0);
        assert_eq!(edits[1].duration, 200, "300 ms asked, less the wait");
    }

    #[test]
    fn an_index_pointing_past_the_end_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (audio, _) = plain(dir.path(), "a.m4a", 4, 1);
        let data = std::fs::read(&audio).unwrap();
        std::fs::write(&audio, &data[..data.len() - 5]).unwrap();
        let err = join(&[audio], &dir.path().join("out.mp4")).unwrap_err();
        assert!(err.contains("cut short"), "{err}");
    }

    #[test]
    fn what_is_not_an_mp4_is_refused_not_panicked_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("page.html");
        std::fs::write(&path, b"<!doctype html><title>no</title>").unwrap();
        assert!(join(&[path], &dir.path().join("out.mp4")).is_err());
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..600)) {
            let _ = boxes(&data).count();
            let _ = tracks_of(&data, 0);
            let _ = plain_samples(&data);
            let mut tracks = vec![Track { id: 1, timescale: 1, ..Track::default() }];
            let _ = read_fragment(0, &data, &[], &mut tracks);
        }
    }
}
