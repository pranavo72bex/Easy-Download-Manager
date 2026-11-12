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

/// A Deno that can run YouTube's player challenges, if any: one the person
/// installed (or one an earlier dm fetched).
pub(crate) fn deno() -> Option<PathBuf> {
    which("deno")
}

/// The command that runs the JavaScript program in `file` with `engine`: Deno with no
/// permissions at all (it then reaches neither files nor programs nor the network, which
/// QuickJS's command line does not prevent), or QuickJS as it is. One place for the flags, for
/// YouTube's challenges (`youtube::run_js`) and a proxy script (`pac::evaluate`).
pub(crate) fn js_command(engine: &Path, file: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(engine);
    let deno = engine
        .file_stem()
        .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case("deno"));
    if deno {
        command.args([
            "run",
            "--ext=js",
            "--no-code-cache",
            "--no-prompt",
            "--no-remote",
            "--no-lock",
            "--no-config",
            "--no-npm",
        ]);
    } else {
        // A classic script, not a module: QuickJS (quickjs-ng 0.17) takes a
        // `.js` file as a module by default, where the top-level `this` is
        // undefined, and YouTube's player, which looks for its global object
        // through `this`, threw "Cannot find global object".
        command.arg("--script");
    }
    command.arg(file);
    command
}

/// The QuickJS that can run YouTube's player challenges, if any: the one dm
/// fetched, or one installed.
pub(crate) fn quickjs() -> Option<PathBuf> {
    which("qjs")
}

/// The first `program` on `PATH`, or in the places package managers put it.
///
/// `PATH` alone is not enough: a program launched from the Dock, a desktop
/// entry, or by a browser's native messaging host inherits the session's
/// minimal `PATH` (`/usr/bin:/bin` and little else), so `/opt/homebrew/bin`
/// and `~/.local/bin` are invisible to it. The usual homes are searched after
/// `PATH`, so the tool a person installed is the tool the app finds.
///
/// `DM_` and the program's name (`DM_DENO`, `DM_QJS`) name the binary
/// outright; set to nothing, they say there is no such tool, which is how a
/// test pins down what dm does without one on a machine that has one.
fn which(program: &str) -> Option<PathBuf> {
    let override_var = format!("DM_{}", program.replace('-', "").to_ascii_uppercase());
    if let Some(named) = std::env::var_os(&override_var) {
        return (!named.is_empty())
            .then(|| PathBuf::from(named))
            .filter(|p| p.is_file());
    }
    // As a computer with nothing installed: only what dm fetched itself.
    // For checking the out-of-the-box route where tools are installed.
    if std::env::var_os("DM_ONLY_FETCHED_TOOLS").is_some_and(|v| !v.is_empty()) {
        return crate::tools::fetched(program);
    }
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    let home = dirs::home_dir().unwrap_or_default();
    let usual = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        home.join(".local/bin"),
        home.join("bin"),
        PathBuf::from("/snap/bin"),
        home.join("Library/Python/3.13/bin"),
        home.join("Library/Python/3.12/bin"),
    ];
    let windows = if cfg!(windows) {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        windows_homes(&Homes {
            local: var("LOCALAPPDATA"),
            home,
            program_data: var("ProgramData"),
        })
    } else {
        Vec::new()
    };
    // Last, the copy dm fetched itself (`tools`): one the person installed
    // is theirs to choose.
    find_in(
        on_path.into_iter().chain(usual).chain(windows),
        &names(program, cfg!(windows)),
    )
    .or_else(|| crate::tools::fetched(program))
}

/// The file names a program goes by: on Windows its `.exe` (and `.com`),
/// which is what is on disk. The bare name was looked for there and never
/// found, so an installed tool was reported missing (issue 10).
fn names(program: &str, windows: bool) -> Vec<String> {
    if windows {
        vec![format!("{program}.exe"), format!("{program}.com")]
    } else {
        vec![program.to_string()]
    }
}

