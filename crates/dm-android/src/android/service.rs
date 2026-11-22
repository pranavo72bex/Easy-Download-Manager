// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's foreground service (gaps row N6; the Java is `DmService`, which hands each event here
//! through `natives`). While it runs Android keeps dm's process, so downloads go on with dm in
//! the background or the screen off.
//!
//! dm-desktop-base's awake watch says what keeps dm busy and when nothing does
//! (`PhoneServices::busy`, `Phone::busy` in `crate::phone`); the ongoing notification it puts
//! up ([`show`], through `Java::ongoing`) starts the service, whose start (`started`) makes
//! that notification its own with `startForeground`, built by `notify::ongoing_builder` from
//! the words kept here. Later words change it in place (the same number). When dm is no longer
//! busy ([`hide`]) the service stops and the notification goes. A start Android refuses (Android 12 and up, from the background) is
//! said and leaves the plain ongoing notification and the wake lock to do what they can.
//!
//! The service is never stopped from outside before it has called `startForeground` (Android
//! ends an app whose service does not): a stop asked for meanwhile is carried out by its start.
//! On Android 15 a dataSync service has six hours a day: its `onTimeout` stops it cleanly, the
//! notification kept, and it is not started again until dm has been idle.

use std::sync::Mutex;
use std::time::Duration;

use jni::objects::JObject;
use jni::refs::Global;
use jni::{jni_sig, jni_str, Env, JValue};

use super::{jvm, notify};

/// `ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC`.
const TYPE_DATA_SYNC: i32 = 1;
/// `Service.STOP_FOREGROUND_REMOVE`: the notification goes with it.
const STOP_REMOVE: i32 = 1;
/// `Service.STOP_FOREGROUND_DETACH`: the notification stays, no longer the service's.
const STOP_DETACH: i32 = 2;
/// dm's service, by its class's name.
const SERVICE: &str = "com.dm.android.DmService";

/// The service as dm has it.
struct Held {
    /// Whether dm wants it running.
    wanted: bool,
    /// The ongoing notification's words.
    title: String,
    body: String,
    /// Asked to start, not yet started.
    asked: bool,
    /// Started: its global reference, for `stopSelf`.
    running: Option<Global<JObject<'static>>>,
    /// Android 15 stopped it for its time: not started again until dm has been idle.
    timed_out: bool,
    /// How many holds of the schedule's (`hold`) are under way.
    holds: u32,
    /// Whether dm itself is busy (`show`/`hide`), as against held for the schedule.
    busy: bool,
}

static HELD: Mutex<Held> = Mutex::new(Held {
    wanted: false,
    title: String::new(),
    body: String::new(),
    asked: false,
    running: None,
    timed_out: false,
    holds: 0,
    busy: false,
});

/// dm is busy: the ongoing notification with `title` and `body`, the service's own once it
/// runs.
pub(crate) fn show(title: &str, body: &str) -> Result<(), String> {
    dm_core::lock(&HELD).busy = true;
    want(title, body)
}

/// dm is no longer busy: the service stopped (where no hold of the schedule's keeps it) and
/// the notification taken down.
pub(crate) fn hide() -> Result<(), String> {
    let stopped = {
        let mut held = dm_core::lock(&HELD);
        held.busy = false;
        held.timed_out = false;
        if held.holds > 0 {
            return Ok(());
        }
        held.wanted = false;
        held.running.take()
    };
    if let Some(service) = stopped {
        let _ = stop(&service, STOP_REMOVE);
    }
    notify::end_ongoing()
}

/// The schedule's alarm: the service started with `title` and `body` for `wait`, so the
/// process is up for the schedule's start; it then goes, unless dm is busy by then.
pub(crate) fn hold(title: &str, body: &str, wait: Duration) -> Result<(), String> {
    {
        let mut held = dm_core::lock(&HELD);
        if held.busy {
            // Already up for the downloads, with their words.
            return Ok(());
        }
        held.holds += 1;
    }
    let shown = want(title, body);
    let ended = std::thread::Builder::new()
        .name("dm-schedule-hold".into())
        .spawn(move || {
            std::thread::sleep(wait);
            let idle = {
                let mut held = dm_core::lock(&HELD);
                held.holds = held.holds.saturating_sub(1);
                held.holds == 0 && !held.busy
            };
            if idle {
                let _ = hide();
            }
        });
    if ended.is_err() {
        // Without the thread nothing would let it go: let go now.
        dm_core::lock(&HELD).holds = 0;
    }
    shown
}

