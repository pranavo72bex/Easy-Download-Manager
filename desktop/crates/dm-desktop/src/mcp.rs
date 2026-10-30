//! A Model Context Protocol server, so an AI client can use dm as tools.
//!
//! ```text
//! dm-desktop mcp [--port N] [--token-file PATH]
//! ```
//!
//! It speaks MCP (JSON-RPC 2.0, one message a line) on stdin and stdout,
//! and relays each tool call to a running dm's control protocol: start dm
//! with `--control 0` (or `--headless --control 0`) first. The port and
//! token are read from dm's home unless given. Nothing leaves the machine.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;

use serde_json::{json, Value};

/// The tools offered, each a control-protocol method.
fn tools() -> Value {
    let tool = |name: &str, description: &str, properties: Value, required: Vec<&str>| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": properties, "required": required },
        })
    };
    json!([
        tool(
            "dm_list_commands",
            "Every command dm's window can run, with whether it can run now.",
            json!({}),
            vec![]
        ),
        tool(
            "dm_run_command",
            "Run one of dm's commands by id, on the selected downloads.",
            json!({ "id": { "type": "string" } }),
            vec!["id"]
        ),
        tool(
            "dm_add_downloads",
            "Add downloads: one link or several. `later` puts them in the main queue unstarted. \
             A link already in the list is not added again (it is named in `already_listed`) \
             unless `again` is true.",
            json!({ "urls": { "type": "array", "items": { "type": "string" } },
                     "later": { "type": "boolean" }, "again": { "type": "boolean" },
                     "folder": { "type": "string" },
                     "name": { "type": "string" }, "description": { "type": "string" },
                     "from": { "type": "string", "description": "With `to`: only this part of a video, as 4:10 or seconds" },
                     "to": { "type": "string" } }),
            vec!["urls"]
        ),
        tool(
            "dm_list_downloads",
            "Every download with its state, progress and where it was saved.",
            json!({}),
            vec![]
        ),
        tool(
            "dm_select",
            "Select downloads by id, for the commands that act on the selection.",
            json!({ "ids": { "type": "array", "items": { "type": "integer" } } }),
            vec!["ids"]
        ),
        tool(
            "dm_pause",
            "Stop the downloads with these ids; with no ids, every download that is running.",
            json!({ "ids": { "type": "array", "items": { "type": "integer" } } }),
            vec![]
        ),
        tool(
            "dm_resume",
            "Start or resume the downloads with these ids; with no ids, every unfinished one.",
            json!({ "ids": { "type": "array", "items": { "type": "integer" } } }),
            vec![]
        ),
        tool(
            "dm_cancel",
            "Cancel the downloads with these ids: they stop, and stay in the list as cancelled. \
             Their files are not deleted.",
            json!({ "ids": { "type": "array", "items": { "type": "integer" } } }),
            vec!["ids"]
        ),
        tool(
            "dm_set_speed_limit",
            "Set the speed limit: `kb` for an exact limit in KB/s (0 for none), or `preset` \
             (low, medium, high, unlimited).",
            json!({ "kb": { "type": "integer", "minimum": 0 },
                     "preset": { "type": "string", "enum": ["low", "medium", "high", "unlimited"] } }),
            vec![]
        ),
        tool(
            "dm_move",
            "Move the downloads with these ids up or down the queue order.",
            json!({ "ids": { "type": "array", "items": { "type": "integer" } },
                     "direction": { "type": "string", "enum": ["up", "down"] } }),
            vec!["ids", "direction"]
        ),
        tool(
            "dm_queue",
            "Start or stop the whole main queue.",
            json!({ "action": { "type": "string", "enum": ["start", "stop"] } }),
            vec!["action"]
        ),
        tool(
            "dm_screen",
            "Everything written on dm's window right now, with where, nearest the front first: \
             what a person reads. Use it to see which buttons, tabs and fields there are.",
            json!({}),
            vec![]
        ),
        tool(
            "dm_click",
            "Click the button, tab, menu item, checkbox or row on screen that says `text`, as a \
             person's mouse would. `button` right opens a context menu; `double` double-clicks; \
             `nth` picks a later match. Opening or previewing a downloaded file is refused. \
             Follow with dm_screen or dm_screenshot to see the result.",
            json!({ "text": { "type": "string" }, "button": { "type": "string", "enum": ["left", "right"] },
                     "double": { "type": "boolean" }, "nth": { "type": "integer", "minimum": 0 } }),
            vec!["text"]
        ),
        tool(
            "dm_type",
            "Type text into the field that has focus (click it first).",
            json!({ "text": { "type": "string" } }),
            vec!["text"]
        ),
        tool(
            "dm_press_key",
            "Press a key: Escape closes the top dialog, Enter presses its default button, Tab \
             moves between fields. `modifiers` may hold ctrl, shift, alt, cmd.",
            json!({ "key": { "type": "string" },
                     "modifiers": { "type": "array", "items": { "type": "string" } } }),
            vec!["key"]
        ),
        tool(
            "dm_inspect",
            "What the window shows: the rows, the selection, the open dialogs.",
            json!({}),
            vec![]
        ),
        tool(
            "dm_screenshot",
            "A PNG of dm's window, saved under dm's home; returns its path.",
            json!({ "name": { "type": "string" } }),
            vec![]
        ),
    ])
}

