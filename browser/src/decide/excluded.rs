//! What nobody came to download: a built-in list of addresses and responses
//! the extension never catches, whatever a person's own settings say. Reference
//! A keeps such a list (site sounds, toolbar images, ad files, tiny Windows
//! media, one ad server by name); the masks here are dm's own, written by class
//! and kept to what is plainly an advert, a beacon, a sound a page plays or a
//! piece of a page's furniture, so no file a person wants is lost to them. A
//! person's own sites to leave alone stay in dm's settings (`CatchRules`).

/// Why an address or a response is never caught.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Excluded {
    /// Served by an advertising network.
    Advert,
    /// A measuring or tracking call: a beacon, a pixel.
    Tracker,
    /// A sound a page plays for its own interface: a click, a chime.
    SiteSound,
    /// A page's furniture: a favicon, a sprite sheet, a toolbar's icons.
    InterfaceImage,
    /// Sent by an ad server that names itself (`Server: DCLK-AdSvr`).
    AdServer,
    /// A Windows media file (ASF, WMV, WMA) of 100 KB or less: an ad stub.
    TinyWindowsMedia,
}

impl Excluded {
    /// In words for the extension's log.
    pub fn reason(self) -> &'static str {
        match self {
            Excluded::Advert => "served by an advertising network",
            Excluded::Tracker => "a tracking beacon",
            Excluded::SiteSound => "a sound the page plays for its interface",
            Excluded::InterfaceImage => "part of the page's own furniture",
            Excluded::AdServer => "sent by an ad server",
            Excluded::TinyWindowsMedia => "a Windows media file too small to be anything but an ad",
        }
    }
}

/// A Windows media file at or under this is an ad's stub (reference A's
/// floor).
pub const WINDOWS_MEDIA_FLOOR: u64 = 100 * 1024;

/// A player smaller than this many pixels (wide, high) is a decoration or a
/// beacon, not a video (reference A's floor).
pub const SMALLEST_PLAYER: (f64, f64) = (15.0, 10.0);

/// Advertising networks and trackers by the end of their host name: a host
/// matches when it is the name or a sub-domain of it.
const ADVERT_HOSTS: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "adservice.google.com",
    "adnxs.com",
    "adsrvr.org",
    "advertising.com",
    "amazon-adsystem.com",
    "criteo.com",
    "criteo.net",
    "pubmatic.com",
    "rubiconproject.com",
    "openx.net",
    "casalemedia.com",
    "moatads.com",
    "taboola.com",
    "outbrain.com",
    "smartadserver.com",
    "adform.net",
    "teads.tv",
];

/// Measuring and tracking services by host.
const TRACKER_HOSTS: &[&str] = &[
    "google-analytics.com",
    "googletagmanager.com",
    "googletagservices.com",
    "scorecardresearch.com",
    "hotjar.com",
    "segment.io",
    "mixpanel.com",
    "quantserve.com",
    "newrelic.com",
    "nr-data.net",
    "bat.bing.com",
    "pixel.facebook.com",
];

/// Whole path segments that mean an advert or a beacon wherever they stand.
const ADVERT_SEGMENTS: &[&str] = &["adserver", "adframe", "pagead", "doubleclick", "adserve"];
const TRACKER_FILES: &[&str] = &[
    "pixel.gif",
    "1x1.gif",
    "1x1.png",
    "blank.gif",
    "tracking.gif",
    "track.gif",
    "beacon.gif",
    "clear.gif",
];

/// Names a page's interface sounds go by (with an extension of `SOUND_EXTENSIONS`).
const SOUND_NAMES: &[&str] = &[
    "click",
    "beep",
    "ding",
    "chime",
    "notify",
    "notification",
    "hover",
    "tick",
    "pop",
    "whoosh",
    "alert",
    "error",
    "success",
];
/// Folders a page keeps its interface sounds in.
const SOUND_FOLDERS: &[&str] = &["sounds", "sfx", "soundfx", "ui-sounds"];
const SOUND_EXTENSIONS: &[&str] = &["wav", "mid", "midi", "au"];

