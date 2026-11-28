// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Subtitles written out as files of their own: SRT and WebVTT, from the cues EDM read
//! (gaps row Y6). Each cue is written with its own times, in start order, overlapping ones
//! too (both formats allow it); one with no text, or that ends before it starts, is left out.

use std::path::{Path, PathBuf};

use super::{Cue, Subtitle};

/// A subtitle file's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// SubRip: numbered cues, `00:00:01,000`, plain text with `<i>`, `<b>` and `<u>`.
    Srt,
    /// WebVTT: `WEBVTT`, then cues timed `00:00:01.000`, their text as WebVTT has it.
    Vtt,
}

impl Format {
    /// The file name's ending, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Srt => "srt",
            Format::Vtt => "vtt",
        }
    }
}

/// `ms` as `hh:mm:ss` and the milliseconds after `separator` (`,` for SRT, `.` for WebVTT).
/// Hours go past 99 rather than wrap.
fn clock(ms: u64, separator: char) -> String {
    let (h, m, s, t) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    format!("{h:02}:{m:02}:{s:02}{separator}{t:03}")
}

/// The cues worth writing, in start order (then end): those with words to show (not tags
/// alone) that end after they start.
fn kept(cues: &[Cue]) -> Vec<&Cue> {
    let mut kept: Vec<&Cue> = cues
        .iter()
        .filter(|c| c.end > c.start && !super::plain(&c.text).trim().is_empty())
        .collect();
    kept.sort_by_key(|c| (c.start, c.end));
    kept
}

/// A cue's lines, the empty ones dropped (an empty line ends a cue in both formats) and
/// `-->` (which marks a timing line) written as `arrow`.
fn lines(text: &str, arrow: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.replace("-->", arrow))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A WebVTT cue's text as SRT takes it: italic, bold and underline kept, every other tag
/// (voices, classes, the times inside a cue, ruby) dropped, and the entities read.
fn srt_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open..).unwrap_or_default();
        let Some(close) = after.find('>') else {
            // A `<` that opens no tag is text.
            out.push_str(after);
            rest = "";
            break;
        };
        let tag = after.get(1..close).unwrap_or_default();
        let name = tag
            .trim_start_matches('/')
            .split(['.', ' '])
            .next()
            .unwrap_or_default();
        if matches!(name, "i" | "b" | "u") {
            let slash = if tag.starts_with('/') { "/" } else { "" };
            out.push_str(&format!("<{slash}{name}>"));
        }
        rest = after.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&lrm;", "\u{200e}")
        .replace("&rlm;", "\u{200f}")
        .replace("&amp;", "&")
}

/// The cues as an SRT file: numbered from 1, a blank line after each. No cues, an empty
/// file.
pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    let mut n = 0;
    for cue in kept(cues) {
        let text = lines(&srt_text(&cue.text), "->");
        if text.is_empty() {
            continue;
        }
        n += 1;
        out.push_str(&format!(
            "{n}\n{} --> {}\n{text}\n\n",
            clock(cue.start, ','),
            clock(cue.end, ',')
        ));
    }
    out
}

/// The cues as a WebVTT file: their text as WebVTT has it (`Cue::text` is kept as written),
/// a `-->` in it written `--&gt;`. No cues, the header alone.
pub fn to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in kept(cues) {
        let text = lines(&cue.text, "--&gt;");
        if text.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "{} --> {}\n{text}\n\n",
            clock(cue.start, '.'),
            clock(cue.end, '.')
        ));
    }
    out
}

/// One language's subtitles written beside `video` as `name.<language>.<srt|vtt>`. A language
/// that is not a plain code (letters, digits, `-`, `_`) is refused, as the fetch step refuses
/// one, so no name leaves the folder.
pub fn write_beside(video: &Path, subtitle: &Subtitle, format: Format) -> std::io::Result<PathBuf> {
    let language = &subtitle.language;
    let plain = !language.is_empty()
        && language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    let (Some(dir), Some(stem), true) = (video.parent(), video.file_stem(), plain) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no name for the subtitles beside this video",
        ));
    };
    let path = dir.join(format!(
        "{}.{language}.{}",
        stem.to_string_lossy(),
        format.extension()
    ));
    let body = match format {
        Format::Srt => to_srt(&subtitle.cues),
        Format::Vtt => to_vtt(&subtitle.cues),
    };
    std::fs::write(&path, body)?;
    Ok(path)
}

#[cfg(test)]
mod tests;
