//! Servers that want a sign-in: a Digest challenge answered, a wrong
//! password refused, and a scheme dm does not speak said rather than hung
//! on. Each stands up a loopback server that checks the sign-in the way a
//! real one would, from the RFC's arithmetic written out here a second time.

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download};
use sha2::Digest as _;

use common::{options, serve, tempdir};

fn body(n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(29).wrapping_add(7))
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Hash {
    Md5,
    Sha256,
}

impl Hash {
    fn of(self, text: &str) -> String {
        match self {
            Hash::Md5 => hex::encode(md5::Md5::digest(text.as_bytes())),
            Hash::Sha256 => hex::encode(sha2::Sha256::digest(text.as_bytes())),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Hash::Md5 => "MD5",
            Hash::Sha256 => "SHA-256",
        }
    }
}

/// A file server behind a Digest sign-in.
struct Gate {
    data: Vec<u8>,
    user: &'static str,
    password: &'static str,
    hash: Hash,
    /// The nonce in force. Replaced after the `rotate`th authorized request
    /// where that is set, with the old one answered `stale=true` from then.
    nonce: Mutex<String>,
    rotate: Option<usize>,
    authorized: AtomicUsize,
    unauthorized: AtomicUsize,
    requests: AtomicUsize,
}

fn params(header: &str) -> HashMap<String, String> {
    // dm's own header has no commas inside its values here, so a split is
    // enough for a test; the module's parser is held to the hard cases in
    // its unit tests.
    header
        .trim_start_matches("Digest ")
        .split(", ")
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect()
}

async fn gate(State(g): State<Arc<Gate>>, headers: HeaderMap) -> Response {
    g.requests.fetch_add(1, Ordering::SeqCst);
    let nonce = g.nonce.lock().unwrap().clone();
    let challenge = |stale: bool| {
        g.unauthorized.fetch_add(1, Ordering::SeqCst);
        Response::builder()
            .status(401)
            .header(
                "www-authenticate",
                format!(
                    "Digest realm=\"dm test\", qop=\"auth\", algorithm={}, nonce=\"{nonce}\", opaque=\"op\"{}",
                    g.hash.name(),
                    if stale { ", stale=true" } else { "" }
                ),
            )
            .body(Body::empty())
            .unwrap()
    };
    let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return challenge(false);
    };
    if !auth.starts_with("Digest ") {
        return challenge(false);
    }
    let p = params(auth);
    let get = |k: &str| p.get(k).cloned().unwrap_or_default();
    if get("nonce") != nonce {
        return challenge(true);
    }
    assert_eq!(get("realm"), "dm test");
    assert_eq!(get("opaque"), "op");
    assert_eq!(get("qop"), "auth");
    assert_eq!(get("algorithm"), g.hash.name());
    let a1 = g
        .hash
        .of(&format!("{}:dm test:{}", get("username"), g.password));
    let a2 = g.hash.of(&format!("GET:{}", get("uri")));
    let expected = g.hash.of(&format!(
        "{a1}:{nonce}:{}:{}:auth:{a2}",
        get("nc"),
        get("cnonce")
    ));
    if get("username") != g.user || get("response") != expected || get("uri") != "/file.bin" {
        return challenge(false);
    }
    let n = g.authorized.fetch_add(1, Ordering::SeqCst) + 1;
    if g.rotate == Some(n) {
        *g.nonce.lock().unwrap() = format!("nonce-{n}");
    }

    let total = g.data.len();
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .map(|v| {
            let (a, b) = v.split_once('-').unwrap();
            let a: usize = a.parse().unwrap();
            let b = b.parse::<usize>().unwrap_or(total - 1).min(total - 1);
            (a, b)
        });
    match range {
        Some((a, b)) => Response::builder()
            .status(206)
            .header("content-range", format!("bytes {a}-{b}/{total}"))
            .header("etag", "\"v1\"")
            .body(Body::from(g.data[a..=b].to_vec()))
            .unwrap(),
        None => Response::builder()
            .status(200)
            .header("etag", "\"v1\"")
            .header("content-length", total)
            .body(Body::from(g.data.clone()))
            .unwrap(),
    }
}

