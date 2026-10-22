//! Streams served over RTMP, RTSP and MMS: the protocols of media servers
//! and older players, which no HTTP request reaches.
//!
//! RTMP (RTMPS and RTMPT too) and RTSP (RTSPS too) are dm's own (`rtmp.rs`,
//! `rtsp.rs`): the stream recorded as it comes into a fragmented MP4 that
//! plays however far it got (`fmp4`), rewritten as a plain one when it ends
//! (`mp4mux`). MMS is asked for over HTTP (`mms.rs`) and kept as the
//! Windows Media it is. RTMPE, Flash's encrypted RTMP, is refused. Each is
//! placed as every finished download is (`finish`: its category,
//! its folder, a digest checked). A live stream has no end and no position
//! to come back to: it runs until its server ends it or the person stops
//! it, and a resume starts it again. Neither has a proxy for these, so with
//! a proxy in force the download is refused rather than sent around it.

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use tokio::fs;

use crate::download::{Finished, Options, Update};
use crate::Error;

/// The stream schemes dm knows, and the container their streams go into:
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

/// Whether dm records such a stream: every one but RTMPE.
fn own(url: &str) -> bool {
    scheme_of(url).is_some_and(|(s, _)| {
        [
            "rtmp", "rtmps", "rtmpt", "rtsp", "rtsps", "mms", "mmsh", "mmst",
        ]
        .contains(s)
    })
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
    // RTMPE is not dm's: Flash's encrypted RTMP, a protection dm does not
    // undo.
    if !own(url) {
        return Err(Error::Internal(format!(
            "{scheme}:// is an encrypted stream dm cannot record"
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

    let recorded = if scheme.starts_with("rtsp") {
        crate::rtsp::record(url, &part, &options.control, &downloaded).await
    } else if scheme.starts_with("mms") {
        crate::mms::record(url, &part, &options.control, &downloaded).await
    } else {
        crate::rtmp::record(url, &part, &options.control, &downloaded).await
    };
    stop_reporting(reporter).await;
    if let Err(stopped) = recorded {
        if !matches!(stopped, Error::Paused) {
            let _ = fs::remove_file(&part).await;
        }
        return Err(stopped);
    }
    // Rewritten plain, its index first so it seeks; kept fragmented (it
    // plays) where that fails. An MMS stream is Windows Media, kept as the
    // server sent it.
    let plain = part.with_extension("plain.mp4");
    if !scheme.starts_with("mms")
        && crate::mp4mux::join_async(vec![part.clone()], plain.clone())
            .await
            .is_ok()
    {
        fs::rename(&plain, &part).await?;
    } else {
        let _ = fs::remove_file(&plain).await;
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
