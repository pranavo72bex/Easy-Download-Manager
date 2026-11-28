// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM on the phone: `android_main`, which the activity (`com.edm.android.EdmActivity`, over
//! gpui-mobile's `GpuiInputActivity`, a NativeActivity, in `java/`) starts through
//! android-activity. It hands EDM's window the phone's services (`crate::phone`, over `java`),
//! sets EDM's folders, and runs edm-gpui's own window (`edm_gpui::run_app`, gaps rows N3, N17) on
//! GPUI's Android platform (gpui-android, N16). What only a phone has is reached through JNI:
//! the link another app hands over (`intent`), notifications and the wake lock (`notify`),
//! opening a file (`open`), where downloads land, the folders EDM writes by path and where a
//! finished one is published (`storage`), the connection and the battery (`network`), EDM's
//! secrets sealed by the Keystore (`keystore`), the foreground service (`service`), the
//! notification's buttons, the schedule's alarm and the phone's start (`receivers`), and the
//! native methods of EDM's Java classes (`natives`).

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

/// Where EDM's activity hands over (android-activity finds it by this name), on its own thread
/// once the activity is made; its window comes later, on the activity's InitWindow, inside
/// GPUI's run. In order: EDM's home and the phone's storage (where downloads land, what Browse…
/// offers and how a finished one is published, gaps row N9), the phone's services handed to EDM
/// (before anything of EDM's runs), EDM's folders and its secrets' store, EDM said to run in this
/// process (for the receivers and a new intent), then edm-gpui's start of EDM (its engine, its
/// settings, the queue) and its window in GPUI's application on gpui-android, until the
/// activity is destroyed. GPUI's quit then stops the downloads, each saved to be resumed
/// (edm-gpui's `on_app_quit`), and the process is left, so the next start begins afresh
/// (android-activity would otherwise run this again in the same process, with gpui-android's
/// platform already made).
#[no_mangle]
fn android_main(app: AndroidApp) {
    gpui_android::android::init_logger();
    gpui_jni::install_panic_hook();
    // Without the Java side EDM still starts; what needs it is then refused in words.
    if let Err(why) = jvm::init(app.vm_as_ptr(), app.activity_as_ptr()) {
        edm_core::note!(error, "{why}");
    }
    let home = match home(&app) {
        Ok(home) => home,
        Err(why) => {
            edm_core::note!(error, "EDM could not start: {why}");
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
    edm_desktop_base::phone::set_phone(Box::new(phone));
    folders(&home, downloads);
    // EDM runs in this process now: a receiver's Retry or Stop All and a new intent find it.
    receivers::edm_started(home.clone());
    // Each of these only makes the phone friendlier; EDM runs without them.
    let _ = open::allow_file_uris();
    let _ = notify::channels();
    // Opened again while EDM goes on with no window in this process: that EDM is stopped (all
    // saved), and EDM starts in a fresh process, as GPUI's platform is made once a process.
    if engine::window_made_before() {
        engine::stop();
        if let Err(why) = engine::relaunch() {
            edm_core::note!(warn, "{why}");
        }
        engine::end_process(0);
    }
    let platform = gpui_jni::init_platform(&app);
    watch(Arc::clone(platform), home.join("torrents"));
    edm_gpui::run_app(&[], || {
        gpui::Application::with_platform(SharedPlatform::new(Arc::clone(platform)).into_rc())
    });
    // Ended only to be made again (an overlay changed): EDM comes back in a fresh process, its
    // downloads saved by the quit and resumed there.
    if engine::made_again() {
        edm_core::note!(info, "Android remade EDM's window: EDM starts afresh");
        if let Err(why) = engine::relaunch() {
            edm_core::note!(warn, "{why}");
        }
        engine::end_process(0);
    }
    // Closed with downloads left, where the person asked for them to go on (Options ›
    // Downloads): EDM goes on with no window (`engine`), and this returns so Android lets the
    // activity go. Else the process ends, and the next start begins afresh.
    if engine::wanted(&home) {
        engine::start();
        return;
    }
    engine::end_process(0);
}

/// EDM's home, the app's own files folder's `EDM`, made with the folder it stages files in
/// (`tmp`: an app may not write to the system's `/data/local/tmp`).
fn home(app: &AndroidApp) -> Result<PathBuf, String> {
    let home = app
        .internal_data_path()
        .ok_or("Android gave EDM no folder of its own")?
        .join("EDM");
    std::fs::create_dir_all(home.join("tmp"))
        .map_err(|e| format!("EDM could not make its folder: {e}"))?;
    Ok(home)
}

/// EDM's folders, set before edm-gpui's start reads any of them: its `home` as `EDM_HOME`, the
/// folder it stages files in as `TMPDIR`, EDM's secrets sealed by the Android Keystore (gaps row
/// N11), and the phone's folders for `edm_core::folders` (gaps row N2), so the download folder
/// of a first start is `downloads`, the phone's.
fn folders(home: &Path, downloads: PathBuf) {
    // Set here, before EDM's threads exist, so nothing reads the environment while it changes.
    std::env::set_var("EDM_HOME", home);
    std::env::set_var("TMPDIR", home.join("tmp"));
    let _ = edm_core::secretstore::set_android_store(Box::new(keystore::store(home)));
    edm_core::folders::set_phone_folders(home.to_path_buf(), downloads);
}

/// On a thread of its own: what the activity's intent hands over goes to EDM's window at once
/// (`intent`; the window takes it from `drops` when it is up), so a link shared to EDM as it
/// starts appears in the window as a browser's does, whether or not the activity has the focus
/// yet (it waited for the focus, and a start behind the lock screen never read its link, CI
/// 2026-11-22). A `.torrent` is copied into `torrents` first. Then, once the activity first
/// comes to the front, the permissions are asked for. A link shared later comes to the running
/// activity as a new intent (`EdmActivity.onNewIntent`, `natives`), so the thread then ends.
fn watch(platform: Arc<AndroidPlatform>, torrents: PathBuf) {
    let watched = std::thread::Builder::new()
        .name("edm-intent".into())
        .spawn(move || {
            intent::take(&torrents);
            while !platform.is_active() {
                std::thread::sleep(LOOK);
            }
            let _ = notify::ask_permissions();
        });
    if let Err(e) = watched {
        edm_core::note!(warn, "EDM could not watch for shared links: {e}");
    }
}