/// Folders and names of a page's furniture, with an image extension.
const FURNITURE_FOLDERS: &[&str] = &["toolbar", "toolbars", "sprites", "icons", "favicons"];
const FURNITURE_NAMES: &[&str] = &["favicon", "sprite", "sprites", "toolbar", "spacer"];
const IMAGE_EXTENSIONS: &[&str] = &["png", "gif", "ico", "svg", "jpg", "jpeg", "webp"];

/// The Windows media types.
const WINDOWS_MEDIA_EXTENSIONS: &[&str] = &["asf", "wmv", "wma", "asx"];
const WINDOWS_MEDIA_TYPES: &[&str] = &[
    "video/x-ms-asf",
    "video/x-ms-wmv",
    "application/vnd.ms-asf",
    "audio/x-ms-wma",
    "application/x-mplayer2",
];

fn host_is(host: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| host == *name || host.ends_with(&format!(".{name}")))
}

fn path_and_file(url: &str) -> (Vec<String>, String, String) {
    let path = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let path = path.split_once('/').map_or("", |(_, p)| p);
    let segments: Vec<String> = path.split('/').map(str::to_ascii_lowercase).collect();
    let file = segments.last().cloned().unwrap_or_default();
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), e.to_string()),
        None => (file.clone(), String::new()),
    };
    (segments, stem, ext)
}

/// Whether an address is on the built-in list, by what it is: its host, its
/// folders, its name. Nothing is asked of the network.
pub fn address(url: &str) -> Option<Excluded> {
    let host = dm_protocol::host_of(url);
    if host_is(&host, ADVERT_HOSTS) {
        return Some(Excluded::Advert);
    }
    if host_is(&host, TRACKER_HOSTS) {
        return Some(Excluded::Tracker);
    }
    let (segments, stem, ext) = path_and_file(url);
    let file = segments.last().map(String::as_str).unwrap_or("");
    let folders = segments
        .get(..segments.len().saturating_sub(1))
        .unwrap_or_default();
    if folders
        .iter()
        .any(|s| ADVERT_SEGMENTS.contains(&s.as_str()))
    {
        return Some(Excluded::Advert);
    }
    if TRACKER_FILES.contains(&file) {
        return Some(Excluded::Tracker);
    }
    if SOUND_EXTENSIONS.contains(&ext.as_str())
        && (SOUND_NAMES.contains(&stem.as_str())
            || folders.iter().any(|s| SOUND_FOLDERS.contains(&s.as_str())))
    {
        return Some(Excluded::SiteSound);
    }
    if IMAGE_EXTENSIONS.contains(&ext.as_str())
        && (FURNITURE_NAMES.contains(&stem.as_str())
            || folders
                .iter()
                .any(|s| FURNITURE_FOLDERS.contains(&s.as_str())))
    {
        return Some(Excluded::InterfaceImage);
    }
    None
}

