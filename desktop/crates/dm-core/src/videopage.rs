// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Downloading from sites that serve video through their own player.
//!
//! YouTube is read by dm itself (`crate::youtube`): the same requests a
//! browser makes, and the public-domain solver scripts for the player's
//! `n` values (run by QuickJS, fetched as a tool when needed). Every
//! other site's video comes from the browser extension's caught player
//! requests — dm never reads those pages itself.
//!
//! This module holds the data types (`Read`, `Plan`, `Stream`), the
//! fetch/merge pipeline that turns a reading into a finished file, the
//! quality choices panel, playlist reading, and the readings cache.

use std::path::{Path, PathBuf};

use crate::download::{Finished, Options};
use crate::Error;

mod choices;
mod fetch;
mod page_data;
mod playlist;

pub use choices::{choices, class_name, list_choices, list_choices_fast, with_mp3, Choice};
pub use fetch::download_page;
pub use playlist::{playlist_of, read_playlist};
/// Whether `url` is a YouTube page whose own data lists its qualities
/// (`list_choices_fast`).
pub fn is_watch_page(url: &str) -> bool {
    page_data::is_watch_page(url)
}

/// Whether a response is a web page rather than a file dm fetches itself.
///
/// A file has a body dm can range over; a web page does not, and saving the
/// page's HTML as though it were the download is the wrong answer whether or
/// not a reader is there to do better. So this is decided on the content type
/// alone, and the caller decides between reading it as a video page and refusing.
pub fn is_page(content_type: Option<&str>) -> bool {
    crate::probe::mime_essence(content_type) == "text/html"
}

/// The picture and the sound left apart, beside the name they are joined under,
/// `name.f137.mp4` and `name.f140.m4a`, picture first.
async fn parts_of(joined: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (joined.parent(), joined.file_stem()) else {
        return Vec::new();
    };
    let prefix = format!("{}.f", stem.to_string_lossy());
    let Ok(mut listing) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    while let Ok(Some(entry)) = listing.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let media = [".mp4", ".m4a", ".m4v", ".mov", ".webm"]
            .iter()
            .any(|ext| name.to_ascii_lowercase().ends_with(ext));
        if name.starts_with(&prefix) && media {
            parts.push(entry.path());
        }
    }
    // Picture before sound, as players expect the first track to be.
    parts.sort_by_key(|p| {
        let audio = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("m4a"));
        (audio, p.clone())
    });
    parts
}

