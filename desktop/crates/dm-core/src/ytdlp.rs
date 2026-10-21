//! Downloading from sites that serve video through their own player.
//!
//! dm fetches files and HLS itself; what it cannot do is turn a page like a
//! video site's watch URL into a file, because the address of the media is
//! not in the page — it is minted by the site's own player at play time.
//! `yt-dlp` is the tool that knows how, for a thousand sites, and it is the
//! one dm hands those to. dm does not reimplement any of that: it runs the
//! yt-dlp the user installed, the way a person would at a shell, and places
//! what it produces like any other download.
//!
//! So this is deliberately a thin wrapper around a child process — no site
//! logic, no token minting, no signature work of dm's own — and it is used
//! only when the URL turned out to be a web page rather than a file and a
//! yt-dlp is actually on the machine.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::download::{Finished, Options, Update};
use crate::stream::Quality;
use crate::{category, Error};

/// The line prefix yt-dlp is told to stamp on its progress, so dm can pick
/// its own numbers out of everything else the tool prints.
const MARK: &str = "@dm@";

/// Whether a yt-dlp is on the machine to hand pages to.
///
/// Checked rather than assumed: dm works without one, and a page it cannot
/// hand anywhere is reported as such rather than failing to launch a program
/// that is not there.
pub fn available() -> bool {
    which("yt-dlp").is_some()
}

/// Where yt-dlp actually is, resolved the same way `available` decides.
pub(crate) fn binary() -> Option<PathBuf> {
    which("yt-dlp")
}

/// The ffmpeg dm can run, if any: for joining picture and sound, and for
/// putting a transport stream into an MP4.
pub(crate) fn ffmpeg() -> Option<PathBuf> {
    which("ffmpeg")
}

/// The first `yt-dlp` on `PATH`, or in the places package managers put it.
///
/// `PATH` alone is not enough, and this is the bug that made "YouTube does
/// not download" true on a machine with yt-dlp installed. A program launched
/// from the Dock, a desktop entry, or by a browser's native messaging host
/// inherits the session's minimal `PATH` — `/usr/bin:/bin` and little else —
/// not the shell's, so `/opt/homebrew/bin` and `~/.local/bin` are invisible to
/// it even though `which yt-dlp` at a terminal finds it fine. The usual homes
/// are searched after `PATH`, so the tool a person installed is the tool the
/// app finds.
///
/// `DM_YTDLP` (and `DM_FFMPEG`) name the binary outright, for an install in
/// none of those places; set to nothing, they say there is no such tool,
/// which is how a test pins down what dm does without one on a machine that
/// has one.
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
            roaming: var("APPDATA"),
            home: home.clone(),
            program_data: var("ProgramData"),
            program_files: var("ProgramFiles"),
            system_drive: var("SystemDrive"),
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
/// found, so an installed yt-dlp was reported missing (issue 10).
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
    roaming: Option<PathBuf>,
    home: PathBuf,
    program_data: Option<PathBuf>,
    program_files: Option<PathBuf>,
    system_drive: Option<PathBuf>,
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

/// Where Windows installs put yt-dlp and ffmpeg, beside `PATH`: winget's
/// links and its package folders (an ffmpeg build keeps its own in a `bin`
/// one level down), pip's Scripts folders for python.org's and the Store's
/// Pythons, Scoop's shims, pipx's folder, Chocolatey's bin, and the folders
/// people unpack ffmpeg into by hand. A dm started by a browser or from the
/// Start menu has a `PATH` that may name none of them.
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
        for python in children(&local.join("Programs").join("Python"), "python") {
            dirs.push(python.join("Scripts"));
        }
        for store in children(&local.join("Packages"), "pythonsoftwarefoundation.python") {
            for python in children(&store.join("LocalCache").join("local-packages"), "python") {
                dirs.push(python.join("Scripts"));
            }
        }
    }
    if let Some(roaming) = &h.roaming {
        for python in children(&roaming.join("Python"), "python") {
            dirs.push(python.join("Scripts"));
        }
    }
    dirs.push(h.home.join("scoop").join("shims"));
    dirs.push(h.home.join(".local").join("bin"));
    if let Some(data) = &h.program_data {
        dirs.push(data.join("chocolatey").join("bin"));
    }
    if let Some(files) = &h.program_files {
        dirs.push(files.join("ffmpeg").join("bin"));
        dirs.push(files.join("yt-dlp"));
    }
    if let Some(drive) = &h.system_drive {
        let root = PathBuf::from(format!("{}\\", drive.display()));
        dirs.push(root.join("ffmpeg").join("bin"));
        dirs.push(root.join("yt-dlp"));
    }
    dirs
}

/// Writes `text` to `path` readable by this account alone.
async fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    let _ = tokio::fs::remove_file(path).await;
    let mut open = tokio::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    open.mode(0o600);
    let mut file = open.open(path).await?;
    tokio::io::AsyncWriteExt::write_all(&mut file, text.as_bytes()).await?;
    Ok(())
}

