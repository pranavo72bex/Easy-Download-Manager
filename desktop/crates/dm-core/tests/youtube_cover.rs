// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's own YouTube reader (`dm_core::youtube`) and the fetch of what it
//! reads (`videopage::download_page`), against a stand-in for YouTube: the
//! watch page, the player API, the player script, the playlist pages, the
//! browse API and the streams are all one local server's, each video id its
//! own case. Nothing is fetched to answer YouTube's checks: dm cuts the
//! player script itself (`youtube/player.rs`).
//!
//! The JavaScript engine is a stand-in too: a shell script named as dm's
//! engine host (`javascript::set_host`), which takes a request as the host
//! does, checks it was handed the player as dm cuts it, with dm's call, and
//! answers the challenges these cases carry, or fails in the way the player
//! script it was handed asks for. What is tested is what dm does with an
//! engine's answer, not the cut itself (its own tests are in
//! `youtube/player.rs`).
//!
//! The stand-in is named by environment variables, which are the
//! process's: they are set once, and the few tests that change one for a
//! moment hold `ENV` alone while every other test holds it shared.

#![cfg(unix)]

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Method, Uri};
use axum::response::Response;
use dm_core::{videopage, youtube, Error, Quality};
use serde_json::{json, Value};

use common::{options, tempdir};

/// Held shared by every test, alone by one that changes the environment.
static ENV: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

/// One request the stand-in was sent.
#[derive(Clone, Debug)]
struct Asked {
    method: String,
    uri: String,
    headers: HeaderMap,
    body: String,
}

impl Asked {
    fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

struct StandIn {
    origin: String,
    /// The stand-in host, and the requests it was handed (`seen`).
    home: PathBuf,
    asked: Mutex<Vec<Asked>>,
    /// How many times each video's player API was asked.
    calls: Mutex<HashMap<String, usize>>,
}

static STAND_IN: OnceLock<StandIn> = OnceLock::new();

/// The stand-in host. It reads one request (JSON: the program and the call)
/// from its input as dm's host does, keeps it in `$SEEN` for the test to
/// read, and answers on a line of its own after the host's mark.
const ENGINE: &str = r#"#!/bin/sh
seen="$SEEN/request-$$.json"
cat > "$seen"
answer() { printf 'dm-javascript-answer %s\n' "$1"; exit 0; }
if ! grep -q '"call":"__dm.solve(' "$seen" || ! grep -q '\\"alr\\",\\"yes\\"' "$seen"; then
  answer '{"error":"not the player as dm cuts it"}'
fi
if grep -q MARK_ERROR "$seen"; then answer '{"error":"the JavaScript stopped: the player changed"}'; fi
if grep -q MARK_NOJSON "$seen"; then answer '{"answer":"nothing useful"}'; fi
if grep -q MARK_NORESP "$seen"; then answer '{"answer":"{}"}'; fi
if grep -q MARK_UNSOLVED "$seen"; then answer '{"answer":"{\"n\":{\"abc\":\"ABC\",\"def\":\"\"},\"sig\":{}}"}'; fi
if grep -q MARK_EXIT "$seen"; then printf 'it broke\n' >&2; exit 2; fi
answer '{"answer":"{\"n\":{\"abc\":\"ABC\",\"def\":\"DEF\"},\"sig\":{\"xyz\":\"XYZ\"}}"}'
"#;

/// The player script of a case that needs solving: the real one's shape,
/// small (as `tests/youtube_own.rs` has it), with its timestamp and the
/// case's marker for the stand-in engine.
fn player_like(marker: &str) -> String {
    format!(
        r#"var _yt_player={{}};(function(g){{var window=this;'use strict';var ca="set;get;n".split(";");
var cfg={{signatureTimestamp:54321}};
var mark="{marker}";
g.ch=function(a,b){{this.url=a;this.p={{}}}};
g.ch.prototype.set=function(a,b){{this.p[a]=b}};
g.ch.prototype.get=function(a){{return this.p[a]}};
g.ch.prototype.Zn=function(){{var a=this.get(ca[2]);a&&this.set(ca[2],a.toUpperCase())}};
g.ch.prototype.clone=function(){{return new g.ch(this.url)}};
Ws=function(a){{return a.toUpperCase()}};
om=function(a,b,c){{b=b===void 0?"":b;c=c===void 0?"":c;a=new g.ch(a,!0);a.set("alr","yes");c&&(c=Ws(decodeURIComponent(c)),a[ca[0]](b,encodeURIComponent(c)));return a}};
window.document.querySelector("video").play();
}})(_yt_player);
"#
    )
}

fn stand_in() -> &'static StandIn {
    STAND_IN.get_or_init(|| {
        // A folder of the build's own, not the system's: the stand-in lives as
        // long as the test program, so a temporary folder of the system's was
        // left behind by every run. One for each test process: nextest runs
        // each test in a process of its own, side by side, and one folder for
        // all had each process empty it under the others (a test whose engine
        // was deleted as it started it, "No such file or directory"). Those
        // left by runs over an hour ago are cleared.
        let build = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        if let Ok(entries) = std::fs::read_dir(&build) {
            for entry in entries.flatten() {
                let old = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|at| at.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(3600));
                if old
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("youtube-stand-in")
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
        let home = build.join(format!("youtube-stand-in-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let host = home.join("engines/host");
        std::fs::create_dir_all(home.join("seen")).unwrap();
        std::fs::create_dir_all(host.parent().unwrap()).unwrap();
        std::fs::write(&host, ENGINE).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(listener.local_addr().unwrap()).unwrap();
                let app = axum::Router::new().fallback(handle);
                axum::serve(listener, app).await.unwrap();
            });
        });
        let origin = format!("http://{}", rx.recv().unwrap());
        std::env::set_var("DM_NO_SECRET_STORE", "1");
        std::env::set_var("DM_YOUTUBE_HOST", &origin);
        dm_core::javascript::set_host(Some(dm_core::javascript::Host {
            program: host,
            args: Vec::new(),
            env: vec![("SEEN".into(), home.join("seen").display().to_string())],
        }));
        StandIn {
            origin,
            home,
            asked: Mutex::new(Vec::new()),
            calls: Mutex::new(HashMap::new()),
        }
    })
}

/// Every request so far whose address holds `needle`.
fn asked_with(needle: &str) -> Vec<Asked> {
    stand_in()
        .asked
        .lock()
        .unwrap()
        .iter()
        .filter(|a| a.uri.contains(needle) || a.body.contains(needle))
        .cloned()
        .collect()
}

fn text(status: u16, kind: &str, body: impl Into<Body>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", kind)
        .body(body.into())
        .unwrap()
}