/// The service wanted, with these words: started, or its notification changed in place.
fn want(title: &str, body: &str) -> Result<(), String> {
    let start = {
        let mut held = dm_core::lock(&HELD);
        held.wanted = true;
        title.clone_into(&mut held.title);
        body.clone_into(&mut held.body);
        let start = !held.asked && held.running.is_none() && !held.timed_out;
        if start {
            held.asked = true;
        }
        start
    };
    if !start {
        // Running (its notification is the ongoing one, by number), starting (it reads the
        // words as it starts), or not to be started: the words shown in place.
        return notify::ongoing(title, body);
    }
    match start_service() {
        Ok(()) => Ok(()),
        Err(why) => {
            dm_core::lock(&HELD).asked = false;
            let _ = notify::ongoing(title, body);
            Err(why)
        }
    }
}

/// `context.startForegroundService(new Intent().setClassName(context, DmService))`.
fn start_service() -> Result<(), String> {
    jvm::with("keep downloading in the background", |env, context| {
        let intent = env.new_object(jni_str!("android/content/Intent"), jni_sig!("()V"), &[])?;
        let class = env.new_string(SERVICE)?;
        env.call_method(
            &intent,
            jni_str!("setClassName"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Landroid/content/Intent;"),
            &[JValue::Object(context), JValue::Object(&class)],
        )?;
        env.call_method(
            context,
            jni_str!("startForegroundService"),
            jni_sig!("(Landroid/content/Intent;)Landroid/content/ComponentName;"),
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    })
}

/// `DmService.onStartCommand`: the service made foreground with the ongoing notification,
/// always (Android ends an app whose service does not), then stopped again where dm no longer
/// wants it.
pub(crate) fn started(service: Global<JObject<'static>>) -> Result<(), String> {
    let (wanted, title, body) = {
        let mut held = dm_core::lock(&HELD);
        held.asked = false;
        (held.wanted, held.title.clone(), held.body.clone())
    };
    jvm::with("keep downloading in the background", |env, _| {
        let builder = notify::ongoing_builder(env, &service, &title, &body)?;
        let built = env
            .call_method(
                &builder,
                jni_str!("build"),
                jni_sig!("()Landroid/app/Notification;"),
                &[],
            )?
            .l()?;
        foreground(env, &service, &built)
    })?;
    if !wanted {
        return stop(&service, STOP_REMOVE);
    }
    let mut held = dm_core::lock(&HELD);
    if held.wanted {
        held.running = Some(service);
        return Ok(());
    }
    drop(held);
    stop(&service, STOP_REMOVE)
}

/// `service.startForeground(id, notification, dataSync)`.
fn foreground(
    env: &mut Env<'_>,
    service: &JObject<'_>,
    built: &JObject<'_>,
) -> jni::errors::Result<()> {
    env.call_method(
        service,
        jni_str!("startForeground"),
        jni_sig!("(ILandroid/app/Notification;I)V"),
        &[
            JValue::Int(notify::ONGOING_ID),
            JValue::Object(built),
            JValue::Int(TYPE_DATA_SYNC),
        ],
    )?;
    Ok(())
}

/// `DmService.onTimeout` (Android 15): stopped cleanly within the seconds Android gives, the
/// notification kept as a plain one while dm is still busy.
pub(crate) fn timed_out(service: Global<JObject<'static>>) -> Result<(), String> {
    {
        let mut held = dm_core::lock(&HELD);
        held.timed_out = true;
        held.running = None;
    }
    stop(&service, STOP_DETACH)
}

/// `DmService.onDestroy`: no longer running.
pub(crate) fn stopped() {
    let mut held = dm_core::lock(&HELD);
    held.running = None;
    held.asked = false;
}

/// `service.stopForeground(how)` and `service.stopSelf()`.
fn stop(service: &JObject<'static>, how: i32) -> Result<(), String> {
    jvm::with("stop its background work", |env, _| {
        env.call_method(
            service,
            jni_str!("stopForeground"),
            jni_sig!("(I)V"),
            &[JValue::Int(how)],
        )?;
        env.call_method(service, jni_str!("stopSelf"), jni_sig!("()V"), &[])?;
        Ok(())
    })
}
