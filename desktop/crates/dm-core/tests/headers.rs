// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download a site guards with a request header (a bearer token), made
//! with the headers the browser's own request carried (gaps row 116).

mod common;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use dm_core::client;

use common::{options, serve, tempdir};

fn token_only() -> axum::Router {
    axum::Router::new().route(
        "/guarded.bin",
        get(|headers: HeaderMap| async move {
            let ok = headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer t")
                && headers.get("x-play-session").is_some();
            if !ok {
                return Response::builder()
                    .status(403)
                    .body(Body::from("no token"))
                    .unwrap();
            }
            Response::builder()
                .status(200)
                .header("content-length", 7)
                .body(Body::from("guarded"))
                .unwrap()
        }),
    )
}

#[tokio::test]
async fn the_browsers_request_headers_are_sent_on_every_request_of_the_row() {
    let addr = serve(token_only()).await;
    let dir = tempdir();
    let url = format!("http://{addr}/guarded.bin");
    let q = dm_core::DownloadList::new(client(), options(dir.path()), 1);

    // Without them: refused, as it is to anything that is not the browser.
    let bare = q.add(&url).await.unwrap();
    q.run().await;
    assert!(matches!(
        q.get(bare).await.unwrap().state,
        dm_core::State::Failed(_)
    ));

    // With them, as a hand-off arrives; the ones dm sets itself are dropped.
    let id = q
        .add_with(
            &format!("{url}?again"),
            dm_core::AddOptions {
                headers: vec![
                    ("Authorization".into(), "Bearer t".into()),
                    ("X-Play-Session".into(), "s1".into()),
                    ("Range".into(), "bytes=5-".into()),
                    ("Host".into(), "evil.example".into()),
                ],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(std::fs::read(&row.path).unwrap(), b"guarded");
    assert_eq!(row.headers.len(), 2, "{:?}", row.headers);
}

#[test]
fn headers_go_to_their_own_server_only() {
    let sign_in = dm_core::cookies::SignIn::with_headers(
        "https://files.example/a.bin",
        "",
        &[("Authorization".into(), "Bearer t".into())],
    )
    .unwrap();
    assert_eq!(sign_in.headers_for("https://files.example/b.bin").len(), 1);
    assert!(sign_in
        .headers_for("https://mirror.example/b.bin")
        .is_empty());
    assert_eq!(sign_in.for_url("https://files.example/b.bin"), None);
}

/// Every request of a ranged download carries the browser's headers, on several connections and
/// the check made of each finished range's last bytes among them: a server that guards the file
/// with one never sees a request without it.
#[tokio::test]
async fn every_request_of_a_ranged_download_carries_the_browsers_headers() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let data: Arc<Vec<u8>> = Arc::new((0..4_000_000u32).map(|i| (i * 31 % 251) as u8).collect());
    let (bare, guarded) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (d, b, g) = (data.clone(), bare.clone(), guarded.clone());
    let app = axum::Router::new().route(
        "/big.bin",
        get(move |headers: HeaderMap| {
            let (d, b, g) = (d.clone(), b.clone(), g.clone());
            async move {
                if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer t") {
                    b.fetch_add(1, Ordering::SeqCst);
                    return Response::builder()
                        .status(403)
                        .body(Body::from("no token"))
                        .unwrap();
                }
                g.fetch_add(1, Ordering::SeqCst);
                let total = d.len();
                let range = headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| {
                        let (a, z) = v.split_once('-')?;
                        let a: usize = a.parse().ok()?;
                        let z: usize = if z.is_empty() {
                            total - 1
                        } else {
                            z.parse().ok()?
                        };
                        Some((a, z.min(total - 1)))
                    });
                match range {
                    Some((a, z)) => Response::builder()
                        .status(206)
                        .header("etag", "\"v1\"")
                        .header("accept-ranges", "bytes")
                        .header("content-range", format!("bytes {a}-{z}/{total}"))
                        .header("content-length", z - a + 1)
                        .body(Body::from(d[a..=z].to_vec()))
                        .unwrap(),
                    None => Response::builder()
                        .status(200)
                        .header("etag", "\"v1\"")
                        .header("accept-ranges", "bytes")
                        .header("content-length", total)
                        .body(Body::from(d.to_vec()))
                        .unwrap(),
                }
            }
        }),
    );
    let addr = serve(app).await;
    let dir = tempdir();
    let q = dm_core::DownloadList::new(
        client(),
        dm_core::Options {
            connections: 4,
            ..options(dir.path())
        },
        1,
    );
    let id = q
        .add_with(
            &format!("http://{addr}/big.bin"),
            dm_core::AddOptions {
                headers: vec![("Authorization".into(), "Bearer t".into())],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(std::fs::read(&row.path).unwrap(), *data);
    assert!(guarded.load(Ordering::SeqCst) > 1, "not fetched in ranges");
    assert_eq!(
        bare.load(Ordering::SeqCst),
        0,
        "a request went without the browser's headers"
    );
}

/// A 2 MB file served in ranges only to `agent` with `X-Api-Key: k1`, and a count of the
/// requests turned away.
fn agent_only(
    agent: &'static str,
    data: std::sync::Arc<Vec<u8>>,
    refused: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) -> axum::Router {
    axum::Router::new().route(
        "/own.bin",
        get(move |headers: HeaderMap| {
            let (d, refused) = (data.clone(), refused.clone());
            async move {
                let said = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
                if said("user-agent") != Some(agent) || said("x-api-key") != Some("k1") {
                    refused.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    return Response::builder()
                        .status(403)
                        .body(Body::from("not you"))
                        .unwrap();
                }
                let total = d.len();
                let (a, z) = said("range")
                    .and_then(|v| v.strip_prefix("bytes="))
                    .and_then(|v| {
                        let (a, z) = v.split_once('-')?;
                        let a: usize = a.parse().ok()?;
                        let z = z.parse().unwrap_or(total - 1).min(total - 1);
                        Some((a, z))
                    })
                    .unwrap_or((0, total - 1));
                Response::builder()
                    .status(206)
                    .header("etag", "\"v1\"")
                    .header("accept-ranges", "bytes")
                    .header("content-range", format!("bytes {a}-{z}/{total}"))
                    .header("content-length", z - a + 1)
                    .body(Body::from(d[a..=z].to_vec()))
                    .unwrap()
            }
        }),
    )
}

/// gaps row F1: a `User-Agent` and a header of the person's own (Properties, or a site's rule)
/// go on every request of the download, on every connection, in place of the names dm would
/// otherwise try; and one set on a row after it failed is used when it is tried again.
#[tokio::test]
async fn a_user_agent_and_headers_of_the_persons_own_go_on_every_request() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    const AGENT: &str = "MyAgent/1.0 (own)";
    let data: Arc<Vec<u8>> = Arc::new((0..2_000_000u32).map(|i| (i * 7 % 253) as u8).collect());
    let refused = Arc::new(AtomicUsize::new(0));
    let addr = serve(agent_only(AGENT, data.clone(), refused.clone())).await;
    let dir = tempdir();
    let q = dm_core::DownloadList::new(
        client(),
        dm_core::Options {
            connections: 4,
            ..options(dir.path())
        },
        1,
    );
    let own = vec![
        ("X-Api-Key".to_string(), "k1".to_string()),
        ("User-Agent".to_string(), AGENT.to_string()),
    ];
    let id = q
        .add_with(
            &format!("http://{addr}/own.bin"),
            dm_core::AddOptions {
                headers: own.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(std::fs::read(&row.path).unwrap(), *data);
    assert_eq!(row.headers, own, "the person's agent is kept with the row");
    assert_eq!(
        refused.load(Ordering::SeqCst),
        0,
        "a request went out without the person's agent or header"
    );

    // Without them it fails; given them afterwards (Properties' OK), it is fetched.
    let later = q
        .add(&format!("http://{addr}/own.bin?later"))
        .await
        .unwrap();
    q.run().await;
    assert!(matches!(
        q.get(later).await.unwrap().state,
        dm_core::State::Failed(_)
    ));
    assert!(q.set_headers(later, own.clone()).await);
    assert!(q.retry(later).await);
    q.run().await;
    let row = q.get(later).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed, "{row:?}");
    assert_eq!(std::fs::read(&row.path).unwrap(), *data);
}

#[test]
fn headers_typed_on_one_line_are_read_back_as_written() {
    use dm_core::cookies::{headers_as_text, headers_from_text, kept_headers};
    let typed = "Authorization: Bearer a:b | Accept: text/html;q=0.9, */*\nX-Empty:| no colon | bad name: x";
    let read = headers_from_text(typed);
    assert_eq!(
        read,
        vec![
            ("Authorization".to_string(), "Bearer a:b".to_string()),
            ("Accept".to_string(), "text/html;q=0.9, */*".to_string()),
            ("X-Empty".to_string(), String::new()),
        ]
    );
    assert_eq!(headers_from_text(&headers_as_text(&read)), read);
    // dm's own headers are still not taken from anyone; the agent alone is the person's.
    let kept = kept_headers(&[
        ("Range".into(), "bytes=1-".into()),
        ("user-agent".into(), " Mine/2 ".into()),
        ("X-A".into(), "1".into()),
    ]);
    assert_eq!(
        kept,
        vec![
            ("X-A".to_string(), "1".to_string()),
            ("User-Agent".to_string(), "Mine/2".to_string())
        ]
    );
    let sign_in = dm_core::cookies::SignIn::with_headers("https://a.example/f", "", &kept).unwrap();
    assert_eq!(sign_in.agent(), Some("Mine/2"));
    assert_eq!(
        dm_core::cookies::agent_with(Some(&sign_in), "https://mirror.example/f"),
        "Mine/2",
        "a name is no secret: it goes to a mirror too"
    );
    assert_eq!(sign_in.headers_for("https://mirror.example/f").len(), 0);
}
