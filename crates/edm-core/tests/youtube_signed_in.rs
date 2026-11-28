// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A video that wants a sign-in, read by EDM's own YouTube reader (`edm_core::youtube`) against a
//! stand-in for YouTube whose player answers "sign in" to a request without the browser's
//! session and plays for one that carries its cookies and the `SAPISIDHASH` YouTube's web
//! clients make from them (gaps row Y4). Without a session the refusal is today's, in
//! YouTube's words; with one the video is read and fetched, and nothing of the session is
//! left in any file EDM wrote.
//!
//! One test in this binary: the stand-in is named by environment variables, which are the
//! process's.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use edm_core::{client, videopage};
use serde_json::{json, Value};
use sha1::Digest;

use common::{options, tempdir};

/// The session's own values, looked for in every file EDM wrote.
const SAPISID: &str = "sapisid-y4-held-in-memory";
const SID: &str = "sid-y4-never-on-disk";

/// One ask of the player: the client named, and the session it carried.
#[derive(Clone, Debug, PartialEq)]
struct Asked {
    id: String,
    client: String,
    cookie: String,
    authorization: String,
    origin: String,
    x_origin: String,
}

fn text(status: StatusCode, kind: &str, body: String) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", kind)
        .body(Body::from(body))
        .unwrap()
}

/// Whether `authorization` is the `SAPISIDHASH` of the session's `SAPISID` for YouTube's web
/// origin, at the time it names.
fn signed_rightly(authorization: &str) -> bool {
    let Some((time, hex)) = authorization
        .strip_prefix("SAPISIDHASH ")
        .and_then(|rest| rest.split_once('_'))
    else {
        return false;
    };
    let want = sha1::Sha1::digest(format!("{time} {SAPISID} https://www.youtube.com").as_bytes());
    let want: String = want.iter().map(|b| format!("{b:02x}")).collect();
    hex == want && time.parse::<u64>().is_ok()
}