/// Hands yt-dlp the proxy in a config file of the run's own under `dir`,
/// readable by this account alone and removed when the run ends. On the
/// command line, the proxy's user name and password were there for any
/// local user's `ps` to read, as the cookies were before them.
async fn proxy_by_file(
    command: &mut Command,
    dir: &Path,
    proxy: &str,
) -> Result<RemoveOnDrop, Error> {
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("dm-proxy-{}-{run}.conf", std::process::id()));
    // yt-dlp splits its config as a POSIX shell does: single quotes keep
    // everything, and a quote inside is closed, escaped and reopened.
    let quoted = proxy.replace('\'', r"'\''");
    write_private(&path, &format!("--proxy '{quoted}'\n")).await?;
    command.arg("--config-locations").arg(&path);
    Ok(RemoveOnDrop(path))
}

/// Removes a file when it goes out of scope: a run's private cookies file,
/// whichever way the run ends.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Whether a response is a web page rather than a file dm fetches itself.
///
/// A file has a body dm can range over; a web page does not, and saving the
/// page's HTML as though it were the download is the wrong answer whether or
/// not a yt-dlp is there to do better. So this is decided on the content type
/// alone, and the caller decides between handing it to yt-dlp and refusing.
pub fn is_page(content_type: Option<&str>) -> bool {
    crate::probe::mime_essence(content_type) == "text/html"
}

/// The `-f` selector for a wanted quality, in yt-dlp's language.
///
/// The best picture and the best sound are usually separate streams, and
/// joining them is ffmpeg's job. Without an ffmpeg to join with, asking for a
/// pair gets two files and no video, so the choice falls to the best single
/// stream that already carries both — lower quality than a joined pair, but
/// a video that plays, which is the point.
fn format_for(quality: Quality, can_merge: bool) -> String {
    match (quality, can_merge) {
        (Quality::Best, true) => "bv*+ba/b".to_string(),
        (Quality::UpTo(height), true) => {
            format!("bv*[height<={height}]+ba/b[height<={height}]/b")
        }
        (Quality::Best, false) => "b".to_string(),
        (Quality::UpTo(height), false) => format!("b[height<={height}]/b"),
        // The best audio-only stream — what "audio" means for a video site.
        (Quality::Audio, _) => "ba/b".to_string(),
    }
}

/// yt-dlp's format-sort preference for the container a panel line named, so
/// what arrives is what the list said: MP4 picture with M4A sound, WebM with
/// WebM, or the named sound alone. With none named (Add URL, a page handed
/// over whole) it is MP4: left to itself
/// yt-dlp picks YouTube's WebM. An MP4 is joined into an MP4 even when the
/// best picture at that height is only in WebM, and H.264 is preferred over
/// AV1 at the same height, since it plays everywhere.
fn container_sort(container: Option<&str>) -> Vec<String> {
    let mp4 = || {
        [
            "-S",
            "res,ext:mp4:m4a,vcodec:h264",
            "--merge-output-format",
            "mp4",
        ]
        .map(String::from)
        .to_vec()
    };
    let sort = match container {
        Some("mp4") | None => return mp4(),
        Some("webm") => "ext:webm:webm",
        Some("m4a") => "ext:m4a:m4a",
        Some(other) if !other.is_empty() => return vec!["-S".into(), format!("ext:{other}")],
        _ => return mp4(),
    };
    vec!["-S".into(), sort.into()]
}

/// One choice the "Download video" panel offers:
/// a quality and the size it comes to.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Choice {
    /// What the list says, such as "MP4 1280×720" or "M4A audio".
    pub label: String,
    /// The quality to download it at, in `Quality::named`'s words.
    pub quality: String,
    /// Bytes, where yt-dlp knows or estimates them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// The choices in yt-dlp's `-J` description of a page: one per picture
