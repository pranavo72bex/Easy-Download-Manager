// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A phone's own lines on Options' Downloads page (gaps row N10): Wi-Fi only, not while
//! roaming, and Data Saver, under W11's metered connection and battery. Built for Android
//! alone: a computer's Options never show them (the behaviour contract), and their wording
//! waits for the maintainer's word (docs/gaps.md, N10). Each holds every download as W11's
//! lines do (`dm_desktop_base::metered::phone`), its reason said on the rows.

use super::*;

/// The phone's three check boxes, each with what it does under it.
pub(super) fn phone_rules(p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    check(
        p,
        cx,
        "wifi-only",
        t("Wi-Fi only"),
        s.downloads.wifi_only,
        true,
        |s| s.downloads.wifi_only = !s.downloads.wifi_only,
    );
    p.hint(t(
        "On mobile data, downloads wait, and go on by themselves on Wi-Fi.",
    ));
    check(
        p,
        cx,
        "not-roaming",
        t("Not while roaming"),
        s.downloads.not_roaming,
        true,
        |s| s.downloads.not_roaming = !s.downloads.not_roaming,
    );
    p.hint(t("Downloads wait while the phone is roaming."));
    check(
        p,
        cx,
        "data-saver-waits",
        t("Pause when Data Saver is on"),
        s.downloads.data_saver_waits,
        true,
        |s| s.downloads.data_saver_waits = !s.downloads.data_saver_waits,
    );
    p.hint(t(
        "Downloads wait while Android's Data Saver is on, and go on once it is off.",
    ));
    check(
        p,
        cx,
        "keep-after-close",
        t("Keep downloads and torrents going when dm is closed"),
        s.downloads.keep_after_close,
        true,
        |s| s.downloads.keep_after_close = !s.downloads.keep_after_close,
    );
    p.hint(t(
        "Swiped away from the recent apps, dm goes on under its notification until every download is done and every torrent has finished sharing.",
    ));
}
