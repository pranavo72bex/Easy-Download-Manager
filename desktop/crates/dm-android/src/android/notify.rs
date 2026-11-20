// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's notifications (gaps rows N5, N6): dm's two channels, a message (a link added, a
//! download finished or failed; a finished file's tap opens it), the ongoing one while anything
//! downloads, the permissions asked for at start, and the partial wake lock held while busy.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;

use jni::objects::JObject;
use jni::refs::Global;
use jni::strings::JNIStr;
use jni::{jni_sig, jni_str, Env, JValue};

use super::{jvm, open};

/// Finished, failed and added: a sound, as the desktop's notifications have.
const DONE: &str = "dm.done";
/// The ongoing one: no sound, low in the shade.
const RUNNING: &str = "dm.running";
/// The ongoing notification's number; the others count up from 100.
const ONGOING_ID: i32 = 1;
static NEXT_ID: AtomicI32 = AtomicI32::new(100);

/// `NotificationManager.IMPORTANCE_LOW` and `IMPORTANCE_DEFAULT`.
const IMPORTANCE_LOW: i32 = 2;
const IMPORTANCE_DEFAULT: i32 = 3;
/// `PendingIntent.FLAG_IMMUTABLE | FLAG_UPDATE_CURRENT` (immutable is required from Android 12).
const PENDING_FLAGS: i32 = 0x0400_0000 | 0x0800_0000;
/// `PowerManager.PARTIAL_WAKE_LOCK`: the processor stays on, the screen may go off.
const PARTIAL_WAKE_LOCK: i32 = 1;

/// Makes dm's channels (Android 8 and up shows nothing without one); making one again is a no-op.
pub(crate) fn channels() -> Result<(), String> {
    jvm::with("set up its notifications", |env, context| {
        let manager = jvm::service(env, context, "notification")?;
        for (id, name, importance) in [
            (DONE, "Finished and failed downloads", IMPORTANCE_DEFAULT),
            (RUNNING, "Downloads in progress", IMPORTANCE_LOW),
        ] {
            let id = env.new_string(id)?;
            let name = env.new_string(name)?;
            let channel = env.new_object(
                jni_str!("android/app/NotificationChannel"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/CharSequence;I)V"),
                &[
                    JValue::Object(&id),
                    JValue::Object(&name),
                    JValue::Int(importance),
                ],
            )?;
            env.call_method(
                &manager,
                jni_str!("createNotificationChannel"),
                jni_sig!("(Landroid/app/NotificationChannel;)V"),
                &[JValue::Object(&channel)],
            )?;
        }
        Ok(())
    })
}

/// Asks, once, for what the phone grants only at run time: notifications on Android 13 and up,
/// writing to the shared Download folder on Android 8 and 9. The answer comes later; until it
/// does, notifications are not shown and the first start downloads into dm's own folder.
pub(crate) fn ask_permissions() -> Result<(), String> {
    jvm::with("ask for its permissions", |env, activity| {
        let sdk = jvm::sdk(env)?;
        let mut wanted = Vec::new();
        if sdk >= 33 {
            wanted.push("android.permission.POST_NOTIFICATIONS");
        }
        if sdk <= 28 {
            wanted.push("android.permission.WRITE_EXTERNAL_STORAGE");
        }
        let mut missing = Vec::new();
        for name in wanted {
            let name = env.new_string(name)?;
            let granted = env
                .call_method(
                    activity,
                    jni_str!("checkSelfPermission"),
                    jni_sig!("(Ljava/lang/String;)I"),
                    &[JValue::Object(&name)],
                )?
                .i()?;
            // `PackageManager.PERMISSION_GRANTED` is 0.
            if granted != 0 {
                missing.push(name);
            }
        }
        if missing.is_empty() {
            return Ok(());
        }
        let length = i32::try_from(missing.len()).unwrap_or(i32::MAX);
        let names = env.new_object_array(length, jni_str!("java/lang/String"), JObject::null())?;
        for (i, name) in missing.iter().enumerate() {
            names.set_element(env, i, name)?;
        }
        env.call_method(
            activity,
            jni_str!("requestPermissions"),
            jni_sig!("([Ljava/lang/String;I)V"),
            &[JValue::Object(&names), JValue::Int(1)],
        )?;
        Ok(())
    })
}

