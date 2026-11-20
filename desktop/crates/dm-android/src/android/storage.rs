// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where downloads land on the phone and where a finished one goes (gaps row N9; the decisions
//! in `crate::storage`, tested on the host): on Android 10 and up dm downloads into its own
//! folder on the shared storage and hands each finished file to the media store's Downloads
//! (`Download/dm`), through the publisher the engine asks as a run finishes
//! (`dm_core::folders::set_publisher`); on 8 and 9 the shared folder by path, as before.
//!
//! The media store's way: the file inserted with `IS_PENDING` 1 (no other app sees it yet),
//! written through a file descriptor the store lends (the copy is Rust's, in chunks, on the
//! engine's blocking thread, never the window's), then `IS_PENDING` 0. Its row in the store
//! names its path (`_data`), which the engine's row then holds: opening it finds its
//! `content://` address by that path (`open.rs`), as before.

use std::fs::File;
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use android_activity::AndroidApp;
use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JValue};

use super::jvm;
use crate::storage::{self, Shared, Way};

/// The phone's storage, worked out once at start: dm's private folders and the Android version,
/// which decide where new downloads go and how a finished file is published. The phone's
/// `PhoneServices` holds one and answers `publish` with [`Storage::publish`].
pub(crate) struct Storage {
    /// The folders only dm sees, most particular first.
    private: Vec<PathBuf>,
    /// dm's own download folder on the shared storage, where it could be made.
    own: Option<PathBuf>,
    /// The phone's API level (0 where it could not be read: then by path, as on 8 and 9).
    sdk: i32,
}

impl Storage {
    /// The storage of the app `app`, dm's home being `home`.
    pub(crate) fn new(app: &AndroidApp, home: &Path) -> Self {
        let external = app.external_data_path();
        let private = storage::private_folders(external.as_deref(), home);
        let sdk = jvm::with("read the Android version", |env, _| jvm::sdk(env)).unwrap_or(0);
        let own = external
            .map(|external| storage::own_downloads(&external))
            .filter(|own| std::fs::create_dir_all(own).is_ok());
        Storage { private, own, sdk }
    }

    /// Where new downloads go on the first start: on Android 10 and up dm's own folder on the
    /// shared storage, else `found` (the shared `Download/dm` where dm may write it).
    pub(crate) fn download_folder(&self, found: PathBuf) -> PathBuf {
        storage::download_folder(self.sdk, self.own.clone(), found)
    }

    /// The finished `file` put in the shared `Download/dm`, if it is in one of dm's private
    /// folders: where it now is, `None` where it is left, or why it could not be.
    pub(crate) fn publish(&self, file: &Path) -> Result<Option<PathBuf>, String> {
        publish(file, &self.private, self.sdk)
    }
}

/// [`Storage`] made and its publisher handed to the engine directly, for an `android_main`
/// that hands in no `PhoneServices`; where new downloads go on the first start.
pub(crate) fn start(app: &AndroidApp, home: &Path, found: PathBuf) -> PathBuf {
    let storage = Storage::new(app, home);
    let folder = storage.download_folder(found);
    dm_core::folders::set_publisher(Arc::new(move |file: &Path| storage.publish(file)));
    folder
}

/// The finished `file` put in the shared `Download/dm`, if it is in one of dm's `private`
/// folders: where it now is, `None` where it is left, or why it could not be.
fn publish(file: &Path, private: &[PathBuf], sdk: i32) -> Result<Option<PathBuf>, String> {
    let Some(shared) = storage::shared_place(file, private) else {
        return Ok(None);
    };
    match storage::way(sdk) {
        Way::MediaStore => media_store(file, &shared).map(Some),
        Way::ByPath => {
            let public = public_downloads()?;
            storage::move_into(file, &shared.folder_in(&public), &shared.name)
                .map(Some)
                .map_err(|e| e.to_string())
        }
    }
}

/// The shared Download folder's path (`Environment.getExternalStoragePublicDirectory`).
fn public_downloads() -> Result<PathBuf, String> {
    jvm::with("find the Download folder", |env, _| {
        public_downloads_in(env)
    })?
    .ok_or_else(|| "the phone has no shared Download folder".to_owned())
}