/// What dm does with a page's fetched streams: joins the picture and sound left
/// apart, puts a transport stream into an MP4, carries the subtitles
/// inside, and places the result.
async fn finish_dm(
    produced: &Path,
    texts: Vec<crate::subtitles::Subtitle>,
    options: &Options,
) -> Result<Finished, Error> {
    let produced = produced.to_path_buf();
    // A part of the video: the pieces fetched hold a little around it, and
    // the joined file's edit lists play the part alone (`clip`).
    let cut = options.span.map(|span| span.millis());
    if !produced.is_file() {
        let parts = parts_of(&produced).await;
        if !parts.is_empty() {
            // A WebM where one was asked for; else an MP4, which takes a
            // WebM picture or sound too.
            let webm = options.container.as_deref() == Some("webm")
                && parts.iter().all(|p| {
                    p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("webm"))
                });
            if webm && cut.is_some() {
                return Err(Error::Internal(
                    "a part is cut from MP4 streams only, not WebM".into(),
                ));
            }
            let joined = produced.with_extension(if webm { "webm" } else { "mp4" });
            let joining = if webm {
                crate::mkvmux::join_with_text_async(parts.clone(), texts, joined.clone()).await
            } else {
                crate::mp4mux::join_cut_async(
                    parts.clone(),
                    texts,
                    joined.clone(),
                    cut,
                    options.shape,
                )
                .await
            };
            joining.map_err(|why| {
                Error::Internal(format!("the picture and sound could not be joined: {why}"))
            })?;
            for part in parts {
                let _ = tokio::fs::remove_file(part).await;
            }
            return place(&joined, options).await;
        }
    } else if let Some(cut) = cut {
        // One stream's part alone (the sound): rewritten with the cut.
        let ext = produced
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mp4".into());
        let cut_file = produced.with_extension(format!("dm-cut.{ext}"));
        crate::mp4mux::join_cut_async(
            vec![produced.clone()],
            texts,
            cut_file.clone(),
            Some(cut),
            options.shape,
        )
        .await
        .map_err(|why| Error::Internal(format!("the part could not be cut: {why}")))?;
        tokio::fs::rename(&cut_file, &produced)
            .await
            .map_err(Error::Disk)?;
        return place(&produced, options).await;
    }
    /* An HLS site's video, fetched by a stream downloader, is the
     * stream's transport stream under an `.mp4` name: dm puts it right. */
    if is_transport_stream(&produced).await {
        let fixed = produced.with_extension("dm-remux.mp4");
        if crate::tsmux::to_mp4_async(produced.clone(), fixed.clone())
            .await
            .is_ok()
        {
            let _ = tokio::fs::rename(&fixed, &produced).await;
        } else {
            let _ = tokio::fs::remove_file(&fixed).await;
        }
    }
    /* One file that already held picture and sound, with subtitles beside
     * it: written again with them inside. A video that will not take them
     * is kept as it is; the subtitles are not worth the video. */
    let kind = produced
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !texts.is_empty() && ["mp4", "m4v", "webm", "mkv"].contains(&kind.as_str()) {
        let with = produced.with_extension(format!("dm-subtitles.{kind}"));
        let written = if kind == "webm" || kind == "mkv" {
            crate::mkvmux::join_with_text_async(vec![produced.clone()], texts, with.clone()).await
        } else {
            crate::mp4mux::join_with_text_async(vec![produced.clone()], texts, with.clone()).await
        };
        if written.is_ok() {
            let _ = tokio::fs::rename(&with, &produced).await;
        } else {
            let _ = tokio::fs::remove_file(&with).await;
        }
    }
    place(&produced, options).await
}

/// Whether a file is an MPEG transport stream, whatever its name: a sync
/// byte at the start of its first two packets.
async fn is_transport_stream(path: &Path) -> bool {
    use tokio::io::AsyncReadExt;
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut head = [0u8; 189];
    file.read_exact(&mut head).await.is_ok() && head[0] == 0x47 && head[188] == 0x47
}

/// Moves a finished video file into its category folder under the download
/// directory, the same placement every other download gets.
async fn place(produced: &Path, options: &Options) -> Result<Finished, Error> {
    // The fetch names the file it meant to leave; if it is not there, the usual
    // cause is a join that never happened, and "No such file" says nothing.
    if !produced.is_file() {
        return Err(Error::Internal(
            "the video was fetched but the file it named is missing".into(),
        ));
    }
    let name = produced
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());

    /* Placed by the same code as every other download — the category
     * folder, a name nothing else has, a rename that falls back to a copy
     * across filesystems, the digest checked and a failing file removed.
     * This used to be a second copy of all of that, one rule at a time. */
    let (dest, bytes, category) =
        crate::download::finish(&name, None, &[produced.to_path_buf()], options).await?;
    let _ = tokio::fs::remove_dir_all(&options.parts_dir).await;

    Ok(Finished {
        left_behind: None,
        path: dest,
        bytes,
        // The streams manage their own connections; the column would be guessing.
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 0,
    })
}

