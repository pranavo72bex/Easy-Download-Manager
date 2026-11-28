// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The native methods of EDM's four Java classes (`java/com/edm/android`, gaps rows N5, N6,
//! N12), each class small and doing nothing but hand its event here. They are found by their
//! exported names, `Java_com_edm_android_<Class>_<method>`, as gpui-android's own are
//! (`Java_dev_gpui_mobile_GpuiInputActivity_nativeIme`): each class loads `libedm_android.so`
//! with `System.loadLibrary` (the activity through gpui-mobile's `onCreate`), so they resolve
//! also where Android starts EDM's process for a receiver or the service alone, before or
//! without `android_main`.
//!
//! Each takes what Java handed it under the caller's frame (keeping the Java machine and a
//! context where none is kept yet, `jvm::adopt`), then leaves that frame and does its work
//! through `jvm::with` as every other call does. Nothing unwinds into Java: jni's `with_env`
//! catches a panic, and a failure is logged in EDM's words.

use jni::errors::LogErrorAndDefault;
use jni::objects::{JClass, JObject, JString};
use jni::refs::Global;
use jni::{Env, EnvUnowned};

use super::{intent, jvm, receivers, service};

/// A failure logged in EDM's words: no one else is there to tell.
fn said(done: Result<(), String>) {
    if let Err(why) = done {
        edm_core::note!(warn, "{why}");
    }
}

/// `context` kept where none is, and a global reference to `object` for after the call.
fn adopted(
    env: &mut Env<'_>,
    context: &JObject<'_>,
    object: &JObject<'_>,
) -> jni::errors::Result<Option<Global<JObject<'static>>>> {
    jvm::adopt(env, context)?;
    env.new_global_ref(object).map(Some)
}

/// A Java `String` (or `null`) as Rust's.
fn text(env: &mut Env<'_>, text: &JString<'_>) -> jni::errors::Result<Option<String>> {
    if text.is_null() {
        return Ok(None);
    }
    text.try_to_string(env).map(Some)
}

/// `EdmActivity.onNewIntent`: the new intent, already the activity's, read as the first one is
/// (`intent::take`), on a thread of its own so that copying a `.torrent` keeps the activity's
/// thread free.
#[no_mangle]
pub extern "system" fn Java_com_edm_android_EdmActivity_newIntent<'caller>(
    _env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    let Some(home) = receivers::started() else {
        return;
    };
    let torrents = home.join("torrents");
    let taken = std::thread::Builder::new()
        .name("edm-new-intent".into())
        .spawn(move || intent::take(&torrents));
    if let Err(e) = taken {
        edm_core::note!(warn, "EDM could not take what was shared: {e}");
    }
}

/// `EdmActivity`'s insets listener: what the status bar, the navigation bar and a cut-out
/// cover, in the screen's pixels, kept as the window's safe area (gpui-android's
/// `set_system_insets`; it touches nothing of Java's).
#[no_mangle]
pub extern "system" fn Java_com_edm_android_EdmActivity_systemInsets<'caller>(
    _env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    top: jni::sys::jint,
    bottom: jni::sys::jint,
    left: jni::sys::jint,
    right: jni::sys::jint,
) {
    gpui_android::android::jni::set_system_insets(top, bottom, left, right);
}

/// `EdmService.onStartCommand`.
#[no_mangle]
pub extern "system" fn Java_com_edm_android_EdmService_started<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    service: JObject<'caller>,
) {
    let held = env
        .with_env(|env| adopted(env, &service, &service))
        .resolve::<LogErrorAndDefault>();
    if let Some(held) = held {
        said(service::started(held));
    }
}

/// `EdmService.onTimeout` (Android 15).
#[no_mangle]
pub extern "system" fn Java_com_edm_android_EdmService_timedOut<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    service: JObject<'caller>,
) {
    let held = env
        .with_env(|env| adopted(env, &service, &service))
        .resolve::<LogErrorAndDefault>();
    if let Some(held) = held {
        said(service::timed_out(held));
    }
}

/// `EdmService.onDestroy`.
#[no_mangle]
pub extern "system" fn Java_com_edm_android_EdmService_stopped<'caller>(
    _env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    service::stopped();
}

/// `EdmActionReceiver.onReceive`: a notification's button or the schedule's alarm.
#[no_mangle]
pub extern "system" fn Java_com_edm_android_EdmActionReceiver_received<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
    action: JString<'caller>,
    row: jni::sys::jlong,
    note: jni::sys::jint,
) {
    let action = env
        .with_env(|env| -> jni::errors::Result<Option<String>> {
            jvm::adopt(env, &context)?;
            text(env, &action)
        })
        .resolve::<LogErrorAndDefault>();
    said(receivers::received(action.as_deref(), row, note));
}

/// `EdmBootReceiver.onReceive`: the phone has started.
#[no_mangle]
pub extern "system" fn Java_com_edm_android_EdmBootReceiver_booted<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) {
    let adopted = env
        .with_env(|env| jvm::adopt(env, &context).map(|()| true))
        .resolve::<LogErrorAndDefault>();
    if adopted {
        said(receivers::booted());
    }
}
