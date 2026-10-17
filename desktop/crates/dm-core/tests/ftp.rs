//! FTP against a server on loopback that speaks enough of it: sign-in,
//! SIZE, MDTM, REST, EPSV and PASV, RETR. Bytes are compared, as for HTTP.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{options, tempdir};
use dm_core::{client, download, Error};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Default)]
struct Quirks {
    /// Only this name and password are let in; anonymous otherwise.
    login: Option<(&'static str, &'static str)>,
    /// No SIZE and no REST, as some old servers.
    bare: bool,
    /// EPSV refused, so PASV is used.
    no_epsv: bool,
    /// The first this-many transfers stop halfway.
    drops: usize,
    /// Each transfer is sent at this many bytes a second, if set.
    rate: Option<u64>,
    /// A greeting that never ends its line.
    endless: bool,
    /// Every transfer stops halfway and says so: 426 on the control line.
    aborts: bool,
}

#[derive(Clone)]
struct Server {
    body: Arc<Vec<u8>>,
    quirks: Quirks,
    /// Every REST offset a RETR started at.
    starts: Arc<Mutex<Vec<u64>>>,
    transfers: Arc<AtomicUsize>,
}

async fn serve(body: Vec<u8>, quirks: Quirks) -> (String, Server) {
    let server = Server {
        body: Arc::new(body),
        quirks,
        starts: Arc::default(),
        transfers: Arc::default(),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = server.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(session(stream, shared.clone()));
        }
    });
    (format!("127.0.0.1:{port}"), server)
}

async fn say(w: &mut tokio::net::tcp::OwnedWriteHalf, text: String) -> std::io::Result<()> {
    w.write_all(format!("{text}\r\n").as_bytes()).await
}

async fn session(stream: TcpStream, s: Server) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    if s.quirks.endless {
        let junk = vec![b'2'; 64 * 1024];
        for _ in 0..64 {
            if write.write_all(&junk).await.is_err() {
                return;
            }
        }
        return;
    }
    let _ = say(&mut write, "220-dm's test server\r\n220 ready".into()).await;
    let (mut user, mut offset) = (String::new(), 0u64);
    let mut passive: Option<TcpListener> = None;
    let mut signed_in = false;
    while let Ok(Some(line)) = lines.next_line().await {
        let (verb, arg) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let reply = match verb {
            "USER" => {
                user = arg.to_string();
                "331 password please".to_string()
            }
            "PASS" => {
                let ok = match s.quirks.login {
                    Some((name, password)) => user == name && arg == password,
                    None => user == "anonymous",
                };
                signed_in = ok;
                if ok { "230 in" } else { "530 Login incorrect." }.to_string()
            }
            _ if !signed_in => "530 sign in first".to_string(),
            "TYPE" => "200 binary".into(),
            "SIZE" if s.quirks.bare => "502 no SIZE here".into(),
            "SIZE" if arg == "/f.bin" => format!("213 {}", s.body.len()),
            "SIZE" | "RETR" if arg != "/f.bin" => "550 No such file.".into(),
            "MDTM" => "213 20260101120000".into(),
            "REST" if s.quirks.bare => "502 no REST here".into(),
            "REST" => {
                offset = arg.parse().unwrap_or(0);
                "350 restarting".into()
            }
            "EPSV" if s.quirks.no_epsv => "500 no EPSV".into(),
            "EPSV" | "PASV" => {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                passive = Some(listener);
                if verb == "EPSV" {
                    format!("229 Entering Extended Passive Mode (|||{port}|)")
                } else {
                    format!(
                        "227 Entering Passive Mode (10,0,0,9,{},{}).",
                        port / 256,
                        port % 256
                    )
                }
            }
            "RETR" => {
                let Some(listener) = passive.take() else {
                    let _ = say(&mut write, "425 no data connection".into()).await;
                    continue;
                };
                let _ = say(&mut write, "150 sending".into()).await;
                let Ok((mut data, _)) = listener.accept().await else {
                    continue;
                };
                s.starts.lock().unwrap().push(offset);
                let n = s.transfers.fetch_add(1, Ordering::SeqCst);
                let from = (offset as usize).min(s.body.len());
                let mut slice = &s.body[from..];
                if n < s.quirks.drops || s.quirks.aborts {
                    slice = &slice[..slice.len() / 2];
                }
                offset = 0;
                let mut ok = true;
                for chunk in slice.chunks(64 * 1024) {
                    if data.write_all(chunk).await.is_err() {
                        ok = false;
                        break;
                    }
                    if let Some(rate) = s.quirks.rate {
                        tokio::time::sleep(Duration::from_secs_f64(
                            chunk.len() as f64 / rate as f64,
                        ))
                        .await;
                    }
                }
                drop(data);
                if n < s.quirks.drops {
                    // The control connection goes too, as a reset would.
                    return;
                }
                if ok && !s.quirks.aborts {
                    "226 done"
                } else {
                    "426 aborted"
                }
                .to_string()
            }
            "QUIT" => {
                let _ = say(&mut write, "221 bye".into()).await;
                return;
            }
            _ => "502 not here".into(),
        };
        if say(&mut write, reply).await.is_err() {
            return;
        }
    }
}