/// The control calls a tool is, in order: most are one method, the
/// ones that act on ids select them first.
fn plan(tool: &str, arguments: &Value) -> Result<Option<Vec<(&'static str, Value)>>, String> {
    let ids = arguments.get("ids").filter(|v| !v.is_null());
    let acting = |all: &'static str, some: &'static str| match ids {
        Some(ids) => vec![(some, json!({ "ids": ids }))],
        None => vec![("command.run", json!({ "id": all }))],
    };
    let run = |id: &str| ("command.run", json!({ "id": id }));
    let select = || ("ui.select", json!({ "ids": ids }));
    Ok(Some(match tool {
        "dm_pause" => acting("downloads.stop_all", "downloads.pause"),
        "dm_resume" => acting("downloads.resume_all", "downloads.resume"),
        "dm_cancel" => {
            if ids.is_none_or(|v| v.as_array().is_none_or(Vec::is_empty)) {
                return Err("give `ids`: the downloads to cancel".into());
            }
            vec![("downloads.cancel", json!({ "ids": ids }))]
        }
        "dm_move" => {
            if ids.is_none_or(|v| v.as_array().is_none_or(Vec::is_empty)) {
                return Err("give `ids`: the downloads to move".into());
            }
            let up = match arguments.get("direction").and_then(Value::as_str) {
                Some("up") => true,
                Some("down") => false,
                _ => return Err("direction is up or down".into()),
            };
            vec![
                select(),
                run(if up { "file.move_up" } else { "file.move_down" }),
            ]
        }
        "dm_queue" => match arguments.get("action").and_then(Value::as_str) {
            Some("start") => vec![run("downloads.start_queue")],
            Some("stop") => vec![run("downloads.stop_queue")],
            _ => return Err("action is start or stop".into()),
        },
        "dm_set_speed_limit" if arguments.get("kb").is_some_and(|k| !k.is_null()) => {
            vec![("downloads.speed_limit", json!({ "kb": arguments["kb"] }))]
        }
        "dm_set_speed_limit" => {
            let preset = arguments
                .get("preset")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !["low", "medium", "high", "unlimited"].contains(&preset) {
                return Err(format!(
                    "preset is low, medium, high or unlimited, not {preset:?}"
                ));
            }
            vec![(
                "command.run",
                json!({ "id": format!("downloads.speed.{preset}") }),
            )]
        }
        other => match method_of(other) {
            Some(method) => vec![(method, arguments.clone())],
            None => return Ok(None),
        },
    }))
}

/// The control method a tool is.
fn method_of(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "dm_list_commands" => "commands.list",
        "dm_run_command" => "command.run",
        "dm_add_downloads" => "downloads.add",
        "dm_list_downloads" => "queue.list",
        "dm_select" => "ui.select",
        "dm_inspect" => "ui.inspect",
        "dm_screenshot" => "ui.screenshot",
        "dm_screen" => "ui.texts",
        "dm_click" => "ui.click",
        "dm_type" => "ui.type",
        "dm_press_key" => "ui.key",
        _ => return None,
    })
}

/// Where the running dm listens and the token it wants, read afresh each
/// time a connection is made.
pub type Finder = Box<dyn Fn() -> Result<(u16, String), String> + Send>;

/// A connection to the running dm's control protocol, made again when dm
/// has gone and come back.
///
/// It used to be made once: after dm restarted, every call failed until
/// the AI client restarted `dm mcp` too — and dm's port is a new one each
/// start, so the port and token are read again rather than reused.
pub struct Control {
    link: Option<(TcpStream, BufReader<TcpStream>)>,
    find: Finder,
    client: Option<&'static str>,
}

impl Control {
    /// Connects to `port` with `token`, and to the same again later.
    pub fn connect(port: u16, token: &str) -> Result<Control, String> {
        Control::connect_as(port, token, Some("mcp"))
    }

    /// As `connect`, saying who is asking: `Some("mcp")` is an AI assistant,
    /// which the window answers only while Options allows it.
    pub fn connect_as(
        port: u16,
        token: &str,
        client: Option<&'static str>,
    ) -> Result<Control, String> {
        let token = token.to_string();
        Control::with_client(Box::new(move || Ok((port, token.clone()))), client)
    }

