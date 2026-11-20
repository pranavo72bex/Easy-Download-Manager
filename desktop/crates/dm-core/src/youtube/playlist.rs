// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube playlist, or a channel's uploads, read by dm itself: the addresses that name one,
//! the page's first videos and the browse API's further pages.

use serde_json::{json, Value};

use super::{
    client_version, initial_data, origin, text_capped, walk, PAGE_AGENT, PAGE_MOST,
    WEB_CLIENT_VERSION, WITHIN,
};
use crate::Error;

/// YouTube's host in `url`, without `www.`, `m.` or `music.`: `youtube.com` for any of them.
fn youtube_host(parsed: &url::Url) -> Option<String> {
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = ["www.", "m.", "music."]
        .iter()
        .find_map(|p| host.strip_prefix(p))
        .unwrap_or(&host)
        .to_string();
    (host == "youtube.com").then_some(host)
}

/// A playlist's id in a playlist address (`/playlist?list=…`, on YouTube or YouTube Music). A
/// watch address with a list beside its video is that video's: `None`.
pub fn playlist_id(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    youtube_host(&parsed)?;
    if parsed.path() != "/playlist" {
        return None;
    }
    parsed
        .query_pairs()
        .find(|(k, _)| k == "list")
        .map(|(_, v)| v.into_owned())
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// A channel in a channel's address: its id where the address names it (`/channel/UC…`), else
/// the path of its page (`/@name`, `/c/name`, `/user/name`), with any tab after it (`/videos`,
/// `/shorts`, `/streams`, `/featured`). Each is read as the channel's uploads (gaps row Y7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// The channel's id, `UC` and 22 more.
    Id(String),
    /// The channel page's path, to read its id from.
    Page(String),
}

/// The channel `url` is the page of, or one of its tabs; `None` for any other address.
pub fn channel_of(url: &str) -> Option<Channel> {
    const TABS: [&str; 6] = [
        "videos", "shorts", "streams", "featured", "live", "podcasts",
    ];
    let parsed = url::Url::parse(url.trim()).ok()?;
    youtube_host(&parsed)?;
    let parts: Vec<&str> = parsed.path_segments()?.filter(|p| !p.is_empty()).collect();
    let plain = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && p.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '%'))
    };
    // The channel's own parts, then at most one of its tabs.
    let (own, tab) = match parts.as_slice() {
        [first, ..] if first.starts_with('@') => parts.split_at(1),
        ["channel" | "c" | "user", ..] if parts.len() >= 2 => parts.split_at(2),
        _ => return None,
    };
    if tab.len() > 1 || tab.first().is_some_and(|t| !TABS.contains(t)) {
        return None;
    }
    match own {
        [handle] => {
            let name = handle.strip_prefix('@')?;
            plain(name).then(|| Channel::Page(format!("/@{name}")))
        }
        ["channel", id] => (id.len() == 24 && id.starts_with("UC") && plain(id))
            .then(|| Channel::Id((*id).to_string())),
        [kind, name] => plain(name).then(|| Channel::Page(format!("/{kind}/{name}"))),
        _ => None,
    }
}

/// Whether `url` is a list of videos dm reads itself: a playlist, or a channel's uploads.
pub fn is_list(url: &str) -> bool {
    playlist_id(url).is_some() || channel_of(url).is_some()
}

/// The id a channel's page names for itself (its canonical address, else its metadata).
pub(super) fn channel_id_in(page: &str) -> Option<String> {
    let sound = |id: &str| {
        id.len() == 24
            && id.starts_with("UC")
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    };
    let after = |mark: &str| {
        let at = page.find(mark)? + mark.len();
        let id: String = page[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .collect();
        sound(&id).then_some(id)
    };
    after("<link rel=\"canonical\" href=\"https://www.youtube.com/channel/")
        .or_else(|| after("\"externalId\":\""))
        .or_else(|| after("\"channelId\":\""))
}

/// One video of a playlist, as its page lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaylistEntry {
    /// The video's id.
    pub id: String,
    /// The video's title.
    pub title: String,
    /// How long it plays, where the list says.
    pub seconds: Option<u64>,
    /// Its own page, for another site's playlist; a YouTube video's is made
    /// from its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

impl PlaylistEntry {
    /// The video's page: its own, or YouTube's watch address.
    pub fn url(&self) -> String {
        match &self.page {
            Some(page) => page.clone(),
            None => format!("https://www.youtube.com/watch?v={}", self.id),
        }
    }
}

/// A playlist as its page lists it: its title and its videos, in order.
/// `cut_short` says the list went on past what was read (a thousand
/// videos, or a continuation YouTube would not answer).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    /// The playlist's id.
    pub id: String,
    /// The playlist's title.
    pub title: String,
    /// Its videos, in the playlist's order.
    pub entries: Vec<PlaylistEntry>,
    /// Whether the list went on past what was read.
    pub cut_short: bool,
}