async fn stand_up(gate_: Gate) -> (Arc<Gate>, std::net::SocketAddr) {
    let g = Arc::new(gate_);
    let app = axum::Router::new()
        .route("/file.bin", get(gate))
        .route(
            "/get",
            get(|| async {
                Response::builder()
                    .status(302)
                    .header("location", "/file.bin")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .with_state(Arc::clone(&g));
    (g, serve(app).await)
}

fn behind(hash: Hash, rotate: Option<usize>) -> Gate {
    Gate {
        data: body(2 * 1024 * 1024 + 123),
        user: "me",
        password: "open sesame",
        hash,
        nonce: Mutex::new("nonce-0".into()),
        rotate,
        authorized: AtomicUsize::new(0),
        unauthorized: AtomicUsize::new(0),
        requests: AtomicUsize::new(0),
    }
}

#[tokio::test]
async fn a_server_that_wants_a_digest_login_gets_one_and_the_file_downloads() {
    let (g, addr) = stand_up(behind(Hash::Md5, None)).await;
    let dir = tempdir();
    let url = format!("http://{addr}/file.bin");
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the download behind a Digest sign-in");
    assert_eq!(std::fs::read(&done.path).unwrap(), g.data, "bytes differ");
    // One challenge, at the probe; every range after it signed in from the
    // start rather than being asked again.
    assert_eq!(
        g.unauthorized.load(Ordering::SeqCst),
        1,
        "ranges were challenged one by one"
    );
    // The probe and each of the two 1 MiB ranges the planner makes of it.
    assert!(
        g.authorized.load(Ordering::SeqCst) >= 3,
        "{} authorized",
        g.authorized.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn a_sha_256_digest_and_a_nonce_that_expires_are_answered() {
    // The nonce the probe signed in with expires right after it, so every
    // range's remembered Digest is answered `stale=true`: taken as a fresh
    // challenge, not a refusal.
    let (g, addr) = stand_up(behind(Hash::Sha256, Some(1))).await;
    let dir = tempdir();
    let url = format!("http://{addr}/file.bin");
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the download across rotated nonces");
    assert_eq!(std::fs::read(&done.path).unwrap(), g.data, "bytes differ");
    assert!(
        g.unauthorized.load(Ordering::SeqCst) >= 2,
        "no nonce was ever stale"
    );
}

#[tokio::test]
async fn a_wrong_password_is_refused_with_the_status_and_not_retried() {
    let (g, addr) = stand_up(behind(Hash::Md5, None)).await;
    let dir = tempdir();
    let url = format!("http://{addr}/file.bin");
    let outcome = download(&client(), &url, &options(dir.path())).await;
    match outcome {
        Err(dm_core::Error::Refused { status: 401, .. }) => {}
        other => panic!("expected a 401, got {other:?}"),
    }
    // Challenged, answered wrongly, challenged again: and then told, not
    // tried a sixth time.
    assert!(
        g.requests.load(Ordering::SeqCst) <= 3,
        "{} requests",
        g.requests.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn a_sign_in_dm_does_not_speak_is_said_rather_than_tried() {
    async fn negotiate() -> Response {
        Response::builder()
            .status(401)
            .header("www-authenticate", "Negotiate")
            .header("www-authenticate", "NTLM")
            .body(Body::empty())
            .unwrap()
    }
    let addr = serve(axum::Router::new().route("/file.bin", get(negotiate))).await;
    let dir = tempdir();
    let url = format!("http://{addr}/file.bin");
    let outcome = download(&client(), &url, &options(dir.path())).await;
    match outcome {
        Err(dm_core::Error::Refused { status: 401, .. }) => {}
        other => panic!("expected a 401, got {other:?}"),
    }
}

#[tokio::test]
async fn a_login_in_the_address_still_goes_out_as_basic_where_that_is_taken() {
    // The ordinary case must not have changed: a server content with Basic
    // sees Basic, and no Digest round trip.
    async fn basic(headers: HeaderMap) -> Response {
        let ok = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == "Basic bWU6cA==");
        Response::builder()
            .status(if ok { 200 } else { 401 })
            .header("www-authenticate", "Basic realm=\"x\"")
            .body(Body::from(if ok {
                "hello".to_string()
            } else {
                String::new()
            }))
            .unwrap()
    }
    let addr = serve(axum::Router::new().route("/file.bin", get(basic))).await;
    let dir = tempdir();
    let url = format!("http://{addr}/file.bin");
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"hello");
}

#[tokio::test]
async fn a_digest_protected_file_behind_a_redirect_on_the_same_server_downloads() {
    let (g, addr) = stand_up(behind(Hash::Md5, None)).await;
    let dir = tempdir();
    let url = format!("http://{addr}/get");
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .expect("the redirected download behind a Digest sign-in");
    assert_eq!(std::fs::read(&done.path).unwrap(), g.data, "bytes differ");
}
