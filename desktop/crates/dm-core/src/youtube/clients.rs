// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The clients YouTube's player is asked as: dm's own, the served rules', and a signed-in browser
//! session's.

use super::*;

/// A client YouTube's player is asked as: dm's own, or one the served rules name (gaps row Y1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerClient {
    /// Its `clientName`.
    pub name: String,
    /// Its `clientVersion`.
    pub version: String,
    /// Its number (`X-Youtube-Client-Name`).
    pub id: String,
    /// The User-Agent it is asked with.
    pub agent: String,
    /// Further fields of its `context.client`.
    pub context: Vec<(String, String)>,
}

/// The clients the served rules name, asked before dm's own, in their order.
static SERVED_CLIENTS: std::sync::Mutex<Vec<PlayerClient>> = std::sync::Mutex::new(Vec::new());

/// The clients the served rules name (`rules.json`'s `youtube.clients`), in place of any before:
/// asked in order before dm's own, so a client YouTube stops answering is changed with no new
/// build (gaps row Y1).
pub fn set_player_clients(clients: Vec<PlayerClient>) {
    *crate::lock(&SERVED_CLIENTS) = clients;
}

/// The clients the served rules name, as set last.
pub fn served_player_clients() -> Vec<PlayerClient> {
    crate::lock(&SERVED_CLIENTS).clone()
}

/// The player scripts whose checks were not answered this run, by id, and how many of them the
/// window has been handed (gaps row Y3).
static UNSOLVED: std::sync::Mutex<(Vec<String>, usize)> = std::sync::Mutex::new((Vec::new(), 0));

/// The id of the player script at `path` (`/s/player/<id>/…`), noted as one whose checks were
/// not answered, once.
pub(super) fn note_unsolved(path: Option<&str>) {
    let Some(player) = path
        .and_then(|p| p.strip_prefix("/s/player/"))
        .and_then(|p| p.split('/').next())
        .filter(|id| !id.is_empty())
    else {
        return;
    };
    let mut held = crate::lock(&UNSOLVED);
    if !held.0.iter().any(|seen| seen == player) {
        held.0.push(player.to_string());
    }
}

/// Every player script whose checks were not answered this run, by id, in the order met.
pub fn unsolved_players() -> Vec<String> {
    crate::lock(&UNSOLVED).0.clone()
}

/// The next player not yet handed on whose checks were not answered: for the window to ask
/// for fresh site rules once for each, out of the weekly turn (gaps row Y3).
pub fn take_unsolved_player() -> Option<String> {
    let mut held = crate::lock(&UNSOLVED);
    let (all, handed) = &mut *held;
    let next = all.get(*handed).cloned()?;
    *handed += 1;
    Some(next)
}

