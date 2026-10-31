//! The qualities a YouTube watch page lists in its own player data.
//!
//! yt-dlp takes seconds over a page (the player's script, its challenge,
//! several API calls), and the panel's list waits for it. The page itself
//! already holds every picture and sound the video comes in, with sizes, in
//! `ytInitialPlayerResponse`: one request of under a second. Only the list
//! is read this way. The addresses are not there (the player streams by
//! SABR), so what is downloaded is still yt-dlp's reading of the page. Where
//! the data is missing or says anything but "playable", nothing is returned
//! and the caller asks yt-dlp, as before.

use serde_json::Value;

use super::Choice;

/// Whether `url` is one YouTube's player data is read from: a watch page or
/// a short link, not a playlist, a channel or another site.
pub(super) fn is_watch_page(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    match host.as_str() {
        "youtu.be" => parsed.path().len() > 1,
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => {
            (parsed.path() == "/watch"
                && parsed.query_pairs().any(|(k, v)| k == "v" && !v.is_empty()))
                || parsed.path().starts_with("/shorts/")
        }
        _ => false,
    }
}

/// Fetches the page and lists what it holds, or `None` for anything that
/// should go to yt-dlp instead: no connection in time, a consent or sign-in
/// page, a live stream, a video the page does not play.
pub(super) async fn fetch_choices(
    url: &str,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Option<Vec<Choice>> {
    let mut builder = crate::with_tls(reqwest::Client::builder())
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
        )
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10));
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy).ok()?);
    }
    let client = builder.build().ok()?;
    // `SOCS` answers the consent page some countries are sent to first.
    let mut cookie = String::from("SOCS=CAI");
    if let Some(theirs) = crate::cookies::header_with(sign_in, url) {
        cookie = format!("{theirs}; {cookie}");
    }
    let reply = client
        .get(url.trim())
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Cookie", cookie)
        .send()
        .await
        .ok()?;
    if !reply.status().is_success() {
        return None;
    }
    let page = reply.text().await.ok()?;
    let list = choices_of_page(&page);
    (!list.is_empty()).then_some(list)
}

/// The choices in a watch page's HTML, in the same shape and order as
/// [`super::choices`] gives from yt-dlp's reading: one per picture height,
/// tallest first, MP4 where there is one, then the best sound alone. Empty
/// where the page's data is absent, unplayable or live.
pub(super) fn choices_of_page(html: &str) -> Vec<Choice> {
    const MARK: &str = "ytInitialPlayerResponse = ";
    let Some(at) = html.find(MARK) else {
        return Vec::new();
    };
    let mut data =
        serde_json::Deserializer::from_str(&html[at + MARK.len()..]).into_iter::<Value>();
    let Some(Ok(player)) = data.next() else {
        return Vec::new();
    };
    choices_of_player(&player)
}

