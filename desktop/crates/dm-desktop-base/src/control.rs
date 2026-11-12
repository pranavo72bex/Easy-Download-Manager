// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A way for a test or an agent to drive the running window.
//!
//! JSON lines over TCP on 127.0.0.1, off unless dm is started with
//! `--control <port>` (0 picks one; the port is written to
//! `<dm home>/control.port`). The first line on a connection must be
//! `{"method":"auth","params":{"token":…}}` with the 64-hex-digit token from
//! `<dm home>/control.token`, created on first use and readable only by its
//! owner. Then each line is `{"id":…,"method":…,"params":{…}}` and each
//! reply `{"id":…,"ok":true,"result":…}` or `{"id":…,"ok":false,"error":…}`.
//!
//! Methods: `auth`, `batch`; `commands.list`, `command.run {id}`;
//! `queue.list`, `downloads.add`, `downloads.pause`, `downloads.resume`,
//! `downloads.cancel`, `downloads.speed_limit`; and the window's own `ui.*`:
//! `ui.inspect`, `ui.select {ids}`, `ui.place {place}`, `ui.texts`,
//! `ui.click`, `ui.type`, `ui.key`, `ui.scroll`, `ui.windows`, `ui.window`,
//! `ui.move_window`, `ui.accessibility` and `ui.screenshot {name}` (a PNG
//! under `<dm home>/screenshots`).
//!
//! The token is a bearer credential with no per-method authority: keep the
//! file private and the protocol on loopback.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// What a window answers on the connection's own thread rather than its
/// own (`listen_reading`): given the method, its parameters and whether the
/// client is an AI assistant (`dm-desktop mcp`), the answer, or `None` to
/// hand the request to the window as any other. For what only reads: a
/// script asking ten times a second for the list woke the window and drew
/// it each time, and waited behind whatever the window was drawing.
pub type Reads = Arc<dyn Fn(&str, &Value, bool) -> Option<Result<Value, String>> + Send + Sync>;

/// One request, handed to the window to answer on its own thread.
pub struct Request {
    /// Sent by `dm-desktop mcp` (it says so when it authenticates), so the
    /// window can refuse it while Options says AI assistants are off.
    pub via_mcp: bool,
    /// The command's name, as the control port received it.
    pub method: String,
    /// The command's arguments.
    pub params: Value,
    /// Where the answer, or the reason the command failed, is sent back.
    pub reply: mpsc::Sender<Result<Value, String>>,
}

const MAX_LINE: u64 = 1024 * 1024;
/// The first line, before the token: an `auth` request is a few dozen bytes.
const MAX_AUTH_LINE: u64 = 4096;
/// Authenticated connections at once.
const MAX_CONNECTIONS: usize = 16;
/// Connections not yet authenticated at once: past this, the oldest is
/// closed to make room, so a newcomer — the owner, with the token — is
/// never the one turned away.
const MAX_WAITING: usize = 16;
/// How long a connection has to present the token.
const AUTH_WITHIN: Duration = Duration::from_secs(5);

/// How long the closing thread sleeps while no connection waits: it is woken
/// as one comes, so this only bounds how long it outlives the port.
const IDLE_LOOK: Duration = Duration::from_secs(60);
const ANSWER_WITHIN: Duration = Duration::from_secs(60);

/// Set as dm quits: a request still waiting is told so, not left to time
/// out with "the window did not answer in time".
static CLOSING: AtomicBool = AtomicBool::new(false);

/// Says dm is quitting, to every connection.
pub fn closing() {
    CLOSING.store(true, Ordering::SeqCst);
}

/// A new token, written over the old one: made at every start of dm's own
/// control port, so a token a stale `control.port` once let a stranger read
/// opens nothing at the next start.
pub fn fresh_token(home: &Path) -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(home)?;
    // A port file a crash left names a port anyone on this computer may now hold: gone before
    // the new token is written, so a client never reads the new token beside the old port and
    // hands it to whoever holds it (security audit, 2026-11-12). The port is written again once
    // it is bound (`write_port`); a client reads the token first (`mcp::finder`). This dm is the
    // only one (`dm_shell::instance`), so the file is no other's.
    match std::fs::remove_file(home.join("control.port")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    dm_core::write_atomic(&home.join("control.token"), token.as_bytes())?;
    Ok(token)
}

