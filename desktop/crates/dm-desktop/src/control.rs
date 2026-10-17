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
//! Methods: `commands.list`, `command.run {id}`, `ui.inspect`,
//! `ui.select {ids}`, `ui.place {place}`, `queue.list`,
//! `ui.screenshot {name}` (a PNG under `<dm home>/screenshots`).
//!
//! The token is a bearer credential with no per-method authority: keep the
//! file private and the protocol on loopback.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde_json::{json, Value};

/// One request, handed to the window to answer on its own thread.
pub struct Request {
    pub method: String,
    pub params: Value,
    pub reply: mpsc::Sender<Result<Value, String>>,
}

const MAX_LINE: u64 = 1024 * 1024;
const MAX_CONNECTIONS: usize = 16;
const ANSWER_WITHIN: Duration = Duration::from_secs(60);

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

/// Listens on loopback and hands each authenticated request to `to_window`,
/// calling `wake` so a hidden window still answers. Returns the port.
pub fn listen(
    port: u16,
    token: String,
    to_window: mpsc::Sender<Request>,
    wake: impl Fn() + Send + Sync + 'static,
) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    let wake = Arc::new(wake);
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                continue;
            }
            open.fetch_add(1, Ordering::SeqCst);
            let (token, to_window, wake, open) = (
                token.clone(),
                to_window.clone(),
                Arc::clone(&wake),
                Arc::clone(&open),
            );
            std::thread::spawn(move || {
                let _ = serve(stream, &token, &to_window, &*wake);
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(port)
}

fn serve(
    stream: TcpStream,
    token: &str,
    to_window: &mpsc::Sender<Request>,
    wake: &(dyn Fn() + Send + Sync),
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut authenticated = false;
    loop {
        let mut line = String::new();
        let read = (&mut reader).take(MAX_LINE).read_line(&mut line)?;
        if read == 0 {
            return Ok(());
        }
        if !line.ends_with('\n') && read as u64 >= MAX_LINE {
            write_reply(
                &mut out,
                &Value::Null,
                Err("a request may be at most 1 MiB".into()),
            )?;
            return Ok(());
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            write_reply(&mut out, &Value::Null, Err("that is not JSON".into()))?;
            continue;
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
                authenticated = true;
                write_reply(&mut out, &id, Ok(json!({ "authenticated": true })))?;
                continue;
            }
            // Nothing else is answered, and the connection ends.
            write_reply(&mut out, &id, Err("authenticate first".into()))?;
            return Ok(());
        }

        let (reply, answered) = mpsc::channel();
        if to_window
            .send(Request {
                method,
                params,
                reply,
            })
            .is_err()
        {
            write_reply(&mut out, &id, Err("dm is closing".into()))?;
            return Ok(());
        }
        wake();
        let answer = answered
            .recv_timeout(ANSWER_WITHIN)
            .unwrap_or_else(|_| Err("the window did not answer in time".into()));
        write_reply(&mut out, &id, answer)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
