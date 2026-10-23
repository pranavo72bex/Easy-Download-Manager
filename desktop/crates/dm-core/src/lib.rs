//! dm's download engine.
//!
//! Deliberately free of any window. Everything a download manager is judged
//! on — several connections at once, resume, throughput, not corrupting the
//! file — lives here, where it can be tested against a real server in
//! milliseconds instead of driven through a user interface.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
pub mod auth;
pub mod category;
pub mod checksum;
pub mod control;
pub mod cookies;
pub mod download;
pub mod extract;
pub mod fmp4;
pub mod ftp;
pub mod limit;
pub mod live;
pub mod metalink;
pub mod mkvmux;
pub mod mms;
pub mod motw;
pub mod mp4mux;
pub mod ntlm;
pub mod plan;
pub mod probe;
pub mod progress;
pub mod proxy;
pub mod queue;
pub mod quota;
pub mod retry;
pub mod rtmp;
pub mod rtsp;
pub mod schedule;
pub mod stream;
pub mod subtitles;
/// The tests' logins and secrets, never written in the code.
#[cfg(test)]
#[path = "../../../test-support/secrets.rs"]
pub(crate) mod test_secrets;
pub mod tools;
pub mod torrent;
pub mod tsmux;
pub mod ytdlp;

/// The HTTP client crate, for callers that take or build a client of the
/// engine's kind without a dependency of their own.
pub use reqwest;

pub use category::{Categories, Category, Custom};
pub use checksum::Checksum;
pub use control::{Control, Wanted};
pub use download::{download, download_reporting, Finished, HostLimit, Options, Room, Update};
pub use limit::Limiter;
pub use metalink::Metalink;
pub use plan::Segment;
pub use probe::Probe;
pub use progress::Progress;
pub use proxy::Proxy;
pub use queue::{AddOptions, Connection, Entry, Id, Queue, State, MAIN_QUEUE, RECHECK_QUEUE};
pub use schedule::Schedule;
pub use stream::Stream;
pub use torrent::is_torrent;

