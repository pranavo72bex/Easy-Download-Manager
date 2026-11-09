// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where an FTP address points and how it is reached, through a proxy or not.

use super::*;

/// Where an FTP URL points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Target {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) user: String,
    pub(super) password: String,
    /// The file's path as the server is asked for it, decoded.
    pub(super) path: String,
    /// The proxy both connections go through, where one is set.
    pub(super) route: Option<Route>,
    /// FTP over TLS, for an `ftps://` address.
    pub(super) tls: Option<Tls>,
    pub(super) trust: Trust,
}

/// A proxy, as the settings hand it to tools: its scheme (`socks5h://`,
/// `http://` …), the user and password, `@`, then host and port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Route {
    pub(super) kind: crate::proxy::Kind,
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) user: String,
    pub(super) password: String,
}

impl Route {
    /// Reads the proxy address, or refuses one FTP cannot go through: an
    /// HTTPS proxy needs TLS to the proxy, which this client has not got,
    /// and going around a configured proxy is the one thing it must never do.
    pub(super) fn parse(proxy: Option<&str>) -> Result<Option<Route>, Error> {
        let Some(proxy) = proxy else {
            return Ok(None);
        };
        let parsed = url::Url::parse(proxy)
            .map_err(|e| Error::Internal(format!("the proxy address is not usable: {e}")))?;
        let kind = match parsed.scheme() {
            "http" => crate::proxy::Kind::Http,
            "socks4" | "socks4a" => crate::proxy::Kind::Socks4,
            "socks5" | "socks5h" => crate::proxy::Kind::Socks5,
            "https" => {
                return Err(Error::Internal(
                    "dm cannot send FTP through an HTTPS proxy, and does not go around it — \
                     use an HTTP or SOCKS proxy, or put this site on the proxy's bypass list"
                        .into(),
                ))
            }
            other => {
                return Err(Error::Internal(format!(
                    "dm cannot send FTP through a {other} proxy, and does not go around it"
                )))
            }
        };
        let host = parsed
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| Error::Internal("the proxy address names no host".into()))?
            .trim_matches(['[', ']'])
            .to_string();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| Error::Internal("the proxy address names no port".into()))?;
        Ok(Some(Route {
            kind,
            host,
            port,
            user: crate::probe::decode(parsed.username()),
            password: crate::probe::decode(parsed.password().unwrap_or_default()),
        }))
    }
}

/// A connection to `host:port`: direct, or through the proxy, after which
/// it is a plain stream either way — the SOCKS handshake and the HTTP
/// `CONNECT` both end with the bytes going straight through.
pub(super) async fn connect(
    route: Option<&Route>,
    host: &str,
    port: u16,
) -> Result<TcpStream, Error> {
    let Some(route) = route else {
        return timed(TcpStream::connect((host, port))).await;
    };
    let refused =
        |e: &dyn std::fmt::Display| dropped(format!("the proxy refused the connection: {e}"));
    let proxy = (route.host.as_str(), route.port);
    let target = (host, port);
    match route.kind {
        crate::proxy::Kind::Socks5 => {
            use tokio_socks::tcp::Socks5Stream;
            let handshake = async {
                if route.user.is_empty() {
                    Socks5Stream::connect(proxy, target).await
                } else {
                    Socks5Stream::connect_with_password(proxy, target, &route.user, &route.password)
                        .await
                }
            };
            tokio::time::timeout(TIMEOUT, handshake)
                .await
                .map_err(|_| dropped("the proxy stopped answering"))?
                .map(Socks5Stream::into_inner)
                .map_err(|e| refused(&e))
        }
        crate::proxy::Kind::Socks4 => {
            use tokio_socks::tcp::Socks4Stream;
            let handshake = async {
                if route.user.is_empty() {
                    Socks4Stream::connect(proxy, target).await
                } else {
                    Socks4Stream::connect_with_userid(proxy, target, &route.user).await
                }
            };
            tokio::time::timeout(TIMEOUT, handshake)
                .await
                .map_err(|_| dropped("the proxy stopped answering"))?
                .map(Socks4Stream::into_inner)
                .map_err(|e| refused(&e))
        }
        crate::proxy::Kind::Http => {
            let mut stream = timed(TcpStream::connect(proxy)).await?;
            let mut request = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
            if !route.user.is_empty() {
                use base64::Engine as _;
                let pair = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", route.user, route.password));
                request.push_str(&format!("Proxy-Authorization: Basic {pair}\r\n"));
            }
            request.push_str("\r\n");
            timed(stream.write_all(request.as_bytes())).await?;
            // The proxy's answer, up to its blank line and no further: the
            // bytes after it are the server's.
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if head.len() >= 8 * 1024 {
                    return Err(Error::Internal("the proxy's answer ran on".into()));
                }
                if timed(stream.read(&mut byte)).await? == 0 {
                    return Err(dropped("the proxy closed the connection"));
                }
                head.extend_from_slice(&byte);
            }
            let status = String::from_utf8_lossy(&head);
            let status = status.lines().next().unwrap_or_default();
            let ok = status
                .split_whitespace()
                .nth(1)
                .is_some_and(|code| code.starts_with('2'));
            if !ok {
                return Err(refused(&status));
            }
            Ok(stream)
        }
        crate::proxy::Kind::Https | crate::proxy::Kind::System => Err(Error::Internal(
            "dm cannot send FTP through this kind of proxy".into(),
        )),
    }
}

impl Target {
    pub(super) fn parse(url: &str) -> Result<Target, Error> {
        Self::parse_as(url, false)
    }

    /// A folder's address, for listing it: a trailing `/` or none at all.
    pub(super) fn parse_folder(url: &str) -> Result<Target, Error> {
        Self::parse_as(url, true)
    }

    pub(super) fn parse_as(url: &str, folder: bool) -> Result<Target, Error> {
        let parsed = url::Url::parse(url)
            .map_err(|e| Error::Internal(format!("that is not an FTP address: {e}")))?;
        let host = parsed
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| Error::Internal("that FTP address names no server".into()))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let path = crate::probe::decode(parsed.path());
        if !folder && (path.ends_with('/') || path.is_empty()) {
            return Err(Error::Internal(
                "that FTP address is a folder, not a file".into(),
            ));
        }
        /* Relative to the login directory, as RFC 1738 reads an FTP
         * address: `ftp://host/pub/f` is `pub/f` from wherever the login
         * lands, and the root is reached by `%2F` (`ftp://host/%2Fpub/f`).
         * Sent whole with its leading slash, a file in a user's home folder
         * was asked for at the root and was not there. */
        let path = path.strip_prefix('/').unwrap_or(&path).to_string();
        // A line break in a path would be a second command to the server.
        let plain = |s: &str| !s.contains(['\r', '\n', '\0']);
        let user = match parsed.username() {
            "" => "anonymous".to_string(),
            name => crate::probe::decode(name),
        };
        let password = match parsed.password() {
            Some(p) => crate::probe::decode(p),
            None => "anonymous@".to_string(),
        };
        if !(plain(&path) && plain(&user) && plain(&password)) {
            return Err(Error::Internal(
                "that FTP address has a line break in it".into(),
            ));
        }
        let secure = parsed.scheme().eq_ignore_ascii_case("ftps");
        let port = parsed.port().unwrap_or(if secure { 990 } else { 21 });
        Ok(Target {
            host,
            port,
            user,
            password,
            path,
            route: None,
            tls: secure.then_some(if port == 990 {
                Tls::Implicit
            } else {
                Tls::Explicit
            }),
            trust: Trust::default(),
        })
    }
}

#[cfg(test)]
mod tests {
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
}
