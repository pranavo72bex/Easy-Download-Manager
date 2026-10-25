//! The computer's own fonts for other scripts.

use super::*;

/// Writing systems Inter and egui's fonts have no letters for, which a
/// file name can still be in. Each is drawn with a font the computer
/// already has, read the first time a name needs it: nothing is bundled,
/// and a computer without one draws what it can, as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    /// Chinese characters, and Japanese with its kana.
    Cjk,
    Hangul,
    Arabic,
    Hebrew,
    Thai,
    Devanagari,
}

impl Script {
    pub const ALL: [Script; 6] = [
        Script::Cjk,
        Script::Hangul,
        Script::Arabic,
        Script::Hebrew,
        Script::Thai,
        Script::Devanagari,
    ];

    /// The writing system a character needs a font for, if any.
    pub fn of(c: char) -> Option<Script> {
        match u32::from(c) {
            0x0590..=0x05FF | 0xFB1D..=0xFB4F => Some(Script::Hebrew),
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
                Some(Script::Arabic)
            }
            0x0900..=0x097F | 0xA8E0..=0xA8FF => Some(Script::Devanagari),
            0x0E00..=0x0E7F => Some(Script::Thai),
            0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => {
                Some(Script::Hangul)
            }
            0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF => {
                Some(Script::Cjk)
            }
            _ => None,
        }
    }

    /// The language fontconfig is asked about, for this script.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) fn language(self) -> &'static str {
        match self {
            Script::Cjk => "zh-cn",
            Script::Hangul => "ko",
            Script::Arabic => "ar",
            Script::Hebrew => "he",
            Script::Thai => "th",
            Script::Devanagari => "hi",
        }
    }

    /// Where this computer's fonts for the script usually are, best first.
    pub(super) fn known_files(self) -> &'static [&'static str] {
        #[cfg(target_os = "macos")]
        return match self {
            Script::Cjk => &[
                "/System/Library/Fonts/PingFang.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
                "/System/Library/Fonts/STHeiti Light.ttc",
            ],
            Script::Hangul => &[
                "/System/Library/Fonts/AppleSDGothicNeo.ttc",
                "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
            ],
            Script::Arabic => &[
                "/System/Library/Fonts/SFArabic.ttf",
                "/System/Library/Fonts/GeezaPro.ttc",
            ],
            Script::Hebrew => &[
                "/System/Library/Fonts/SFHebrew.ttf",
                "/System/Library/Fonts/ArialHB.ttc",
            ],
            Script::Thai => &[
                "/System/Library/Fonts/ThonburiUI.ttc",
                "/System/Library/Fonts/Thonburi.ttc",
                "/System/Library/Fonts/Supplemental/Thonburi.ttc",
            ],
            Script::Devanagari => &[
                "/System/Library/Fonts/Kohinoor.ttc",
                "/System/Library/Fonts/Supplemental/DevanagariMT.ttc",
            ],
        };
        #[cfg(windows)]
        return match self {
            Script::Cjk => &[
                "C:\\Windows\\Fonts\\msyh.ttc",
                "C:\\Windows\\Fonts\\YuGothM.ttc",
                "C:\\Windows\\Fonts\\meiryo.ttc",
                "C:\\Windows\\Fonts\\simsun.ttc",
            ],
            Script::Hangul => &[
                "C:\\Windows\\Fonts\\malgun.ttf",
                "C:\\Windows\\Fonts\\gulim.ttc",
            ],
            Script::Arabic | Script::Hebrew => &[
                "C:\\Windows\\Fonts\\tahoma.ttf",
                "C:\\Windows\\Fonts\\arial.ttf",
            ],
            Script::Thai => &[
                "C:\\Windows\\Fonts\\LeelawUI.ttf",
                "C:\\Windows\\Fonts\\tahoma.ttf",
            ],
            Script::Devanagari => &[
                "C:\\Windows\\Fonts\\Nirmala.ttf",
                "C:\\Windows\\Fonts\\mangal.ttf",
            ],
        };
        #[cfg(not(any(target_os = "macos", windows)))]
        return match self {
            Script::Cjk => &[
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            ],
            Script::Hangul => &[
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            ],
            Script::Arabic => &["/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf"],
            Script::Hebrew => &["/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf"],
            Script::Thai => &["/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"],
            Script::Devanagari => {
                &["/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf"]
            }
        };
    }
}

