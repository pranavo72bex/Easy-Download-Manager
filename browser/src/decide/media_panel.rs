// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The "Download video" panel on a video or audio player.
//!
//! A panel at the top right of a video or audio player, raised when the pointer
//! is over it or it starts playing, that hands the media to the app. The page
//! script draws it; what it offers, where it sits and what a click hands over
//! are decided here.

use super::*;
use crate::words::{self, Word};

/// A video or audio element the page script found.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Media {
    /// `video` or `audio`.
    pub tag: String,
    /// `currentSrc`, falling back to `src`.
    pub src: String,
    /// The element's width in pixels.
    pub width: f64,
    /// The element's height in pixels.
    pub height: f64,
}

/// What the panel offers for one element.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    /// The panel's heading, said in the browser's language where it is drawn.
    pub title: Word,
    /// Where the file comes from, or how it is fetched: the bar's tooltip.
    pub sub: String,
    /// A player page rather than a file: its qualities are listed, so the
    /// bar carries a ▾.
    pub page: bool,
}

/// Below these a media element is decoration — an avatar, a spacer, a muted
/// background loop — and a download button over it would be noise.
pub const MIN_VIDEO: (f64, f64) = (200.0, 120.0);
/// The smallest audio element that gets a panel, in pixels (width, height).
pub const MIN_AUDIO: (f64, f64) = (160.0, 24.0);

/// Whether a media element gets the panel, and what it says.
pub fn offer(media: &Media) -> Option<Offer> {
    let (min_w, min_h, title) = match media.tag.to_ascii_lowercase().as_str() {
        "video" => (MIN_VIDEO.0, MIN_VIDEO.1, words::PANEL_DOWNLOAD_VIDEO),
        "audio" => (MIN_AUDIO.0, MIN_AUDIO.1, words::PANEL_DOWNLOAD_AUDIO),
        _ => return None,
    };
    if !(media.width >= min_w && media.height >= min_h) {
        return None;
    }
    let src = media.src.trim();
    let sub = if handable(src) {
        dm_protocol::host_of(src)
    } else {
        // A `blob:` or Media Source player: no file exists to fetch, so the
        // page goes to the app, which reads it.
        words::PANEL_THROUGH_APP.say()
    };
    Some(Offer {
        title,
        sub,
        page: !handable(src),
    })
}

/// Narrower than this, a video gets the compact bar: the download mark and the close mark
/// alone, as reference A shrinks its bar on a narrow video (gaps row 76).
pub const COMPACT_BELOW: f64 = 400.0;

/// The bar's size, whole or compact.
pub fn panel_size(video_width: f64) -> (f64, f64) {
    if video_width < COMPACT_BELOW {
        (COMPACT_WIDTH, 30.0)
    } else {
        (240.0, 30.0)
    }
}

/// How wide the compact bar is: the mark's button and the close button.
pub const COMPACT_WIDTH: f64 = 72.0;

/// The part of the element a person sees: inside the viewport and below a page's fixed bar
/// over its top (`covered_to`, that bar's bottom), so the panel sits on the picture and not
/// under a sticky header (gaps row 76). The element's own box where nothing of it is seen.
pub fn visible_box(
    element: (f64, f64, f64, f64),
    viewport: (f64, f64),
    covered_to: Option<f64>,
) -> (f64, f64, f64, f64) {
    let (left, top, width, height) = element;
    let (view_w, view_h) = viewport;
    let new_left = left.max(0.0);
    let new_top = top.max(0.0).max(covered_to.unwrap_or(f64::MIN));
    let right = (left + width).min(view_w);
    let bottom = (top + height).min(view_h);
    if !(right > new_left && bottom > new_top)
        || [new_left, new_top, right, bottom]
            .iter()
            .any(|v| !v.is_finite())
    {
        return element;
    }
    (new_left, new_top, right - new_left, bottom - new_top)
}