fn query(uri: &Uri, key: &str) -> Option<String> {
    url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

async fn handle(method: Method, uri: Uri, headers: HeaderMap, body: String) -> Response {
    let state = STAND_IN.get().expect("the stand-in is up");
    state.asked.lock().unwrap().push(Asked {
        method: method.to_string(),
        uri: uri.to_string(),
        headers: headers.clone(),
        body: body.clone(),
    });
    let path = uri.path().to_string();
    match (method.as_str(), path.as_str()) {
        ("GET", "/watch") => watch_page(&query(&uri, "v").unwrap_or_default()),
        ("POST", "/youtubei/v1/player") => {
            let asked: Value = serde_json::from_str(&body).unwrap_or_default();
            let id = asked["videoId"].as_str().unwrap_or("").to_string();
            let call = {
                let mut calls = state.calls.lock().unwrap();
                let n = calls.entry(id.clone()).or_default();
                *n += 1;
                *n
            };
            player(&id, call, &state.origin)
        }
        ("GET", "/playlist") => playlist_page(&query(&uri, "list").unwrap_or_default()),
        ("POST", "/youtubei/v1/browse") => {
            let asked: Value = serde_json::from_str(&body).unwrap_or_default();
            browse(asked["continuation"].as_str().unwrap_or(""))
        }
        (_, p) if p.starts_with("/s/player/") => player_script(p),
        (_, p) if p.starts_with("/media/") => media(p, &uri, &headers).await,
        _ => text(404, "text/plain", "no such page"),
    }
}

/// A `ytcfg.set` with `config` in it.
fn ytcfg(config: Value) -> String {
    format!("<script>ytcfg.set({config});</script>")
}

/// Calls a page makes that are no use to dm: no object, an object that is
/// not JSON, one with nothing dm reads, and one left open. Each is passed
/// over for the calls after it.
const ODD_CONFIG: &str = r#"<script>ytcfg.set(window.x);ytcfg.set({not: json});ytcfg.set({"LOGGED_IN": false});ytcfg.set({"left": "open"</script>"#;

/// The watch page of each case.
fn watch_page(id: &str) -> Response {
    let usual = json!({"PLAYER_JS_URL": format!("/s/player/{id}/base.js"), "STS": 12345, "VISITOR_DATA": "visitor-1"});
    let html = match id {
        "pageRefuse1" => return text(403, "text/plain", "not for you"),
        "hugePage001" => return text(200, "text/html", vec![b'a'; 17 << 20]),
        "solvedVid01" => ytcfg(json!({
            "INNERTUBE_CONTEXT": {"client": {"visitorData": "visitor-2"}},
            "PLAYER_JS_URL": format!("https://www.youtube.com/s/player/{id}/player_ias.vflset/en_US/base.js"),
        })),
        "noScriptUrl" => ytcfg(json!({"STS": 12345})),
        "farScript01" => ytcfg(json!({"PLAYER_JS_URL": "https://far.example/s/player/x/base.js"})),
        "scriptGone1" => ytcfg(json!({"PLAYER_JS_URL": "/s/player/gone/base.js"})),
        "scriptGone2" => ytcfg(json!({"PLAYER_JS_URL": "/s/player/gone/base.js", "STS": 12345})),
        "titleOnPage" => format!(
            "<html><head><title>Rock &amp; Roll &quot;live&quot; - YouTube</title></head>{}</html>",
            ytcfg(usual)
        ),
        "captions001" => {
            let tracks = json!({"videoDetails": {"title": "A talk"}, "captions": {"playerCaptionsTracklistRenderer": {"captionTracks": [
                {"baseUrl": "/api/timedtext?v=captions001&lang=en&fmt=srv3", "languageCode": "en"},
                {"baseUrl": "https://www.youtube.com/api/timedtext?v=captions001&lang=de&kind=asr", "languageCode": "de", "kind": "asr"},
                {"baseUrl": "http://www.youtube.com/api/timedtext?lang=fr", "languageCode": "fr"},
                {"languageCode": "es"},
                {"baseUrl": ":99999", "languageCode": "it"}
            ]}}});
            format!("<script>var ytInitialPlayerResponse = {tracks};</script>")
        }
        "captions002" => {
            let untitled = json!({"captions": {"playerCaptionsTracklistRenderer": {"captionTracks": [
                {"baseUrl": "https://www.youtube.com/api/timedtext?lang=pt", "languageCode": "pt"}
            ]}}});
            format!("<script>var ytInitialPlayerResponse = {untitled};</script>")
        }
        _ => format!("{ODD_CONFIG}{}", ytcfg(usual)),
    };
    text(200, "text/html", format!("<html>{html}</html>"))
}

/// The player script of each case.
fn player_script(path: &str) -> Response {
    let id = path
        .strip_prefix("/s/player/")
        .and_then(|p| p.split('/').next())
        .unwrap_or("");
    let marker = match id {
        "gone" => return text(404, "text/plain", "gone"),
        "solverErr01" => "MARK_ERROR",
        "solverJunk1" => "MARK_NOJSON",
        "solverNone1" => "MARK_NORESP",
        "solverFail1" | "solveNext01" => "MARK_UNSOLVED",
        "solverExit1" => "MARK_EXIT",
        // A player whose shape dm does not know: its timestamp is read, but
        // no function in it deciphers a signature.
        "oddPlayer01" => {
            return text(
                200,
                "text/javascript",
                "var player = {signatureTimestamp:54321};",
            )
        }
        _ => "",
    };
    text(200, "text/javascript", player_like(marker))
}

fn video(itag: u64, mime: &str, (w, h, fps): (u64, u64, u64), size: usize, url: &str) -> Value {
    json!({"itag": itag, "mimeType": mime, "width": w, "height": h, "fps": fps,
           "bitrate": w * h, "contentLength": size.to_string(), "url": url})
}

fn audio(itag: u64, mime: &str, bitrate: u64, size: usize, url: &str) -> Value {
    json!({"itag": itag, "mimeType": mime, "averageBitrate": bitrate,
           "contentLength": size.to_string(), "url": url})
}

const MP4: &str = "video/mp4; codecs=\"avc1.640028\"";
const WEBM: &str = "video/webm; codecs=\"vp9\"";
const M4A: &str = "audio/mp4; codecs=\"mp4a.40.2\"";
const OPUS: &str = "audio/webm; codecs=\"opus\"";

fn ok(title: Option<&str>, adaptive: Vec<Value>) -> Value {
    let mut details = json!({"lengthSeconds": "19"});
    if let Some(title) = title {
        details["title"] = json!(title);
    }
    json!({"playabilityStatus": {"status": "OK"}, "videoDetails": details,
           "streamingData": {"adaptiveFormats": adaptive}})
}

/// The usual streams of a reading case: no `n` values, so no solver.
fn plain_streams() -> Vec<Value> {
    let r = "https://r1.example";
    let mut dub = audio(140, M4A, 999_000, 600, &format!("{r}/dub"));
    dub["audioTrack"] = json!({"audioIsDefault": false});
    let mut indexed = video(137, MP4, (1920, 1080, 30), 8000, &format!("{r}/v137"));
    indexed["initRange"] = json!({"start": "0", "end": "740"});
    indexed["indexRange"] = json!({"start": 741, "end": 900});
    vec![
        indexed,
        video(248, WEBM, (1920, 1080, 30), 6000, &format!("{r}/v248")),
        video(136, MP4, (1280, 720, 30), 2000, &format!("{r}/v136")),
        audio(140, M4A, 130_000, 500, &format!("{r}/a140")),
        audio(251, OPUS, 140_000, 400, &format!("{r}/a251")),
        dub,
    ]
}

/// The streams of a download case, served by the stand-in.
fn served_streams(origin: &str, id: &str, call: usize, extra: &str) -> Vec<Value> {
    let m = media_data();
    let edges = |(a, b): (u64, u64)| json!({"start": a.to_string(), "end": b.to_string()});
    let at = |kind: &str| format!("{origin}/media/{kind}?id={id}&try={call}{extra}");
    let mut picture = video(137, MP4, (1920, 1080, 30), m.picture.len(), &at("picture"));
    picture["initRange"] = edges(m.picture_init);
    picture["indexRange"] = edges(m.picture_index);
    let mut sound = audio(140, M4A, 130_000, m.sound.len(), &at("sound"));
    sound["initRange"] = edges(m.sound_init);
    sound["indexRange"] = edges(m.sound_index);
    vec![picture, sound]
}

/// The player API's answer for each case; `call` counts its asks.
fn player(id: &str, call: usize, origin: &str) -> Response {
    let r = "https://r1.example";
    let answer = match id {
        "apiRefuse01" => return text(500, "text/plain", "broken"),
        "notJsonAns1" => return text(200, "text/html", "<html>not json</html>"),
        "notPlayabl1" => {
            json!({"playabilityStatus": {"status": "UNPLAYABLE", "reason": "Video unavailable"}})
        }
        "loginNeeded" => json!({"playabilityStatus": {"status": "LOGIN_REQUIRED"}}),
        "liveStream1" => {
            let mut live = ok(Some("Live"), plain_streams());
            live["videoDetails"]["isLive"] = json!(true);
            live
        }
        "liveHlsUrl1" => {
            let mut live = ok(Some("Live now"), plain_streams());
            live["videoDetails"]["isLive"] = json!(true);
            live["streamingData"]["hlsManifestUrl"] =
                json!("https://manifest.googlevideo.com/api/manifest/hls_variant/live.m3u8");
            live
        }
        "noStreams01" => ok(
            Some("Nothing"),
            vec![
                json!({"itag": 22, "mimeType": MP4, "drmFamilies": ["w"], "url": format!("{r}/drm")}),
                json!({"itag": 99, "mimeType": MP4, "type": "FORMAT_STREAM_TYPE_OTF", "url": format!("{r}/otf")}),
                json!({"itag": 98, "mimeType": MP4, "targetDurationSec": 5, "url": format!("{r}/live")}),
                json!({"itag": 97, "mimeType": "text/vtt", "url": format!("{r}/text")}),
                json!({"itag": 96, "mimeType": MP4, "width": 1, "height": 1}),
                json!({"itag": 95, "url": format!("{r}/no-type")}),
            ],
        ),
        "solvedVid01" => ok(
            Some("Solved"),
            vec![
                video(
                    137,
                    MP4,
                    (1920, 1080, 30),
                    8000,
                    &format!("{r}/v?itag=137&n=abc"),
                ),
                json!({"itag": 140, "mimeType": M4A, "averageBitrate": 130000,
                       "signatureCipher": format!("s=xyz&sp=sig&url={}", encoded(&format!("{r}/a?itag=140&n=def")))}),
            ],
        ),
        "sigDefault1" => ok(
            Some("Signed"),
            vec![
                video(
                    137,
                    MP4,
                    (1920, 1080, 30),
                    8000,
                    &format!("{r}/v?itag=137&n=abc"),
                ),
                json!({"itag": 140, "mimeType": M4A, "averageBitrate": 130000,
                       "cipher": format!("s=xyz&url={}", encoded(&format!("{r}/a?itag=140&n=def")))}),
            ],
        ),
        "noScriptUrl" | "farScript01" | "scriptGone2" | "badAddress1" | "solverErr01"
        | "noEngine001" | "denoEngine1" | "oddPlayer01" | "solverJunk1" | "solverNone1"
        | "solverFail1" | "solverExit1" => {
            let mut streams = vec![
                video(
                    137,
                    MP4,
                    (1920, 1080, 30),
                    8000,
                    &format!("{r}/v?itag=137&n=abc"),
                ),
                audio(140, M4A, 130_000, 500, &format!("{r}/a?itag=140&n=def")),
            ];
            if id == "badAddress1" {
                streams.push(audio(141, M4A, 1, 1, "not an address"));
            }
            ok(Some("Needs solving"), streams)
        }
        // Each reading's first client (odd asks) has addresses with an `n` the player gives
        // no answer for; the next client's (even asks) want none.
        "solveNext01" if call % 2 == 0 => ok(Some("Next client"), plain_streams()),
        "solveNext01" => ok(
            Some("Needs solving"),
            vec![
                video(137, MP4, (1920, 1080, 30), 8000, &format!("{r}/v?n=abc")),
                audio(140, M4A, 130_000, 500, &format!("{r}/a?n=def")),
            ],
        ),
        "cipherNoUrl" | "cipherNoSig" | "sigUnknown1" => {
            let cipher = match id {
                "cipherNoUrl" => "s=xyz&sp=sig".to_string(),
                "cipherNoSig" => format!("sp=sig&url={}", encoded(&format!("{r}/a?n=def"))),
                _ => format!("s=qqq&url={}", encoded(&format!("{r}/a?n=def"))),
            };
            ok(
                Some("Locked"),
                vec![
                    video(137, MP4, (1920, 1080, 30), 8000, &format!("{r}/v?n=abc")),
                    json!({"itag": 140, "mimeType": M4A, "averageBitrate": 1, "signatureCipher": cipher}),
                ],
            )
        }
        "muxedOnly01" => json!({"playabilityStatus": {"status": "OK"},
        "videoDetails": {"title": "Muxed"},
        "streamingData": {"formats": [
            video(18, "video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"", (640, 360, 30), 5000, &format!("{r}/v18")),
        ]}}),
        "webmNoOpus1" => ok(
            Some("No Opus"),
            vec![
                video(248, WEBM, (1920, 1080, 30), 6000, &format!("{r}/v248")),
                audio(140, M4A, 130_000, 500, &format!("{r}/a140")),
            ],
        ),
        "titleOnPage" => ok(None, plain_streams()),
        "untitled001" => ok(None, plain_streams()),
        "av1Picture1" => {
            let mut streams = plain_streams();
            streams.push(video(
                399,
                "video/mp4; codecs=\"av01.0.08M.08\"",
                (1920, 1080, 30),
                3000,
                &format!("{r}/v399"),
            ));
            ok(Some("AV1"), streams)
        }
        "ftpStreams1" => ok(
            Some("Elsewhere"),
            vec![
                video(137, MP4, (1920, 1080, 30), 8000, "ftp://r1.example/v137"),
                audio(140, M4A, 130_000, 500, "ftp://r1.example/a140"),
            ],
        ),
        "webmPair001" => {
            let m = media_data();
            let at = |kind: &str| format!("{origin}/media/{kind}?id={id}");
            ok(
                Some("Talk"),
                vec![
                    video(
                        247,
                        WEBM,
                        (1280, 720, 30),
                        m.webm_picture.len(),
                        &at("wpicture"),
                    ),
                    audio(251, OPUS, 140_000, m.webm_sound.len(), &at("wsound")),
                ],
            )
        }
        "renewed0001" | "renewDiff01" | "renewFail01" => {
            if call > 1 && id == "renewFail01" {
                json!({"playabilityStatus": {"status": "ERROR", "reason": "gone now"}})
            } else {
                let mut streams = served_streams(origin, id, call, "&expire=1");
                if call > 1 && id == "renewDiff01" {
                    streams[0]["contentLength"] = json!("1");
                }
                ok(Some("Talk"), streams)
            }
        }
        id if id.starts_with("dl")
            || matches!(
                id,
                "gone0000001" | "retry403001" | "cancelled01" | "paused00001"
            ) =>
        {
            ok(Some("Talk"), served_streams(origin, id, call, ""))
        }
        _ => ok(Some("Plain: one"), plain_streams()),
    };
    text(200, "application/json", answer.to_string())
}

/// `text` percent-encoded as a query value.
fn encoded(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

// ---- The streams' bytes ----

struct Media {
    picture: Vec<u8>,
    picture_init: (u64, u64),
    picture_index: (u64, u64),
    sound: Vec<u8>,
    sound_init: (u64, u64),
    sound_index: (u64, u64),
    webm_picture: Vec<u8>,
    webm_sound: Vec<u8>,
}

fn media_data() -> &'static Media {
    static MEDIA: OnceLock<Media> = OnceLock::new();
    MEDIA.get_or_init(|| {
        let (picture, picture_init, picture_index) = pieced("mp4/picture.f137.mp4", 6);
        let (sound, sound_init, sound_index) = pieced("mp4/sound.f140.m4a", 6);
        Media {
            picture,
            picture_init,
            picture_index,
            sound,
            sound_init,
            sound_index,
            webm_picture: fixture("webm/picture.f247.webm"),
            webm_sound: fixture("webm/sound.f251.webm"),
        }
    })
}

async fn media(path: &str, uri: &Uri, headers: &HeaderMap) -> Response {
    let id = query(uri, "id").unwrap_or_default();
    let first_try = query(uri, "try").as_deref() == Some("1");
    match id.as_str() {
        "gone0000001" => return text(410, "text/plain", "gone"),
        "renewed0001" | "renewDiff01" | "renewFail01" if first_try => {
            return text(410, "text/plain", "this address has run out")
        }
        // Refused for the first two seconds, through every retry of the
        // engine's own, then served.
        "retry403001" if first_asked(&id).elapsed() < Duration::from_secs(2) => {
            return text(403, "text/plain", "not yet")
        }
        "cancelled01" | "paused00001" => {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        _ => {}
    }
    let m = media_data();
    let data: &[u8] = match path {
        "/media/picture" => &m.picture,
        "/media/sound" => &m.sound,
        "/media/wpicture" => &m.webm_picture,
        "/media/wsound" => &m.webm_sound,
        _ => return text(404, "text/plain", "no such stream"),
    };
    ranged(data, headers)
}

/// When `id`'s streams were first asked for.
fn first_asked(id: &str) -> std::time::Instant {
    static FIRST: OnceLock<Mutex<HashMap<String, std::time::Instant>>> = OnceLock::new();
    *FIRST
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(id.to_string())
        .or_insert_with(std::time::Instant::now)
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// The top-level boxes of `data`: each one's kind, where it starts and its size.
fn top_boxes(data: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let mut at = 0;
    let mut out = Vec::new();
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().unwrap();
        let size = if size == 0 { data.len() - at } else { size };
        out.push((kind, at, size));
        at += size;
    }
    out
}

/// A box of `kind` inside `data`, depth first: where its body starts and its length.
fn find_box(data: &[u8], kind: &[u8; 4]) -> Option<(usize, usize)> {
    let mut at = 0;
    while at + 8 <= data.len() {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        let k = &data[at + 4..at + 8];
        if size < 8 || at + size > data.len() {
            return None;
        }
        if k == kind {
            return Some((at + 8, size - 8));
        }
        if matches!(
            k,
            b"moov" | b"trak" | b"edts" | b"moof" | b"traf" | b"mdia" | b"minf"
        ) {
            if let Some(found) = find_box(&data[at + 8..at + size], kind) {
                return Some((at + 8 + found.0, found.1));
            }
        }
        at += size;
    }
    None
}

/// A one-piece fixture as a stream of `pieces` pieces under a new index,
/// as YouTube's DASH streams come (the same as `tests/youtube_own.rs`):
/// the file, its first boxes' range and its index's range.
fn pieced(name: &str, pieces: u32) -> (Vec<u8>, (u64, u64), (u64, u64)) {
    let whole = fixture(name);
    let boxes = top_boxes(&whole);
    let at = |kind: &[u8; 4]| {
        boxes
            .iter()
            .find(|(k, _, _)| k == kind)
            .map(|(_, a, s)| (*a, *s))
            .unwrap()
    };
    let (moov_at, moov_size) = at(b"moov");
    let (sidx_at, sidx_size) = at(b"sidx");
    let (moof_at, moof_size) = at(b"moof");
    let (mdat_at, mdat_size) = at(b"mdat");
    let init = whole[..moov_at + moov_size].to_vec();
    let sidx = &whole[sidx_at..sidx_at + sidx_size];
    let timescale = u32::from_be_bytes(sidx[16..20].try_into().unwrap());
    let entries = if sidx[8] == 1 { 40 } else { 32 };
    let duration = u32::from_be_bytes(sidx[entries + 4..entries + 8].try_into().unwrap());
    let piece_len = (moof_size + mdat_size) as u32;
    let mut index = Vec::new();
    index.extend_from_slice(&(32 + 12 * pieces).to_be_bytes());
    index.extend_from_slice(b"sidx");
    index.extend_from_slice(&[0, 0, 0, 0]);
    index.extend_from_slice(&1u32.to_be_bytes());
    index.extend_from_slice(&timescale.to_be_bytes());
    index.extend_from_slice(&0u32.to_be_bytes());
    index.extend_from_slice(&0u32.to_be_bytes());
    index.extend_from_slice(&0u16.to_be_bytes());
    index.extend_from_slice(&(pieces as u16).to_be_bytes());
    for _ in 0..pieces {
        index.extend_from_slice(&piece_len.to_be_bytes());
        index.extend_from_slice(&duration.to_be_bytes());
        index.extend_from_slice(&0x9000_0000u32.to_be_bytes());
    }
    let mut file = init.clone();
    file.extend_from_slice(&index);
    for n in 0..pieces {
        let mut piece = whole[moof_at..mdat_at + mdat_size].to_vec();
        let (seq_at, _) = find_box(&piece, b"mfhd").unwrap();
        piece[seq_at + 4..seq_at + 8].copy_from_slice(&(n + 1).to_be_bytes());
        let (tfdt_at, _) = find_box(&piece, b"tfdt").unwrap();
        let time = u64::from(n) * u64::from(duration);
        if piece[tfdt_at] == 1 {
            piece[tfdt_at + 4..tfdt_at + 12].copy_from_slice(&time.to_be_bytes());
        } else {
            piece[tfdt_at + 4..tfdt_at + 8].copy_from_slice(&(time as u32).to_be_bytes());
        }
        file.extend_from_slice(&piece);
    }
    let init_range = (0, init.len() as u64 - 1);
    let index_range = (init.len() as u64, (init.len() + index.len()) as u64 - 1);
    (file, init_range, index_range)
}

/// A piece of `data` as a range request asks for it.
fn ranged(data: &[u8], headers: &HeaderMap) -> Response {
    let size = data.len();
    let (a, b) = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            (
                a.parse::<usize>().unwrap(),
                b.parse::<usize>().unwrap_or(size - 1).min(size - 1),
            )
        })
        .unwrap_or((0, size - 1));
    Response::builder()
        .status(206)
        .header("content-range", format!("bytes {a}-{b}/{size}"))
        .header("content-length", b - a + 1)
        .header("etag", "\"v\"")
        .body(Body::from(bytes::Bytes::copy_from_slice(&data[a..=b])))
        .unwrap()
}

// ---- Playlists ----

fn lockup(id: &str, title: &str) -> Value {
    json!({"lockupViewModel": {"contentId": id, "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
        "metadata": {"lockupMetadataViewModel": {"title": {"content": title}}}}})
}

fn next(token: &str) -> Value {
    json!({"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": token}}}})
}

