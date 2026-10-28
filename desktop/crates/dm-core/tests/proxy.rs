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
    // SAFETY: as at the `set_var` above: the one test in this binary that
    // touches the environment.
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

/// A SOCKS5 proxy that knows one name the machine's resolver does not, and
/// records how each connection named its destination: by name (ATYP 3) or
/// by an address resolved before it arrived (1 or 4).
async fn socks5_knowing(
    name: &'static str,
    at: std::net::SocketAddr,
) -> (u16, Arc<Mutex<Vec<(u8, String)>>>) {
    use tokio::io::AsyncReadExt;
    let asked = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Arc::clone(&asked);
    tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = listener.accept().await else {
                return;
            };
            let asked = Arc::clone(&shared);
            tokio::spawn(async move {
                // Greeting: version, method count, methods; no login asked.
                let mut head = [0u8; 2];
                client.read_exact(&mut head).await.unwrap();
                let mut methods = vec![0u8; head[1] as usize];
                client.read_exact(&mut methods).await.unwrap();
                client.write_all(&[5, 0]).await.unwrap();
                // CONNECT: version, command, reserved, address type.
                let mut request = [0u8; 4];
                client.read_exact(&mut request).await.unwrap();
                let atyp = request[3];
                let host = match atyp {
                    3 => {
                        let mut len = [0u8; 1];
                        client.read_exact(&mut len).await.unwrap();
                        let mut name = vec![0u8; len[0] as usize];
                        client.read_exact(&mut name).await.unwrap();
                        String::from_utf8(name).unwrap()
                    }
                    1 => {
                        let mut ip = [0u8; 4];
                        client.read_exact(&mut ip).await.unwrap();
                        std::net::Ipv4Addr::from(ip).to_string()
                    }
                    _ => {
                        let mut ip = [0u8; 16];
                        client.read_exact(&mut ip).await.unwrap();
                        std::net::Ipv6Addr::from(ip).to_string()
                    }
                };
                let mut port = [0u8; 2];
                client.read_exact(&mut port).await.unwrap();
                asked.lock().unwrap().push((atyp, host.clone()));
                if host != name {
                    // Host unreachable.
                    let _ = client.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]).await;
                    return;
                }
                let mut origin = tokio::net::TcpStream::connect(at).await.unwrap();
                client
                    .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut client, &mut origin).await;
            });
        }
    });
    (port, asked)
}

/// Through a SOCKS5 proxy, by a name only the proxy can resolve. reqwest
/// was built without SOCKS, so every download failed with "unsupported
/// scheme"; and resolved locally, a name like this one cannot be reached
/// at all, and every site visited is told to the local resolver.
#[tokio::test]
async fn a_socks5_proxy_carries_the_download_and_resolves_its_name() {
    let _one = ONE_AT_A_TIME.lock().await;
    let server = serve(axum::Router::new().route("/f.txt", get(direct))).await;
    let (port, asked) = socks5_knowing("files.only-the-proxy-knows.test", server).await;
    let settings = Proxy {
        enabled: true,
        kind: Kind::Socks5,
        host: "127.0.0.1".into(),
        port,
        ..Default::default()
    };
    let client = client_with(&settings).unwrap();
    let dir = tempdir();
    let done = download(
        &client,
        &format!(
            "http://files.only-the-proxy-knows.test:{}/f.txt",
            server.port()
        ),
        &options(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        b"direct from the server"
    );
    let asked = asked.lock().unwrap().clone();
    assert!(!asked.is_empty(), "the proxy was not used");
    assert!(
        asked.iter().all(|(atyp, _)| *atyp == 3),
        "a name was resolved before it reached the proxy: {asked:?}"
    );
}
