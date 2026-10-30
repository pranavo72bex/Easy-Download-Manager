//! Another site's playlist page, read by yt-dlp as a list of its videos
//! (`--flat-playlist`: each video named, none of them read), so the
//! Playlist dialog lists it as it does a YouTube one.

use super::*;
use crate::youtube::{Playlist, PlaylistEntry};

/// The most videos listed of one playlist, as for YouTube's.
const MOST: usize = 1000;

/// The playlist in yt-dlp's flat `-J` description of a page, `None` where
/// the page is one video (or nothing yt-dlp could read). An entry without
/// a web address of its own is passed over; a playlist none of whose
/// entries has one is no playlist.
pub fn playlist_of(described: &str) -> Option<Playlist> {
    let page: serde_json::Value = serde_json::from_str(described).ok()?;
    if page.get("_type").and_then(|t| t.as_str()) != Some("playlist") {
        return None;
    }
    let listed = page.get("entries")?.as_array()?;
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for entry in listed {
        let Some(link) = entry
            .get("url")
            .or_else(|| entry.get("webpage_url"))
            .and_then(|u| u.as_str())
            .map(str::trim)
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        else {
            continue;
        };
        if !seen.insert(link.to_string()) {
            continue;
        }
        let id = entry
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();
        let title = entry
            .get("title")
            .and_then(|t| t.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if id.is_empty() {
                    link.to_string()
                } else {
                    id.clone()
                }
            });
        let seconds = entry
            .get("duration")
            .and_then(|d| d.as_f64())
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map(|d| d.round() as u64);
        entries.push(PlaylistEntry {
            id,
            title,
            seconds,
            page: Some(link.to_string()),
        });
        if entries.len() == MOST {
            break;
        }
    }
    if entries.is_empty() {
        return None;
    }
    let count = page
        .get("playlist_count")
        .and_then(|c| c.as_u64())
        .unwrap_or(0) as usize;
    // Read up to `MOST` (`--playlist-items`); the page says how many more.
    let cut_short = count > listed.len();
    Some(Playlist {
        id: page
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string(),
        title: page
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        entries,
        cut_short,
    })
}

/// `url` read by yt-dlp as a playlist: `Ok(None)` where it is a page of one
/// video, the playlist's videos where it lists several.
pub async fn read_playlist(
    url: &str,
    referrer: Option<&str>,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Option<Playlist>, Error> {
    let binary = binary()
        .ok_or_else(|| Error::Internal("dm's video helper is not available here".into()))?;
    let mut command = super::writable_to(&binary, &[]);
    crate::no_window_async(&mut command);
    js_runtime(&mut command);
    unpack_where_programs_run(&mut command);
    command
        .arg("-J")
        .arg("--flat-playlist")
        .arg("--playlist-items")
        .arg(format!("1:{MOST}"))
        .arg("--no-warnings")
        .arg("--socket-timeout")
        .arg("15")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let _proxied = match proxy {
        Some(proxy) => Some(proxy_by_file(&mut command, &std::env::temp_dir(), proxy).await?),
        None => None,
    };
    if let Some(referrer) = referrer {
        command.arg("--referer").arg(referrer);
    }
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "dm-playlist-{}-{run}.cookies.txt",
        std::process::id()
    ));
    let _signed = cookies_by_file(&mut command, &path, sign_in, url).await?;
    command.arg("--").arg(url);
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), command.output())
        .await
        .map_err(|_| Error::Internal("yt-dlp took too long to read the playlist".into()))?
        .map_err(|e| Error::Internal(format!("yt-dlp would not start: {e}")))?;
    if !output.status.success() {
        return Err(Error::Internal(said_by(
            &String::from_utf8_lossy(&output.stderr),
            "yt-dlp could not read that playlist",
        )));
    }
    Ok(playlist_of(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_playlist_lists_its_videos_by_their_own_addresses() {
        let json = r#"{"_type":"playlist","id":"showcase-1","title":" Short films ",
            "playlist_count":3,
            "entries":[
              {"_type":"url","id":"101","url":"https://vimeo.example/101","title":"First","duration":61.6},
              {"_type":"url","id":"102","url":"https://vimeo.example/102","title":""},
              {"_type":"url","id":"102","url":"https://vimeo.example/102","title":"again"},
              {"_type":"url","id":"103","url":"/relative/103","title":"no address"}
            ]}"#;
        let list = playlist_of(json).expect("a playlist");
        assert_eq!(list.id, "showcase-1");
        assert_eq!(list.title, "Short films");
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[0].title, "First");
        assert_eq!(list.entries[0].seconds, Some(62));
        assert_eq!(list.entries[0].url(), "https://vimeo.example/101");
        assert_eq!(list.entries[1].title, "102");
        assert_eq!(list.entries[1].url(), "https://vimeo.example/102");
        assert!(!list.cut_short, "all it has were read");
        let more = json.replace("\"playlist_count\":3", "\"playlist_count\":1500");
        assert!(playlist_of(&more).expect("a playlist").cut_short);
    }

    #[test]
    fn one_video_or_garbage_is_no_playlist() {
        assert_eq!(
            playlist_of(r#"{"_type":"video","id":"x","formats":[]}"#),
            None
        );
        assert_eq!(playlist_of(r#"{"id":"x","formats":[]}"#), None);
        assert_eq!(playlist_of("not json"), None);
        assert_eq!(
            playlist_of(r#"{"_type":"playlist","entries":[{"url":"ftp://x/1"}]}"#),
            None
        );
    }
}
