// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
/// Not Instagram's: its videos come from the same network as its pictures.
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
mod tests;