fn body(n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u32).wrapping_mul(2_654_435_761) as u8)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ftp_file_is_fetched_over_several_connections() {
    let data = body(8 * 1024 * 1024 + 13);
    let (addr, server) = serve(data.clone(), Quirks::default()).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("ftp://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the FTP download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data, "not the file");
    assert_eq!(done.path.file_name().unwrap(), "f.bin");
    let starts = server.starts.lock().unwrap().clone();
    assert!(starts.len() > 1, "one connection only: {starts:?}");
    assert!(
        starts.iter().any(|&s| s > 0),
        "no range began past the start"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_ftp_download_resumes_from_its_parts() {
    let data = body(4 * 1024 * 1024);
    let quirks = Quirks {
        rate: Some(1024 * 1024),
        ..Quirks::default()
    };
    let (addr, server) = serve(data.clone(), quirks).await;
    let dir = tempdir();
    let opts = options(dir.path());
    let url = format!("ftp://{addr}/f.bin");
    let running = {
        let (url, opts) = (url.clone(), opts.clone());
        tokio::spawn(async move { download(&client(), &url, &opts).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    opts.control.pause();
    let first = running.await.unwrap();
    assert!(matches!(first, Err(Error::Paused)), "got {first:?}");
    let staged: u64 = std::fs::read_dir(&opts.parts_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("part-"))
        .map(|e| e.metadata().unwrap().len())
        .sum();
    assert!(staged > 0, "nothing was staged before the pause");

    server.starts.lock().unwrap().clear();
    let again = dm_core::Options {
        control: dm_core::Control::new(),
        connections: 2,
        ..opts
    };
    let done = download(&client(), &url, &again)
        .await
        .expect("the resume failed");
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        data,
        "the resume spliced wrong"
    );
    let starts = server.starts.lock().unwrap().clone();
    assert!(
        !starts.contains(&0),
        "the first range was fetched again from its start: {starts:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_ftp_transfer_is_tried_again_from_where_it_stopped() {
    let data = body(3 * 1024 * 1024);
    let quirks = Quirks {
        drops: 1,
        no_epsv: true,
        ..Quirks::default()
    };
    let (addr, server) = serve(data.clone(), quirks).await;
    let dir = tempdir();
    let mut opts = options(dir.path());
    opts.connections = 1;
    let done = download(&client(), &format!("ftp://{addr}/f.bin"), &opts)
        .await
        .expect("a dropped transfer failed the download");
    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    let starts = server.starts.lock().unwrap().clone();
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert!(starts[1] > 0, "the retry started over: {starts:?}");
}

#[tokio::test]
async fn a_server_without_size_or_rest_is_fetched_whole() {
    let data = body(1024 * 1024 + 5);
    let quirks = Quirks {
        bare: true,
        ..Quirks::default()
    };
    let (addr, _) = serve(data.clone(), quirks).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("ftp://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("the download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    assert_eq!(done.connections, 1);
}

#[tokio::test]
async fn a_sign_in_comes_from_the_address_and_a_wrong_one_is_said() {
    let data = body(200_000);
    let quirks = Quirks {
        login: Some(("me", "p@ss")),
        ..Quirks::default()
    };
    let (addr, _) = serve(data.clone(), quirks).await;
    let dir = tempdir();
    let done = download(
        &client(),
        &format!("ftp://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect("signed in, it failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data);

    let other = tempdir();
    let refused = download(
        &client(),
        &format!("ftp://{addr}/f.bin"),
        &options(other.path()),
    )
    .await
    .expect_err("a wrong password was let through");
    assert!(
        refused.to_string().contains("refused the sign-in"),
        "{refused}"
    );
    let missing = download(
        &client(),
        &format!("ftp://{addr}/nothing.bin"),
        &options(other.path()),
    )
    .await
    .expect_err("a missing file was saved");
    assert!(missing.to_string().contains("has no file"), "{missing}");
}

#[test]
fn the_queue_takes_ftp_addresses() {
    assert!(dm_core::queue::accepted(
        "ftp://ftp.gnu.org/gnu/hello/hello-2.12.tar.gz"
    ));
}

#[tokio::test]
async fn a_server_whose_greeting_never_ends_is_refused_not_held_in_memory() {
    let quirks = Quirks {
        endless: true,
        ..Quirks::default()
    };
    let (addr, _) = serve(body(10), quirks).await;
    let dir = tempdir();
    let failed = download(
        &client(),
        &format!("ftp://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect_err("an endless reply was taken");
    assert!(failed.to_string().contains("ran on"), "{failed}");
}

#[tokio::test]
async fn a_file_of_unknown_length_cut_short_is_not_reported_complete() {
    let quirks = Quirks {
        bare: true,
        aborts: true,
        ..Quirks::default()
    };
    let (addr, _) = serve(body(1024 * 1024), quirks).await;
    let dir = tempdir();
    let failed = download(
        &client(),
        &format!("ftp://{addr}/f.bin"),
        &options(dir.path()),
    )
    .await
    .expect_err("half a file was called the file");
    assert!(failed.to_string().contains("426"), "{failed}");
    assert!(
        !dir.path().join("out/f.bin").exists(),
        "a short file reached the download folder"
    );
}

/* ------------------------------------------------------------ proxies */

/// What a proxy on loopback saw: the targets it was asked to reach.
#[derive(Clone, Default)]
struct Seen {
    targets: Arc<Mutex<Vec<String>>>,
}

/// Copies bytes both ways until one side closes.
async fn pipe(a: TcpStream, b: TcpStream) {
    let (mut ar, mut aw) = a.into_split();
    let (mut br, mut bw) = b.into_split();
    let _ = tokio::join!(
        tokio::io::copy(&mut ar, &mut bw),
        tokio::io::copy(&mut br, &mut aw)
    );
}

/// A SOCKS5 proxy that wants a name and password, enough of RFC 1928 and
/// 1929 for a CONNECT to an address or a name.
async fn socks5_proxy(user: &'static str, password: &'static str) -> (String, Seen) {
    use tokio::io::AsyncReadExt;
    let seen = Seen::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let seen = shared.clone();
            tokio::spawn(async move {
                let mut head = [0u8; 2];
                stream.read_exact(&mut head).await.unwrap();
                assert_eq!(head[0], 5, "not SOCKS5");
                let mut methods = vec![0u8; head[1] as usize];
                stream.read_exact(&mut methods).await.unwrap();
                assert!(
                    methods.contains(&2),
                    "no name-and-password offered: {methods:?}"
                );
                stream.write_all(&[5, 2]).await.unwrap();
                // RFC 1929: version, name, password.
                let mut v = [0u8; 2];
                stream.read_exact(&mut v).await.unwrap();
                let mut name = vec![0u8; v[1] as usize];
                stream.read_exact(&mut name).await.unwrap();
                let mut plen = [0u8; 1];
                stream.read_exact(&mut plen).await.unwrap();
                let mut pass = vec![0u8; plen[0] as usize];
                stream.read_exact(&mut pass).await.unwrap();
                let ok = name == user.as_bytes() && pass == password.as_bytes();
                stream
                    .write_all(&[1, if ok { 0 } else { 1 }])
                    .await
                    .unwrap();
                if !ok {
                    return;
                }
                let mut req = [0u8; 4];
                stream.read_exact(&mut req).await.unwrap();
                assert_eq!((req[0], req[1]), (5, 1), "not a CONNECT");
                let host = match req[3] {
                    1 => {
                        let mut a = [0u8; 4];
                        stream.read_exact(&mut a).await.unwrap();
                        std::net::Ipv4Addr::from(a).to_string()
                    }
                    3 => {
                        let mut n = [0u8; 1];
                        stream.read_exact(&mut n).await.unwrap();
                        let mut name = vec![0u8; n[0] as usize];
                        stream.read_exact(&mut name).await.unwrap();
                        String::from_utf8(name).unwrap()
                    }
                    other => panic!("address type {other}"),
                };
                let mut p = [0u8; 2];
                stream.read_exact(&mut p).await.unwrap();
                let port = u16::from_be_bytes(p);
                seen.targets.lock().unwrap().push(format!("{host}:{port}"));
                let upstream = TcpStream::connect((host.as_str(), port)).await.unwrap();
                stream
                    .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
                    .await
                    .unwrap();
                pipe(stream, upstream).await;
            });
        }
    });
    (format!("127.0.0.1:{port}"), seen)
}

/// An HTTP proxy that tunnels `CONNECT host:port`, and nothing else.
async fn connect_proxy() -> (String, Seen) {
    let seen = Seen::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let seen = shared.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let target = line
                    .strip_prefix("CONNECT ")
                    .and_then(|r| r.split_whitespace().next())
                    .unwrap_or_else(|| panic!("not a CONNECT: {line:?}"))
                    .to_string();
                // The rest of the head.
                loop {
                    let mut l = String::new();
                    reader.read_line(&mut l).await.unwrap();
                    if l == "\r\n" || l.is_empty() {
                        break;
                    }
                }
                seen.targets.lock().unwrap().push(target.clone());
                let upstream = TcpStream::connect(target.as_str()).await.unwrap();
                write
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await
                    .unwrap();
                let stream = reader.into_inner().reunite(write).unwrap();
                pipe(stream, upstream).await;
            });
        }
    });
    (format!("127.0.0.1:{port}"), seen)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ftp_download_goes_through_a_socks5_proxy_and_not_around_it() {
    let data = body(3 * 1024 * 1024);
    let (addr, _server) = serve(data.clone(), Quirks::default()).await;
    let (proxy, seen) = socks5_proxy("me", "s3cret").await;
    let dir = tempdir();
    let through = dm_core::Options {
        proxy: Some(format!("socks5://{proxy}")),
        ..options(dir.path())
    };
    let done = download(&client(), &format!("ftp://{addr}/f.bin"), &through)
        .await
        .expect("the FTP download through SOCKS5 failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data, "not the file");
    let targets = seen.targets.lock().unwrap().clone();
    // Control connections to the server's port and data connections to the
    // ports it opened: every connection went through the proxy.
    assert!(
        targets.iter().any(|t| t == &addr),
        "no control connection: {targets:?}"
    );
    assert!(
        targets.iter().any(|t| t != &addr),
        "no data connection: {targets:?}"
    );
    assert!(targets.len() >= 4, "{targets:?}");

    // A wrong proxy password is the proxy's refusal, said, not a direct route.
    let dir = tempdir();
    let wrong = dm_core::Options {
        proxy: Some(format!("socks5://{proxy}")),
        attempts: 1,
        ..options(dir.path())
    };
    let outcome = download(&client(), &format!("ftp://{addr}/f.bin"), &wrong).await;
    assert!(
        matches!(&outcome, Err(Error::Connection(said)) if said.contains("proxy")),
        "{outcome:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ftp_download_goes_through_an_http_proxy_by_connect() {
    let data = body(2 * 1024 * 1024 + 5);
    let (addr, _server) = serve(data.clone(), Quirks::default()).await;
    let (proxy, seen) = connect_proxy().await;
    let dir = tempdir();
    let through = dm_core::Options {
        proxy: Some(format!("http://{proxy}")),
        ..options(dir.path())
    };
    let done = download(&client(), &format!("ftp://{addr}/f.bin"), &through)
        .await
        .expect("the FTP download through CONNECT failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data, "not the file");
    let targets = seen.targets.lock().unwrap().clone();
    assert!(targets.iter().any(|t| t == &addr), "{targets:?}");
    assert!(targets.len() >= 2, "{targets:?}");
}

#[tokio::test]
async fn a_bypassed_site_is_fetched_directly_and_an_https_proxy_is_refused() {
    let data = body(64 * 1024);
    let (addr, _server) = serve(data.clone(), Quirks::default()).await;
    let dir = tempdir();
    // A proxy nobody answers at, and this site on its bypass list.
    let bypassed = dm_core::Options {
        proxy: Some("socks5://127.0.0.1:9".into()),
        proxy_bypass: vec!["127.0.0.1".into()],
        ..options(dir.path())
    };
    let done = download(&client(), &format!("ftp://{addr}/f.bin"), &bypassed)
        .await
        .expect("a bypassed site was sent to the proxy");
    assert_eq!(std::fs::read(&done.path).unwrap(), data);

    // Not bypassed: the dead proxy is a failure, never a direct route.
    let dir = tempdir();
    let dead = dm_core::Options {
        proxy: Some("socks5://127.0.0.1:9".into()),
        attempts: 1,
        ..options(dir.path())
    };
    assert!(download(&client(), &format!("ftp://{addr}/f.bin"), &dead)
        .await
        .is_err());

    let dir = tempdir();
    let https = dm_core::Options {
        proxy: Some("https://proxy.example:443".into()),
        ..options(dir.path())
    };
    let outcome = download(&client(), &format!("ftp://{addr}/f.bin"), &https).await;
    assert!(
        matches!(&outcome, Err(Error::Internal(said)) if said.contains("HTTPS proxy")),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn under_the_computers_proxy_with_no_address_ftp_is_refused_not_sent_direct() {
    let data = body(16 * 1024);
    let (addr, server) = serve(data.clone(), Quirks::default()).await;
    let dir = tempdir();
    let system = dm_core::Options {
        system_proxy: true,
        proxy: None,
        ..options(dir.path())
    };
    let outcome = download(&client(), &format!("ftp://{addr}/f.bin"), &system).await;
    assert!(
        matches!(&outcome, Err(Error::Internal(said)) if said.contains("computer's proxy")),
        "{outcome:?}"
    );
    assert!(
        server.starts.lock().unwrap().is_empty(),
        "the server was reached directly"
    );
    // On the bypass list, it goes direct, as asked.
    let dir = tempdir();
    let bypassed = dm_core::Options {
        proxy_bypass: vec!["127.0.0.1".into()],
        ..system.clone()
    };
    let bypassed = dm_core::Options {
        dest_dir: dir.path().join("out"),
        parts_dir: dir.path().join("parts"),
        ..bypassed
    };
    let done = download(&client(), &format!("ftp://{addr}/f.bin"), &bypassed)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), data);
}

#[tokio::test]
async fn an_empty_ftp_file_downloads() {
    let (addr, _server) = serve(Vec::new(), Quirks::default()).await;
    let dir = tempdir();
    let done = tokio::time::timeout(
        Duration::from_secs(10),
        download(
            &client(),
            &format!("ftp://{addr}/f.bin"),
            &options(dir.path()),
        ),
    )
    .await
    .expect("an empty file was waited for")
    .expect("an empty file failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), Vec::<u8>::new());
}
