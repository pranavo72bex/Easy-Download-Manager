//! FTP, taken over from a link as HTTP is.
//!
//! A client of dm's own rather than a library: what a download needs of FTP
//! is small — sign in, ask the size and the date, open a passive data
//! connection, start it at an offset — and every reply it reads comes from
//! outside, so it is parsed here, where it is fuzzed with the rest.
//!
//! Each range is its own session: `REST` sets where `RETR` starts, and a
//! range that ends before the file does is cut off by closing both
//! connections, which is how FTP stops a transfer early. The part files and
//! the plan beside them work as they do for HTTP, so a pause resumes and a
//! resume keeps the plan it started with.
//!
//! `ftps://` is FTP over TLS: on port 990, as such an address means unless
//! it names another, the connection is TLS from its first byte; on any
//! other port dm asks for it (`AUTH TLS`) before signing in. The data
//! connections are TLS too (`PROT P`), each begun once the server has
//! answered the command it carries, which is when a server starts its end.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::fs;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt, BufReader,
    ReadHalf, WriteHalf,
};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::ClientConfig;

use crate::download::{Finished, Options, Update};
use crate::plan::{self, Segment};
use crate::{category, retry, Error};

/// Whether this is an FTP address dm fetches itself: `ftp://`, or
/// `ftps://` for FTP over TLS.
pub fn is_ftp(url: &str) -> bool {
    url.get(..6)
        .is_some_and(|s| s.eq_ignore_ascii_case("ftp://"))
        || url
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("ftps://"))
}

/// A connection, plain or TLS.
trait Io: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Io for T {}

type Conn = Box<dyn Io>;

/// How an `ftps://` address is spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tls {
    /// TLS from the first byte, on port 990.
    Implicit,
    /// Asked for with `AUTH TLS` before signing in.
    Explicit,
}

/// The TLS settings a session is spoken with: the system's roots, or in a
/// test, a certificate of the test's own. Never compared: two targets are
/// the same address whatever trusts them.
#[derive(Clone, Default)]
struct Trust(Option<Arc<ClientConfig>>);

impl std::fmt::Debug for Trust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Trust")
    }
}