    /// Connects wherever `find` says, now and on each reconnection.
    pub fn with(find: Finder) -> Result<Control, String> {
        Control::with_client(find, None)
    }

    /// As `with`, as an AI assistant: the window answers it only while
    /// Options allows assistants.
    pub fn as_assistant(find: Finder) -> Result<Control, String> {
        Control::with_client(find, Some("mcp"))
    }

    fn with_client(find: Finder, client: Option<&'static str>) -> Result<Control, String> {
        let mut control = Control {
            link: None,
            find,
            client,
        };
        control.reconnect()?;
        Ok(control)
    }

    fn reconnect(&mut self) -> Result<(), String> {
        self.link = None;
        let (port, token) = (self.find)()?;
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| {
            // Most often a port file left by a dm that has since gone.
            format!(
                "dm is not running: nothing answers on port {port} ({e}); start it with \
                 --control 0"
            )
        })?;
        // An answer that never comes (a minimized window draws no frame to
        // take a screenshot in) ends the wait rather than hanging the
        // assistant for good.
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(90)))
            .map_err(|e| e.to_string())?;
        let replies = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        self.link = Some((stream, replies));
        if let Err(why) = self.exchange("auth", json!({ "token": token, "client": self.client })) {
            self.link = None;
            return Err(why);
        }
        Ok(())
    }

    /// Whether the connection is still there: dm, quitting, closes it, and
    /// a write to it would seem to succeed until the reply never came.
    fn alive(&self) -> bool {
        let Some((stream, replies)) = &self.link else {
            return false;
        };
        if !replies.buffer().is_empty() {
            return true;
        }
        if stream.set_nonblocking(true).is_err() {
            return false;
        }
        let mut byte = [0u8; 1];
        let open = match stream.peek(&mut byte) {
            Ok(0) => false,
            Ok(_) => true,
            Err(e) => e.kind() == std::io::ErrorKind::WouldBlock,
        };
        stream.set_nonblocking(false).is_ok() && open
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if !self.alive() {
            self.reconnect()?;
        }
        let answer = self.exchange(method, params);
        // A click, a typed word or a key is done a frame at a time: what
        // is drawn next is read after it has been.
        if answer.is_ok() && matches!(method, "ui.click" | "ui.type" | "ui.key") {
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
        if answer.is_err() && !self.alive() {
            // Gone mid-call: the next call connects again.
            self.link = None;
        }
        answer
    }

    fn exchange(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let Some((out, replies)) = &mut self.link else {
            return Err("dm closed the connection".into());
        };
        let line = json!({ "id": 1, "method": method, "params": params });
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
        let mut reply = String::new();
        if let Err(e) = replies.read_line(&mut reply) {
            // A reply that comes late would be read as the next call's:
            // the connection is made afresh instead.
            self.link = None;
            return Err(e.to_string());
        }
        let reply: Value =
            serde_json::from_str(&reply).map_err(|_| "dm closed the connection".to_string())?;
        if reply.get("ok") == Some(&Value::Bool(true)) {
            Ok(reply.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("dm refused")
                .to_string())
        }
    }
}

