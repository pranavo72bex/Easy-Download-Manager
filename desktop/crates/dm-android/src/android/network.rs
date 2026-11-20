// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's connection and battery, for its rules (gaps row N10), read through JNI with no
//! Java of dm's own: `ConnectivityManager`'s active network and its capabilities (metered,
//! roaming, Wi-Fi or a cable), its `getRestrictBackgroundStatus` (Data Saver), and the
//! battery's sticky `ACTION_BATTERY_CHANGED` broadcast, taken by `registerReceiver(null,
//! filter)`, which needs no receiver class. What Android's answers mean is decided in
//! `dm_desktop_base::phone::network`, where the host's tests reach it; the services the app
//! hands to `dm_desktop_base::phone::set_phone` answer `PhoneServices::network` with [`read`].
//! A call that fails leaves its part unknown, which holds nothing.

use dm_desktop_base::phone::network::{
    BatteryExtras, Capabilities, NET_CAPABILITY_NOT_METERED, NET_CAPABILITY_NOT_ROAMING,
    TRANSPORT_ETHERNET, TRANSPORT_WIFI,
};
use dm_desktop_base::phone::PhoneNetwork;
use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JValue};

use super::jvm;

/// What the phone says now; read every few seconds while one of Options' lines asks for it
/// (`dm_desktop_base::session`'s waits). Needs `ACCESS_NETWORK_STATE`, which the manifest has.
#[allow(
    dead_code,
    reason = "answered by the app's PhoneServices once it is handed in (N3)"
)]
pub(crate) fn read() -> PhoneNetwork {
    let network = jvm::with("read the connection", capabilities)
        .ok()
        .flatten();
    let saver = jvm::with("read Data Saver", restrict_background).ok();
    let battery = jvm::with("read the battery", battery).ok().flatten();
    PhoneNetwork::of_android(network, saver, battery)
}

/// The active network's capabilities; `None` with no network.
fn capabilities(
    env: &mut Env<'_>,
    activity: &JObject<'static>,
) -> jni::errors::Result<Option<Capabilities>> {
    let connectivity = jvm::service(env, activity, "connectivity")?;
    let network = env
        .call_method(
            &connectivity,
            jni_str!("getActiveNetwork"),
            jni_sig!("()Landroid/net/Network;"),
            &[],
        )?
        .l()?;
    if network.is_null() {
        return Ok(None);
    }
    let caps = env
        .call_method(
            &connectivity,
            jni_str!("getNetworkCapabilities"),
            jni_sig!("(Landroid/net/Network;)Landroid/net/NetworkCapabilities;"),
            &[JValue::Object(&network)],
        )?
        .l()?;
    if caps.is_null() {
        return Ok(None);
    }
    let not_metered = has_capability(env, &caps, NET_CAPABILITY_NOT_METERED)?;
    let not_roaming = has_capability(env, &caps, NET_CAPABILITY_NOT_ROAMING)?;
    let wifi_or_cable = has_transport(env, &caps, TRANSPORT_WIFI)?
        || has_transport(env, &caps, TRANSPORT_ETHERNET)?;
    Ok(Some(Capabilities {
        not_metered,
        not_roaming,
        wifi_or_cable,
    }))
}

/// `caps.hasCapability(value)`.
fn has_capability(env: &mut Env<'_>, caps: &JObject<'_>, value: i32) -> jni::errors::Result<bool> {
    env.call_method(
        caps,
        jni_str!("hasCapability"),
        jni_sig!("(I)Z"),
        &[JValue::Int(value)],
    )?
    .z()
}

/// `caps.hasTransport(value)`.
fn has_transport(env: &mut Env<'_>, caps: &JObject<'_>, value: i32) -> jni::errors::Result<bool> {
    env.call_method(
        caps,
        jni_str!("hasTransport"),
        jni_sig!("(I)Z"),
        &[JValue::Int(value)],
    )?
    .z()
}

/// `ConnectivityManager.getRestrictBackgroundStatus()` (Android 7 and up; dm needs 8).
fn restrict_background(env: &mut Env<'_>, activity: &JObject<'static>) -> jni::errors::Result<i32> {
    let connectivity = jvm::service(env, activity, "connectivity")?;
    env.call_method(
        &connectivity,
        jni_str!("getRestrictBackgroundStatus"),
        jni_sig!("()I"),
        &[],
    )?
    .i()
}

/// The battery's last `ACTION_BATTERY_CHANGED`, a sticky broadcast: `registerReceiver` with no
/// receiver hands it back and registers nothing.
fn battery(
    env: &mut Env<'_>,
    activity: &JObject<'static>,
) -> jni::errors::Result<Option<BatteryExtras>> {
    let action = env.new_string("android.intent.action.BATTERY_CHANGED")?;
    let filter = env.new_object(
        jni_str!("android/content/IntentFilter"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(&action)],
    )?;
    let sticky = env
        .call_method(
            activity,
            jni_str!("registerReceiver"),
            jni_sig!(
                "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;"
            ),
            &[JValue::Object(&JObject::null()), JValue::Object(&filter)],
        )?
        .l()?;
    if sticky.is_null() {
        return Ok(None);
    }
    Ok(Some(BatteryExtras {
        level: int_extra(env, &sticky, "level")?,
        scale: int_extra(env, &sticky, "scale")?,
        plugged: int_extra(env, &sticky, "plugged")?,
    }))
}

/// `intent.getIntExtra(name, -1)`.
fn int_extra(env: &mut Env<'_>, intent: &JObject<'_>, name: &str) -> jni::errors::Result<i32> {
    let name = env.new_string(name)?;
    env.call_method(
        intent,
        jni_str!("getIntExtra"),
        jni_sig!("(Ljava/lang/String;I)I"),
        &[JValue::Object(&name), JValue::Int(-1)],
    )?
    .i()
}
