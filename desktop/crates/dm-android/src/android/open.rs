// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Handing a finished file to the app the phone opens it with.
//!
//! Android passes files between apps by `content://` address. The usual way to make one,
//! androidx's `FileProvider`, is a Java class an APK without Java code (dm's: NativeActivity,
//! built by cargo-apk, no Gradle) cannot carry. So dm asks the system's media index instead:
//! a file dm wrote under the shared Download folder is added to it (`scan`, at the moment it
//! finishes) and then has a `content://media/…` address of its own, which dm owns and may lend
//! with `FLAG_GRANT_READ_URI_PERMISSION`. A file the index does not hold (dm's own folder, or
//! one not indexed yet) goes as a `file://` address: since Android 7 the VM's StrictMode policy
//! refuses those with an exception, so dm sets an empty policy once at start
//! ([`allow_file_uris`]); the app given it reads it where the phone lets it (Android 8 and 9,
//! and media files on later ones).

use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JValue};

use super::jvm;

/// `Intent.FLAG_GRANT_READ_URI_PERMISSION`.
const GRANT_READ: i32 = 0x0000_0001;

/// Sets an empty StrictMode VM policy, so a `file://` address may leave dm (see the module's
/// comment). Called once at start; nothing else in dm sets a policy.
pub(crate) fn allow_file_uris() -> Result<(), String> {
    jvm::with("set up opening files", |env, _| {
        let builder = env.new_object(
            jni_str!("android/os/StrictMode$VmPolicy$Builder"),
            jni_sig!("()V"),
            &[],
        )?;
        let policy = env
            .call_method(
                &builder,
                jni_str!("build"),
                jni_sig!("()Landroid/os/StrictMode$VmPolicy;"),
                &[],
            )?
            .l()?;
        env.call_static_method(
            jni_str!("android/os/StrictMode"),
            jni_str!("setVmPolicy"),
            jni_sig!("(Landroid/os/StrictMode$VmPolicy;)V"),
            &[JValue::Object(&policy)],
        )?;
        Ok(())
    })
}

/// Adds a finished file to the system's media index (in the background: it is there a moment
/// later), so it gets the `content://` address [`view`] lends.
pub(crate) fn scan(path: &str) -> Result<(), String> {
    jvm::with("add the file to the phone's index", |env, context| {
        let path = env.new_string(path)?;
        let paths = env.new_object_array(1, jni_str!("java/lang/String"), &path)?;
        env.call_static_method(
            jni_str!("android/media/MediaScannerConnection"),
            jni_str!("scanFile"),
            jni_sig!(
                "(Landroid/content/Context;[Ljava/lang/String;[Ljava/lang/String;Landroid/media/MediaScannerConnection$OnScanCompletedListener;)V"
            ),
            &[
                JValue::Object(context),
                JValue::Object(&paths),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
            ],
        )?;
        Ok(())
    })
}

/// An `ACTION_VIEW` intent for the file at `path`, of its type, with reading lent to the app
/// that takes it, ready to start from outside an activity.
pub(crate) fn view<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
    path: &str,
) -> jni::errors::Result<JObject<'local>> {
    let uri = match indexed(env, context, path)? {
        Some(uri) => uri,
        None => file_uri(env, path)?,
    };
    let kind = mime_type(env, path)?;
    let kind = env.new_string(kind)?;
    let intent = jvm::intent(env, "android.intent.action.VIEW")?;
    env.call_method(
        &intent,
        jni_str!("setDataAndType"),
        jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;"),
        &[JValue::Object(&uri), JValue::Object(&kind)],
    )?;
    jvm::add_flags(env, &intent, GRANT_READ | jvm::FLAG_ACTIVITY_NEW_TASK)?;
    Ok(intent)
}

/// `path`'s address in the media index (`MediaStore.Files`, the `external` volume), where the
/// index holds it.
fn indexed<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
    path: &str,
) -> jni::errors::Result<Option<JObject<'local>>> {
    let volume = env.new_string("external")?;
    let files = env
        .call_static_method(
            jni_str!("android/provider/MediaStore$Files"),
            jni_str!("getContentUri"),
            jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
            &[JValue::Object(&volume)],
        )?
        .l()?;
    let resolver = env
        .call_method(
            context,
            jni_str!("getContentResolver"),
            jni_sig!("()Landroid/content/ContentResolver;"),
            &[],
        )?
        .l()?;
    let id = env.new_string("_id")?;
    let projection = env.new_object_array(1, jni_str!("java/lang/String"), &id)?;
    let selection = env.new_string("_data=?")?;
    let path = env.new_string(path)?;
    let arguments = env.new_object_array(1, jni_str!("java/lang/String"), &path)?;
    let cursor = env
        .call_method(
            &resolver,
            jni_str!("query"),
            jni_sig!(
                "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;"
            ),
            &[
                JValue::Object(&files),
                JValue::Object(&projection),
                JValue::Object(&selection),
                JValue::Object(&arguments),
                JValue::Object(&JObject::null()),
            ],
        )?
        .l()?;
    if cursor.is_null() {
        return Ok(None);
    }
    let found = if env
        .call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?
        .z()?
    {
        Some(
            env.call_method(
                &cursor,
                jni_str!("getLong"),
                jni_sig!("(I)J"),
                &[JValue::Int(0)],
            )?
            .j()?,
        )
    } else {
        None
    };
    env.call_method(&cursor, jni_str!("close"), jni_sig!("()V"), &[])?;
    let Some(id) = found else {
        return Ok(None);
    };
    let uri = env
        .call_static_method(
            jni_str!("android/content/ContentUris"),
            jni_str!("withAppendedId"),
            jni_sig!("(Landroid/net/Uri;J)Landroid/net/Uri;"),
            &[JValue::Object(&files), JValue::Long(id)],
        )?
        .l()?;
    Ok(Some(uri))
}

/// `Uri.fromFile(new File(path))`.
fn file_uri<'local>(env: &mut Env<'local>, path: &str) -> jni::errors::Result<JObject<'local>> {
    let path = env.new_string(path)?;
    let file = env.new_object(
        jni_str!("java/io/File"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(&path)],
    )?;
    env.call_static_method(
        jni_str!("android/net/Uri"),
        jni_str!("fromFile"),
        jni_sig!("(Ljava/io/File;)Landroid/net/Uri;"),
        &[JValue::Object(&file)],
    )?
    .l()
}

/// The file's type by its extension, as the phone knows it (`MimeTypeMap`); `*/*` where it
/// knows none, so the system's chooser offers every app.
fn mime_type(env: &mut Env<'_>, path: &str) -> jni::errors::Result<String> {
    let Some(extension) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return Ok("*/*".into());
    };
    let map = env
        .call_static_method(
            jni_str!("android/webkit/MimeTypeMap"),
            jni_str!("getSingleton"),
            jni_sig!("()Landroid/webkit/MimeTypeMap;"),
            &[],
        )?
        .l()?;
    let extension = env.new_string(extension)?;
    let kind = env
        .call_method(
            &map,
            jni_str!("getMimeTypeFromExtension"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
            &[JValue::Object(&extension)],
        )?
        .l()?;
    Ok(jvm::string_of(env, kind)?.unwrap_or_else(|| "*/*".into()))
}
