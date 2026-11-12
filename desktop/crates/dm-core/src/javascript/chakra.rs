// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
    use windows_sys::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
    let name: Vec<u16> = "chakra.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: `name` is a NUL-ended wide string that outlives the call. Only System32 is
    // searched, so no file of the same name elsewhere is loaded in its place. The library
    // is never freed, so the functions taken from it stay valid for the process.
    let module = unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
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
unsafe fn take<F: Copy>(module: windows_sys::Win32::Foundation::HMODULE, name: &str) -> Option<F> {
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
pub(super) fn run_blocking(program: &str, call: &str, within: Duration) -> Result<String, String> {
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
        THREW | NO_PARSE | NO_EVAL => Err(format!("the JavaScript stopped: {}", exception(api))),
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
