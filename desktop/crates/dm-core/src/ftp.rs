//! FTP, taken over from a link as HTTP is.
//!
//! A client of dm's own rather than a library: what a download needs of FTP
//! is small — sign in, ask the size and the date, open a passive data
//! connection, start it at an offset — and every reply it reads comes from
//! outside, so it is parsed here, where it is fuzzed with the rest.
//!
//! Each range is its own session: `REST` sets where `RETR` starts, and a
//! range that ends before the file does is cut off by closing both
//! connections, which is how FTP stops a transfer early. Each range has a
//! part file of its own, and the plan is written beside them as an HTTP
//! download's is, so a pause resumes and a resume keeps the plan it started
//! with.
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

mod listing;
mod reply;
mod session;
mod target;
mod transfer;
mod transport;

pub use listing::*;
use reply::*;
use session::*;
use target::*;
pub use transfer::*;
use transport::*;

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
            rustls::pki_types::PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
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
        assert_eq!(hex::encode(sha2::Sha256::digest(&whole)), sha256);
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