/// The first of `names` in the first of `dirs` that has one.
fn find_in(dirs: impl Iterator<Item = PathBuf>, names: &[String]) -> Option<PathBuf> {
    for dir in dirs {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The folders a Windows account's own environment names.
struct Homes {
    local: Option<PathBuf>,
    home: PathBuf,
    program_data: Option<PathBuf>,
}

/// The folders in `dir` whose names start with `prefix` (any case), newest
/// first where they are versioned: Python313 before Python312.
fn children(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let prefix = prefix.to_ascii_lowercase();
    let mut found: Vec<PathBuf> = listing
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(&prefix)
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found.reverse();
    found
}

/// Where Windows installs put Deno and QuickJS, beside `PATH`: winget's links
/// and its package folders (a build that keeps its program in a `bin` one level
/// down), Scoop's shims and Chocolatey's bin. A dm started by a browser or from
/// the Start menu has a `PATH` that may name none of them.
fn windows_homes(h: &Homes) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = &h.local {
        let winget = local.join("Microsoft").join("WinGet");
        dirs.push(winget.join("Links"));
        for package in children(&winget.join("Packages"), "") {
            dirs.push(package.join("bin"));
            for inner in children(&package, "") {
                dirs.push(inner.join("bin"));
                dirs.push(inner);
            }
            dirs.push(package);
        }
    }
    dirs.push(h.home.join("scoop").join("shims"));
    dirs.push(h.home.join(".local").join("bin"));
    if let Some(data) = &h.program_data {
        dirs.push(data.join("chocolatey").join("bin"));
    }
    dirs
}

/// Writes `text` to `path` readable by this account alone.
pub(crate) async fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    let _ = tokio::fs::remove_file(path).await;
    let mut open = tokio::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    open.mode(0o600);
    let mut file = open.open(path).await?;
    tokio::io::AsyncWriteExt::write_all(&mut file, text.as_bytes()).await?;
    // A tokio file finishes its writes in the background: flushed, the
    // text is in the file before the program that reads it is started (on
    // macOS a reader at once found the old, or no, text).
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(())
}

