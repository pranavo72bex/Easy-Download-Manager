//! Streams served over RTMP, RTSP and MMS: the protocols of media servers
//! and older players, which no HTTP request reaches.
//!
//! RTMP is dm's own (`rtmp.rs`): the stream recorded as it comes into a
//! fragmented MP4 that plays however far it got (`fmp4`), rewritten as a
//! plain one when it ends (`mp4mux`). The rest — RTMPE, RTMPT, RTSP, MMS —
//! is handed to ffmpeg, fetched by dm where the computer has none: the
//! stream copied, picture and sound untouched, into a part file. Either
//! way it is placed as every finished download is (`finish`: its category,
//! its folder, a digest checked). A live stream has no end and no position
//! to come back to: it runs until its server ends it or the person stops
//! it, and a resume starts it again. Neither has a proxy for these, so with
//! a proxy in force the download is refused rather than sent around it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::fs;

use crate::download::{Finished, Options, Update};
use crate::Error;

/// The schemes handed to ffmpeg, and the container their streams go into:
/// MMS carries Windows Media, which only its own container takes as it is.
const SCHEMES: [(&str, &str); 9] = [
    ("rtmp", "mp4"),
    ("rtmps", "mp4"),
    ("rtmpe", "mp4"),
    ("rtmpt", "mp4"),
    ("rtsp", "mp4"),
    ("rtsps", "mp4"),
    ("mms", "wmv"),
    ("mmsh", "wmv"),
    ("mmst", "wmv"),
];

fn scheme_of(url: &str) -> Option<&'static (&'static str, &'static str)> {
    let (scheme, _) = url.trim().split_once("://")?;
    SCHEMES.iter().find(|(s, _)| s.eq_ignore_ascii_case(scheme))
}

/// Whether `url` is one of the stream addresses this module fetches.
pub fn is_live(url: &str) -> bool {
    scheme_of(url).is_some()
}

/// Whether dm fetches such a stream itself, without ffmpeg.
fn own(url: &str) -> bool {
    scheme_of(url).is_some_and(|(s, _)| ["rtmp", "rtmps"].contains(s))
}

/// Whether a stream address needs ffmpeg: one dm does not speak itself.
pub fn needs_ffmpeg(url: &str) -> bool {
    is_live(url) && !own(url)
}

/// The name a stream is saved under: the address's last part, in the
/// container its stream goes into.
pub fn name_for(url: &str) -> String {
    let ext = scheme_of(url).map_or("mp4", |(_, ext)| ext);
    let last = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('/')
        .find(|part| !part.is_empty() && !part.contains("://") && !part.contains(':'))
        .map(crate::probe::decode)
        .unwrap_or_default();
    let stem = match last.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => last,
    };
    let stem = crate::probe::sanitize(&stem);
    let stem = if stem.trim().is_empty() {
        "stream".to_string()
    } else {
        stem
    };
    format!("{stem}.{ext}")
}

/// What ffmpeg is told: the stream, copied, into a file that plays even
/// when it is cut short (an MP4 fragmented as it is written).
fn arguments(url: &str, out: &std::path::Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = ["-y", "-nostdin", "-loglevel", "error", "-i"]
        .iter()
        .map(Into::into)
        .collect();
    args.push(url.into());
    args.extend(["-c", "copy"].iter().map(Into::into));
    if out.extension().is_some_and(|e| e == "mp4") {
        args.extend(
            ["-movflags", "+frag_keyframe+empty_moov"]
                .iter()
                .map(Into::into),
        );
    }
    args.push(out.as_os_str().to_owned());
    args
}

