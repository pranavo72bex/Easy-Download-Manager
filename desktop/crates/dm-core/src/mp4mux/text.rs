//! The subtitle track a joined file can carry.

use super::*;

/// A language's subtitles as an MP4 text track (`tx3g`, what players read
/// as subtitles): a sample per cue, holding its plain text, and an empty
/// one for each gap between cues, so every cue shows when it should. Its
/// samples go on the end of `data`, which is input `input`.
pub(super) fn text_track(
    text: &crate::subtitles::Subtitle,
    input: usize,
    data: &mut Vec<u8>,
) -> Track {
    let mut samples = Vec::new();
    let mut at = 0u64;
    let mut push = |data: &mut Vec<u8>, body: &[u8], duration: u64| {
        let offset = data.len() as u64;
        data.extend_from_slice(&(body.len() as u16).to_be_bytes());
        data.extend_from_slice(body);
        samples.push(Sample {
            offset,
            size: body.len() as u32 + 2,
            duration: u32::try_from(duration).unwrap_or(u32::MAX),
            cts: 0,
            sync: true,
        });
    };
    for cue in &text.cues {
        let start = cue.start.max(at);
        if cue.end <= start {
            continue;
        }
        if start > at {
            push(data, b"", start - at);
        }
        let plain = crate::subtitles::plain(&cue.text);
        // A sample's text has a 16-bit length; a cue that long is cut, at a
        // character's edge.
        let mut cut = plain.len().min(0xFFFF);
        while !plain.is_char_boundary(cut) {
            cut -= 1;
        }
        push(
            data,
            plain.get(..cut).unwrap_or_default().as_bytes(),
            cue.end - start,
        );
        at = cue.end;
    }
    // An empty sample to close the last cue: readers time a subtitle by the
    // sample after it, and with none the last one ended when the picture did.
    if !text.cues.is_empty() {
        push(data, b"", 1);
    }
    let [a, b, c] = crate::subtitles::iso639_2(&text.language);
    let language = (u16::from(a.saturating_sub(0x60) & 0x1f) << 10)
        | (u16::from(b.saturating_sub(0x60) & 0x1f) << 5)
        | u16::from(c.saturating_sub(0x60) & 0x1f);
    // tx3g: no special display, centred at the bottom, white on clear.
    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&[1, 0xff]);
    entry.extend_from_slice(&[0, 0, 0, 0]);
    entry.extend_from_slice(&[0u8; 8]);
    entry.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 18, 0xff, 0xff, 0xff, 0xff]);
    let mut ftab = 1u16.to_be_bytes().to_vec();
    ftab.extend_from_slice(&1u16.to_be_bytes());
    ftab.push(10);
    ftab.extend_from_slice(b"Sans-Serif");
    let tx3g = boxed(b"tx3g", &[&entry, &boxed(b"ftab", &[&ftab])]);
    let stsd = full(b"stsd", 0, 0, &[&1u32.to_be_bytes()[..], &tx3g].concat());
    let mut tail = vec![0u8; 8];
    tail.extend_from_slice(&0u16.to_be_bytes());
    // One alternate group for every language: a player shows one at a time.
    tail.extend_from_slice(&2u16.to_be_bytes());
    tail.extend_from_slice(&[0u8; 4]);
    for m in MATRIX {
        tail.extend_from_slice(&m.to_be_bytes());
    }
    tail.extend_from_slice(&[0u8; 8]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend_from_slice(b"sbtl");
    hdlr.extend_from_slice(&[0u8; 12]);
    hdlr.extend_from_slice(b"SubtitleHandler\0");
    Track {
        input,
        id: 0,
        tkhd_tail: tail,
        timescale: 1000,
        language,
        hdlr: full(b"hdlr", 0, 0, &hdlr),
        media_header: full(b"nmhd", 0, 0, &[]),
        stsd,
        edits: Vec::new(),
        edits_timescale: MOVIE_TIMESCALE,
        samples,
        start: None,
    }
}

// ---- reading ------------------------------------------------------------
