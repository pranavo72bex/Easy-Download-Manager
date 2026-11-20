// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Opening the connection: plain, TLS or through a tunnel, and the handshake.

use super::*;

/// How long a server is given to take the connection, plain, TLS or the tunnel's.
const CONNECT_WITHIN: Duration = Duration::from_secs(15);

/// How long one of the tunnel's requests is given, its answer included.
const TUNNEL_REQUEST_WITHIN: Duration = Duration::from_secs(30);

/// How long the tunnel waits for what the client writes while bytes flow, before it asks the
/// server with nothing to send.
const TUNNEL_QUICK: Duration = Duration::from_millis(10);

pub(super) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub(super) async fn open(target: &Target) -> Result<Box<dyn Io>, Error> {
    open_with(target, tls_config).await
}

/// [`open`], with the TLS `config` an `rtmps://` address is checked by
/// (a test's own root, where a test needs one).
async fn open_with(
    target: &Target,
    config: impl FnOnce() -> Result<Arc<tokio_rustls::rustls::ClientConfig>, Error>,
) -> Result<Box<dyn Io>, Error> {
    if target.tunnel {
        return tunnel(target).await;
    }
    let tcp = tokio::time::timeout(
        CONNECT_WITHIN,
        tokio::net::TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    .map_err(|_| Error::Internal(format!("{} did not answer", target.host)))??;
    let _ = tcp.set_nodelay(true);
    if !target.tls {
        return Ok(Box::new(tcp));
    }
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(target.host.clone())
        .map_err(|e| Error::Internal(format!("{}: {e}", target.host)))?;
    let tls = tokio_rustls::TlsConnector::from(config()?)
        .connect(name, tcp)
        .await?;
    Ok(Box::new(tls))
}

/// TLS as a stream server is checked (dm-net's, shared with every HTTP client).
pub(crate) use dm_net::tls_config;

/// The most one RTMPT answer may carry: far more than the few hundred
/// milliseconds of stream a server holds between polls.
pub(super) const TUNNEL_MOST: usize = 16 * 1024 * 1024;

/// RTMPT: the RTMP conversation in HTTP POSTs. `/open/1` names a session;
/// what dm writes goes out in `/send/<session>/<n>`, and with nothing to
/// send it asks `/idle/<session>/<n>` for what the server has. Each answer
/// starts with a byte saying how soon to ask again, then the server's
/// bytes. Spoken by a task of its own behind an in-memory pipe, so the
/// client above sees a stream like any other.
pub(super) async fn tunnel(target: &Target) -> Result<Box<dyn Io>, Error> {
    let base = if target.host.contains(':') {
        format!("http://[{}]:{}", target.host, target.port)
    } else {
        format!("http://{}:{}", target.host, target.port)
    };
    // dm's own client (`builder`), as every HTTP request is: its resolver connects a name a
    // public page led to at its public addresses alone, so one that resolves inside alone is
    // refused here as a download's is (gaps row 200); and it follows no redirect, where a bare
    // client followed any, unjudged. No proxy: a stream is refused under one before it gets here.
    let http = crate::builder()
        .no_proxy()
        .connect_timeout(CONNECT_WITHIN)
        .timeout(TUNNEL_REQUEST_WITHIN)
        .build()
        .map_err(|e| Error::Internal(format!("RTMPT: {e}")))?;
    let post = move |path: String, body: Vec<u8>| {
        let request = http
            .post(format!("{base}{path}"))
            .header("Content-Type", "application/x-fcs")
            .body(body);
        async move {
            let mut answer = request.send().await.map_err(tunnel_failure)?;
            answer = answer.error_for_status().map_err(|e| e.to_string())?;
            // Read in pieces, and no more than a stream's answers hold: a
            // server sending without end would otherwise fill the memory.
            let mut body = Vec::new();
            while let Some(piece) = answer.chunk().await.map_err(|e| e.to_string())? {
                if body.len() + piece.len() > TUNNEL_MOST {
                    return Err("an RTMPT answer too large".to_string());
                }
                body.extend_from_slice(&piece);
            }
            Ok::<_, String>(body)
        }
    };
    let opened = post("/open/1".into(), vec![0]).await.map_err(|e| {
        Error::Internal(format!(
            "{} did not open an RTMPT session: {e}",
            target.host
        ))
    })?;
    let session = String::from_utf8_lossy(&opened).trim().to_string();
    if session.is_empty() || session.contains('/') {
        return Err(Error::Internal(format!(
            "{} answered no RTMPT session",
            target.host
        )));
    }
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let (mut from_client, mut to_client) = tokio::io::split(theirs);
        let mut buffer = vec![0u8; 64 * 1024];
        let mut wait = TUNNEL_QUICK;
        for n in 1u64.. {
            // What the client has written, or nothing after a short wait.
            let read = tokio::time::timeout(wait, from_client.read(&mut buffer)).await;
            let (path, body) = match read {
                Ok(Ok(0)) | Ok(Err(_)) => break,
                Ok(Ok(len)) => (
                    format!("/send/{session}/{n}"),
                    buffer.get(..len).unwrap_or_default().to_vec(),
                ),
                Err(_) => (format!("/idle/{session}/{n}"), vec![0]),
            };
            let Ok(answer) = post(path, body).await else {
                break;
            };
            let Some((&interval, data)) = answer.split_first() else {
                break;
            };
            if to_client.write_all(data).await.is_err() {
                break;
            }
            // Quick while bytes flow; as slow as the server says otherwise,
            // within reason.
            wait = if data.is_empty() {
                Duration::from_millis(u64::from(interval.clamp(1, 50)) * 10)
            } else {
                TUNNEL_QUICK
            };
        }
        let _ = post(format!("/close/{session}/0"), vec![0]).await;
    });
    Ok(Box::new(ours))
}