pub async fn download_live(
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let scheme = scheme_of(url).map_or("stream", |(s, _)| *s);
    if options.proxy_for(url).is_some()
        || options.no_route_through_system_proxy(url, scheme).is_some()
    {
        return Err(Error::Internal(format!(
            "a {scheme}:// stream cannot go through a proxy, and dm does not send it around one: \
             turn the proxy off for this site (Options › Proxy) to fetch it"
        )));
    }
    let ffmpeg = crate::ytdlp::ffmpeg();
    if ffmpeg.is_none() && !own(url) {
        return Err(Error::Internal(format!(
            "a {scheme}:// stream is fetched by ffmpeg — install ffmpeg"
        )));
    }
    let name = name_for(url);
    fs::create_dir_all(&options.parts_dir).await?;
    let ext = name.rsplit('.').next().unwrap_or("mp4");
    let part: PathBuf = options.parts_dir.join(format!("stream.{ext}"));
    // A stream has no place to resume from: what an earlier run left is
    // replaced.
    let _ = fs::remove_file(&part).await;

    let category = crate::category::classify(&name, None);
    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: name.clone(),
                category,
            })
            .await;
    }
    let downloaded = Arc::new(AtomicU64::new(0));
    let reporter = report.map(|tx| crate::download::watch(Arc::clone(&downloaded), None, tx));

    let ffmpeg = match ffmpeg {
        Some(ffmpeg) if !own(url) => ffmpeg,
        _ => {
            let recorded = crate::rtmp::record(url, &part, &options.control, &downloaded).await;
            stop_reporting(reporter).await;
            if let Err(stopped) = recorded {
                if !matches!(stopped, Error::Paused) {
                    let _ = fs::remove_file(&part).await;
                }
                return Err(stopped);
            }
            // Rewritten plain, its index first so it seeks; kept fragmented
            // (it plays) where that fails.
            let plain = part.with_extension("plain.mp4");
            if crate::mp4mux::join_async(vec![part.clone()], plain.clone())
                .await
                .is_ok()
            {
                fs::rename(&plain, &part).await?;
            } else {
                let _ = fs::remove_file(&plain).await;
            }
            return placed(&name, &part, options).await;
        }
    };

    let mut command = tokio::process::Command::new(&ffmpeg);
    let child = crate::no_window_async(&mut command)
        .args(arguments(url, &part))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            stop_reporting(reporter).await;
            return Err(Error::Internal(format!("ffmpeg could not be started: {e}")));
        }
    };
    let stderr = child.stderr.take();
    let said = tokio::spawn(async move {
        let mut text = String::new();
        if let Some(mut err) = stderr {
            use tokio::io::AsyncReadExt as _;
            let mut buf = Vec::new();
            let _ = (&mut err).take(64 * 1024).read_to_end(&mut buf).await;
            text = String::from_utf8_lossy(&buf).into_owned();
        }
        text
    });

    // The bytes so far, from the file ffmpeg writes, for the row's
    // progress; and the control, which ends ffmpeg on a pause or a cancel.
    let watched = {
        let (part, downloaded) = (part.clone(), Arc::clone(&downloaded));
        tokio::spawn(async move {
            loop {
                if let Ok(meta) = fs::metadata(&part).await {
                    downloaded.store(meta.len(), Ordering::Relaxed);
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        })
    };
    let waited = crate::download::or_stop(&options.control, async {
        child.wait().await.map_err(Error::Disk)
    })
    .await;
    watched.abort();
    stop_reporting(reporter).await;
    let status = match waited {
        Ok(status) => status,
        Err(stopped) => {
            drop(child);
            if matches!(stopped, Error::Cancelled) {
                let _ = fs::remove_file(&part).await;
            }
            return Err(stopped);
        }
    };
    let got = fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    if !status.success() || got == 0 {
        let why = said.await.unwrap_or_default();
        let why = why
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        let _ = fs::remove_file(&part).await;
        return Err(Error::Internal(if why.is_empty() {
            format!("ffmpeg could not fetch the {scheme}:// stream")
        } else {
            format!(
                "ffmpeg could not fetch the {scheme}:// stream: {}",
                why.trim()
            )
        }));
    }
    placed(&name, &part, options).await
}

/// The stream's part file placed as any finished download is.
async fn placed(name: &str, part: &std::path::Path, options: &Options) -> Result<Finished, Error> {
    let (path, bytes, category) =
        crate::download::finish(name, None, &[part.to_path_buf()], options).await?;
    Ok(Finished {
        path,
        bytes,
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
        left_behind: None,
    })
}

/// Ends the progress reporter, as the other routes do: told it is over,
/// and waited for so its last word is sent.
async fn stop_reporting(
    reporter: Option<(
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Sender<()>,
    )>,
) {
    if let Some((handle, finished)) = reporter {
        let _ = finished.send(());
        let _ = handle.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_addresses_are_told_and_named() {
        assert!(is_live("rtmp://media.example/live/show"));
        assert!(is_live("RTSP://cam.example:554/stream1"));
        assert!(is_live("mms://old.example/clip.wmv"));
        assert!(!is_live("https://example.com/a.mp4"));
        assert!(!is_live("ftp://example.com/a.mp4"));
        assert_eq!(name_for("rtmp://media.example/live/show"), "show.mp4");
        assert_eq!(
            name_for("rtmp://media.example/live/show.flv?key=1"),
            "show.mp4"
        );
        assert_eq!(name_for("mms://old.example/clip.asf"), "clip.wmv");
        assert_eq!(name_for("rtsp://cam.example:554/"), "stream.mp4");
        // A decoded "../" never makes a path of the name.
        let sly = name_for("rtmp://media.example/live/%2E%2E%2Fescape");
        assert!(!sly.contains('/') && !sly.contains('\\'), "{sly}");
    }
}
