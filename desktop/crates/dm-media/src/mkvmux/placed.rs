// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A join whose subtitles are timed once its inputs are read (gaps row Y20): an HLS stream's
//! WebVTT segments say where their cues sit on the picture's 90 kHz clock, and the file starts
//! where its earliest input does on that clock, which only the reading finds (`line_up`), as
//! the MP4 writer's `join_cut_placed` finds it.

use super::*;

/// [`join_with_text`], with the subtitles timed once the inputs are read: `place` is given when
/// the output starts on the inputs' own timeline, in 90 kHz ticks (the earliest first time an
/// input says, `tfdt`, as the inputs are lined up; 0 where none says), and gives back the
/// subtitles timed from there, carried as subtitle tracks, one per language.
pub fn join_placed<F>(inputs: &[PathBuf], place: F, out: &Path) -> Result<u64, String>
where
    F: FnOnce(i64) -> Vec<crate::subtitles::Subtitle>,
{
    join_tagged_placed(inputs, place, out, &crate::mp4mux::Tags::default())
}

/// [`join_tagged`], its subtitles timed as [`join_placed`] times them.
pub(super) fn join_tagged_placed<F>(
    inputs: &[PathBuf],
    place: F,
    out: &Path,
    tags: &crate::mp4mux::Tags,
) -> Result<u64, String>
where
    F: FnOnce(i64) -> Vec<crate::subtitles::Subtitle>,
{
    let mut read = Vec::new();
    let mut webm = Vec::new();
    for (i, path) in inputs.iter().enumerate() {
        webm.push(is_webm(path));
        let input = if webm.last() == Some(&true) {
            read_input(path, i)
                .map_err(|e| format!("{} is not a WebM dm can join: {e}", path.display()))?
        } else {
            from_mp4::read(path, i)
                .map_err(|e| format!("{} is not an MP4 dm can join: {e}", path.display()))?
        };
        let length = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        let past = input.blocks.iter().any(|b| {
            b.data_at
                .checked_add(b.data_len)
                .is_none_or(|end| end > length)
        });
        if past {
            return Err(format!("{} is cut short", path.display()));
        }
        read.push(input);
    }
    line_up(&mut read, &webm);
    let texts = place(zero_of(&read));
    let mut scratch_name = out.as_os_str().to_owned();
    scratch_name.push(".text");
    let scratch = PathBuf::from(scratch_name);
    let mut sources = inputs.to_vec();
    if !texts.is_empty() {
        let mut data = Vec::new();
        read.push(text_input(&texts, inputs.len(), &mut data));
        std::fs::write(&scratch, &data).map_err(|e| format!("subtitles: {e}"))?;
        sources.push(scratch.clone());
    }
    let joined = join_read(&read, &sources, out, tags);
    let _ = std::fs::remove_file(&scratch);
    joined
}

/// Where the output starts on the inputs' own timeline, in 90 kHz ticks, once `line_up` has
/// moved them: the earliest start an input says (its origin), which it moved to zero; 0 where
/// none says, as then none was moved.
fn zero_of(read: &[Input]) -> i64 {
    read.iter()
        .filter_map(|input| {
            input
                .origin
                .map(|o| i128::from(o) * i128::from(input.scale))
        })
        .min()
        .map_or(0, |ns| i64::try_from(ns * 9 / 100_000).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(scale: u64, origin: Option<i64>) -> Input {
        Input {
            scale,
            tracks: Vec::new(),
            blocks: Vec::new(),
            origin,
        }
    }

    /// The earliest input that says where it starts, in 90 kHz ticks: an MP4's nanoseconds and
    /// a WebM's milliseconds alike; none saying is 0.
    #[test]
    fn the_output_starts_at_the_earliest_input_that_says_where_it_starts() {
        let picture = input(1, Some(10_000_000_000));
        let sound = input(SCALE_NS, Some(9_500));
        assert_eq!(zero_of(&[picture, sound]), 855_000);
        assert_eq!(
            zero_of(&[input(1, None), input(SCALE_NS, Some(10_000))]),
            900_000
        );
        assert_eq!(zero_of(&[input(1, None)]), 0);
    }

    /// A WebM picture that says nothing of its start: the cues are asked for from 0 and
    /// carried as a WebVTT track, each at its time.
    #[test]
    fn the_cues_placed_are_carried_at_their_times() {
        let dir = tempfile::tempdir().unwrap();
        let picture = dir.path().join("v.webm");
        testing::tiny_webm(&picture, "V_VP8", 1, 30);
        let out = dir.path().join("out.mkv");
        let mut asked = None;
        let place = |zero: i64| {
            asked = Some(zero);
            vec![crate::subtitles::Subtitle {
                language: "en".into(),
                cues: vec![crate::subtitles::Cue {
                    start: 300,
                    end: 700,
                    text: "Hello".into(),
                }],
            }]
        };
        join_placed(&[picture], place, &out).unwrap();
        assert_eq!(asked, Some(0));
        assert_eq!(testing::cues_in(&out), [(300, "Hello".to_string())]);
    }
}
