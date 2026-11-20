// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! [`crate::phone::Java`] over JNI: notifications, the media index, opening a file, the
//! system's Downloads, the clipboard, the ongoing notification, the wake lock and a finished
//! file published to the shared Download folder (`storage`). Every call comes from one of dm's
//! threads; a failure is said in dm's words, never a panic.

use std::path::{Path, PathBuf};

use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JValue};

use super::storage::Storage;
use super::{jvm, notify, open};
use crate::phone::Java;

/// The phone's Java side, as dm's services call it, with the phone's storage worked out at
/// start.
pub(crate) struct Jni {
    storage: Storage,
}

impl Jni {
    /// The Java side, publishing finished files as `storage` says.
    pub(crate) fn new(storage: Storage) -> Self {
        Jni { storage }
    }
}

impl Java for Jni {
    fn post(&self, title: &str, body: &str, open: Option<&str>) -> Result<(), String> {
        notify::post(title, body, open)
    }

    fn index(&self, path: &str) -> Result<(), String> {
        open::scan(path)
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
    }

    fn set_clipboard(&self, text: &str) {
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

    fn open(&self, path: &str) -> Result<(), String> {
        let name = std::path::Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        jvm::with("open the file", |env, context| {
            let intent = open::view(env, context, path)?;
            start(env, context, &intent)
        })
        .map_err(|why| no_app(why, &format!("no app on this phone opens {name}")))
    }

    fn reveal(&self, _path: &str) -> Result<(), String> {
        // A phone shows no one folder of a file: its Downloads app lists what was downloaded.
        jvm::with("show the phone's Downloads", |env, context| {
            let intent = jvm::intent(env, "android.intent.action.VIEW_DOWNLOADS")?;
            jvm::add_flags(env, &intent, jvm::FLAG_ACTIVITY_NEW_TASK)?;
            start(env, context, &intent)
        })
        .map_err(|why| no_app(why, "this phone has no Downloads app to show the file in"))
    }

    fn ongoing(&self, title: &str, body: &str) -> Result<(), String> {
        notify::ongoing(title, body)
    }

    fn end_ongoing(&self) -> Result<(), String> {
        notify::end_ongoing()
    }

    fn stay_awake(&self, on: bool) -> Result<(), String> {
        notify::stay_awake(on)
    }

    fn publish(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        self.storage.publish(path)
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
