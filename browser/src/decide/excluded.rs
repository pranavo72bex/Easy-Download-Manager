//! What nobody came to download: a built-in list of addresses and responses
//! the extension never catches, whatever a person's own settings say. Reference
//! A keeps such a list (site sounds, toolbar images, ad files, tiny Windows
//! media, one ad server by name); the masks here are dm's own, written by class
//! and kept to what is plainly an advert, a beacon, a sound a page plays or a
//! piece of a page's furniture, so no file a person wants is lost to them. A
//! person's own sites to leave alone stay in dm's settings (`CatchRules`).

use dm_protocol::ExcludeRules;

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

/// Hosts whose downloads should never be caught: OS update servers and
/// content-delivery networks that serve only GIF stickers and image embeds.
/// A file from these hosts is never the file a person is looking for.
const NEVER_CATCH_HOSTS: &[&str] = &[
    "update.microsoft.com",
    "download.windowsupdate.com",
    "windowsupdate.com",
    "gstatic.com",
    "media.tenor.com",
    "media.tenor.co",
    "giphy.com",
    "media.giphy.com",
    "cdninstagram.com",
];

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

/// Whether `host` is one of the names the app served, or a sub-domain of one.
fn served_host(host: &str, names: &[String]) -> bool {
    names
        .iter()
        .any(|name| host == name || host.ends_with(&format!(".{name}")))
}