/// Where the panel goes: inside the element's top right corner, kept on
/// screen. All in CSS pixels of the viewport.
pub fn panel_at(
    element: (f64, f64, f64, f64),
    panel: (f64, f64),
    viewport: (f64, f64),
) -> (f64, f64) {
    let (left, top, width, _height) = element;
    let (panel_w, panel_h) = panel;
    let (view_w, view_h) = viewport;
    let margin = 8.0;
    let x = (left + width - panel_w - margin)
        .min(view_w - panel_w - margin)
        .max(margin);
    let y = (top + margin).min(view_h - panel_h - margin).max(margin);
    // A NaN from a page's odd layout lands in the corner, not off screen.
    (
        if x.is_finite() { x } else { margin },
        if y.is_finite() { y } else { margin },
    )
}

/// Where the panel goes moved by `by` from `spot` (where `panel_at` puts
/// it), kept whole inside the viewport.
pub fn dragged(
    spot: (f64, f64),
    by: (f64, f64),
    panel: (f64, f64),
    viewport: (f64, f64),
) -> (f64, f64) {
    let margin = 8.0;
    let clamp = |v: f64, size: f64, room: f64| {
        let most = (room - size - margin).max(margin);
        if v.is_finite() {
            v.clamp(margin, most)
        } else {
            margin
        }
    };
    (
        clamp(spot.0 + by.0, panel.0, viewport.0),
        clamp(spot.1 + by.1, panel.1, viewport.1),
    )
}

/// What the page script sends when the panel is clicked.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelClick {
    /// The kind of click: what the page script reports.
    #[serde(rename = "type")]
    pub kind: String,
    /// The address of the element's media.
    pub src: String,
    /// The address of the frame the player is in.
    pub frame: String,
    /// The quality picked from the panel's list, and what the list said.
    pub quality: Option<String>,
    /// What the quality's list entry said.
    pub label: Option<String>,
    /// A sound the player fetched apart from the picture at `src`.
    pub sound: Option<String>,
    /// The panel's "Part" line: where the part starts, as typed.
    pub from: Option<String>,
    /// Where the part ends, as typed.
    pub to: Option<String>,
    /// The page's title, to name a file whose own name says nothing.
    pub title: Option<String>,
}

/// How long the panel waits for the list of qualities before it gives up
/// and says so, in milliseconds: the background waits at most 40 s for the
/// app, so this is longer than that and no more.
pub const FORMATS_PATIENCE_MS: i32 = 50_000;

/// How long the app's reading of a page is waited for, in milliseconds,
/// when the streams the page's player loaded already make a list of their own.
pub const STREAMS_PATIENCE_MS: f64 = 8_000.0;

/// Whether the app's reading of a page may be cut short, after
/// [`STREAMS_PATIENCE_MS`], for the `stream_lines` lines the page's own streams
/// make: only when they offer a choice by themselves. One line is no choice:
/// a click answered with it alone hands that one over at once, at whatever
/// quality the player happened to fetch, while the qualities the app was
/// still reading go unseen (YouTube on macOS, 2026-11-11, gaps row 178).
pub fn cut_reading_short(stream_lines: usize) -> bool {
    stream_lines >= 2
}

/// How long a click on a YouTube player waits for the player to list the
/// video's qualities, in milliseconds, when it has no list yet (still
/// loading, or playing an advertisement first) and the app has given fewer
/// than two lines: past it, what the app gave is used as before.
pub const PLAYER_LIST_PATIENCE_MS: f64 = 15_000.0;

/// How often the player is asked again for its list while a click waits.
pub const PLAYER_ASK_EVERY_MS: i32 = 500;

/// The page script telling what a video player asked for.
pub const MEDIA_SEEN_MESSAGE: &str = "dm.media-seen";

/// The page script asking what qualities a player's video comes in.
pub const FORMATS_MESSAGE: &str = "dm.formats";

/// What the panel's "Part" line was typed as, ready to hand over: only a
/// short run of digits, colons, dots and the letters h, m, s ("1:30",
/// "90", "1m30s") is kept, so nothing else a page might have put in the box
/// travels to the app.
pub fn part_text(typed: &str) -> String {
    typed
        .trim()
        .chars()
        .filter(|c| {
            c.is_ascii_digit() || matches!(c, ':' | '.' | 'h' | 'm' | 's' | 'H' | 'M' | 'S')
        })
        .take(16)
        .collect()
}

/// The most of a video's title a list line carries before "…".
pub const LINE_TITLE_MOST: usize = 40;

