//! Sharing one finished file on the local network: a link another computer
//! pastes into its download manager, which then fetches the file over
//! several connections at once.
//!
//! Only on request. A share listens from the moment a person starts it until
//! they stop it or its time runs out, and serves that one file, to whoever
//! has the link: the address holds 128 random bits, so it cannot be guessed.
//! Nothing else on the computer is reachable through it. The server answers
//! byte ranges, which is what lets the other side open several connections;
//! it sends no cookies and keeps no log of who asked.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How long a share lasts when nobody stops it.
pub const LASTS: Duration = Duration::from_secs(60 * 60);
/// How many connections are held at once, answered or waiting to be; more
/// are closed unanswered, and a download manager tries again.
const MOST_AT_ONCE: usize = 32;
/// How long a connection has to send its request.
const REQUEST_WITHIN: Duration = Duration::from_secs(10);
/// How long a write may stall before the connection is dropped.
const STALLED_FOR: Duration = Duration::from_secs(60);
/// The longest request head read.
const MOST_HEAD: usize = 8 * 1024;
const CHUNK: usize = 256 * 1024;

/// A file being shared. Dropping it, or calling [`Share::stop`], ends it.
pub struct Share {
    url: String,
    sent: Arc<AtomicU64>,
    asked: Arc<AtomicU64>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Share {
    /// The link to give out.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// How many bytes have gone out so far.
    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    /// How many requests for the file have been answered.
    pub fn asked(&self) -> u64 {
        self.asked.load(Ordering::Relaxed)
    }

    /// Ends the share: the port closes and the link stops working.
    pub fn stop(mut self) {
        self.close();
    }

    fn close(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        self.close();
    }
}

impl std::fmt::Debug for Share {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Share")
            .field("sent", &self.sent())
            .finish_non_exhaustive()
    }
}

/// The address this computer has on the network a neighbour would reach it
/// by, or why there is none. Nothing is sent: connecting a datagram socket
/// only makes the system pick the interface it would use.
pub fn local_address() -> Result<Ipv4Addr, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket
        .connect("192.0.2.1:9")
        .map_err(|_| "this computer is not on a network".to_string())?;
    match socket.local_addr().map_err(|e| e.to_string())?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Ok(ip),
        _ => Err("this computer is not on a network".into()),
    }
}

/// Starts sharing `path` on the local network for `lasts`.
pub fn start(path: &Path, lasts: Duration) -> Result<Share, String> {
    let ip = local_address()?;
    // Only on the address the link names: not on the loopback, a VPN or any
    // other network this computer happens to be on.
    start_on(path, ip, ip, lasts)
}