/// Removes a file when it goes out of scope: a run's private cookies file,
/// whichever way the run ends.
pub(crate) struct RemoveOnDrop(pub(crate) PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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
            // Matroska where it was asked for, or where the picture and
            // sound are a pair no MP4 holds (VP8 or Vorbis beside an MP4's
            // samples), rather than refused. A part or a reshaped picture
            // is made by the MP4 writer alone, as before.
            let mkv = !webm
                && cut.is_none()
                && options.shape.is_none()
                && (options.container.as_deref() == Some("mkv")
                    || crate::mkvmux::needs_matroska(&parts));
            let joined = produced.with_extension(if webm {
                "webm"
            } else if mkv {
                "mkv"
            } else {
                "mp4"
            });
            let joining = if webm || mkv {
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
mod windows_homes_tests {
    use super::{find_in, names, windows_homes, Homes};
    use std::path::PathBuf;

    /// On Windows the program is `qjs.exe`, installed where winget, Scoop or
    /// Chocolatey put it, none of them on the `PATH` a dm started by a
    /// browser has. Each is found there, laid out as a Windows profile is
    /// under a folder of this test's.
    #[test]
    fn windows_installs_are_found_by_their_exe() {
        let root = std::env::temp_dir().join(format!("dm-win-homes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (local, home, data) = (
            root.join("Local"),
            root.join("me"),
            root.join("ProgramData"),
        );
        let homes = Homes {
            local: Some(local.clone()),
            home: home.clone(),
            program_data: Some(data.clone()),
        };
        let put = |dir: PathBuf, name: &str| {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), b"MZ").unwrap();
            dir.join(name)
        };
        let exe = names("qjs", true);
        assert_eq!(exe[0], "qjs.exe");
        let find = |h: &Homes| find_in(windows_homes(h).into_iter(), &exe);
        assert_eq!(find(&homes), None);

        // winget's link, then its package folder alone.
        let winget = local.join("Microsoft").join("WinGet");
        let link = put(winget.join("Links"), "qjs.exe");
        assert_eq!(find(&homes), Some(link.clone()));
        std::fs::remove_file(&link).unwrap();
        let package = put(
            winget
                .join("Packages")
                .join("quickjs-ng.quickjs_8wekyb3d8bbwe"),
            "qjs.exe",
        );
        assert_eq!(find(&homes), Some(package.clone()));
        std::fs::remove_file(&package).unwrap();
        // Scoop, then Chocolatey.
        let scoop = put(home.join("scoop").join("shims"), "qjs.exe");
        assert_eq!(find(&homes), Some(scoop.clone()));
        std::fs::remove_file(&scoop).unwrap();
        let choco = put(data.join("chocolatey").join("bin"), "qjs.exe");
        assert_eq!(find(&homes), Some(choco));

        // Elsewhere the bare name, as before.
        assert_eq!(names("qjs", false), ["qjs"]);
        let _ = std::fs::remove_dir_all(&root);
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

    /// A VP8 picture beside an MP4's sound, which no MP4 holds, is saved as
    /// an MKV; a pair an MP4 holds is an MP4 still, unless MKV was asked.
    #[tokio::test]
    async fn a_pair_no_mp4_holds_is_saved_as_an_mkv() {
        let dir = tempfile::tempdir().unwrap();
        crate::mkvmux::tests::tiny_webm(&dir.path().join("v.f1.webm"), "V_VP8", 1, 5);
        std::fs::write(
            dir.path().join("v.f140.m4a"),
            fixture("media/mp4mux/sound.m4a"),
        )
        .unwrap();
        let done = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &options(dir.path()))
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mkv");
        let data = std::fs::read(&done.path).unwrap();
        assert!(has(&data, b"V_VP8") && has(&data, b"A_AAC"));

        let dir = tempfile::tempdir().unwrap();
        let both = |dir: &Path| {
            std::fs::write(dir.join("v.f137.mp4"), fixture("media/mp4mux/picture.mp4")).unwrap();
            std::fs::write(dir.join("v.f140.m4a"), fixture("media/mp4mux/sound.m4a")).unwrap();
        };
        both(dir.path());
        let done = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &options(dir.path()))
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mp4", "as before");

        let dir = tempfile::tempdir().unwrap();
        both(dir.path());
        let mut asked = options(dir.path());
        asked.container = Some("mkv".into());
        let done = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &asked)
            .await
            .unwrap();
        assert_eq!(done.path.extension().unwrap(), "mkv");
        assert!(has(&std::fs::read(&done.path).unwrap(), b"V_MPEG4/ISO/AVC"));
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

    /// A private file replaces what was there, readable by this account
    /// alone.
    #[tokio::test]
    async fn a_private_file_replaces_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.js");
        std::fs::write(&file, "old").unwrap();
        write_private(&file, "new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(write_private(&dir.path().join("no/such/dir.js"), "x")
            .await
            .is_err());
        drop(RemoveOnDrop(file.clone()));
        assert!(!file.exists());
    }

    /// A Deno is run with no permissions at all; any other engine with the
    /// program alone.
    #[test]
    fn a_deno_is_given_its_flags_and_another_engine_none() {
        let args = |engine: &str| {
            js_command(Path::new(engine), Path::new("/p.js"))
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let deno = args("/usr/bin/Deno");
        assert_eq!(deno.first().map(String::as_str), Some("run"));
        assert!(deno.contains(&"--no-remote".to_string()));
        assert_eq!(deno.last().map(String::as_str), Some("/p.js"));
        // QuickJS is told the program is a classic script, whose top-level
        // `this` is the global object (it takes a `.js` file as a module).
        assert_eq!(args("/usr/bin/qjs"), ["--script", "/p.js"]);
        assert!(is_page(Some("text/html; charset=utf-8")));
        assert!(!is_page(Some("video/mp4")));
        assert!(!is_page(None));
    }

    /// A winget package that keeps its program a folder down, in a `bin`
    /// or not, is found there.
    #[test]
    fn a_winget_package_a_folder_down_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let homes = Homes {
            local: Some(dir.path().join("Local")),
            home: dir.path().join("me"),
            program_data: None,
        };
        let package = dir
            .path()
            .join("Local/Microsoft/WinGet/Packages/denoland.deno");
        let inner = package.join("deno-2.1");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("deno.exe"), b"MZ").unwrap();
        let exe = names("deno", true);
        assert_eq!(
            find_in(windows_homes(&homes).into_iter(), &exe),
            Some(inner.join("deno.exe"))
        );
        std::fs::create_dir_all(inner.join("bin")).unwrap();
        std::fs::write(inner.join("bin/deno.exe"), b"MZ").unwrap();
        assert_eq!(
            find_in(windows_homes(&homes).into_iter(), &exe),
            Some(inner.join("bin/deno.exe")),
            "its bin first"
        );
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

    /// Versioned folders are listed newest first, and only those with the
    /// prefix asked.
    #[test]
    fn versioned_folders_are_listed_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Python312", "Python313", "Ruby"] {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        }
        std::fs::write(dir.path().join("python399"), b"a file").unwrap();
        assert_eq!(
            children(dir.path(), "python"),
            [dir.path().join("Python313"), dir.path().join("Python312")]
        );
        assert!(children(&dir.path().join("none"), "").is_empty());
    }
}
