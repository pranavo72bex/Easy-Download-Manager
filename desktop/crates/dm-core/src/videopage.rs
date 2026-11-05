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