/// A message in the shade: `open` is a finished file a tap opens; without one a tap brings dm
/// to the front.
pub(crate) fn post(title: &str, body: &str, open: Option<&str>) -> Result<(), String> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    jvm::with("show a notification", |env, context| {
        let tap = match open {
            Some(path) => open::view(env, context, path)?,
            None => launch(env, context)?,
        };
        let builder = builder(
            env,
            context,
            DONE,
            jni_str!("stat_sys_download_done"),
            title,
            body,
        )?;
        tap_opens(env, context, &builder, &tap, id)?;
        set_bool(env, &builder, jni_str!("setAutoCancel"), true)?;
        show(env, context, &builder, id)
    })
}

/// The ongoing notification (gaps row N6): put up, or changed in place, while anything
/// downloads; a tap brings dm to the front.
pub(crate) fn ongoing(title: &str, body: &str) -> Result<(), String> {
    jvm::with("show its progress", |env, context| {
        let tap = launch(env, context)?;
        let builder = builder(
            env,
            context,
            RUNNING,
            jni_str!("stat_sys_download"),
            title,
            body,
        )?;
        tap_opens(env, context, &builder, &tap, ONGOING_ID)?;
        set_bool(env, &builder, jni_str!("setOngoing"), true)?;
        set_bool(env, &builder, jni_str!("setOnlyAlertOnce"), true)?;
        show(env, context, &builder, ONGOING_ID)
    })
}

/// Takes the ongoing notification down: nothing downloads.
pub(crate) fn end_ongoing() -> Result<(), String> {
    jvm::with("take its progress down", |env, context| {
        let manager = jvm::service(env, context, "notification")?;
        env.call_method(
            &manager,
            jni_str!("cancel"),
            jni_sig!("(I)V"),
            &[JValue::Int(ONGOING_ID)],
        )?;
        Ok(())
    })
}

/// The partial wake lock dm holds while it downloads.
static WAKE_LOCK: Mutex<Option<Global<JObject<'static>>>> = Mutex::new(None);

/// Holds the processor awake while `wanted` (anything downloads), and lets it sleep after.
/// The screen still goes off; Android may yet stop a process left in the background, which
/// only a foreground service prevents (gaps row N6).
pub(crate) fn stay_awake(wanted: bool) -> Result<(), String> {
    let mut held = dm_core::lock(&WAKE_LOCK);
    if wanted == held.is_some() {
        return Ok(());
    }
    if wanted {
        let lock = jvm::with("keep the phone awake while it downloads", |env, context| {
            let power = jvm::service(env, context, "power")?;
            let tag = env.new_string("dm:downloading")?;
            let lock = env
                .call_method(
                    &power,
                    jni_str!("newWakeLock"),
                    jni_sig!("(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;"),
                    &[JValue::Int(PARTIAL_WAKE_LOCK), JValue::Object(&tag)],
                )?
                .l()?;
            env.call_method(
                &lock,
                jni_str!("setReferenceCounted"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(false)],
            )?;
            env.call_method(&lock, jni_str!("acquire"), jni_sig!("()V"), &[])?;
            env.new_global_ref(&lock)
        })?;
        *held = Some(lock);
    } else if let Some(lock) = held.take() {
        jvm::with("let the phone sleep", |env, _| {
            env.call_method(&*lock, jni_str!("release"), jni_sig!("()V"), &[])?;
            Ok(())
        })?;
    }
    Ok(())
}

