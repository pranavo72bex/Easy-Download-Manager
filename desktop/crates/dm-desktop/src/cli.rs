//! dm's commands from a terminal: what is downloading, and pausing,
//! resuming or cancelling it.
//!
//! ```text
//! dm-desktop list | status ID | pause ID… | resume ID… | cancel ID…  [--json]
//! ```
//!
//! This is a client of the running dm's control protocol (the one `mcp`
//! uses), so there is one download engine and nothing new listens: dm has to
//! be running with `--control 0`, and the token that protocol wants is the
//! file only its owner can read. Nothing printed holds a sign-in, a cookie
//! or a referrer: a row is shown as its id, state, name, size, speed,
//! address (without any login in it) and where it was saved.
//!
//! Exit codes: 0 done, 1 dm refused or an id was not acted on, 2 used
//! wrongly, 3 no dm to talk to.

use std::path::Path;

use serde_json::{json, Value};

use crate::mcp::{finder, Control};

/// What was asked for.
#[derive(Debug, PartialEq)]
pub enum Ask {
    List,
    Status(u64),
    Act(&'static str, Vec<u64>),
}

/// The words that start this module's commands.
pub fn is_command(word: &str) -> bool {
    matches!(word, "list" | "status" | "pause" | "resume" | "cancel")
}

/// The command, whether `--json` was given, and the port and token file if
/// named, from the arguments after the program name.
pub fn parse(args: &[String]) -> Result<(Ask, bool, Option<String>, Option<String>), String> {
    let (mut json, mut port, mut token) = (false, None, None);
    let mut words = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--port" => port = Some(it.next().ok_or("--port needs a number")?.clone()),
            "--token-file" => token = Some(it.next().ok_or("--token-file needs a path")?.clone()),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => words.push(other.to_string()),
        }
    }
    let mut words = words.into_iter();
    let verb = words
        .next()
        .ok_or("say what to do: list, status, pause, resume or cancel")?;
    let ids = words
        .map(|w| {
            w.parse::<u64>()
                .map_err(|_| format!("{w:?} is not a download number (see `list`)"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // `pause 1 1` is one download paused once.
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<u64> = ids.into_iter().filter(|id| seen.insert(*id)).collect();
    let ask = match (verb.as_str(), ids.as_slice()) {
        ("list", []) => Ask::List,
        ("list", _) => return Err("list takes no download numbers".into()),
        ("status", [one]) => Ask::Status(*one),
        ("status", _) => return Err("status takes one download number".into()),
        ("pause" | "resume" | "cancel", []) => {
            return Err(format!("{verb} needs a download number (see `list`)"))
        }
        ("pause", _) => Ask::Act("downloads.pause", ids),
        ("resume", _) => Ask::Act("downloads.resume", ids),
        ("cancel", _) => Ask::Act("downloads.cancel", ids),
        (other, _) => return Err(format!("{other:?} is not a command")),
    };
    Ok((ask, json, port, token))
}

/// A row as printed: only what is safe to hand to a script.
fn brief(row: &Value) -> Value {
    let get = |k: &str| row.get(k).cloned().unwrap_or(Value::Null);
    let progress = row.get("progress");
    let of = |k: &str| {
        progress
            .and_then(|p| p.get(k))
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({
        "id": get("id"),
        "state": row.pointer("/state/kind").cloned().unwrap_or(Value::Null),
        "reason": row.pointer("/state/reason").cloned().unwrap_or(Value::Null),
        "name": get("filename"),
        "downloaded": of("downloaded"),
        "total": of("total"),
        "speed": of("speed"),
        "url": get("url"),
        "path": get("path"),
    })
}

fn size(n: &Value) -> String {
    match n.as_u64() {
        Some(b) if b >= 1 << 30 => format!("{:.1} GiB", b as f64 / (1u64 << 30) as f64),
        Some(b) if b >= 1 << 20 => format!("{:.1} MiB", b as f64 / (1u64 << 20) as f64),
        Some(b) if b >= 1 << 10 => format!("{:.1} KiB", b as f64 / 1024.0),
        Some(b) => format!("{b} B"),
        None => "?".into(),
    }
}

/// One row as a line of text.
fn line(row: &Value) -> String {
    let text = |k: &str| row.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut state = text("state");
    if !text("reason").is_empty() {
        state = format!("{state} ({})", text("reason"));
    }
    let name = match text("name") {
        n if n.is_empty() => text("url"),
        n => n,
    };
    let (done, total) = (
        row.get("downloaded").and_then(Value::as_u64),
        row.get("total").and_then(Value::as_u64),
    );
    let percent = match (done, total) {
        (Some(d), Some(t)) if t > 0 => format!("{}%", (d.min(t) * 100 / t)),
        _ => String::new(),
    };
    let speed = match row.get("speed").and_then(Value::as_f64) {
        Some(s) if text("state") == "running" && s >= 1.0 => {
            format!("{}/s", size(&json!(s as u64)))
        }
        _ => String::new(),
    };
    format!(
        "{:>5}  {:<10}  {:>10} / {:<10} {:>4}  {:>11}  {}",
        row.get("id").and_then(Value::as_u64).unwrap_or(0),
        state,
        size(row.get("downloaded").unwrap_or(&Value::Null)),
        size(row.get("total").unwrap_or(&Value::Null)),
        percent,
        speed,
        name
    )
}

/// Every row, as [`brief`] shows it.
fn rows(control: &mut Control) -> Result<Vec<Value>, String> {
    let all = control.call("queue.list", json!({}))?;
    Ok(all
        .as_array()
        .map(|a| a.iter().map(brief).collect())
        .unwrap_or_default())
}

/// Runs a command against `control`, printing to `out`. The exit code.
pub fn run_with(
    ask: &Ask,
    json_out: bool,
    control: &mut Control,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let mut put = |rows: &[Value]| {
        if json_out {
            let _ = writeln!(out, "{}", Value::Array(rows.to_vec()));
        } else {
            for row in rows {
                let _ = writeln!(out, "{}", line(row));
            }
        }
    };
    match ask {
        Ask::List => match rows(control) {
            Ok(rows) => {
                put(&rows);
                if rows.is_empty() && !json_out {
                    let _ = writeln!(err, "dm: no downloads");
                }
                0
            }
            Err(e) => {
                let _ = writeln!(err, "dm: {e}");
                1
            }
        },
        Ask::Status(id) => match rows(control) {
            Ok(rows) => match rows.iter().find(|r| r.get("id") == Some(&json!(id))) {
                Some(row) => {
                    put(std::slice::from_ref(row));
                    0
                }
                None => {
                    let _ = writeln!(err, "dm: no download is numbered {id}");
                    1
                }
            },
            Err(e) => {
                let _ = writeln!(err, "dm: {e}");
                1
            }
        },
        Ask::Act(method, ids) => match control.call(method, json!({ "ids": ids })) {
            Ok(answer) => {
                let skipped = answer
                    .get("skipped")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if json_out {
                    let _ = writeln!(out, "{answer}");
                }
                for s in &skipped {
                    let _ = writeln!(
                        err,
                        "dm: {} not done: {}",
                        s.get("id").and_then(Value::as_u64).unwrap_or(0),
                        s.get("why").and_then(Value::as_str).unwrap_or("refused")
                    );
                }
                if !json_out {
                    let n = answer
                        .get("done")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    let _ = writeln!(out, "done for {n}");
                }
                i32::from(!skipped.is_empty())
            }
            Err(e) => {
                let _ = writeln!(err, "dm: {e}");
                1
            }
        },
    }
}

const USAGE: &str = "\
Usage: dm-desktop list | status ID | pause ID... | resume ID... | cancel ID...
                  [--json] [--port PORT] [--token-file FILE]

Talks to the dm that is running (started with --control 0, or with Options >
General > Let the command line... on). Numbers are the ones `list` shows.
Exit: 0 done, 1 refused or not acted on, 2 used wrongly, 3 no dm running.
";

/// `dm-desktop list|status|pause|resume|cancel …`: the exit code.
pub fn main(args: &[String], home: &Path) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return 0;
    }
    let (ask, json_out, port, token) = match parse(args) {
        Ok(parsed) => parsed,
        Err(why) => {
            eprintln!("dm: {why}");
            return 2;
        }
    };
    let mut control = match Control::with(finder(home, port, token)) {
        Ok(control) => control,
        Err(why) => {
            eprintln!("dm: {why}");
            return 3;
        }
    };
    run_with(
        &ask,
        json_out,
        &mut control,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn commands_and_their_numbers_are_read_and_misuse_is_said() {
        let ok = |s: &str| parse(&words(s)).unwrap().0;
        assert_eq!(ok("list"), Ask::List);
        assert_eq!(ok("status 7"), Ask::Status(7));
        assert_eq!(ok("pause 1 2"), Ask::Act("downloads.pause", vec![1, 2]));
        assert_eq!(ok("--json cancel 3"), Ask::Act("downloads.cancel", vec![3]));
        assert!(parse(&words("--json list")).unwrap().1);
        for bad in [
            "",
            "pause",
            "status",
            "status 1 2",
            "list 4",
            "pause x",
            "fly 1",
            "list --what",
        ] {
            assert!(parse(&words(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_row_shows_no_cookie_referrer_or_note_even_if_dm_sent_them() {
        let row = json!({
            "id": 4, "url": "https://h/f.bin", "filename": "f.bin", "path": "/d/f.bin",
            "state": { "kind": "failed", "reason": "connection reset" },
            "progress": { "downloaded": 1536, "total": 4096, "speed": 0.0 },
            "cookies": "session=SECRET", "referrer": "https://secret/page",
            "description": "private note", "authorization": "Bearer SECRET"
        });
        let shown = brief(&row).to_string();
        for secret in ["SECRET", "secret", "private"] {
            assert!(!shown.contains(secret), "{shown}");
        }
        assert!(line(&brief(&row)).contains("failed (connection reset)"));
        assert!(line(&brief(&row)).contains("1.5 KiB"));
        assert!(line(&brief(&row)).contains(" 37%"), "{}", line(&brief(&row)));
        let running = json!({
            "id": 5, "url": "u", "filename": "g.bin", "state": { "kind": "running" },
            "progress": { "downloaded": 1 << 20, "total": 4 << 20, "speed": 2048.0 }
        });
        assert!(line(&brief(&running)).contains("25%  "), "{}", line(&brief(&running)));
        assert!(line(&brief(&running)).contains("2.0 KiB/s"));
        assert_eq!(parse(&words("pause 3 3 4 3")).unwrap().0, Ask::Act("downloads.pause", vec![3, 4]));
    }

    /// A dm that answers `queue.list` with one row and records the rest.
    fn fake_dm() -> (u16, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (send, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            loop {
                let mut l = String::new();
                if reader.read_line(&mut l).unwrap_or(0) == 0 {
                    return;
                }
                let m: Value = serde_json::from_str(&l).unwrap();
                let method = m["method"].as_str().unwrap().to_string();
                let _ = send.send(l.trim().to_string());
                let result = match method.as_str() {
                    "queue.list" => json!([{ "id": 1, "url": "https://h/a", "filename": "a",
                        "state": { "kind": "running" },
                        "progress": { "downloaded": 10, "total": 20, "speed": 5.0 },
                        "path": "" }]),
                    "downloads.pause" => {
                        json!({ "done": [1], "skipped": [{ "id": 9, "why": "no such download" }] })
                    }
                    _ => json!({}),
                };
                writeln!(out, "{}", json!({ "id": 1, "ok": true, "result": result })).unwrap();
            }
        });
        (port, got)
    }

    #[test]
    fn list_status_and_pause_go_through_the_control_protocol_with_exit_codes() {
        let (port, got) = fake_dm();
        let mut control = Control::connect(port, "t").unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            run_with(&Ask::List, true, &mut control, &mut out, &mut err),
            0
        );
        let listed: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(listed[0]["state"], "running");
        out.clear();
        assert_eq!(
            run_with(&Ask::Status(2), false, &mut control, &mut out, &mut err),
            1
        );
        assert!(String::from_utf8_lossy(&err).contains("no download is numbered 2"));
        err.clear();
        let pause = Ask::Act("downloads.pause", vec![1, 9]);
        assert_eq!(run_with(&pause, false, &mut control, &mut out, &mut err), 1);
        assert!(String::from_utf8_lossy(&err).contains("9 not done: no such download"));
        let sent: Vec<String> = got.try_iter().collect();
        assert!(sent.iter().any(|l| l.contains("\"auth\"")));
        assert!(sent
            .iter()
            .any(|l| l.contains("downloads.pause") && l.contains("[1,9]")));
    }
}