/// A quality line as reference A words it (gaps row 72a): its number, the
/// video's title and "…", then the kind and quality. A bare height ("1080p",
/// what the player names) is said as a file, "MP4 file, quality 1080p HD",
/// HD from 720 up; any other line (a stream, a page's own "MP4 file, …") is
/// kept as it is.
pub fn reference_line(number: usize, title: &str, line: &str) -> String {
    // A size after the quality ("1080p — 50.1 MB", `choice_line`) stays after it.
    let (head, size) = match line.split_once(" — ") {
        Some((head, size)) => (head, format!(" — {size}")),
        None => (line, String::new()),
    };
    // "1080p", or dm's reading's "1080p MP4": a height, and the container
    // where one is named (MP4 where none is, as dm saves the player's).
    let (height, container) = match head.split_once(' ') {
        Some((height, container)) => (height, container),
        None => (head, "MP4"),
    };
    // A height listed at two frame rates names the faster ("720p MP4 60 fps"),
    // said after the quality as reference A says it: "720p HD 60 fps".
    let (container, rate) = match container.rsplit_once(' ') {
        Some((front, "fps")) => match front.rsplit_once(' ') {
            Some((kind, n)) if n.parse::<u32>().is_ok() => (kind, format!(" {n} fps")),
            _ => (container, String::new()),
        },
        _ => (container, String::new()),
    };
    let quality = match height.strip_suffix('p').and_then(|h| h.parse::<u32>().ok()) {
        Some(height) if !container.contains(' ') => words::PANEL_FILE_QUALITY.with(&[
            container,
            &format!(
                "{height}p{}{rate}{size}",
                if height >= 720 { " HD" } else { "" }
            ),
        ]),
        _ => line.to_string(),
    };
    let title = title.trim();
    if title.is_empty() {
        return format!("{number}. {quality}");
    }
    let short: String = title.chars().take(LINE_TITLE_MOST).collect();
    format!("{number}. {}… {quality}", short.trim_end())
}

/// One line of the panel's list: the format and size.
pub fn choice_line(label: &str, size: Option<u64>) -> String {
    match size {
        Some(bytes) => format!("{label} — {}", human_size(bytes)),
        None => label.to_string(),
    }
}

/// Bytes the way a download list says them.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    let unit = UNITS.get(unit).copied().unwrap_or("TB");
    if value < 10.0 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

/// The message type a panel click is sent as.
pub const PANEL_MESSAGE: &str = "dm.media";

/// A page that found a player asking that the app's rules be current, so
/// the panel switch in the app's Options reaches it within a minute of
/// being changed rather than at the next download.
pub const RULES_MESSAGE: &str = "dm.rules";

/// Alt-click: a link clicked with Alt held is left to the browser.
/// The page script says so before the browser starts the download.
pub const SKIP_MESSAGE: &str = "dm.skip";

/// A click with the rules' take key held: its download goes to the app
/// whatever the types and size floor say.
pub const TAKE_MESSAGE: &str = "dm.take";

/// A click on a magnet link on a page: the torrent is the app's, as a
/// clicked `.torrent` file is. Left alone, it went to whatever the system
/// opens magnets with, which on macOS and Windows was never dm.
pub const MAGNET_MESSAGE: &str = "dm.magnet";

/// A plain click on a link to a file the browser would show in the tab (a
/// PDF, a video, a song) instead of downloading it. Such a click starts no
/// download, so dm never saw it; the page script asks the app to take it, and
/// the browser opens it as it would have where the app cannot.
pub const INLINE_MESSAGE: &str = "dm.inline";

/// The page script asking for the panel's code once its page shows media: the background puts
/// [`PANEL_FILE`] into the asking frame itself (an `import()` from the page was checked against
/// the page's origin, and Brave refused it on YouTube).
pub const PANEL_LOAD_MESSAGE: &str = "dm.panel-load";

/// The panel's code with its WebAssembly, as `scripts/package.sh` builds it.
pub const PANEL_FILE: &str = "pkg/panel.js";

/// The address a plain click on an inline file asks the app to take.
pub fn inline_url(click: &PanelClick) -> Result<String, String> {
    if click.kind != INLINE_MESSAGE {
        return Err("that is not a click on a file the browser shows".into());
    }
    web_address(&click.src)
}

