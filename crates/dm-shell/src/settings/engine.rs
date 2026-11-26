// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The engine's view of the settings: its Options, limits and client.

use std::path::PathBuf;

use dm_core::{Categories, Limiter, Options, Quality};

use super::{home, Settings};

impl Settings {
    /// The engine's view of all of this.
    ///
    /// One translation, used to build the queue at startup and to update it
    /// on every change. Written out twice it had already drifted — only the
    /// startup copy knew about the parts directory, and a new setting had to
    /// be remembered in both.
    pub fn options(&self, parts_dir: PathBuf, limiter: Limiter) -> Options {
        Options {
            connections: self.downloads.connections,
            control: dm_core::Control::new(),
            dest_dir: self.folders.download_dir.clone(),
            parts_dir,
            limiter,
            categories: self.categories(),
            // Per download, not a setting: the queue fills this in from the
            // row when it starts one.
            checksum: None,
            torrent: dm_core::TorrentOptions {
                // Per download: the queue fills these in from the row.
                only: None,
                tiers: Vec::new(),
                blocklist: self.peer_blocklist(),
                share: self.torrent_share.then(|| dm_core::Share {
                    ratio: self.torrent_share_ratio.clamp(0.1, 100.0),
                    minutes: self.torrent_share_minutes.clamp(1, 7 * 24 * 60),
                    upload_kbps: self.torrent_upload_limit,
                }),
                // Peers through the DHT and local discovery as well as trackers.
                no_public_discovery: false,
                dht_nodes: Some(home().join(dm_core::paths::DHT_NODES)),
            },
            quality: self.quality(),
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            media: false,
            referrer: None,
            // For the tools dm runs; the client has its own copy.
            proxy: self.advanced.proxy.url_for_tools(),
            container: None,
            attempts: self.advanced.attempts,
            host_limits: self.advanced.host_limits.clone(),
            subtitles: self.downloads.subtitles,
            room: Default::default(),
            remux: self.downloads.remux_ts,
            server_dates: self.general.server_dates,
            retry_wait: self.advanced.retry_wait,
            record_limit: (self.downloads.record_minutes > 0)
                .then(|| std::time::Duration::from_secs(self.downloads.record_minutes * 60)),
            frame_rate: None,
            caption: None,
            format: None,
            sign_in: None,
            save_name: None,
            title_hint: None,
            accept_page: false,
            same_files: None,
            system_proxy: self.advanced.proxy.enabled
                && self.advanced.proxy.kind == dm_core::ProxyKind::System,
            proxy_bypass: if self.advanced.proxy.enabled {
                self.advanced.proxy.bypass.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// The peer blocklist's address, where one is set.
    pub fn peer_blocklist(&self) -> Option<String> {
        let address = self.peer_blocklist.trim();
        (!address.is_empty()).then(|| address.to_string())
    }

    /// How often the recheck queue asks its servers again, if on its own.
    pub fn recheck_every(&self) -> Option<std::time::Duration> {
        (self.downloads.recheck_hours > 0)
            .then(|| std::time::Duration::from_secs(self.downloads.recheck_hours * 3600))
    }

    /// Where unfinished downloads are staged: the folder chosen, or dm's own.
    pub fn parts_dir(&self) -> PathBuf {
        let chosen = self.folders.parts_dir.trim();
        if chosen.is_empty() {
            home().join(dm_core::paths::PARTS)
        } else {
            PathBuf::from(chosen)
        }
    }

    /// The start times that are times, for the queue.
    pub fn queue_start_list(&self) -> Vec<(String, u32)> {
        self.downloads
            .queue_starts
            .iter()
            .filter_map(|s| Some((s.queue.clone(), dm_core::minutes_of(&s.at)?)))
            .collect()
    }

    /// The per-queue limits as the engine takes them.
    pub fn queue_limit_map(&self) -> std::collections::HashMap<String, usize> {
        self.downloads
            .queue_limits
            .iter()
            .map(|l| (l.queue.clone(), l.files))
            .collect()
    }

    /// A client as these settings ask for one: through the proxy, giving up on a silent
    /// connection after the read timeout (`dm_core::client_with_timeout`).
    pub fn client(&self) -> Result<dm_core::reqwest::Client, String> {
        dm_core::client_with_timeout(&self.advanced.proxy, self.read_timeout())
    }

    /// How long a connection may send nothing, in the client's units.
    pub fn read_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.advanced.read_timeout.max(1))
    }

    /// The engine's view of the sorting settings.
    pub fn categories(&self) -> Categories {
        Categories {
            enabled: self.folders.categorize,
            folders: self.folders.category_folders.clone(),
            custom: self.custom_categories.clone(),
            ..Default::default()
        }
    }

    /// The download limit, in the engine's units.
    pub fn allowance(&self) -> dm_core::quota::Allowance {
        dm_core::quota::Allowance {
            bytes: self.limit_mb.saturating_mul(dm_core::KB * dm_core::KB),
            period: std::time::Duration::from_secs(self.limit_hours.max(1) * 3600),
        }
    }

    /// What dm calls the network `interface`: the name a person gave it, else the system's,
    /// else the interface's own (gaps row K4).
    pub fn network_name(&self, interface: &str) -> String {
        self.network_names
            .get(interface)
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| dm_core::system_name(interface))
            .unwrap_or_else(|| interface.to_string())
    }