/// The computer's font file for `script`, and the face in it: on Linux,
/// whichever fontconfig says covers the language; elsewhere, and failing
/// that, the first of the usual files that is there.
pub fn system_font(script: Script) -> Option<(std::path::PathBuf, u32)> {
    // A Japanese window draws its kanji in a Japanese face: a Chinese one
    // shapes some of the same characters differently.
    let japanese = script == Script::Cjk && crate::i18n::code() == "ja";
    #[cfg(target_os = "linux")]
    if let Some(found) = fontconfig(script, japanese) {
        return Some(found);
    }
    let first: &[&str] = if japanese { JAPANESE_FILES } else { &[] };
    first
        .iter()
        .chain(script.known_files())
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
        .map(|p| (p, 0))
}

/// Japanese faces, tried first for a Japanese window's kanji and kana.
#[cfg(target_os = "macos")]
pub(super) const JAPANESE_FILES: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans W3.ttc",
];
#[cfg(windows)]
pub(super) const JAPANESE_FILES: &[&str] = &[
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\meiryo.ttc",
];
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) const JAPANESE_FILES: &[&str] = &[];

/// Asks fontconfig for a sans font that covers `script`'s language, and
/// takes it only if it does: `fc-match` always answers with something.
#[cfg(target_os = "linux")]
pub(super) fn fontconfig(script: Script, japanese: bool) -> Option<(std::path::PathBuf, u32)> {
    let language = if japanese { "ja" } else { script.language() };
    let out = std::process::Command::new("fc-match")
        .arg("-f")
        .arg("%{file}\\n%{index}\\n%{lang}")
        .arg(format!("sans-serif:lang={language}"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut lines = text.lines();
    let file = std::path::PathBuf::from(lines.next()?);
    let index = lines.next()?.trim().parse().unwrap_or(0);
    let covers = lines
        .next()?
        .split('|')
        .any(|l| l == language || l.split('-').next() == language.split('-').next());
    (covers && file.is_file()).then_some((file, index))
}

/// The font for `script`, read from disk once a process; `None` where the
/// computer has none (asked once too).
pub(super) fn system_font_data(script: Script) -> Option<egui::FontData> {
    pub(super) static READ: [std::sync::OnceLock<Option<egui::FontData>>; 6] =
        [const { std::sync::OnceLock::new() }; 6];
    let at = Script::ALL.iter().position(|s| *s == script)?;
    READ.get(at)?
        .get_or_init(|| {
            let (path, index) = system_font(script)?;
            Some(egui::FontData::from_blob(font_file(&path)?, index))
        })
        .clone()
}

/// A font file of the computer's, mapped into memory rather than read
/// into it: a CJK collection is about 20 MB, all of it held for a few
/// glyphs, where mapped only the pages read are in memory and the system
/// can drop them again (+19 MB for one Japanese
/// name). Read whole where it cannot be mapped.
pub(super) fn font_file(path: &std::path::Path) -> Option<egui::epaint::text::Blob> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: a system font, which package managers and system updates
    // replace by putting a new file in its place, leaving this mapping
    // whole; macOS keeps its own fonts on a read-only volume. A font cut
    // short where it lies, under a running dm, is the case this does not
    // survive.
    match unsafe { memmap2::Mmap::map(&file) } {
        Ok(map) => Some(std::sync::Arc::new(map)),
        Err(_) => Some(std::sync::Arc::new(std::fs::read(path).ok()?)),
    }
}

/// The scripts asked for so far in this window.
pub(super) fn scripts_asked(ctx: &Context) -> Vec<Script> {
    ctx.data(|d| d.get_temp::<Vec<Script>>(egui::Id::unique("dm-scripts")))
        .unwrap_or_default()
}

/// Makes sure every script in `texts` has a font: the first name in
/// Chinese, Korean, Arabic… has the computer's font for it read and added
/// behind dm's own, to be drawn from the next frame. Cheap when nothing is
/// new, which is every frame but the first that needs one.
pub fn cover<'a>(ctx: &Context, texts: impl IntoIterator<Item = &'a str>) {
    let mut asked = scripts_asked(ctx);
    if asked.len() == Script::ALL.len() {
        return;
    }
    let before = asked.len();
    for text in texts {
        if text.is_ascii() {
            continue;
        }
        for script in text.chars().filter_map(Script::of) {
            if !asked.contains(&script) {
                asked.push(script);
            }
        }
    }
    if asked.len() == before {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("dm-scripts"), asked.clone()));
    ctx.set_fonts(definitions(&asked));
}