/// The magnet a clicked link names, if it names a torrent.
pub fn magnet_url(click: &PanelClick) -> Result<String, String> {
    if click.kind != MAGNET_MESSAGE {
        return Err("that is not a magnet link's click".into());
    }
    let url = click.src.trim();
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("magnet:?") && lower.contains("xt=urn:bt") {
        Ok(url.to_string())
    } else {
        Err("that magnet link names no torrent".into())
    }
}

/// What to call a magnet in a notice: its `dn=`, or "the torrent".
pub fn magnet_name(url: &str) -> String {
    url.split(['?', '&'])
        .find_map(|part| part.strip_prefix("dn="))
        .map(|name| name.replace('+', " "))
        .map(|name| percent_decoded(&name))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "the torrent".to_string())
}

/// `%XX` escapes undone, bytes read as UTF-8 where they make it.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16);
    while let Some(&byte) = bytes.get(i) {
        match (byte, bytes.get(i + 1), bytes.get(i + 2)) {
            (b'%', Some(&h), Some(&l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push((hex(h).unwrap_or(0) * 16 + hex(l).unwrap_or(0)) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The address a click with the take key held asks the app to take.
pub fn take_url(click: &PanelClick) -> Result<String, String> {
    if click.kind != TAKE_MESSAGE {
        return Err("that is not a click to hand over".into());
    }
    web_address(&click.src)
}

/// The address an Alt-click asks to leave with the browser, if it is one a
/// download could come from.
pub fn skip_url(click: &PanelClick) -> Result<String, String> {
    if click.kind != SKIP_MESSAGE {
        return Err("that is not an Alt-click".into());
    }
    web_address(&click.src)
}

fn web_address(src: &str) -> Result<String, String> {
    let url = src.trim();
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(url.to_string())
    } else {
        Err("only a web address is a download to leave alone".into())
    }
}

/// What a panel click hands over: the file, or the page for a player that
/// has none. `tab_url` is the top page, as the browser says it.
pub fn panel_target(click: &PanelClick, tab_url: &str, private: bool) -> Result<Target, String> {
    if click.kind != PANEL_MESSAGE && click.kind != FORMATS_MESSAGE {
        return Err("that is not a message from dm's panel".into());
    }
    let menu = MenuClick {
        menu_item_id: Value::String(MENU_MEDIA.to_string()),
        src_url: Some(click.src.clone()),
        page_url: Some(tab_url.to_string()),
        frame_url: Some(click.frame.clone()).filter(|f| !f.is_empty() && f != tab_url),
        ..MenuClick::default()
    };
    menu_target(&menu, private)
}

/// The video's id in a YouTube watch, Shorts, embed or live address, on YouTube, its no-cookie
/// host or YouTube Music: the pages whose own player lists its qualities for the panel, an
/// embedded player on another site's page included (gaps row Y12). `None` elsewhere.
pub fn youtube_video_id(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .or_else(|| host.strip_prefix("music."))
        .unwrap_or(host);
    if host != "youtube.com" && host != "youtube-nocookie.com" {
        return None;
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let id = if path == "watch" {
        query.split('&').find_map(|pair| pair.strip_prefix("v="))?
    } else {
        ["shorts/", "embed/", "live/"]
            .iter()
            .find_map(|kind| path.strip_prefix(kind))?
            .split(['/', '?', '#'])
            .next()?
    };
    let id: String = id
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    (!id.is_empty()).then_some(id)
}

/// The heights YouTube's player names its qualities by ("hd1080"), tallest
/// first, each once; words it does not use are passed over.
pub fn player_heights(levels: &[String]) -> Vec<u32> {
    let mut heights: Vec<u32> = levels
        .iter()
        .filter_map(|level| match level.as_str() {
            "highres" => Some(4320),
            "hd2880" => Some(2880),
            "hd2160" => Some(2160),
            "hd1440" => Some(1440),
            "hd1080" => Some(1080),
            "hd720" => Some(720),
            "large" => Some(480),
            "medium" => Some(360),
            "small" => Some(240),
            "tiny" => Some(144),
            _ => None,
        })
        .collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    heights
}
