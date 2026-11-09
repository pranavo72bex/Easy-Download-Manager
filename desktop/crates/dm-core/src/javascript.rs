// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The computer's own JavaScript engine, for the little JavaScript dm runs: the program that
//! answers YouTube's player checks (`youtube::player`), and a proxy configuration script
//! (`pac::evaluate_here`).
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
//! On Linux it is the same JavaScriptCore, WebKitGTK's (`libjavascriptcoregtk`), where the
//! system has it: GNOME's own programs and every WebKitGTK browser bring it, and dm's rpm and
//! Arch packages recommend it. It runs as macOS's does.
//!
//! Where none of them is there (a Linux without WebKitGTK, a Windows or a macOS without its
//! engine): an installed Deno, run with no permissions, else QuickJS, found or fetched
//! (`crate::tools`).

use std::path::{Path, PathBuf};
use std::time::Duration;

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
                    (Some(program), Some(call)) => here(program, call, within),
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

/// Whether a person or a test has named the engine outright (`DM_QJS`, `DM_DENO`; set empty, it
/// names none): then that one is used, or none, and not the computer's own.
fn engine_named() -> bool {
    std::env::var_os("DM_QJS").is_some() || std::env::var_os("DM_DENO").is_some()
}

/// Whether the computer's own engine is here (Windows' own Chakra, macOS's JavaScriptCore,
/// WebKitGTK's JavaScriptCore on Linux), rather than one dm runs as a program of its own.
#[doc(hidden)]
pub fn system_engine_here() -> bool {
    if engine_named() {
        return false;
    }
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

/// Runs `program`, then `call`, and gives the value of `call` as text. `staging` is where a
/// program is written for an engine run as a program of its own.
pub(crate) async fn run(
    program: &str,
    call: &str,
    within: Duration,
    staging: &Path,
) -> Result<String, String> {
    #[cfg(windows)]
    if !engine_named() && chakra::here() {
        let host = crate::lock(&HOST).clone();
        return match host {
            Some(host) => in_host(&host, program, call, within).await,
            None => chakra::run(program, call, within).await,
        };
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if !engine_named() && jsc::here() {
        let host = crate::lock(&HOST).clone();
        return match host {
            Some(host) => in_host(&host, program, call, within).await,
            None => jsc::run(program, call, within).await,
        };
    }
    command::run(program, call, within, staging).await
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

#[cfg(windows)]
mod chakra {
    //! Windows' own Chakra, through the hosting functions it exports (JSRT, `jsrt.h`).

    use std::ffi::c_void;
    use std::ptr::null_mut;
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::OnceLock;
    use std::time::Duration;

    /// What each call answers: 0 when it worked.
    type Code = u32;
    /// A runtime, a context or a value.
    type Ref = *mut c_void;
    /// What `GetProcAddress` gives.
    type Found = unsafe extern "system" fn() -> isize;

    type CreateRuntime = unsafe extern "system" fn(u32, *mut c_void, *mut Ref) -> Code;
    type CreateContext = unsafe extern "system" fn(Ref, *mut Ref) -> Code;
    type OfRef = unsafe extern "system" fn(Ref) -> Code;
    type RunScript = unsafe extern "system" fn(*const u16, usize, *const u16, *mut Ref) -> Code;
    type Convert = unsafe extern "system" fn(Ref, *mut Ref) -> Code;
    type ToPointer = unsafe extern "system" fn(Ref, *mut *const u16, *mut usize) -> Code;
    type TakeException = unsafe extern "system" fn(*mut Ref) -> Code;
    type Count = unsafe extern "system" fn(Ref, *mut u32) -> Code;
    type SetLimit = unsafe extern "system" fn(Ref, usize) -> Code;

    /// No background threads of its own; stoppable from another thread; no compiling to
    /// machine code; no `eval` (`JsRuntimeAttributes`).
    const ATTRIBUTES: u32 = 0x1 | 0x2 | 0x8 | 0x10;
    /// `JsErrorScriptException` and `JsErrorScriptCompile`: a program that threw, or does not
    /// parse.
    const THREW: Code = 0x30001;
    const NO_PARSE: Code = 0x30002;
    /// `JsErrorScriptTerminated`: stopped at its time.
    const STOPPED: Code = 0x30003;
    /// `JsErrorScriptEvalDisabled`: a program that asked for `eval`.
    const NO_EVAL: Code = 0x30004;
    /// The stack the engine's thread has: room for a deep program.
    const STACK: usize = 16 << 20;
    /// The name a script is run under, for its errors.
    const NAME: &[u16] = &[b'd' as u16, b'm' as u16, 0];

    /// The functions dm calls.
    struct Api {
        create_runtime: CreateRuntime,
        create_context: CreateContext,
        set_current_context: OfRef,
        run_script: RunScript,
        convert_to_string: Convert,
        string_to_pointer: ToPointer,
        take_exception: TakeException,
        add_ref: Count,
        release: Count,
        disable_execution: OfRef,
        set_memory_limit: SetLimit,
        dispose_runtime: OfRef,
    }

    /// The engine, loaded once, or `None` where Windows has none.
    fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(load).as_ref()
    }

    /// Whether Windows' own engine is here.
    pub(super) fn here() -> bool {
        api().is_some()
    }

    /// Loads `chakra.dll` from System32 and takes its functions.
    fn load() -> Option<Api> {
        use windows_sys::Win32::System::LibraryLoader::{
            LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
        };
        let name: Vec<u16> = "chakra.dll".encode_utf16().chain(Some(0)).collect();
        // SAFETY: `name` is a NUL-ended wide string that outlives the call. Only System32 is
        // searched, so no file of the same name elsewhere is loaded in its place. The library
        // is never freed, so the functions taken from it stay valid for the process.
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return None;
        }
        // SAFETY: each field's type is its export's own (`ChakraCommon.h`, `ChakraCore.h`; the
        // Windows edition, `jsrt.h`), and `module` is loaded and never freed.
        unsafe {
            Some(Api {
                create_runtime: take(module, "JsCreateRuntime\0")?,
                create_context: take(module, "JsCreateContext\0")?,
                set_current_context: take(module, "JsSetCurrentContext\0")?,
                run_script: take(module, "JsRunScript\0")?,
                convert_to_string: take(module, "JsConvertValueToString\0")?,
                string_to_pointer: take(module, "JsStringToPointer\0")?,
                take_exception: take(module, "JsGetAndClearException\0")?,
                add_ref: take(module, "JsAddRef\0")?,
                release: take(module, "JsRelease\0")?,
                disable_execution: take(module, "JsDisableRuntimeExecution\0")?,
                set_memory_limit: take(module, "JsSetRuntimeMemoryLimit\0")?,
                dispose_runtime: take(module, "JsDisposeRuntime\0")?,
            })
        }
    }

    /// The function `module` exports under `name` (NUL-ended), as the function pointer type `F`.
    ///
    /// # Safety
    ///
    /// `module` is loaded and stays so, and `F` is the type of that export.
    unsafe fn take<F: Copy>(
        module: windows_sys::Win32::Foundation::HMODULE,
        name: &str,
    ) -> Option<F> {
        if !name.ends_with('\0') || std::mem::size_of::<F>() != std::mem::size_of::<Found>() {
            return None;
        }
        // SAFETY: `module` is loaded (the caller's word) and `name` is NUL-ended.
        let found: Found = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(module, name.as_ptr())
        }?;
        // SAFETY: `F` is this export's type (the caller's word), a function pointer of the same
        // size as `Found`, checked above.
        Some(unsafe { std::mem::transmute_copy::<Found, F>(&found) })
    }

    /// A runtime, disposed when dropped.
    struct Runtime {
        api: &'static Api,
        handle: Ref,
    }

    impl Drop for Runtime {
        fn drop(&mut self) {
            // SAFETY: the runtime was made on this thread and no other thread holds it now (the
            // one that stops it at its time is joined before this is dropped); a runtime is
            // disposed with no context current.
            unsafe {
                (self.api.set_current_context)(null_mut());
                (self.api.dispose_runtime)(self.handle);
            }
        }
    }

    /// A runtime's handle, sent to the thread that stops it at its time.
    struct Watched(Ref);
    // SAFETY: the handle is only used there for `JsDisableRuntimeExecution`, which may be called
    // from any thread, and only before the runtime is disposed (that thread is joined first).
    unsafe impl Send for Watched {}

    /// Runs `program`, then `call`, on a thread of its own, and gives `call`'s value as text.
    pub(super) async fn run(program: &str, call: &str, within: Duration) -> Result<String, String> {
        // The texts as the engine reads them. They outlive the runtime: the engine reads a
        // function's text from them when it first runs it.
        let program: Vec<u16> = program.encode_utf16().chain(Some(0)).collect();
        let call: Vec<u16> = call.encode_utf16().chain(Some(0)).collect();
        let (sent, answer) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("dm-javascript".into())
            .stack_size(STACK)
            .spawn(move || {
                let _ = sent.send(run_here(&program, &call, within));
            })
            .map_err(|e| format!("Windows' own JavaScript engine could not start: {e}"))?;
        answer
            .await
            .map_err(|_| "Windows' own JavaScript engine stopped".to_string())?
    }

    /// [`run`], waited for on this thread: the host's.
    pub(super) fn run_blocking(
        program: &str,
        call: &str,
        within: Duration,
    ) -> Result<String, String> {
        let program: Vec<u16> = program.encode_utf16().chain(Some(0)).collect();
        let call: Vec<u16> = call.encode_utf16().chain(Some(0)).collect();
        std::thread::Builder::new()
            .name("dm-javascript".into())
            .stack_size(STACK)
            .spawn(move || run_here(&program, &call, within))
            .map_err(|e| format!("Windows' own JavaScript engine could not start: {e}"))?
            .join()
            .map_err(|_| "Windows' own JavaScript engine stopped".to_string())?
    }

    /// [`run`] on this thread.
    fn run_here(program: &[u16], call: &[u16], within: Duration) -> Result<String, String> {
        let api = api().ok_or("Windows' own JavaScript engine is not on this computer")?;
        let mut handle: Ref = null_mut();
        // SAFETY: the out-pointer is valid; no thread service is passed.
        let code = unsafe { (api.create_runtime)(ATTRIBUTES, null_mut(), &mut handle) };
        if code != 0 || handle.is_null() {
            return Err(format!(
                "Windows' own JavaScript engine would not start ({code:#x})"
            ));
        }
        let runtime = Runtime { api, handle };
        // SAFETY: `handle` is the runtime just made, on this thread.
        let code = unsafe { (api.set_memory_limit)(runtime.handle, super::MEMORY) };
        if code != 0 {
            return Err(format!(
                "Windows' own JavaScript engine took no memory limit ({code:#x})"
            ));
        }
        let mut context: Ref = null_mut();
        // SAFETY: as above; the out-pointer is valid.
        let code = unsafe { (api.create_context)(runtime.handle, &mut context) };
        if code != 0 || context.is_null() {
            return Err(format!(
                "Windows' own JavaScript engine made no context ({code:#x})"
            ));
        }
        // SAFETY: `context` is the runtime's own, made just now on this thread.
        let code = unsafe { (api.set_current_context)(context) };
        if code != 0 {
            return Err(format!(
                "Windows' own JavaScript engine took no context ({code:#x})"
            ));
        }
        // Stopped at its time from a second thread, joined before the runtime is disposed.
        let (done, wait) = std::sync::mpsc::channel::<()>();
        let watched = Watched(runtime.handle);
        let watchdog = std::thread::spawn(move || {
            let watched = watched;
            if wait.recv_timeout(within) == Err(RecvTimeoutError::Timeout) {
                // SAFETY: the runtime is disposed only after this thread is joined.
                unsafe { (api.disable_execution)(watched.0) };
            }
        });
        // The program's own value is not read: it can be a function, which this engine has
        // been seen to fail reading the text of after a long program.
        let answer = run_script(api, program, 1)
            .and_then(|value| {
                release(api, value);
                run_script(api, call, 2)
            })
            .and_then(|value| {
                let text = text_of(api, value);
                release(api, value);
                text
            });
        drop(done);
        let _ = watchdog.join();
        drop(runtime);
        answer
    }

    /// Runs one script and gives its value, held (`JsAddRef`) until [`release`]d.
    fn run_script(api: &Api, source: &[u16], cookie: usize) -> Result<Ref, String> {
        let mut value: Ref = null_mut();
        // SAFETY: `source` and `NAME` are NUL-ended and outlive the runtime (see `run`); a
        // context is current on this thread; the out-pointer is valid.
        let code = unsafe { (api.run_script)(source.as_ptr(), cookie, NAME.as_ptr(), &mut value) };
        match code {
            0 => {
                // SAFETY: `value` is a value of the current context, just given.
                unsafe { (api.add_ref)(value, null_mut()) };
                Ok(value)
            }
            STOPPED => Err("the JavaScript took too long".into()),
            THREW | NO_PARSE | NO_EVAL => {
                Err(format!("the JavaScript stopped: {}", exception(api)))
            }
            other => Err(format!("the JavaScript engine failed ({other:#x})")),
        }
    }

    /// Lets the engine free a value [`run_script`] held.
    fn release(api: &Api, value: Ref) {
        // SAFETY: `value` was held with `JsAddRef` and its context is current.
        unsafe { (api.release)(value, null_mut()) };
    }

    /// The exception a script stopped with, in its own words.
    fn exception(api: &Api) -> String {
        let mut error: Ref = null_mut();
        // SAFETY: a context is current and in an exception state; the out-pointer is valid.
        let code = unsafe { (api.take_exception)(&mut error) };
        if code != 0 || error.is_null() {
            return "an error with no words".into();
        }
        // SAFETY: `error` is a value of the current context, just given.
        unsafe { (api.add_ref)(error, null_mut()) };
        let words = text_of(api, error).unwrap_or_else(|_| "an error dm could not read".into());
        release(api, error);
        words
    }

    /// A value as text, as `String(value)` gives it.
    fn text_of(api: &Api, value: Ref) -> Result<String, String> {
        let mut string: Ref = null_mut();
        // SAFETY: `value` is held and its context current; the out-pointer is valid.
        let code = unsafe { (api.convert_to_string)(value, &mut string) };
        if code != 0 || string.is_null() {
            return Err(format!("the JavaScript's answer is not text ({code:#x})"));
        }
        // SAFETY: as above, for the string just made.
        unsafe { (api.add_ref)(string, null_mut()) };
        let mut chars: *const u16 = std::ptr::null();
        let mut length = 0usize;
        // SAFETY: `string` is held; the out-pointers are valid.
        let code = unsafe { (api.string_to_pointer)(string, &mut chars, &mut length) };
        let text = if code == 0 && !chars.is_null() {
            // SAFETY: the engine gives `length` UTF-16 units at `chars`, valid while `string` is
            // held; they are copied out before it is released.
            let units = unsafe { std::slice::from_raw_parts(chars, length) };
            Ok(String::from_utf16_lossy(units))
        } else {
            Err(format!(
                "the JavaScript's answer could not be read ({code:#x})"
            ))
        };
        release(api, string);
        text
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn run_now(program: &str, call: &str, within: Duration) -> Result<String, String> {
            let program: Vec<u16> = program.encode_utf16().chain(Some(0)).collect();
            let call: Vec<u16> = call.encode_utf16().chain(Some(0)).collect();
            run_here(&program, &call, within)
        }

        #[test]
        fn windows_own_engine_runs_a_program_and_gives_a_calls_value() {
            assert!(here(), "chakra.dll is in every Windows 10 and 11");
            let got = run_now(
                "var a = function (x) { return x + '€'; };",
                "a('ü') + 1",
                Duration::from_secs(10),
            );
            assert_eq!(got.unwrap(), "ü€1");
            // A program that throws, or does not parse, says why.
            let why = run_now(
                "throw new TypeError('no such thing');",
                "1",
                Duration::from_secs(10),
            )
            .unwrap_err();
            assert!(
                why.contains("TypeError") && why.contains("no such thing"),
                "{why}"
            );
            let why = run_now("var = ;", "1", Duration::from_secs(10)).unwrap_err();
            assert!(why.contains("the JavaScript stopped"), "{why}");
        }

        #[test]
        fn a_program_that_runs_on_is_stopped_at_its_time_and_eval_is_off() {
            let started = std::time::Instant::now();
            let why = run_now("for (;;) {}", "1", Duration::from_millis(300)).unwrap_err();
            assert_eq!(why, "the JavaScript took too long");
            assert!(started.elapsed() < Duration::from_secs(10));
            let why = run_now("var x = 1;", "eval('x')", Duration::from_secs(10)).unwrap_err();
            assert!(why.contains("the JavaScript stopped"), "{why}");
            let why = run_now(
                "var x = 1;",
                "new Function('return x')()",
                Duration::from_secs(10),
            )
            .unwrap_err();
            assert!(why.contains("the JavaScript stopped"), "{why}");
        }

        /// Set where this test binary is started as the engine's host.
        const AS_HOST: &str = "DM_JAVASCRIPT_HOST_TEST";

        /// The host's `main`, where this test binary is started as the engine's host (the test
        /// below starts it so); as a test of its own, nothing.
        #[test]
        fn the_engines_host_when_this_binary_is_started_as_one() {
            if std::env::var_os(AS_HOST).is_some() {
                std::process::exit(crate::javascript::host_main());
            }
        }

        #[tokio::test]
        async fn in_a_process_of_its_own_a_program_that_takes_too_much_memory_ends_only_its_host() {
            let host = crate::javascript::Host {
                program: std::env::current_exe().unwrap(),
                args: vec![
                    "--exact".into(),
                    "javascript::chakra::tests::the_engines_host_when_this_binary_is_started_as_one"
                        .into(),
                    "--nocapture".into(),
                ],
                env: vec![(AS_HOST.into(), "1".into())],
            };
            let run = |program: &'static str, call: &'static str, within: Duration| {
                let host = host.clone();
                async move { crate::javascript::in_host(&host, program, call, within).await }
            };
            assert_eq!(
                run("var a = 2;", "a * 21 + 'ü'", Duration::from_secs(20))
                    .await
                    .unwrap(),
                "42ü"
            );
            let why = run("throw new RangeError('far');", "1", Duration::from_secs(20))
                .await
                .unwrap_err();
            assert!(why.contains("RangeError") && why.contains("far"), "{why}");
            let why = run(
                "var a = []; for (;;) { a.push(new Array(1 << 16).join('x')); }",
                "1",
                Duration::from_secs(60),
            )
            .await
            .unwrap_err();
            assert!(why.contains("memory"), "{why}");
            let why = run("for (;;) {}", "1", Duration::from_millis(500))
                .await
                .unwrap_err();
            assert_eq!(why, "the JavaScript took too long");
            // This process is still here to say so, and the next run answers.
            assert_eq!(
                run("", "'still'", Duration::from_secs(20)).await.unwrap(),
                "still"
            );
        }

        #[test]
        fn there_is_nothing_of_the_computer_in_reach() {
            for name in [
                "require",
                "process",
                "Deno",
                "std",
                "os",
                "WScript",
                "ActiveXObject",
                "XMLHttpRequest",
                "fetch",
            ] {
                let got = run_now("", &format!("typeof {name}"), Duration::from_secs(10)).unwrap();
                assert_eq!(got, "undefined", "{name} is in reach");
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod jsc {
    //! JavaScriptCore, through its C functions (`JavaScriptCore/JavaScript.h`), taken from the
    //! library when first needed: macOS's own framework at its system path, or on Linux
    //! WebKitGTK's (the same C functions, the time limit among them). A system without it, or
    //! one whose library lacks a function dm takes, runs an installed engine instead.

    use std::ffi::c_void;
    use std::ptr::null_mut;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    /// A context, a context group, a string, a value or an object.
    type Ref = *mut c_void;
    /// What `JSContextGroupSetExecutionTimeLimit` calls when the time is up: `true` stops it.
    type ShouldStop = Option<unsafe extern "C" fn(Ref, *mut c_void) -> bool>;

    type ContextCreate = unsafe extern "C" fn(Ref) -> Ref;
    type OfRef = unsafe extern "C" fn(Ref);
    type GetGroup = unsafe extern "C" fn(Ref) -> Ref;
    type StringOf = unsafe extern "C" fn(*const u16, usize) -> Ref;
    type Evaluate = unsafe extern "C" fn(Ref, Ref, Ref, Ref, i32, *mut Ref) -> Ref;
    type ToString = unsafe extern "C" fn(Ref, Ref, *mut Ref) -> Ref;
    type Length = unsafe extern "C" fn(Ref) -> usize;
    type Characters = unsafe extern "C" fn(Ref) -> *const u16;
    type Hold = unsafe extern "C" fn(Ref, Ref);
    type SetLimit = unsafe extern "C" fn(Ref, f64, ShouldStop, *mut c_void);

    /// Where the library is, the first found taken (NUL-ended).
    #[cfg(target_os = "macos")]
    const LIBRARIES: &[&str] =
        &["/System/Library/Frameworks/JavaScriptCore.framework/JavaScriptCore\0"];
    /// WebKitGTK's, by its library's name, found where the system's loader finds libraries: the
    /// GTK 4 build's, then the GTK 3 builds' (libsoup 3, then 2).
    #[cfg(target_os = "linux")]
    const LIBRARIES: &[&str] = &[
        "libjavascriptcoregtk-6.0.so.1\0",
        "libjavascriptcoregtk-4.1.so.0\0",
        "libjavascriptcoregtk-4.0.so.18\0",
    ];
    /// The engine, in what dm says.
    #[cfg(target_os = "macos")]
    const ENGINE: &str = "macOS's own JavaScript engine";
    #[cfg(target_os = "linux")]
    const ENGINE: &str = "WebKitGTK's JavaScript engine";
    /// The stack the engine's thread has: room for a deep program.
    const STACK: usize = 16 << 20;

    /// The functions dm calls.
    struct Api {
        context_create: ContextCreate,
        context_release: OfRef,
        context_group: GetGroup,
        string_create: StringOf,
        string_release: OfRef,
        string_length: Length,
        string_characters: Characters,
        evaluate: Evaluate,
        to_string: ToString,
        protect: Hold,
        unprotect: Hold,
        set_time_limit: SetLimit,
    }

    /// The engine, loaded once, or `None` where this system has none.
    fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(load).as_ref()
    }

    /// Whether the engine is here.
    pub(super) fn here() -> bool {
        api().is_some()
    }

    /// Loads the library and takes its functions.
    fn load() -> Option<Api> {
        LIBRARIES.iter().find_map(|library| {
            if !library.ends_with('\0') {
                return None;
            }
            // SAFETY: `library` is NUL-ended; the handle is never closed, so the functions
            // taken from it stay valid for the process.
            let handle = unsafe {
                libc::dlopen(library.as_ptr().cast(), libc::RTLD_LAZY | libc::RTLD_LOCAL)
            };
            if handle.is_null() {
                return None;
            }
            // SAFETY: `handle` was just opened and is never closed.
            unsafe { take_all(handle) }
        })
    }

    /// The functions dm calls, from an open library.
    ///
    /// # Safety
    ///
    /// `handle` is open and stays so.
    unsafe fn take_all(handle: *mut c_void) -> Option<Api> {
        // SAFETY: each field's type is its function's own (`JSContextRef.h`, `JSStringRef.h`,
        // `JSValueRef.h`, `JSBase.h`; the time limit, `JSContextRefPrivate.h`, exported by the
        // library on macOS and Linux alike), and `handle` is open and never closed.
        unsafe {
            Some(Api {
                context_create: take(handle, "JSGlobalContextCreate\0")?,
                context_release: take(handle, "JSGlobalContextRelease\0")?,
                context_group: take(handle, "JSContextGetGroup\0")?,
                string_create: take(handle, "JSStringCreateWithCharacters\0")?,
                string_release: take(handle, "JSStringRelease\0")?,
                string_length: take(handle, "JSStringGetLength\0")?,
                string_characters: take(handle, "JSStringGetCharactersPtr\0")?,
                evaluate: take(handle, "JSEvaluateScript\0")?,
                to_string: take(handle, "JSValueToStringCopy\0")?,
                protect: take(handle, "JSValueProtect\0")?,
                unprotect: take(handle, "JSValueUnprotect\0")?,
                set_time_limit: take(handle, "JSContextGroupSetExecutionTimeLimit\0")?,
            })
        }
    }

    /// The function `handle` exports under `name` (NUL-ended), as the function pointer type `F`.
    ///
    /// # Safety
    ///
    /// `handle` is open and stays so, and `F` is the type of that function.
    unsafe fn take<F: Copy>(handle: *mut c_void, name: &str) -> Option<F> {
        if !name.ends_with('\0') || std::mem::size_of::<F>() != std::mem::size_of::<*mut c_void>() {
            return None;
        }
        // SAFETY: `handle` is open (the caller's word) and `name` is NUL-ended.
        let found = unsafe { libc::dlsym(handle, name.as_ptr().cast()) };
        if found.is_null() {
            return None;
        }
        // SAFETY: `F` is this function's type (the caller's word), a function pointer of the
        // size of a pointer, checked above.
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&found) })
    }

    /// Runs `program`, then `call`, on a thread of its own, and gives `call`'s value as text.
    pub(super) async fn run(program: &str, call: &str, within: Duration) -> Result<String, String> {
        let (program, call) = (utf16(program), utf16(call));
        let (sent, answer) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("dm-javascript".into())
            .stack_size(STACK)
            .spawn(move || {
                let _ = sent.send(run_here(&program, &call, within));
            })
            .map_err(|e| format!("{ENGINE} could not start: {e}"))?;
        answer.await.map_err(|_| format!("{ENGINE} stopped"))?
    }

    /// [`run`], waited for on this thread: the host's.
    pub(super) fn run_blocking(
        program: &str,
        call: &str,
        within: Duration,
    ) -> Result<String, String> {
        let (program, call) = (utf16(program), utf16(call));
        std::thread::Builder::new()
            .name("dm-javascript".into())
            .stack_size(STACK)
            .spawn(move || run_here(&program, &call, within))
            .map_err(|e| format!("{ENGINE} could not start: {e}"))?
            .join()
            .map_err(|_| format!("{ENGINE} stopped"))?
    }

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// A context, released when dropped.
    struct Context {
        api: &'static Api,
        handle: Ref,
    }

    impl Drop for Context {
        fn drop(&mut self) {
            // SAFETY: the context was made on this thread by `JSGlobalContextCreate` and is
            // released once; values held in it were let go first.
            unsafe { (self.api.context_release)(self.handle) };
        }
    }

    /// [`run`] on this thread.
    fn run_here(program: &[u16], call: &[u16], within: Duration) -> Result<String, String> {
        let api = api().ok_or_else(|| format!("{ENGINE} is not on this computer"))?;
        // SAFETY: no class is given, so the global object is JavaScript's own and nothing else.
        let handle = unsafe { (api.context_create)(null_mut()) };
        if handle.is_null() {
            return Err(format!("{ENGINE} made no context"));
        }
        let context = Context { api, handle };
        // Stopped when its time is up: the group's limit covers the program and the call.
        // SAFETY: the group is the context's own; no callback means the engine stops it.
        unsafe {
            let group = (api.context_group)(context.handle);
            (api.set_time_limit)(group, within.as_secs_f64(), None, null_mut());
        }
        let started = Instant::now();
        let timed_out = |why: String| {
            if started.elapsed() >= within {
                "the JavaScript took too long".to_string()
            } else {
                why
            }
        };
        let answer = evaluate(&context, program)
            .and_then(|value| {
                unhold(&context, value);
                evaluate(&context, call)
            })
            .and_then(|value| {
                let text = text_of(&context, value);
                unhold(&context, value);
                text
            })
            .map_err(timed_out);
        drop(context);
        answer
    }

    /// Runs one script and gives its value, held (`JSValueProtect`) until [`unhold`]n.
    fn evaluate(context: &Context, source: &[u16]) -> Result<Ref, String> {
        let api = context.api;
        // SAFETY: `source` is valid for its length while the string is made; the string is
        // released after the script runs.
        let script = unsafe { (api.string_create)(source.as_ptr(), source.len()) };
        if script.is_null() {
            return Err("the JavaScript could not be read".into());
        }
        let mut exception: Ref = null_mut();
        // SAFETY: the context and script are live; no `this`, source name or line is needed;
        // the out-pointer is valid.
        let value = unsafe {
            (api.evaluate)(
                context.handle,
                script,
                null_mut(),
                null_mut(),
                1,
                &mut exception,
            )
        };
        // SAFETY: made above, released once.
        unsafe { (api.string_release)(script) };
        if value.is_null() || !exception.is_null() {
            let words = if exception.is_null() {
                "an error with no words".to_string()
            } else {
                text_of(context, exception).unwrap_or_else(|_| "an error dm could not read".into())
            };
            return Err(format!("the JavaScript stopped: {words}"));
        }
        // SAFETY: `value` is a value of this context, just given.
        unsafe { (api.protect)(context.handle, value) };
        Ok(value)
    }

    /// Lets the engine collect a value [`evaluate`] held.
    fn unhold(context: &Context, value: Ref) {
        // SAFETY: `value` was held with `JSValueProtect` in this context.
        unsafe { (context.api.unprotect)(context.handle, value) };
    }

    /// A value as text, as `String(value)` gives it.
    fn text_of(context: &Context, value: Ref) -> Result<String, String> {
        let api = context.api;
        let mut exception: Ref = null_mut();
        // SAFETY: `value` is live in this context; the out-pointer is valid.
        let string = unsafe { (api.to_string)(context.handle, value, &mut exception) };
        if string.is_null() || !exception.is_null() {
            return Err("the JavaScript's answer is not text".into());
        }
        // SAFETY: `string` is a string just made; its characters are valid until it is
        // released, and are copied out before that.
        let text = unsafe {
            let length = (api.string_length)(string);
            let chars = (api.string_characters)(string);
            let text = if chars.is_null() {
                String::new()
            } else {
                String::from_utf16_lossy(std::slice::from_raw_parts(chars, length))
            };
            (api.string_release)(string);
            text
        };
        Ok(text)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn run_now(program: &str, call: &str, within: Duration) -> Result<String, String> {
            run_here(&utf16(program), &utf16(call), within)
        }

        /// Whether the engine is here to test: always on macOS, where it is part of the system;
        /// on Linux where WebKitGTK is installed (with `DM_REQUIRE_JSC` set the test fails
        /// without it, as `scripts/linux/inside.sh` sets it).
        fn engine_to_test() -> bool {
            if cfg!(target_os = "macos") {
                assert!(here(), "JavaScriptCore is part of every macOS");
                return true;
            }
            if !here() {
                assert!(
                    std::env::var_os("DM_REQUIRE_JSC").is_none(),
                    "WebKitGTK's JavaScriptCore was asked for and is not here"
                );
                eprintln!("skipped: WebKitGTK's JavaScriptCore is not installed");
            }
            here()
        }

        #[test]
        fn the_engine_runs_a_program_and_gives_a_calls_value() {
            if !engine_to_test() {
                return;
            }
            let got = run_now(
                "var a = function (x) { return x + '€'; };",
                "a('ü') + 1",
                Duration::from_secs(10),
            );
            assert_eq!(got.unwrap(), "ü€1");
            // The top-level `this` is the global object: the program's opening line, as
            // YouTube's player needs it (QuickJS's module mode had it undefined).
            assert_eq!(
                run_now("var g = this;", "g === globalThis", Duration::from_secs(10)).unwrap(),
                "true"
            );
            // A program that throws, or does not parse, says why.
            let why = run_now(
                "throw new TypeError('no such thing');",
                "1",
                Duration::from_secs(10),
            )
            .unwrap_err();
            assert!(
                why.contains("TypeError") && why.contains("no such thing"),
                "{why}"
            );
            let why = run_now("var = ;", "1", Duration::from_secs(10)).unwrap_err();
            assert!(why.contains("the JavaScript stopped"), "{why}");
        }

        #[test]
        fn a_program_that_runs_on_is_stopped_at_its_time() {
            if !engine_to_test() {
                return;
            }
            let started = Instant::now();
            let why = run_now("for (;;) {}", "1", Duration::from_millis(300)).unwrap_err();
            assert_eq!(why, "the JavaScript took too long");
            assert!(started.elapsed() < Duration::from_secs(10));
            // And the next one runs.
            assert_eq!(
                run_now("", "'next'", Duration::from_secs(10)).unwrap(),
                "next"
            );
        }

        /// JavaScriptCore's own `console` is there: it writes to the system's log, and reaches
        /// nothing else.
        #[test]
        fn there_is_nothing_of_the_computer_in_reach() {
            if !engine_to_test() {
                return;
            }
            for name in [
                "require",
                "process",
                "Deno",
                "std",
                "os",
                "XMLHttpRequest",
                "fetch",
                "window",
                "document",
            ] {
                let got = run_now("", &format!("typeof {name}"), Duration::from_secs(10)).unwrap();
                assert_eq!(got, "undefined", "{name} is in reach");
            }
        }

        #[test]
        fn the_host_runs_the_engine_with_no_jit() {
            let host = crate::javascript::Host {
                program: "dm".into(),
                args: Vec::new(),
                env: Vec::new(),
            };
            let command = crate::javascript::host_command(&host);
            let jit = command
                .as_std()
                .get_envs()
                .find(|(name, _)| *name == "JSC_useJIT")
                .and_then(|(_, value)| value);
            assert_eq!(jit, Some(std::ffi::OsStr::new("false")));
        }

        /// Set where this test binary is started as the engine's host.
        const AS_HOST: &str = "DM_JAVASCRIPT_HOST_TEST";

        /// The host's `main`, where this test binary is started as the engine's host (the test
        /// below starts it so); as a test of its own, nothing.
        #[test]
        fn the_engines_host_when_this_binary_is_started_as_one() {
            if std::env::var_os(AS_HOST).is_some() {
                std::process::exit(crate::javascript::host_main());
            }
        }

        #[tokio::test]
        async fn in_a_process_of_its_own_a_program_is_answered_and_one_that_runs_on_is_stopped() {
            if !engine_to_test() {
                return;
            }
            let host = crate::javascript::Host {
                program: std::env::current_exe().unwrap(),
                args: vec![
                    "--exact".into(),
                    "javascript::jsc::tests::the_engines_host_when_this_binary_is_started_as_one"
                        .into(),
                    "--nocapture".into(),
                ],
                env: vec![(AS_HOST.into(), "1".into())],
            };
            let run = |program: &'static str, call: &'static str, within: Duration| {
                let host = host.clone();
                async move { crate::javascript::in_host(&host, program, call, within).await }
            };
            assert_eq!(
                run("var a = 2;", "a * 21 + 'ü'", Duration::from_secs(20))
                    .await
                    .unwrap(),
                "42ü"
            );
            let why = run("throw new RangeError('far');", "1", Duration::from_secs(20))
                .await
                .unwrap_err();
            assert!(why.contains("RangeError") && why.contains("far"), "{why}");
            let why = run("for (;;) {}", "1", Duration::from_millis(500))
                .await
                .unwrap_err();
            assert_eq!(why, "the JavaScript took too long");
            assert_eq!(
                run("", "'still'", Duration::from_secs(20)).await.unwrap(),
                "still"
            );
        }
    }
}