/// height, tallest first, each with the biggest file at that height (and the
/// sound it would be joined with), then the best sound alone. Nothing it
/// cannot read is listed; garbage gives an empty list, not an error.
pub fn choices(described: &str) -> Vec<Choice> {
    let Ok(page) = serde_json::from_str::<serde_json::Value>(described) else {
        return Vec::new();
    };
    let formats = page
        .get("formats")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    let size_of = |f: &serde_json::Value| {
        f.get("filesize")
            .and_then(|v| v.as_u64())
            .or_else(|| f.get("filesize_approx").and_then(|v| v.as_u64()))
    };
    let has = |f: &serde_json::Value, key: &str| {
        f.get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|c| c != "none")
    };
    let is = |f: &serde_json::Value, ext: &str| f.get("ext").and_then(|v| v.as_str()) == Some(ext);
    // The sound on its own that a picture-only format is joined with: M4A
    // first, as MP4 is what the list offers first, then the best of the rest.
    let audio = formats
        .iter()
        .filter(|f| has(f, "acodec") && !has(f, "vcodec"))
        .max_by_key(|f| {
            (
                is(f, "m4a"),
                f.get("abr").and_then(|v| v.as_f64()).unwrap_or(0.0) as u64,
            )
        });
    let audio_size = audio.and_then(size_of);

    let mut by_height: Vec<(u32, String, String, Option<u64>)> = Vec::new();
    for f in &formats {
        let Some(height) = f.get("height").and_then(|v| v.as_u64()).map(|h| h as u32) else {
            continue;
        };
        // A picture where it has a height, unless yt-dlp says it has none:
        // an HLS master's renditions list no codecs, and come as
        // `vcodec: null`, which left a stream with no list (UI round 7).
        let no_picture = f.get("vcodec").and_then(|v| v.as_str()) == Some("none");
        if height == 0 || no_picture {
            continue;
        }
        let width = f.get("width").and_then(|v| v.as_u64()).unwrap_or(0);
        let ext = f.get("ext").and_then(|v| v.as_str()).unwrap_or("video");
        let label = if width > 0 {
            format!("{} {width}×{height}", ext.to_ascii_uppercase())
        } else {
            format!("{} {height}p", ext.to_ascii_uppercase())
        };
        // A picture alone is joined with the sound, so its size counts both.
        let size = match (size_of(f), has(f, "acodec")) {
            (Some(s), true) => Some(s),
            (Some(s), false) => Some(s + audio_size.unwrap_or(0)),
            (None, _) => None,
        };
        let ext = ext.to_ascii_lowercase();
        // MP4 at a height wherever there is one; another container only where MP4 has nothing that tall.
        let rank = |ext: &str, size: Option<u64>| (ext == "mp4", size);
        match by_height.iter_mut().find(|(h, _, _, _)| *h == height) {
            Some(slot) if rank(&ext, size) > rank(&slot.2, slot.3) => {
                *slot = (height, label, ext, size)
            }
            Some(_) => {}
            None => by_height.push((height, label, ext, size)),
        }
    }
    by_height.sort_by_key(|slot| std::cmp::Reverse(slot.0));
    let mut list: Vec<Choice> = by_height
        .into_iter()
        .map(|(height, label, ext, size)| Choice {
            label,
            quality: format!("{height}/{ext}"),
            size,
        })
        .collect();
    if let Some(audio) = audio {
        let ext = audio.get("ext").and_then(|v| v.as_str()).unwrap_or("audio");
        list.push(Choice {
            label: format!("{} audio", ext.to_ascii_uppercase()),
            quality: format!("audio/{}", ext.to_ascii_lowercase()),
            size: audio_size,
        });
    }
    list
}

/// Asks yt-dlp what a page offers, for the panel's list. Bounded: a page
/// yt-dlp cannot read in half a minute gets no list, and the panel falls
/// back to downloading the best.
pub async fn list_choices(
    url: &str,
    referrer: Option<&str>,
    proxy: Option<&str>,
) -> Result<Vec<Choice>, Error> {
    let binary = binary().ok_or_else(|| Error::Internal("yt-dlp is not installed".into()))?;
    let mut command = Command::new(binary);
    crate::no_window_async(&mut command);
    command
        .arg("-J")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--socket-timeout")
        .arg("15")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let _proxied = match proxy {
        Some(proxy) => Some(proxy_by_file(&mut command, &std::env::temp_dir(), proxy).await?),
        None => None,
    };
    if let Some(referrer) = referrer {
        command.arg("--referer").arg(referrer);
    }
    // The cookies file a person chose, so a video that needs them signed
    // in (or a site's "not a bot" check) is fetched as their browser would.
    if let Some(cookies) = crate::cookies::file() {
        command.arg("--cookies").arg(cookies);
    }
    // After `--`: whatever the address, yt-dlp never reads it as an option.
    command.arg("--").arg(url);
    let run = command.output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), run)
        .await
        .map_err(|_| Error::Internal("yt-dlp took too long to read the page".into()))?
        .map_err(|e| Error::Internal(format!("yt-dlp would not start: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(
            refusal(&stderr).unwrap_or_else(|| "yt-dlp could not read that page".into()),
        ));
    }
    Ok(choices(&String::from_utf8_lossy(&output.stdout)))
}