/// A `Notification.Builder` on `channel` with Android's own `icon` (`android.R.drawable`), the
/// title and the body.
fn builder<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
    channel: &str,
    icon: &JNIStr,
    title: &str,
    body: &str,
) -> jni::errors::Result<JObject<'local>> {
    let channel = env.new_string(channel)?;
    let builder = env.new_object(
        jni_str!("android/app/Notification$Builder"),
        jni_sig!("(Landroid/content/Context;Ljava/lang/String;)V"),
        &[JValue::Object(context), JValue::Object(&channel)],
    )?;
    let icon = env
        .get_static_field(jni_str!("android/R$drawable"), icon, jni_sig!(jint))?
        .i()?;
    env.call_method(
        &builder,
        jni_str!("setSmallIcon"),
        jni_sig!("(I)Landroid/app/Notification$Builder;"),
        &[JValue::Int(icon)],
    )?;
    let title = env.new_string(title)?;
    env.call_method(
        &builder,
        jni_str!("setContentTitle"),
        jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"),
        &[JValue::Object(&title)],
    )?;
    let body = env.new_string(body)?;
    env.call_method(
        &builder,
        jni_str!("setContentText"),
        jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"),
        &[JValue::Object(&body)],
    )?;
    Ok(builder)
}

/// One of the builder's `(Z)Builder` setters.
fn set_bool(
    env: &mut Env<'_>,
    builder: &JObject<'_>,
    name: &JNIStr,
    value: bool,
) -> jni::errors::Result<()> {
    env.call_method(
        builder,
        name,
        jni_sig!("(Z)Landroid/app/Notification$Builder;"),
        &[JValue::Bool(value)],
    )?;
    Ok(())
}

/// A tap on the notification starts `intent` (an activity), through a `PendingIntent` numbered
/// as the notification is.
fn tap_opens(
    env: &mut Env<'_>,
    context: &JObject<'_>,
    builder: &JObject<'_>,
    intent: &JObject<'_>,
    id: i32,
) -> jni::errors::Result<()> {
    let pending = env
        .call_static_method(
            jni_str!("android/app/PendingIntent"),
            jni_str!("getActivity"),
            jni_sig!(
                "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;"
            ),
            &[
                JValue::Object(context),
                JValue::Int(id),
                JValue::Object(intent),
                JValue::Int(PENDING_FLAGS),
            ],
        )?
        .l()?;
    env.call_method(
        builder,
        jni_str!("setContentIntent"),
        jni_sig!("(Landroid/app/PendingIntent;)Landroid/app/Notification$Builder;"),
        &[JValue::Object(&pending)],
    )?;
    Ok(())
}

/// Builds the notification and shows it as number `id` (a second show of a number replaces it).
fn show(
    env: &mut Env<'_>,
    context: &JObject<'_>,
    builder: &JObject<'_>,
    id: i32,
) -> jni::errors::Result<()> {
    let notification = env
        .call_method(
            builder,
            jni_str!("build"),
            jni_sig!("()Landroid/app/Notification;"),
            &[],
        )?
        .l()?;
    let manager = jvm::service(env, context, "notification")?;
    env.call_method(
        &manager,
        jni_str!("notify"),
        jni_sig!("(ILandroid/app/Notification;)V"),
        &[JValue::Int(id), JValue::Object(&notification)],
    )?;
    Ok(())
}

/// The intent that brings dm itself to the front (its launcher's).
fn launch<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
) -> jni::errors::Result<JObject<'local>> {
    let packages = env
        .call_method(
            context,
            jni_str!("getPackageManager"),
            jni_sig!("()Landroid/content/pm/PackageManager;"),
            &[],
        )?
        .l()?;
    let name = env
        .call_method(
            context,
            jni_str!("getPackageName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let intent = env
        .call_method(
            &packages,
            jni_str!("getLaunchIntentForPackage"),
            jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    if intent.is_null() {
        return Err(jni::errors::Error::NullPtr("dm's launch intent"));
    }
    Ok(intent)
}
