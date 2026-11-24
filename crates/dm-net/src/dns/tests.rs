// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

/// What a name answers in these tests instead of the system: each answer once, the last for good.
type Answers = HashMap<String, VecDeque<Vec<SocketAddr>>>;

/// The names these tests answer for ([`answers`]); no system's hosts file is touched.
static ANSWERS: LazyLock<Mutex<Answers>> = LazyLock::new(Default::default);

/// The next answer for `name`, where a test gave it one ([`look_up`]).
pub(super) fn answer(name: &str) -> Option<Vec<SocketAddr>> {
    let mut answers = crate::lock(&ANSWERS);
    let queue = answers.get_mut(name)?;
    if queue.len() > 1 {
        queue.pop_front()
    } else {
        queue.front().cloned()
    }
}

/// Has `name` answer each of `ips` in turn, then the last of them from then on.
fn answers(name: &str, ips: &[&[&str]]) {
    let queue: VecDeque<Vec<SocketAddr>> = ips
        .iter()
        .map(|ips| {
            ips.iter()
                .map(|ip| SocketAddr::new(ip.parse().unwrap(), 0))
                .collect()
        })
        .collect();
    crate::lock(&ANSWERS).insert(name.to_string(), queue);
}

/// Lets go of what was kept for `name`, as a minute's passing does ([`KEEP`]).
fn forget(name: &str) {
    crate::lock(&KEPT).remove(name);
}

async fn resolve(name: &str) -> Result<Vec<SocketAddr>, String> {
    Kept.resolve(name.parse().unwrap())
        .await
        .map(|addrs| addrs.collect())
        .map_err(|e| e.to_string())
}

/// Holds `name` as a hop a public page led to does (`redirects::inward`).
fn led_from_public(name: &str) {
    let page = url::Url::parse("https://cdn.example/index.m3u8").unwrap();
    let to = url::Url::parse(&format!("https://{name}/seg1.ts")).unwrap();
    assert!(!crate::redirects::inward(&to, &[page]));
}

/// A server on this computer: `/start` sends on to `?to=`'s address, `/file` answers "the file"
/// and is counted, anything else answers "via <the request's target>" (what a proxy is asked).
async fn server() -> (u16, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let files = Arc::new(AtomicUsize::new(0));
    let counted = files.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let files = counted.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if socket.read(&mut byte).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head);
                let target = head.split(' ').nth(1).unwrap_or("/").to_string();
                let answer = if let Some((_, to)) = target.split_once("/start?to=") {
                    format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                } else {
                    let body = if target.ends_with("/file") {
                        files.fetch_add(1, Ordering::SeqCst);
                        "the file".to_string()
                    } else {
                        format!("via {target}")
                    };
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = socket.write_all(answer.as_bytes()).await;
            });
        }
    });
    (port, files)
}

/// A client of dm's that finds `origin.test` (a public name, as far as dm knows) at this
/// computer's server, outside dm's resolver, so it is never found to lead inside.
fn client_with_origin(port: u16) -> reqwest::Client {
    crate::builder()
        .resolve("origin.test", SocketAddr::from(([127, 0, 0, 1], port)))
        .build()
        .unwrap()
}

/// A name is looked up once and kept; the kept addresses are what a second ask gets.
#[tokio::test]
async fn a_name_is_looked_up_once_and_kept() {
    let first = resolve("localhost").await.unwrap();
    assert!(!first.is_empty());
    assert_eq!(kept("localhost"), Some(first.clone()));
    assert_eq!(resolve("localhost").await.unwrap(), first);
}

