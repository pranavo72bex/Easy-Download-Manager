// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The JavaScript dm runs: the program that answers YouTube's player checks
//! (`youtube::player`), and a proxy configuration script (`pac::evaluate_here`).
//!
//! dm's own interpreter answers first (`dm_js`, gaps row F15): Rust, in dm's own process, with
//! nothing of the computer in its reach and a time limit. On YouTube's live player it gives the
//! same answers as the computer's engine. Where it cannot answer, the computer's own engine
//! below still does, as it did before (`DM_SYSTEM_JS` set asks it alone), until dm-js has run
//! long enough on its own for that engine and its sandbox to go (rows F15, 201).
//!
//! The computer's own engine:
//!
//! On Windows it is Windows' own Chakra (`chakra.dll`, part of Windows 10 and 11), as reference
//! A runs its YouTube pieces in Windows' own JScript engine: nothing is fetched or shipped for
//! it. It runs with its compiler to machine code off (an interpreter only), `eval` off, no more
//! than [`MEMORY`] bytes and the time it is given, and with nothing of the computer in reach: a
//! program there has JavaScript's own objects and no files, programs or network. In the app it
//! runs in a process of its own, dm's own program started as the engine's host ([`HOST_FLAG`],
//! [`host_main`]): the engine ends its whole process when a program passes its memory, and a
//! fault in it would too, so neither can take the app with it.
//!
//! On macOS it is the system's own JavaScriptCore (`JavaScriptCore.framework`, part of macOS),
//! loaded from its system path when first needed: nothing is fetched or shipped for it either. It
//! runs with the time it is given (`JSContextGroupSetExecutionTimeLimit`) and with nothing of
//! the computer in reach (a context of JavaScript's own objects: no files, programs, network or
//! page), in the same process of its own as Chakra (macOS has no limit on one program's memory to
//! set; the process keeps a program that runs away from the app). `eval` is the language's own
//! there and is left: it reaches nothing a program does not already have.
//!
//! On Linux it is the same JavaScriptCore, WebKitGTK's (`libjavascriptcoregtk`): GNOME's own
//! programs and every WebKitGTK browser bring it, dm's .deb, .rpm and Arch packages depend on it
//! and its AppImage carries it. It runs as macOS's does.
//!
//! Where none is there, nothing else is run in its place (gaps row P1: before, dm used an
//! installed Deno or QuickJS, else fetched QuickJS and ran it): the reading says which engine is
//! missing and how to get it ([`NO_ENGINE`]).

use std::path::PathBuf;
use std::time::Duration;

// One file for each system's engine, and the wall around the host.
#[cfg(windows)]
mod chakra;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod jsc;
mod sandbox;

/// The most memory a program may take in Windows' own engine: the program that answers
/// YouTube's checks takes about 30 MB.
#[cfg(windows)]
pub(crate) const MEMORY: usize = 256 << 20;

/// The argument, alone, that starts dm's own program as the engine's host: its `main` then
/// calls [`host_main`] and nothing else.
pub const HOST_FLAG: &str = "--javascript-host";

/// What the host writes before its answer, on a line of its own.
const ANSWER: &str = "dm-javascript-answer ";

/// The program that runs the engine in a process of its own, with its arguments and anything
/// it is given in its environment.
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

/// The engine's host, once a program has said it is one; with none, the engine runs in this
/// process (a test, a tool).
static HOST: std::sync::Mutex<Option<Host>> = std::sync::Mutex::new(None);

/// Says that this program, started with [`HOST_FLAG`] alone, is the engine's host: dm's own
/// `main` says so, and the engine then runs in a process of its own.
pub fn host_is_this_program() {
    if let Ok(program) = std::env::current_exe() {
        set_host(Some(Host {
            program,
            args: vec![HOST_FLAG.into()],
            env: Vec::new(),
        }));
    }
}

/// The engine's host from now on, or none (the engine in this process).
#[doc(hidden)]
pub fn set_host(host: Option<Host>) {
    *crate::lock(&HOST) = host;
}

/// The host's own `main`: one request on standard input (JSON: the program, the call and the
/// milliseconds it may take), run in this process, and its answer on standard output (JSON on
/// a line of its own: the call's value, or why there is none). Its exit code, 0 once it has
/// answered (an error is an answer too).
pub fn host_main() -> i32 {
    use std::io::{Read as _, Write as _};
    let mut request = String::new();
    let answer = match std::io::stdin().read_to_string(&mut request) {
        Err(e) => Err(format!("the host read no request: {e}")),
        Ok(_) => match serde_json::from_str::<serde_json::Value>(&request) {
            Err(e) => Err(format!("the host's request is not one: {e}")),
            Ok(asked) => {
                let text = |key: &str| asked.get(key).and_then(serde_json::Value::as_str);
                let within = asked
                    .get("within_ms")
                    .and_then(serde_json::Value::as_u64)
                    .map_or(Duration::from_secs(30), Duration::from_millis);
                match (text("program"), text("call")) {
                    (Some(program), Some(call)) => {
                        shut_off().and_then(|()| here(program, call, within))
                    }
                    _ => Err("the host's request names no program and call".into()),
                }
            }
        },
    };
    let said = match answer {
        Ok(value) => serde_json::json!({ "answer": value }),
        Err(why) => serde_json::json!({ "error": why }),
    };
    let mut out = std::io::stdout().lock();
    let written = writeln!(out, "{ANSWER}{said}").and_then(|()| out.flush());
    i32::from(written.is_err())
}

