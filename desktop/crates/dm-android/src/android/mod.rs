// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm on the phone: `android_main`, which Android's NativeActivity starts (through
//! android-activity, as Slint's Android backend wants), and what only a phone has, reached
//! through JNI: the link another app hands over (`intent`), notifications and the wake lock
//! (`notify`, `busy`), opening a file (`open`), and the engine's [`crate::AndroidSystem`]
//! (`system`).

mod busy;
mod intent;
mod jvm;
mod notify;
mod open;
mod system;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use jni::{jni_sig, jni_str, JValue};
use slint::android::android_activity::{MainEvent, PollEvent};
use slint::android::AndroidApp;

use crate::Engine;

/// The engine, and the folder a shared `.torrent` is copied into, once started: what the
/// window's resume reads a new intent with.
static STARTED: OnceLock<(Arc<Engine>, PathBuf)> = OnceLock::new();

/// Where Android's NativeActivity hands over (android-activity finds it by this name): starts
/// dm, runs its window until the activity is closed, then stops the downloads, saving each to
/// be resumed, and leaves the process, so the next start begins afresh (android-activity would
/// otherwise run this again in the same process, with Slint's platform already set).
#[no_mangle]
fn android_main(app: AndroidApp) {
    if let Err(why) = start(&app) {
        // The window says it where Slint started; there is nothing else to tell it with.
        let _ = crate::app::failed(&why);
    }
    std::process::exit(0);
}

fn start(app: &AndroidApp) -> Result<(), String> {
    jvm::init(app.vm_as_ptr(), app.activity_as_ptr())?;
    slint::android::init_with_event_listener(app.clone(), |event| {
        // Each return to dm reads the activity's intent again (`intent.rs` says what reaches it).
        if let PollEvent::Main(MainEvent::Resume { .. }) = event {
            if let Some((engine, torrents)) = STARTED.get() {
                intent::take(engine, torrents);
            }
        }
    })
    .map_err(|e| format!("dm could not open its window: {e}"))?;
    // Each of these only makes the phone friendlier; dm runs without them.
    let _ = open::allow_file_uris();
    let _ = notify::channels();
    let _ = notify::ask_permissions();
    let home = app
        .internal_data_path()
        .ok_or("Android gave dm no folder of its own")?
        .join("dm");
    let downloads = downloads(app, &home);
    let engine = Engine::start(
        home.to_string_lossy().into_owned(),
        downloads.to_string_lossy().into_owned(),
        Arc::new(system::Droid),
    )
    .map_err(|e| e.to_string())?;
    let torrents = home.join("torrents");
    intent::take(&engine, &torrents);
    let _ = STARTED.set((Arc::clone(&engine), torrents));
    let _ = busy::watch(&engine);
    let ran = crate::app::run(Arc::clone(&engine));
    engine.shut_down();
    ran
}

/// Where downloads go on the first start: `Download/dm` in the phone's shared storage, which
/// the Files app shows; else the app's own folder on it (`Android/data/com.dm.android/files`,
/// on Android 10, which lets no app write to the shared folders by path, or on 8 and 9 before
/// the storage permission is granted); else one in dm's home. Options changes it later (N9).
fn downloads(app: &AndroidApp, home: &Path) -> PathBuf {
    let public = jvm::with("find the Download folder", |env, _| {
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
        jvm::string_of(env, path)
    });
    if let Ok(Some(public)) = public {
        let folder = PathBuf::from(public).join("dm");
        if writable(&folder) {
            return folder;
        }
    }
    if let Some(own) = app.external_data_path() {
        let folder = own.join("Download");
        if writable(&folder) {
            return folder;
        }
    }
    home.join("Download")
}

/// Whether dm can make `folder` and write a file in it.
fn writable(folder: &Path) -> bool {
    if std::fs::create_dir_all(folder).is_err() {
        return false;
    }
    let probe = folder.join(".dm-probe");
    let written = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    written
}