/// The most videos read of one playlist: ten pages of a hundred.
const PLAYLIST_MOST: usize = 1000;

/// A playlist page read by dm itself: the page's own data lists its first
/// hundred videos, and the browse API the rest, a hundred at a time, as
/// the page itself asks for them.
pub async fn read_playlist(
    client: &reqwest::Client,
    url: &str,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Playlist, Error> {
    tokio::time::timeout(WITHIN, async {
        let id = match (playlist_id(url), channel_of(url)) {
            (Some(id), _) => id,
            // A channel's uploads are a playlist of their own: `UU` and the rest of its id.
            (None, Some(channel)) => {
                let channel = match channel {
                    Channel::Id(id) => id,
                    Channel::Page(path) => read_channel_id(client, &path, sign_in).await?,
                };
                format!("UU{}", &channel[2..])
            }
            (None, None) => {
                return Err(Error::Internal("not a YouTube playlist address".into()));
            }
        };
        read_playlist_within(client, &id, sign_in).await
    })
    .await
    .map_err(|_| Error::Internal("the playlist took too long to read".into()))?
}

/// The id of the channel whose page is at `path` (`/@name`), read from the page.
async fn read_channel_id(
    client: &reqwest::Client,
    path: &str,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<String, Error> {
    let address = format!("{}{path}?hl=en", origin());
    let mut cookie = String::from("PREF=hl=en&tz=UTC; SOCS=CAI");
    if let Some(theirs) = crate::cookies::header_with(sign_in, &address) {
        cookie = format!("{theirs}; {cookie}");
    }
    let page = crate::redirects::send(
        client
            .get(&address)
            .header(reqwest::header::USER_AGENT, PAGE_AGENT)
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(reqwest::header::COOKIE, &cookie),
    )
    .await?;
    let page = text_capped(Error::refused_or(page).await?, PAGE_MOST).await?;
    channel_id_in(&page).ok_or_else(|| {
        Error::Internal("the channel's page names no channel (removed, or not a channel)".into())
    })
}

async fn read_playlist_within(
    client: &reqwest::Client,
    id: &str,
    sign_in: Option<&crate::cookies::SignIn>,
) -> Result<Playlist, Error> {
    let origin = origin();
    let address = format!("{origin}/playlist?list={id}&hl=en");
    let mut cookie = String::from("PREF=hl=en&tz=UTC; SOCS=CAI");
    if let Some(theirs) = crate::cookies::header_with(sign_in, &address) {
        cookie = format!("{theirs}; {cookie}");
    }
    let page = crate::redirects::send(
        client
            .get(&address)
            .header(reqwest::header::USER_AGENT, PAGE_AGENT)
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(reqwest::header::COOKIE, &cookie),
    )
    .await?;
    let page = text_capped(Error::refused_or(page).await?, PAGE_MOST).await?;
    let data = initial_data(&page).ok_or_else(|| {
        Error::Internal(
            "the playlist page holds no list (private, removed, or a sign-in wanted)".into(),
        )
    })?;
    let mut playlist = Playlist {
        id: id.to_string(),
        title: playlist_title(&data).unwrap_or_else(|| format!("Playlist {id}")),
        ..Playlist::default()
    };
    let mut token = gather(&data, &mut playlist.entries);
    if playlist.entries.is_empty() {
        return Err(Error::Internal(
            "the playlist page lists no videos (private, removed, or a sign-in wanted)".into(),
        ));
    }
    // The page's own client version, for the browse API; a stand-in gives none.
    let version = client_version(&page).unwrap_or_else(|| WEB_CLIENT_VERSION.to_string());
    while let Some(next) = token.take() {
        if playlist.entries.len() >= PLAYLIST_MOST {
            playlist.cut_short = true;
            break;
        }
        let body = json!({
            "context": {"client": {"clientName": "WEB", "clientVersion": version, "hl": "en", "gl": "US"}},
            "continuation": next,
        });
        let answer = crate::redirects::send(
            client
                .post(format!("{origin}/youtubei/v1/browse?prettyPrint=false"))
                .header(reqwest::header::USER_AGENT, PAGE_AGENT)
                .header(reqwest::header::COOKIE, &cookie)
                .header("X-YouTube-Client-Name", "1")
                .header("X-YouTube-Client-Version", &version)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string()),
        )
        .await;
        let Ok(answer) = answer else {
            playlist.cut_short = true;
            break;
        };
        let Ok(text) = text_capped(answer, PAGE_MOST).await else {
            playlist.cut_short = true;
            break;
        };
        let Ok(more) = serde_json::from_str::<Value>(&text) else {
            playlist.cut_short = true;
            break;
        };
        let before = playlist.entries.len();
        token = gather(&more, &mut playlist.entries);
        if playlist.entries.len() == before {
            // A page that adds nothing would loop for ever.
            break;
        }
    }
    Ok(playlist)
}

/// The playlist's title, from wherever the page's data keeps it.
pub(super) fn playlist_title(data: &Value) -> Option<String> {
    let text = |v: &Value| -> Option<String> {
        v.as_str()
            .map(str::to_string)
            .or_else(|| v.get("simpleText")?.as_str().map(str::to_string))
            .or_else(|| {
                let runs = v.get("runs")?.as_array()?;
                let joined: String = runs
                    .iter()
                    .filter_map(|r| r.get("text")?.as_str())
                    .collect();
                (!joined.is_empty()).then_some(joined)
            })
            .or_else(|| text_of(v.get("content")?))
    };
    fn text_of(v: &Value) -> Option<String> {
        v.as_str().map(str::to_string)
    }
    [
        "/metadata/playlistMetadataRenderer/title",
        "/header/playlistHeaderRenderer/title",
        "/header/pageHeaderRenderer/pageTitle",
        "/microformat/microformatDataRenderer/title",
    ]
    .iter()
    .find_map(|path| data.pointer(path).and_then(text))
    .map(|t| t.trim().to_string())
    .filter(|t| !t.is_empty())
}

/// Every video the data lists, in order, added to `entries` (one a video,
/// however often the data names it), and the token for the next page,
/// where the data has one.
pub(super) fn gather(data: &Value, entries: &mut Vec<PlaylistEntry>) -> Option<String> {
    let mut token = None;
    let mut seen: std::collections::HashSet<String> =
        entries.iter().map(|e| e.id.clone()).collect();
    // A video's id as YouTube writes them: eleven of these characters.
    let sound_id = |id: &str| {
        id.len() == 11
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    };
    // A video that cannot be played is not offered: private, deleted.
    let unplayable = |title: &str, playable: Option<bool>| {
        playable == Some(false) || matches!(title, "[Private video]" | "[Deleted video]")
    };
    walk(data, &mut |key, value| {
        match key {
            // The page's newer shape (2026): each video a "lockup", its id
            // under `contentId`, its title in its metadata, its length the
            // badge on its picture ("12:15").
            "lockupViewModel" => {
                let id = value.get("contentId").and_then(Value::as_str).unwrap_or("");
                let video = value
                    .get("contentType")
                    .and_then(Value::as_str)
                    .is_none_or(|t| t == "LOCKUP_CONTENT_TYPE_VIDEO");
                if !sound_id(id) || !video || seen.contains(id) {
                    return;
                }
                let title = value
                    .pointer("/metadata/lockupMetadataViewModel/title/content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let mut badge = None;
                if let Some(image) = value.get("contentImage") {
                    walk(image, &mut |k, v| {
                        if k == "thumbnailBadgeViewModel" && badge.is_none() {
                            badge = v
                                .get("text")
                                .and_then(Value::as_str)
                                .filter(|t| t.contains(':'))
                                .and_then(crate::clip::Span::seconds);
                        }
                    });
                }
                if unplayable(&title, None) {
                    return;
                }
                seen.insert(id.to_string());
                entries.push(PlaylistEntry {
                    id: id.to_string(),
                    title: if title.is_empty() {
                        id.to_string()
                    } else {
                        title
                    },
                    seconds: badge.map(|s| s as u64),
                    page: None,
                });
            }
            "playlistVideoRenderer" => {
                let id = value.get("videoId").and_then(Value::as_str).unwrap_or("");
                if !sound_id(id) || seen.contains(id) {
                    return;
                }
                if value.get("isPlayable").and_then(Value::as_bool) == Some(false) {
                    return;
                }
                let title = value
                    .pointer("/title/runs/0/text")
                    .or_else(|| value.pointer("/title/simpleText"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let seconds = value
                    .get("lengthSeconds")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
                if unplayable(&title, None) {
                    return;
                }
                seen.insert(id.to_string());
                entries.push(PlaylistEntry {
                    id: id.to_string(),
                    title: if title.is_empty() {
                        id.to_string()
                    } else {
                        title
                    },
                    seconds,
                    page: None,
                });
            }
            // The next page's token, wherever the page's shape puts it.
            "continuationCommand" if token.is_none() => {
                token = value
                    .get("token")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    });
    token
}