/// Answers one MCP message; `None` for a notification, which has no reply.
pub fn answer(message: &Value, control: &mut Control) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": message.pointer("/params/protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "dm", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = message
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = message
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or(json!({}));
            match plan(name, &arguments) {
                Ok(None) => Err((-32602, format!("no tool is called {name}"))),
                // A tool that fails is a result the model reads, not a
                // protocol error.
                Ok(Some(calls)) => Ok({
                    let mut last = Ok(Value::Null);
                    for (method, params) in calls {
                        last = control.call(method, params);
                        if last.is_err() {
                            break;
                        }
                    }
                    match last {
                        Ok(result) => {
                            json!({ "content": [{ "type": "text", "text": result.to_string() }] })
                        }
                        Err(error) => {
                            json!({ "content": [{ "type": "text", "text": error }], "isError": true })
                        }
                    }
                }),
                Err(why) => {
                    Ok(json!({ "content": [{ "type": "text", "text": why }], "isError": true }))
                }
            }
        }
        other => Err((-32601, format!("no method is called {other}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

/// Serves MCP on `input` and `output` until the input ends.
pub fn serve(
    input: impl BufRead,
    mut output: impl Write,
    control: &mut Control,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => answer(&message, control),
            Err(_) => Some(json!({ "jsonrpc": "2.0", "id": null,
                                   "error": { "code": -32700, "message": "that is not JSON" } })),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// Where the running dm's control port and token are: the options, else
/// the files dm keeps in its home. Read again on each reconnection.
pub fn finder(home: &Path, port: Option<String>, token_file: Option<String>) -> Finder {
    let home = home.to_path_buf();
    Box::new(move || {
        let port = port
            .clone()
            .or_else(|| std::fs::read_to_string(home.join("control.port")).ok())
            .and_then(|p| p.trim().parse::<u16>().ok());
        let token = token_file
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join("control.token"));
        match (port, std::fs::read_to_string(&token)) {
            (Some(port), Ok(token)) => Ok((port, token.trim().to_string())),
            _ => Err("no running dm to talk to — start dm with --control 0 first".to_string()),
        }
    })
}

/// `dm-desktop mcp`: the exit code.
pub fn main(args: &[String], home: &Path) -> i32 {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let find = finder(home, value("--port"), value("--token-file"));
    let mut control = match Control::as_assistant(find) {
        Ok(control) => control,
        Err(why) => {
            eprintln!("dm mcp: {why}");
            return 2;
        }
    };
    let stdin = std::io::stdin();
    match serve(stdin.lock(), std::io::stdout(), &mut control) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("dm mcp: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A stand-in for dm's control protocol that answers every line, and
    /// closes after `lines` of them as a dm quitting does.
    fn fake_dm(lines: usize) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            for _ in 0..lines {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                writeln!(out, "{}", json!({ "id": 1, "ok": true, "result": port })).unwrap();
            }
        });
        port
    }

    #[test]
    fn the_human_tools_are_control_calls() {
        assert_eq!(method_of("dm_click"), Some("ui.click"));
        assert_eq!(method_of("dm_screen"), Some("ui.texts"));
        assert_eq!(method_of("dm_type"), Some("ui.type"));
        assert_eq!(method_of("dm_press_key"), Some("ui.key"));
    }

    #[test]
    fn tools_become_the_control_calls_they_are() {
        let one = |tool: &str, args: Value| plan(tool, &args).unwrap().unwrap();
        let run = |id: &str| ("command.run", json!({ "id": id }));
        assert_eq!(
            one("dm_pause", json!({ "ids": [3, 4] })),
            vec![("downloads.pause", json!({ "ids": [3, 4] }))]
        );
        assert_eq!(
            one("dm_resume", json!({})),
            vec![run("downloads.resume_all")]
        );
        assert_eq!(
            one("dm_cancel", json!({ "ids": [5] })),
            vec![("downloads.cancel", json!({ "ids": [5] }))]
        );
        assert_eq!(
            one("dm_set_speed_limit", json!({ "preset": "low" })),
            vec![run("downloads.speed.low")]
        );
        assert_eq!(
            one("dm_set_speed_limit", json!({ "kb": 500 })),
            vec![("downloads.speed_limit", json!({ "kb": 500 }))]
        );
        assert_eq!(
            one("dm_move", json!({ "ids": [2], "direction": "down" })),
            vec![("ui.select", json!({ "ids": [2] })), run("file.move_down")]
        );
        assert_eq!(
            one("dm_queue", json!({ "action": "stop" })),
            vec![run("downloads.stop_queue")]
        );
        assert!(plan("dm_set_speed_limit", &json!({ "preset": "x" })).is_err());
        assert!(plan("dm_move", &json!({ "ids": [1] })).is_err());
        // Without ids, cancel and move refuse rather than act on a stray
        // selection or none.
        assert!(plan("dm_cancel", &json!({})).is_err());
        assert!(plan("dm_cancel", &json!({ "ids": [] })).is_err());
        assert!(plan("dm_move", &json!({ "direction": "up" })).is_err());
        // A null `kb` is no `kb`: the preset is used.
        assert_eq!(
            one("dm_set_speed_limit", json!({ "kb": null, "preset": "low" })),
            vec![run("downloads.speed.low")]
        );
        assert!(plan("dm_queue", &json!({ "action": "x" })).is_err());
        assert_eq!(plan("dm_nonsense", &json!({})), Ok(None));
    }

    #[test]
    fn a_dm_that_restarts_is_connected_to_again() {
        /* Connected once: after dm restarted, on a new port, every call
         * failed until the AI client restarted `dm mcp` as well. */
        let first = fake_dm(2);
        let at = Arc::new(Mutex::new(first));
        let found = Arc::clone(&at);
        let mut control =
            Control::with(Box::new(move || Ok((*found.lock().unwrap(), "t".into())))).unwrap();
        assert_eq!(control.call("queue.list", json!({})), Ok(json!(first)));

        // dm quits, and starts again elsewhere.
        let second = fake_dm(3);
        *at.lock().unwrap() = second;
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(control.call("queue.list", json!({})), Ok(json!(second)));
        assert_eq!(control.call("queue.list", json!({})), Ok(json!(second)));
    }
}