/// Whether `item` is in the built-in list or in the one the app served.
fn has(builtin: &[&str], served: &[String], item: &str) -> bool {
    builtin.contains(&item) || served.iter().any(|name| name == item)
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

/// Whether an address is on the built-in list or the one the app served
/// (see [`super::rules`]), by what it is: its host, its folders, its name.
/// Nothing is asked of the network.
pub fn address(url: &str) -> Option<Excluded> {
    super::rules::with_served(|served| address_with(url, &served.exclude))
}

/// [`address`] against the built-in list and `served`.
pub fn address_with(url: &str, served: &ExcludeRules) -> Option<Excluded> {
    let host = dm_protocol::host_of(url);
    // OS update servers and sticker/GIF CDNs are never the file a person wants.
    if host_is(&host, NEVER_CATCH_HOSTS) {
        return Some(Excluded::Advert);
    }
    if host_is(&host, ADVERT_HOSTS) || served_host(&host, &served.advert_hosts) {
        return Some(Excluded::Advert);
    }
    if host_is(&host, TRACKER_HOSTS) || served_host(&host, &served.tracker_hosts) {
        return Some(Excluded::Tracker);
    }
    let (segments, stem, ext) = path_and_file(url);
    let file = segments.last().map(String::as_str).unwrap_or("");
    let folders = segments
        .get(..segments.len().saturating_sub(1))
        .unwrap_or_default();
    if folders
        .iter()
        .any(|s| has(ADVERT_SEGMENTS, &served.advert_segments, s))
    {
        return Some(Excluded::Advert);
    }
    if has(TRACKER_FILES, &served.tracker_files, file) {
        return Some(Excluded::Tracker);
    }
    if has(SOUND_EXTENSIONS, &served.sound_extensions, &ext)
        && (has(SOUND_NAMES, &served.sound_names, &stem)
            || folders
                .iter()
                .any(|s| has(SOUND_FOLDERS, &served.sound_folders, s)))
    {
        return Some(Excluded::SiteSound);
    }
    if IMAGE_EXTENSIONS.contains(&ext.as_str())
        && (has(FURNITURE_NAMES, &served.furniture_names, &stem)
            || folders
                .iter()
                .any(|s| has(FURNITURE_FOLDERS, &served.furniture_folders, s)))
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
    super::rules::with_served(|served| response_with(url, mime, server, size, &served.exclude))
}

/// [`response`] against the built-in list and `served`.
pub fn response_with(
    url: &str,
    mime: &str,
    server: Option<&str>,
    size: Option<u64>,
    served: &ExcludeRules,
) -> Option<Excluded> {
    if let Some(found) = address_with(url, served) {
        return Some(found);
    }
    if server.is_some_and(|s| {
        let named = s.to_ascii_lowercase();
        named.contains("dclk-adsvr")
            || served
                .ad_server_names
                .iter()
                .any(|word| named.contains(word.as_str()))
    }) {
        return Some(Excluded::AdServer);
    }
    let (_, _, ext) = path_and_file(url);
    let windows_media = WINDOWS_MEDIA_EXTENSIONS.contains(&ext.as_str())
        || WINDOWS_MEDIA_TYPES.contains(&mime.trim().to_ascii_lowercase().as_str());
    // A served floor can raise the built-in one and never lower it.
    let floor = WINDOWS_MEDIA_FLOOR.max(served.windows_media_floor.unwrap_or(0));
    if windows_media && size.is_some_and(|bytes| bytes <= floor) {
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

    fn served(json: &str) -> ExcludeRules {
        serde_json::from_str(json).expect("served rules")
    }

    #[test]
    fn what_the_app_served_is_added_to_the_built_in_list() {
        let extra = served(
            r#"{ "advert_hosts": ["promo.example"], "tracker_hosts": ["stats.example"],
                 "advert_segments": ["sponsor"], "tracker_files": ["hit.gif"],
                 "sound_names": ["blip"], "sound_folders": ["jingles"], "sound_extensions": ["ogg"],
                 "furniture_folders": ["chrome"], "furniture_names": ["divider"],
                 "ad_server_names": ["adsrv"] }"#,
        );
        let cases = [
            ("https://cdn.promo.example/a/clip.mp4", Excluded::Advert),
            ("https://stats.example/c.mp3", Excluded::Tracker),
            ("https://site.example/sponsor/clip.mp4", Excluded::Advert),
            ("https://site.example/t/hit.gif", Excluded::Tracker),
            ("https://site.example/a/blip.ogg", Excluded::SiteSound),
            ("https://site.example/jingles/one.ogg", Excluded::SiteSound),
            (
                "https://site.example/chrome/a.png",
                Excluded::InterfaceImage,
            ),
            (
                "https://site.example/i/divider.gif",
                Excluded::InterfaceImage,
            ),
        ];
        for (url, why) in cases {
            assert_eq!(address_with(url, &extra), Some(why), "{url}");
            // Without what was served the same address is an ordinary one.
            assert_eq!(address_with(url, &ExcludeRules::default()), None, "{url}");
        }
        assert_eq!(
            response_with(
                "https://a.example/x.mp4",
                "video/mp4",
                Some("MyAdSrv/2"),
                None,
                &extra
            ),
            Some(Excluded::AdServer)
        );
        // The built-in list still holds when the served one is empty.
        assert_eq!(
            address_with("https://ad.doubleclick.net/x.mp4", &ExcludeRules::default()),
            Some(Excluded::Advert)
        );
    }

    #[test]
    fn a_served_floor_raises_the_stub_size_and_never_lowers_it() {
        let raised = served(r#"{ "windows_media_floor": 300000 }"#);
        let lowered = served(r#"{ "windows_media_floor": 10 }"#);
        let url = "https://a.example/promo.wmv";
        let mime = "video/x-ms-wmv";
        assert_eq!(
            response_with(url, mime, None, Some(200_000), &raised),
            Some(Excluded::TinyWindowsMedia)
        );
        assert_eq!(
            response_with(url, mime, None, Some(200_000), &ExcludeRules::default()),
            None
        );
        assert_eq!(
            response_with(url, mime, None, Some(WINDOWS_MEDIA_FLOOR), &lowered),
            Some(Excluded::TinyWindowsMedia)
        );
    }

    #[test]
    fn the_rules_installed_from_a_hello_are_the_ones_address_uses() {
        use super::super::rules;
        let list = dm_protocol::Rules {
            format: dm_protocol::RULES_FORMAT,
            revision: 2,
            exclude: served(r#"{ "advert_hosts": ["Promo.Example"] }"#),
            sites: Vec::new(),
        };
        rules::install(Some(list));
        assert_eq!(
            address("https://promo.example/a.mp4"),
            Some(Excluded::Advert)
        );
        // A document in a format this side does not read is not used.
        rules::install(Some(dm_protocol::Rules {
            format: 99,
            ..dm_protocol::Rules::default()
        }));
        assert_eq!(address("https://promo.example/a.mp4"), None);
        rules::install(None);
        assert!(!rules::installed());
    }

    #[test]
    fn the_smallest_player_is_the_one_the_notes_give() {
        assert_eq!(SMALLEST_PLAYER, (15.0, 10.0));
    }

    #[test]
    fn os_update_servers_and_sticker_cdns_are_never_caught() {
        for url in [
            "https://update.microsoft.com/windowsupdate/v6/v5controls/en/x86/client/wuwebv.cab",
            "https://download.windowsupdate.com/c/msdownload/update/software/secu/2024/x.msu",
            "https://gstatic.com/a.mp4",
            "https://media.tenor.com/abc123/sticker.mp4",
            "https://media.tenor.co/abc123.mp4",
            "https://media.giphy.com/media/abc/giphy.mp4",
        ] {
            assert!(
                address(url).is_some(),
                "{url} should be excluded but was not"
            );
        }
    }
}