/// Runs yt-dlp for one page, reporting progress and placing the result.
///
/// Staged into the download's own directory and moved into place on success,
/// the same shape every other download follows: nothing lands in the finished
/// folder until it is finished.
pub async fn download_ytdlp(
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let staging = &options.parts_dir;
    tokio::fs::create_dir_all(staging)
        .await
        .map_err(Error::Disk)?;
    // Where yt-dlp writes the final path it settled on, away from the noise
    // of its progress so dm reads it back cleanly.
    let result_file = staging.join(".dm-ytdlp-result");
    let _ = tokio::fs::remove_file(&result_file).await;

    // Spawned by its resolved path, not its bare name: the bare name goes
    // through `PATH`, which is exactly what a GUI-launched app lacks, so
    // finding it in /opt/homebrew/bin and then failing to start it would be
    // the same bug one step later.
    let binary = binary().ok_or_else(|| Error::Internal("yt-dlp is not installed".into()))?;
    // ffmpeg is what joins a separate picture and sound, and yt-dlp finds it
    // on the same minimal PATH that hid yt-dlp itself — so it is resolved the
    // same way and handed over explicitly. Without one, the format asks for a
    // single stream that needs no joining.
    let ffmpeg = which("ffmpeg");
    let mut command = Command::new(binary);
    crate::no_window_async(&mut command);
    command
        .arg("--no-playlist")
        .arg("--no-color")
        .arg("--newline")
        // Resume from a part left by a previous run, which is what a pause and
        // a later start turn into.
        .arg("--continue")
        .arg("-o")
        .arg(staging.join("%(title).200B [%(id)s].%(ext)s"))
        .arg("-f")
        .arg(format_for(options.quality, ffmpeg.is_some()))
        .args(container_sort(options.container.as_deref()))
        .arg("--progress-template")
        .arg(format!(
            "download:{MARK}%(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s"
        ))
        .arg("--print-to-file")
        .arg("after_move:filepath")
        .arg(&result_file)
        // Bounded, where yt-dlp's defaults are ten tries per fragment with
        // no socket timeout: a segment host answering 503 would otherwise
        // keep the row "running" for minutes before dm hears it failed.
        .arg("--retries")
        .arg("3")
        .arg("--fragment-retries")
        .arg("3")
        .arg("--socket-timeout")
        .arg("15")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    /* A process group of its own, so a pause reaches what yt-dlp started as
     * well as yt-dlp: killed alone, the ffmpeg joining picture and sound
     * ran on, writing into the parts a resume was about to use. */
    #[cfg(unix)]
    command.process_group(0);

    if let Some(ffmpeg) = &ffmpeg {
        command.arg("--ffmpeg-location").arg(ffmpeg);
        // Subtitles ride inside the file, which is ffmpeg's work; without
        // one they would land beside it as files dm does not place.
        if options.subtitles {
            command.args(["--write-subs", "--sub-langs", "all", "--embed-subs"]);
        }
    }

    /* The same two things every request of dm's own carries. yt-dlp makes
     * its own connections, so the proxy the client was built with does not
     * reach it unless it is told — and a machine where everything must go
     * through the proxy is exactly where a direct connection is the one
     * outcome nobody wants. The referrer is what a hotlink-protected page
     * checks for, and the probe that routed here already needed it. */
    let _proxied = match options.proxy_for(url) {
        Some(proxy) => Some(proxy_by_file(&mut command, staging, proxy).await?),
        None => None,
    };
    if let Some(referrer) = &options.referrer {
        command.arg("--referer").arg(referrer);
    }
    /* The site's sign-in goes in a cookies file of this run's own, readable
     * by this account alone and removed when the run ends — never on the
     * command line, where any local user's `ps` reads it — and scoped to the
     * site's own domain, so yt-dlp sends it there and nowhere else. A
     * cookies file a person chose is copied in beside it, since yt-dlp
     * takes one. */
    let _signed = match options
        .sign_in
        .as_ref()
        .filter(|s| s.for_url(url).is_some())
    {
        Some(sign_in) => {
            let mut text = String::from("# Netscape HTTP Cookie File\n");
            if let Some(chosen) = crate::cookies::file() {
                if let Ok(theirs) = tokio::fs::read_to_string(&chosen).await {
                    text.push_str(&theirs);
                    if !text.ends_with('\n') {
                        text.push('\n');
                    }
                }
            }
            text.push_str(&sign_in.netscape_lines());
            let path = staging.join("sign-in.cookies.txt");
            write_private(&path, &text).await?;
            command.arg("--cookies").arg(&path);
            Some(RemoveOnDrop(path))
        }
        None => {
            if let Some(cookies) = crate::cookies::file() {
                command.arg("--cookies").arg(cookies);
            }
            None
        }
    };

    // This run's part of the shared ceiling, if one is set, held until it
    // ends. yt-dlp takes bytes per second, once: it keeps the part it was
    // given when others start or finish.
    let _share = options.limiter.share();
    let rate = options.limiter.part().await;
    if rate > 0 {
        command.arg("--limit-rate").arg(rate.to_string());
    }

    // After `--`: whatever the address, yt-dlp never reads it as an option.
    command.arg("--").arg(url);

    let mut child = command
        .spawn()
        .map_err(|e| Error::Internal(format!("yt-dlp would not start: {e}")))?;
    // Declared after the child, so dropped before it: the group is ended
    // while yt-dlp, its leader, is still there to name it.
    let mut tree = Tree::of(child.id());

    // Drained on its own task, not left in the pipe. A long download that
    // logged more warnings than the pipe holds would otherwise block yt-dlp
    // writing to a stderr nobody is reading. Kept to report on a failure.
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Internal("yt-dlp started without its error output".into()))?;
    let errors = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut text).await;
        text
    });

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Internal("yt-dlp started without its output".into()))?;
    let mut lines = BufReader::new(stdout).lines();
    let mut named = false;
    let mut sum = Cumulative::default();

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line.map_err(Error::Disk)? else { break };

                if let Some(rest) = line.strip_prefix(MARK) {
                    if let Some(tx) = &report {
                        if let Some(progress) = parse_progress(rest) {
                            let _ = tx.send(Update::Progress(sum.add(progress))).await;
                        }
                    }
                } else if let Some(dest) = line.strip_prefix("[download] Destination: ") {
                    // Each stream of a pair announces itself here.
                    sum.next_stream();
                    // An early name, before the merge settles the final one.
                    if !named {
                        if let Some(name) = Path::new(dest.trim()).file_name() {
                            let name = name.to_string_lossy().into_owned();
                            if let Some(tx) = &report {
                                let _ = tx
                                    .send(Update::Named {
                                        category: category::classify(&name, None),
                                        name,
                                    })
                                    .await;
                            }
                            named = true;
                        }
                    }
                }
            }
            // Between lines, so a pause or cancel is honoured even while
            // yt-dlp is quiet. Killing the child leaves its part behind for
            // --continue to resume from; a cancel throws that away.
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                if let Err(stop) = options.control.check() {
                    // The tree first, and waited for, off the runtime: on
                    // Windows `taskkill /T` finds yt-dlp's workers through
                    // yt-dlp, and a launcher killed first left them
                    // running (gap 98).
                    if let Some(pid) = tree.take() {
                        let _ = tokio::task::spawn_blocking(move || end_tree(pid)).await;
                    }
                    let _ = child.kill().await;
                    if matches!(stop, Error::Cancelled) {
                        remove_staging(staging).await;
                    }
                    return Err(stop);
                }
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| Error::Internal(format!("yt-dlp did not finish: {e}")))?;
    // Reaped: its number may now be another program's, never to be killed.
    tree.disarm();

    // Checked after draining stdout, so a pause noticed on the last line wins.
    options.control.check()?;

    if !status.success() {
        let stderr = errors.await.unwrap_or_default();
        let tail = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        /* With no ffmpeg the format asks for one stream carrying both
         * picture and sound, and a stream that only comes as two has none:
         * yt-dlp says "Requested format is not available", which names
         * neither the cause nor the fix. */
        if let Some(said) = refusal(&stderr) {
            return Err(Error::Internal(said));
        }
        if ffmpeg.is_none() && tail.contains("Requested format is not available") {
            return Err(Error::Internal(
                "that stream's picture and sound are separate and joining them needs \
                 ffmpeg — install ffmpeg"
                    .into(),
            ));
        }
        return Err(Error::Internal(format!(
            "yt-dlp could not download that{}",
            if tail.is_empty() {
                String::new()
            } else {
                format!(": {tail}")
            }
        )));
    }

    let produced = tokio::fs::read_to_string(&result_file)
        .await
        .ok()
        .and_then(|s| s.lines().next_back().map(str::trim).map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| Error::Internal("yt-dlp reported no output file".into()))?;

    place(&produced, options).await
}

