// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's Java side, reached from Rust: the process's Java machine and EDM's activity, kept
//! at start (or the application's context, where a receiver started the process), and the few
//! helpers every call into Android shares. Each call goes through [`with`], which attaches the
//! calling thread (the window's, or one of the engine's) and turns whatever Android throws into
//! EDM's words; nothing here panics.

use std::ffi::c_void;
use std::sync::{Arc, OnceLock, RwLock};

use jni::errors::Error;
use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{jni_sig, jni_str, Env, JValue, JavaVM};

/// The process's one Java machine.
static VM: OnceLock<JavaVM> = OnceLock::new();

/// The `Context` every call is made with: EDM's activity once it has started (`init`), else the
/// application's, kept by a receiver or the service started without the activity (`adopt`:
/// the phone's start, the schedule's alarm, gaps rows N6, N12).
static CONTEXT: RwLock<Option<Arc<Global<JObject<'static>>>>> = RwLock::new(None);

/// Keeps the Java machine and the activity android-activity gives (`AndroidApp::vm_as_ptr`,
/// `activity_as_ptr`), with a global reference of EDM's own to the activity, so a call from any
/// thread can reach it for as long as the process lives. The activity takes the place of an
/// application context a receiver kept before it.
pub(crate) fn init(vm: *mut c_void, activity: *mut c_void) -> Result<(), String> {
    if vm.is_null() || activity.is_null() {
        return Err("EDM could not reach Android: it gave no Java machine or activity".into());
    }
    // A Java machine kept already (a receiver ran first in this process) is the same one.
    let vm = VM.get_or_init(|| {
        // SAFETY: `vm` is android-activity's pointer to the process's one Java machine, not
        // null (checked above) and valid for the whole process.
        unsafe { JavaVM::from_raw(vm.cast()) }
    });
    let raw = activity as jni::sys::jobject;
    let activity = vm
        .attach_current_thread(|env| -> jni::errors::Result<Global<JObject<'static>>> {
            // SAFETY: `raw` is android-activity's global reference to the activity, valid while
            // its `AndroidApp` lives, which `android_main` holds for this whole call; it is only
            // borrowed here to make EDM's own global reference, which is never deleted.
            let borrowed = unsafe { env.as_cast_raw::<Global<JObject<'static>>>(&raw)? };
            env.new_global_ref(&*borrowed)
        })
        .map_err(|e| format!("EDM could not reach Android: {}", words(&e)))?;
    keep(Some(Arc::new(activity)));
    Ok(())
}

/// Called first by each of EDM's Java entries (`natives`): keeps the Java machine and, where no
/// context is kept yet (the process was started for a receiver or the service, not the
/// activity), `context`'s application context, so `with` works there too.
pub(crate) fn adopt(env: &mut Env<'_>, context: &JObject<'_>) -> jni::errors::Result<()> {
    if VM.get().is_none() {
        let vm = env.get_java_vm()?;
        let _ = VM.set(vm);
    }
    if context.is_null() || kept().is_some() {
        return Ok(());
    }
    let app = env
        .call_method(
            context,
            jni_str!("getApplicationContext"),
            jni_sig!("()Landroid/content/Context;"),
            &[],
        )?
        .l()?;
    let global = if app.is_null() {
        env.new_global_ref(context)?
    } else {
        env.new_global_ref(&app)?
    };
    // A context kept meanwhile by another thread stands.
    if kept().is_none() {
        keep(Some(Arc::new(global)));
    }
    Ok(())
}

/// The context kept, if any.
fn kept() -> Option<Arc<Global<JObject<'static>>>> {
    match CONTEXT.read() {
        Ok(held) => held.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Keeps `context` in place of the one kept.
fn keep(context: Option<Arc<Global<JObject<'static>>>>) {
    match CONTEXT.write() {
        Ok(mut held) => *held = context,
        Err(poisoned) => *poisoned.into_inner() = context,
    }
}

/// Runs `call` with this thread attached and EDM's context (its activity, where it has
/// started), `what` saying in EDM's words what was being done ("open the file") for the
/// sentence a failure is told in.
pub(crate) fn with<T>(
    what: &str,
    call: impl FnOnce(&mut Env<'_>, &JObject<'static>) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let unreached = || format!("EDM could not {what}: Android was not reached at start");
    let vm = VM.get().ok_or_else(unreached)?;
    let context = kept().ok_or_else(unreached)?;
    vm.attach_current_thread(|env| call(env, &context))
        .map_err(|e| format!("EDM could not {what}: {}", words(&e)))
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

/// The phone's Android version as its API level (`Build.VERSION.SDK_INT`): 29 is Android 10, the oldest EDM takes.
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

/// `Intent.FLAG_ACTIVITY_NEW_TASK`: an activity started from outside one of EDM's own (a
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