fn with_data(data: Value, config: Option<Value>) -> String {
    let cfg = format!("{ODD_CONFIG}{}", config.map(ytcfg).unwrap_or_default());
    format!("<html>{cfg}<script>var ytInitialData = {data};</script></html>")
}

/// The playlist page of each case.
fn playlist_page(list: &str) -> Response {
    let version = json!({"INNERTUBE_CLIENT_VERSION": "2.20260101.07.00"});
    let html = match list {
        "PLgone" => return text(404, "text/plain", "no such list"),
        "PLnodata" => "<html><p>sign in</p></html>".to_string(),
        "PLempty" => with_data(
            json!({"metadata": {"playlistMetadataRenderer": {"title": "Empty"}}, "contents": []}),
            None,
        ),
        "PLmain" => with_data(
            json!({"header": {"playlistHeaderRenderer": {"title": {"simpleText": " Main list "}}},
                   "contents": [lockup("main0000001", "One"), lockup("main0000002", "Two"),
                                lockup("main0000005", "[Private video]"),
                                {"playlistVideoRenderer": {"videoId": "main0000004", "lengthSeconds": 61}},
                                next("T1")]}),
            Some(version),
        ),
        "PLnover" => with_data(
            json!({"header": {"pageHeaderRenderer": {"pageTitle": "Paged"}},
                   "contents": [lockup("nover000001", ""), next("T9")]}),
            None,
        ),
        "PLjunk" => with_data(
            json!({"microformat": {"microformatDataRenderer": {"title": {"runs": [{"text": "Micro"}, {"text": " format"}]}}},
                   "contents": [lockup("junk0000001", "One"), next("JUNK")]}),
            Some(version),
        ),
        "PLhuge" => with_data(
            json!({"header": {"playlistHeaderRenderer": {"title": {"content": "Huge answer"}}},
                   "contents": [lockup("huge0000001", "One"), next("HUGE")]}),
            Some(version),
        ),
        "PLbadver" => with_data(
            json!({"contents": [lockup("badver00001", "One"), next("T1")]}),
            Some(json!({"INNERTUBE_CLIENT_VERSION": "2.0\u{1}bad"})),
        ),
        "PLthousand" => {
            let mut contents: Vec<Value> = (0..1000)
                .map(|n| lockup(&format!("k{n:010}"), "x"))
                .collect();
            contents.push(next("NEVER"));
            with_data(json!({"contents": contents}), Some(version))
        }
        _ => with_data(json!({"contents": [lockup("other000001", "One")]}), None),
    };
    text(200, "text/html", html)
}

