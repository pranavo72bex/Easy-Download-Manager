// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm on the phone: `android_main`, which the activity (gpui-mobile's `GpuiInputActivity`, a
//! NativeActivity, in `java/`) starts through android-activity, bringing up GPUI on gpui-android
//! (gaps rows N3, N16), and what only a phone has, reached through JNI: the link another app hands over
//! (`intent`), notifications and the wake lock (`notify`, `busy`), opening a file (`open`), and
//! the engine's [`crate::AndroidSystem`] (`system`).

mod busy;
mod intent;
mod jvm;
mod notify;
mod open;
mod system;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use android_activity::AndroidApp;
use gpui_android::android::{jni as gpui_jni, SharedPlatform};
use jni::{jni_sig, jni_str, JValue};

use crate::Engine;

/// The engine, and the folder a shared `.torrent` is copied into, once started: what each
/// return to the window reads a new intent with.
static STARTED: OnceLock<(Arc<Engine>, PathBuf)> = OnceLock::new();

/// Where dm's activity hands over (android-activity finds it by this name): GPUI on the phone's
/// platform, dm started inside its launch and its window opened, until the activity is closed;
/// then the downloads are stopped, each saved to be resumed, and the process left, so the next
/// start begins afresh (android-activity would otherwise run this again in the same process,
/// with gpui-android's platform already made).
#[no_mangle]
fn android_main(app: AndroidApp) {
    gpui_android::android::init_logger();
    gpui_jni::install_panic_hook();
    // Without the Java side dm still starts; what needs it is then refused in words.
    let reached = jvm::init(app.vm_as_ptr(), app.activity_as_ptr());
    let platform = SharedPlatform::new(Arc::clone(gpui_jni::init_platform(&app))).into_rc();
    let launched = app.clone();
    gpui::Application::with_platform(platform).run(move |cx| {
        let opened = reached.and_then(|()| start(&launched)).and_then(|engine| {
            let resumed = Rc::new(|| {
                if let Some((engine, torrents)) = STARTED.get() {
                    intent::take(engine, torrents);
                }
            });
            crate::app::open(engine, Some(resumed), cx)
        });
        if let Err(why) = opened {
            // The window says it; there is nothing else to tell it with.
            let _ = crate::app::failed(&why, cx);
        }
    });
    if let Some((engine, _)) = STARTED.get() {
        engine.shut_down();
    }
    std::process::exit(0);
}

/// dm started in the app's own folders, the activity's intent read once, the ongoing
/// notification watched: the engine the window shows.
fn start(app: &AndroidApp) -> Result<Arc<Engine>, String> {
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
    Ok(engine)
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