/// Why a download stopped.
///
/// The variants are the distinctions worth acting on, not a catalogue of
/// everything that can go wrong: a refusal names what refused and what it
/// said, because "it failed" sends people looking in the wrong place.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the server answered {} for {url}{}", status_line(*.status), if .body.is_empty() { String::new() } else { format!(" — {body}") })]
    Refused {
        status: u16,
        url: String,
        body: String,
        /// What the server asked us to wait, where it said.
        retry_after: Option<std::time::Duration>,
    },

    /// The body ended before the range did. Usually a dropped connection,
    /// which is worth another attempt; a server that does it every time is
    /// caught by the length check instead.
    #[error("the connection ended after {got} of {expected} bytes")]
    Incomplete { got: u64, expected: u64 },

    /// The file was republished while it was being fetched. Everything staged
    /// belongs to the version that is gone.
    #[error("the file changed on the server, so the download has to start again")]
    Changed,

    /// A range came back longer than it was asked for, so the bytes are not
    /// the bytes of that range. Almost always a compressed representation
    /// ranged over its encoded length and served decoded.
    #[error("this server does not serve ranges honestly")]
    DishonestRanges,

    /// A range was answered with the whole file, and the file had not
    /// changed: the server stopped serving ranges for now. Wikimedia's
    /// caches do it to several connections at once and serve one connection
    /// its range. Not a changed file, so not reported as one.
    #[error("the server stopped serving parts of the file")]
    RangesIgnored,

    /// Stopped on request. Not a failure — the bytes so far are staged and a
    /// later attempt resumes from them.
    #[error("paused")]
    Paused,

    #[error("cancelled")]
    Cancelled,

    /// The parts add up to less than the length the server stated.
    #[error("the server sent {got} bytes of a file it said was {expected}")]
    ShortFile { got: u64, expected: u64 },

    /// The common causes in plain words, with what the system said after
    /// them; anything else with its causes, since reqwest's own message is
    /// "error sending request" and why is underneath it.
    #[error("{}", network(.0))]
    Network(#[from] reqwest::Error),

    /// A connection of dm's own (FTP's) dropped, timed out, or was told to
    /// try later. Worth another attempt, as a dropped HTTP request is.
    #[error("network: {0}")]
    Connection(String),

    #[error("disk: {0}")]
    Disk(#[from] std::io::Error),

    /// What arrived is not what was published. The only evidence from
    /// outside the transfer that the transfer was honest, so it names both
    /// digests rather than saying the file is bad.
    #[error("{algorithm} was {got}, but {expected} was published")]
    Checksum {
        algorithm: &'static str,
        expected: String,
        got: String,
    },

    /// A sentence written for the person reading the row. It used to be
    /// prefixed "internal:", which made a site that wants a sign-in read as
    /// a fault in dm.
    #[error("{0}")]
    Internal(String),
}

impl Error {
    /// The delay a refusal asked for, where it asked for one.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Error::Refused { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl Error {
    /// Passes a response through, or turns a refusal into an error.
    ///
    /// The predicate belongs next to the constructor. Left at the call sites
    /// it was written out six times, and the seventh fetch anybody adds is
    /// the one that writes a 404 page to disk and calls it a download.
    pub async fn refused_or(res: reqwest::Response) -> Result<reqwest::Response, Error> {
        let status = res.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(Error::from_response(res).await);
        }
        Ok(res)
    }

    /// Turns a refused response into an error that says what refused and why.
    ///
    /// One place, because both the probe and the range fetch need it and the
    /// second one used to build it by hand with an empty body — so a refusal
    /// that happened mid-download explained nothing at all.
    pub async fn from_response(mut res: reqwest::Response) -> Error {
        let status = res.status().as_u16();
        let url = res.url().to_string();
        // A refusal's body is either a short explanation or a whole error
        // page, whose sentence comes after its styles; it is read down to
        // that sentence rather than shown as markup.
        let retry_after = crate::retry::retry_after(res.headers());
        /* Read only as far as an explanation goes, and for no longer than
         * a person would wait for one. Read whole (and decompressed whole)
         * before it was cut down, a 500 whose body never ended, or a small
         * gzip that unpacks to gigabytes, ran dm out of memory. */
        const MOST: usize = 64 * 1024;
        let mut head = Vec::new();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while head.len() < MOST {
                match res.chunk().await {
                    Ok(Some(chunk)) => head.extend_from_slice(&chunk),
                    _ => break,
                }
            }
        })
        .await;
        drop(res);
        head.truncate(MOST);
        let body = worth_saying(
            &readable(
                &String::from_utf8_lossy(&head)
                    .chars()
                    .take(16_000)
                    .collect::<String>(),
            ),
            status,
        );
        Error::Refused {
            status,
            url,
            body,
            retry_after,
        }
    }
}

/// A status as a person reads it: "404 Not Found", not "404".
fn status_line(status: u16) -> String {
    match reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
    {
        Some(reason) => format!("{status} {reason}"),
        None => status.to_string(),
    }
}

/// What a refusal's body adds to its status, or nothing when it adds
/// nothing. Most error pages only restate the status in the server's
/// boilerplate: Python's "Error response Error code: 404 Message: File not
/// found. Error code explanation: …", nginx's "404 Not Found nginx/1.24",
/// Go's "404 page not found". A failed row read like a stack of them; it
/// now says "the server answered 404 Not Found", and keeps a body that
/// says something of its own ("Too many requests, please contact …").
fn worth_saying(body: &str, status: u16) -> String {
    // Python's http.server: boilerplate around a message, which is its own
    // ("Token expired") or a restatement ("File not found"). Only the
    // message is judged, and said.
    let body = match body.strip_prefix("Error response Error code:") {
        Some(rest) => {
            let message = rest.split_once("Message:").map_or("", |(_, m)| m);
            let message = message
                .split_once("Error code explanation:")
                .map_or(message, |(m, _)| m);
            message.trim().trim_end_matches('.').trim()
        }
        None => body,
    };
    let lower = body.to_ascii_lowercase();
    let reason = reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let code = status.to_string();
    let rest = lower.replace(&reason, " ").replace(&code, " ");
    const FILLER: [&str; 9] = [
        "error", "page", "response", "http", "status", "code", "the", "message", "file",
    ];
    // What is left once the status and its filler are gone; a server's
    // signature ("nginx/1.24.0") is not a reason either.
    let said = rest
        .split(|c: char| !c.is_alphanumeric() && c != '/' && c != '.')
        .map(|word| word.trim_matches('.'))
        .filter(|word| !FILLER.contains(word))
        .filter(|word| {
            !word
                .split_once('/')
                .is_some_and(|(_, version)| version.starts_with(|c: char| c.is_ascii_digit()))
        })
        .any(|word| word.chars().any(char::is_alphanumeric));
    if said {
        body.to_string()
    } else {
        String::new()
    }
}