/// The browse API's answer for each continuation.
fn browse(token: &str) -> Response {
    match token {
        "T1" => text(
            200,
            "application/json",
            json!({"onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
                lockup("main0000003", "Three"), lockup("main0000001", "One again"), next("T2")]}}]})
            .to_string(),
        ),
        // A page that adds nothing new ends the reading, token or not.
        "T2" | "T9" => text(
            200,
            "application/json",
            json!({"items": [lockup("main0000003", "Three"), next("T2")]}).to_string(),
        ),
        "HUGE" => text(200, "application/json", vec![b' '; 17 << 20]),
        _ => text(500, "text/html", "<html>broken</html>"),
    }
}

// ---- Helpers for the tests ----

fn page(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// `read` at `quality` in `container`, as the parsed reading.
async fn reading(id: &str, quality: Quality, container: Option<&str>) -> Result<Value, Error> {
    let staging = tempdir();
    let client = dm_core::client();
    youtube::read(
        &client,
        &page(id),
        quality,
        container,
        false,
        None,
        staging.path(),
    )
    .await
    .map(|text| serde_json::from_str(&text).unwrap())
}

/// The error a reading of `id` fails with, in words.
async fn failure(id: &str) -> String {
    match reading(id, Quality::Best, None).await {
        Ok(read) => panic!("{id} was read: {read}"),
        Err(e) => e.to_string(),
    }
}

fn itags(reading: &Value) -> Vec<String> {
    match reading["requested_formats"].as_array() {
        Some(pair) => pair
            .iter()
            .map(|f| f["format_id"].as_str().unwrap().to_string())
            .collect(),
        None => vec![reading["format_id"].as_str().unwrap().to_string()],
    }
}

// ---- Reading a watch page ----

/// A page whose streams carry no `n` value is read with no solver: the
/// page as a browser, the player API as the TV client with the page's
/// visitor id and timestamp, and the biggest MP4 picture with the best M4A
/// sound in the reading, named after the title.
#[tokio::test]
async fn a_plain_page_is_read_as_the_tv_client_without_a_solver() {
    let _env = ENV.read().await;
    stand_in();
    let read = reading("plainFirst1", Quality::Best, None).await.unwrap();
    assert_eq!(itags(&read), ["137", "140"]);
    assert_eq!(read["duration"], 19);
    assert_eq!(read["title"], "Plain: one");
    assert!(read["filename"]
        .as_str()
        .unwrap()
        .ends_with("Plain_ one [plainFirst1].mp4"));
    assert_eq!(read["formats"].as_array().unwrap().len(), 6);
    let picture = &read["requested_formats"][0];
    assert_eq!(picture["dm_init_range"], "0-740");
    assert_eq!(picture["dm_index_range"], "741-900");
    assert_eq!(picture["filesize"], 8000);
    assert_eq!(picture["fps"], 30);
    assert_eq!(read["requested_formats"][1]["dm_init_range"], Value::Null);

    let watch = asked_with("/watch?v=plainFirst1");
    assert_eq!(watch[0].method, "GET");
    assert!(watch[0].uri.contains("bpctr=9999999999&has_verified=1"));
    assert_eq!(watch[0].header("cookie"), "PREF=hl=en&tz=UTC; SOCS=CAI");
    assert!(watch[0].header("user-agent").contains("Chrome/"));
    let api: Vec<Asked> = asked_with("\"videoId\":\"plainFirst1\"");
    assert_eq!(api.len(), 1);
    assert_eq!(api[0].header("x-youtube-client-name"), "7");
    assert_eq!(api[0].header("x-goog-visitor-id"), "visitor-1");
    assert!(api[0].header("user-agent").contains("SMART-TV"));
    let body: Value = serde_json::from_str(&api[0].body).unwrap();
    assert_eq!(
        body["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"],
        12345
    );
    assert_eq!(body["context"]["client"]["clientName"], "TVHTML5");
    assert!(
        asked_with("/s/player/plainFirst1").is_empty(),
        "the page gave the timestamp and nothing needs solving: no player script"
    );
}

/// The quality and container asked choose the streams, and the sound alone
/// is one format at the top of the reading.
#[tokio::test]
async fn the_quality_and_container_asked_choose_the_streams() {
    let _env = ENV.read().await;
    stand_in();
    let at = |q, c| async move { itags(&reading("plainVideo1", q, c).await.unwrap()) };
    assert_eq!(at(Quality::Best, Some("webm")).await, ["248", "251"]);
    assert_eq!(at(Quality::UpTo(720), None).await, ["136", "140"]);
    let sound = reading("plainVideo1", Quality::Audio, None).await.unwrap();
    assert_eq!(itags(&sound), ["140"]);
    assert!(sound.get("requested_formats").is_none());
    assert_eq!(sound["url"], "https://r1.example/a140");
    assert_eq!(sound["ext"], "m4a");
    assert_eq!(sound["vcodec"], "none");
    assert_eq!(at(Quality::Audio, Some("webm")).await, ["251"]);
    // A WebM picture with no WebM sound takes the M4A one, and the sound
    // alone in WebM is the best there is where WebM has none.
    let at = |q, c| async move { itags(&reading("webmNoOpus1", q, c).await.unwrap()) };
    assert_eq!(at(Quality::Best, Some("webm")).await, ["248", "140"]);
    assert_eq!(at(Quality::Audio, Some("webm")).await, ["140"]);
    let none = reading("plainVideo1", Quality::UpTo(100), None).await;
    assert!(
        none.unwrap_err()
            .to_string()
            .contains("no stream at that quality"),
        "nothing that small"
    );
}

/// For a part (`mp4_only`), a WebM asked for gives way to the MP4; a codec
/// asked for keeps the pictures to it, and one the video lacks is said.
#[tokio::test]
async fn mp4_only_and_a_codec_asked_keep_the_pictures_to_them() {
    let _env = ENV.read().await;
    stand_in();
    let staging = tempdir();
    let client = dm_core::client();
    let read = |id: &'static str, container, mp4_only, codec| {
        let client = client.clone();
        let staging = staging.path().to_path_buf();
        async move {
            youtube::read_at(
                &client,
                &page(id),
                Quality::Best,
                container,
                Some(60),
                None,
                mp4_only,
                codec,
                None,
                &staging,
            )
            .await
            .map(|t| itags(&serde_json::from_str(&t).unwrap()))
        }
    };
    assert_eq!(
        read("plainVideo1", Some("webm"), true, None).await.unwrap(),
        ["137", "140"]
    );
    assert_eq!(
        read("plainVideo1", None, false, Some("vp9")).await.unwrap(),
        ["248", "251"],
        "a VP9 picture in WebM, with the WebM sound"
    );
    assert_eq!(
        read("av1Picture1", Some("mp4"), false, Some("av01"))
            .await
            .unwrap(),
        ["399", "140"]
    );
    let no_av1 = read("plainVideo1", None, false, Some("av01")).await;
    assert!(no_av1.unwrap_err().to_string().contains("no AV1 stream"));
    let no_hevc = read("plainVideo1", None, false, Some("hev1")).await;
    assert!(no_hevc
        .unwrap_err()
        .to_string()
        .contains("this video has no hev1 stream"));
}

