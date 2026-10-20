//! A server that signs connections in with NTLM: each connection is given
//! its own challenge, an answer is taken only on the connection its
//! challenge went out on, and a connection signed in serves the file. dm
//! downloads it over several connections, each signing itself in.

mod common;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use base64::Engine as _;
use dm_core::{client, download, ntlm};

use common::{options, tempdir};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// A connection's challenge: the server's eight bytes and its target info.
type Given = ([u8; 8], Vec<u8>);

struct Gate {
    data: Vec<u8>,
    user: String,
    domain: String,
    password: String,
    /// The word the server asks under: NTLM, or Negotiate.
    scheme: &'static str,
    /// The challenge each connection was given.
    challenges: Mutex<HashMap<SocketAddr, Given>>,
    /// The connections signed in.
    signed_in: Mutex<HashSet<SocketAddr>>,
    answered: AtomicUsize,
    refused: AtomicUsize,
}

fn u16_at(m: &[u8], at: usize) -> usize {
    u16::from_le_bytes([m[at], m[at + 1]]) as usize
}

fn field(m: &[u8], at: usize) -> &[u8] {
    let len = u16_at(m, at);
    let offset = u32::from_le_bytes([m[at + 4], m[at + 5], m[at + 6], m[at + 7]]) as usize;
    &m[offset..offset + len]
}

fn text(utf16: &[u8]) -> String {
    let units: Vec<u16> = utf16
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// A challenge message: the server's challenge and a target info naming
/// its domain.
fn challenge_message(server_challenge: [u8; 8], target_info: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(b"NTLMSSP\0");
    m.extend_from_slice(&2u32.to_le_bytes());
    m.extend_from_slice(&[0, 0, 0, 0, 56, 0, 0, 0]);
    // Unicode, NTLM, extended session security, target info.
    m.extend_from_slice(&0x0088_8205u32.to_le_bytes());
    m.extend_from_slice(&server_challenge);
    m.extend_from_slice(&[0; 8]);
    let len = target_info.len() as u16;
    m.extend_from_slice(&len.to_le_bytes());
    m.extend_from_slice(&len.to_le_bytes());
    m.extend_from_slice(&56u32.to_le_bytes());
    m.extend_from_slice(&[0; 8]);
    m.extend_from_slice(target_info);
    m
}

fn refuse(g: &Gate, token: Option<Vec<u8>>) -> Response {
    let value = match token {
        Some(t) => format!("{} {}", g.scheme, B64.encode(t)),
        None => g.scheme.to_string(),
    };
    Response::builder()
        .status(401)
        .header("www-authenticate", value)
        .body(Body::from("sign in"))
        .unwrap()
}

async fn gate(
    State(g): State<Arc<Gate>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix(&format!("{} ", g.scheme)))
        .and_then(|t| B64.decode(t).ok());
    if !g.signed_in.lock().unwrap().contains(&peer) {
        let Some(message) = token else {
            g.refused.fetch_add(1, Ordering::SeqCst);
            return refuse(&g, None);
        };
        match message.get(8) {
            // Negotiate: this connection's challenge.
            Some(1) => {
                let server_challenge: [u8; 8] = rand::random();
                let mut info = Vec::new();
                let domain: Vec<u8> = g.domain.encode_utf16().flat_map(u16::to_le_bytes).collect();
                info.extend_from_slice(&2u16.to_le_bytes());
                info.extend_from_slice(&(domain.len() as u16).to_le_bytes());
                info.extend_from_slice(&domain);
                info.extend_from_slice(&[0, 0, 0, 0]);
                g.challenges
                    .lock()
                    .unwrap()
                    .insert(peer, (server_challenge, info.clone()));
                return refuse(&g, Some(challenge_message(server_challenge, &info)));
            }
            // The answer, checked against this connection's challenge only.
            Some(3) => {
                let Some((server_challenge, info)) = g.challenges.lock().unwrap().remove(&peer)
                else {
                    g.refused.fetch_add(1, Ordering::SeqCst);
                    return refuse(&g, None);
                };
                let nt = field(&message, 20);
                let (user, domain) = (text(field(&message, 36)), text(field(&message, 28)));
                // Its NTLMv2 blob: version, reserved, time, client challenge.
                let time: [u8; 8] = nt[24..32].try_into().unwrap();
                let client_challenge: [u8; 8] = nt[32..40].try_into().unwrap();
                let key = ntlm::ntowf_v2(&g.user, &g.domain, &g.password);
                let (_, expected) =
                    ntlm::responses(&key, &server_challenge, &client_challenge, &time, &info);
                if user != g.user || domain != g.domain || nt != expected.as_slice() {
                    g.refused.fetch_add(1, Ordering::SeqCst);
                    return refuse(&g, None);
                }
                g.answered.fetch_add(1, Ordering::SeqCst);
                g.signed_in.lock().unwrap().insert(peer);
            }
            _ => return refuse(&g, None),
        }
    }
    // Signed in: the file, or the range asked for.
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .map(|(a, b)| {
            let start: usize = a.parse().unwrap();
            let end = b.parse().unwrap_or(g.data.len() - 1).min(g.data.len() - 1);
            (start, end)
        });
    match range {
        Some((start, end)) => Response::builder()
            .status(206)
            .header("accept-ranges", "bytes")
            .header(
                "content-range",
                format!("bytes {start}-{end}/{}", g.data.len()),
            )
            .body(Body::from(g.data[start..=end].to_vec()))
            .unwrap(),
        None => Response::builder()
            .header("accept-ranges", "bytes")
            .body(Body::from(g.data.clone()))
            .unwrap(),
    }
}