/// Listens on `bind` and puts `advertise` in the link: apart so that a test
/// can share on the loopback.
pub fn start_on(
    path: &Path,
    bind: Ipv4Addr,
    advertise: Ipv4Addr,
    lasts: Duration,
) -> Result<Share, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("the file cannot be read: {e}"))?;
    let meta = std::fs::metadata(&path).map_err(|e| format!("the file cannot be read: {e}"))?;
    if !meta.is_file() {
        return Err("only a file can be shared".into());
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let token = token()?;
    let listener = std::net::TcpListener::bind(SocketAddr::from((bind, 0)))
        .map_err(|e| format!("a port could not be opened: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("dm-share")
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let sent = Arc::new(AtomicU64::new(0));
    let asked = Arc::new(AtomicU64::new(0));
    let served = Arc::new(Served {
        path,
        prefix: format!("/{token}/"),
        name: name.clone(),
        sent: Arc::clone(&sent),
        asked: Arc::clone(&asked),
        busy: AtomicUsize::new(0),
    });
    let listener = {
        let _in = runtime.enter();
        TcpListener::from_std(listener).map_err(|e| e.to_string())?
    };
    runtime.spawn(async move {
        tokio::select! {
            _ = accept_loop(listener, served) => {}
            _ = tokio::time::sleep(lasts) => {}
        }
    });
    Ok(Share {
        url: format!("http://{advertise}:{port}/{token}/{}", encode(&name)),
        sent,
        asked,
        runtime: Some(runtime),
    })
}

/// A place among the connections being answered, given back when dropped.
struct Slot<'a>(&'a AtomicUsize);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct Served {
    path: PathBuf,
    /// `/<token>/`: a request must begin with it.
    prefix: String,
    name: String,
    sent: Arc<AtomicU64>,
    asked: Arc<AtomicU64>,
    busy: AtomicUsize,
}

fn token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "no random numbers are available".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

async fn accept_loop(listener: TcpListener, served: Arc<Served>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            // A failed accept (a descriptor shortage, say) is not the end of
            // the share; a pause keeps it from spinning.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        // A connection costs a task and a descriptor from the moment it is
        // accepted, and nothing has been proved yet: the count includes the
        // ones that never send a request, and a full house is not answered.
        if served.busy.fetch_add(1, Ordering::Relaxed) >= MOST_AT_ONCE {
            served.busy.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        let served = Arc::clone(&served);
        tokio::spawn(async move {
            let _slot = Slot(&served.busy);
            let _ = answer(stream, &served).await;
        });
    }
}

/// What a request asked for.
#[derive(Debug, PartialEq)]
enum Wanted {
    /// The whole file.
    Whole,
    /// The bytes from `.0` to `.1`, both included.
    Part(u64, u64),
    /// A range the file has nothing for.
    Beyond,
}

/// Reads a `Range` header against a file of `len` bytes. Only a single range
/// is answered as one; a list of ranges, or anything unreadable, is answered
/// with the whole file, which the protocol allows.
fn wanted(range: Option<&str>, len: u64) -> Wanted {
    let Some(spec) = range
        .map(str::trim)
        .and_then(|r| r.strip_prefix("bytes="))
        .filter(|s| !s.contains(','))
    else {
        return Wanted::Whole;
    };
    let Some((first, last)) = spec.split_once('-') else {
        return Wanted::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    if first.is_empty() {
        // The last `n` bytes.
        let Ok(n) = last.parse::<u64>() else {
            return Wanted::Whole;
        };
        if n == 0 || len == 0 {
            return Wanted::Beyond;
        }
        return Wanted::Part(len.saturating_sub(n), len - 1);
    }
    let Ok(first) = first.parse::<u64>() else {
        return Wanted::Whole;
    };
    let last = if last.is_empty() {
        len.saturating_sub(1)
    } else {
        match last.parse::<u64>() {
            Ok(n) => n.min(len.saturating_sub(1)),
            Err(_) => return Wanted::Whole,
        }
    };
    if first >= len || first > last {
        Wanted::Beyond
    } else {
        Wanted::Part(first, last)
    }
}

async fn answer(mut stream: TcpStream, served: &Served) -> std::io::Result<()> {
    let _ = stream.set_nodelay(true);
    let Some(head) = tokio::time::timeout(REQUEST_WITHIN, read_head(&mut stream))
        .await
        .ok()
        .flatten()
    else {
        return Ok(());
    };
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "GET" && method != "HEAD" {
        return refuse(
            &mut stream,
            "405 Method Not Allowed",
            "Allow: GET, HEAD\r\n",
        )
        .await;
    }
    // The same answer for a wrong token as for anything else: it says
    // nothing about what is behind it.
    let path = target.split(['?', '#']).next().unwrap_or("");
    if !same(
        path.get(..served.prefix.len()).unwrap_or(""),
        &served.prefix,
    ) {
        return refuse(&mut stream, "404 Not Found", "").await;
    }
    let range = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.trim().to_string());

    let Ok(mut file) = tokio::fs::File::open(&served.path).await else {
        return refuse(&mut stream, "404 Not Found", "").await;
    };
    let Ok(meta) = file.metadata().await else {
        return refuse(&mut stream, "404 Not Found", "").await;
    };
    let len = meta.len();
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let common = format!(
        "Accept-Ranges: bytes\r\nETag: \"{len:x}-{stamp:x}\"\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename*=UTF-8''{}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n",
        encode(&served.name)
    );
    let (status, from, to, extra) = match wanted(range.as_deref(), len) {
        Wanted::Whole => ("200 OK", 0, len, String::new()),
        Wanted::Part(a, b) => (
            "206 Partial Content",
            a,
            b + 1,
            format!("Content-Range: bytes {a}-{b}/{len}\r\n"),
        ),
        Wanted::Beyond => {
            let said = format!("Content-Range: bytes */{len}\r\n");
            return refuse(&mut stream, "416 Range Not Satisfiable", &said).await;
        }
    };
    let head = format!(
        "HTTP/1.1 {status}\r\n{common}{extra}Content-Length: {}\r\n\r\n",
        to - from
    );
    write_all(&mut stream, head.as_bytes()).await?;
    if method == "HEAD" {
        return Ok(());
    }
    served.asked.fetch_add(1, Ordering::Relaxed);
    file.seek(std::io::SeekFrom::Start(from)).await?;
    let mut left = to - from;
    let mut buffer = vec![0u8; CHUNK];
    while left > 0 {
        let want = buffer.len().min(left as usize);
        let Some(chunk) = buffer.get_mut(..want) else {
            break;
        };
        let n = file.read(chunk).await?;
        if n == 0 {
            // The file shrank under the transfer: the connection is cut, so
            // the other side sees a short body and not a whole one.
            break;
        }
        write_all(&mut stream, chunk.get(..n).unwrap_or(&[])).await?;
        served.sent.fetch_add(n as u64, Ordering::Relaxed);
        left -= n as u64;
    }
    let _ = stream.shutdown().await;
    Ok(())
}

async fn write_all(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    tokio::time::timeout(STALLED_FOR, stream.write_all(bytes))
        .await
        .unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into()))
}