impl PartialEq for Trust {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Trust {}

/// A TLS data connection that a server closes without saying so first,
/// as many do: its end, not a failure. Whether the file is whole is
/// judged by its size and the server's own reply, as over plain FTP.
struct Lenient(Conn);

impl AsyncRead for Lenient {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match std::pin::Pin::new(&mut self.0).poll_read(cx, buf) {
            std::task::Poll::Ready(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                std::task::Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl AsyncWrite for Lenient {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// How long a server may stay silent before the attempt is given up and
/// retried, as the HTTP client's read timeout does.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The longest reply dm reads. A server that sends more is not answering.
const MAX_REPLY: usize = 64 * 1024;

/// Where an FTP URL points.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    host: String,
    port: u16,
    user: String,
    password: String,
    /// The file's path as the server is asked for it, decoded.
    path: String,
    /// The proxy both connections go through, where one is set.
    route: Option<Route>,
    /// FTP over TLS, for an `ftps://` address.
    tls: Option<Tls>,
    trust: Trust,
}

/// A proxy, as the settings hand it to tools: `socks5://`, the user and password, `@`, then host and port.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Route {
    kind: crate::proxy::Kind,
    host: String,
    port: u16,
    user: String,
    password: String,
}

impl Route {
    /// Reads the proxy address, or refuses one FTP cannot go through: an
    /// HTTPS proxy needs TLS to the proxy, which this client has not got,
    /// and going around a configured proxy is the one thing it must never do.
    fn parse(proxy: Option<&str>) -> Result<Option<Route>, Error> {
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
async fn connect(route: Option<&Route>, host: &str, port: u16) -> Result<TcpStream, Error> {
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
    fn parse(url: &str) -> Result<Target, Error> {
        Self::parse_as(url, false)
    }

    /// A folder's address, for listing it: a trailing `/` or none at all.
    fn parse_folder(url: &str) -> Result<Target, Error> {
        Self::parse_as(url, true)
    }

    fn parse_as(url: &str, folder: bool) -> Result<Target, Error> {
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

/// A reply's code, and whether this line ends it (`"230 "`) or more lines
/// follow (`"230-"`). `None` for a line that is neither.
fn reply_code(line: &str) -> Option<(u16, bool)> {
    let code = line.get(..3)?.parse::<u16>().ok()?;
    if !(100..600).contains(&code) {
        return None;
    }
    match line.as_bytes().get(3) {
        Some(b' ') | None => Some((code, true)),
        Some(b'-') => Some((code, false)),
        _ => None,
    }
}

/// The port in an `EPSV` reply: `229 Entering Extended Passive Mode (|||6446|)`.
fn epsv_port(reply: &str) -> Option<u16> {
    let open = reply.find('(')?;
    let inner = reply.get(open + 1..)?;
    let close = inner.find(')')?;
    let inner = inner.get(..close)?;
    let delimiter = inner.chars().next()?;
    let mut fields = inner.split(delimiter);
    // "", "", "", port, ""
    fields.nth(3)?.parse().ok()
}

/// The port in a `PASV` reply: `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`.
/// The address in it is not used: the data connection goes to the server
/// the control connection reached, as curl does, which is right behind a
/// NAT that reports its inside address and never somewhere else entirely.
fn pasv_port(reply: &str) -> Option<u16> {
    // After the parenthesis where there is one, else after the code.
    let from = reply.find('(').map_or(4, |p| p + 1);
    let numbers: Vec<u16> = reply
        .get(from..)?
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(6)
        .map(|s| s.parse::<u16>().ok())
        .collect::<Option<_>>()?;
    let (&high, &low) = (numbers.get(4)?, numbers.get(5)?);
    if high > 255 || low > 255 {
        return None;
    }
    Some(high * 256 + low)
}

/// A number after the code: `213 1048576`.
fn number_in(reply: &str) -> Option<u64> {
    reply.get(4..)?.trim().parse().ok()
}

/// A dropped connection, a timeout or a busy server: worth another try.
fn dropped(e: impl std::fmt::Display) -> Error {
    Error::Connection(e.to_string())
}

async fn timed<T>(work: impl std::future::Future<Output = std::io::Result<T>>) -> Result<T, Error> {
    tokio::time::timeout(TIMEOUT, work)
        .await
        .map_err(|_| dropped("the FTP server stopped answering"))?
        .map_err(dropped)
}

/// Runs FTP's back and forth, stopping as soon as a pause or a cancel is
/// asked for rather than when the server next answers.
async fn or_stop<T>(
    control: &crate::control::Control,
    work: impl std::future::Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            done = &mut work => return done,
            _ = tokio::time::sleep(Duration::from_millis(250)) => control.check()?,
        }
    }
}

/// The error a reply that is not the one hoped for comes to.
fn refusal(code: u16, text: &str, path: &str) -> Error {
    let said = text.get(4..).unwrap_or("").trim();
    match code {
        // "Try again later": too many users, a data connection that failed.
        400..=499 => Error::Connection(format!("the FTP server said {code} {said}")),
        530 => Error::Internal(format!(
            "the FTP server refused the sign-in ({said}) — put the name and password in the address: ftp:// followed by name:password@ and the server"
        )),
        550 => Error::Internal(format!("the FTP server has no file {path} ({said})")),
        _ => Error::Internal(format!("the FTP server answered {code} {said}")),
    }
}

/// One signed-in control connection.
struct Session {
    reader: BufReader<ReadHalf<Conn>>,
    writer: WriteHalf<Conn>,
    /// Where the data connection goes: the address the control connection
    /// reached, or, through a proxy (whose address is all this end sees),
    /// the server's name for the proxy to resolve.
    data_host: String,
    route: Option<Route>,
    /// The TLS the data connections are spoken in, and the server's name,
    /// for an `ftps://` address: one config, so each data connection can
    /// take up the control connection's TLS session, as servers ask.
    tls: Option<(tokio_rustls::TlsConnector, ServerName<'static>)>,
}

impl Session {
    async fn open(target: &Target) -> Result<Session, Error> {
        let tls = match target.tls {
            None => None,
            Some(_) => {
                let config = match &target.trust.0 {
                    Some(config) => Arc::clone(config),
                    None => crate::rtmp::tls_config()?,
                };
                let name = ServerName::try_from(target.host.clone()).map_err(|e| {
                    Error::Internal(format!("{} is not a name TLS can check: {e}", target.host))
                })?;
                Some((tokio_rustls::TlsConnector::from(config), name))
            }
        };
        let stream = connect(target.route.as_ref(), &target.host, target.port).await?;
        // The data connection goes to the address reached, not to the name
        // looked up again: a server answering its own DNS could otherwise
        // point the second lookup at this computer or its network.
        let data_host = if target.route.is_some() {
            target.host.clone()
        } else {
            stream.peer_addr().map_err(dropped)?.ip().to_string()
        };
        let stream: Conn = match (&tls, target.tls) {
            (Some((connector, name)), Some(Tls::Implicit)) => {
                Box::new(handshake(connector, name, Box::new(stream)).await?)
            }
            _ => Box::new(stream),
        };
        let (read, writer) = tokio::io::split(stream);
        let mut session = Session {
            reader: BufReader::new(read),
            writer,
            data_host,
            route: target.route.clone(),
            tls: tls.clone(),
        };
        let (code, text) = session.reply().await?;
        if code != 220 {
            return Err(refusal(code, &text, &target.path));
        }
        // Explicit TLS: asked for, and the same connection carried on in it.
        if let (Some((connector, name)), Some(Tls::Explicit)) = (&tls, target.tls) {
            let (code, text) = session.command("AUTH TLS").await?;
            if code != 234 {
                return Err(Error::Internal(format!(
                    "the FTP server would not speak TLS ({})",
                    text.trim()
                )));
            }
            let Session {
                reader,
                writer,
                data_host,
                route,
                tls,
            } = session;
            let plain = reader.into_inner().unsplit(writer);
            let secure: Conn = Box::new(handshake(connector, name, plain).await?);
            let (read, writer) = tokio::io::split(secure);
            session = Session {
                reader: BufReader::new(read),
                writer,
                data_host,
                route,
                tls,
            };
        }
        let (code, text) = session.command(&format!("USER {}", target.user)).await?;
        let (code, text) = if code == 331 {
            session
                .command(&format!("PASS {}", target.password))
                .await?
        } else {
            (code, text)
        };
        if code != 230 && code != 202 {
            return Err(refusal(code, &text, &target.path));
        }
        // The data connections in TLS too, never in the clear.
        if session.tls.is_some() {
            for command in ["PBSZ 0", "PROT P"] {
                let (code, text) = session.command(command).await?;
                if code != 200 {
                    return Err(Error::Internal(format!(
                        "the FTP server would not keep the data in TLS ({})",
                        text.trim()
                    )));
                }
            }
        }
        let (code, text) = session.command("TYPE I").await?;
        if code != 200 {
            return Err(refusal(code, &text, &target.path));
        }
        Ok(session)
    }

    /// Reads one reply, however many lines it runs to.
    ///
    /// Bounded twice over: in bytes, read no further than the cap allows
    /// (a line is not read whole and then measured, or a server sending one
    /// endless line fills memory first), and in time, the whole reply
    /// within one timeout (a line every 29 seconds would otherwise keep a
    /// download for days).
    async fn reply(&mut self) -> Result<(u16, String), Error> {
        tokio::time::timeout(TIMEOUT, self.reply_unbounded_in_time())
            .await
            .map_err(|_| dropped("the FTP server stopped answering"))?
    }

    async fn reply_unbounded_in_time(&mut self) -> Result<(u16, String), Error> {
        let mut used = 0usize;
        let mut first: Option<u16> = None;
        loop {
            let mut bytes = Vec::new();
            let left = (MAX_REPLY - used.min(MAX_REPLY)) as u64 + 1;
            let read = (&mut self.reader)
                .take(left)
                .read_until(b'\n', &mut bytes)
                .await
                .map_err(dropped)?;
            if read == 0 {
                return Err(dropped("the FTP server closed the connection"));
            }
            used += read;
            if used > MAX_REPLY || bytes.last() != Some(&b'\n') {
                return Err(Error::Internal("the FTP server's reply ran on".into()));
            }
            let line = String::from_utf8_lossy(&bytes);
            let line = line.trim_end();
            match (first, reply_code(line)) {
                (None, Some((code, true))) => return Ok((code, line.to_string())),
                (None, Some((code, false))) => first = Some(code),
                (None, None) => {
                    return Err(Error::Internal(format!(
                        "the FTP server answered something that is not FTP: {}",
                        line.chars().take(80).collect::<String>()
                    )))
                }
                (Some(code), Some((same, true))) if same == code => {
                    return Ok((code, line.to_string()))
                }
                // A line inside a multi-line reply.
                (Some(_), _) => {}
            }
        }
    }

    async fn command(&mut self, command: &str) -> Result<(u16, String), Error> {
        timed(self.writer.write_all(format!("{command}\r\n").as_bytes())).await?;
        self.reply().await
    }

    /// Opens a passive data connection: `EPSV`, or `PASV` where that is
    /// refused. Plain still: [`Session::secure`] starts its TLS once the
    /// command it carries has been answered.
    async fn data(&mut self, path: &str) -> Result<TcpStream, Error> {
        let (code, text) = self.command("EPSV").await?;
        let port = if code == 229 {
            epsv_port(&text)
        } else {
            let (code, text) = self.command("PASV").await?;
            if code != 227 {
                return Err(refusal(code, &text, path));
            }
            pasv_port(&text)
        };
        let port = port.ok_or_else(|| {
            Error::Internal("the FTP server's passive reply named no port".into())
        })?;
        connect(self.route.as_ref(), &self.data_host, port).await
    }

    /// A data connection as the transfer is spoken: in TLS for `ftps://`,
    /// begun now, after the server has answered the command it carries.
    async fn secure(&self, data: TcpStream) -> Result<Conn, Error> {
        match &self.tls {
            Some((connector, name)) => Ok(Box::new(Lenient(Box::new(
                handshake(connector, name, Box::new(data)).await?,
            )))),
            None => Ok(Box::new(data)),
        }
    }

    /// Starts sending `path` from `offset`.
    async fn retrieve(&mut self, path: &str, offset: u64) -> Result<Conn, Error> {
        let data = self.data(path).await?;
        if offset > 0 {
            let (code, text) = self.command(&format!("REST {offset}")).await?;
            if code != 350 {
                return Err(refusal(code, &text, path));
            }
        }
        let (code, text) = self.command(&format!("RETR {path}")).await?;
        if code != 150 && code != 125 {
            return Err(refusal(code, &text, path));
        }
        self.secure(data).await
    }
}

/// A TLS handshake as the client, within the timeout.
async fn handshake(
    connector: &tokio_rustls::TlsConnector,
    name: &ServerName<'static>,
    stream: Conn,
) -> Result<tokio_rustls::client::TlsStream<Conn>, Error> {
    timed(connector.connect(name.clone(), stream))
        .await
        .map_err(|e| Error::Internal(format!("the FTP server's TLS failed: {e}")))
}

/// What the server says about the file.
#[derive(Debug)]
struct Described {
    size: Option<u64>,
    resumable: bool,
    validator: Option<String>,
    /// The path from the root, where the file was found there and not
    /// relative to the login folder.
    absolute: Option<String>,
}

async fn describe(target: &Target) -> Result<Described, Error> {
    let mut session = Session::open(target).await?;
    let (mut code, mut text) = session.command(&format!("SIZE {}", target.path)).await?;
    /* Not there relative to the login folder: asked for from the root, as
     * dm read every FTP address before it read them as RFC 1738 says. A
     * server that does not keep a user in their home folder had that file
     * at `/home/user/f` for a link `ftp://user@host/home/user/f`, and such
     * a link, queued then, still works. */
    let mut absolute = None;
    if code == 550 && !target.path.starts_with('/') {
        let rooted = format!("/{}", target.path);
        let (again, said) = session.command(&format!("SIZE {rooted}")).await?;
        if again == 213 {
            (code, text) = (again, said);
            absolute = Some(rooted);
        }
    }
    let path = absolute.as_deref().unwrap_or(&target.path);
    let size = if code == 213 { number_in(&text) } else { None };
    if code == 550 {
        return Err(refusal(code, &text, &target.path));
    }
    let (code, text) = session.command(&format!("MDTM {path}")).await?;
    let validator = (code == 213)
        .then(|| text.get(4..).unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty());
    // A server that takes REST starts a transfer where it is told to.
    let (code, _) = session.command("REST 0").await?;
    let _ = session.command("QUIT").await;
    Ok(Described {
        size,
        resumable: code == 350 && size.is_some(),
        validator,
        absolute,
    })
}

/// One thing in an FTP folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub folder: bool,
}

/// The most of a folder's listing read: a server sending one without end
/// does not fill memory.
const MAX_LISTING: usize = 4 * 1024 * 1024;

/// An `MLSD` listing: `facts; name` a line, the type a fact.
fn parse_mlsd(text: &str) -> Vec<Listed> {
    text.lines()
        .filter_map(|line| {
            let (facts, name) = line.split_once(' ')?;
            let kind = facts.split(';').find_map(|f| {
                let (k, v) = f.split_once('=')?;
                k.eq_ignore_ascii_case("type")
                    .then(|| v.to_ascii_lowercase())
            })?;
            let folder = match kind.as_str() {
                "dir" => true,
                "file" => false,
                // The folder itself and its parent.
                _ => return None,
            };
            Some(Listed {
                name: name.to_string(),
                folder,
            })
        })
        .collect()
}

/// A `LIST` listing as Unix servers write it (`drwxr-xr-x 2 u g 4096 Jan 1
/// 00:00 name`) or Windows ones (`01-01-24 12:00AM <DIR> name`).
fn parse_list(text: &str) -> Vec<Listed> {
    text.lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let (folder, name) =
                if words.first()?.len() == 10 && words.first()?.starts_with(['d', '-', 'l']) {
                    // Unix: the name is everything after the eighth field.
                    let mut rest = line;
                    for _ in 0..8 {
                        rest = rest.trim_start();
                        rest = rest.split_once(char::is_whitespace)?.1;
                    }
                    let name = rest.trim_start();
                    // A link's "name -> target".
                    let name = name.split(" -> ").next().unwrap_or(name);
                    (words.first()?.starts_with('d'), name)
                } else if words.len() >= 4 {
                    let mut rest = line;
                    for _ in 0..3 {
                        rest = rest.trim_start();
                        rest = rest.split_once(char::is_whitespace)?.1;
                    }
                    (*words.get(2)? == "<DIR>", rest.trim_start())
                } else {
                    return None;
                };
            (!name.is_empty() && name != "." && name != "..").then(|| Listed {
                name: name.to_string(),
                folder,
            })
        })
        .collect()
}

/// What an FTP folder holds: its files and folders, by `MLSD` where the
/// server has it, else by `LIST`. Through the proxy the settings name for
/// it, as a download is; never around one.
pub async fn list(url: &str, options: &Options) -> Result<Vec<Listed>, Error> {
    if let Some(why) = options.no_route_through_system_proxy(url, "FTP") {
        return Err(Error::Internal(why));
    }
    let mut target = Target::parse_folder(url)?;
    target.route = Route::parse(options.proxy_for(url))?;
    let mut session = Session::open(&target).await?;
    let folder = target.path.trim_end_matches('/').to_string();
    for (verb, parse) in [
        ("MLSD", parse_mlsd as fn(&str) -> Vec<Listed>),
        ("LIST", parse_list),
    ] {
        let data = session.data(&folder).await?;
        let command = if folder.is_empty() {
            verb.to_string()
        } else {
            format!("{verb} {folder}")
        };
        let (code, text) = session.command(&command).await?;
        if code != 150 && code != 125 {
            drop(data);
            // Not a command this server knows: the older one.
            if verb == "MLSD" && (500..=504).contains(&code) {
                continue;
            }
            return Err(refusal(code, &text, &folder));
        }
        let data = session.secure(data).await?;
        let mut bytes = Vec::new();
        timed(async {
            let mut data = data;
            (&mut data)
                .take(MAX_LISTING as u64)
                .read_to_end(&mut bytes)
                .await
        })
        .await?;
        let _ = session.reply().await;
        let _ = session.command("QUIT").await;
        return Ok(parse(&String::from_utf8_lossy(&bytes)));
    }
    Err(Error::Internal(
        "the FTP server would not list the folder".into(),
    ))
}

/// Fetches an FTP file into `options.dest_dir`, over several connections
/// where the server takes `REST`.
pub async fn download_ftp(
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    // Through the proxy, where one is set and this site is not bypassed;
    // a proxy FTP cannot go through is a refusal, never a direct route.
    let mut target = Target::parse(url)?;
    if let Some(why) = options.no_route_through_system_proxy(url, "FTP") {
        return Err(Error::Internal(why));
    }
    target.route = Route::parse(options.proxy_for(url))?;
    let target = target;
    let options = &Options {
        connections: options.connections_for(url),
        ..options.clone()
    };
    let described = retry::run_up_to(&options.control, options.tries(), || {
        or_stop(&options.control, describe(&target))
    })
    .await?;
    let target = Target {
        path: described.absolute.clone().unwrap_or(target.path.clone()),
        ..target
    };
    let name = crate::probe::filename_for(None, url, None);

    fs::create_dir_all(&options.parts_dir).await?;
    fs::create_dir_all(&options.dest_dir).await?;
    /* An empty file has no range to fetch: planned as one byte it was
     * waited for and never came, and the download failed (or, set to keep
     * trying, never ended). It is placed as it is. */
    if described.size == Some(0) {
        let part = part_path(&options.parts_dir, 0);
        fs::write(&part, b"").await?;
        let (path, bytes, category) =
            crate::download::finish(&name, None, &[part], options).await?;
        return Ok(Finished {
            left_behind: None,
            path,
            bytes,
            connections: 1,
            category,
            verified: options.checksum.is_some(),
            mirrors: 1,
        });
    }
    let segments = planned(&described, options).await?;

    let downloaded = Arc::new(AtomicU64::new(0));
    let mut ranges = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let part = part_path(&options.parts_dir, index);
        let on_disk = fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
        if on_disk > segment.span() {
            if let Ok(file) = fs::OpenOptions::new().write(true).open(&part).await {
                let _ = file.set_len(segment.span()).await;
            }
        }
        let done = on_disk.min(segment.span());
        downloaded.fetch_add(done, Ordering::Relaxed);
        ranges.push((segment.clone(), part));
    }

    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: name.clone(),
                category: category::classify(&name, None),
            })
            .await;
        if let Some(validator) = &described.validator {
            let _ = tx.send(Update::Validator(validator.clone())).await;
        }
    }
    let reporter =
        report.map(|tx| crate::download::watch(Arc::clone(&downloaded), described.size, tx));

    let workers = options
        .connections
        .clamp(1, plan::MAX_CONNECTIONS)
        .min(ranges.len());
    let ranges = Arc::new(ranges);
    let next = Arc::new(AtomicUsize::new(0));
    let mut running = Vec::new();
    for _ in 0..workers {
        let (target, ranges, next, downloaded, options) = (
            target.clone(),
            Arc::clone(&ranges),
            Arc::clone(&next),
            Arc::clone(&downloaded),
            options.clone(),
        );
        let resumable = described.resumable;
        running.push(tokio::spawn(async move {
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((segment, part)) = ranges.get(index) else {
                    return Ok::<(), Error>(());
                };
                // As for HTTP: without REST each attempt starts from nothing.
                let on_disk = || {
                    if resumable {
                        std::fs::metadata(part).map_or(0, |m| m.len())
                    } else {
                        0
                    }
                };
                retry::run_making_progress(&options.control, options.tries(), on_disk, || {
                    fetch_range(&target, segment, part, resumable, &options, &downloaded)
                })
                .await?;
            }
        }));
    }
    let mut outcome = Ok(());
    for worker in running {
        let result = worker
            .await
            .map_err(|e| Error::Internal(e.to_string()))
            .and_then(|r| r);
        if outcome.is_ok() {
            outcome = result;
        }
    }
    if let Some((handle, finished)) = reporter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    outcome?;

    if let Some(expected) = described.size {
        let got = downloaded.load(Ordering::Relaxed);
        if got != expected {
            return Err(Error::ShortFile { got, expected });
        }
    }
    let parts: Vec<PathBuf> = ranges.iter().map(|(_, part)| part.clone()).collect();
    let (path, bytes, category) = crate::download::finish(&name, None, &parts, options).await?;
    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: workers,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// The plan recorded beside the parts where it is this file's, or a new one.