/// A network failure as a person reads it. "network: error sending request
/// for url (…): client error (Connect): dns error: …" was what a mistyped
/// address showed; the cause is said first now, plainly, and the system's
/// own words for it after.
fn network(error: &reqwest::Error) -> String {
    let mut innermost = String::new();
    let mut refused = false;
    let mut cause: Option<&dyn std::error::Error> = Some(error);
    while let Some(next) = cause {
        if let Some(io) = next.downcast_ref::<std::io::Error>() {
            refused |= io.kind() == std::io::ErrorKind::ConnectionRefused;
        }
        let text = next.to_string();
        if !text.is_empty() {
            innermost = text;
        }
        cause = next.source();
    }
    let all = with_causes(error).to_ascii_lowercase();
    let plain = if all.contains("dns error")
        || all.contains("failed to lookup address")
        || all.contains("name or service not known")
        || all.contains("nodename nor servname")
        || all.contains("no such host")
    {
        "the server's name could not be found (check the address)"
    } else if refused || all.contains("connection refused") {
        "the server refused the connection"
    } else if error.is_timeout() || all.contains("timed out") {
        "the server did not answer in time"
    } else {
        return format!("network: {}", with_causes(error));
    };
    if innermost.is_empty() {
        plain.to_string()
    } else {
        format!("{plain}: {innermost}")
    }
}

/// An error and what caused it, each said once: "error sending request
/// for url (…): client error (Connect): tcp connect error: Connection
/// refused (os error 61)" rather than the first of those alone.
fn with_causes(error: &dyn std::error::Error) -> String {
    let mut said = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        let text = next.to_string();
        if !text.is_empty() && !said.contains(&text) {
            said.push_str(": ");
            said.push_str(&text);
        }
        cause = next.source();
    }
    said
}

/// What an error page says, as a sentence: the markup, its styles and
/// scripts dropped, its entities decoded, and the first two hundred
/// characters kept. Wikimedia's 403 put a page of CSS in front of "Too many
/// requests", and dm's row showed the CSS.
pub fn readable(body: &str) -> String {
    const MOST: usize = 200;
    let mut text = String::new();
    let lower = body.to_ascii_lowercase();
    let mut at = 0;
    while at < body.len() {
        let Some(rest) = body.get(at..) else { break };
        let Some(open) = rest.find('<') else {
            text.push_str(rest);
            break;
        };
        text.push_str(rest.get(..open).unwrap_or_default());
        let tag_at = at + open;
        let tag = lower.get(tag_at + 1..).unwrap_or_default();
        // A block whose content is not words is skipped to its end tag.
        let skipped = ["style", "script", "title", "head"]
            .iter()
            .find_map(|name| {
                let named = tag.starts_with(name)
                    && tag.get(name.len()..).is_some_and(|after| {
                        after.starts_with(|c: char| c == '>' || c.is_whitespace())
                    });
                named.then(|| {
                    lower
                        .get(tag_at..)
                        .and_then(|from| from.find(&format!("</{name}")))
                        .map(|end| tag_at + end)
                })
            });
        let from = match skipped {
            Some(Some(end)) => end,
            Some(None) => break,
            None => tag_at,
        };
        match body.get(from..).and_then(|from_here| from_here.find('>')) {
            Some(close) => {
                at = from + close + 1;
                text.push(' ');
            }
            None => break,
        }
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let words: Vec<&str> = decoded.split_whitespace().collect();
    let joined = words.join(" ");
    if joined.chars().count() <= MOST {
        return joined;
    }
    let cut: String = joined.chars().take(MOST).collect();
    format!("{}…", cut.trim_end())
}

/// Writes a file whole, or not at all.
///
/// Written to a sibling and moved, because the files dm keeps — the queue,
/// the settings, a request handed over by a browser and the rest — are read
/// back by something that must never see half of one.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, bytes, true)
}