/// The same, inside a call already attached.
fn public_downloads_in(env: &mut Env<'_>) -> jni::errors::Result<Option<PathBuf>> {
    let name = env
        .get_static_field(
            jni_str!("android/os/Environment"),
            jni_str!("DIRECTORY_DOWNLOADS"),
            jni_sig!(java.lang.String),
        )?
        .l()?;
    let folder = env
        .call_static_method(
            jni_str!("android/os/Environment"),
            jni_str!("getExternalStoragePublicDirectory"),
            jni_sig!("(Ljava/lang/String;)Ljava/io/File;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    if folder.is_null() {
        return Ok(None);
    }
    let path = env
        .call_method(
            &folder,
            jni_str!("getAbsolutePath"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    Ok(jvm::string_of(env, path)?.map(PathBuf::from))
}

/// The file through `MediaStore.Downloads`: inserted pending, its bytes copied in through the
/// descriptor lent, then shown; dm's copy removed. Where the store put it. A copy that failed
/// takes its half-made entry out again, and dm's copy is kept.
fn media_store(file: &Path, shared: &Shared) -> Result<PathBuf, String> {
    let (uri, fd) = jvm::with("put the file in Download", |env, context| {
        let resolver = resolver(env, context)?;
        let values = values(env)?;
        put_text(env, &values, "_display_name", &shared.name)?;
        put_text(env, &values, "relative_path", &shared.relative_path())?;
        if let Some(kind) = mime_type(env, &shared.name)? {
            put_text(env, &values, "mime_type", &kind)?;
        }
        put_int(env, &values, "is_pending", 1)?;
        let collection = env
            .get_static_field(
                jni_str!("android/provider/MediaStore$Downloads"),
                jni_str!("EXTERNAL_CONTENT_URI"),
                jni_sig!(android.net.Uri),
            )?
            .l()?;
        let uri = env
            .call_method(
                &resolver,
                jni_str!("insert"),
                jni_sig!("(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;"),
                &[JValue::Object(&collection), JValue::Object(&values)],
            )?
            .l()?;
        if uri.is_null() {
            return Err(jni::errors::Error::NullPtr(
                "the Download folder's new entry",
            ));
        }
        let text = env
            .call_method(
                &uri,
                jni_str!("toString"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let text = jvm::string_of(env, text)?
            .ok_or(jni::errors::Error::NullPtr("the new entry's address"))?;
        let fd = match descriptor(env, &resolver, &uri) {
            Ok(fd) => fd,
            Err(e) => {
                let _ = delete(env, &resolver, &uri);
                return Err(e);
            }
        };
        Ok((text, fd))
    })?;
    // SAFETY: `fd` is the descriptor `ParcelFileDescriptor.detachFd` gave up, open and owned by
    // nothing else from here on: the `File` closes it once.
    let mut out = unsafe { File::from_raw_fd(fd) };
    let copied = storage::stream(file, &mut out);
    drop(out);
    if let Err(e) = copied {
        let _ = jvm::with("take the half-made file out of Download", |env, context| {
            let resolver = resolver(env, context)?;
            let uri = parse(env, &uri)?;
            delete(env, &resolver, &uri)
        });
        return Err(e.to_string());
    }
    let path = jvm::with("show the file in Download", |env, context| {
        let resolver = resolver(env, context)?;
        let parsed = parse(env, &uri)?;
        let values = values(env)?;
        put_int(env, &values, "is_pending", 0)?;
        env.call_method(
            &resolver,
            jni_str!("update"),
            jni_sig!(
                "(Landroid/net/Uri;Landroid/content/ContentValues;Ljava/lang/String;[Ljava/lang/String;)I"
            ),
            &[
                JValue::Object(&parsed),
                JValue::Object(&values),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
            ],
        )?;
        where_put(env, &resolver, &parsed, shared)
    })?;
    // Published whole: dm's copy is a spare one now, and one it could not remove is left.
    let _ = std::fs::remove_file(file);
    Ok(path)
}

/// Where the store put the entry at `uri`: its path (`_data`), else the shared Download
/// folder's `dm` folder and the name it was given (the store numbers a name already taken).
fn where_put(
    env: &mut Env<'_>,
    resolver: &JObject<'_>,
    uri: &JObject<'_>,
    shared: &Shared,
) -> jni::errors::Result<PathBuf> {
    let data = env.new_string("_data")?;
    let projection = env.new_object_array(2, jni_str!("java/lang/String"), &data)?;
    let name = env.new_string("_display_name")?;
    projection.set_element(env, 1, name)?;
    let cursor = env
        .call_method(
            resolver,
            jni_str!("query"),
            jni_sig!(
                "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;"
            ),
            &[
                JValue::Object(uri),
                JValue::Object(&projection),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
            ],
        )?
        .l()?;
    let (mut path, mut given) = (None, None);
    if !cursor.is_null() {
        if env
            .call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?
            .z()?
        {
            for (column, found) in [(0, &mut path), (1, &mut given)] {
                let text = env
                    .call_method(
                        &cursor,
                        jni_str!("getString"),
                        jni_sig!("(I)Ljava/lang/String;"),
                        &[JValue::Int(column)],
                    )?
                    .l()?;
                *found = jvm::string_of(env, text)?;
            }
        }
        env.call_method(&cursor, jni_str!("close"), jni_sig!("()V"), &[])?;
    }
    if let Some(path) = path.filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let public = public_downloads_in(env)?.unwrap_or_else(|| PathBuf::from("/sdcard/Download"));
    let name = given.unwrap_or_else(|| shared.name.clone());
    Ok(shared.folder_in(&public).join(name))
}

/// The descriptor of the entry at `uri`, opened for writing and handed over
/// (`openFileDescriptor(uri, "w").detachFd()`).
fn descriptor(
    env: &mut Env<'_>,
    resolver: &JObject<'_>,
    uri: &JObject<'_>,
) -> jni::errors::Result<i32> {
    let mode = env.new_string("w")?;
    let parcel = env
        .call_method(
            resolver,
            jni_str!("openFileDescriptor"),
            jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;"),
            &[JValue::Object(uri), JValue::Object(&mode)],
        )?
        .l()?;
    if parcel.is_null() {
        return Err(jni::errors::Error::NullPtr("the new entry's descriptor"));
    }
    let fd = env
        .call_method(&parcel, jni_str!("detachFd"), jni_sig!("()I"), &[])?
        .i()?;
    if fd < 0 {
        return Err(jni::errors::Error::NullPtr("the new entry's descriptor"));
    }
    Ok(fd)
}

/// Takes the entry at `uri` out of the store.
fn delete(env: &mut Env<'_>, resolver: &JObject<'_>, uri: &JObject<'_>) -> jni::errors::Result<()> {
    env.call_method(
        resolver,
        jni_str!("delete"),
        jni_sig!("(Landroid/net/Uri;Ljava/lang/String;[Ljava/lang/String;)I"),
        &[
            JValue::Object(uri),
            JValue::Object(&JObject::null()),
            JValue::Object(&JObject::null()),
        ],
    )?;
    Ok(())
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

/// `Uri.parse(text)`.
fn parse<'local>(env: &mut Env<'local>, text: &str) -> jni::errors::Result<JObject<'local>> {
    let text = env.new_string(text)?;
    env.call_static_method(
        jni_str!("android/net/Uri"),
        jni_str!("parse"),
        jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
        &[JValue::Object(&text)],
    )?
    .l()
}

/// A new, empty `ContentValues`.
fn values<'local>(env: &mut Env<'local>) -> jni::errors::Result<JObject<'local>> {
    env.new_object(
        jni_str!("android/content/ContentValues"),
        jni_sig!("()V"),
        &[],
    )
}

/// `values.put(key, text)`.
fn put_text(
    env: &mut Env<'_>,
    values: &JObject<'_>,
    key: &str,
    text: &str,
) -> jni::errors::Result<()> {
    let key = env.new_string(key)?;
    let text = env.new_string(text)?;
    env.call_method(
        values,
        jni_str!("put"),
        jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
        &[JValue::Object(&key), JValue::Object(&text)],
    )?;
    Ok(())
}

/// `values.put(key, Integer.valueOf(n))`.
fn put_int(env: &mut Env<'_>, values: &JObject<'_>, key: &str, n: i32) -> jni::errors::Result<()> {
    let key = env.new_string(key)?;
    let boxed = env
        .call_static_method(
            jni_str!("java/lang/Integer"),
            jni_str!("valueOf"),
            jni_sig!("(I)Ljava/lang/Integer;"),
            &[JValue::Int(n)],
        )?
        .l()?;
    env.call_method(
        values,
        jni_str!("put"),
        jni_sig!("(Ljava/lang/String;Ljava/lang/Integer;)V"),
        &[JValue::Object(&key), JValue::Object(&boxed)],
    )?;
    Ok(())
}

/// The file's type by its extension (`MimeTypeMap`), where the phone knows it; without one the
/// store works it out from the name.
fn mime_type(env: &mut Env<'_>, name: &str) -> jni::errors::Result<Option<String>> {
    let Some(extension) = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return Ok(None);
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
    jvm::string_of(env, kind)
}
