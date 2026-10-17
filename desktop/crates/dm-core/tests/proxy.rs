//! HTTP downloads and the proxy settings: a site on the bypass list goes
//! out directly, and "the computer's proxy settings" means the environment's.
//!
//! One test at a time: the system proxy is read from environment variables,
//! which are process-wide.

mod common;

use std::sync::{Arc, Mutex};

/// Held across the awaits on purpose (tokio's, so clippy allows it): the
/// environment is process-wide.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

use axum::body::Body;
use axum::response::Response;
use axum::routing::get;
use dm_core::proxy::{client_with, Kind, Proxy};
use dm_core::{download, Options};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use common::{options, serve, tempdir};

/// An HTTP proxy that answers every request itself, so a download that went
/// through it is told apart from one that did not by its bytes.
async fn answering_proxy() -> (String, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Arc::clone(&asked);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let asked = Arc::clone(&shared);
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                asked.lock().unwrap().push(line.trim().to_string());
                loop {
                    let mut l = String::new();
                    reader.read_line(&mut l).await.unwrap();
                    if l == "\r\n" || l.is_empty() {
                        break;
                    }
                }
                let body = b"via the proxy";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                write.write_all(head.as_bytes()).await.unwrap();
                write.write_all(body).await.unwrap();
            });
        }
    });
    (format!("127.0.0.1:{port}"), asked)
}

async fn direct() -> Response {
    Response::builder()
        .status(200)
        .body(Body::from("direct from the server"))
        .unwrap()
}

#[tokio::test]
async fn a_site_on_the_bypass_list_is_fetched_directly_and_the_rest_through_the_proxy() {
    let _one = ONE_AT_A_TIME.lock().await;
    let server = serve(axum::Router::new().route("/f.txt", get(direct))).await;
    let (proxy, asked) = answering_proxy().await;
    let (host, port) = proxy.split_once(':').unwrap();
    let settings = Proxy {
        enabled: true,
        kind: Kind::Http,
        host: host.into(),
        port: port.parse().unwrap(),
        bypass: vec!["localhost".into(), "intranet.example".into()],
        ..Default::default()
    };
    let client = client_with(&settings).unwrap();

    // Not on the list: the proxy answers.
    let dir = tempdir();
    let done = download(
        &client,
        &format!("http://{server}/f.txt"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"via the proxy");
    assert!(asked.lock().unwrap().iter().any(|l| l.contains("/f.txt")));

    // On the list, by name: the server itself answers, and the proxy is not
    // asked. `localhost` resolves to the same loopback server.
    let before = asked.lock().unwrap().len();
    let dir = tempdir();
    let done = download(
        &client,
        &format!("http://localhost:{}/f.txt", server.port()),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        b"direct from the server"
    );
    assert_eq!(asked.lock().unwrap().len(), before, "the proxy was asked");
}

#[tokio::test]
async fn the_computers_proxy_settings_are_the_environments() {
    let _one = ONE_AT_A_TIME.lock().await;
    let server = serve(axum::Router::new().route("/f.txt", get(direct))).await;
    let (proxy, asked) = answering_proxy().await;
    // SAFETY: the one test in this binary that touches the environment, and
    // the tests here run one at a time.
    unsafe {
        std::env::set_var("HTTP_PROXY", format!("http://{proxy}"));
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
    }
    let system = Proxy {
        enabled: true,
        kind: Kind::System,
        ..Default::default()
    };
    assert_eq!(system.url(), None, "no address of dm's own");
    assert_eq!(
        system.url_for_tools().as_deref(),
        Some(format!("http://{proxy}").as_str()),
        "the tools are told what the environment says"
    );
    let client = client_with(&system).unwrap();
    let dir = tempdir();
    let done = download(
        &client,
        &format!("http://{server}/f.txt"),
        &options(dir.path()),
    )
    .await
    .unwrap();
    unsafe {
        std::env::remove_var("HTTP_PROXY");
    }
    assert_eq!(std::fs::read(&done.path).unwrap(), b"via the proxy");
    assert!(!asked.lock().unwrap().is_empty());

    // The options' view: a bypassed site is not handed to the tools either.
    let o = Options {
        proxy: Some("socks5://p:1080".into()),
        proxy_bypass: vec![".example.org".into()],
        ..options(dir.path())
    };
    assert_eq!(o.proxy_for("https://a.example.org/x"), None);
    assert_eq!(
        o.proxy_for("https://example.com/x"),
        Some("socks5://p:1080")
    );
}
