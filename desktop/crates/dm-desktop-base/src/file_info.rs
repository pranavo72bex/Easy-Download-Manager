// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details, for every window (B02): what it offers (the places to
//! save in, the name a file will really get, the times "Delete file after"
//! offers, a video's qualities), and what Start now and Add for later add.
//! What each window draws, and where the keyboard goes, is its own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dm_core::{AddOptions, Entry, Id};
use dm_shell::Settings;

use crate::format;
use crate::i18n::{t, tf};

/// The key a file's kind is remembered under in `last_folders`.
pub fn kind_key(name: &str) -> &'static str {
    dm_core::by_name(name)
        .unwrap_or(dm_core::Category::Other)
        .folder()
}

/// The times "Delete file after" offers, in seconds, in the language in
/// force; `None` keeps the file.
pub fn delete_after_times() -> [(&'static str, Option<u64>); 4] {
    [
        (t("Never"), None),
        (t("10 minutes"), Some(600)),
        (t("1 hour"), Some(3600)),
        (t("1 day"), Some(86_400)),
    ]
}

/// Each kind's folder, and each category a person made, to pick from (its
/// name, its folder): a file can go to another kind's folder without typing
/// its path. Each kind's own folder, whether or not finished files are
/// sorted into them: picked here, it is asked for (with sorting off every
/// kind named the download folder itself).
pub fn places(settings: &Settings) -> Vec<(String, String)> {
    let mut categories = settings.categories();
    categories.enabled = true;
    let root = &settings.download_dir;
    let mut places: Vec<(String, String)> = dm_core::Category::ALL
        .iter()
        .map(|kind| {
            let word = format::titled(&dm_shell::app::names::category_word(*kind));
            (
                crate::i18n::name(&word),
                categories.destination(root, *kind).display().to_string(),
            )
        })
        .collect();
    places.extend(settings.custom_categories.iter().map(|custom| {
        let name = custom.name.trim().to_string();
        (name.clone(), root.join(&name).display().to_string())
    }));
    places
}

/// The folder an empty "Save to folder" means, as its hint says it: sorted
/// into kinds' folders only where Options › Folders sorts them.
pub fn default_folder_words(settings: &Settings) -> String {
    let dir = settings.download_dir.display().to_string();
    if settings.categorize {
        tf("{} (sorted by kind)", &[&dir])
    } else {
        dir
    }
}

/// The folder a download of `name` from `url` is saved in, as the dialog
/// stands: the folder typed, else the site rule's, else its category's, else
/// the download folder.
pub fn folder_for(settings: &Settings, typed: &str, url: &str, name: &str) -> PathBuf {
    if !typed.trim().is_empty() {
        PathBuf::from(typed.trim())
    } else if let Some(rule) = settings
        .site_rule_for(url)
        .filter(|rule| !rule.folder.is_empty())
    {
        // The site's own folder, as its rule says.
        PathBuf::from(&rule.folder)
    } else {
        let mut categories = settings.categories();
        if categories.enabled {
            categories.site = dm_core::host_of(url);
            let kind = dm_core::by_name(name).unwrap_or(dm_core::Category::Other);
            categories.place(&settings.download_dir, kind, name)
        } else {
            settings.download_dir.clone()
        }
    }
}

/// The folder this kind of file went to last time, where it still is.
pub fn last_folder(settings: &Settings, name: &str) -> Option<String> {
    settings
        .last_folders
        .get(kind_key(name.trim()))
        .filter(|f| Path::new(f).is_dir())
        .cloned()
}

/// Where the name Download details shows is taken already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clash {
    /// By another download in the list: this one takes a number.
    Listed,
    /// By a file in the folder: this one takes `free`, or replaces `taken`
    /// once it is whole.
    OnDisk {
        /// The name it takes otherwise.
        free: String,
        /// The file there.
        taken: PathBuf,
    },
}

/// Whether `name`, saved from `url` into `folder` (as typed), is taken
/// already: the name it would really get differs (Save as said `big.zip`
/// and the file came as `big (1).zip`). `early` is the row begun at once for
/// this very dialog, which is this file.
pub fn clash(
    settings: &Settings,
    rows: &[Arc<Entry>],
    folder: &str,
    url: &str,
    name: &str,
    early: Option<Id>,
) -> Option<Clash> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let dir = folder_for(settings, folder, url, name);
    // Every download of that name still waiting to be placed there takes a
    // free name first (two rows read "big (1).zip" and one landed as
    // "big (2).zip").
    let waiting = rows
        .iter()
        .filter(|r| {
            Some(r.id) != early
                && !r.state.settled()
                && r.path.is_empty()
                && dm_shell::app::names::name_of(r) == name
                && r.save_dir
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty() || Path::new(s) == dir)
        })
        .count();
    let free = dm_core::nth_free_name(&dir, name, waiting);
    let taken = dir.join(name);
    if free == name {
        None
    } else if dm_core::free_name(&dir, name) == name {
        // Free on disk: it is the list that has the name.
        Some(Clash::Listed)
    } else if waiting == 0 && taken.is_file() {
        Some(Clash::OnDisk { free, taken })
    } else {
        Some(Clash::Listed)
    }
}