fn choices_of_player(player: &Value) -> Vec<Choice> {
    let status = player
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str);
    let live = player
        .pointer("/videoDetails/isLive")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || player
            .pointer("/videoDetails/isLiveContent")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && player.pointer("/streamingData/hlsManifestUrl").is_some();
    if status != Some("OK") || live {
        return Vec::new();
    }
    let Some(formats) = player
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    // "video/mp4; codecs=\"avc1.640028\"" is an MP4 picture, "audio/mp4" an
    // M4A sound; WebM is WebM, as yt-dlp names them.
    let kind = |f: &Value| -> Option<(bool, String)> {
        let mime = f.get("mimeType")?.as_str()?;
        let (kind, rest) = mime.split_once('/')?;
        let container = rest.split(';').next()?.trim().to_ascii_lowercase();
        match kind {
            "video" => Some((true, container)),
            "audio" => Some((
                false,
                if container == "mp4" {
                    "m4a".to_string()
                } else {
                    container
                },
            )),
            _ => None,
        }
    };
    let size = |f: &Value| {
        f.get("contentLength")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
    };
    let audio = formats
        .iter()
        .filter(|f| kind(f).is_some_and(|(video, _)| !video))
        .max_by_key(|f| {
            (
                kind(f).is_some_and(|(_, ext)| ext == "m4a"),
                f.get("averageBitrate")
                    .or_else(|| f.get("bitrate"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )
        });
    let audio_size = audio.and_then(size);
    let mut by_height: Vec<(u32, String, String, Option<u64>)> = Vec::new();
    for f in formats {
        let Some((true, ext)) = kind(f) else {
            continue;
        };
        let Some(height) = f.get("height").and_then(Value::as_u64).map(|h| h as u32) else {
            continue;
        };
        if height == 0 {
            continue;
        }
        let width = f.get("width").and_then(Value::as_u64).unwrap_or(0);
        let label = if width > 0 {
            format!("{} {width}×{height}", ext.to_ascii_uppercase())
        } else {
            format!("{} {height}p", ext.to_ascii_uppercase())
        };
        // A picture alone is joined with the sound, so its size counts both.
        let total = size(f).map(|s| s + audio_size.unwrap_or(0));
        let rank = |ext: &str, size: Option<u64>| (ext == "mp4", size);
        match by_height.iter_mut().find(|(h, _, _, _)| *h == height) {
            Some(slot) if rank(&ext, total) > rank(&slot.2, slot.3) => {
                *slot = (height, label, ext, total)
            }
            Some(_) => {}
            None => by_height.push((height, label, ext, total)),
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
                label: super::choices::shown_label(&label, &ext, height),
                quality: format!("{side}/{ext}"),
                size,
            }
        })
        .collect();
    if let Some((_, ext)) = audio.and_then(kind) {
        list.push(Choice {
            label: format!("{} audio", ext.to_ascii_uppercase()),
            quality: format!("audio/{ext}"),
            size: audio_size,
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(player: &str) -> String {
        format!("<html><script>var ytInitialPlayerResponse = {player};var meta = 1;</script>")
    }

    const PLAYABLE: &str = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"isLive":false},
      "streamingData":{"adaptiveFormats":[
        {"itag":137,"mimeType":"video/mp4; codecs=\"avc1.640028\"","width":1920,"height":1080,"contentLength":"8000"},
        {"itag":248,"mimeType":"video/webm; codecs=\"vp9\"","width":1920,"height":1080,"contentLength":"3000"},
        {"itag":136,"mimeType":"video/mp4; codecs=\"avc1.4d401f\"","width":1280,"height":720,"contentLength":"2000"},
        {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500"},
        {"itag":251,"mimeType":"audio/webm; codecs=\"opus\"","averageBitrate":140000,"contentLength":"400"}]}}"#;

    #[test]
    fn a_pages_own_data_gives_the_list_yt_dlp_would() {
        let list = choices_of_page(&page(PLAYABLE));
        let said: Vec<(&str, &str, Option<u64>)> = list
            .iter()
            .map(|c| (c.label.as_str(), c.quality.as_str(), c.size))
            .collect();
        assert_eq!(
            said,
            [
                ("1080p MP4", "1080/mp4", Some(8500)),
                ("720p MP4", "720/mp4", Some(2500)),
                ("M4A audio", "audio/m4a", Some(500)),
            ]
        );
    }

    #[test]
    fn a_portrait_videos_qualities_are_named_by_the_shorter_side() {
        // A Short: 1080x1920 and 720x1280 are 1080 and 720, as the chooser
        // reads them, so a click on the second fetches the 720-wide stream.
        let short = r#"{"playabilityStatus":{"status":"OK"},"videoDetails":{"isLive":false},
          "streamingData":{"adaptiveFormats":[
            {"itag":1,"mimeType":"video/mp4; codecs=\"avc1\"","width":1080,"height":1920,"contentLength":"9000"},
            {"itag":2,"mimeType":"video/mp4; codecs=\"avc1\"","width":720,"height":1280,"contentLength":"4000"},
            {"itag":140,"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","averageBitrate":130000,"contentLength":"500"}]}}"#;
        let words: Vec<String> = choices_of_page(&page(short))
            .into_iter()
            .map(|c| c.quality)
            .collect();
        assert_eq!(words, ["1080/mp4", "720/mp4", "audio/m4a"]);
    }

    #[test]
    fn a_page_that_does_not_play_or_is_live_or_has_no_data_gives_nothing() {
        let blocked = PLAYABLE.replace(r#""status":"OK""#, r#""status":"LOGIN_REQUIRED""#);
        assert!(choices_of_page(&page(&blocked)).is_empty());
        let live = PLAYABLE.replace(r#""isLive":false"#, r#""isLive":true"#);
        assert!(choices_of_page(&page(&live)).is_empty());
        assert!(choices_of_page("<html>consent</html>").is_empty());
        assert!(choices_of_page("var ytInitialPlayerResponse = {broken").is_empty());
    }

    #[test]
    fn only_watch_pages_are_read_this_way() {
        for yes in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=RD",
            "https://m.youtube.com/watch?v=abc",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://www.youtube.com/shorts/abc",
        ] {
            assert!(is_watch_page(yes), "{yes}");
        }
        for no in [
            "https://www.youtube.com/playlist?list=PL1",
            "https://www.youtube.com/@channel",
            "https://www.youtube.com/watch",
            "https://evil.example/watch?v=abc",
            "https://youtu.be/",
            "https://notyoutube.com/watch?v=a",
        ] {
            assert!(!is_watch_page(no), "{no}");
        }
    }
}