/// A name a public address led to, which resolves inside alone, is refused at the
/// connection; one that also resolves outside is connected to outside alone; one never led to
/// goes where it resolves (gaps row 200).
#[tokio::test]
async fn a_public_name_that_leads_inside_is_refused() {
    let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let public: SocketAddr = "93.184.215.14:0".parse().unwrap();
    keep("rebinds.example", &[loopback]);
    keep("home-nas.example", &[loopback]);
    keep("both.example", &[loopback, public]);
    // Typed or handed over directly: a name of one's own network goes inside.
    assert!(resolve("home-nas.example").await.is_ok());
    led_from_public("rebinds.example");
    led_from_public("both.example");
    let why = resolve("rebinds.example").await.unwrap_err();
    assert!(why.contains("rebinds.example"), "{why}");
    // The connection is made to the address judged: the public one alone.
    assert_eq!(resolve("both.example").await.unwrap(), [public]);
    // An intranet's own name, found inside, leads on to another inside.
    keep("files.intranet.example", &[loopback]);
    keep("mirror.intranet.example", &[loopback]);
    let from = url::Url::parse("https://files.intranet.example/a").unwrap();
    let to = url::Url::parse("https://mirror.intranet.example/a").unwrap();
    assert!(!crate::redirects::inward(&to, &[from]));
    assert!(resolve("mirror.intranet.example").await.is_ok());
}

/// DNS rebinding: a name a public page led to answers a public address, then, once that
/// answer has run out, `127.0.0.1`. The second answer is judged where it is connected to and
/// refused; the first was connected to as it was judged.
#[tokio::test]
async fn a_name_that_answers_public_then_inside_is_refused_when_it_turns() {
    answers("turns.rebind.test", &[&["93.184.215.14"], &["127.0.0.1"]]);
    led_from_public("turns.rebind.test");
    let first = resolve("turns.rebind.test").await.unwrap();
    assert_eq!(first, ["93.184.215.14:0".parse::<SocketAddr>().unwrap()]);
    // Kept: asked again within the minute, the same public answer.
    assert_eq!(resolve("turns.rebind.test").await.unwrap(), first);
    forget("turns.rebind.test");
    let why = resolve("turns.rebind.test").await.unwrap_err();
    assert!(why.contains("turns.rebind.test"), "{why}");
    // Asked again: the inside answer is kept, and refused from it.
    assert!(resolve("turns.rebind.test").await.is_err());
}

/// A public address that redirects to a name resolving to `127.0.0.1` is refused in an inward
/// redirect's words, and the server inside is never asked; the same name typed directly, and a
/// redirect from it to another name inside, still reach it (gaps row 200).
#[tokio::test]
async fn a_redirect_to_a_name_that_resolves_inside_is_refused_and_typed_it_is_not() {
    let (port, files) = server().await;
    answers("led.inside.test", &[&["127.0.0.1"]]);
    let client = client_with_origin(port);
    let led = format!("http://led.inside.test:{port}/file");
    let refused =
        crate::redirects::send(client.get(format!("http://origin.test:{port}/start?to={led}")))
            .await
            .unwrap_err();
    assert!(matches!(refused, crate::Error::Internal(_)), "{refused:?}");
    let said = refused.to_string();
    assert!(said.starts_with(crate::INWARD_REDIRECT), "{said}");
    assert!(said.contains("led.inside.test"), "{said}");
    assert_eq!(
        files.load(Ordering::SeqCst),
        0,
        "the server inside was asked"
    );

    // A name of one's own network, typed: reached, and it may send dm on to another.
    answers("nas.by-choice.test", &[&["127.0.0.1"]]);
    answers("mirror.by-choice.test", &[&["127.0.0.1"]]);
    let typed =
        crate::redirects::send(client.get(format!("http://nas.by-choice.test:{port}/file")))
            .await
            .unwrap();
    assert_eq!(typed.text().await.unwrap(), "the file");
    let on = format!("http://mirror.by-choice.test:{port}/file");
    let sent_on = crate::redirects::send(
        client.get(format!("http://nas.by-choice.test:{port}/start?to={on}")),
    )
    .await
    .unwrap();
    assert_eq!(sent_on.text().await.unwrap(), "the file");
    // An address inside, typed, as before.
    let literal = crate::redirects::send(client.get(format!("http://127.0.0.1:{port}/file")))
        .await
        .unwrap();
    assert_eq!(literal.text().await.unwrap(), "the file");
    assert_eq!(files.load(Ordering::SeqCst), 3);
}