async fn refuse(stream: &mut TcpStream, status: &str, extra: &str) -> std::io::Result<()> {
    let out = format!("HTTP/1.1 {status}\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n");
    write_all(stream, out.as_bytes()).await?;
    let _ = stream.shutdown().await;
    Ok(())
}

/// The request's head, up to its blank line.
async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut got = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        got.extend_from_slice(buf.get(..n)?);
        if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            return String::from_utf8(got.get(..end)?.to_vec()).ok();
        }
        if got.len() > MOST_HEAD {
            return None;
        }
    }
}

/// Equal without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// A file name as it goes in an address and in `filename*`: everything but
/// letters, digits and `-._~` as `%XX`.
fn encode(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_range_is_read_against_the_length() {
        assert_eq!(wanted(None, 100), Wanted::Whole);
        assert_eq!(wanted(Some("bytes=0-9"), 100), Wanted::Part(0, 9));
        assert_eq!(wanted(Some("bytes=90-"), 100), Wanted::Part(90, 99));
        assert_eq!(wanted(Some("bytes=-10"), 100), Wanted::Part(90, 99));
        assert_eq!(wanted(Some("bytes=-500"), 100), Wanted::Part(0, 99));
        assert_eq!(wanted(Some("bytes=50-5000"), 100), Wanted::Part(50, 99));
    }

    #[test]
    fn a_range_with_nothing_behind_it_is_not_satisfiable() {
        assert_eq!(wanted(Some("bytes=100-"), 100), Wanted::Beyond);
        assert_eq!(wanted(Some("bytes=200-300"), 100), Wanted::Beyond);
        assert_eq!(wanted(Some("bytes=9-3"), 100), Wanted::Beyond);
        assert_eq!(wanted(Some("bytes=-0"), 100), Wanted::Beyond);
        assert_eq!(wanted(Some("bytes=0-"), 0), Wanted::Beyond);
    }

    #[test]
    fn what_is_not_one_range_is_answered_whole() {
        assert_eq!(wanted(Some("bytes=0-1,5-6"), 100), Wanted::Whole);
        assert_eq!(wanted(Some("lines=0-1"), 100), Wanted::Whole);
        assert_eq!(wanted(Some("bytes=a-b"), 100), Wanted::Whole);
        assert_eq!(wanted(Some("bytes=5"), 100), Wanted::Whole);
    }

    #[test]
    fn a_name_goes_in_an_address_without_breaking_it() {
        assert_eq!(encode("a b/ü.zip"), "a%20b%2F%C3%BC.zip");
        assert_eq!(encode("plain-1_2.bin"), "plain-1_2.bin");
    }

    #[test]
    fn tokens_are_long_and_differ() {
        let (a, b) = (token().unwrap(), token().unwrap());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn comparing_tokens_notices_every_difference() {
        assert!(same("/ab/", "/ab/"));
        assert!(!same("/ab/", "/ac/"));
        assert!(!same("/ab/", "/abc/"));
    }
}
