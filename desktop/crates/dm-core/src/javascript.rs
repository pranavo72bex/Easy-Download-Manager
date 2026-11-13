// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The JavaScript dm runs: the program that answers YouTube's player checks
//! (`youtube::player`), and a proxy configuration script (`pac::evaluate_here`).
//!
//! dm's own interpreter runs it (`dm_js`, gaps row F15): Rust, in dm's own process, with
//! nothing of the computer in its reach (no files, programs, network, clock or page) and a time
//! limit, on a thread of its own. Nothing of the computer's is used for it any more: before, the
//! computer's own engine ran it (Chakra on Windows, JavaScriptCore on macOS and Linux) in a
//! process of its own behind the system's sandbox (gaps row 201); dm-js gives the same answers
//! on YouTube's live player and reads live videos on each system (2026-11-13).
//!
//! A test may put a stand-in in its place (`set_host`): a program that takes a request on its
//! standard input and answers on its output, so a test of what dm does with an answer runs on
//! answers it makes up.

use std::path::PathBuf;
use std::time::Duration;

/// What a stand-in host writes before its answer, on a line of its own.
const ANSWER: &str = "dm-javascript-answer ";

/// A test's stand-in for the interpreter: a program, its arguments and its environment.
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct Host {
    /// The program.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its environment, beyond what it inherits.
    pub env: Vec<(String, String)>,
}

/// The stand-in in use, where a test set one.
static HOST: std::sync::Mutex<Option<Host>> = std::sync::Mutex::new(None);

/// A test's stand-in from now on, or none (dm-js again).
#[doc(hidden)]
pub fn set_host(host: Option<Host>) {
    *crate::lock(&HOST) = host;
}

/// Runs `program`, then `call`, and gives the value of `call` as text, or why there is none.
pub(crate) async fn run(program: &str, call: &str, within: Duration) -> Result<String, String> {
    let host = crate::lock(&HOST).clone();
    if let Some(host) = host {
        return in_host(&host, program, call, within).await;
    }
    let (program, call) = (program.to_string(), call.to_string());
    tokio::task::spawn_blocking(move || dm_js::evaluate(&program, &call, within))
        .await
        .unwrap_or_else(|_| Err("dm's JavaScript interpreter stopped".into()))
}

/// How the host is started: its program, arguments and environment, its standard input and
/// output piped, and ended if it is let go.
fn host_command(host: &Host) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(&host.program);
    command
        .args(&host.args)
        .envs(host.env.iter().map(|(name, value)| (name, value)))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::no_window_async(&mut command);
    command
}

/// Runs `program`, then `call`, in a process of the host's: its answer, or why there is none.
async fn in_host(
    host: &Host,
    program: &str,
    call: &str,
    within: Duration,
) -> Result<String, String> {
    use tokio::io::AsyncWriteExt as _;
    let request = serde_json::json!({
        "program": program,
        "call": call,
        "within_ms": u64::try_from(within.as_millis()).unwrap_or(u64::MAX),
    })
    .to_string();
    let mut child = host_command(host)
        .spawn()
        .map_err(|e| format!("dm's JavaScript host would not start: {e}"))?;
    let Some(mut input) = child.stdin.take() else {
        return Err("dm's JavaScript host took no request".into());
    };
    // Written while the answer is read: a long program fills the pipe before the host is done
    // reading it. A host that ended early refuses the rest, and says why below.
    let writing = async move {
        let _ = input.write_all(request.as_bytes()).await;
        let _ = input.shutdown().await;
    };
    // Its own watch stops the program at its time; this one is for a host that never answers.
    let output = tokio::time::timeout(within + Duration::from_secs(15), async {
        let ((), output) = tokio::join!(writing, child.wait_with_output());
        output
    })
    .await
    .map_err(|_| "the JavaScript took too long".to_string())?
    .map_err(|e| format!("dm's JavaScript host could not be heard: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(said) = stdout
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(ANSWER))
    else {
        let why = format!(
            "the stand-in JavaScript host ended without an answer ({})",
            output.status
        );
        return Err(why);
    };
    let said: serde_json::Value = serde_json::from_str(said)
        .map_err(|e| format!("dm's JavaScript host answered with what is not an answer: {e}"))?;
    match (
        said.get("answer").and_then(serde_json::Value::as_str),
        said.get("error").and_then(serde_json::Value::as_str),
    ) {
        (Some(answer), _) => Ok(answer.to_string()),
        (None, Some(why)) => Err(why.to_string()),
        (None, None) => Err("dm's JavaScript host answered nothing".into()),
    }
}