/// The TV client this reads with today: its answers carry plain addresses and want no
/// proof-of-origin token.
fn own_client() -> PlayerClient {
    PlayerClient {
        name: TV_CLIENT_NAME.into(),
        version: TV_CLIENT_VERSION.into(),
        id: TV_CLIENT_ID.into(),
        agent: TV_AGENT.into(),
        context: [
            ("deviceMake", "Samsung"),
            ("deviceModel", "UKS9800"),
            ("osName", "Tizen"),
            ("osVersion", "2.4.0"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .to_vec(),
    }
}

/// Every client to ask as, in order: the served ones, then dm's own (once).
pub(super) fn player_clients() -> Vec<PlayerClient> {
    let mut all = crate::lock(&SERVED_CLIENTS).clone();
    let own = own_client();
    if !all
        .iter()
        .any(|c| c.name == own.name && c.version == own.version)
    {
        all.push(own);
    }
    all
}

/// The player's answer for video `id`, asked as each of `clients` in turn until one answers
/// with a video that plays, the last one's reason where none does: the turn `read_within`
/// takes, without the streams (which a client is also passed over for), for its tests.
#[cfg(test)]
pub(super) async fn ask_player(
    client: &reqwest::Client,
    origin: &str,
    id: &str,
    facts: &PageFacts,
    clients: &[PlayerClient],
) -> Result<Value, Error> {
    let mut failed = None;
    for asked_as in clients {
        match ask_player_as(client, origin, id, facts, asked_as).await {
            Ok(player) => return Ok(player),
            Err(e) => failed = Some(e),
        }
    }
    Err(failed.unwrap_or_else(|| Error::Internal("no client to ask YouTube's player as".into())))
}

/// The player's answer for video `id`, asked as `asked_as`; an error unless it plays.
#[cfg(test)]
async fn ask_player_as(
    client: &reqwest::Client,
    origin: &str,
    id: &str,
    facts: &PageFacts,
    asked_as: &PlayerClient,
) -> Result<Value, Error> {
    plays(player_answer(client, origin, id, facts, asked_as, None).await?)
}

/// The player's answer for video `id`, asked as `asked_as`, whatever it says of playing: with
/// the browser's `session` where one is given (gaps row Y4).
pub(super) async fn player_answer(
    client: &reqwest::Client,
    origin: &str,
    id: &str,
    facts: &PageFacts,
    asked_as: &PlayerClient,
    session: Option<&Session>,
) -> Result<Value, Error> {
    let mut who = serde_json::Map::new();
    for (key, value) in &asked_as.context {
        who.insert(key.clone(), Value::String(value.clone()));
    }
    for (key, value) in [
        ("clientName", &asked_as.name),
        ("clientVersion", &asked_as.version),
        ("userAgent", &asked_as.agent),
    ] {
        who.insert(key.into(), Value::String(value.clone()));
    }
    who.insert("hl".into(), json!("en"));
    who.insert("timeZone".into(), json!("UTC"));
    who.insert("utcOffsetMinutes".into(), json!(0));
    let body = json!({
        "context": {"client": who},
        "videoId": id,
        "playbackContext": {"contentPlaybackContext": {
            "html5Preference": "HTML5_PREF_WANTS",
            "signatureTimestamp": facts.sts,
        }},
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    let mut ask = client
        .post(format!("{origin}/youtubei/v1/player?prettyPrint=false"))
        .header(reqwest::header::USER_AGENT, &asked_as.agent)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("X-Youtube-Client-Name", &asked_as.id)
        .header("X-Youtube-Client-Version", &asked_as.version);
    ask = match session {
        // As YouTube's web clients ask signed in: the session's cookies, and its hash made
        // for the web's origin, which the request says it is from.
        Some(session) => ask
            .header(reqwest::header::COOKIE, &session.cookie)
            .header(
                reqwest::header::AUTHORIZATION,
                sapisid_hash(&session.sapisid, WEB_ORIGIN, unix_now()),
            )
            .header("X-Goog-AuthUser", "0")
            .header("X-Origin", WEB_ORIGIN)
            .header(reqwest::header::ORIGIN, WEB_ORIGIN),
        None => ask.header(reqwest::header::ORIGIN, origin),
    };
    if let Some(visitor) = &facts.visitor {
        ask = ask.header("X-Goog-Visitor-Id", visitor);
    }
    let answer = crate::redirects::send(ask.body(body.to_string())).await?;
    let answered = text_capped(Error::refused_or(answer).await?, PAGE_MOST).await?;
    serde_json::from_str(&answered)
        .map_err(|e| Error::Internal(format!("the player API answered no JSON ({e})")))
}

/// The player's answer where it plays, else what YouTube says of why not.
pub(super) fn plays(player: Value) -> Result<Value, Error> {
    let status = player
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("");
    if status != "OK" {
        let reason = player
            .pointer("/playabilityStatus/reason")
            .and_then(Value::as_str)
            .unwrap_or(status);
        return Err(Error::Internal(format!("YouTube says: {reason}")));
    }
    Ok(player)
}

/// Whether the player refuses for want of a sign-in: an age check, a private video, a
/// members-only one, or a plain "sign in" (gaps row Y4).
pub(super) fn wants_sign_in(player: &Value) -> bool {
    let said = |at: &str| {
        player
            .pointer(at)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let status = said("/playabilityStatus/status");
    let reason = said("/playabilityStatus/reason");
    matches!(
        status.as_str(),
        "login_required" | "age_check_required" | "age_verification_required"
    ) || (matches!(
        status.as_str(),
        "unplayable" | "error" | "content_check_required"
    ) && ["sign in", "members", "private", "age-restricted"]
        .iter()
        .any(|word| reason.contains(word)))
}

/// The `Authorization` YouTube's web clients send with a signed-in request: the time, and the
/// SHA-1 of the time, the session's `SAPISID` and the origin asked from.
pub(super) fn sapisid_hash(sapisid: &str, origin: &str, now: u64) -> String {
    use sha1::Digest;
    let digest = sha1::Sha1::digest(format!("{now} {sapisid} {origin}").as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("SAPISIDHASH {now}_{hex}")
}

/// Seconds since 1970, as a `SAPISIDHASH` is timed.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The clients a video that wants a sign-in is asked as again with the browser's session: dm's
/// own TV client, then the web client at the version the watch page names (gaps row Y4).
pub(super) fn signed_in_clients(page: &str) -> Vec<PlayerClient> {
    let web = PlayerClient {
        name: "WEB".into(),
        version: client_version(page).unwrap_or_else(|| WEB_CLIENT_VERSION.to_string()),
        id: "1".into(),
        agent: PAGE_AGENT.into(),
        context: Vec::new(),
    };
    vec![own_client(), web]
}

/// Where YouTube is: its own host, or a test's stand-in.
pub(super) fn origin() -> String {
    crate::env::get("DM_YOUTUBE_HOST")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://www.youtube.com".to_string())
}
