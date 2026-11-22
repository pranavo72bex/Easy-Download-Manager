// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's receivers hand to Rust (gaps rows N6, N12; the Java is `DmActionReceiver` and
//! `DmBootReceiver`, each a native call and nothing more, `natives`).
//!
//! A notification's button runs what the window runs, through the path every other thread
//! and process asks the running dm by: its inbox (`dm_shell::shell`), which the engine's own
//! watcher takes up whether or not the window is drawn. Retry is the desktop notification's
//! Retry (`ask_running_to_retry`: the row tried again, as `Command::DownloadNow` does a failed
//! row); Stop All leaves `stop-all` (`ask_running_to_stop_all`: every download stopped, as
//! `Command::StopAll` does). A dm that is not running takes them at its next start. Open brings
//! dm to the front. The schedule's alarm holds dm up for its start (the foreground service),
//! where dm runs; else says the schedule is due. The phone's start sets the alarm again and
//! says what dm has to go on with (`crate::wakeup`).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JValue};

use super::{jvm, notify, service};
use crate::wakeup::{self, Tell};

/// A failure's Retry: the row in [`ROW`].
pub(crate) const RETRY: &str = "com.dm.android.RETRY";
/// The ongoing notification's Stop All.
pub(crate) const STOP_ALL: &str = "com.dm.android.STOP_ALL";
/// Brings dm to the front.
pub(crate) const OPEN: &str = "com.dm.android.OPEN";
/// The schedule's alarm.
pub(crate) const SCHEDULE: &str = "com.dm.android.SCHEDULE";
/// The extra that names the row (a `long`).
const ROW: &str = "com.dm.android.ROW";
/// The extra that names the notification the button is on (an `int`).
const NOTE: &str = "com.dm.android.NOTE";
/// dm's receiver of its own broadcasts (not exported).
const RECEIVER: &str = "com.dm.android.DmActionReceiver";

/// The alarm's request number: one alarm, set again in its place.
const ALARM_REQUEST: i32 = 7;
/// `AlarmManager.RTC_WAKEUP`.
const RTC_WAKEUP: i32 = 0;
/// How long the schedule's alarm holds dm up for its start before letting go, where nothing
/// started.
const SCHEDULE_HOLD: Duration = Duration::from_secs(3 * 60);

/// dm's home, once `android_main` has started dm in this process.
static STARTED: OnceLock<PathBuf> = OnceLock::new();

/// Said by `android_main` once dm's folders are set: dm runs in this process, at `home`.
pub(crate) fn dm_started(home: PathBuf) {
    let _ = STARTED.set(home);
}

/// dm's home where dm runs in this process.
pub(crate) fn started() -> Option<&'static PathBuf> {
    STARTED.get()
}

/// dm's home: where dm runs, its own; else the app's files folder's `dm`, as `android_main`
/// makes it.
fn home() -> Result<PathBuf, String> {
    if let Some(home) = started() {
        return Ok(home.clone());
    }
    let files = jvm::with("find its folder", |env, context| {
        let folder = env
            .call_method(
                context,
                jni_str!("getFilesDir"),
                jni_sig!("()Ljava/io/File;"),
                &[],
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
    })?;
    files
        .map(|files| PathBuf::from(files).join("dm"))
        .ok_or_else(|| "Android gave dm no folder of its own".into())
}

/// A broadcast of `action` to dm's receiver, as a `PendingIntent` a notification's button or
/// an alarm sends: `row` and `note` (the notification's number, which also tells one button's
/// intent from another's) go with it.
pub(crate) fn broadcast<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
    action: &str,
    row: i64,
    note: i32,
) -> jni::errors::Result<JObject<'local>> {
    let intent = jvm::intent(env, action)?;
    let class = env.new_string(RECEIVER)?;
    env.call_method(
        &intent,
        jni_str!("setClassName"),
        jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Landroid/content/Intent;"),
        &[JValue::Object(context), JValue::Object(&class)],
    )?;
    let key = env.new_string(ROW)?;
    env.call_method(
        &intent,
        jni_str!("putExtra"),
        jni_sig!("(Ljava/lang/String;J)Landroid/content/Intent;"),
        &[JValue::Object(&key), JValue::Long(row)],
    )?;
    let key = env.new_string(NOTE)?;
    env.call_method(
        &intent,
        jni_str!("putExtra"),
        jni_sig!("(Ljava/lang/String;I)Landroid/content/Intent;"),
        &[JValue::Object(&key), JValue::Int(note)],
    )?;
    env.call_static_method(
        jni_str!("android/app/PendingIntent"),
        jni_str!("getBroadcast"),
        jni_sig!(
            "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;"
        ),
        &[
            JValue::Object(context),
            JValue::Int(note),
            JValue::Object(&intent),
            JValue::Int(notify::PENDING_FLAGS),
        ],
    )?
    .l()
}

/// `DmActionReceiver` heard `action` (row `row`, on notification `note`; -1 for none).
pub(crate) fn received(action: Option<&str>, row: i64, note: i32) -> Result<(), String> {
    match action {
        Some(RETRY) => {
            let row = u64::try_from(row).map_err(|_| "a Retry named no download".to_string())?;
            dm_shell::shell::ask_running_to_retry(&home()?, row);
            if note >= 0 {
                let _ = notify::cancel(note);
            }
            Ok(())
        }
        Some(STOP_ALL) => dm_shell::shell::ask_running_to_stop_all(&home()?)
            .map_err(|e| format!("dm could not stop the downloads: {e}")),
        Some(OPEN) => open_dm(),
        Some(SCHEDULE) => scheduled(),
        other => Err(format!(
            "dm was sent something it does not know: {}",
            other.unwrap_or("nothing")
        )),
    }
}

