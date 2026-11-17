// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
    let key = rustls::pki_types::PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
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

/// A real server, where one is named: `DM_FTPS_CHECK` its address,
/// `DM_FTPS_CA` its certificate (PEM) and `DM_FTPS_SHA256` the file's
/// digest. vsftpd with `require_ssl_reuse` refuses a data connection
/// that does not take up the control connection's TLS session, which an
/// in-test server cannot show. Nothing is checked where none is named.
#[tokio::test]
async fn a_real_ftps_server_when_one_is_named() {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    use sha2::Digest;
    let (Ok(url), Ok(ca), Ok(sha256)) = (
        std::env::var("DM_FTPS_CHECK"),
        std::env::var("DM_FTPS_CA"),
        std::env::var("DM_FTPS_SHA256"),
    ) else {
        return;
    };
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_file(ca).unwrap())
        .unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut target = Target::parse(&url).unwrap();
    target.trust = Trust(Some(Arc::new(config)));
    let size = describe(&target).await.unwrap().size.unwrap();
    let whole = fetched(&target, 0).await;
    assert_eq!(whole.len() as u64, size);
    assert_eq!(crate::hex::encode(sha2::Sha256::digest(&whole)), sha256);
    let half = size / 2;
    assert_eq!(fetched(&target, half).await, whole[half as usize..]);
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