/// Every file under `dir`, read whole.
fn files_under(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push((path, bytes));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_video_that_wants_a_sign_in_is_read_with_the_browsers_session() {
    let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let video = Arc::new(
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mp4/picture.f137.mp4"),
        )
        .unwrap(),
    );

    let player_api = {
        let (asked, origin, size) = (asked.clone(), origin.clone(), video.len());
        move |headers: HeaderMap, body: String| {
            let (asked, origin) = (asked.clone(), origin.clone());
            async move {
                let header = |name: &str| {
                    headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string()
                };
                let body: Value = serde_json::from_str(&body).unwrap_or_default();
                let id = body["videoId"].as_str().unwrap_or("").to_string();
                let client = body
                    .pointer("/context/client/clientName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let version = body
                    .pointer("/context/client/clientVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let ask = Asked {
                    id: id.clone(),
                    client: client.clone(),
                    cookie: header("cookie"),
                    authorization: header("authorization"),
                    origin: header("origin"),
                    x_origin: header("x-origin"),
                };
                asked.lock().unwrap().push(ask.clone());
                let signed_in = ask.cookie.contains(&format!("SAPISID={SAPISID}"))
                    && ask.cookie.contains(&format!("SID={SID}"))
                    && signed_rightly(&ask.authorization)
                    && ask.origin == "https://www.youtube.com"
                    && ask.x_origin == "https://www.youtube.com";
                // The second video plays for the web client alone, at the page's version.
                let web_only = id == "webOnly0001";
                let plays =
                    signed_in && (!web_only || (client == "WEB" && version == "2.20261101.00.00"));
                let answer = if plays {
                    json!({
                        "playabilityStatus": {"status": "OK"},
                        "videoDetails": {"videoId": id, "title": format!("Signed {id}"), "lengthSeconds": "1"},
                        "streamingData": {"formats": [
                            {"itag": 18, "mimeType": "video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"",
                             "width": 640, "height": 360, "bitrate": 500000,
                             "contentLength": size.to_string(),
                             "url": format!("{origin}/media/{id}")}
                        ]}
                    })
                } else if signed_in {
                    json!({"playabilityStatus": {"status": "UNPLAYABLE", "reason": "not for this client"}})
                } else {
                    json!({"playabilityStatus": {
                        "status": "LOGIN_REQUIRED",
                        "reason": "Sign in to confirm your age"
                    }})
                };
                text(StatusCode::OK, "application/json", answer.to_string())
            }
        }
    };

    let app = axum::Router::new()
        .route(
            "/watch",
            get(|| async {
                text(
                    StatusCode::OK,
                    "text/html",
                    r#"<html><script>ytcfg.set({"STS":12345,"VISITOR_DATA":"visitor-1","INNERTUBE_CLIENT_VERSION":"2.20261101.00.00"});</script></html>"#.into(),
                )
            }),
        )
        .route("/youtubei/v1/player", post(player_api))
        .fallback(get({
            let video = video.clone();
            move |headers: HeaderMap| {
                let video = video.clone();
                async move {
                    let size = video.len();
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
                        .header("content-type", "video/mp4")
                        .header("etag", "\"v\"")
                        .body(Body::from(bytes::Bytes::copy_from_slice(&video[a..=b])))
                        .unwrap()
                }
            }
        }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempdir();
    // EDM's data folder, and the stand-in.
    std::env::set_var("EDM_HOME", dir.path().join("home"));
    std::env::set_var("EDM_YOUTUBE_HOST", &origin);
    let session = format!("SID={SID}; HSID=h; SAPISID={SAPISID}; __Secure-3PAPISID={SAPISID}");

    // Without a session: today's refusal in YouTube's words, and no ask signed in.
    let page = "https://www.youtube.com/watch?v=ageGated001";
    let why = videopage::download_page(&client(), page, &options(dir.path()), None)
        .await
        .expect_err("a video that wants a sign-in was read without one");
    assert!(
        why.to_string()
            .contains("YouTube says: Sign in to confirm your age"),
        "{why}"
    );
    let seen = std::mem::take(&mut *asked.lock().unwrap());
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].client, "TVHTML5");
    assert!(seen[0].cookie.is_empty() && seen[0].authorization.is_empty());

    // A session with nothing to sign with: the same refusal, asked once.
    let mut unsigned = options(dir.path());
    unsigned.sign_in = edm_core::cookies::SignIn::new(&origin, "PREF=f6=1; VISITOR=v");
    let why = videopage::download_page(&client(), page, &unsigned, None)
        .await
        .expect_err("a session with no SAPISID read the video");
    assert!(
        why.to_string().contains("Sign in to confirm your age"),
        "{why}"
    );
    assert_eq!(std::mem::take(&mut *asked.lock().unwrap()).len(), 1);

    // With the browser's session: refused unsigned, then read as the TV client signed in.
    let mut signed = options(dir.path());
    signed.sign_in = edm_core::cookies::SignIn::new(&origin, &session);
    let done = videopage::download_page(&client(), page, &signed, None)
        .await
        .expect("the video was not read with the session");
    assert_eq!(
        done.path.file_name().unwrap().to_string_lossy(),
        "Signed ageGated001 [ageGated001].mp4"
    );
    let fetched = std::fs::read(&done.path).unwrap();
    assert!(
        fetched.windows(4).any(|w| w == b"moov"),
        "the stream was not fetched"
    );
    let seen = std::mem::take(&mut *asked.lock().unwrap());
    let shape: Vec<(&str, bool)> = seen
        .iter()
        .map(|a| (a.client.as_str(), signed_rightly(&a.authorization)))
        .collect();
    assert_eq!(shape, [("TVHTML5", false), ("TVHTML5", true)], "{seen:?}");
    assert!(
        !seen[0].cookie.contains(SAPISID),
        "the first ask carried the session"
    );

    // One the TV client is refused even signed in: the web client, at the page's version.
    let page = "https://www.youtube.com/watch?v=webOnly0001";
    videopage::download_page(&client(), page, &signed, None)
        .await
        .expect("the video was not read as the web client with the session");
    let seen = std::mem::take(&mut *asked.lock().unwrap());
    let shape: Vec<(&str, bool)> = seen
        .iter()
        .map(|a| (a.client.as_str(), signed_rightly(&a.authorization)))
        .collect();
    assert_eq!(
        shape,
        [("TVHTML5", false), ("TVHTML5", true), ("WEB", true)],
        "{seen:?}"
    );

    // Nothing of the session in any file EDM wrote: the downloads, their parts, its data folder.
    let mut written = Vec::new();
    files_under(dir.path(), &mut written);
    assert!(!written.is_empty());
    for (path, bytes) in &written {
        for secret in [SAPISID, SID] {
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "{} holds the session",
                path.display()
            );
        }
    }
}
