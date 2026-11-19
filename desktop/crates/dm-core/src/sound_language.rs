// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The language a video's sound is wanted in, where a video offers it in several (gaps row
//! Y10): a dub, or the original of a video whose player marks a dub to play. Unset, a video's
//! own sound is taken, as before: YouTube's original (`youtube::set_dubs_aside`), HLS's default
//! rendition (`stream::audio_playlist`) and DASH's main one (`dash::choose`). Set, the first
//! track speaking that language is taken in each, a described one only where no plain one does,
//! and the video's own where none speaks it.

use std::sync::RwLock;

static WANTED: RwLock<Option<String>> = RwLock::new(None);

/// Puts the language in force for every video read from now on: a language tag (`es`,
/// `pt-BR`), as Options keeps it. Empty, or `original`, takes each video's own sound.
pub fn set_sound_language(tag: &str) {
    let tag = tag.trim();
    let wanted =
        (!tag.is_empty() && !tag.eq_ignore_ascii_case("original")).then(|| tag.replace('_', "-"));
    *WANTED.write().unwrap_or_else(|held| held.into_inner()) = wanted;
}

/// The language in force; `None` for each video's own sound.
pub fn wanted() -> Option<String> {
    WANTED
        .read()
        .unwrap_or_else(|held| held.into_inner())
        .clone()
}

/// Whether a track named `tag` (`es-US`, `es_419`, `ES`) speaks the language `want` asks: each
/// of `want`'s parts is the track's, in order, whatever their case. `es` is spoken by `es-US`
/// and `es-419`; `pt-BR` by `pt-BR` alone, not by `pt` or `pt-PT`.
pub fn speaks(tag: &str, want: &str) -> bool {
    let parts = |text: &str| {
        text.trim()
            .split(['-', '_'])
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
    };
    let (tag, want) = (parts(tag), parts(want));
    !want.iter().all(String::is_empty) && tag.starts_with(&want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_is_spoken_by_its_regions_and_a_region_by_itself_alone() {
        assert!(speaks("es-US", "es"));
        assert!(speaks("es_419", "ES"));
        assert!(speaks("es", "es"));
        assert!(speaks("pt-BR", "pt-br"));
        assert!(!speaks("pt", "pt-BR"));
        assert!(!speaks("pt-PT", "pt-BR"));
        assert!(!speaks("en-US", "es"));
        assert!(!speaks("est", "es"), "Estonian is not Spanish");
        assert!(!speaks("es", ""));
        assert!(!speaks("", "es"));
    }
}