mod command {
    //! An engine run as a program of its own: an installed Deno, else QuickJS found or fetched.

    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    /// What an engine said when the program stopped: its last line that is
    /// not a frame of the stack after it. QuickJS and Deno put the error
    /// first and the frames (`at …`) after, so the last line alone was a
    /// frame ("at <anonymous> (….js:17852:15)") and the error was lost.
    pub(super) fn why_stopped(stderr: &str) -> &str {
        stderr
            .lines()
            .map(str::trim)
            .rev()
            .find(|l| !l.is_empty() && !l.starts_with("at "))
            .unwrap_or("")
    }

    /// Each run's own file name, for runs at the same time.
    static RUNS: AtomicU64 = AtomicU64::new(0);

    /// Runs `program` and prints `call`'s value, in a private file in `staging`.
    pub(super) async fn run(
        program: &str,
        call: &str,
        within: Duration,
        staging: &Path,
    ) -> Result<String, String> {
        // Why QuickJS could not be fetched, kept for the failure that follows.
        let mut not_fetched = None;
        let quickjs = match crate::videopage::quickjs() {
            Some(found) => Some(found),
            None if crate::videopage::deno().is_none() => {
                match crate::tools::ensure(&crate::client(), crate::tools::Tool::QuickJs).await {
                    Ok(fetched) => Some(fetched),
                    Err(e) => {
                        not_fetched = Some(e);
                        None
                    }
                }
            }
            None => None,
        };
        // Deno first where it is installed: run with no permissions it reaches neither files
        // nor programs, which QuickJS's command line does not prevent.
        let Some(engine) = crate::videopage::deno().or(quickjs) else {
            let why = not_fetched.map_or(String::new(), |e| {
                format!(" (QuickJS could not be fetched: {e})")
            });
            return Err(format!("no JavaScript engine to run it in{why}"));
        };
        // In a file of this account's own: QuickJS does not read a program from its input.
        tokio::fs::create_dir_all(staging)
            .await
            .map_err(|e| format!("no room for the JavaScript: {e}"))?;
        let run = RUNS.fetch_add(1, Ordering::Relaxed);
        let file = staging.join(format!(".dm-js-{}-{run}.js", std::process::id()));
        crate::videopage::write_private(&file, &format!("{program}\nconsole.log({call});\n"))
            .await
            .map_err(|e| format!("could not write the JavaScript: {e}"))?;
        let _gone = crate::videopage::RemoveOnDrop(file.clone());
        let mut command = crate::videopage::js_command(&engine, &file);
        crate::no_window_async(&mut command);
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let output = tokio::time::timeout(within, command.output())
            .await
            .map_err(|_| "the JavaScript took too long".to_string())?
            .map_err(|e| format!("the JavaScript engine would not start: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("the JavaScript stopped: {}", why_stopped(&stderr)));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .to_string())
    }
}

#[cfg(all(test, not(windows)))]
mod command_tests {
    #[test]
    fn the_error_is_said_and_not_the_stack_after_it() {
        let stderr = "Error: Cannot find global object\n    at ea (/tmp/p.js:3:9)\n    at <anonymous> (/tmp/p.js:17852:15)\n\n";
        assert_eq!(
            super::command::why_stopped(stderr),
            "Error: Cannot find global object"
        );
        assert_eq!(super::command::why_stopped("it broke\n"), "it broke");
        assert_eq!(super::command::why_stopped(""), "");
    }
}