/// Brings dm to the front, or starts it.
fn open_dm() -> Result<(), String> {
    jvm::with("open itself", |env, context| {
        let intent = notify::launch(env, context)?;
        jvm::add_flags(env, &intent, jvm::FLAG_ACTIVITY_NEW_TASK)?;
        env.call_method(
            context,
            jni_str!("startActivity"),
            jni_sig!("(Landroid/content/Intent;)V"),
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    })
}

/// The schedule's alarm went off. Where dm runs, the foreground service holds it up (an exact
/// alarm lets an app start one from the background) and dm's own schedule starts the queue on
/// time; where it does not (the process was started for the alarm), its window cannot be run
/// without the activity, so a notification says the schedule is due.
fn scheduled() -> Result<(), String> {
    if started().is_none() {
        let _ = notify::channels();
        let (title, body) = Tell::Due.words();
        return notify::post(title, body, None);
    }
    service::hold(wakeup::SCHEDULE_TITLE, wakeup::SCHEDULE_BODY, SCHEDULE_HOLD)
}

/// The phone has started: the schedule's alarm set again from its kept time, and a
/// notification where the saved list has downloads that were going or the schedule's time
/// went by (`crate::wakeup`).
pub(crate) fn booted() -> Result<(), String> {
    let home = home()?;
    let rows = dm_core::saved_rows(&home.join(dm_core::paths::QUEUE));
    let going = rows.iter().any(|row| wakeup::goes_on(&row.state));
    let kept = std::fs::read_to_string(home.join(wakeup::NEXT_START))
        .ok()
        .and_then(|text| wakeup::read_next_start(&text));
    let after = wakeup::after_boot(going, kept, now());
    if let Some(at) = after.alarm {
        set_alarm(at)?;
    }
    if let Some(tell) = after.tell {
        let _ = notify::channels();
        let (title, body) = tell.words();
        notify::post(title, body, None)?;
    }
    Ok(())
}

/// The time now, in milliseconds since 1970.
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Sets the schedule's alarm for `at` (milliseconds since 1970), or clears it (`None`), and
/// keeps its time in dm's home for the phone's next start.
pub(crate) fn arm(at: Option<i64>) -> Result<(), String> {
    let kept = home()?.join(wakeup::NEXT_START);
    match at {
        Some(at) => {
            let exact = set_alarm(at)?;
            std::fs::write(&kept, at.to_string())
                .map_err(|e| format!("dm could not keep the schedule's time: {e}"))?;
            ask_for_exact(exact);
            Ok(())
        }
        None => {
            let _ = std::fs::remove_file(&kept);
            clear_alarm()
        }
    }
}

/// Where the alarm could only be inexact, asks once, in a notification with its reason, for
/// Android's "Alarms & reminders" (gaps row N12). Its refusal costs only the asking.
fn ask_for_exact(exact: bool) {
    let Ok(asked) = home().map(|home| home.join(wakeup::EXACT_ASKED)) else {
        return;
    };
    if !wakeup::asks_for_exact(exact, asked.exists()) {
        return;
    }
    let posted = notify::post_settings(
        wakeup::EXACT_TITLE,
        wakeup::EXACT_BODY,
        "android.settings.REQUEST_SCHEDULE_EXACT_ALARM",
    );
    match posted {
        Ok(()) => {
            let _ = std::fs::write(&asked, b"");
        }
        Err(why) => dm_core::note!(warn, "dm could not ask for exact alarms: {why}"),
    }
}

/// `AlarmManager.setExactAndAllowWhileIdle` for `at`, waking the phone; where Android 12 and up
/// do not allow dm exact alarms (the permission not granted), the inexact
/// `setAndAllowWhileIdle`, which Android may put off a little. Says whether it was exact.
fn set_alarm(at: i64) -> Result<bool, String> {
    jvm::with("set the schedule's alarm", |env, context| {
        let alarms = jvm::service(env, context, "alarm")?;
        let pending = broadcast(env, context, SCHEDULE, -1, ALARM_REQUEST)?;
        let exact = jvm::sdk(env)? < 31
            || env
                .call_method(
                    &alarms,
                    jni_str!("canScheduleExactAlarms"),
                    jni_sig!("()Z"),
                    &[],
                )?
                .z()?;
        let name = if exact {
            jni_str!("setExactAndAllowWhileIdle")
        } else {
            jni_str!("setAndAllowWhileIdle")
        };
        env.call_method(
            &alarms,
            name,
            jni_sig!("(IJLandroid/app/PendingIntent;)V"),
            &[
                JValue::Int(RTC_WAKEUP),
                JValue::Long(at),
                JValue::Object(&pending),
            ],
        )?;
        Ok(exact)
    })
}

/// Clears the schedule's alarm.
fn clear_alarm() -> Result<(), String> {
    jvm::with("clear the schedule's alarm", |env, context| {
        let alarms = jvm::service(env, context, "alarm")?;
        let pending = broadcast(env, context, SCHEDULE, -1, ALARM_REQUEST)?;
        env.call_method(
            &alarms,
            jni_str!("cancel"),
            jni_sig!("(Landroid/app/PendingIntent;)V"),
            &[JValue::Object(&pending)],
        )?;
        Ok(())
    })
}
