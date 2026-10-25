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