/// What a tunnel request that was not sent says: a name a public page led to that the resolver
/// found inside alone in an inward redirect's words (`dns::LedInside`, gaps row 200), not as a
/// request that failed; anything else as reqwest says it.
fn tunnel_failure(error: reqwest::Error) -> String {
    if dm_net::dns::refused_inside(&error) {
        Error::Network(error).to_string()
    } else {
        error.to_string()
    }
}

/// The simple handshake: C0 and C1 out, S0, S1 and S2 in, C2 (S1 back).
pub(super) async fn handshake(io: &mut dyn Io) -> Result<(), Error> {
    let mut hello = vec![3u8];
    hello.extend_from_slice(&[0u8; 8]);
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
        | 1;
    for _ in 0..1528 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        hello.push(seed as u8);
    }
    io.write_all(&hello).await?;
    let mut s0s1 = vec![0u8; 1537];
    io.read_exact(&mut s0s1).await?;
    if s0s1.first() != Some(&3) {
        return Err(Error::Internal(
            "the server does not speak plain RTMP".into(),
        ));
    }
    io.write_all(s0s1.get(1..).unwrap_or_default()).await?;
    let mut s2 = vec![0u8; 1536];
    io.read_exact(&mut s2).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_rustls::rustls;

    /// A TLS server on loopback with a certificate made here: what it read
    /// first, once a client has written to it.
    async fn tls_server() -> (
        u16,
        rustls::pki_types::CertificateDer<'static>,
        tokio::task::JoinHandle<Vec<u8>>,
    ) {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = made.cert.der().clone();
        let key =
            rustls::pki_types::PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
        let server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let served = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let Ok(mut tls) = acceptor.accept(tcp).await else {
                return Vec::new();
            };
            let mut first = vec![0u8; 1];
            match tls.read_exact(&mut first).await {
                Ok(_) => first,
                Err(_) => Vec::new(),
            }
        });
        (port, cert, served)
    }

    fn rtmps(port: u16) -> Target {
        super::super::target(&format!("rtmps://localhost:{port}/live/show")).unwrap()
    }

    #[tokio::test]
    async fn an_rtmps_address_is_carried_over_tls() {
        let (port, cert, served) = tls_server().await;
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let target = rtmps(port);
        assert!(target.tls && !target.tunnel);
        let mut io = open_with(&target, || Ok(Arc::new(config))).await.unwrap();
        // RTMP's C0: the version byte, the handshake's first.
        io.write_all(&[3]).await.unwrap();
        io.flush().await.unwrap();
        assert_eq!(
            served.await.unwrap(),
            vec![3],
            "the server did not read it over TLS"
        );
    }

    #[tokio::test]
    async fn an_rtmps_server_the_public_roots_do_not_vouch_for_is_refused() {
        let (port, _cert, served) = tls_server().await;
        let refused = open(&rtmps(port)).await;
        assert!(
            refused.is_err(),
            "a certificate nobody vouches for was taken"
        );
        assert!(
            served.await.unwrap().is_empty(),
            "bytes went to an unchecked server"
        );
    }
}
