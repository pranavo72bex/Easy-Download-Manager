// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A link handed to dm by another app (gaps row N5): the activity's intent, read once dm's
//! window is up (`android.rs`) and again for each new one (`DmActivity.onNewIntent`, which makes
//! it the activity's intent and calls in through `natives`),
//! decided by `shared.rs`, and handed to dm's window as a browser's link is
//! (`dm_desktop_base::phone::shared`): the window adds it as it adds a link dropped on it. A
//! `.torrent` is copied into dm's home first and handed as that file
//! (`dm_desktop_base::phone::shared_file`), which the window adds as `file://`.
//!
//! Each intent is taken once: after reading it dm puts an empty intent in its place
//! (`setIntent`), and one read at a time, so the first start's read and a new intent's never
//! hand the same link over twice.

use std::path::Path;
use std::sync::Mutex;

use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JValue};

use super::{jvm, notify};
use crate::shared::{self, Handed};

/// The biggest `.torrent` dm copies in: far above any real one, so a wrong file is not read
/// into memory whole.
const MOST: usize = 32 * 1024 * 1024;

/// Reads the activity's intent, if it hands dm anything, and hands it to dm's window;
/// `torrents` is the folder a shared `.torrent` is copied into. What cannot be taken is said
/// in a notification, as the window has nothing to show for it.
pub(crate) fn take(torrents: &Path) {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _reading = dm_core::lock(&ONE_AT_A_TIME);
    let taken = match read() {
        Ok(Handed::Nothing) => Ok(()),
        Ok(Handed::Text(text)) => {
            dm_desktop_base::phone::shared(text);
            Ok(())
        }
        Ok(Handed::File(uri)) => {
            copy_within(uri, torrents.to_path_buf()).map(dm_desktop_base::phone::shared_file)
        }
        Err(why) => Err(why),
    };
    if let Err(why) = taken {
        // Without the permission to notify there is no one else to tell.
        let _ = notify::post("dm could not take what was shared", &why, None);
    }
}

/// What the activity's intent hands over; the intent is then replaced by an empty one.
fn read() -> Result<Handed, String> {
    jvm::with("read what was shared", |env, activity| {
        let intent = env
            .call_method(
                activity,
                jni_str!("getIntent"),
                jni_sig!("()Landroid/content/Intent;"),
                &[],
            )?
            .l()?;
        if intent.is_null() {
            return Ok(Handed::Nothing);
        }
        let action = string_call(env, &intent, StringCall::Action)?;
        let text = if action.as_deref() == Some(shared::SEND) {
            let key = env.new_string("android.intent.extra.TEXT")?;
            let text = env
                .call_method(
                    &intent,
                    jni_str!("getStringExtra"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                    &[JValue::Object(&key)],
                )?
                .l()?;
            jvm::string_of(env, text)?
        } else {
            None
        };
        let data = string_call(env, &intent, StringCall::Data)?;
        let handed = shared::handed(action.as_deref(), text.as_deref(), data.as_deref());
        if handed != Handed::Nothing {
            let empty = env.new_object(jni_str!("android/content/Intent"), jni_sig!("()V"), &[])?;
            env.call_method(
                activity,
                jni_str!("setIntent"),
                jni_sig!("(Landroid/content/Intent;)V"),
                &[JValue::Object(&empty)],
            )?;
        }
        Ok(handed)
    })
}

/// The intent's string getters dm reads.
enum StringCall {
    Action,
    Data,
}

/// `intent.getAction()` or `intent.getDataString()`.
fn string_call(
    env: &mut Env<'_>,
    intent: &JObject<'_>,
    which: StringCall,
) -> jni::errors::Result<Option<String>> {
    let name = match which {
        StringCall::Action => jni_str!("getAction"),
        StringCall::Data => jni_str!("getDataString"),
    };
    let value = env
        .call_method(intent, name, jni_sig!("()Ljava/lang/String;"), &[])?
        .l()?;
    jvm::string_of(env, value)
}

/// How long a shared file's copy may take: a `.torrent` is kilobytes.
const COPY_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

/// [`copy`] on a thread of its own, waited for up to [`COPY_WITHIN`]: a sharing app whose
/// stream never ends held the share lock for good, and every later share waited behind it
/// (gaps row H37). Past the time the copy is left to its thread and the share is refused.
fn copy_within(uri: String, torrents: std::path::PathBuf) -> Result<std::path::PathBuf, String> {
    let (sent, got) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("dm-shared-copy".into())
        .spawn(move || {
            let _ = sent.send(copy(&uri, &torrents));
        })
        .map_err(|e| format!("dm could not read the file shared: {e}"))?;
    got.recv_timeout(COPY_WITHIN).unwrap_or_else(|_| {
        Err("the app that shared the file did not hand it over within 30 seconds".into())
    })
}

/// Copies the file at `uri` (a `.torrent` from Files, a browser or a mail) into `torrents`,
/// giving the copy's path, which dm adds as `file://`.
fn copy(uri: &str, torrents: &Path) -> Result<std::path::PathBuf, String> {
    // The name first, in a call of its own: a sender that does not answer the question still
    // lets the file be read.
    let display = jvm::with("read the file's name", |env, context| {
        display_name(env, context, uri)
    })
    .ok()
    .flatten();
    let last = jvm::with("read the file's name", |env, _| {
        let parsed = parse(env, uri)?;
        let last = env
            .call_method(
                &parsed,
                jni_str!("getLastPathSegment"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        jvm::string_of(env, last)
    })
    .ok()
    .flatten();
    let bytes = jvm::with("read the .torrent", |env, context| {
        read_all(env, context, uri)
    })?;
    if bytes.len() > MOST {
        return Err("the file shared is too big to be a .torrent".into());
    }
    shared::keep_torrent(
        torrents,
        &shared::torrent_name(display.as_deref(), last.as_deref()),
        &bytes,
    )
}

/// `Uri.parse(uri)`.
fn parse<'local>(env: &mut Env<'local>, uri: &str) -> jni::errors::Result<JObject<'local>> {
    let text = env.new_string(uri)?;
    env.call_static_method(
        jni_str!("android/net/Uri"),
        jni_str!("parse"),
        jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
        &[JValue::Object(&text)],
    )?
    .l()
}

/// The `ContentResolver` of `context`.
fn resolver<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
) -> jni::errors::Result<JObject<'local>> {
    env.call_method(
        context,
        jni_str!("getContentResolver"),
        jni_sig!("()Landroid/content/ContentResolver;"),
        &[],
    )?
    .l()
}