/// The person's own session for the site goes first in the page's cookies.
#[tokio::test]
async fn a_sign_in_is_sent_with_the_page() {
    let _env = ENV.read().await;
    let origin = &stand_in().origin;
    let sign_in = dm_core::cookies::SignIn::new(origin, "SID=t").unwrap();
    let staging = tempdir();
    youtube::read(
        &dm_core::client(),
        &page("signedIn001"),
        Quality::Best,
        None,
        false,
        Some(&sign_in),
        staging.path(),
    )
    .await
    .unwrap();
    let watch = asked_with("/watch?v=signedIn001");
    assert_eq!(
        watch[0].header("cookie"),
        "SID=t; PREF=hl=en&tz=UTC; SOCS=CAI"
    );
}

/// Where the page gives no timestamp the player script is fetched for it,
/// from YouTube's own path even when the page names it in full; the same
/// script is then cut and run, and its answers unlock every address.
#[tokio::test]
async fn n_values_and_a_cipher_are_unlocked_by_the_player() {
    let _env = ENV.read().await;
    stand_in();
    let staging = tempdir();
    let text = youtube::read(
        &dm_core::client(),
        &page("solvedVid01"),
        Quality::Best,
        None,
        false,
        None,
        staging.path(),
    )
    .await
    .unwrap();
    let read: Value = serde_json::from_str(&text).unwrap();
    let urls: Vec<&str> = read["requested_formats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["url"].as_str().unwrap())
        .collect();
    assert_eq!(
        urls,
        [
            "https://r1.example/v?itag=137&n=ABC",
            "https://r1.example/a?itag=140&n=DEF&sig=XYZ"
        ]
    );
    let scripts = asked_with("/s/player/solvedVid01/player_ias.vflset/en_US/base.js");
    assert_eq!(scripts.len(), 1, "fetched once, for both uses");
    let api = asked_with("\"videoId\":\"solvedVid01\"");
    assert_eq!(api[0].header("x-goog-visitor-id"), "visitor-2");
    let body: Value = serde_json::from_str(&api[0].body).unwrap();
    assert_eq!(
        body["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"], 54321,
        "read from the player script"
    );
    // The program the engine was handed: the player as dm cuts it, its
    // calls made while it loads left out, and every challenge asked.
    let request = std::fs::read_dir(stand_in().home.join("seen"))
        .unwrap()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .find(|r| {
            let call = r["call"].as_str().unwrap_or_default();
            ["\"abc\"", "\"def\"", "\"xyz\""]
                .iter()
                .all(|v| call.contains(v))
        })
        .expect("the engine ran");
    let program = request["program"].as_str().unwrap();
    assert!(program.contains(r#"a.set("alr","yes")"#));
    assert!(!program.contains("querySelector"));
    assert!(request["call"].as_str().unwrap().starts_with("__dm.solve("));
    // No program is written to the disk for it.
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
    // Nothing was fetched to answer the checks.
    assert!(asked_with("/ejs/").is_empty());
}

/// A cipher with no `sp` puts its signature under `signature`.
#[tokio::test]
async fn a_cipher_without_a_name_signs_as_signature() {
    let _env = ENV.read().await;
    stand_in();
    let read = reading("sigDefault1", Quality::Best, None).await.unwrap();
    assert_eq!(
        read["requested_formats"][1]["url"],
        "https://r1.example/a?itag=140&n=DEF&signature=XYZ"
    );
}

/// Every way the page, the API or the streams' addresses can fail is said
/// in words, not passed over.
#[tokio::test]
async fn what_youtube_refuses_is_said_in_words() {
    let _env = ENV.read().await;
    stand_in();
    for (id, said) in [
        ("notPlayabl1", "YouTube says: Video unavailable"),
        ("loginNeeded", "YouTube says: LOGIN_REQUIRED"),
        ("liveStream1", "a live stream"),
        ("noStreams01", "the player names no stream dm can fetch"),
        ("notJsonAns1", "the player API answered no JSON"),
        ("noScriptUrl", "the page names no player script"),
        ("farScript01", "the page names no player script"),
        ("hugePage001", "the page is larger than any YouTube page"),
        ("badAddress1", "a stream's address is not one"),
        ("cipherNoUrl", "a cipher with no address"),
        ("cipherNoSig", "a cipher with no signature"),
        ("sigUnknown1", "no answer for the sig value qqq"),
    ] {
        let why = failure(id).await;
        assert!(why.contains(said), "{id}: {why}");
    }
    for (id, status) in [
        ("pageRefuse1", 403),
        ("apiRefuse01", 500),
        ("scriptGone1", 404),
        ("scriptGone2", 404),
    ] {
        match reading(id, Quality::Best, None).await {
            Err(Error::Refused { status: got, .. }) => assert_eq!(got, status, "{id}"),
            other => panic!("{id}: {other:?}"),
        }
    }
    // A page that names a script elsewhere is not followed there, and its
    // timestamp is then not sent.
    let api = asked_with("\"videoId\":\"farScript01\"");
    let body: Value = serde_json::from_str(&api[0].body).unwrap();
    assert_eq!(
        body["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"],
        Value::Null
    );
    assert!(asked_with("far.example").is_empty());
    // A caption reading never asks the API.
    assert!(asked_with("\"videoId\":\"captions001\"").is_empty());
}

/// A live video whose player names YouTube's HLS playlist of it is read as that live stream,
/// for the engine's live recording (gaps row Y5); one that names none is refused as before.
#[tokio::test]
async fn a_live_video_is_read_as_its_live_playlist() {
    let _env = ENV.read().await;
    stand_in();
    let staging = tempdir();
    let text = youtube::read(
        &dm_core::client(),
        &page("liveHlsUrl1"),
        Quality::Best,
        None,
        false,
        None,
        staging.path(),
    )
    .await
    .unwrap();
    let read: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(read["is_live"], true);
    assert_eq!(read["protocol"], "m3u8_native");
    assert!(read["url"].as_str().unwrap().ends_with("live.m3u8"));
    assert!(read["filename"].as_str().unwrap().contains("Live now"));
}

/// What the engine says when the player fails in it, or what it answers
/// that is of no use, is the reading's failure, in words.
#[tokio::test]
async fn a_player_that_fails_fails_the_reading_with_its_words() {
    let _env = ENV.read().await;
    stand_in();
    for (id, said) in [
        ("solverErr01", "the JavaScript stopped: the player changed"),
        ("solverJunk1", "the answer was \"nothing useful\""),
        ("solverNone1", "no answer for the n value abc"),
        ("solverFail1", "no answer for the n value def"),
        ("solverExit1", "ended without an answer"),
        ("oddPlayer01", "YouTube's player is not as dm knows it"),
    ] {
        let why = failure(id).await;
        assert!(
            why.contains("YouTube's checks were not answered") && why.contains(said),
            "{id}: {why}"
        );
    }
}

/// A player whose checks are not answered for one client's addresses passes the reading on to
/// the next client, notes the player's id once and hands it to the window once, for fresh
/// rules (gaps row Y3). Held alone: the served clients are the process's.
#[tokio::test]
async fn a_player_not_answered_for_one_client_passes_on_to_the_next_and_is_noted() {
    let _env = ENV.write().await;
    stand_in();
    youtube::set_player_clients(vec![youtube::PlayerClient {
        name: "SERVED".into(),
        version: "1.0".into(),
        id: "9".into(),
        agent: "served agent".into(),
        context: Vec::new(),
    }]);
    let read = reading("solveNext01", Quality::Best, None).await;
    let again = reading("solveNext01", Quality::Best, None).await;
    youtube::set_player_clients(Vec::new());
    again.expect("read again by the next client");
    let read = read.expect("the next client's streams want no answer");
    assert_eq!(read["title"], "Next client");
    assert!(
        youtube::unsolved_players().contains(&"solveNext01".to_string()),
        "{:?}",
        youtube::unsolved_players()
    );
    // Met twice: noted and handed on once.
    assert_eq!(
        youtube::unsolved_players()
            .iter()
            .filter(|id| *id == "solveNext01")
            .count(),
        1
    );
    let handed: Vec<String> = std::iter::from_fn(youtube::take_unsolved_player).collect();
    assert_eq!(
        handed.iter().filter(|id| *id == "solveNext01").count(),
        1,
        "{handed:?}"
    );
    assert!(youtube::take_unsolved_player().is_none());
}

/// The TV client names no title: the page's own `<title>` is used without
/// the site's name and with its entities read back, else "video".
#[tokio::test]
async fn the_title_comes_from_the_page_or_is_video() {
    let _env = ENV.read().await;
    stand_in();
    let read = reading("titleOnPage", Quality::Best, None).await.unwrap();
    assert_eq!(read["title"], "Rock & Roll \"live\"");
    assert!(read["filename"]
        .as_str()
        .unwrap()
        .ends_with("Rock & Roll _live_ [titleOnPage].mp4"));
    let read = reading("untitled001", Quality::Best, None).await.unwrap();
    assert_eq!(read["title"], "video");
}

/// A video with only formats that hold picture and sound together is read
/// as that one format, as the reader's comment says it is.
#[tokio::test]
async fn a_video_with_only_muxed_formats_is_read_as_one() {
    let _env = ENV.read().await;
    stand_in();
    let read = reading("muxedOnly01", Quality::Best, None).await.unwrap();
    assert_eq!(itags(&read), ["18"]);
}

/// A caption line is the track the page's player data names, as TTML;
/// a relative address is YouTube's, and one off YouTube or not over HTTPS
/// is refused.
#[tokio::test]
async fn a_caption_line_reads_its_track_from_the_page() {
    let _env = ENV.read().await;
    stand_in();
    let staging = tempdir();
    let caption = |id: &'static str, language: &str, asr: bool| {
        let staging = staging.path().to_path_buf();
        let language = language.to_string();
        async move {
            youtube::read_at(
                &dm_core::client(),
                &page(id),
                Quality::Best,
                None,
                None,
                Some((language, asr)),
                false,
                None,
                None,
                &staging,
            )
            .await
        }
    };
    let en: Value =
        serde_json::from_str(&caption("captions001", "en", false).await.unwrap()).unwrap();
    assert_eq!(
        en["url"],
        "https://www.youtube.com/api/timedtext?v=captions001&lang=en&fmt=ttml"
    );
    assert_eq!(en["format_id"], "caption-en");
    assert!(en["filename"]
        .as_str()
        .unwrap()
        .ends_with("A talk [captions001].en.ttml"));
    let de: Value =
        serde_json::from_str(&caption("captions001", "de", true).await.unwrap()).unwrap();
    assert!(de["url"].as_str().unwrap().contains("kind=asr"));
    let pt: Value =
        serde_json::from_str(&caption("captions002", "pt", false).await.unwrap()).unwrap();
    assert_eq!(pt["title"], "video");
    for (id, language, asr, said) in [
        (
            "captions001",
            "fr",
            false,
            "the captions are not on YouTube",
        ),
        ("captions001", "es", false, "the captions have no address"),
        (
            "captions001",
            "it",
            false,
            "the captions' address is not one",
        ),
        (
            "captions001",
            "en",
            true,
            "the video has no en captions now",
        ),
        ("plainVideo1", "en", false, "the page names no captions"),
    ] {
        let why = caption(id, language, asr).await.unwrap_err().to_string();
        assert!(why.contains(said), "{language}: {why}");
    }
}

/// `read_for` reads with a download's options: through the proxy they
/// name, as a part (MP4 only), at their quality.
#[tokio::test]
async fn a_download_reads_through_its_proxy_and_as_a_part() {
    let _env = ENV.read().await;
    let origin = stand_in().origin.clone();
    let dir = tempdir();
    let mut through = options(dir.path());
    through.proxy = Some(origin);
    through.container = Some("webm".into());
    let read: Value = serde_json::from_str(
        &youtube::read_for(&page("proxied0001"), &through, dir.path())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(itags(&read), ["248", "251"]);
    let watch = asked_with("/watch?v=proxied0001");
    assert!(
        watch[0].uri.starts_with("http://127.0.0.1:"),
        "asked of the proxy in full: {}",
        watch[0].uri
    );

    let mut part = options(dir.path());
    part.container = Some("webm".into());
    part.span = dm_core::Span::new(1.0, 2.0);
    let read: Value = serde_json::from_str(
        &youtube::read_for(&page("plainVideo1"), &part, dir.path())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(itags(&read), ["137", "140"], "a part is cut from MP4");

    // A GIF of a part is made from the AV1 stream on Linux alone.
    part.gif = true;
    let gif = youtube::read_for(&page("plainVideo1"), &part, dir.path()).await;
    if cfg!(target_os = "linux") {
        assert!(gif.unwrap_err().to_string().contains("no AV1 stream"));
    } else {
        assert!(gif.is_ok());
    }

    let mut broken = options(dir.path());
    broken.proxy = Some("http://[::1".into());
    assert!(youtube::read_for(&page("plainVideo1"), &broken, dir.path())
        .await
        .is_err());
    assert!(youtube::read_for("https://example.com/", &part, dir.path())
        .await
        .unwrap_err()
        .to_string()
        .contains("not a YouTube video address"));
}

// ---- The engine ----

/// A player dm cannot cut is not run at all: no engine is started and no
/// program is left behind.
#[tokio::test]
async fn a_player_dm_cannot_cut_is_not_run() {
    let _env = ENV.read().await;
    stand_in();
    let staging = tempdir();
    let result = youtube::read(
        &dm_core::client(),
        &page("oddPlayer01"),
        Quality::Best,
        None,
        false,
        None,
        staging.path(),
    )
    .await;
    let why = result.unwrap_err().to_string();
    assert!(
        why.contains("YouTube's player is not as dm knows it"),
        "{why}"
    );
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
}

// ---- Playlists ----

async fn playlist(list: &str) -> Result<youtube::Playlist, Error> {
    youtube::read_playlist(
        &dm_core::client(),
        &format!("https://www.youtube.com/playlist?list={list}"),
        None,
    )
    .await
}

/// A playlist is its page's first videos and then the browse API's, asked
/// with the page's client version and each continuation, until a page
/// adds nothing new.
#[tokio::test]
async fn a_playlist_is_read_page_by_page() {
    let _env = ENV.read().await;
    stand_in();
    let list = playlist("PLmain").await.unwrap();
    assert_eq!(list.id, "PLmain");
    assert_eq!(list.title, "Main list");
    let ids: Vec<&str> = list.entries.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids,
        ["main0000001", "main0000002", "main0000004", "main0000003"]
    );
    assert_eq!(list.entries[2].title, "main0000004", "no title: its id");
    assert_eq!(list.entries[2].seconds, Some(61));
    assert!(!list.cut_short);
    let browse = asked_with("\"continuation\":\"T1\"");
    assert_eq!(browse.len(), 1);
    assert_eq!(
        browse[0].header("x-youtube-client-version"),
        "2.20260101.07.00"
    );
    assert_eq!(browse[0].header("x-youtube-client-name"), "1");
    assert_eq!(browse[0].header("cookie"), "PREF=hl=en&tz=UTC; SOCS=CAI");

    // The person's own session for the site goes first in its cookies.
    let sign_in = dm_core::cookies::SignIn::new(&stand_in().origin, "SID=t").unwrap();
    youtube::read_playlist(
        &dm_core::client(),
        "https://www.youtube.com/playlist?list=PLsigned",
        Some(&sign_in),
    )
    .await
    .unwrap();
    let page = asked_with("/playlist?list=PLsigned");
    assert_eq!(
        page[0].header("cookie"),
        "SID=t; PREF=hl=en&tz=UTC; SOCS=CAI"
    );

    // No client version on the page: the one dm knows. A video with no
    // title is named by its id.
    let list = playlist("PLnover").await.unwrap();
    assert_eq!(list.title, "Paged");
    assert_eq!(list.entries[0].title, "nover000001");
    let browse = asked_with("\"continuation\":\"T9\"");
    assert_eq!(
        browse[0].header("x-youtube-client-version"),
        "2.20261020.00.00"
    );
}

/// A continuation that cannot be read ends the list there, said as cut
/// short; a thousand videos end it without asking for more.
#[tokio::test]
async fn a_playlist_cut_short_says_so() {
    let _env = ENV.read().await;
    stand_in();
    for (list, title) in [
        ("PLjunk", "Micro format"),
        ("PLhuge", "Huge answer"),
        ("PLbadver", "Playlist PLbadver"),
    ] {
        let read = playlist(list).await.unwrap();
        assert!(read.cut_short, "{list}");
        assert_eq!(read.entries.len(), 1, "{list}");
        assert_eq!(read.title, title);
    }
    let thousand = playlist("PLthousand").await.unwrap();
    assert_eq!(thousand.entries.len(), 1000);
    assert!(thousand.cut_short);
    assert!(asked_with("NEVER").is_empty(), "no page past a thousand");
}

/// A playlist page with no list, no videos, or refused, is an error in
/// words; so is an address that is not a playlist's.
#[tokio::test]
async fn a_playlist_that_cannot_be_read_says_why() {
    let _env = ENV.read().await;
    stand_in();
    let why = |r: Result<youtube::Playlist, Error>| r.unwrap_err().to_string();
    assert!(why(playlist("PLnodata").await).contains("holds no list"));
    assert!(why(playlist("PLempty").await).contains("lists no videos"));
    assert!(matches!(
        playlist("PLgone").await,
        Err(Error::Refused { status: 404, .. })
    ));
    let not = youtube::read_playlist(
        &dm_core::client(),
        "https://www.youtube.com/watch?v=plainVideo1",
        None,
    )
    .await;
    assert!(why(not).contains("not a YouTube playlist address"));
    // Through the page reader too: another site's address is no playlist.
    assert_eq!(
        videopage::read_playlist("https://example.com/list", None, None, None)
            .await
            .unwrap(),
        None
    );
}

// ---- Fetching what was read (`videopage::download_page`) ----

/// The media requests for `id` so far.
fn media_asked(id: &str) -> Vec<String> {
    asked_with(&format!("id={id}"))
        .into_iter()
        .filter(|a| a.uri.contains("/media/"))
        .map(|a| a.uri)
        .collect()
}

fn mp4_tracks(path: &Path) -> usize {
    let data = std::fs::read(path).unwrap();
    data.windows(4).filter(|w| *w == b"trak").count()
}

/// The picture and the sound are fetched by dm's engine and joined into
/// one MP4, named after the title, with the row told its name and the two
/// streams' progress as one.
#[tokio::test(flavor = "multi_thread")]
async fn a_video_is_fetched_joined_and_its_progress_summed() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let heard = tokio::spawn(async move {
        let mut all = Vec::new();
        while let Some(update) = rx.recv().await {
            all.push(update);
        }
        all
    });
    let done = videopage::download_page(
        &dm_core::client(),
        &page("dlWhole0001"),
        &options(dir.path()),
        Some(tx),
    )
    .await
    .unwrap();
    assert_eq!(
        done.path,
        dir.path().join("out").join("Talk [dlWhole0001].mp4")
    );
    assert_eq!(done.connections, 8, "the row's own connection count");
    assert_eq!(mp4_tracks(&done.path), 2);
    assert!(!dir.path().join("parts").exists(), "the staging is cleared");
    let updates = heard.await.unwrap();
    match &updates[0] {
        dm_core::Update::Named { name, .. } => assert_eq!(name, "Talk [dlWhole0001].mp4"),
        other => panic!("not named first: {other:?}"),
    }
    let m = media_data();
    let last = updates
        .iter()
        .rev()
        .find_map(|u| match u {
            dm_core::Update::Progress(p) => Some(*p),
            _ => None,
        })
        .expect("progress");
    assert_eq!(last.total, Some((m.picture.len() + m.sound.len()) as u64));
    assert_eq!(last.downloaded, last.total.unwrap());
}

/// A stream already whole in the staging (a run that stopped after it) is
/// not fetched again.
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_already_fetched_is_not_fetched_again() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let staging = dir.path().join("parts");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(
        staging.join("Talk [dlResume001].f137.mp4"),
        &media_data().picture,
    )
    .unwrap();
    let done = videopage::download_page(
        &dm_core::client(),
        &page("dlResume001"),
        &options(dir.path()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(mp4_tracks(&done.path), 2);
    let asked = media_asked("dlResume001");
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|u| u.starts_with("/media/sound")),
        "only the sound was asked for: {asked:?}"
    );
}

/// A WebM asked for, where the picture and sound are WebM, is joined as
/// WebM.
#[tokio::test(flavor = "multi_thread")]
async fn webm_streams_are_joined_as_webm() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let mut webm = options(dir.path());
    webm.container = Some("webm".into());
    let done = videopage::download_page(&dm_core::client(), &page("webmPair001"), &webm, None)
        .await
        .unwrap();
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Talk [webmPair001].webm"
    );
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(&data[..4], &[0x1a, 0x45, 0xdf, 0xa3], "a Matroska file");
}