/// The host shut off from the computer before the script runs (`sandbox`): the engine loaded
/// and a first call run first, so its libraries and data are open, then the system's wall.
fn shut_off() -> Result<(), String> {
    here("", "0", Duration::from_secs(10))?;
    sandbox::enter()
}

/// [`run`], in this process, for the host.
fn here(program: &str, call: &str, within: Duration) -> Result<String, String> {
    #[cfg(windows)]
    {
        chakra::run_blocking(program, call, within)
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        jsc::run_blocking(program, call, within)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (program, call, within);
        Err("this system's own JavaScript engine is not used yet".into())
    }
}

/// What is said where the computer has no engine dm can use.
#[cfg(target_os = "linux")]
pub const NO_ENGINE: &str = "this computer has no JavaScript engine for dm: install WebKitGTK's \
     JavaScriptCore (libjavascriptcoregtk-4.1-0 on Debian and Ubuntu, javascriptcoregtk4.1 on \
     Fedora, webkit2gtk-4.1 on Arch)";
/// What is said where the computer has no engine dm can use.
#[cfg(windows)]
pub const NO_ENGINE: &str =
    "this computer has no JavaScript engine for dm: Windows' own Chakra (chakra.dll) is missing";
/// What is said where the computer has no engine dm can use.
#[cfg(target_os = "macos")]
pub const NO_ENGINE: &str = "this computer has no JavaScript engine for dm: macOS's own \
     JavaScriptCore could not be loaded";
/// What is said where the computer has no engine dm can use.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub const NO_ENGINE: &str = "this system's own JavaScript engine is not used yet";

/// Whether the computer's own engine is here (Windows' own Chakra, macOS's JavaScriptCore,
/// WebKitGTK's JavaScriptCore on Linux).
#[doc(hidden)]
pub fn system_engine_here() -> bool {
    #[cfg(windows)]
    {
        chakra::here()
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        jsc::here()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// Runs `program`, then `call`, and gives the value of `call` as text: in the host's process
/// where a program said it is one (the app), else in this process (a test, a tool).
///
/// A host that is not this program is a test's stand-in, and is asked whatever this computer
/// has: that is how a test of what dm does with an answer runs where no engine is.
pub(crate) async fn run(program: &str, call: &str, within: Duration) -> Result<String, String> {
    let host = crate::lock(&HOST).clone();
    // A test's stand-in host answers in place of any engine (what it is asked is made up).
    let stand_in = host
        .as_ref()
        .is_some_and(|host| std::env::current_exe().map_or(true, |me| me != host.program));
    // dm's own interpreter first (gaps row F15): Rust, in this process, nothing of the computer
    // in its reach. Where it cannot answer, the computer's engine below still does, as before;
    // `DM_SYSTEM_JS` set asks the computer's engine alone (to set the two side by side).
    if !stand_in && std::env::var_os("DM_SYSTEM_JS").is_none() {
        let (p, c) = (program.to_string(), call.to_string());
        let own = tokio::task::spawn_blocking(move || dm_js::evaluate(&p, &c, within))
            .await
            .unwrap_or_else(|_| Err("dm's JavaScript interpreter stopped".into()));
        match own {
            Ok(answer) => return Ok(answer),
            Err(why) if !system_engine_here() => return Err(why),
            Err(_) => {}
        }
    }
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    if let Some(host) = host {
        let stand_in = std::env::current_exe().map_or(true, |me| me != host.program);
        if !stand_in && !system_engine_here() {
            return Err(NO_ENGINE.into());
        }
        return in_host(&host, program, call, within).await;
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let _ = host;
    if !system_engine_here() {
        return Err(NO_ENGINE.into());
    }
    #[cfg(windows)]
    {
        chakra::run(program, call, within).await
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        jsc::run(program, call, within).await
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (program, call, within);
        Err(NO_ENGINE.into())
    }
}

/// How the host is started: its program, arguments and environment, its standard input and
/// output piped, and ended if it is let go.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn host_command(host: &Host) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(&host.program);
    command
        .args(&host.args)
        .envs(host.env.iter().map(|(name, value)| (name, value)))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    // JavaScriptCore with no compiling to machine code, as Chakra runs on Windows
    // (`DisableNativeCodeGeneration`): its interpreter only, which removes the JIT, where engine
    // exploits usually live. The engine reads it from its environment as it starts; a YouTube
    // reading takes no longer for it (2.0 to 2.9 s either way, 2026-11-09).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    command.env("JSC_useJIT", "false");
    crate::no_window_async(&mut command);
    command
}

/// Runs `program`, then `call`, in a process of the host's: its answer, or why there is none.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
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
        #[cfg(windows)]
        let why = format!(
            "the JavaScript stopped its engine, which ends where a program takes more than its \
             memory ({} MB) or the engine fails ({})",
            MEMORY >> 20,
            output.status
        );
        #[cfg(not(windows))]
        let why = format!(
            "the JavaScript stopped its engine, which ends where the engine fails ({})",
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