/// The name the sending app gives the file (`OpenableColumns.DISPLAY_NAME`), where it gives one.
fn display_name(
    env: &mut Env<'_>,
    context: &JObject<'_>,
    uri: &str,
) -> jni::errors::Result<Option<String>> {
    let parsed = parse(env, uri)?;
    let resolver = resolver(env, context)?;
    let column = env.new_string("_display_name")?;
    let projection = env.new_object_array(1, jni_str!("java/lang/String"), &column)?;
    let cursor = env
        .call_method(
            &resolver,
            jni_str!("query"),
            jni_sig!(
                "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;"
            ),
            &[
                JValue::Object(&parsed),
                JValue::Object(&projection),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
            ],
        )?
        .l()?;
    if cursor.is_null() {
        return Ok(None);
    }
    let name = if env
        .call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?
        .z()?
    {
        let name = env
            .call_method(
                &cursor,
                jni_str!("getString"),
                jni_sig!("(I)Ljava/lang/String;"),
                &[JValue::Int(0)],
            )?
            .l()?;
        jvm::string_of(env, name)?
    } else {
        None
    };
    env.call_method(&cursor, jni_str!("close"), jni_sig!("()V"), &[])?;
    Ok(name)
}

/// Every byte of the file at `uri`, through `ContentResolver.openInputStream`, up to just past
/// [`MOST`]. A read Android refuses halfway leaves the stream to the collector to close.
fn read_all(env: &mut Env<'_>, context: &JObject<'_>, uri: &str) -> jni::errors::Result<Vec<u8>> {
    const CHUNK: usize = 64 * 1024;
    let parsed = parse(env, uri)?;
    let resolver = resolver(env, context)?;
    let stream = env
        .call_method(
            &resolver,
            jni_str!("openInputStream"),
            jni_sig!("(Landroid/net/Uri;)Ljava/io/InputStream;"),
            &[JValue::Object(&parsed)],
        )?
        .l()?;
    if stream.is_null() {
        return Err(jni::errors::Error::NullPtr("the shared file's stream"));
    }
    let buffer = env.new_byte_array(CHUNK)?;
    let mut chunk = vec![0i8; CHUNK];
    let mut bytes = Vec::new();
    loop {
        let read = env
            .call_method(
                &stream,
                jni_str!("read"),
                jni_sig!("([B)I"),
                &[JValue::Object(&buffer)],
            )?
            .i()?;
        // -1 at the end; never more than the buffer holds.
        let Ok(read) = usize::try_from(read) else {
            break;
        };
        let read = read.min(CHUNK);
        buffer.get_region(env, 0, &mut chunk[..read])?;
        bytes.extend(chunk[..read].iter().map(|b| b.to_ne_bytes()[0]));
        if bytes.len() > MOST {
            break;
        }
    }
    env.call_method(&stream, jni_str!("close"), jni_sig!("()V"), &[])?;
    Ok(bytes)
}
