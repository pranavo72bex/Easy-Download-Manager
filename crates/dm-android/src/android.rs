// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm on the phone: `android_main`, which the activity (`com.dm.android.DmActivity`, over
//! gpui-mobile's `GpuiInputActivity`, a NativeActivity, in `java/`) starts through
//! android-activity. It hands dm's window the phone's services (`crate::phone`, over `java`),
//! sets dm's folders, and runs dm-gpui's own window (`dm_gpui::run_app`, gaps rows N3, N17) on
//! GPUI's Android platform (gpui-android, N16). What only a phone has is reached through JNI:
//! the link another app hands over (`intent`), notifications and the wake lock (`notify`),
//! opening a file (`open`), where downloads land, the folders dm writes by path and where a
//! finished one is published (`storage`), the connection and the battery (`network`), dm's
//! secrets sealed by the Keystore (`keystore`), the foreground service (`service`), the
//! notification's buttons, the schedule's alarm and the phone's start (`receivers`), and the
//! native methods of dm's Java classes (`natives`).

mod engine;
mod intent;
mod java;
mod jvm;
mod keystore;
mod natives;
mod network;
mod notify;
mod open;
mod receivers;
mod service;
mod storage;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use android_activity::AndroidApp;
use gpui_android::android::{jni as gpui_jni, AndroidPlatform, SharedPlatform};

/// How often the activity's first coming to the front is looked for.
const LOOK: Duration = Duration::from_millis(500);

/// Where dm's activity hands over (android-activity finds it by this name), on its own thread
/// once the activity is made; its window comes later, on the activity's InitWindow, inside
/// GPUI's run. In order: dm's home and the phone's storage (where downloads land, what Browse…
/// offers and how a finished one is published, gaps row N9), the phone's services handed to dm
/// (before anything of dm's runs), dm's folders and its secrets' store, dm said to run in this
/// process (for the receivers and a new intent), then dm-gpui's start of dm (its engine, its
/// settings, the queue) and its window in GPUI's application on gpui-android, until the
/// activity is destroyed. GPUI's quit then stops the downloads, each saved to be resumed
/// (dm-gpui's `on_app_quit`), and the process is left, so the next start begins afresh
/// (android-activity would otherwise run this again in the same process, with gpui-android's
/// platform already made).
#[no_mangle]
fn android_main(app: AndroidApp) {
    gpui_android::android::init_logger();
    gpui_jni::install_panic_hook();
    // Without the Java side dm still starts; what needs it is then refused in words.
    if let Err(why) = jvm::init(app.vm_as_ptr(), app.activity_as_ptr()) {
        dm_core::note!(error, "{why}");
    }
    let home = match home(&app) {
        Ok(home) => home,
        Err(why) => {
            dm_core::note!(error, "dm could not start: {why}");
            engine::end_process(1);
        }
    };
    // The phone's storage, worked out once (gaps row N9): the first start's download folder,
    // the folders Browse… offers and Export writes to (`Places`), and the publisher the engine
    // asks as each download finishes (`PhoneServices::publish`).
    let storage = storage::Storage::new(&app, &home);
    let downloads = storage.download_folder(&home);
    let places = storage.places().clone();
    let phone = crate::phone::Phone::new(java::Jni::new(storage)).with_places(places);
    dm_desktop_base::phone::set_phone(Box::new(phone));
    folders(&home, downloads);
    // dm runs in this process now: a receiver's Retry or Stop All and a new intent find it.
    receivers::dm_started(home.clone());
    // Each of these only makes the phone friendlier; dm runs without them.
    let _ = open::allow_file_uris();
    let _ = notify::channels();
    // Opened again while dm goes on with no window in this process: that dm is stopped (all
    // saved), and dm starts in a fresh process, as GPUI's platform is made once a process.
    if engine::window_made_before() {
        engine::stop();
        if let Err(why) = engine::relaunch() {
            dm_core::note!(warn, "{why}");
        }
        engine::end_process(0);
    }
    let platform = gpui_jni::init_platform(&app);
    watch(Arc::clone(platform), home.join("torrents"));
    dm_gpui::run_app(&[], || {
        gpui::Application::with_platform(SharedPlatform::new(Arc::clone(platform)).into_rc())
    });
    // Closed with downloads left, where the person asked for them to go on (Options ›
    // Downloads): dm goes on with no window (`engine`), and this returns so Android lets the
    // activity go. Else the process ends, and the next start begins afresh.
    if engine::wanted(&home) {
        engine::start();
        return;
    }
    engine::end_process(0);
}

/// dm's home, the app's own files folder's `dm`, made with the folder it stages files in
/// (`tmp`: an app may not write to the system's `/data/local/tmp`).
fn home(app: &AndroidApp) -> Result<PathBuf, String> {
    let home = app
        .internal_data_path()
        .ok_or("Android gave dm no folder of its own")?
        .join("dm");
    std::fs::create_dir_all(home.join("tmp"))
        .map_err(|e| format!("dm could not make its folder: {e}"))?;
    Ok(home)
}

/// dm's folders, set before dm-gpui's start reads any of them: its `home` as `DM_HOME`, the
/// folder it stages files in as `TMPDIR`, dm's secrets sealed by the Android Keystore (gaps row
/// N11), and the phone's folders for `dm_core::folders` (gaps row N2), so the download folder
/// of a first start is `downloads`, the phone's.
fn folders(home: &Path, downloads: PathBuf) {
    // Set here, before dm's threads exist, so nothing reads the environment while it changes.
    std::env::set_var("DM_HOME", home);
    std::env::set_var("TMPDIR", home.join("tmp"));
    let _ = dm_core::secretstore::set_android_store(Box::new(keystore::store(home)));
    dm_core::folders::set_phone_folders(home.to_path_buf(), downloads);
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