/// What Save as shows while it is empty: the name the file will get.
/// `titled`: a video picked from a player page, named from its title (the
/// page's own name, "player2.html", was shown, and the file came as
/// something else).
pub fn name_hint(url: &str, titled: bool, remux: bool) -> String {
    if titled {
        t("The video's own title").to_string()
    } else if dm_core::is_torrent(url) {
        t("The torrent's own name").to_string()
    } else {
        dm_shell::app::lookups::stream_name(url, None, remux)
            .unwrap_or_else(|| dm_shell::app::names::provisional(url))
    }
}

/// The part's ends as typed, where "Only a part of the video" is ticked: a
/// pair that is not a span is said, and keeps the dialog.
pub fn span_of(part: bool, from: &str, to: &str) -> Result<Option<dm_core::Span>, &'static str> {
    if !part {
        return Ok(None);
    }
    dm_core::Span::parse(from, to).map(Some).ok_or_else(|| {
        t("Enter the start and end as minutes and seconds, such as 4:10 and 4:45. The end comes after the start.")
    })
}

/// What the dialog holds when Start now or Add for later is pressed.
#[derive(Debug, Clone, Default)]
pub struct Form {
    /// The page a handed-over video came from, sent as Referer.
    pub referrer: String,
    /// The quality picked.
    pub quality: Option<String>,
    /// A separate sound track, fetched and joined with it.
    pub sound: Option<String>,
    /// The part of the video, where one was asked for.
    pub span: Option<dm_core::Span>,
    /// The part made vertical or square.
    pub shape: Option<dm_core::shape::Shape>,
    /// The part saved as a GIF.
    pub gif: bool,
    /// "Delete file after", in seconds.
    pub delete_after: Option<u64>,
    /// The folder, as typed.
    pub folder: String,
    /// The name, as typed.
    pub name: String,
    /// Whether a person typed in Save as.
    pub name_typed: bool,
    /// The note.
    pub description: String,
    /// The queue Add for later puts it in.
    pub queue: String,
    /// The site's sign-in the browser handed over.
    pub cookies: String,
    /// Those cookies are for this download alone.
    pub cookies_once: bool,
    /// The request headers the browser sent.
    pub headers: Vec<(String, String)>,
    /// The form it is the answer to.
    pub post: Option<dm_core::cookies::Posted>,
    /// Replace the file of its name once this one is whole.
    pub replace: bool,
}