/// Moves a finished yt-dlp file into its category folder under the download
/// directory, the same placement every other download gets.
async fn place(produced: &Path, options: &Options) -> Result<Finished, Error> {
    // yt-dlp names the file it meant to leave; if it is not there, the usual
    // cause is a join that never happened, and "No such file" says nothing.
    if !produced.is_file() {
        return Err(Error::Internal(
            "yt-dlp finished but the file it named is missing — install ffmpeg so \
             separate video and audio can be joined"
                .into(),
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
        // yt-dlp manages its own connections; the column would be guessing.
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 0,
    })
}

/// yt-dlp and everything it started, ended together.
///
/// On unix yt-dlp leads a process group of its own and the group is sent
/// SIGKILL by `killpg`; on Windows `taskkill /T` ends the tree under it.
/// `None` once the run has been reaped, when the number may belong to
/// someone else.
///
/// No `kill` program on unix: looked up on `PATH`, it was not found under
/// the minimal one a browser's native host or the Dock starts dm with, the
/// error went unread, and the ffmpeg under yt-dlp ran on. On unix nothing
/// here waits. On Windows `taskkill` is waited for, at most
/// [`TREE_KILL_WAIT`], before yt-dlp itself is killed: the one-file yt-dlp
/// is a launcher with the real yt-dlp as its child, and `/T` finds that
/// child through the launcher, so a launcher killed first (dm used to start
/// taskkill and kill yt-dlp at once) left the worker and its ffmpeg running
/// (gap 98). The async path waits off the runtime; `Drop`, the rare path of
/// a run abandoned mid-way, waits where it is.
struct Tree(Option<u32>);

/// How long `taskkill` is given to end a tree.
#[cfg_attr(not(windows), allow(dead_code))]
const TREE_KILL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Every yt-dlp tree still running, for `end_all`.
static LIVE: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// Ends every yt-dlp tree still running. For a dm leaving at once (a second
/// interrupt) without the unwinding that would end each on its own: yt-dlp
/// runs in a process group of its own, which a terminal's Ctrl+C no longer
/// reaches, so nothing else would stop it and its ffmpeg. Each tree is
/// waited for (on Windows, briefly) before dm exits and takes yt-dlp's
/// launcher down with it.
pub fn end_all() {
    let pids = std::mem::take(&mut *crate::lock(&LIVE));
    for pid in pids {
        end_tree(pid);
    }
}

impl Tree {
    fn of(pid: Option<u32>) -> Self {
        if let Some(pid) = pid {
            crate::lock(&LIVE).push(pid);
        }
        Tree(pid)
    }

    /// yt-dlp has been reaped: its number may be another process's now.
    fn disarm(&mut self) {
        let _ = self.take();
    }

    /// The number to end, handed over once and forgotten here.
    fn take(&mut self) -> Option<u32> {
        let pid = self.0.take()?;
        crate::lock(&LIVE).retain(|p| *p != pid);
        Some(pid)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Some(pid) = self.take() {
            end_tree(pid);
        }
    }
}

/// Ends the tree under `pid`, and on Windows returns once `taskkill` has.
fn end_tree(pid: u32) {
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(pid) {
        // SAFETY: `killpg` takes two integers and touches no memory of
        // ours. The group is yt-dlp's own (`process_group(0)` at spawn)
        // and is not yet reaped, so the number is still its.
        let _ = unsafe { libc::killpg(group, libc::SIGKILL) };
    }
    #[cfg(windows)]
    if let Ok(mut taskkill) = crate::no_window(&mut std::process::Command::new("taskkill"))
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        wait_at_most(&mut taskkill, TREE_KILL_WAIT);
    }
}

