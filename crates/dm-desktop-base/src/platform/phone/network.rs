// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's network and battery as its rules read them (gaps row N10): metered, roaming,
//! on Wi-Fi, Data Saver, and the battery. `dm-android` reads them through JNI from
//! `ConnectivityManager` and the battery's sticky broadcast, and turns Android's raw values
//! into a [`PhoneNetwork`] here ([`PhoneNetwork::of_android`]), where the host's tests reach
//! it. Every field is `None` where nothing said, as on a computer, and an unknown holds
//! nothing (`metered::phone`).

/// `NetworkCapabilities.NET_CAPABILITY_NOT_METERED`.
pub const NET_CAPABILITY_NOT_METERED: i32 = 11;
/// `NetworkCapabilities.NET_CAPABILITY_NOT_ROAMING`.
pub const NET_CAPABILITY_NOT_ROAMING: i32 = 18;
/// `NetworkCapabilities.TRANSPORT_WIFI`.
pub const TRANSPORT_WIFI: i32 = 1;
/// `NetworkCapabilities.TRANSPORT_ETHERNET`.
pub const TRANSPORT_ETHERNET: i32 = 3;
/// `ConnectivityManager.RESTRICT_BACKGROUND_STATUS_ENABLED`: Data Saver on, and dm not among
/// the apps it lets through (`..._WHITELISTED`, 2, and `..._DISABLED`, 1, are not held).
pub const RESTRICT_BACKGROUND_STATUS_ENABLED: i32 = 3;

/// What the phone says of its connection and its battery now; `None` where it does not say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhoneNetwork {
    /// The connection is metered: mobile data, or a Wi-Fi the phone marks as metered.
    pub metered: Option<bool>,
    /// The phone is roaming.
    pub roaming: Option<bool>,
    /// The connection is Wi-Fi or a cable, not mobile data.
    pub wifi: Option<bool>,
    /// The system's Data Saver holds dm's background data.
    pub data_saver: Option<bool>,
    /// How full the battery is, 0 to 100.
    pub battery_percent: Option<u8>,
    /// On a charger.
    pub charging: Option<bool>,
}

/// The active network's capabilities, as `NetworkCapabilities` answers `hasCapability` and
/// `hasTransport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// `NET_CAPABILITY_NOT_METERED`.
    pub not_metered: bool,
    /// `NET_CAPABILITY_NOT_ROAMING`.
    pub not_roaming: bool,
    /// `TRANSPORT_WIFI` or `TRANSPORT_ETHERNET`.
    pub wifi_or_cable: bool,
}

/// The battery's sticky broadcast (`ACTION_BATTERY_CHANGED`): its `level`, `scale` and
/// `plugged` extras, each `-1` where it is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryExtras {
    /// `BatteryManager.EXTRA_LEVEL`.
    pub level: i32,
    /// `BatteryManager.EXTRA_SCALE`.
    pub scale: i32,
    /// `BatteryManager.EXTRA_PLUGGED`: 0 on the battery, a charger's kind otherwise.
    pub plugged: i32,
}

impl PhoneNetwork {
    /// What Android's answers say: `network`, the active network's capabilities (`None` with
    /// no network, or none Android would describe); `restrict_background`,
    /// `getRestrictBackgroundStatus()` (`None` where it could not be read); `battery`, the
    /// sticky broadcast's extras (`None` where there was none).
    #[must_use]
    pub fn of_android(
        network: Option<Capabilities>,
        restrict_background: Option<i32>,
        battery: Option<BatteryExtras>,
    ) -> Self {
        let percent = battery.and_then(|b| {
            if b.level < 0 || b.scale <= 0 {
                return None;
            }
            let percent = i64::from(b.level) * 100 / i64::from(b.scale);
            u8::try_from(percent.clamp(0, 100)).ok()
        });
        PhoneNetwork {
            metered: network.map(|n| !n.not_metered),
            roaming: network.map(|n| !n.not_roaming),
            wifi: network.map(|n| n.wifi_or_cable),
            data_saver: restrict_background.map(|s| s == RESTRICT_BACKGROUND_STATUS_ENABLED),
            battery_percent: percent,
            charging: battery.and_then(|b| (b.plugged >= 0).then_some(b.plugged > 0)),
        }
    }
}

/// What the phone says now, through the services `dm-android` handed in; all unknown on a
/// computer, or before they are.
#[must_use]
pub fn network() -> PhoneNetwork {
    super::phone().map_or_else(PhoneNetwork::default, |phone| phone.network())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wifi() -> Capabilities {
        Capabilities {
            not_metered: true,
            not_roaming: true,
            wifi_or_cable: true,
        }
    }

    /// Wi-Fi at home, mobile data abroad, and no network at all, as Android's answers say.
    #[test]
    fn androids_answers_are_read() {
        let home = PhoneNetwork::of_android(
            Some(wifi()),
            Some(1),
            Some(BatteryExtras {
                level: 45,
                scale: 100,
                plugged: 0,
            }),
        );
        assert_eq!(
            home,
            PhoneNetwork {
                metered: Some(false),
                roaming: Some(false),
                wifi: Some(true),
                data_saver: Some(false),
                battery_percent: Some(45),
                charging: Some(false),
            }
        );
        let abroad = PhoneNetwork::of_android(
            Some(Capabilities::default()),
            Some(RESTRICT_BACKGROUND_STATUS_ENABLED),
            Some(BatteryExtras {
                level: 3,
                scale: 4,
                plugged: 2,
            }),
        );
        assert_eq!(
            (
                abroad.metered,
                abroad.roaming,
                abroad.wifi,
                abroad.data_saver
            ),
            (Some(true), Some(true), Some(false), Some(true))
        );
        assert_eq!(
            (abroad.battery_percent, abroad.charging),
            (Some(75), Some(true))
        );
        // No network, no Data Saver answer, no battery broadcast: nothing known.
        assert_eq!(
            PhoneNetwork::of_android(None, None, None),
            PhoneNetwork::default()
        );
    }

    /// Data Saver with dm let through is not Data Saver for dm; a broken broadcast is unknown.
    #[test]
    fn what_android_leaves_out_is_unknown() {
        assert_eq!(
            PhoneNetwork::of_android(None, Some(2), None).data_saver,
            Some(false)
        );
        let broken = PhoneNetwork::of_android(
            None,
            None,
            Some(BatteryExtras {
                level: -1,
                scale: 100,
                plugged: -1,
            }),
        );
        assert_eq!((broken.battery_percent, broken.charging), (None, None));
        let zero_scale = BatteryExtras {
            level: 5,
            scale: 0,
            plugged: 1,
        };
        let read = PhoneNetwork::of_android(None, None, Some(zero_scale));
        assert_eq!((read.battery_percent, read.charging), (None, Some(true)));
    }
}