async fn planned(described: &Described, options: &Options) -> Result<Vec<Segment>, Error> {
    let dir = options.parts_dir.clone();
    let recorded = tokio::task::spawn_blocking(move || plan::recorded(&dir))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    if let Some(recorded) = recorded {
        if recorded.size == described.size
            && recorded.resumable
            && described.resumable
            && recorded.validator == described.validator
        {
            return Ok(recorded.segments());
        }
    }
    let mut listing = fs::read_dir(&options.parts_dir).await?;
    while let Some(entry) = listing.next_entry().await? {
        let _ = fs::remove_file(entry.path()).await;
    }
    let segments = plan::pieces(described.size, options.connections, described.resumable);
    let (dir, size, resumable, validator, written) = (
        options.parts_dir.clone(),
        described.size,
        described.resumable,
        described.validator.clone(),
        segments.clone(),
    );
    tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &written)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(segments)
}

fn part_path(parts_dir: &Path, index: usize) -> PathBuf {
    parts_dir.join(format!("part-{index:04}"))
}

/// One attempt at one range, from where its part file ends.
async fn fetch_range(
    target: &Target,
    segment: &Segment,
    part: &Path,
    resumable: bool,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<(), Error> {
    let span = segment.span();
    let mut already = fs::metadata(part)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
        .min(span);
    if already >= span {
        return Ok(());
    }
    // Without REST a transfer starts at the beginning, so it is kept from
    // there rather than written after what an earlier attempt left.
    if !resumable && already > 0 {
        let file = fs::OpenOptions::new().write(true).open(part).await?;
        file.set_len(0).await?;
        downloaded.fetch_sub(already, Ordering::Relaxed);
        already = 0;
    }

    let (mut session, mut data) = or_stop(&options.control, async {
        let mut session = Session::open(target).await?;
        let data = session
            .retrieve(&target.path, segment.start + already)
            .await?;
        Ok((session, data))
    })
    .await?;

    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .await?;
    let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, file);
    file.seek(std::io::SeekFrom::Start(already)).await?;

    let mut written = already;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if let Err(stopped) = options.control.check() {
            file.flush().await?;
            return Err(stopped);
        }
        // Raced against the control, as HTTP's reads are: a silent server
        // does not hold a pause for the read timeout.
        let read = match or_stop(&options.control, timed(data.read(&mut buffer))).await {
            Ok(read) => read,
            Err(e) => {
                file.flush().await?;
                return Err(e);
            }
        };
        if read == 0 {
            break;
        }
        let keep = (read as u64).min(span - written) as usize;
        file.write_all(buffer.get(..keep).unwrap_or_default())
            .await?;
        written += keep as u64;
        downloaded.fetch_add(keep as u64, Ordering::Relaxed);
        if let Err(stopped) = or_stop(&options.control, async {
            options.limiter.take(keep as u64).await;
            Ok(())
        })
        .await
        {
            file.flush().await?;
            return Err(stopped);
        }
        if written >= span {
            // This range is whole; the rest of the stream is another's.
            break;
        }
    }
    file.flush().await?;
    drop(data);

    // A file of unknown length ends where its transfer ends — and only a
    // transfer the server says is complete: a data connection closed early,
    // with 426 or 451 on the control one, is a short file, not the file.
    if segment.end == u64::MAX {
        let (code, text) = or_stop(&options.control, session.reply()).await?;
        return match code {
            226 | 250 => Ok(()),
            _ => Err(refusal(code, &text, &target.path)),
        };
    }
    if written < span {
        return Err(Error::Incomplete {
            got: written,
            expected: span,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tls_tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_rustls::rustls;

    /// One line from the client, without its line end.
    async fn line(conn: &mut Conn) -> Option<String> {
        let mut bytes = Vec::new();
        let mut one = [0u8; 1];
        loop {
            if conn.read(&mut one).await.ok()? == 0 {
                return None;
            }
            if one[0] == b'\n' {
                return Some(String::from_utf8_lossy(&bytes).trim_end().to_string());
            }
            bytes.push(one[0]);
        }
    }

    async fn say(conn: &mut Conn, text: &str) {
        conn.write_all(format!("{text}\r\n").as_bytes())
            .await
            .unwrap();
        conn.flush().await.unwrap();
    }

    /// An FTP server over TLS, as `implicit` or asked for with AUTH TLS,
    /// serving `body` as any file, its data connections in TLS too and
    /// closed without a word first, as many servers close them.
    async fn serve(implicit: bool, body: Vec<u8>) -> (u16, Arc<ClientConfig>) {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = made.cert.der().clone();
        let key =
            rustls::pki_types::PrivateKeyDer::try_from(made.key_pair.serialize_der()).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], key)
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = Arc::new(body);
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let (acceptor, body) = (acceptor.clone(), Arc::clone(&body));
                tokio::spawn(async move {
                    let mut conn: Conn = if implicit {
                        Box::new(acceptor.accept(tcp).await.unwrap())
                    } else {
                        Box::new(tcp)
                    };
                    say(&mut conn, "220 ready").await;
                    let mut from = 0usize;
                    let mut data: Option<TcpListener> = None;
                    let mut secure = implicit;
                    while let Some(asked) = line(&mut conn).await {
                        let verb = asked.split(' ').next().unwrap_or("").to_string();
                        match verb.as_str() {
                            "AUTH" => {
                                say(&mut conn, "234 go on").await;
                                conn = Box::new(acceptor.accept(conn).await.unwrap());
                                secure = true;
                            }
                            // Nothing is taken before the connection is TLS.
                            _ if !secure => say(&mut conn, "530 TLS first").await,
                            "USER" => say(&mut conn, "331 password").await,
                            "PASS" => say(&mut conn, "230 in").await,
                            "PBSZ" | "PROT" | "TYPE" => say(&mut conn, "200 ok").await,
                            "SIZE" => say(&mut conn, &format!("213 {}", body.len())).await,
                            "MDTM" => say(&mut conn, "550 no date").await,
                            "REST" => {
                                from = asked[5..].trim().parse().unwrap();
                                say(&mut conn, "350 from there").await;
                            }
                            "EPSV" => {
                                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                                let port = listener.local_addr().unwrap().port();
                                data = Some(listener);
                                say(&mut conn, &format!("229 passive (|||{port}|)")).await;
                            }
                            "RETR" => {
                                let listener = data.take().unwrap();
                                say(&mut conn, "150 sending").await;
                                let (tcp, _) = listener.accept().await.unwrap();
                                let mut out = acceptor.accept(tcp).await.unwrap();
                                let _ = out.write_all(&body[from.min(body.len())..]).await;
                                let _ = out.flush().await;
                                drop(out);
                                say(&mut conn, "226 done").await;
                            }
                            "QUIT" => {
                                say(&mut conn, "221 bye").await;
                                return;
                            }
                            _ => say(&mut conn, "502 not that").await,
                        }
                    }
                });
            }
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let client = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        (port, Arc::new(client))
    }

    async fn fetched(target: &Target, offset: u64) -> Vec<u8> {
        let mut session = Session::open(target).await.unwrap();
        let mut data = session.retrieve(&target.path, offset).await.unwrap();
        let mut got = Vec::new();
        data.read_to_end(&mut got).await.unwrap();
        got
    }

    #[test]
    fn an_ftps_address_is_implicit_on_990_and_asked_for_elsewhere() {
        let implicit = Target::parse("ftps://e.com/f.bin").unwrap();
        assert_eq!((implicit.port, implicit.tls), (990, Some(Tls::Implicit)));
        let explicit = Target::parse("ftps://e.com:21/f.bin").unwrap();
        assert_eq!((explicit.port, explicit.tls), (21, Some(Tls::Explicit)));
        assert_eq!(Target::parse("ftp://e.com/f.bin").unwrap().tls, None);
    }

    #[tokio::test]
    async fn a_file_comes_over_tls_asked_for_or_from_the_first_byte() {
        let body: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        for implicit in [false, true] {
            let (port, trust) = serve(implicit, body.clone()).await;
            let mut target = Target::parse(&format!("ftps://localhost:{port}/f.bin")).unwrap();
            target.tls = Some(if implicit {
                Tls::Implicit
            } else {
                Tls::Explicit
            });
            target.trust = Trust(Some(trust));
            let described = describe(&target).await.unwrap();
            assert_eq!(described.size, Some(body.len() as u64));
            assert!(described.resumable);
            assert_eq!(fetched(&target, 0).await, body, "implicit: {implicit}");
            // Resumed part-way, over a TLS data connection of its own.
            assert_eq!(fetched(&target, 150_000).await, body[150_000..]);
        }
    }

    #[tokio::test]
    async fn a_certificate_that_is_not_trusted_is_refused() {
        let (port, _) = serve(true, vec![1, 2, 3]).await;
        let mut target = Target::parse(&format!("ftps://localhost:{port}/f.bin")).unwrap();
        target.tls = Some(Tls::Implicit);
        // The system's roots, which do not hold the test's own certificate.
        let refused = Session::open(&target).await.err().unwrap().to_string();
        assert!(refused.contains("TLS"), "{refused}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_listings_are_read_in_both_forms() {
        let mlsd = "type=cdir;modify=20240101000000; .\r\ntype=pdir; ..\r\ntype=dir;modify=1; pub stuff\r\ntype=file;size=12; a file.zip\r\n";
        assert_eq!(
            parse_mlsd(mlsd),
            vec![
                Listed {
                    name: "pub stuff".into(),
                    folder: true
                },
                Listed {
                    name: "a file.zip".into(),
                    folder: false
                },
            ]
        );
        let unix = "drwxr-xr-x   2 ftp ftp   4096 Jan 01 00:00 releases\r\n-rw-r--r--   1 ftp ftp 123456 Jan 01  2024 big file.iso\r\nlrwxrwxrwx 1 ftp ftp 4 Jan 01 00:00 latest -> releases\r\ndrwxr-xr-x 2 ftp ftp 4096 Jan 01 00:00 ..\r\n";
        assert_eq!(
            parse_list(unix),
            vec![
                Listed {
                    name: "releases".into(),
                    folder: true
                },
                Listed {
                    name: "big file.iso".into(),
                    folder: false
                },
                Listed {
                    name: "latest".into(),
                    folder: false
                },
            ]
        );
        let dos = "01-01-24  12:00AM       <DIR>          old\r\n01-01-24  12:00AM               1024 notes.txt\r\n";
        assert_eq!(
            parse_list(dos),
            vec![
                Listed {
                    name: "old".into(),
                    folder: true
                },
                Listed {
                    name: "notes.txt".into(),
                    folder: false
                },
            ]
        );
    }

    #[test]
    fn an_ftp_address_is_read_into_its_parts() {
        let password = crate::test_secrets::marked("password");
        let t = Target::parse(&format!(
            "ftp://me:{}@ftp.example.com:2121/pub/a%20file.iso",
            crate::test_secrets::escaped(&password)
        ))
        .unwrap();
        assert_eq!(t.host, "ftp.example.com");
        assert_eq!(t.port, 2121);
        assert_eq!(t.user, "me");
        assert_eq!(t.password, password);
        assert_eq!(t.path, "pub/a file.iso", "relative to the login directory");
        let rooted = Target::parse("ftp://e.com/%2Fetc/motd.txt").unwrap();
        assert_eq!(rooted.path, "/etc/motd.txt", "the root, asked for by %2F");
        let anonymous = Target::parse("ftp://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz").unwrap();
        assert_eq!((anonymous.user.as_str(), anonymous.port), ("anonymous", 21));
        assert!(Target::parse("ftp://ftp.gnu.org/gnu/").is_err(), "a folder");
        assert!(
            Target::parse("ftp://e.com/a%0D%0ADELE%20b").is_err(),
            "a second command hidden in the path"
        );
        assert!(is_ftp("FTP://e.com/f") && is_ftp("ftps://e.com/f") && !is_ftp("http://e.com"));
    }

    #[test]
    fn replies_are_read_as_ftp_writes_them() {
        assert_eq!(reply_code("230 Logged in"), Some((230, true)));
        assert_eq!(reply_code("230-Welcome"), Some((230, false)));
        assert_eq!(reply_code("226"), Some((226, true)));
        assert_eq!(reply_code(" 230 x"), None);
        assert_eq!(reply_code("999 x"), None);
        assert_eq!(
            epsv_port("229 Entering Extended Passive Mode (|||6446|)"),
            Some(6446)
        );
        assert_eq!(
            pasv_port("227 Entering Passive Mode (192,168,1,2,19,137)."),
            Some(19 * 256 + 137)
        );
        assert_eq!(pasv_port("227 Entering Passive Mode (1,2,3,4,999,1)"), None);
        assert_eq!(number_in("213 1048576"), Some(1_048_576));
        assert_eq!(number_in("213"), None);
    }

    proptest::proptest! {
        #[test]
        fn no_reply_crashes_the_reader(line in "\\PC{0,200}") {
            let _ = reply_code(&line);
            let _ = epsv_port(&line);
            let _ = pasv_port(&line);
            let _ = number_in(&line);
            let _ = Target::parse(&format!("ftp://{line}"));
        }
    }
}
