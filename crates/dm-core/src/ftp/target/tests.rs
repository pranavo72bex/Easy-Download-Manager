// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_proxy_ftp_can_go_through_is_read_and_any_other_refused() {
    assert_eq!(Route::parse(None).unwrap(), None);
    let login = ["u", "p%40ss"].join(":");
    let http = Route::parse(Some(&format!("http://{login}@[::1]:8080")))
        .unwrap()
        .unwrap();
    assert_eq!(
        (http.kind, http.host.as_str(), http.port),
        (crate::proxy::Kind::Http, "::1", 8080)
    );
    assert_eq!((http.user.as_str(), http.password.as_str()), ("u", "p@ss"));
    let socks4 = Route::parse(Some("socks4a://p.example:1080"))
        .unwrap()
        .unwrap();
    assert_eq!(
        (socks4.kind, socks4.port),
        (crate::proxy::Kind::Socks4, 1080)
    );
    let why = Route::parse(Some("https://p.example"))
        .unwrap_err()
        .to_string();
    assert!(why.contains("HTTPS proxy"), "{why}");
    let why = Route::parse(Some("ftp://p.example"))
        .unwrap_err()
        .to_string();
    assert!(why.contains("a ftp proxy"), "{why}");
    assert!(Route::parse(Some("not an address")).is_err());
    let why = Route::parse(Some("socks5://p.example"))
        .unwrap_err()
        .to_string();
    assert!(why.contains("names no port"), "{why}");
}

#[test]
fn an_ftp_address_is_read_as_a_file_from_the_login_folder() {
    let t = Target::parse("ftps://files.example:2121/pub/a%20b.iso").unwrap();
    assert_eq!(
        (t.host.as_str(), t.port, t.path.as_str()),
        ("files.example", 2121, "pub/a b.iso")
    );
    assert_eq!(
        (t.user.as_str(), t.password.as_str()),
        ("anonymous", "anonymous@")
    );
    assert_eq!(t.tls, Some(Tls::Explicit));
    assert_eq!(
        Target::parse("ftps://h/f").unwrap().tls,
        Some(Tls::Implicit)
    );
    assert_eq!(Target::parse("ftp://h/f").unwrap().port, 21);
    for bad in ["ftp://h/pub/", "ftp://h", "ftp://h/a%0Db", "nonsense"] {
        assert!(Target::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(Target::parse_folder("ftp://h/pub/").unwrap().path, "pub/");
}

/// A proxy on 127.0.0.1 that reads one request up to its blank line, keeps
/// it, and answers `reply`: then echoes what follows, or hangs up where the answer is
/// not whole.
async fn http_proxy(reply: &'static [u8]) -> (u16, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut asked = Vec::new();
        let mut byte = [0u8; 1];
        while !asked.ends_with(b"\r\n\r\n") {
            if socket.read(&mut byte).await.unwrap() == 0 {
                break;
            }
            asked.extend_from_slice(&byte);
        }
        let _ = socket.write_all(reply).await;
        if !reply.ends_with(b"\r\n\r\n") {
            // A proxy that hangs up mid-answer.
            return String::from_utf8_lossy(&asked).into_owned();
        }
        let mut rest = [0u8; 16];
        if let Ok(n) = socket.read(&mut rest).await {
            let _ = socket.write_all(&rest[..n]).await;
        }
        String::from_utf8_lossy(&asked).into_owned()
    });
    (port, task)
}

fn route(kind: crate::proxy::Kind, port: u16, user: &str) -> Route {
    Route {
        kind,
        host: "127.0.0.1".into(),
        port,
        user: user.into(),
        password: if user.is_empty() {
            String::new()
        } else {
            "pw".into()
        },
    }
}

#[tokio::test]
async fn an_http_proxy_is_asked_to_connect_with_its_sign_in_and_then_carries_the_bytes() {
    let (port, asked) = http_proxy(b"HTTP/1.1 200 Connection established\r\n\r\n").await;
    let mut stream = connect(
        Some(&route(crate::proxy::Kind::Http, port, "me")),
        "ftp.example",
        21,
    )
    .await
    .unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut back = [0u8; 4];
    stream.read_exact(&mut back).await.unwrap();
    assert_eq!(&back, b"ping");
    let asked = asked.await.unwrap();
    use base64::Engine as _;
    let pair = base64::engine::general_purpose::STANDARD.encode("me:pw");
    assert!(
        asked.starts_with("CONNECT ftp.example:21 HTTP/1.1\r\n"),
        "{asked}"
    );
    assert!(
        asked.contains(&format!("Proxy-Authorization: Basic {pair}")),
        "{asked}"
    );
}

#[tokio::test]
async fn an_http_proxy_that_refuses_hangs_up_or_runs_on_is_not_gone_around() {
    let kind = crate::proxy::Kind::Http;
    let (port, _) = http_proxy(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
    let why = connect(Some(&route(kind, port, "")), "h", 21)
        .await
        .unwrap_err()
        .to_string();
    assert!(why.contains("refused") && why.contains("403"), "{why}");
    let (port, _) = http_proxy(b"HTTP/1.1 200 OK\r\n").await;
    let why = connect(Some(&route(kind, port, "")), "h", 21)
        .await
        .unwrap_err()
        .to_string();
    assert!(why.contains("closed the connection"), "{why}");
    let endless: &'static [u8] = Box::leak(vec![b'x'; 9000].into_boxed_slice());
    let (port, _) = http_proxy(endless).await;
    let why = connect(Some(&route(kind, port, "")), "h", 21)
        .await
        .unwrap_err()
        .to_string();
    assert!(why.contains("ran on"), "{why}");
    for kind in [crate::proxy::Kind::Https, crate::proxy::Kind::System] {
        assert!(connect(Some(&route(kind, 1, "")), "h", 21).await.is_err());
    }
}

#[tokio::test]
async fn a_socks4_proxy_is_asked_for_the_server_with_or_without_a_user() {
    for user in ["", "me"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = [0u8; 8];
            socket.read_exact(&mut head).await.unwrap();
            let mut id = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                socket.read_exact(&mut byte).await.unwrap();
                if byte[0] == 0 {
                    break;
                }
                id.push(byte[0]);
            }
            socket
                .write_all(&[0, 0x5A, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
            (head, String::from_utf8(id).unwrap())
        });
        connect(
            Some(&route(crate::proxy::Kind::Socks4, port, user)),
            "127.0.0.1",
            2121,
        )
        .await
        .unwrap();
        let (head, id) = served.await.unwrap();
        assert_eq!(head[..4], [4, 1, 0x08, 0x49], "version, connect, port 2121");
        assert_eq!(head[4..], [127, 0, 0, 1]);
        assert_eq!(id, user);
    }
    // A proxy that is not there is said to have refused.
    let gone = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let why = connect(
        Some(&route(crate::proxy::Kind::Socks4, gone, "")),
        "127.0.0.1",
        21,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(why.contains("proxy refused"), "{why}");
}