/// Waits for `child` up to `limit`, then kills it rather than wait on.
/// Whether it ended on its own in time.
#[cfg_attr(not(windows), allow(dead_code))]
fn wait_at_most(child: &mut std::process::Child, limit: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if start.elapsed() < limit => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
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

/// Progress over every stream of a run, not each on its own.
///
/// Picture and sound are fetched one after the other, each counting from
/// zero, so the bar ran to the end of the picture and fell back to nothing
/// for the sound. Streams that have finished are carried in `before`.
#[derive(Default)]
struct Cumulative {
    before: u64,
    current: u64,
}

impl Cumulative {
    /// A stream has started: whatever the last one reached is behind us.
    fn next_stream(&mut self) {
        self.before += self.current;
        self.current = 0;
    }

    fn add(&mut self, progress: crate::Progress) -> crate::Progress {
        // A count that falls is a new stream, announced or not.
        if progress.downloaded < self.current {
            self.next_stream();
        }
        self.current = progress.downloaded;
        crate::Progress {
            downloaded: self.before + progress.downloaded,
            total: progress.total.map(|t| self.before + t),
            ..progress
        }
    }
}

/// Reads one progress line, `<downloaded> <total> <estimate>`.
///
/// yt-dlp writes `NA` where it does not know, and knows the total only for
/// some formats — an estimate stands in until then, and neither is fatal.
/// The estimate is a float (`1234.5`) more often than not, and read as one.
fn parse_progress(rest: &str) -> Option<crate::Progress> {
    let bytes = |s: &str| {
        s.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64)
    };
    let mut fields = rest.split_whitespace();
    let downloaded = fields.next().and_then(bytes)?;
    let total = fields.next().and_then(bytes);
    let estimate = fields.next().and_then(bytes);

    Some(crate::Progress {
        downloaded,
        total: total.or(estimate),
        // yt-dlp's own speed line is separate; dm's watcher is not on this
        // path, so speed and ETA are left for the numbers that are certain.
        speed: 0.0,
        eta: None,
    })
}

/// What a run ends with when yt-dlp has no extractor for the address: the
/// one refusal after which the address may still be an ordinary file.
pub const UNKNOWN_SITE: &str = "yt-dlp does not know this site";