/// As [`write_atomic`], without waiting for the disk: for a file whose
/// stale or empty copy after a power cut costs time but not correctness.
pub fn write_atomic_unsynced(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, bytes, false)
}

fn replace(path: &std::path::Path, bytes: &[u8], durable: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A sibling of its own per write: two saves at once shared one `.tmp`,
    // and whichever renamed second found it gone and reported a failure
    // that had not happened.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{n}.tmp", std::process::id()));
    let temp = path.with_file_name(name);
    /* On disk before it takes the name. Renamed with its bytes still in the
     * page cache, a power cut could leave the name on an empty or zeroed
     * file: the queue then unreadable, and every download in it forgotten. */
    let written = (|| {
        use std::io::Write;
        // Readable by this account alone: dm's files hold a proxy's
        // password, site logins and a site's handed-over session.
        let mut open = std::fs::OpenOptions::new();
        open.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
        let mut file = open.open(&temp)?;
        file.write_all(bytes)?;
        if durable {
            file.sync_all()?;
        }
        Ok(())
    })();
    if let Err(e) = written.and_then(|()| std::fs::rename(&temp, path)) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    // And the rename itself, which lives in the directory. Best effort:
    // not every system lets a directory be opened to sync it.
    #[cfg(unix)]
    if let Some(dir) = path
        .parent()
        .filter(|_| durable)
        .and_then(|p| std::fs::File::open(p).ok())
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// How every client this engine builds is configured.
///
/// One builder rather than two: the proxied client is the one that actually
/// runs downloads once a proxy is set, so anything added here and not there
/// would apply to everybody except the people who configured a proxy.
pub fn builder() -> reqwest::ClientBuilder {
    builder_with(READ_TIMEOUT, false)
}

/// How long a connection may send nothing before it is given up on, unless
/// a setting says otherwise. Long enough for a server assembling a file;
/// short enough that a pause is honoured within it.
pub const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// What a redirect is let do: ten at most, and none from `https` to
/// `http`. (reqwest itself drops a sign-in or cookies sent by dm on a
/// redirect to another server.)
pub fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let downgrade = attempt.url().scheme() == "http"
            && attempt
                .previous()
                .iter()
                .any(|before| before.scheme() == "https");
        if downgrade {
            let to = attempt.url().to_string();
            attempt.error(format!("{INSECURE_REDIRECT} ({to})"))
        } else if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

/// What a refused downgrade says.
pub const INSECURE_REDIRECT: &str =
    "the secure address sent dm on to an insecure (http) one, where the file could be swapped on the way, so dm stopped";

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
    let builder = reqwest::Client::builder();
    let builder = if system_proxy {
        builder
    } else {
        builder.no_proxy()
    };
    builder
        // Redirects are followed, but not indefinitely: a loop is a server
        // problem that should not become a hang. And never from a secure
        // address to a plain one, where anyone on the way could swap the
        // file, as browsers block such a download too.
        .redirect(redirect_policy())
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
        .read_timeout(read_timeout.max(std::time::Duration::from_secs(1)))
        .user_agent(USER_AGENT)
}

/// What dm says it is. Wikimedia, as its policy says, refuses an agent with
/// no way to reach whoever runs it: a bare `dm/0.1.0` got 403 for every
/// Wikipedia image, and the same agent with the project's address got the
/// file.
pub const USER_AGENT: &str = concat!(
    "dm/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/pranavo72bex/Easy-Download-Manager)"
);

/// dm's name alone. Some sites refuse an agent that names a code host as a
/// bot's (W3Schools refused anything with "github" in it, with 403), and
/// Wikimedia refuses this one; so a site that refuses [`USER_AGENT`] is
/// asked again with this, once, and if it answers it is asked with this
/// from then on.
pub const PLAIN_AGENT: &str = concat!("dm/", env!("CARGO_PKG_VERSION"));

/// The last agent asked with: a browser's, naming dm. A site that refuses
/// both of dm's own (W3Schools did on one network, though a browser there
/// played the same video) is asked once more as browsers ask.
pub const BROWSER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0 dm/",
    env!("CARGO_PKG_VERSION")
);