/// What Start now (`later` false) or Add for later adds, with the name the
/// server gave and the clash the dialog showed.
pub fn options(
    form: &Form,
    later: bool,
    server_name: Option<&str>,
    clash: Option<&Clash>,
) -> AddOptions {
    AddOptions {
        description: form.description.trim().to_string(),
        queue: if later {
            form.queue.clone()
        } else {
            String::new()
        },
        title_hint: None,
        save_dir: Some(form.folder.trim().to_string()).filter(|f| !f.is_empty()),
        // The server's own name, untouched, is what dm names the file
        // anyway: left for it to place as it always does.
        save_name: Some(form.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .filter(|n| form.name_typed || server_name != Some(n.as_str())),
        later,
        referrer: form.referrer.clone(),
        quality: form.quality.clone(),
        sound: form.sound.clone(),
        span: form.span,
        shape: form.shape.filter(|_| form.span.is_some()),
        // An MP3 is asked for by its quality ("audio/mp3").
        mp3: false,
        gif: form.gif && form.span.is_some(),
        delete_after: form.delete_after,
        cookies: form.cookies.clone(),
        cookies_once: form.cookies_once,
        headers: form.headers.clone(),
        // The form for the first address alone, which is the one it was sent to.
        post: form.post.clone(),
        checksum: None,
        speed_limit: 0,
        // Added from Download details, by a person: not a hand-over that
        // may take a waiting row's place.
        fresh_link_ok: false,
        replaces: match clash {
            Some(Clash::OnDisk { taken, .. }) if form.replace => {
                Some(taken.to_string_lossy().into_owned())
            }
            _ => None,
        },
    }
}

/// Whether the dialog's one file starts at once, while it asks (where
/// Options says so; reference A's "start downloading while the dialog is
/// open"): a plain file, not a video page, a torrent, a form's answer or
/// several addresses, which wait for the person as before.
pub fn begins_early(settings: &Settings, urls: &[String], form: &Form, video: bool) -> bool {
    let [url] = urls else {
        return false;
    };
    settings.start_while_asking
        && (url.starts_with("https://") || url.starts_with("http://"))
        && !dm_core::is_torrent(url)
        && !video
        && form.quality.is_none()
        && form.post.is_none()
}

/// The choice the quality setting would take from a page's list (tallest
/// first, the sound alone last): the tallest at or under the height asked,
/// the first for "best", the sound for "audio"; `None` for an empty list.
pub fn default_choice(
    list: &[dm_core::videopage::Choice],
    setting: dm_core::Quality,
) -> Option<String> {
    use dm_core::Quality;
    let height = |choice: &dm_core::videopage::Choice| {
        choice
            .quality
            .split('/')
            .next()
            .and_then(|h| h.parse::<u32>().ok())
    };
    let chosen = match setting {
        Quality::Audio => list.iter().find(|c| c.quality.starts_with("audio")),
        Quality::UpTo(limit) => list
            .iter()
            .find(|c| height(c).is_some_and(|h| h <= limit))
            .or_else(|| list.iter().find(|c| height(c).is_some())),
        Quality::Best => list.iter().find(|c| height(c).is_some()),
    };
    chosen.or(list.first()).map(|c| c.quality.clone())
}

/// A quality's chip: its height and container over its size ("1080p MP4"
/// over "275.0 MB"), the sound alone said so.
pub fn chip_words(choice: &dm_core::videopage::Choice) -> String {
    let (head, ext) = choice
        .quality
        .split_once('/')
        .unwrap_or((&choice.quality, ""));
    let top = if head == "audio" {
        format!("{} {}", t("Sound only"), ext.to_ascii_uppercase())
    } else if let Ok(side) = head.parse::<u32>() {
        // By its class, as the panel's list says it: "4K MP4", "1080p MP4".
        format!(
            "{} {}",
            dm_core::videopage::class_name(0, side),
            ext.to_ascii_uppercase()
        )
    } else {
        choice.label.clone()
    };
    match choice.size {
        Some(size) => format!("{}\n{}", top.trim(), format::bytes(size)),
        None => top.trim().to_string(),
    }
}

/// Seconds as a person writes a moment of a video: `0:34`, `12:05`,
/// `1:02:03`.
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The video qualities a person picks from (Options, Playlist): the
/// setting's word, and its words in English.
pub const QUALITIES: [(&str, &str); 5] = [
    ("best", "Best available"),
    ("1080", "Up to 1080p"),
    ("720", "Up to 720p"),
    ("480", "Up to 480p"),
    ("audio", "Audio only"),
];

/// What the Playlist dialog adds: the ticked videos' addresses, at
/// `quality`, the playlist's page `page` their Referer, now or for `later`
/// (to the Main queue); none ticked is said.
pub fn playlist_adds(
    list: &dm_core::youtube::Playlist,
    picked: &[bool],
    quality: &str,
    page: &str,
    later: bool,
) -> Result<(Vec<String>, AddOptions), &'static str> {
    let urls: Vec<String> = list
        .entries
        .iter()
        .zip(picked)
        .filter(|(_, picked)| **picked)
        .map(|(entry, _)| entry.url())
        .collect();
    if urls.is_empty() {
        return Err(t("Pick at least one video."));
    }
    let with = AddOptions {
        later,
        queue: if later {
            dm_core::MAIN_QUEUE.into()
        } else {
            String::new()
        },
        quality: Some(quality.to_string()).filter(|q| !q.is_empty()),
        referrer: page.to_string(),
        ..AddOptions::default()
    };
    Ok((urls, with))
}

/// How many videos a playlist lists ("12 videos"), and that only the
/// first are, where the list went on past what was read.
pub fn playlist_count_words(list: &dm_core::youtube::Playlist) -> String {
    let n = list.entries.len();
    let words = crate::i18n::tn("{} video||{} videos", n as u64, &[&n]);
    if list.cut_short {
        format!(
            "{words} · {}",
            tf("Only the first {} videos are listed.", &[&n])
        )
    } else {
        words
    }
}

/// A video's line in the Playlist dialog: its number, its title and how
/// long it plays ("3. Intro · 4:05").
pub fn playlist_entry_words(index: usize, entry: &dm_core::youtube::PlaylistEntry) -> String {
    let words = format!("{}. {}", index + 1, entry.title);
    match entry.seconds {
        Some(seconds) => format!("{words} · {}", dm_core::Span::clock(seconds as f64)),
        None => words,
    }
}

/// "Not available: …" for what the server said of the address, where it
/// said something was wrong.
pub fn problem_words(why: &str) -> String {
    let plain = format::reason(why).map(t).unwrap_or(why);
    tf("Not available: {}.", &[&plain])
}