async fn stand_up(scheme: &'static str) -> (Arc<Gate>, SocketAddr) {
    let g = Arc::new(Gate {
        data: (0..3 * 1024 * 1024 + 77).map(|i| (i % 251) as u8).collect(),
        user: "someone".into(),
        domain: "CORP".into(),
        password: common::secrets::secret("password"),
        scheme,
        challenges: Mutex::default(),
        signed_in: Mutex::default(),
        answered: AtomicUsize::new(0),
        refused: AtomicUsize::new(0),
    });
    let app = axum::Router::new()
        .route("/file.bin", get(gate))
        .with_state(Arc::clone(&g));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    (g, addr)
}

async fn fetch_through(scheme: &'static str) {
    let (g, addr) = stand_up(scheme).await;
    let dir = tempdir();
    let url = common::with_login(
        &format!("http://{addr}/file.bin"),
        r"CORP\someone",
        &common::secrets::secret("password"),
    );
    let done = download(&client(), &url, &options(dir.path()))
        .await
        .unwrap_or_else(|e| panic!("the download behind an {scheme} sign-in: {e}"));
    assert_eq!(std::fs::read(&done.path).unwrap(), g.data, "bytes differ");
    assert!(
        g.answered.load(Ordering::SeqCst) >= 1,
        "no connection signed in"
    );
}

#[tokio::test]
async fn a_server_that_signs_connections_in_with_ntlm_serves_the_file() {
    fetch_through("NTLM").await;
}

#[tokio::test]
async fn a_server_that_asks_for_negotiate_takes_ntlm_under_it() {
    fetch_through("Negotiate").await;
}

#[tokio::test]
async fn a_wrong_ntlm_password_is_refused_with_the_status() {
    let (g, addr) = stand_up("NTLM").await;
    let dir = tempdir();
    let url = common::with_login(
        &format!("http://{addr}/file.bin"),
        r"CORP\someone",
        &common::secrets::secret("wrong"),
    );
    match download(&client(), &url, &options(dir.path())).await {
        Err(dm_core::Error::Refused { status: 401, .. }) => {}
        other => panic!("expected a 401, got {other:?}"),
    }
    assert_eq!(g.answered.load(Ordering::SeqCst), 0);
}