fn plain_hosts() -> &'static std::sync::Mutex<std::collections::HashMap<String, &'static str>> {
    static HOSTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, &'static str>>,
    > = std::sync::OnceLock::new();
    HOSTS.get_or_init(Default::default)
}

/// The server a URL names, host and port: two servers on one host are two
/// servers, each with its own answer about which agent it takes.
fn host_of(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// The agent a request to `url` goes out with: the one its site last
/// answered, else [`USER_AGENT`].
pub fn agent_for(url: &str) -> &'static str {
    host_of(url)
        .and_then(|host| lock(plain_hosts()).get(&host).copied())
        .unwrap_or(USER_AGENT)
}

/// The agents a refused probe tries, in order, after [`USER_AGENT`].
pub const FALLBACK_AGENTS: [&str; 2] = [PLAIN_AGENT, BROWSER_AGENT];

/// From now on, `url`'s site is asked with `agent`.
pub fn prefer_agent(url: &str, agent: &'static str) {
    if let Some(host) = host_of(url) {
        lock(plain_hosts()).insert(host, agent);
    }
}

/// On Windows, starts a helper program without a console window: dm's
/// release build has none of its own, so each `powershell`, `reg` or
/// yt-dlp it ran flashed one up. Nothing elsewhere.
pub fn no_window(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// [`no_window`] for tokio's commands.
pub fn no_window_async(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// A path as Windows programs read it: `canonicalize` there gives the
/// `\\?\C:\…` form, which a browser registered with it — and `cmd`, which
/// starts the host — can mishandle. A UNC share keeps its form.
pub fn plain_path(path: std::path::PathBuf) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => std::path::PathBuf::from(rest),
        _ => path,
    }
}