/// The token a client must present, made on first use.
pub fn token(home: &Path) -> std::io::Result<String> {
    let path = home.join("control.token");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim().to_string();
        if text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit()) {
            // One written looser by an older dm, or by hand, is tightened.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            return Ok(text);
        }
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(home)?;
    // Created readable by this account alone, not made so afterwards: there
    // was a moment when another user could open it.
    dm_core::write_atomic(&path, token.as_bytes())?;
    Ok(token)
}

/// Connections that have not presented the token yet, oldest first, each
/// with when it came and a handle to close it by.
///
/// They used to share the sixteen slots with everyone, held for the
/// five-minute read timeout: sixteen idle connections from any program on
/// the machine kept the owner out, and a line that was not JSON was
/// answered and the connection kept.
#[derive(Default)]
struct Waiting {
    next: u64,
    list: std::collections::VecDeque<(u64, Instant, TcpStream)>,
}

impl Waiting {
    /// Takes in a new connection, closing the oldest waiting one if full.
    fn admit(&mut self, stream: &TcpStream) -> Option<u64> {
        let handle = stream.try_clone().ok()?;
        while self.list.len() >= MAX_WAITING {
            if let Some((_, _, oldest)) = self.list.pop_front() {
                let _ = oldest.shutdown(std::net::Shutdown::Both);
            }
        }
        self.next += 1;
        self.list.push_back((self.next, Instant::now(), handle));
        Some(self.next)
    }

    /// No longer waiting: authenticated, refused or gone.
    fn leave(&mut self, id: u64) {
        self.list.retain(|(each, _, _)| *each != id);
    }

    /// How long until the oldest waiting connection's time is up; none while
    /// none waits.
    fn next_due(&self) -> Option<Duration> {
        self.list
            .front()
            .map(|(_, came, _)| (*came + AUTH_WITHIN).saturating_duration_since(Instant::now()))
    }

    /// Closes every connection that has waited past `AUTH_WITHIN`, however
    /// slowly it is still sending.
    fn expire(&mut self) {
        let now = Instant::now();
        self.list.retain(|(_, came, stream)| {
            let keep = now.duration_since(*came) < AUTH_WITHIN;
            if !keep {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            keep
        });
    }
}

/// Listens on loopback and hands each authenticated request to `to_window`,
/// calling `wake` so a hidden window still answers. Returns the port.
pub fn listen(
    port: u16,
    token: String,
    to_window: mpsc::Sender<Request>,
    wake: impl Fn() + Send + Sync + 'static,
) -> std::io::Result<u16> {
    listen_reading(port, token, to_window, wake, None)
}

/// As [`listen`], with what `reads` answers answered on the connection's
/// own thread, the window neither woken nor waited for.
pub fn listen_reading(
    port: u16,
    token: String,
    to_window: mpsc::Sender<Request>,
    wake: impl Fn() + Send + Sync + 'static,
    reads: Option<Reads>,
) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    let wake = Arc::new(wake);
    let open = Arc::new(AtomicUsize::new(0));
    let waiting = Arc::new(Mutex::new(Waiting::default()));
    // Rung as a connection is taken in: the thread that closes those that
    // wait too long sleeps until the oldest one's time is up, or for good
    // while none waits, rather than looking four times a second for the
    // life of dm (gaps row S18).
    let arrived = Arc::new(std::sync::Condvar::new());
    {
        let waiting = Arc::downgrade(&waiting);
        let arrived = Arc::clone(&arrived);
        std::thread::spawn(move || {
            while let Some(held) = waiting.upgrade() {
                let mut list = dm_core::lock(&held);
                list.expire();
                let due = list.next_due().unwrap_or(IDLE_LOOK);
                let _ = arrived.wait_timeout(list, due);
            }
        });
    }
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Some(id) = dm_core::lock(&waiting).admit(&stream) else {
                continue;
            };
            arrived.notify_one();
            let (token, to_window, wake, open, waiting, reads) = (
                token.clone(),
                to_window.clone(),
                Arc::clone(&wake),
                Arc::clone(&open),
                Arc::clone(&waiting),
                reads.clone(),
            );
            std::thread::spawn(move || {
                let connection = Connection {
                    id,
                    open: &open,
                    waiting: &waiting,
                    counted: false,
                };
                let window = Window {
                    to_window: &to_window,
                    wake: &*wake,
                    reads: reads.as_ref(),
                };
                let _ = serve(stream, &token, window, connection);
            });
        }
    });
    Ok(port)
}

