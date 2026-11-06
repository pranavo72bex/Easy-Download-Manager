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
    /// Bytes, where the reader knows or estimates them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// The choices in a reader's JSON description of a page: one per picture
/// height, tallest first, each with the biggest file at that height (and the
/// sound it would be joined with), then the best sound alone. A height with
/// pictures above 30 frames a second and at or below it has a line for each,
/// the faster first, as reference A lists "720p HD 60 fps" and "720p HD". Nothing it
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

    // Height, faster than 30 frames a second, label, container, size, frame rate.
    let mut by_height: Vec<(u32, bool, String, String, Option<u64>, u64)> = Vec::new();
    for f in &formats {
        let Some(height) = f.get("height").and_then(|v| v.as_u64()).map(|h| h as u32) else {
            continue;
        };
        // A picture where it has a height, unless the reading says it has none:
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
        let fps = f.get("fps").and_then(|v| v.as_f64()).unwrap_or(0.0).round() as u64;
        let fast = fps > 30;
        // MP4 at a height wherever there is one; another container only where MP4 has nothing that tall.
        let rank = |ext: &str, size: Option<u64>| (ext == "mp4", size);
        match by_height
            .iter_mut()
            .find(|slot| slot.0 == height && slot.1 == fast)
        {
            Some(slot) if rank(&ext, size) > rank(&slot.3, slot.4) => {
                *slot = (height, fast, label, ext, size, fps)
            }
            Some(_) => {}
            None => by_height.push((height, fast, label, ext, size, fps)),
        }
    }
    by_height.sort_by_key(|slot| (std::cmp::Reverse(slot.0), std::cmp::Reverse(slot.1)));
    let twice: Vec<u32> = by_height
        .iter()
        .filter(|slot| slot.1)
        .map(|slot| slot.0)
        .filter(|h| by_height.iter().any(|slot| slot.0 == *h && !slot.1))
        .collect();
    let mut list: Vec<Choice> = by_height
        .into_iter()
        .map(|(height, fast, label, ext, size, fps)| {
            // The quality word is the picture's shorter side, which is what
            // the chooser and the selector measure: a portrait video's
            // width (720 for 720x1280).
            let side = label
                .rsplit(' ')
                .next()
                .and_then(|wh| wh.split_once('×'))
                .and_then(|(w, _)| w.parse::<u32>().ok())
                .filter(|w| *w > 0 && *w < height)
                .unwrap_or(height);
            // A height listed twice says which of its frame rates each line is.
            let (label, quality) = if fast {
                (
                    format!("{} {fps} fps", shown_label(&label, &ext, height)),
                    format!("{side}/{ext}/{fps}"),
                )
            } else if twice.contains(&height) {
                (
                    shown_label(&label, &ext, height),
                    format!("{side}/{ext}/30"),
                )
            } else {
                (shown_label(&label, &ext, height), format!("{side}/{ext}"))
            };
            Choice {
                label,
                quality,
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

/// A picture's line as a person reads it: its class ("4K", "1080p") and
/// its container, from the "EXT WxH" (or "EXT Hp") the list is built with.
pub(crate) fn shown_label(built: &str, ext: &str, height: u32) -> String {
    let width = built
        .rsplit(' ')
        .next()
        .and_then(|wh| wh.split_once('×'))
        .and_then(|(w, _)| w.parse::<u32>().ok())
        .unwrap_or(0);
    format!("{} {}", class_name(width, height), ext.to_ascii_uppercase())
}

/// The class a picture is known by: the larger of its shorter side and its
/// longer side's 16:9 height, snapped to the nearest of 144p ... 4K, 8K
/// (3840x2026 is 4K, a portrait 720x1280 is 720p); else its own lines.
pub fn class_name(width: u32, height: u32) -> String {
    // In u64: the sides come from a page's own numbers, which can be any.
    let (short, long) = if width == 0 {
        (u64::from(height), u64::from(height) * 16 / 9)
    } else {
        (u64::from(width.min(height)), u64::from(width.max(height)))
    };
    let value = short.max(long * 9 / 16) as f64;
    let classes = [144u32, 240, 360, 480, 720, 1080, 1440, 2160, 4320];
    let nearest = classes
        .iter()
        .copied()
        .min_by(|a, b| {
            let d = |c: u32| (f64::from(c) - value).abs();
            d(*a).total_cmp(&d(*b))
        })
        .unwrap_or(0);
    if (f64::from(nearest) - value).abs() > value * 0.1 {
        return format!("{short}p");
    }
    match nearest {
        2160 => "4K".into(),
        4320 => "8K".into(),
        n => format!("{n}p"),
    }
}

/// What a page offers, for the panel's list. A YouTube watch page is read
/// from its own player data ([`list_choices_fast`], under a second).
/// Non-YouTube sites have no quality list: their video comes from
/// the browser extension's caught player requests.
pub async fn list_choices(
    url: &str,
    _referrer: Option<&str>,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Vec<Choice>, Error> {
    if let Some(list) = list_choices_fast(url, proxy, sign_in).await {
        return Ok(list);
    }
    Ok(Vec::new())
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
/// a reader run: `None` for another site or a page that does not give one.
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

    #[test]
    fn a_picture_is_named_by_its_class_whatever_its_shape() {
        assert_eq!(class_name(3840, 2026), "4K");
        assert_eq!(class_name(2560, 1350), "1440p");
        assert_eq!(class_name(1920, 1014), "1080p");
        assert_eq!(class_name(1920, 1080), "1080p");
        assert_eq!(class_name(720, 1280), "720p");
        assert_eq!(class_name(426, 224), "240p");
        assert_eq!(class_name(7680, 4320), "8K");
        assert_eq!(class_name(0, 720), "720p");
        // Nothing near a class: its own lines.
        assert_eq!(class_name(1000, 900), "900p");
        // A page's numbers out of all proportion: named, not a panic.
        assert_eq!(class_name(0, u32::MAX), format!("{}p", u32::MAX));
        assert_eq!(class_name(u32::MAX, u32::MAX), format!("{}p", u32::MAX));
    }
}