    /// The second network's and the third's own allowances, in the engine's units, as
    /// [`Settings::allowance`] says all downloads' (gaps row K5d).
    pub fn network_allowances(&self) -> Vec<dm_core::quota::Allowance> {
        [
            (self.second_limit_mb, self.second_limit_hours),
            (self.third_limit_mb, self.third_limit_hours),
        ]
        .into_iter()
        .map(|(mb, hours)| dm_core::quota::Allowance {
            bytes: mb.saturating_mul(dm_core::KB * dm_core::KB),
            period: std::time::Duration::from_secs(hours.max(1) * 3600),
        })
        .collect()
    }

    /// The speed ceiling in bytes a second, which is what the engine counts.
    pub fn speed_limit_bytes(&self) -> u64 {
        self.advanced.speed_limit.saturating_mul(dm_core::KB)
    }

    /// The speed limit in force now, in bytes a second: the lower of the
    /// two in the slow hours, the speed limit the rest of the day.
    pub fn speed_limit_bytes_now(&self) -> u64 {
        let (weekday, minute) = dm_core::weekday_now();
        self.speed_limit_bytes_at(weekday, minute)
    }

    /// `speed_limit_bytes_now` on a given weekday (0 = Sunday) and minute
    /// past midnight.
    pub fn speed_limit_bytes_at(&self, weekday: u8, minute: u32) -> u64 {
        let normal = self.speed_limit_bytes();
        if !self.slow_hours.open_at(weekday, minute) {
            return normal;
        }
        let slow = self.slow_hours.limit.saturating_mul(dm_core::KB);
        if normal == 0 {
            slow
        } else {
            normal.min(slow)
        }
    }

    /// [`Self::speed_limit_bytes_at`], where `present` says whether someone is at the
    /// computer: with "Only while I'm at the computer" on and nobody there, no limit (gaps row
    /// C5).
    pub fn speed_limit_bytes_for(&self, weekday: u8, minute: u32, present: bool) -> u64 {
        if self.advanced.limit_only_present && !present {
            return 0;
        }
        self.speed_limit_bytes_at(weekday, minute)
    }

    /// The engine's view of the stream quality.
    ///
    /// Anything unrecognised means the best available — a typo in a settings
    /// file should not silently fetch the smallest rendition on offer.
    pub fn quality(&self) -> Quality {
        Quality::named(&self.downloads.stream_quality)
    }
}