/// A part of the sound alone: its pieces in the span, cut, under a name of
/// the span's own.
#[tokio::test(flavor = "multi_thread")]
async fn a_part_of_the_sound_alone_is_cut() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let mut part = options(dir.path());
    part.quality = Quality::Audio;
    part.span = dm_core::Span::new(2.0, 4.0);
    let done = videopage::download_page(&dm_core::client(), &page("dlSoundPart"), &part, None)
        .await
        .unwrap();
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Talk [dlSoundPart] (0m02s-0m04s).m4a"
    );
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 1);
    assert_eq!(data.windows(4).filter(|w| *w == b"elst").count(), 1);
    assert!(
        media_asked("dlSoundPart")
            .iter()
            .all(|u| u.starts_with("/media/sound")),
        "no picture for the sound alone"
    );
}

/// An address that has run out (410, its `expire` passed) is read again
/// and the same streams fetched from the fresh addresses.
#[tokio::test(flavor = "multi_thread")]
async fn an_address_that_ran_out_is_read_again() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let done = videopage::download_page(
        &dm_core::client(),
        &page("renewed0001"),
        &options(dir.path()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(mp4_tracks(&done.path), 2);
    assert_eq!(stand_in().calls.lock().unwrap()["renewed0001"], 2);
    assert!(media_asked("renewed0001")
        .iter()
        .any(|u| u.contains("try=2")));
}

/// A refusal that is not an address running out, or a fresh reading that
/// is not the same streams or fails, leaves the refusal as the outcome.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_is_the_outcome_where_reading_again_cannot_help() {
    let _env = ENV.read().await;
    stand_in();
    for (id, readings) in [("gone0000001", 1), ("renewDiff01", 2), ("renewFail01", 2)] {
        let dir = tempdir();
        let outcome =
            videopage::download_page(&dm_core::client(), &page(id), &options(dir.path()), None)
                .await;
        assert!(
            matches!(outcome, Err(Error::Refused { status: 410, .. })),
            "{id}: {outcome:?}"
        );
        assert_eq!(stand_in().calls.lock().unwrap()[id], readings, "{id}");
    }
}