/// A cancelled run's staging folder, removed. Tried again for a few
/// seconds: on Windows a file still open by a process that is only now
/// ending cannot be deleted, and the first try left the folder behind.
async fn remove_staging(dir: &Path) {
    for attempt in 0..15 {
        match tokio::fs::remove_dir_all(dir).await {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) if attempt < 14 => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    /// A stream whose renditions name no codecs, as a player
    /// reads an HLS master playlist without CODECS, still lists its heights.
    #[test]
    fn renditions_without_codecs_are_listed_by_height() {
        let described = serde_json::json!({ "formats": [
            { "format_id": "hls-360", "ext": "mp4", "height": 360, "width": 640, "vcodec": null, "acodec": null },
            { "format_id": "hls-720", "ext": "mp4", "height": 720, "width": 1280 },
            { "format_id": "a", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2" },
        ]})
        .to_string();
        let heights: Vec<String> = choices(&described).into_iter().map(|c| c.quality).collect();
        assert_eq!(heights, ["720/mp4", "360/mp4", "audio/m4a"]);
    }

    #[tokio::test]
    async fn a_cancelled_runs_staging_is_removed_and_a_missing_one_is_no_wait() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("parts");
        std::fs::create_dir_all(staging.join("deeper")).unwrap();
        std::fs::write(staging.join("deeper/a.part"), b"x").unwrap();
        super::remove_staging(&staging).await;
        assert!(!staging.exists());
        let at = std::time::Instant::now();
        super::remove_staging(&staging).await;
        assert!(at.elapsed() < std::time::Duration::from_millis(150));
    }

    use super::*;
    use crate::stream::Quality;

    #[test]
    fn the_list_offers_mp4_where_there_is_one_as_youtube_lists_webm_bigger() {
        // YouTube's real shape: VP9 WebM bigger than H.264 MP4 at a height,
        // Opus sound at a higher bitrate than M4A, and 4K only in WebM.
        let described = serde_json::json!({"formats": [
            {"ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.0, "filesize": 1_000},
            {"ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 140.0, "filesize": 1_100},
            {"ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1920, "height": 1080, "filesize": 9_000},
            {"ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 1920, "height": 1080, "filesize": 12_000},
            {"ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 3840, "height": 2160, "filesize": 40_000},
        ]})
        .to_string();
        let list = choices(&described);
        let labels: Vec<&str> = list.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["4K WEBM", "1080p MP4", "M4A audio"]);
        assert_eq!(list[1].quality, "1080/mp4");
        assert_eq!(list[1].size, Some(10_000), "joined with the M4A sound");
    }

    #[test]
    fn a_pages_formats_become_one_choice_per_height_and_the_sound_alone() {
        let described = serde_json::json!({"formats": [
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.5, "filesize": 1_000},
            {"format_id": "249", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 50.0, "filesize": 400},
            {"format_id": "18", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "width": 640, "height": 360, "filesize": 5_000},
            {"format_id": "136", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720, "filesize": 9_000},
            {"format_id": "247", "ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 1280, "height": 720, "filesize_approx": 7_000},
            {"format_id": "sb0", "ext": "mhtml", "vcodec": "none", "acodec": "none", "height": 90},
        ]})
        .to_string();
        let list = choices(&described);
        let labels: Vec<&str> = list.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["720p MP4", "360p MP4", "M4A audio"]);
        assert_eq!(list[0].quality, "720/mp4");
        assert_eq!(
            list[0].size,
            Some(10_000),
            "the picture and the sound it is joined with"
        );
        assert_eq!(list[1].size, Some(5_000), "already carries its sound");
        assert_eq!(list[2].quality, "audio/m4a");
        assert_eq!(Quality::named(&list[0].quality), Quality::UpTo(720));
        assert_eq!(Quality::named(&list[2].quality), Quality::Audio);
        assert_eq!(
            Quality::container_of(&list[0].quality).as_deref(),
            Some("mp4")
        );
        assert_eq!(Quality::container_of("720"), None);
        assert_eq!(Quality::container_of("720/../x"), None, "only a plain word");
        assert!(choices("not json").is_empty());
        assert!(choices("{}").is_empty());
    }

    /// A height with a picture above 30 frames a second and one at or below
    /// is two lines, as reference A lists "720p HD 60 fps" and "720p HD"; a
    /// height with one frame rate is one line, as before (gaps row 72d).
    #[test]
    fn a_height_at_two_frame_rates_is_two_lines() {
        let described = serde_json::json!({"formats": [
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 129.5, "filesize": 1_000},
            {"format_id": "136", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720, "fps": 30, "filesize": 9_000},
            {"format_id": "298", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720, "fps": 60, "filesize": 14_000},
            {"format_id": "135", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 854, "height": 480, "fps": 30, "filesize": 4_000},
        ]})
        .to_string();
        let list = choices(&described);
        let lines: Vec<(&str, &str)> = list
            .iter()
            .map(|c| (c.label.as_str(), c.quality.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                ("720p MP4 60 fps", "720/mp4/60"),
                ("720p MP4", "720/mp4/30"),
                ("480p MP4", "480/mp4"),
                ("M4A audio", "audio/m4a"),
            ]
        );
        assert_eq!(list[0].size, Some(15_000));
        assert_eq!(Quality::named("720/mp4/60"), Quality::UpTo(720));
        assert_eq!(Quality::container_of("720/mp4/60").as_deref(), Some("mp4"));
        assert_eq!(Quality::frame_rate_of("720/mp4/60"), Some(60));
        assert_eq!(Quality::frame_rate_of("720/mp4"), None);
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use crate::{Categories, Control, Limiter};

    fn options(dir: &Path) -> Options {
        Options {
            connections: 4,
            control: Control::new(),
            dest_dir: dir.join("out"),
            parts_dir: dir.join("parts"),
            limiter: Limiter::unlimited(),
            room: Default::default(),
            categories: Categories::default(),
            checksum: None,
            quality: crate::stream::Quality::Best,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            media: false,
            referrer: None,
            proxy: None,
            container: None,
            attempts: 1,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            sign_in: None,
            save_name: None,
            accept_page: false,
            torrent: Default::default(),
            system_proxy: false,
            title_hint: None,
            remux: false,
            server_dates: false,
            retry_wait: 0,
            record_limit: None,
            frame_rate: None,
            caption: None,
        }
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap()
    }

    fn hello() -> Vec<crate::subtitles::Subtitle> {
        vec![crate::subtitles::Subtitle {
            language: "en".into(),
            cues: crate::subtitles::parse_vtt("WEBVTT\n\n00:00.000 --> 00:00.500\nhello there\n"),
        }]
    }

    fn has(data: &[u8], what: &[u8]) -> bool {
        data.windows(what.len()).any(|w| w == what)
    }

    /// A part is cut from MP4 streams only: WebM ones asked to be joined as
    /// WebM with a span are refused in words.
    #[tokio::test]
    async fn a_part_of_webm_streams_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("v.f247.webm"),
            fixture("webm/picture.f247.webm"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("v.f251.webm"),
            fixture("webm/sound.f251.webm"),
        )
        .unwrap();
        let mut opts = options(dir.path());
        opts.container = Some("webm".into());
        opts.span = crate::clip::Span::new(0.0, 0.5);
        let why = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &opts)
            .await
            .unwrap_err()
            .to_string();
        assert!(why.contains("a part is cut from MP4 streams only"), "{why}");
    }

    /// Streams that are not media, joined or cut, fail in words rather than
    /// leaving a broken file placed as the download.
    #[tokio::test]
    async fn streams_that_cannot_be_joined_or_cut_fail_in_words() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("v.f137.mp4"), b"not a picture").unwrap();
        std::fs::write(dir.path().join("v.f140.m4a"), b"not a sound").unwrap();
        // Neither of these is a part of `v`.
        std::fs::write(dir.path().join("v.f1.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("w.f1.mp4"), b"x").unwrap();
        assert_eq!(
            parts_of(&dir.path().join("v.mp4")).await,
            [dir.path().join("v.f137.mp4"), dir.path().join("v.f140.m4a")],
            "the picture first"
        );
        let opts = options(dir.path());
        let why = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &opts)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            why.contains("the picture and sound could not be joined"),
            "{why}"
        );

        let single = dir.path().join("s.m4a");
        std::fs::write(&single, b"not a sound either").unwrap();
        let mut cut = options(dir.path());
        cut.span = crate::clip::Span::new(0.0, 0.5);
        let why = finish_dm(&single, Vec::new(), &cut)
            .await
            .unwrap_err()
            .to_string();
        assert!(why.contains("the part could not be cut"), "{why}");
    }

    /// A transport stream under an `.mp4` name is put into an MP4; one that
    /// only starts like one is kept as it came.
    #[tokio::test]
    async fn a_transport_stream_is_put_into_an_mp4() {
        let dir = tempfile::tempdir().unwrap();
        let produced = dir.path().join("t.mp4");
        std::fs::write(&produced, fixture("media/mp4mux/stream.ts")).unwrap();
        let done = finish_dm(&produced, Vec::new(), &options(dir.path()))
            .await
            .unwrap();
        let data = std::fs::read(&done.path).unwrap();
        assert_eq!(&data[4..8], b"ftyp", "an MP4 now");
        assert_eq!(done.connections, 1);
        assert!(!done.verified);

        let odd = dir.path().join("odd.mp4");
        let mut bytes = vec![0u8; 400];
        bytes[0] = 0x47;
        bytes[188] = 0x47;
        std::fs::write(&odd, &bytes).unwrap();
        let done = finish_dm(&odd, Vec::new(), &options(dir.path()))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&done.path).unwrap(), bytes);
        assert!(!dir.path().join("odd.dm-remux.mp4").exists());
        assert!(!is_transport_stream(&dir.path().join("none.ts")).await);
    }

    /// Subtitles beside a video that already holds picture and sound are
    /// written into it, MP4 or WebM; a video that will not take them is
    /// kept as it is.
    #[tokio::test]
    async fn subtitles_go_inside_one_file_or_it_is_kept_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = dir.path().join("m.mp4");
        std::fs::write(&mp4, fixture("mp4/picture.f137.mp4")).unwrap();
        let done = finish_dm(&mp4, hello(), &options(dir.path()))
            .await
            .unwrap();
        let data = std::fs::read(&done.path).unwrap();
        assert!(has(&data, b"tx3g") && has(&data, b"hello there"));

        let webm = dir.path().join("w.webm");
        std::fs::write(&webm, fixture("webm/picture.f247.webm")).unwrap();
        let done = finish_dm(&webm, hello(), &options(dir.path()))
            .await
            .unwrap();
        assert!(has(&std::fs::read(&done.path).unwrap(), b"hello there"));

        let broken = dir.path().join("b.mp4");
        std::fs::write(&broken, b"no boxes here").unwrap();
        let done = finish_dm(&broken, hello(), &options(dir.path()))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&done.path).unwrap(), b"no boxes here");
        assert!(!dir.path().join("b.dm-subtitles.mp4").exists());
    }

    /// A fetch that left nothing under the name it gave is said as that,
    /// not as "No such file".
    #[tokio::test]
    async fn a_missing_file_is_said_in_words() {
        let dir = tempfile::tempdir().unwrap();
        for produced in [dir.path().join("gone.mp4"), PathBuf::from("/")] {
            let why = finish_dm(&produced, Vec::new(), &options(dir.path()))
                .await
                .unwrap_err()
                .to_string();
            assert!(why.contains("the file it named is missing"), "{why}");
        }
        assert!(parts_of(Path::new("/")).await.is_empty());
        assert!(parts_of(&dir.path().join("nowhere/v.mp4")).await.is_empty());
    }

    /// A page is told from a stream by its type.
    #[test]
    fn a_page_is_told_by_its_type() {
        assert!(is_page(Some("text/html; charset=utf-8")));
        assert!(!is_page(Some("video/mp4")));
        assert!(!is_page(None));
    }

    /// A staging folder that cannot be removed is tried for a few seconds,
    /// then left: a cancel does not hang on it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_staging_that_cannot_be_removed_is_given_up_on() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("parts/locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("a.part"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        // Root may remove it anyway; then there is nothing to wait for.
        let at = std::time::Instant::now();
        remove_staging(&dir.path().join("parts")).await;
        let held = locked.join("a.part").exists();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        if held {
            assert!(
                at.elapsed() >= std::time::Duration::from_secs(2),
                "it tried again"
            );
            assert!(
                at.elapsed() < std::time::Duration::from_secs(5),
                "then gave up"
            );
        }
    }
}
