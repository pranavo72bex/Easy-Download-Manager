// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm on the phone: `android_main`, which the activity (`com.dm.android.DmActivity`, gpui-mobile's
//! `GpuiInputActivity`, a NativeActivity, in `java/`) starts through android-activity. It hands dm's window the phone's
//! services (`crate::phone`, over `java`), sets dm's folders, and runs dm-gpui's own window
//! (`dm_gpui::run_app`, gaps rows N3, N17) on GPUI's Android platform (gpui-android, N16).
//! What only a phone has is reached through JNI: the link another app hands over (`intent`),
//! notifications and the wake lock (`notify`), opening a file (`open`), the foreground service
//! (`service`), the notification's buttons, the schedule's alarm and the phone's start
//! (`receivers`), and the native methods of dm's Java classes (`natives`).

mod intent;
mod java;
mod jvm;
mod natives;
mod notify;
mod open;
mod receivers;
mod service;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use android_activity::AndroidApp;
use gpui_android::android::{jni as gpui_jni, AndroidPlatform, SharedPlatform};
use jni::{jni_sig, jni_str, JValue};

/// How often the activity's first coming to the front is looked for.
const LOOK: Duration = Duration::from_millis(500);

/// Where dm's activity hands over (android-activity finds it by this name), on its own thread
/// once the activity is made; its window comes later, on the activity's InitWindow, inside
/// GPUI's run. In order: the phone's services handed to dm (before anything of dm's runs), dm's
/// folders, then dm-gpui's start of dm (its engine, its settings, the queue) and its window in
/// GPUI's application on gpui-android, until the activity is destroyed. GPUI's quit then stops
/// the downloads, each saved to be resumed (dm-gpui's `on_app_quit`), and the process is left,
/// so the next start begins afresh (android-activity would otherwise run this again in the same
/// process, with gpui-android's platform already made).
#[no_mangle]
fn android_main(app: AndroidApp) {
    gpui_android::android::init_logger();
    gpui_jni::install_panic_hook();
    // Without the Java side dm still starts; what needs it is then refused in words.
    if let Err(why) = jvm::init(app.vm_as_ptr(), app.activity_as_ptr()) {
        dm_core::note!(error, "{why}");
    }
    dm_desktop_base::phone::set_phone(Box::new(crate::phone::Phone::new(java::Jni)));
    let home = match folders(&app) {
        Ok(home) => home,
        Err(why) => {
            dm_core::note!(error, "dm could not start: {why}");
            std::process::exit(1);
        }
    };
    // Each of these only makes the phone friendlier; dm runs without them.
    let _ = open::allow_file_uris();
    let _ = notify::channels();
    receivers::dm_started(home.clone());
    let platform = gpui_jni::init_platform(&app);
    watch(Arc::clone(platform), home.join("torrents"));
    dm_gpui::run_app(&[], || {
        gpui::Application::with_platform(SharedPlatform::new(Arc::clone(platform)).into_rc())
    });
    std::process::exit(0);
}

/// dm's folders, set before dm-gpui's start reads any of them: its home (the app's own files
/// folder, `files/dm`) as `DM_HOME`, the folder it stages files in (`TMPDIR`: an app may not
/// write to the system's `/data/local/tmp`), and the phone's folders for `dm_core::folders`
/// (gaps row N2), so the download folder of a first start is the phone's. dm's home comes back.
fn folders(app: &AndroidApp) -> Result<PathBuf, String> {
    let home = app
        .internal_data_path()
        .ok_or("Android gave dm no folder of its own")?
        .join("dm");
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("dm could not make its folder: {e}"))?;
    // Set here, before dm's threads exist, so nothing reads the environment while it changes.
    std::env::set_var("DM_HOME", &home);
    std::env::set_var("TMPDIR", tmp);
    let downloads = downloads(app, &home);
    dm_core::folders::set_phone_folders(home.clone(), downloads);
    Ok(home)
}

/// Waits, on a thread of its own, for the activity's first coming to the front (dm's window is
/// up): the permissions are asked for, and what the activity's intent hands over goes to dm's
/// window (`intent`), so a link shared to dm as it starts appears in the window as a browser's
/// does. A `.torrent` is copied into `torrents` first. A link shared later comes to the running
/// activity as a new intent (`DmActivity.onNewIntent`, `natives`), so the thread then ends.
fn watch(platform: Arc<AndroidPlatform>, torrents: PathBuf) {
    let watched = std::thread::Builder::new()
        .name("dm-intent".into())
        .spawn(move || {
            while !platform.is_active() {
                std::thread::sleep(LOOK);
            }
            let _ = notify::ask_permissions();
            intent::take(&torrents);
        });
    if let Err(e) = watched {
        dm_core::note!(warn, "dm could not watch for shared links: {e}");
    }
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