/// What a site's refusal means, in words a person can act on, when yt-dlp's
/// error output shows one of the common ones. A live run against YouTube
/// failed with yt-dlp's raw "HTTP Error 429" after Google sent it to its
/// "sorry" page; the row should say the network was rate-limited, and that
/// waiting is the fix, not that dm broke.
pub fn refusal(stderr: &str) -> Option<String> {
    let said = stderr.to_ascii_lowercase();
    let has = |needle: &str| said.contains(needle);
    if has("http error 429") || has("too many requests") || has("google.com/sorry") {
        return Some(
            "the site is limiting downloads from this network right now (HTTP 429); \
             try again later"
                .into(),
        );
    }
    if has("confirm you") && has("not a bot") {
        return Some(
            "the site wants this network to prove it is not a bot; open the video in \
             the browser, or try again later"
                .into(),
        );
    }
    if has("sign in") || has("log in") || has("login required") || has("logged-in") {
        return Some("the site only shows this to someone signed in".into());
    }
    if has("private video") {
        return Some("that video is private".into());
    }
    if has("video unavailable") || has("this video has been removed") || has("http error 404") {
        return Some("that video is not available any more".into());
    }
    if has("unsupported url") {
        return Some(UNKNOWN_SITE.into());
    }
    // A stream encrypted for the site's own player alone. Neither yt-dlp
    // nor dm takes protection off, and yt-dlp's advice to pick another
    // format leads nowhere when every format is protected (Vimeo's, seen
    // 2026-10-16 through the panel).
    if has("drm protected") || has("drm-protected") {
        return Some(
            "that video is copy-protected (DRM), so only the site's own player can play it; \
             dm does not download protected video"
                .into(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::refusal;

    /// Gap 98: the tree's killer is waited for, but never for ever.
    #[cfg(unix)]
    /// UI round 7: a stream whose renditions name no codecs, as yt-dlp
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

    #[test]
    fn the_trees_killer_is_waited_for_but_not_for_ever() {
        use std::time::{Duration, Instant};
        let mut quick = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        assert!(super::wait_at_most(&mut quick, Duration::from_secs(5)));

        let mut stuck = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let at = Instant::now();
        assert!(!super::wait_at_most(&mut stuck, Duration::from_millis(200)));
        assert!(at.elapsed() < Duration::from_secs(5), "waited on");
        assert!(stuck.try_wait().unwrap().is_some(), "killed and reaped");
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

    #[test]
    fn a_sites_refusal_is_said_in_words_a_person_can_act_on() {
        let limited = "WARNING: [youtube] got redirected to https://www.google.com/sorry/index\n\
                       ERROR: [youtube] jNQXAC9IVRw: Unable to download webpage: HTTP Error 429: Too Many Requests";
        assert!(refusal(limited).unwrap().contains("try again later"));
        assert!(
            refusal("ERROR: Sign in to confirm you're not a bot. Use --cookies")
                .unwrap()
                .contains("not a bot")
        );
        assert!(
            refusal("ERROR: [vimeo] 1: The web client only works when logged-in")
                .unwrap()
                .contains("signed in")
        );
        assert_eq!(
            refusal("ERROR: [youtube] x: Private video").as_deref(),
            Some("that video is private")
        );
        assert!(refusal("ERROR: Unsupported URL: https://e.com/")
            .unwrap()
            .contains("does not know"));
        assert!(refusal(
            "ERROR: This format is DRM protected; Try selecting another format with --format"
        )
        .is_some_and(|said| said.contains("copy-protected")));
        assert_eq!(refusal("ERROR: something else entirely"), None);
    }

    use super::*;

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
        assert_eq!(labels, ["WEBM 3840×2160", "MP4 1920×1080", "M4A audio"]);
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
        assert_eq!(labels, ["MP4 1280×720", "MP4 640×360", "M4A audio"]);
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
        assert_eq!(
            container_sort(Some("mp4")),
            [
                "-S",
                "res,ext:mp4:m4a,vcodec:h264",
                "--merge-output-format",
                "mp4"
            ]
        );
        assert_eq!(
            container_sort(None),
            container_sort(Some("mp4")),
            "MP4 unless a line asked otherwise"
        );
        assert_eq!(container_sort(Some("webm")), ["-S", "ext:webm:webm"]);
        assert!(choices("not json").is_empty());
        assert!(choices("{}").is_empty());
    }

    #[test]
    fn a_quality_becomes_a_format_selector() {
        assert_eq!(format_for(Quality::Best, true), "bv*+ba/b");
        assert_eq!(format_for(Quality::Audio, true), "ba/b");
        assert!(format_for(Quality::UpTo(720), true).contains("height<=720"));
    }

    #[test]
    fn without_ffmpeg_a_single_stream_is_asked_for() {
        // Two streams and no way to join them is two files and no video, so
        // the selector must not ask for a pair when nothing can merge it.
        assert_eq!(format_for(Quality::Best, false), "b");
        assert_eq!(format_for(Quality::UpTo(480), false), "b[height<=480]/b");
        assert!(!format_for(Quality::Best, false).contains('+'));
    }

    #[test]
    fn progress_reads_bytes_and_falls_back_to_the_estimate() {
        let p = parse_progress("500 1000 1000").unwrap();
        assert_eq!(p.downloaded, 500);
        assert_eq!(p.total, Some(1000));

        // Total unknown, estimate present: the estimate stands in.
        let p = parse_progress("500 NA 2000").unwrap();
        assert_eq!(p.total, Some(2000));

        // Neither known yet: a count with no total is still honest.
        let p = parse_progress("500 NA NA").unwrap();
        assert_eq!(p.downloaded, 500);
        assert_eq!(p.total, None);
    }

    #[test]
    fn an_estimate_written_as_a_float_is_read() {
        let p = parse_progress("500 NA 2048.75").unwrap();
        assert_eq!(p.total, Some(2048));
        let p = parse_progress("500.0 NA NA").unwrap();
        assert_eq!(p.downloaded, 500);
        assert!(parse_progress("NaN NA NA").is_none());
        assert!(parse_progress("-1 NA NA").is_none());
    }

    #[test]
    fn progress_over_picture_then_sound_never_goes_back() {
        let mut sum = Cumulative::default();
        let seen: Vec<u64> = ["1000 2000 NA", "2000 2000 NA", "0 500 NA", "500 500 NA"]
            .iter()
            .map(|l| sum.add(parse_progress(l).unwrap()).downloaded)
            .collect();
        assert_eq!(seen, [1000, 2000, 2000, 2500]);
        // Announced by a Destination line, the second stream counts on too.
        let mut sum = Cumulative::default();
        sum.add(parse_progress("2000 2000 NA").unwrap());
        sum.next_stream();
        let p = sum.add(parse_progress("100 500 NA").unwrap());
        assert_eq!((p.downloaded, p.total), (2100, Some(2500)));
    }

    #[test]
    fn a_line_without_the_numbers_is_ignored() {
        assert!(parse_progress("not numbers here").is_none());
    }
}

#[cfg(test)]
mod windows_homes_tests {
    use super::{find_in, names, windows_homes, Homes};
    use std::path::PathBuf;

    /// Issue 10: on Windows the program is `yt-dlp.exe`, installed where
    /// pip, winget, Scoop or Chocolatey put it, none of them on the `PATH`
    /// a dm started by a browser has. Each is found there, laid out as a
    /// Windows profile is under a folder of this test's.
    #[test]
    fn windows_installs_are_found_by_their_exe() {
        let root = std::env::temp_dir().join(format!("dm-win-homes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (local, roaming, home, data) = (
            root.join("Local"),
            root.join("Roaming"),
            root.join("me"),
            root.join("ProgramData"),
        );
        let homes = Homes {
            local: Some(local.clone()),
            roaming: Some(roaming.clone()),
            home: home.clone(),
            program_data: Some(data.clone()),
            program_files: None,
            system_drive: None,
        };
        let put = |dir: PathBuf, name: &str| {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), b"MZ").unwrap();
            dir.join(name)
        };
        let exe = names("yt-dlp", true);
        assert_eq!(exe[0], "yt-dlp.exe");
        let find = |h: &Homes| find_in(windows_homes(h).into_iter(), &exe);
        assert_eq!(find(&homes), None);

        // pip install --user, with python.org's Python 3.13.
        let pip = put(
            roaming.join("Python").join("Python313").join("Scripts"),
            "yt-dlp.exe",
        );
        assert_eq!(find(&homes), Some(pip.clone()));
        std::fs::remove_file(&pip).unwrap();
        // winget's link, then its package folder alone.
        let winget = local.join("Microsoft").join("WinGet");
        let link = put(winget.join("Links"), "yt-dlp.exe");
        assert_eq!(find(&homes), Some(link.clone()));
        std::fs::remove_file(&link).unwrap();
        let package = put(
            winget.join("Packages").join("yt-dlp.yt-dlp_8wekyb3d8bbwe"),
            "yt-dlp.exe",
        );
        assert_eq!(find(&homes), Some(package.clone()));
        std::fs::remove_file(&package).unwrap();
        // Scoop, then Chocolatey.
        let scoop = put(home.join("scoop").join("shims"), "yt-dlp.exe");
        assert_eq!(find(&homes), Some(scoop.clone()));
        std::fs::remove_file(&scoop).unwrap();
        let choco = put(data.join("chocolatey").join("bin"), "yt-dlp.exe");
        assert_eq!(find(&homes), Some(choco));

        // A winget ffmpeg build keeps it in a bin folder one level down.
        let ffmpeg = put(
            winget
                .join("Packages")
                .join("Gyan.FFmpeg_8wekyb3d8bbwe")
                .join("ffmpeg-7.1-full_build")
                .join("bin"),
            "ffmpeg.exe",
        );
        assert_eq!(
            find_in(windows_homes(&homes).into_iter(), &names("ffmpeg", true)),
            Some(ffmpeg)
        );
        // Elsewhere the bare name, as before.
        assert_eq!(names("yt-dlp", false), ["yt-dlp"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
