// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The HTTP client every request is made with: TLS, timeouts and which redirects it follows.

use crate::USER_AGENT;

/// How every client this engine builds is configured.
///
/// One builder rather than two: the proxied client is the one that actually
/// runs downloads once a proxy is set, so anything added here and not there
/// would apply to everybody except the people who configured a proxy.
pub fn builder() -> reqwest::ClientBuilder {
    builder_with(READ_TIMEOUT, false)
}

/// `builder` given dm's TLS: rustls with ring and the public roots, the
/// set-up the stream protocols use (`rtmp::tls_config`). Every
/// client is built through this: reqwest is built without a TLS provider of
/// its own, so one without it could not reach an `https://` address.
pub fn with_tls(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    match crate::rtmp::tls_config() {
        Ok(config) => builder.tls_backend_preconfigured((*config).clone()),
        Err(_) => builder,
    }
}

/// How long a connection may send nothing before it is given up on, unless
/// a setting says otherwise. Long enough for a server assembling a file;
/// short enough that a pause is honoured within it.
pub const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// What a refused downgrade says.
pub const INSECURE_REDIRECT: &str =
    "the secure address sent dm on to an insecure (http) one, where the file could be swapped on the way, so dm stopped";

/// What a refused inward redirect says.
pub const INWARD_REDIRECT: &str = "the address sent dm on to one inside this computer or its own network, which a public site has no business reaching, so dm stopped";

/// [`builder`], with the read timeout a setting: a server that takes a
/// minute to start sending a large archive is not a dead connection to the
/// person who knows it does that.
///
/// `system_proxy` has the client use the computer's proxy settings — the
/// environment's variables and, on macOS, the network settings. Off, the
/// client goes out directly whatever the environment says: a proxy dm uses
/// is one chosen in its Options, and reading the system's settings costs
/// over a second on some Macs, which every client built paid until it was
/// made the choice of the one kind that wants it.
pub fn builder_with(
    read_timeout: std::time::Duration,
    system_proxy: bool,
) -> reqwest::ClientBuilder {
    let builder = with_tls(reqwest::Client::builder());
    let builder = if system_proxy {
        builder
    } else {
        builder.no_proxy()
    };
    builder
        // Redirects are dm's to follow (`redirects`): reqwest wrote the
        // address each hop came from into `Referer`, for another host too.
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        // HTTP/1.1, on purpose, and this is the whole point of the program.
        //
        // Splitting a file into several ranges only beats a per-connection
        // throttle if the ranges travel on separate connections. Over HTTP/2
        // they do not: several requests to one host multiplex onto a single
        // TCP connection, sharing one congestion window and one throttle, so
        // eight ranges over h2 download at the speed of one. Modern CDNs
        // negotiate h2 by default (jsdelivr, Fastly, Cloudflare all do), so
        // without this pin the multi-connection feature quietly does nothing
        // on exactly the servers people download from most. Pinning h1 makes
        // each range a real connection again — and it helps the segmented
        // HLS and mirror paths for the same reason. A file server that spoke
        // only h2 would be refused, but for downloading files that server
        // does not exist; h2-only is API and gRPC territory.
        .http1_only()
        /* Bounded waits, or a pause is not.
         *
         * Every route honours pause and cancel between chunks, so a server
         * that accepts the connection, sends its headers and then sends
         * nothing holds the task forever with the flag never read: the row
         * says Paused because the queue said so, and a resume starts a second
         * writer on the same part files while the first still holds them. A
         * read that stalls this long is a dead connection, and the retry
         * policy already knows what to do with one of those. */
        .connect_timeout(std::time::Duration::from_secs(20))
        // A name looked up once a minute, not once a connection (`dns`, gaps row S19).
        .dns_resolver(crate::dns::resolver())
        // An idle connection, its buffers grown by a download, kept ten seconds
        // for the next request to the same server rather than reqwest's ninety
        // (gaps row S14): long enough for the next file of a batch, short
        // enough that dm at rest holds none of them.
        .pool_idle_timeout(std::time::Duration::from_secs(10))
        .read_timeout(read_timeout.max(std::time::Duration::from_secs(1)))
        .user_agent(USER_AGENT)
}

/// A client that speaks HTTP/2 from its first byte, for a server that refuses HTTP/1.1 (gaps
/// row F9): every other server keeps HTTP/1.1, whose ranges are connections of their own
/// ([`builder_with`]'s pin). Over it a download's ranges share one connection, which is all
/// such a server offers.
// As `client`: only a build without a TLS backend fails here.
#[allow(clippy::expect_used)]
pub fn http2_client() -> reqwest::Client {
    builder()
        .http2_prior_knowledge()
        .build()
        .expect("a client with no TLS backend cannot be built")
}

