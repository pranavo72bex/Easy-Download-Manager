//! The "Download video" panel on a video or audio player.
//!
//! A panel at the top right of a video or audio player, raised when the pointer
//! is over it or it starts playing, that hands the media to the app. The page
//! script draws it; what it offers, where it sits and what a click hands over
//! are decided here.

use super::*;

/// A video or audio element the page script found.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Media {
    /// `video` or `audio`.
    pub tag: String,
    /// `currentSrc`, falling back to `src`.
    pub src: String,
    pub width: f64,
    pub height: f64,
}

/// What the panel offers for one element.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub title: &'static str,
    /// Where the file comes from, or how it is fetched: the bar's tooltip.
    pub sub: String,
    /// A player page rather than a file: its qualities are listed, so the
    /// bar carries a ▾.
    pub page: bool,
}

/// Below these a media element is decoration — an avatar, a spacer, a muted
/// background loop — and a download button over it would be noise.
pub const MIN_VIDEO: (f64, f64) = (200.0, 120.0);
pub const MIN_AUDIO: (f64, f64) = (160.0, 24.0);

/// Whether a media element gets the panel, and what it says.
pub fn offer(media: &Media) -> Option<Offer> {
    let (min_w, min_h, title) = match media.tag.to_ascii_lowercase().as_str() {
        "video" => (MIN_VIDEO.0, MIN_VIDEO.1, "Download video"),
        "audio" => (MIN_AUDIO.0, MIN_AUDIO.1, "Download this audio"),
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
        // page goes to the app, whose yt-dlp reads it.
        "this page, through the dm app".to_string()
    };
    Some(Offer {
        title,
        sub,
        page: !handable(src),
    })
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
    #[serde(rename = "type")]
    pub kind: String,
    pub src: String,
    /// The address of the frame the player is in.
    pub frame: String,
    /// The quality picked from the panel's list, and what the list said.
    pub quality: Option<String>,
    pub label: Option<String>,
    /// A sound the player fetched apart from the picture at `src`.
    pub sound: Option<String>,
}

/// How long the panel waits for the list of qualities before it gives up
/// and says so, in milliseconds: the background waits at most 40 s for the
/// app, so this is longer than that and no more.
pub const FORMATS_PATIENCE_MS: i32 = 50_000;

/// The page script telling what a video player asked for.
pub const MEDIA_SEEN_MESSAGE: &str = "dm.media-seen";

/// The page script asking what qualities a player's video comes in.
pub const FORMATS_MESSAGE: &str = "dm.formats";

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

/// What the panel says over a copy-protected player (one the page plays
/// through the browser's DRM, EME): offered nothing, and said so, rather
/// than handing over something that could only fail or come out scrambled.
pub const PROTECTED_TITLE: &str = "Protected video";
pub const PROTECTED_SAID: &str =
    "This video is copy-protected, so only the site's own player can play it. dm does not download protected video.";

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
