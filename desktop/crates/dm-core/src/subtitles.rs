//! Subtitles read from WebVTT, for dm to put into a video itself.
//!
//! yt-dlp fetches a video's subtitles as WebVTT files beside it,
//! `name.en.vtt`, and left putting them into the video to ffmpeg. dm reads
//! their cues here and the MP4 and WebM writers carry them as text tracks
//! (`mp4mux`, `mkvmux`), one per language, so a player offers them.
//!
//! The files came from a server: anything unreadable is skipped, never a
//! panic.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::path::{Path, PathBuf};

/// One cue: when it shows and hides, in milliseconds, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub start: u64,
    pub end: u64,
    /// As written, tags and all (WebM carries WebVTT as WebVTT).
    pub text: String,
}

/// One language's subtitles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitle {
    /// The code in the file's name: `en`, `pt-BR`, `de`.
    pub language: String,
    pub cues: Vec<Cue>,
}

/// A WebVTT timestamp, `hh:mm:ss.ttt` or `mm:ss.ttt`, in milliseconds.
fn timestamp(text: &str) -> Option<u64> {
    let (clock, millis) = text.trim().split_once(['.', ','])?;
    let millis: u64 = millis.get(..3.min(millis.len()))?.parse().ok()?;
    let parts: Vec<u64> = clock
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let seconds = match parts.as_slice() {
        [h, m, s] => h * 3600 + m * 60 + s,
        [m, s] => m * 60 + s,
        _ => return None,
    };
    Some(seconds * 1000 + millis)
}

/// The cues in a WebVTT file, in time order. Notes, styles and regions are
/// passed over; a cue with no text or that ends before it starts is
/// dropped.
pub fn parse_vtt(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let mut lines = block.lines().skip_while(|l| l.trim().is_empty());
        let Some(mut line) = lines.next() else {
            continue;
        };
        if !line.contains("-->") {
            // An identifier, or WEBVTT / NOTE / STYLE / REGION: a cue's
            // timing is on the next line, or there is no cue here.
            match lines.next() {
                Some(next) if next.contains("-->") => line = next,
                _ => continue,
            }
        }
        let Some((from, rest)) = line.split_once("-->") else {
            continue;
        };
        let to = rest.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (timestamp(from), timestamp(to)) else {
            continue;
        };
        let body: Vec<&str> = lines.collect();
        let body = body.join("\n");
        if end <= start || body.trim().is_empty() {
            continue;
        }
        cues.push(Cue {
            start,
            end,
            text: body,
        });
    }
    cues.sort_by_key(|c| c.start);
    cues
}

/// A cue's text without WebVTT's tags and with its entities read, for a
/// format that takes plain text (an MP4's `tx3g`).
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&lrm;", "\u{200e}")
        .replace("&rlm;", "\u{200f}")
        .replace("&amp;", "&")
}

/// The ISO 639-2 code an MP4 or WebM names a language by, from the code in
/// a subtitle file's name. `und` for one this does not know.
pub fn iso639_2(language: &str) -> [u8; 3] {
    let base = language
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if base.len() == 3 && base.bytes().all(|b| b.is_ascii_lowercase()) {
        let b = base.as_bytes();
        if let (Some(x), Some(y), Some(z)) = (b.first(), b.get(1), b.get(2)) {
            return [*x, *y, *z];
        }
    }
    const CODES: [(&str, &[u8; 3]); 48] = [
        ("en", b"eng"),
        ("es", b"spa"),
        ("fr", b"fra"),
        ("de", b"deu"),
        ("it", b"ita"),
        ("pt", b"por"),
        ("ru", b"rus"),
        ("ja", b"jpn"),
        ("ko", b"kor"),
        ("zh", b"zho"),
        ("ar", b"ara"),
        ("hi", b"hin"),
        ("bn", b"ben"),
        ("pa", b"pan"),
        ("ur", b"urd"),
        ("id", b"ind"),
        ("ms", b"msa"),
        ("tr", b"tur"),
        ("vi", b"vie"),
        ("th", b"tha"),
        ("nl", b"nld"),
        ("pl", b"pol"),
        ("uk", b"ukr"),
        ("cs", b"ces"),
        ("sk", b"slk"),
        ("sv", b"swe"),
        ("da", b"dan"),
        ("no", b"nor"),
        ("nb", b"nob"),
        ("fi", b"fin"),
        ("el", b"ell"),
        ("he", b"heb"),
        ("iw", b"heb"),
        ("hu", b"hun"),
        ("ro", b"ron"),
        ("bg", b"bul"),
        ("hr", b"hrv"),
        ("sr", b"srp"),
        ("sl", b"slv"),
        ("lt", b"lit"),
        ("lv", b"lav"),
        ("et", b"est"),
        ("fa", b"fas"),
        ("ta", b"tam"),
        ("te", b"tel"),
        ("ne", b"nep"),
        ("fil", b"fil"),
        ("ca", b"cat"),
    ];
    CODES
        .iter()
        .find(|(two, _)| *two == base)
        .map_or(*b"und", |(_, three)| **three)
}