/// A 403 from a reading only seconds old is asked again once the site
/// serves it (a TV client's streams come a few seconds after their
/// addresses), without reading the page again.
#[tokio::test(flavor = "multi_thread")]
async fn a_young_readings_403_is_asked_again_after_a_wait() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let at = std::time::Instant::now();
    let done = videopage::download_page(
        &dm_core::client(),
        &page("retry403001"),
        &options(dir.path()),
        None,
    )
    .await
    .unwrap();
    assert!(at.elapsed() >= Duration::from_secs(5), "it waited");
    assert_eq!(mp4_tracks(&done.path), 2);
    assert_eq!(stand_in().calls.lock().unwrap()["retry403001"], 1);
}

/// Cancelled while a stream is fetched: the staging is removed. Paused:
/// it is kept, for the resume.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_removes_the_staging_and_a_pause_keeps_it() {
    let _env = ENV.read().await;
    stand_in();
    for (id, cancel) in [("cancelled01", true), ("paused00001", false)] {
        let dir = tempdir();
        let opts = options(dir.path());
        let control = opts.control.clone();
        let url = page(id);
        let run = tokio::spawn(async move {
            videopage::download_page(&dm_core::client(), &url, &opts, None).await
        });
        let wait = std::time::Instant::now();
        while media_asked(id).is_empty() {
            assert!(
                wait.elapsed() < Duration::from_secs(10),
                "{id}: never asked"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if cancel {
            control.cancel();
        } else {
            control.pause();
        }
        let outcome = tokio::time::timeout(Duration::from_secs(10), run)
            .await
            .expect("the run did not stop")
            .unwrap();
        if cancel {
            assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
            assert!(!dir.path().join("parts").exists());
        } else {
            assert!(matches!(outcome, Err(Error::Paused)), "{outcome:?}");
            assert!(dir.path().join("parts").exists());
        }
    }
}

/// What dm's own engine cannot fetch, a page that is not YouTube's, a
/// staging that cannot be made and a reading that fails are each said.
#[tokio::test(flavor = "multi_thread")]
async fn what_cannot_be_fetched_is_said() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let why = |r: Result<dm_core::Finished, Error>| r.unwrap_err().to_string();
    let client = dm_core::client();
    let opts = options(dir.path());
    assert!(
        why(videopage::download_page(&client, "https://example.com/watch", &opts, None).await)
            .contains("that is a web page")
    );
    assert!(
        why(videopage::download_page(&client, &page("ftpStreams1"), &opts, None).await)
            .contains("cannot be fetched by dm's own engine")
    );
    let mut part = options(dir.path());
    part.span = dm_core::Span::new(1.0, 2.0);
    assert!(
        why(videopage::download_page(&client, &page("ftpStreams1"), &part, None).await)
            .contains("a part of it cannot be cut")
    );
    assert!(
        why(videopage::download_page(&client, &page("notPlayabl1"), &opts, None).await)
            .contains("YouTube says: Video unavailable")
    );
    std::fs::write(dir.path().join("blocker"), b"x").unwrap();
    let mut blocked = options(dir.path());
    blocked.parts_dir = dir.path().join("blocker/parts");
    assert!(matches!(
        videopage::download_page(&client, &page("dlWhole0002"), &blocked, None).await,
        Err(Error::Disk(_))
    ));
}