/// Where a connection's requests go: what `reads` answers on the
/// connection's thread, the rest to the window, woken for each.
struct Window<'a> {
    to_window: &'a mpsc::Sender<Request>,
    wake: &'a (dyn Fn() + Send + Sync),
    reads: Option<&'a Reads>,
}

/// One connection's place in the counts, given back however it ends.
struct Connection<'a> {
    id: u64,
    open: &'a AtomicUsize,
    waiting: &'a Mutex<Waiting>,
    counted: bool,
}

impl Connection<'_> {
    /// Out of the waiting list and into a slot, if there is one.
    fn authenticated(&mut self) -> bool {
        dm_core::lock(self.waiting).leave(self.id);
        // A compare-and-swap loop rather than `fetch_update`, which newer
        // compilers have renamed (and warn about) and older ones lack the
        // new name of.
        let mut n = self.open.load(Ordering::SeqCst);
        let taken = loop {
            if n >= MAX_CONNECTIONS {
                break false;
            }
            match self
                .open
                .compare_exchange(n, n + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => break true,
                Err(now) => n = now,
            }
        };
        self.counted = taken;
        taken
    }
}

impl Drop for Connection<'_> {
    fn drop(&mut self) {
        dm_core::lock(self.waiting).leave(self.id);
        if self.counted {
            self.open.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn serve(
    stream: TcpStream,
    token: &str,
    window: Window<'_>,
    mut connection: Connection<'_>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(AUTH_WITHIN))?;
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut authenticated = false;
    let mut via_mcp = false;
    loop {
        let mut line = String::new();
        let limit = if authenticated {
            MAX_LINE
        } else {
            MAX_AUTH_LINE
        };
        let read = (&mut reader).take(limit).read_line(&mut line)?;
        if read == 0 {
            return Ok(());
        }
        if !line.ends_with('\n') && read as u64 >= limit {
            write_reply(
                &mut out,
                &Value::Null,
                Err(if authenticated {
                    "a request may be at most 1 MiB".into()
                } else {
                    "authenticate first".into()
                }),
            )?;
            return Ok(());
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            write_reply(&mut out, &Value::Null, Err("that is not JSON".into()))?;
            if authenticated {
                continue;
            }
            // Before the token, nothing but the token is answered twice.
            return Ok(());
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        if !authenticated {
            let given = params.get("token").and_then(Value::as_str).unwrap_or("");
            if method == "auth" && constant_time_eq(given.as_bytes(), token.as_bytes()) {
                via_mcp = params.get("client").and_then(Value::as_str) == Some("mcp");
                if !connection.authenticated() {
                    write_reply(
                        &mut out,
                        &id,
                        Err(format!(
                            "dm takes {MAX_CONNECTIONS} connections at once; close one first"
                        )),
                    )?;
                    return Ok(());
                }
                authenticated = true;
                // On the reader's own socket: on Windows a clone does not carry the
                // option back, and a connection idle for the authentication limit was cut.
                reader
                    .get_ref()
                    .set_read_timeout(Some(Duration::from_secs(300)))?;
                out.set_read_timeout(Some(Duration::from_secs(300)))?;
                write_reply(&mut out, &id, Ok(json!({ "authenticated": true })))?;
                continue;
            }
            // Nothing else is answered, and the connection ends.
            write_reply(&mut out, &id, Err("authenticate first".into()))?;
            return Ok(());
        }

        if CLOSING.load(Ordering::SeqCst) {
            write_reply(&mut out, &id, Err("dm is closing".into()))?;
            return Ok(());
        }
        // What only reads, answered here: the window is not woken for it.
        if let Some(answer) = window
            .reads
            .and_then(|reads| reads(&method, &params, via_mcp))
        {
            write_reply(&mut out, &id, answer)?;
            continue;
        }
        let (reply, answered) = mpsc::channel();
        if window
            .to_window
            .send(Request {
                via_mcp,
                method,
                params,
                reply,
            })
            .is_err()
        {
            write_reply(&mut out, &id, Err("dm is closing".into()))?;
            return Ok(());
        }
        (window.wake)();
        let answer = wait_for(&answered, &CLOSING, ANSWER_WITHIN);
        write_reply(&mut out, &id, answer)?;
    }
}

/// The window's answer; or, when dm quits first, that it is closing.
fn wait_for(
    answered: &mpsc::Receiver<Result<Value, String>>,
    closing: &AtomicBool,
    within: Duration,
) -> Result<Value, String> {
    let until = Instant::now() + within;
    loop {
        match answered.recv_timeout(Duration::from_millis(100)) {
            Ok(answer) => return answer,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("dm is closing".into()),
            Err(mpsc::RecvTimeoutError::Timeout) if closing.load(Ordering::SeqCst) => {
                return Err("dm is closing".into())
            }
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= until => {
                return Err("the window did not answer in time".into())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn write_reply(
    out: &mut TcpStream,
    id: &Value,
    answer: Result<Value, String>,
) -> std::io::Result<()> {
    let reply = match answer {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(error) => json!({ "id": id, "ok": false, "error": error }),
    };
    let mut text = reply.to_string();
    text.push('\n');
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// Compares without stopping at the first difference, so the time a wrong
/// guess takes says nothing about how much of it was right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The port this dm wrote to `control.port`, if it wrote one.
static WRITTEN: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// Says where the protocol is listening, for `dm mcp` and scripts to find.
pub fn write_port(home: &Path, port: u16) -> std::io::Result<()> {
    std::fs::write(home.join("control.port"), port.to_string())?;
    WRITTEN.store(port, Ordering::Relaxed);
    Ok(())
}

/// Takes `control.port` away as dm quits, so `dm mcp` run later is not
/// sent to a port nobody listens on — or that something else has since
/// taken. Only the port this dm wrote: one another dm wrote since is its.
pub fn forget_port(home: &Path) {
    let port = WRITTEN.swap(0, Ordering::Relaxed);
    let file = home.join("control.port");
    let ours = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .is_some_and(|written| port != 0 && written == port);
    if ours {
        let _ = std::fs::remove_file(file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new token takes away the port file a crash left, so the two are never read as a pair
    /// that sends the new token to an old port (security audit, 2026-11-12).
    #[test]
    fn a_new_token_takes_away_a_port_file_left_behind() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("control.port"), "41234").unwrap();
        let token = fresh_token(home.path()).unwrap();
        assert!(!home.path().join("control.port").exists());
        assert_eq!(
            std::fs::read_to_string(home.path().join("control.token")).unwrap(),
            token
        );
        // And with none there, as at a first start.
        fresh_token(home.path()).unwrap();
    }

    #[test]
    fn the_port_file_goes_as_dm_quits_unless_another_dm_wrote_it() {
        let home = tempfile::tempdir().unwrap();
        write_port(home.path(), 4711).unwrap();
        forget_port(home.path());
        assert!(!home.path().join("control.port").exists());

        write_port(home.path(), 4711).unwrap();
        std::fs::write(home.path().join("control.port"), "4712").unwrap();
        forget_port(home.path());
        assert!(home.path().join("control.port").exists(), "another dm's");
    }

    #[test]
    fn a_token_is_made_once_kept_and_private() {
        let home = tempfile::tempdir().unwrap();
        let first = token(home.path()).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(
            token(home.path()).unwrap(),
            first,
            "a second start made a new token"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.path().join("control.token"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn nothing_is_answered_before_the_token() {
        let (to_window, from_net) = mpsc::channel();
        let port = listen(0, "a".repeat(64), to_window, || {}).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(b"{\"id\":1,\"method\":\"commands.list\"}\n")
            .unwrap();
        let mut reply = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut reply)
            .unwrap();
        assert!(reply.contains("authenticate first"), "{reply}");
        assert!(
            from_net.try_recv().is_err(),
            "a request got through unauthenticated"
        );

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(b"{\"id\":1,\"method\":\"auth\",\"params\":{\"token\":\"wrong\"}}\n")
            .unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        assert!(reply.contains("\"ok\":false"), "{reply}");
    }

    fn auth(port: u16, token: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let line = json!({ "id": 1, "method": "auth", "params": { "token": token } });
        stream.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        let _ = BufReader::new(stream).read_line(&mut reply);
        reply
    }

    #[test]
    fn idle_connections_without_the_token_do_not_keep_the_owner_out() {
        /* Sixteen connections that said nothing held every slot for the
         * five-minute read timeout, and the owner's was closed unanswered. */
        let token = "b".repeat(64);
        let (to_window, _from_net) = mpsc::channel();
        let port = listen(0, token.clone(), to_window, || {}).unwrap();
        let idle: Vec<TcpStream> = (0..MAX_WAITING + 4)
            .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        let reply = auth(port, &token);
        assert!(reply.contains("\"authenticated\":true"), "{reply:?}");
        drop(idle);
    }

    #[test]
    fn a_connection_that_does_not_authenticate_in_time_is_closed() {
        let (to_window, _from_net) = mpsc::channel();
        let port = listen(0, "c".repeat(64), to_window, || {}).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(AUTH_WITHIN * 3)).unwrap();
        let started = Instant::now();
        // A byte now and then, never a whole line: each read is quick, so
        // only a deadline for the whole of it ends this.
        let mut writer = stream.try_clone().unwrap();
        std::thread::spawn(move || {
            for _ in 0..40 {
                if writer.write_all(b"{").is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
        let took = started.elapsed();
        assert!(
            took < AUTH_WITHIN + Duration::from_secs(3),
            "held for {took:?}"
        );
    }

    #[test]
    fn a_line_that_is_not_json_before_the_token_ends_the_connection() {
        let (to_window, _from_net) = mpsc::channel();
        let port = listen(0, "d".repeat(64), to_window, || {}).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(b"garbage\n").unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut reply = String::new();
        reader.read_line(&mut reply).unwrap();
        assert!(reply.contains("not JSON"), "{reply}");
        let _ = stream.write_all(b"garbage\n");
        let mut more = String::new();
        assert_eq!(
            reader.read_line(&mut more).unwrap_or(0),
            0,
            "answered again: {more}"
        );
    }

    /// What only reads is answered on the connection's thread, the window
    /// neither woken nor handed it; anything else still goes to the window.
    #[test]
    fn a_reading_is_answered_on_the_port_without_the_window() {
        let token = "c".repeat(64);
        let (to_window, from_net) = mpsc::channel();
        let woken = Arc::new(AtomicUsize::new(0));
        let reads: Reads =
            Arc::new(|method, _, _| (method == "queue.list").then(|| Ok(json!([{ "id": 7 }]))));
        let port = {
            let woken = Arc::clone(&woken);
            listen_reading(
                0,
                token.clone(),
                to_window,
                move || {
                    woken.fetch_add(1, Ordering::SeqCst);
                },
                Some(reads),
            )
            .unwrap()
        };
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut replies = BufReader::new(stream.try_clone().unwrap());
        let mut ask = |line: Value| {
            stream.write_all(format!("{line}\n").as_bytes()).unwrap();
            let mut reply = String::new();
            replies.read_line(&mut reply).unwrap();
            reply
        };
        let said = ask(json!({ "id": 1, "method": "auth", "params": { "token": token } }));
        assert!(said.contains("\"ok\":true"), "{said}");

        let said = ask(json!({ "id": 2, "method": "queue.list" }));
        assert!(said.contains("\"id\":7"), "{said}");
        assert_eq!(woken.load(Ordering::SeqCst), 0, "the window was woken");
        assert!(from_net.try_recv().is_err(), "the window was handed it");

        // Anything else: the window's, woken for it.
        let window = std::thread::spawn(move || {
            let request = from_net.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(request.method, "commands.list");
            request.reply.send(Ok(json!("the window's"))).unwrap();
        });
        let said = ask(json!({ "id": 3, "method": "commands.list" }));
        window.join().unwrap();
        assert!(said.contains("the window's"), "{said}");
        assert_eq!(woken.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_request_waiting_as_dm_quits_is_told_dm_is_closing() {
        let (_reply, answered) = mpsc::channel();
        let closing = AtomicBool::new(true);
        let started = Instant::now();
        let answer = wait_for(&answered, &closing, Duration::from_secs(30));
        assert_eq!(answer, Err("dm is closing".to_string()));
        assert!(started.elapsed() < Duration::from_secs(5));

        // And when the window that would have answered is gone.
        let (reply, answered) = mpsc::channel::<Result<Value, String>>();
        drop(reply);
        let closing = AtomicBool::new(false);
        let answer = wait_for(&answered, &closing, Duration::from_secs(30));
        assert_eq!(answer, Err("dm is closing".to_string()));
    }
}