/// A lock that is taken even if a thread that held it panicked.
///
/// The data under dm's plain mutexes — the problem list, the schedule, the
/// window's settings — stays meaningful after a panic elsewhere, and a
/// second panic over a poisoned lock would turn one failure into a crash.
pub fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// What a file error means, in words a person reads: "No such file or
/// directory (os error 2)" said nothing a person could act on.
pub fn file_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "it is not there".into(),
        std::io::ErrorKind::PermissionDenied => "dm is not allowed to use it".into(),
        std::io::ErrorKind::AlreadyExists => "something of that name is already there".into(),
        _ => {
            // The system's words without its error number.
            let said = e.to_string();
            match said.rfind(" (os error ") {
                Some(at) => said[..at].to_string(),
                None => said,
            }
        }
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
            rustls::pki_types::PrivateKeyDer::try_from(made.key_pair.serialize_der()).unwrap();
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
        let client = reqwest::Client::builder()
            .redirect(super::redirect_policy())
            .add_root_certificate(reqwest::Certificate::from_der(&cert).unwrap())
            .build()
            .unwrap();
        let refused = client
            .get(format!("https://localhost:{port}/f.bin"))
            .send()
            .await
            .unwrap_err();
        let said = format!("{refused:?}");
        assert!(said.contains("insecure (http)"), "{said}");
        // The plain server was never asked.
        let asked =
            tokio::time::timeout(std::time::Duration::from_millis(200), plain.accept()).await;
        assert!(asked.is_err(), "the plain address was fetched");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_file_error_is_said_without_its_system_number() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            super::file_error(&Error::from(ErrorKind::NotFound)),
            "it is not there"
        );
        assert_eq!(
            super::file_error(&Error::from(ErrorKind::PermissionDenied)),
            "dm is not allowed to use it"
        );
        assert!(!super::file_error(&Error::from_raw_os_error(5)).contains("os error"));
    }

    use super::readable;

    #[tokio::test]
    async fn a_network_error_says_why_not_only_that_it_failed() {
        // A port nothing listens on: the refusal is the reason, and it is
        // under reqwest's "error sending request".
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let err = super::client()
            .get(format!("http://127.0.0.1:{port}/f"))
            .send()
            .await
            .unwrap_err();
        let said = super::Error::Network(err).to_string();
        assert!(said.to_ascii_lowercase().contains("refused"), "{said}");
    }

    #[test]
    fn an_error_page_is_read_down_to_its_sentence() {
        // Wikimedia's 403, abridged: a head of CSS, then the sentence.
        let page = "<!DOCTYPE html><html lang=\"en\"><meta charset=\"utf-8\">\
            <title>Wikimedia Error</title><style>* { margin: 0; } body { font: 15px }</style>\
            <div class=\"content\"><h1>Error</h1>\
            <p>Too many requests. Please contact noc@wikimedia.org &amp; say so</p></div>";
        assert_eq!(
            readable(page),
            "Error Too many requests. Please contact noc@wikimedia.org & say so"
        );
    }

    #[test]
    fn a_plain_refusal_stays_as_it_was_and_a_long_one_is_cut() {
        assert_eq!(
            readable("  rate limited,\n try later "),
            "rate limited, try later"
        );
        let long = "word ".repeat(100);
        let cut = readable(&long);
        assert!(cut.ends_with('…') && cut.chars().count() <= 201, "{cut}");
    }

    /// A failed row carried the server's error page —
    /// "Error response Error code: 404 Message: File not found. …" — where
    /// "404 Not Found" says it all. A body with a sentence of its own stays.
    #[test]
    fn an_error_page_that_only_restates_the_status_is_not_repeated() {
        let python = "<!DOCTYPE HTML><html lang=\"en\"><head><meta charset=\"utf-8\">\
            <title>Error response</title></head><body><h1>Error response</h1>\
            <p>Error code: 404</p><p>Message: File not found.</p>\
            <p>Error code explanation: 404 - Nothing matches the given URI.</p></body></html>";
        assert_eq!(super::worth_saying(&readable(python), 404), "");
        // A message of its own in the same page is kept.
        let own = python
            .replace("404", "401")
            .replace("File not found", "Token expired");
        assert_eq!(super::worth_saying(&readable(&own), 401), "Token expired");
        let nginx = "<html><head><title>404 Not Found</title></head><body>\
            <center><h1>404 Not Found</h1></center><hr><center>nginx/1.24.0</center></body></html>";
        assert_eq!(super::worth_saying(&readable(nginx), 404), "");
        assert_eq!(super::worth_saying("404 page not found\n", 404), "");
        assert_eq!(super::worth_saying("Forbidden", 403), "");
        assert_eq!(
            super::worth_saying("rate limited, try later", 429),
            "rate limited, try later"
        );
        let wikimedia = "Error Too many requests. Please contact noc@wikimedia.org";
        assert_eq!(super::worth_saying(wikimedia, 429), wikimedia);
        let refused = super::Error::Refused {
            status: 404,
            url: "http://e.com/a".into(),
            body: String::new(),
            retry_after: None,
        };
        assert_eq!(
            refused.to_string(),
            "the server answered 404 Not Found for http://e.com/a"
        );
    }

    #[test]
    fn markup_that_never_closes_ends_the_reading() {
        assert_eq!(readable("before <style> never closed"), "before");
        assert_eq!(readable("before <p never closed"), "before");
    }

    #[cfg(unix)]
    #[test]
    fn dms_files_are_readable_by_this_account_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("dm-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        super::write_atomic(&path, b"[]").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(mode, 0o600, "{mode:o}");
    }

    #[test]
    fn a_windows_long_path_reads_as_a_plain_one() {
        use std::path::PathBuf;
        assert_eq!(
            super::plain_path(PathBuf::from(r"\\?\C:\Program Files\dm\dm.exe")),
            PathBuf::from(r"C:\Program Files\dm\dm.exe")
        );
        assert_eq!(
            super::plain_path(PathBuf::from(r"\\?\UNC\server\share\dm.exe")),
            PathBuf::from(r"\\?\UNC\server\share\dm.exe"),
            "a share keeps its form"
        );
        assert_eq!(
            super::plain_path(PathBuf::from("/usr/bin/dm")),
            PathBuf::from("/usr/bin/dm")
        );
    }

    #[test]
    fn writes_to_one_file_at_once_all_succeed() {
        let dir = std::env::temp_dir().join(format!("dm-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        let writers: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        super::write_atomic(&path, format!("[{i}]").as_bytes()).unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().expect("a write failed");
        }
        let left = std::fs::read_dir(&dir).unwrap().count();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(left, 1, "temporary files were left beside the file");
    }
}