/// A part of the picture and sound: each stream's pieces in the span, under
/// names of the span's own, joined and cut by edit lists.
#[tokio::test(flavor = "multi_thread")]
async fn a_part_of_picture_and_sound_is_joined_and_cut() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let mut part = options(dir.path());
    part.span = dm_core::Span::new(2.0, 4.0);
    let done = videopage::download_page(&dm_core::client(), &page("dlPart00001"), &part, None)
        .await
        .unwrap();
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Talk [dlPart00001] (0m02s-0m04s).mp4"
    );
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 2);
    assert_eq!(data.windows(4).filter(|w| *w == b"elst").count(), 2);
}

/// A run cancelled before it starts reads nothing at all.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_cancelled_before_it_starts_reads_nothing() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let opts = options(dir.path());
    opts.control.cancel();
    let outcome =
        videopage::download_page(&dm_core::client(), &page("dlNever0001"), &opts, None).await;
    assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
    assert!(asked_with("dlNever0001").is_empty());
}

/// Subtitles left beside the video in its staging go inside the joined
/// file, and the files beside it are removed.
#[tokio::test(flavor = "multi_thread")]
async fn subtitles_beside_the_streams_go_inside_the_joined_file() {
    let _env = ENV.read().await;
    stand_in();
    let dir = tempdir();
    let staging = dir.path().join("parts");
    std::fs::create_dir_all(&staging).unwrap();
    let vtt = staging.join("Talk [dlSubs00001].en.vtt");
    std::fs::write(&vtt, "WEBVTT\n\n00:00.000 --> 00:00.500\nhello there\n").unwrap();
    let done = videopage::download_page(
        &dm_core::client(),
        &page("dlSubs00001"),
        &options(dir.path()),
        None,
    )
    .await
    .unwrap();
    let data = std::fs::read(&done.path).unwrap();
    assert!(data.windows(4).any(|w| w == b"tx3g"), "a text track");
    assert!(data.windows(11).any(|w| w == b"hello there"));
    assert!(!vtt.exists());
}