/// The subtitle files yt-dlp left beside a video named `video`
/// (`name.<language>.vtt`), read. Unreadable ones are skipped.
pub fn beside(video: &Path) -> Vec<(PathBuf, Subtitle)> {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem()) else {
        return Vec::new();
    };
    let prefix = format!("{}.", stem.to_string_lossy());
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(PathBuf, Subtitle)> = listing
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let language = name
                .strip_prefix(&prefix)?
                .strip_suffix(".vtt")?
                .to_string();
            if language.is_empty() || language.contains('.') {
                return None;
            }
            let cues = parse_vtt(&std::fs::read_to_string(entry.path()).ok()?);
            (!cues.is_empty()).then(|| (entry.path(), Subtitle { language, cues }))
        })
        .collect();
    found.sort_by(|a, b| a.1.language.cmp(&b.1.language));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_webvtt_file_is_read_into_cues() {
        let vtt = "\u{feff}WEBVTT\nKind: captions\nLanguage: en\n\nNOTE a note\n\n1\n00:00:01.000 --> 00:00:03.500 align:start position:0%\nHello <i>there</i>\nsecond line\n\n00:04.250 --> 00:05.000\nA &amp; B\n\n00:09.000 --> 00:08.000\nbackwards\n";
        let cues = parse_vtt(vtt);
        assert_eq!(cues.len(), 2, "{cues:?}");
        assert_eq!((cues[0].start, cues[0].end), (1000, 3500));
        assert_eq!(cues[0].text, "Hello <i>there</i>\nsecond line");
        assert_eq!((cues[1].start, cues[1].end), (4250, 5000));
        assert_eq!(plain(&cues[0].text), "Hello there\nsecond line");
        assert_eq!(plain(&cues[1].text), "A & B");
    }

    #[test]
    fn languages_are_named_as_containers_name_them() {
        assert_eq!(&iso639_2("en"), b"eng");
        assert_eq!(&iso639_2("pt-BR"), b"por");
        assert_eq!(&iso639_2("deu"), b"deu");
        assert_eq!(&iso639_2("xx"), b"und");
    }

    #[test]
    fn the_files_beside_a_video_are_found_by_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("talk [id].mp4");
        std::fs::write(&video, b"").unwrap();
        let cue = "WEBVTT\n\n00:00.000 --> 00:01.000\nhi\n";
        std::fs::write(dir.path().join("talk [id].en.vtt"), cue).unwrap();
        std::fs::write(dir.path().join("talk [id].de.vtt"), cue).unwrap();
        std::fs::write(dir.path().join("talk [id].live_chat.json"), "{}").unwrap();
        std::fs::write(dir.path().join("other.en.vtt"), cue).unwrap();
        std::fs::write(dir.path().join("talk [id].fr.vtt"), "WEBVTT\n").unwrap();
        let found: Vec<String> = beside(&video)
            .into_iter()
            .map(|(_, s)| s.language)
            .collect();
        assert_eq!(found, ["de", "en"]);
    }

    proptest::proptest! {
        #[test]
        fn any_text_is_read_without_a_panic(text in ".{0,400}") {
            for cue in parse_vtt(&text) {
                proptest::prop_assert!(cue.end > cue.start);
                let _ = plain(&cue.text);
            }
            let _ = iso639_2(&text);
        }
    }
}
