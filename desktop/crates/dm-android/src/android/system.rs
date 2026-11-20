// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! [`AndroidSystem`] on a phone, over JNI: notifications, the clipboard, opening a file, the
//! system's Downloads, the notification sound, and the window drawn again when the list changes.
//! Every call comes from one of the engine's threads; a failure is said in dm's words (or, where
//! the engine wants none, let go), never a panic.

use std::path::Path;
use std::time::Duration;

use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JValue};

use super::{jvm, notify, open};
use crate::AndroidSystem;

/// The phone's side of the engine.
pub(crate) struct Droid;

impl AndroidSystem for Droid {
    fn notify(&self, title: String, body: String, path: Option<String>, _failed: Option<u64>) {
        // A Retry button on a failure needs a broadcast receiver, a Java class (docs/gaps.md,
        // N6); until then the failure's notification brings dm to the front.
        let Some(path) = path else {
            let _ = notify::post(&title, &body, None);
            return;
        };
        // A finished file is added to the media index first, so the tap lends it by a
        // `content://` address (`open.rs`); the index takes a moment, so the notification
        // waits for it off the engine's thread.
        let _ = open::scan(&path);
        let posted = std::thread::Builder::new()
            .name("dm-notify".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                let _ = notify::post(&title, &body, Some(&path));
            });
        drop(posted);
    }

    fn clipboard(&self) -> Option<String> {
        // Android 10 and up answer only while dm's window has the focus; `None` otherwise.
        jvm::with("read the clipboard", |env, context| {
            let manager = jvm::service(env, context, "clipboard")?;
            let clip = env
                .call_method(
                    &manager,
                    jni_str!("getPrimaryClip"),
                    jni_sig!("()Landroid/content/ClipData;"),
                    &[],
                )?
                .l()?;
            if clip.is_null() {
                return Ok(None);
            }
            let count = env
                .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
                .i()?;
            if count < 1 {
                return Ok(None);
            }
            let item = env
                .call_method(
                    &clip,
                    jni_str!("getItemAt"),
                    jni_sig!("(I)Landroid/content/ClipData$Item;"),
                    &[JValue::Int(0)],
                )?
                .l()?;
            let text = env
                .call_method(
                    &item,
                    jni_str!("coerceToText"),
                    jni_sig!("(Landroid/content/Context;)Ljava/lang/CharSequence;"),
                    &[JValue::Object(context)],
                )?
                .l()?;
            jvm::chars_of(env, text)
        })
        .ok()
        .flatten()
        .filter(|text| !text.is_empty())
    }

    fn set_clipboard(&self, text: String) {
        let _ = jvm::with("put the link on the clipboard", |env, context| {
            let manager = jvm::service(env, context, "clipboard")?;
            let label = env.new_string("dm")?;
            let text = env.new_string(text)?;
            let clip = env
                .call_static_method(
                    jni_str!("android/content/ClipData"),
                    jni_str!("newPlainText"),
                    jni_sig!(
                        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"
                    ),
                    &[JValue::Object(&label), JValue::Object(&text)],
                )?
                .l()?;
            env.call_method(
                &manager,
                jni_str!("setPrimaryClip"),
                jni_sig!("(Landroid/content/ClipData;)V"),
                &[JValue::Object(&clip)],
            )?;
            Ok(())
        });
    }

    fn open(&self, path: String) -> Option<String> {
        let name = Path::new(&path)
            .file_name()
            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        jvm::with("open the file", |env, context| {
            let intent = open::view(env, context, &path)?;
            start(env, context, &intent)
        })
        .err()
        .map(|why| no_app(why, &format!("no app on this phone opens {name}")))
    }

    fn reveal(&self, _path: String) -> Option<String> {
        // A phone shows no one folder of a file: its Downloads app lists what was downloaded.
        jvm::with("show the phone's Downloads", |env, context| {
            let intent = jvm::intent(env, "android.intent.action.VIEW_DOWNLOADS")?;
            jvm::add_flags(env, &intent, jvm::FLAG_ACTIVITY_NEW_TASK)?;
            start(env, context, &intent)
        })
        .err()
        .map(|why| no_app(why, "this phone has no Downloads app to show the file in"))
    }

    fn sound(&self, _failed: bool) {
        // The phone's own notification sound, for a finish and a failure alike.
        let _ = jvm::with("play a sound", |env, context| {
            // `RingtoneManager.TYPE_NOTIFICATION`.
            let uri = env
                .call_static_method(
                    jni_str!("android/media/RingtoneManager"),
                    jni_str!("getDefaultUri"),
                    jni_sig!("(I)Landroid/net/Uri;"),
                    &[JValue::Int(2)],
                )?
                .l()?;
            let ringtone = env
                .call_static_method(
                    jni_str!("android/media/RingtoneManager"),
                    jni_str!("getRingtone"),
                    jni_sig!(
                        "(Landroid/content/Context;Landroid/net/Uri;)Landroid/media/Ringtone;"
                    ),
                    &[JValue::Object(context), JValue::Object(&uri)],
                )?
                .l()?;
            if !ringtone.is_null() {
                env.call_method(&ringtone, jni_str!("play"), jni_sig!("()V"), &[])?;
            }
            Ok(())
        });
    }

    fn changed(&self) {
        crate::app::refresh();
    }
}

/// `context.startActivity(intent)`.
fn start(
    env: &mut Env<'_>,
    context: &JObject<'_>,
    intent: &JObject<'_>,
) -> jni::errors::Result<()> {
    env.call_method(
        context,
        jni_str!("startActivity"),
        jni_sig!("(Landroid/content/Intent;)V"),
        &[JValue::Object(intent)],
    )?;
    Ok(())
}

/// `why`, or `instead` when it is Android's "no activity found" (no app takes the intent).
fn no_app(why: String, instead: &str) -> String {
    if why.contains("No Activity found") || why.contains("ActivityNotFoundException") {
        instead.to_owned()
    } else {
        why
    }
}
