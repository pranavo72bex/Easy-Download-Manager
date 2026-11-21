// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A premiere, or a live stream that has not begun (gaps row Y5): the player answers that it
//! is offline (`LIVE_STREAM_OFFLINE`, or `isUpcoming`), names the time it is set to start, and
//! lists no streams yet. Read as that time alone (`premiere_at`), which
//! `videopage::download_page` waits for before it reads the video again.

use serde_json::{json, Value};

/// When the upcoming live stream or premiere the player answers about is set to start, in
/// seconds since 1970: the offline slate's `scheduledStartTime`, else the broadcast's
/// `startTimestamp`, else an `upcomingEventData`'s `startTime`. `None` for a video that is
/// not upcoming, one that already lists streams, or one that names no time.
pub(super) fn scheduled_start(player: &Value) -> Option<u64> {
    let status = player
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("");
    let upcoming = status == "LIVE_STREAM_OFFLINE"
        || player
            .pointer("/videoDetails/isUpcoming")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if !upcoming || has_streams(player) {
        return None;
    }
    const SLATE: &str = "/playabilityStatus/liveStreamability/liveStreamabilityRenderer\
                         /offlineSlate/liveStreamOfflineSlateRenderer/scheduledStartTime";
    const BROADCAST: &str =
        "/microformat/playerMicroformatRenderer/liveBroadcastDetails/startTimestamp";
    let broadcast = || {
        let text = player.pointer(BROADCAST)?.as_str()?;
        let at = crate::clock::rfc3339(text.trim())?.unix;
        u64::try_from(at).ok().filter(|&at| at > 0)
    };
    player
        .pointer(SLATE)
        .and_then(seconds)
        .or_else(broadcast)
        .or_else(|| {
            player
                .pointer("/upcomingEventData/startTime")
                .and_then(seconds)
        })
}

/// Whether the player lists any stream: a format, or a playlist.
fn has_streams(player: &Value) -> bool {
    let listed = |at: &str| {
        player
            .pointer(at)
            .and_then(Value::as_array)
            .is_some_and(|list| !list.is_empty())
    };
    listed("/streamingData/formats")
        || listed("/streamingData/adaptiveFormats")
        || player.pointer("/streamingData/hlsManifestUrl").is_some()
}

/// Whole seconds since 1970, as YouTube writes them: a number, or digits in a string.
fn seconds(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|&at| at > 0)
}

/// The reading of a premiere set to start at `at`: the video's id and title and that time,
/// with no stream (`premiere_at`).
pub(super) fn premiere_reading(id: &str, player: &Value, at: u64) -> String {
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .unwrap_or("premiere");
    json!({
        "id": id,
        "title": title,
        "extractor": "youtube",
        "extractor_key": "Youtube",
        "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
        "premiere_at": at,
        "read_by": "dm",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An offline answer as the player gives one: `slate` its offline slate's time, `micro` its
    /// broadcast's start, `upcoming` its `isUpcoming`.
    fn offline(slate: Option<Value>, micro: Option<&str>, upcoming: bool) -> Value {
        let mut player = json!({
            "playabilityStatus": {"status": "LIVE_STREAM_OFFLINE",
                "reason": "This live event will begin in a few moments."},
            "videoDetails": {"videoId": "premiere001", "title": "Launch", "isUpcoming": upcoming,
                "isLiveContent": true},
        });
        if let Some(at) = slate {
            player["playabilityStatus"]["liveStreamability"] = json!({
                "liveStreamabilityRenderer": {"videoId": "premiere001", "offlineSlate": {
                    "liveStreamOfflineSlateRenderer": {"scheduledStartTime": at}}}});
        }
        if let Some(at) = micro {
            player["microformat"] = json!({"playerMicroformatRenderer": {
                "liveBroadcastDetails": {"isLiveNow": false, "startTimestamp": at}}});
        }
        player
    }

    #[test]
    fn the_offline_slates_time_is_read_as_digits_or_a_number() {
        let digits = offline(Some(json!("1795183200")), None, true);
        assert_eq!(scheduled_start(&digits), Some(1_795_183_200));
        let number = offline(Some(json!(1_795_183_200u64)), None, false);
        assert_eq!(scheduled_start(&number), Some(1_795_183_200));
    }

    #[test]
    fn the_broadcasts_start_is_read_where_no_slate_names_one() {
        let micro = offline(None, Some("2026-11-20T15:00:00+00:00"), true);
        assert_eq!(scheduled_start(&micro), Some(1_795_186_800));
        let zoned = offline(None, Some("2026-11-20T20:45:00+05:45"), true);
        assert_eq!(scheduled_start(&zoned), Some(1_795_186_800));
        // The slate's time first, where both are given.
        let both = offline(
            Some(json!("1795183200")),
            Some("2026-11-20T15:00:00+00:00"),
            true,
        );
        assert_eq!(scheduled_start(&both), Some(1_795_183_200));
        let event = json!({"playabilityStatus": {"status": "OK"}, "videoDetails": {
            "isUpcoming": true}, "upcomingEventData": {"startTime": "1795183200"}});
        assert_eq!(scheduled_start(&event), Some(1_795_183_200));
    }

    #[test]
    fn a_video_that_plays_or_names_no_time_is_no_premiere() {
        assert_eq!(scheduled_start(&offline(None, None, true)), None);
        assert_eq!(
            scheduled_start(&offline(Some(json!("soon")), None, true)),
            None
        );
        assert_eq!(
            scheduled_start(&offline(Some(json!("0")), None, true)),
            None
        );
        let mut live = offline(Some(json!("1795183200")), None, false);
        live["playabilityStatus"]["status"] = json!("OK");
        assert_eq!(scheduled_start(&live), None);
        let mut listed = offline(Some(json!("1795183200")), None, true);
        listed["streamingData"] = json!({"hlsManifestUrl": "https://x.example/live.m3u8"});
        assert_eq!(scheduled_start(&listed), None);
    }

    #[test]
    fn a_premieres_reading_is_its_time_and_title_alone() {
        let player = offline(Some(json!("1795183200")), None, true);
        let read: Value =
            serde_json::from_str(&premiere_reading("premiere001", &player, 7)).unwrap_or_default();
        assert_eq!(read["premiere_at"], 7);
        assert_eq!(read["title"], "Launch");
        assert!(read.get("url").is_none() && read.get("formats").is_none());
    }
}
