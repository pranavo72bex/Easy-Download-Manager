//! The qualities a page offers, for the "Download video" panel's list.

use super::*;

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
        // `vcodec: null`, which left a stream with no list.
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
        .map(|(height, label, ext, size)| {
            // The quality word is the picture's shorter side, which is what
            // the chooser and yt-dlp's selector measure: a portrait video's
            // width (720 for 720x1280).
            let side = label
                .rsplit(' ')
                .next()
                .and_then(|wh| wh.split_once('×'))
                .and_then(|(w, _)| w.parse::<u32>().ok())
                .filter(|w| *w > 0 && *w < height)
                .unwrap_or(height);
            Choice {
                label,
                quality: format!("{side}/{ext}"),
                size,
            }
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

/// What a page offers, for the panel's list. A YouTube watch page is read
/// from its own player data first ([`list_choices_fast`], under a second);
/// anything that does not give a list that way, and every other site, is
/// read by yt-dlp ([`read_choices`]).
pub async fn list_choices(
    url: &str,
    referrer: Option<&str>,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Vec<Choice>, Error> {
    if let Some(list) = list_choices_fast(url, proxy, sign_in).await {
        return Ok(list);
    }
    read_choices(url, referrer, proxy, sign_in).await
}

/// `list` with the sound as MP3 offered after its M4A sound, where dm can
/// make one (`crate::mp3::choice_after`).
pub fn with_mp3(mut list: Vec<Choice>) -> Vec<Choice> {
    if let Some(at) = list.iter().position(|c| c.quality == "audio/m4a") {
        if let Some(mp3) = crate::mp3::choice_after(list[at].size) {
            list.insert(at + 1, mp3);
        }
    }
    list
}

/// The panel's list from a YouTube watch page's own player data, with no
/// yt-dlp run: `None` for another site or a page that does not give one.
pub async fn list_choices_fast(
    url: &str,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Option<Vec<Choice>> {
    if !page_data::is_watch_page(url) {
        return None;
    }
    page_data::fetch_choices(url, proxy, sign_in)
        .await
        .map(with_mp3)
}

/// Asks yt-dlp what a page offers, for the panel's list. Bounded: a page
/// yt-dlp cannot read in half a minute gets no list, and the panel falls
/// back to downloading the best. Its whole reading is kept for the
/// download (`remember_reading`).
pub async fn read_choices(
    url: &str,
    referrer: Option<&str>,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Vec<Choice>, Error> {
    // A YouTube watch page is read through one of yt-dlp's own clients
    // first: asked of its default two, one after the other, the read takes
    // about a second longer (2.0 s against 3.2 s, the same 31 formats up to
    // 2160p). Where that gives nothing (the client gone from this yt-dlp, a
    // video it does not serve), the default read is made as before.
    if page_data::is_watch_page(url) {
        if let Ok(list) =
            read_choices_as(url, referrer, proxy, sign_in, Some(FAST_YOUTUBE_CLIENT)).await
        {
            if !list.is_empty() {
                return Ok(with_mp3(list));
            }
        }
    }
    read_choices_as(url, referrer, proxy, sign_in, None)
        .await
        .map(with_mp3)
}

/// The yt-dlp YouTube client whose read is the quickest that still lists
/// every height.
const FAST_YOUTUBE_CLIENT: &str = "visionos";

async fn read_choices_as(
    url: &str,
    referrer: Option<&str>,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
    client: Option<&str>,
) -> Result<Vec<Choice>, Error> {
    let mut unknown_failure = false;
    match read_choices_once(url, referrer, proxy, sign_in, client, &mut unknown_failure).await {
        // As `download_ytdlp_from`: a newer yt-dlp fetched and one more
        // try, where the failure was not a site's refusal (gaps.md row 33).
        Err(why) if unknown_failure && crate::tools::newer_ytdlp(&crate::client()).await => {
            eprintln!("dm: yt-dlp could not read the page ({why}); a newer yt-dlp was fetched, so once more");
            let mut again = false;
            read_choices_once(url, referrer, proxy, sign_in, client, &mut again).await
        }
        outcome => outcome,
    }
}

/// One read of a page's choices by yt-dlp. `unknown_failure` is set where
/// it failed in words that are not a known refusal.
async fn read_choices_once(
    url: &str,
    referrer: Option<&str>,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
    client: Option<&str>,
    unknown_failure: &mut bool,
) -> Result<Vec<Choice>, Error> {
    let binary = binary()
        .ok_or_else(|| Error::Internal("dm's video helper is not available here".into()))?;
    // Its proxy and cookie files are in the temporary folder, which the
    // sandbox leaves open.
    let mut command = super::writable_to(&binary, &[]);
    crate::no_window_async(&mut command);
    js_runtime(&mut command);
    unpack_where_programs_run(&mut command);
    command
        .arg("-J")
        .arg("--no-playlist")
        // A page that is a playlist and no one video is said at once as a
        // list of names (`playlist::read_playlist` then lists it), not
        // read video by video: 2 s, not 24 s or minutes.
        .arg("--flat-playlist")
        .arg("--no-warnings")
        // A list needs neither the HLS manifests (the same heights are in
        // the DASH ones) nor the translated subtitles: leaving them out
        // saves requests, about a third of the time on YouTube. Other
        // sites ignore it.
        .arg("--extractor-args")
        .arg(match client {
            Some(client) => format!("youtube:skip=hls,translated_subs;player_client={client}"),
            None => "youtube:skip=hls,translated_subs".to_string(),
        })
        .arg("--socket-timeout")
        .arg("15")
        .stdout(Stdio::piped())
        // Kept, so a refusal is said as yt-dlp said it (`said_by`); thrown
        // away, every failure read "could not read that page".
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let _proxied = match proxy {
        Some(proxy) => Some(proxy_by_file(&mut command, &std::env::temp_dir(), proxy).await?),
        None => None,
    };
    if let Some(referrer) = referrer {
        command.arg("--referer").arg(referrer);
    }
    // The page's session from the browser, or the cookies file a person
    // chose, so a video that needs them signed in (or a site's "not a bot"
    // check) is read as their browser would.
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("dm-list-{}-{run}.cookies.txt", std::process::id()));
    let _signed = cookies_by_file(&mut command, &path, sign_in, url).await?;
    // After `--`: whatever the address, yt-dlp never reads it as an option.
    command.arg("--").arg(url);
    let run = command.output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), run)
        .await
        .map_err(|_| Error::Internal("yt-dlp took too long to read the page".into()))?
        .map_err(|e| Error::Internal(format!("yt-dlp would not start: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        *unknown_failure = refusal(&stderr).is_none();
        return Err(Error::Internal(said_by(
            &stderr,
            "yt-dlp could not read that page",
        )));
    }
    // Kept, so the download a click on the list starts does not read the
    // page again (`fetch::download_page`).
    // A playlist's names are no reading a download could use.
    if super::playlist_of(&String::from_utf8_lossy(&output.stdout)).is_none() {
        remember_reading(url, &output.stdout).await;
    }
    Ok(choices(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod mp3_tests {
    use super::*;

    #[test]
    fn the_sound_as_mp3_follows_the_m4a_where_dm_can_make_one() {
        let list = vec![
            Choice {
                label: "MP4 1280×720".into(),
                quality: "720/mp4".into(),
                size: Some(9),
            },
            Choice {
                label: "M4A audio".into(),
                quality: "audio/m4a".into(),
                size: Some(100),
            },
        ];
        let with = with_mp3(list.clone());
        if crate::mp3::possible() {
            assert_eq!(with.len(), 3);
            assert_eq!(with[2].quality, "audio/mp3");
            assert_eq!(with[2].size, Some(150));
        } else {
            assert_eq!(with, list);
        }
        // No M4A sound, no MP3 line.
        let only = vec![list[0].clone()];
        assert_eq!(with_mp3(only.clone()), only);
    }
}
