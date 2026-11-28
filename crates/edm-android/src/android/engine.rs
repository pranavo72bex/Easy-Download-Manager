// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM going on with no window, when the person has asked for it (Options › Downloads, "Keep
//! downloads and torrents going when EDM is closed"; the maintainer, 2026-11-21).
//!
//! Closing EDM's activity (swiping it away from the recent apps) ends `android_main`: Android's
//! `onDestroy` waits for it (android-activity's `notify_destroyed`). The window's quit has by
//! then stopped every download and put it back in line. With the setting on and something left
//! to do, `android_main` starts EDM again on a thread of its own, as `--headless` runs on a
//! computer, and returns: the queue resumes, a torrent that finishes is shared, the foreground
//! service and its ongoing notification hold the process (its Stop All reaches this EDM through
//! the inbox), and once nothing has been downloading or sharing for a while the headless loop
//! ends (`headless::QUIT_WHEN_IDLE`) and the process is let go.
//!
//! Opened again while this runs, EDM stops it (everything saved, as Exit saves it) and starts in
//! a fresh process: GPUI's platform is made once a process (gpui-android's `init_platform`), so
//! the window cannot be made twice in one.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;

use jni::{jni_sig, jni_str, JValue};

use super::jvm;

/// Ends this process at once, as Android's own `Process.killProcess` would. Never
/// `std::process::exit` here: that runs the C exit handlers, which free the Java runtime's
/// statics while its own threads (the JIT's, SharedPreferences') still use them, and the
/// process then dies of a SIGSEGV (seen in 2 of 4 reopens from the notification on a phone,
/// 2026-11-22, gaps row N19). Everything EDM keeps has been saved before this is called.
pub(crate) fn end_process(code: i32) -> ! {
    // SAFETY: `_exit` takes any status and does not return; it runs no handlers, so nothing
    // of this process is used after it.
    unsafe { libc::_exit(code) }
}

/// The headless EDM's thread, while one runs in this process.
static ENGINE: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Set once EDM's window has been made in this process: a window made again needs a new one.
static WINDOW_MADE: AtomicBool = AtomicBool::new(false);

/// Whether a window was made in this process before: GPUI's platform cannot be made twice.
pub(crate) fn window_made_before() -> bool {
    WINDOW_MADE.swap(true, Ordering::SeqCst)
}

/// Whether EDM is to go on with no window: the person's setting, and something left to do.
pub(crate) fn wanted(home: &Path) -> bool {
    let (settings, _) = edm_shell::settings::Settings::load();
    settings.downloads.keep_after_close
        && edm_core::saved_rows(&home.join(edm_core::paths::QUEUE))
            .iter()
            .any(|row| crate::wakeup::goes_on(&row.state))
}

/// Starts EDM with no window on a thread of its own; `android_main` then returns. When it ends
/// by itself (nothing left to do), the process ends with it.
pub(crate) fn start() {
    edm_desktop_base::awake::retire_phone_watch();
    edm_desktop_base::headless::QUIT_WHEN_IDLE.store(true, Ordering::SeqCst);
    edm_desktop_base::signals::SIGNALLED.store(false, Ordering::SeqCst);
    let spawned = std::thread::Builder::new()
        .name("edm-engine".into())
        .spawn(|| {
            edm_core::note!(
                info,
                "EDM goes on with no window until its downloads are done"
            );
            edm_gpui::run_app(&["--headless".to_string()], || {
                unreachable!("EDM with no window makes no application")
            });
            // Ended by itself: let the process go. Stopped by a new window (`stop`), that
            // window's start ends the process itself, once this thread has ended.
            if !STOPPING.load(Ordering::SeqCst) {
                edm_core::note!(info, "EDM's downloads are done: its windowless run ends");
                end_process(0);
            }
        });
    match spawned {
        Ok(thread) => *edm_core::lock(&ENGINE) = Some(thread),
        Err(e) => {
            edm_core::note!(warn, "EDM could not go on with no window: {e}");
            end_process(0);
        }
    }
}

/// Set while a new window stops the windowless EDM.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// Stops the windowless EDM, if one runs: everything saved, as Exit saves it. Waits for it.
pub(crate) fn stop() {
    let Some(thread) = edm_core::lock(&ENGINE).take() else {
        return;
    };
    edm_core::note!(
        info,
        "EDM opened again: its windowless run stops, and EDM starts afresh"
    );
    STOPPING.store(true, Ordering::SeqCst);
    // The headless loop looks at this once a second and ends, saving the queue.
    edm_desktop_base::signals::SIGNALLED.store(true, Ordering::SeqCst);
    let _ = thread.join();
}

/// Starts EDM's activity again, in a task of its own, for a process that cannot make its window
/// a second time: this process then ends, and Android starts the activity in a fresh one.
pub(crate) fn relaunch() -> Result<(), String> {
    jvm::with("start EDM again", |env, context| {
        let manager = env
            .call_method(
                context,
                jni_str!("getPackageManager"),
                jni_sig!("()Landroid/content/pm/PackageManager;"),
                &[],
            )?
            .l()?;
        let package = env
            .call_method(
                context,
                jni_str!("getPackageName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let intent = env
            .call_method(
                &manager,
                jni_str!("getLaunchIntentForPackage"),
                jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
                &[JValue::Object(&package)],
            )?
            .l()?;
        if intent.is_null() {
            return Ok(());
        }
        // A new task, the old one cleared: EDM opens as from the launcher.
        jvm::add_flags(
            env,
            &intent,
            jvm::FLAG_ACTIVITY_NEW_TASK | FLAG_ACTIVITY_CLEAR_TASK,
        )?;
        env.call_method(
            context,
            jni_str!("startActivity"),
            jni_sig!("(Landroid/content/Intent;)V"),
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    })
}

/// Whether EDM's activity is ending only to be made again at once, as Android remakes it: before
/// Android 16 every activity is remade when a system overlay changes (a new wallpaper's colours,
/// the navigation mode), whatever the manifest's `configChanges` says. Its window then cannot be
/// made a second time in this process, so EDM starts again in a fresh one (`relaunch`) rather than
/// being gone (seen on Android 15's emulator, 2026-11-25: EDM ended 2 s after it started).
pub(crate) fn made_again() -> bool {
    jvm::with("ask whether its window is made again", |env, activity| {
        env.call_method(
            activity,
            jni_str!("isChangingConfigurations"),
            jni_sig!("()Z"),
            &[],
        )?
        .z()
    })
    .unwrap_or(false)
}

/// `Intent.FLAG_ACTIVITY_CLEAR_TASK`.
const FLAG_ACTIVITY_CLEAR_TASK: i32 = 0x0000_8000;