/// The same refusal said where it reaches a person through reqwest's error, not as a name that
/// could not be found.
#[tokio::test]
async fn the_refusal_is_said_as_an_inward_redirect() {
    answers("said.inside.test", &[&["127.0.0.1"]]);
    led_from_public("said.inside.test");
    let err = crate::builder()
        .build()
        .unwrap()
        .get("http://said.inside.test:9/f")
        .send()
        .await
        .unwrap_err();
    assert!(refused_inside(&err));
    let said = crate::Error::Network(err).to_string();
    assert!(said.starts_with(crate::INWARD_REDIRECT), "{said}");
}

/// Under a proxy that resolves names itself (HTTP, `socks5h`), the name never reaches dm's
/// resolver: an inside address as written is still refused, a name is the proxy's to find. A
/// `socks5` proxy is handed the address dm's resolver gave, so a name led inside is refused
/// before the proxy is asked.
#[tokio::test]
async fn under_a_proxy_the_rule_judges_what_dm_resolves() {
    let (proxy, files) = server().await;
    let client = crate::builder()
        .proxy(reqwest::Proxy::http(format!("http://127.0.0.1:{proxy}")).unwrap())
        .build()
        .unwrap();
    // Written inside: refused, as without a proxy.
    let refused = crate::redirects::send(
        client.get("http://public.proxied.test/start?to=http://127.0.0.1:9/file"),
    )
    .await
    .unwrap_err();
    assert!(refused.to_string().starts_with(crate::INWARD_REDIRECT));
    // A name: handed to the proxy as written, whatever dm would find it at.
    answers("led.proxied.test", &[&["127.0.0.1"]]);
    let through = crate::redirects::send(
        client.get("http://public.proxied.test/start?to=http://led.proxied.test/file"),
    )
    .await
    .unwrap();
    assert_eq!(through.text().await.unwrap(), "the file");
    assert_eq!(files.load(Ordering::SeqCst), 1);

    // socks5 (not socks5h): dm resolves, and judges, before the proxy is reached. Nothing
    // listens at the proxy's port, so getting further would be a refused connection instead.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let socks = closed.local_addr().unwrap().port();
    drop(closed);
    answers("led.socks.test", &[&["127.0.0.1"]]);
    led_from_public("led.socks.test");
    let client = crate::builder()
        .proxy(reqwest::Proxy::all(format!("socks5://127.0.0.1:{socks}")).unwrap())
        .build()
        .unwrap();
    let refused = crate::redirects::send(client.get("http://led.socks.test/file"))
        .await
        .unwrap_err();
    assert!(
        refused.to_string().starts_with(crate::INWARD_REDIRECT),
        "{refused}"
    );
}

/// Past [`MOST_HELD`] names a new one is not held, and is refused rather than let through
/// unheld; a name already held is held again, and room comes back as names run out.
#[test]
fn a_name_that_cannot_be_held_is_not_let_through() {
    let mut held = HashMap::new();
    for n in 0..MOST_HELD {
        assert!(hold(&mut held, format!("n{n}.example")));
    }
    assert!(!hold(&mut held, "one-more.example".into()));
    assert!(hold(&mut held, "n7.example".into()));
    let run_out = Instant::now().checked_sub(HELD_PUBLIC + Duration::from_secs(1));
    held.insert("n3.example".into(), run_out.unwrap());
    assert!(hold(&mut held, "one-more.example".into()));
}

/// The sockets dm opens itself (FTP, RTMP, RTSP) look a name up as the HTTP clients do: one a
/// public page led to that resolves inside is refused, the port given set on each address, an
/// address written as one taken as it is (gaps row H38).
#[tokio::test]
async fn a_socket_of_dms_own_judges_the_name_as_a_client_does() {
    let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
    keep("own-socket-inside.example", &[loopback]);
    keep("own-socket-typed.example", &[loopback]);
    led_from_public("own-socket-inside.example");
    let refused = super::addresses("own-socket-inside.example", 1935)
        .await
        .unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(
        super::addresses("own-socket-typed.example", 1935)
            .await
            .unwrap(),
        ["127.0.0.1:1935".parse::<SocketAddr>().unwrap()]
    );
    assert_eq!(
        super::addresses("[::1]", 554).await.unwrap(),
        ["[::1]:554".parse::<SocketAddr>().unwrap()]
    );
}