/// Whether `why` is a server refusing HTTP/1.1: it answered 505, answered an HTTP/1.1 request
/// with HTTP/2's own preface or a GOAWAY, or would agree only on HTTP/2 in the TLS handshake. Not
/// a connection that merely closed: that is any download cut short, and asked again over HTTP/2
/// one failed in the wrong words. The words alone, not the address in them: a 404 from
/// `http://127.0.0.1:50587/…` read as a 505 and was asked again over HTTP/2, which that
/// server does not speak (human QA on macOS, 2026-11-13).
pub fn refuses_http1(why: &str) -> bool {
    let why: String = why
        .split_whitespace()
        .filter(|word| !word.contains("://"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    [
        "answered 505",
        "http version not supported",
        "invalid http version",
        "noapplicationprotocol",
        "no application protocol",
        // An HTTP/2 server's own preface, read as an HTTP/1.1 answer.
        "unexpectedmessage",
        "unexpected message",
        "parse(version)",
        "goaway",
    ]
    .iter()
    .any(|said| why.contains(said))
}

/// A client configured the way a download manager wants one.
// The one `expect` dm ships: rustls is compiled in, so the only way this
// fails is a build without a TLS backend, which is a build bug.
#[allow(clippy::expect_used)]
pub fn client() -> reqwest::Client {
    builder()
        .build()
        .expect("a client with no TLS backend cannot be built")
}

/// reqwest's own defaults (every redirect followed, no timeouts, the
/// computer's proxy) with dm's TLS, for the tests that talk to a server of
/// their own. `reqwest::Client::new()` cannot stand in for it: reqwest is
/// built without a TLS provider of its own (gaps row P7), and a client built
/// with none panics unless a process-wide default was installed first.
// As `client`: only a build without a TLS backend fails here.
#[allow(clippy::expect_used)]
pub fn reqwest_defaults() -> reqwest::Client {
    with_tls(reqwest::Client::builder())
        .build()
        .expect("a client with no TLS backend cannot be built")
}

#[cfg(test)]
mod refusal_tests {
    use super::refuses_http1;

    #[test]
    fn a_505_is_a_refusal_and_an_address_with_505_in_it_is_not() {
        assert!(refuses_http1(
            "the server answered 505 HTTP Version Not Supported for http://e.com/a"
        ));
        assert!(!refuses_http1(
            "the server answered 404 Not Found for http://127.0.0.1:50587/files/missing.bin"
        ));
        assert!(!refuses_http1(
            "the server answered 404 Not Found for https://e.com/goaway/505.bin"
        ));
        assert!(refuses_http1("connection error: GOAWAY received"));
    }
}

#[cfg(test)]
mod redirect_tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::rustls;

    /// A secure server that sends every request on to `to`.
    async fn sending_on(to: String) -> (u16, Vec<u8>) {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = made.cert.der().to_vec();
        let key =
            rustls::pki_types::PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![made.cert.der().clone()], key)
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (acceptor, to) = (acceptor.clone(), to.clone());
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut asked = [0u8; 2048];
                    let _ = tls.read(&mut asked).await;
                    let answer = format!(
                        "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = tls.write_all(answer.as_bytes()).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        (port, cert)
    }

    #[tokio::test]
    async fn a_secure_address_is_not_followed_to_a_plain_one() {
        let plain = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let plain_port = plain.local_addr().unwrap().port();
        let (port, cert) = sending_on(format!("http://127.0.0.1:{plain_port}/f.bin")).await;
        // dm's own set-up, but trusting this server's certificate alone.
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(cert))
            .unwrap();
        let trusting = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = super::builder()
            .tls_backend_preconfigured(trusting)
            .build()
            .unwrap();
        let refused = crate::redirects::send(client.get(format!("https://localhost:{port}/f.bin")))
            .await
            .unwrap_err();
        let said = format!("{refused:?}");
        assert!(said.contains("insecure (http)"), "{said}");
        // The plain server was never asked.
        let asked =
            tokio::time::timeout(std::time::Duration::from_millis(200), plain.accept()).await;
        assert!(asked.is_err(), "the plain address was fetched");
    }

    /// Every client dm builds has its TLS (reqwest is built without a
    /// provider of its own): a secure server answers it, and one whose
    /// certificate the public roots do not vouch for is refused.
    #[tokio::test]
    async fn a_client_dm_builds_speaks_tls_with_the_public_roots_alone() {
        let (port, _) = sending_on("http://127.0.0.1:9/x".into()).await;
        let client = super::builder().build().unwrap();
        let said = format!(
            "{:?}",
            client
                .get(format!("https://localhost:{port}/f.bin"))
                .send()
                .await
                .unwrap_err()
        );
        assert!(said.contains("UnknownIssuer"), "{said}");
    }
}
