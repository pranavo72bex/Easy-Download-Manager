// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's Java side, reached from Rust: the process's Java machine and dm's activity, kept
//! once at start, and the few helpers every call into Android shares. Each call goes through
//! [`with`], which attaches the calling thread (the window's, or one of the engine's) and turns
//! whatever Android throws into dm's words; nothing here panics.

use std::ffi::c_void;
use std::sync::OnceLock;

use jni::errors::Error;
use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{jni_sig, jni_str, Env, JValue, JavaVM};

/// What every call needs: the Java machine and dm's activity (a `Context`).
struct Jvm {
    vm: JavaVM,
    activity: Global<JObject<'static>>,
}

static JVM: OnceLock<Jvm> = OnceLock::new();

/// Keeps the Java machine and the activity android-activity gives (`AndroidApp::vm_as_ptr`,
/// `activity_as_ptr`), with a global reference of dm's own to the activity, so a call from any
/// thread can reach it for as long as the process lives.
pub(crate) fn init(vm: *mut c_void, activity: *mut c_void) -> Result<(), String> {
    if vm.is_null() || activity.is_null() {
        return Err("dm could not reach Android: it gave no Java machine or activity".into());
    }
    // SAFETY: `vm` is android-activity's pointer to the process's one Java machine, not null
    // (checked above) and valid for the whole process.
    let vm = unsafe { JavaVM::from_raw(vm.cast()) };
    let raw = activity as jni::sys::jobject;
    let activity = vm
        .attach_current_thread(|env| -> jni::errors::Result<Global<JObject<'static>>> {
            // SAFETY: `raw` is android-activity's global reference to the activity, valid while
            // its `AndroidApp` lives, which `android_main` holds for this whole call; it is only
            // borrowed here to make dm's own global reference, which is never deleted.
            let borrowed = unsafe { env.as_cast_raw::<Global<JObject<'static>>>(&raw)? };
            env.new_global_ref(&*borrowed)
        })
        .map_err(|e| format!("dm could not reach Android: {}", words(&e)))?;
    // A second start in the same process keeps the first: the Java machine is the same one.
    let _ = JVM.set(Jvm { vm, activity });
    Ok(())
}

/// Runs `call` with this thread attached and dm's activity, `what` saying in dm's words what was
/// being done ("open the file") for the sentence a failure is told in.
pub(crate) fn with<T>(
    what: &str,
    call: impl FnOnce(&mut Env<'_>, &JObject<'static>) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let jvm = JVM
        .get()
        .ok_or_else(|| format!("dm could not {what}: Android was not reached at start"))?;
    jvm.vm
        .attach_current_thread(|env| call(env, &jvm.activity))
        .map_err(|e| format!("dm could not {what}: {}", words(&e)))
}

/// What Android said, without the crate's framing: a Java exception's own message (or its
/// class's name when it has none), else the JNI error.
fn words(e: &Error) -> String {
    match e {
        Error::CaughtJavaException { name, msg, .. } if msg.is_empty() => name.clone(),
        Error::CaughtJavaException { msg, .. } => msg.clone(),
        other => other.to_string(),
    }
}

/// A Java `String` (or `null`) as Rust's.
pub(crate) fn string_of(
    env: &mut Env<'_>,
    obj: JObject<'_>,
) -> jni::errors::Result<Option<String>> {
    if obj.is_null() {
        return Ok(None);
    }
    let text = env.cast_local::<JString>(obj)?;
    text.try_to_string(env).map(Some)
}

/// Any `CharSequence` (or `null`) as Rust's text, through its `toString`.
pub(crate) fn chars_of(env: &mut Env<'_>, obj: JObject<'_>) -> jni::errors::Result<Option<String>> {
    if obj.is_null() {
        return Ok(None);
    }
    let text = env
        .call_method(
            &obj,
            jni_str!("toString"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    string_of(env, text)
}

/// `context.getSystemService(name)`: `"notification"`, `"power"`, `"clipboard"`.
pub(crate) fn service<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
    name: &str,
) -> jni::errors::Result<JObject<'local>> {
    let name = env.new_string(name)?;
    let service = env
        .call_method(
            context,
            jni_str!("getSystemService"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    if service.is_null() {
        return Err(Error::NullPtr("system service"));
    }
    Ok(service)
}

/// The phone's Android version as its API level (`Build.VERSION.SDK_INT`): 26 is Android 8.0.
pub(crate) fn sdk(env: &mut Env<'_>) -> jni::errors::Result<i32> {
    env.get_static_field(
        jni_str!("android/os/Build$VERSION"),
        jni_str!("SDK_INT"),
        jni_sig!(jint),
    )?
    .i()
}

/// A new `Intent` for `action`.
pub(crate) fn intent<'local>(
    env: &mut Env<'local>,
    action: &str,
) -> jni::errors::Result<JObject<'local>> {
    let action = env.new_string(action)?;
    env.new_object(
        jni_str!("android/content/Intent"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(&action)],
    )
}

/// `Intent.FLAG_ACTIVITY_NEW_TASK`: an activity started from outside one of dm's own (a
/// notification, an engine thread) starts in a task of its own.
pub(crate) const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;

/// `intent.addFlags(flags)`.
pub(crate) fn add_flags(
    env: &mut Env<'_>,
    intent: &JObject<'_>,
    flags: i32,
) -> jni::errors::Result<()> {
    env.call_method(
        intent,
        jni_str!("addFlags"),
        jni_sig!("(I)Landroid/content/Intent;"),
        &[JValue::Int(flags)],
    )?;
    Ok(())
}
