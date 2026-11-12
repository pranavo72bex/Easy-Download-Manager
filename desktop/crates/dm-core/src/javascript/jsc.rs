// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
        let handle =
            unsafe { libc::dlopen(library.as_ptr().cast(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
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
pub(super) fn run_blocking(program: &str, call: &str, within: Duration) -> Result<String, String> {
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