/// Whether a response is on the list: its address, the server that sent it
/// naming itself an ad server, or a Windows media file of 100 KB or less.
pub fn response(
    url: &str,
    mime: &str,
    server: Option<&str>,
    size: Option<u64>,
) -> Option<Excluded> {
    if let Some(found) = address(url) {
        return Some(found);
    }
    if server.is_some_and(|s| s.to_ascii_lowercase().contains("dclk-adsvr")) {
        return Some(Excluded::AdServer);
    }
    let (_, _, ext) = path_and_file(url);
    let windows_media = WINDOWS_MEDIA_EXTENSIONS.contains(&ext.as_str())
        || WINDOWS_MEDIA_TYPES.contains(&mime.trim().to_ascii_lowercase().as_str());
    if windows_media && size.is_some_and(|bytes| bytes <= WINDOWS_MEDIA_FLOOR) {
        return Some(Excluded::TinyWindowsMedia);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_advertising_network_is_never_caught_whatever_it_serves() {
        for url in [
            "https://ad.doubleclick.net/ddm/trackclk/file.mp4",
            "https://pagead2.googlesyndication.com/pagead/ads/clip.mp4",
            "https://static.criteo.net/img/video.mp4",
            "https://cdn.example.com/adserver/promo.mp4",
        ] {
            assert_eq!(address(url), Some(Excluded::Advert), "{url}");
        }
    }

    #[test]
    fn a_tracking_call_is_never_caught() {
        for url in [
            "https://www.google-analytics.com/collect.mp3",
            "https://stats.example.com/img/1x1.gif",
            "https://x.example.com/t/pixel.gif?id=3",
        ] {
            assert_eq!(address(url), Some(Excluded::Tracker), "{url}");
        }
    }

    #[test]
    fn the_sounds_a_page_plays_for_itself_are_not_caught_but_a_song_is() {
        assert_eq!(
            address("https://site.example/assets/click.wav"),
            Some(Excluded::SiteSound)
        );
        assert_eq!(
            address("https://site.example/sounds/level-up.mid"),
            Some(Excluded::SiteSound)
        );
        // A recording named as music is a file a person may want.
        assert_eq!(address("https://site.example/music/track-01.wav"), None);
        assert_eq!(address("https://site.example/files/podcast.mp3"), None);
    }

    #[test]
    fn a_pages_furniture_is_not_caught_but_a_picture_is() {
        assert_eq!(
            address("https://site.example/favicon.ico"),
            Some(Excluded::InterfaceImage)
        );
        assert_eq!(
            address("https://site.example/img/toolbar/save.png"),
            Some(Excluded::InterfaceImage)
        );
        assert_eq!(address("https://site.example/photos/holiday.jpg"), None);
    }

    #[test]
    fn a_response_from_a_server_that_names_itself_an_ad_server_is_left() {
        assert_eq!(
            response(
                "https://a.example/x.mp4",
                "video/mp4",
                Some("DCLK-AdSvr"),
                Some(9_000_000)
            ),
            Some(Excluded::AdServer)
        );
        assert_eq!(
            response(
                "https://a.example/x.mp4",
                "video/mp4",
                Some("nginx"),
                Some(9_000_000)
            ),
            None
        );
    }

    #[test]
    fn windows_media_of_100_kb_or_less_is_an_ad_stub_and_a_bigger_one_is_not() {
        assert_eq!(
            response(
                "https://a.example/promo.asf",
                "video/x-ms-asf",
                None,
                Some(100 * 1024)
            ),
            Some(Excluded::TinyWindowsMedia)
        );
        assert_eq!(
            response("https://a.example/clip", "audio/x-ms-wma", None, Some(2048)),
            Some(Excluded::TinyWindowsMedia)
        );
        assert_eq!(
            response(
                "https://a.example/film.wmv",
                "video/x-ms-wmv",
                None,
                Some(100 * 1024 + 1)
            ),
            None
        );
        // Size unknown: not excluded on size.
        assert_eq!(response("https://a.example/film.wmv", "", None, None), None);
        // Other formats are not held to the floor here (`STREAM_FLOOR` is).
        assert_eq!(
            response("https://a.example/tiny.mp4", "video/mp4", None, Some(2048)),
            None
        );
    }

    #[test]
    fn an_ordinary_download_is_not_on_the_list() {
        for url in [
            "https://github.com/user/repo/releases/download/v1/setup.exe",
            "https://example.com/files/report.pdf",
            "https://cdn.example.com/video/talk.mp4",
        ] {
            assert_eq!(address(url), None, "{url}");
        }
    }

    #[test]
    fn the_smallest_player_is_the_one_the_notes_give() {
        assert_eq!(SMALLEST_PLAYER, (15.0, 10.0));
    }
}