#[cfg(test)]
mod clock_tests {
    #[test]
    fn a_moment_reads_as_a_player_shows_it() {
        assert_eq!(super::clock(34), "0:34");
        assert_eq!(super::clock(725), "12:05");
        assert_eq!(super::clock(3723), "1:02:03");
    }
}

#[cfg(test)]
mod choice_tests {
    use super::{chip_words, default_choice};
    use dm_core::videopage::Choice;
    use dm_core::Quality;

    fn choice(quality: &str, label: &str, size: Option<u64>) -> Choice {
        Choice {
            label: label.into(),
            quality: quality.into(),
            size,
        }
    }

    fn list() -> Vec<Choice> {
        vec![
            choice("2160/mp4", "MP4 3840×2160", Some(900_000_000)),
            choice("1080/mp4", "MP4 1920×1080", Some(275_000_000)),
            choice("480/webm", "WebM 854×480", None),
            choice("audio/m4a", "M4A audio", Some(4_000_000)),
        ]
    }

    #[test]
    fn the_setting_picks_the_tallest_it_allows_or_the_sound() {
        let list = list();
        let pick = |q| default_choice(&list, q);
        assert_eq!(pick(Quality::Best).as_deref(), Some("2160/mp4"));
        assert_eq!(pick(Quality::UpTo(1080)).as_deref(), Some("1080/mp4"));
        assert_eq!(pick(Quality::UpTo(720)).as_deref(), Some("480/webm"));
        // Nothing that short: the tallest there is rather than nothing.
        assert_eq!(pick(Quality::UpTo(144)).as_deref(), Some("2160/mp4"));
        assert_eq!(pick(Quality::Audio).as_deref(), Some("audio/m4a"));
        assert_eq!(default_choice(&[], Quality::Best), None);
        // A list of only labels: its first.
        let odd = [choice("hls", "Stream", None)];
        assert_eq!(default_choice(&odd, Quality::Audio).as_deref(), Some("hls"));
        assert_eq!(
            default_choice(&odd, Quality::UpTo(720)).as_deref(),
            Some("hls")
        );
    }

    #[test]
    fn a_chip_says_the_class_and_container_over_the_size() {
        let chip = chip_words(&choice("1080/mp4", "MP4 1920×1080", Some(2048)));
        assert!(chip.starts_with("1080p"), "{chip}");
        assert!(chip.contains("MP4\n"), "{chip}");
        assert!(chip.ends_with(&crate::format::bytes(2048)), "{chip}");
        let sound = chip_words(&choice("audio/m4a", "M4A audio", None));
        assert!(sound.ends_with("M4A"), "{sound}");
        assert!(!sound.contains('\n'), "no size, no second line: {sound}");
        // Not a height: the list's own words.
        assert_eq!(
            chip_words(&choice("hls", "Live stream", None)),
            "Live stream"
        );
    }
}

#[cfg(test)]
mod playlist_tests {
    use super::*;
    use dm_core::youtube::{Playlist, PlaylistEntry};

    fn list() -> Playlist {
        let entry = |id: &str, title: &str, seconds| PlaylistEntry {
            id: id.into(),
            title: title.into(),
            seconds,
            page: None,
        };
        Playlist {
            id: "PL1".into(),
            title: "Talks".into(),
            entries: vec![entry("a1", "Intro", Some(245)), entry("b2", "Outro", None)],
            cut_short: false,
        }
    }

    #[test]
    fn the_ticked_videos_are_added_at_the_quality_with_the_page_as_referer() {
        let list = list();
        let (urls, with) =
            playlist_adds(&list, &[false, true], "720", "https://p/l", true).unwrap();
        assert_eq!(urls, vec![list.entries[1].url()]);
        assert!(with.later);
        assert_eq!(with.queue, dm_core::MAIN_QUEUE);
        assert_eq!(with.quality.as_deref(), Some("720"));
        assert_eq!(with.referrer, "https://p/l");
        let (_, now) = playlist_adds(&list, &[true, true], "", "p", false).unwrap();
        assert_eq!((now.queue.as_str(), now.quality), ("", None));
        assert_eq!(
            playlist_adds(&list, &[false, false], "best", "p", false).unwrap_err(),
            "Pick at least one video."
        );
    }

    #[test]
    fn a_playlist_says_how_many_and_each_video_its_number_and_length() {
        let mut list = list();
        assert_eq!(playlist_count_words(&list), "2 videos");
        list.cut_short = true;
        assert_eq!(
            playlist_count_words(&list),
            "2 videos · Only the first 2 videos are listed."
        );
        assert_eq!(playlist_entry_words(0, &list.entries[0]), "1. Intro · 4:05");
        assert_eq!(playlist_entry_words(1, &list.entries[1]), "2. Outro");
    }
}
