// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A file looked at before a join: whether it needs Matroska, whether it is WebM, its sound's
//! codec, and its tags.

use super::*;

/// Whether `inputs` hold a picture or sound no MP4 EDM writes can hold: a
/// WebM's VP8 or Vorbis (or any codec but VP9, AV1 and Opus). Such a pair
/// is joined into a Matroska file instead of being refused. An input that
/// cannot be read says nothing here; the join that follows says why.
pub fn needs_matroska(inputs: &[PathBuf]) -> bool {
    inputs.iter().any(|path| {
        is_webm(path)
            && read_webm(path, 0, true).is_ok_and(|input| {
                input
                    .tracks
                    .iter()
                    .any(|t| !MP4_TAKES.contains(&t.codec.as_str()))
            })
    })
}

// ---- a WebM's tracks for an MP4 ------------------------------------------

/// Whether the file at `path` starts as an EBML (WebM, Matroska) file.
pub(crate) fn is_webm(path: &Path) -> bool {
    let mut head = [0u8; 4];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && u32::from_be_bytes(head) == EBML
}

/// The codec of the first sound track of the WebM (or Matroska) file at `path`, as Matroska
/// names it ("A_OPUS", "A_VORBIS"): `None` where it is not one or has no sound. What a video's
/// sound saved as MP3 is read from (`mp3::takes`).
pub(crate) fn sound_codec(path: &Path) -> Option<String> {
    if !is_webm(path) {
        return None;
    }
    read_webm(path, 0, true)
        .ok()?
        .tracks
        .into_iter()
        .find(|t| t.kind == 2)
        .map(|t| t.codec)
}

/// A WebM's own words, as EDM writes them (`tags_element`) and other writers do: the title in
/// its Info (or a TITLE tag), and its ARTIST, DATE_RELEASED and URL tags, read before its first
/// cluster. Empty where it says none or cannot be read. What its sound saved as MP3 carries in
/// its ID3 header (`mp3::id3`, gaps row Y25).
pub(crate) fn tags(path: &Path) -> crate::mp4mux::Tags {
    let mut tags = crate::mp4mux::Tags::default();
    let _ = read_tags(path, &mut tags);
    tags
}

fn read_tags(path: &Path, tags: &mut crate::mp4mux::Tags) -> Result<(), Error> {
    // Words, not pictures: an Info or a Tags this large is not one EDM reads.
    const MOST: u64 = 1 << 20;
    let length = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let mut r = Reader {
        file: Positioned::open(path).map_err(|e| e.to_string())?,
        length,
    };
    let (id, _, end) = r.header(0, length)?;
    if id != EBML {
        return Err("it is not a WebM file".into());
    }
    let mut at = end.ok_or("a damaged element")?;
    let mut segment_end = None;
    while at < length {
        let (id, start, end) = r.header(at, length)?;
        if id == SEGMENT {
            at = start;
            segment_end = Some(end.unwrap_or(length).min(length));
            break;
        }
        at = end.ok_or("a damaged element")?;
    }
    let segment_end = segment_end.ok_or("it has no segment")?;
    let text = |value: &[u8]| String::from_utf8_lossy(value).trim().to_string();
    while at + 2 <= segment_end {
        let (id, start, end) = r.header(at, segment_end)?;
        let Some(end) = end.map(|e| e.min(segment_end)) else {
            break;
        };
        match id {
            CLUSTER => break,
            INFO | TAGS if end - start <= MOST => {
                let body = r.read_at(start, (end - start) as usize)?;
                if id == INFO {
                    if let Some(title) = find(&body, TITLE) {
                        tags.title = text(title);
                    }
                } else {
                    let simple = elements(&body)
                        .filter(|(id, _, _)| *id == TAG)
                        .flat_map(|(_, tag, _)| elements(tag).collect::<Vec<_>>())
                        .filter(|(id, _, _)| *id == SIMPLE_TAG);
                    for (_, simple, _) in simple {
                        let (Some(name), Some(value)) =
                            (find(simple, TAG_NAME), find(simple, TAG_STRING))
                        else {
                            continue;
                        };
                        let slot = match text(name).to_ascii_uppercase().as_str() {
                            "TITLE" if tags.title.is_empty() => &mut tags.title,
                            "ARTIST" => &mut tags.artist,
                            "DATE_RELEASED" => &mut tags.date,
                            "URL" => &mut tags.source,
                            _ => continue,
                        };
                        *slot = text(value);
                    }
                }
            }
            _ => {}
        }
        at = end;
    }
    Ok(())
}

/// An element's body among `elements`, by its ID.
pub(super) fn find(elements: &[u8], id: u32) -> Option<&[u8]> {
    self::elements(elements)
        .find(|(i, _, _)| *i == id)
        .map(|(_, body, _)| body)
}
