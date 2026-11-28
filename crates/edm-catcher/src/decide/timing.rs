// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How long the extension waits for, and believes, each answer.

/// How long an app that answered is believed. Its settings may change, so
/// not for ever; a minute keeps a burst of downloads from starting the host
/// once each.
pub const PRESENT_FOR_MS: f64 = 60_000.0;
/// How long "no app" is believed. Very short: a start that was slow, or a
/// host that failed once, must not send the next downloads to the browser, and
/// an app installed or registered since is found without restarting it.
pub const ABSENT_FOR_MS: f64 = 2_000.0;
/// How long the host has to answer the handshake. Not its reply time, which
/// is milliseconds, but its start: the browser spawns it cold, and a freshly
/// installed binary can be held by the system's first-run checks.
pub const HELLO_WITHIN_MS: f64 = 10_000.0;
/// How long a host that has said hello has to send its settings before an
/// older host that does not know the question is assumed.
pub const SETTINGS_WITHIN_MS: f64 = 3_000.0;
/// How long the host has to take a download. It files it and, if the app is
/// closed, starts it; both are quick, but a slow disk is not an error.
/// Long enough for the host's own first request (`PROBE_WITHIN_MS`, 16 s),
/// its margin for answering, and five seconds for the host to start.
pub const TAKEN_WITHIN_MS: f64 = 24_000.0;

/// How long the app has to say whether it takes a download the browser started
/// (`catch.judge`): its rules are read from a file, in milliseconds. Past it the rules the app
/// last sent judge, as they would for an app that does not judge.
pub const JUDGED_WITHIN_MS: f64 = 5_000.0;

/// How long the app has to make the panel's list of a tab's streams (`media.streams`). Past it
/// this extension makes the list itself.
pub const STREAMS_LISTED_WITHIN_MS: f64 = 3_000.0;
/// How long a URL given back to the browser is left alone if it comes round
/// again, which it does when the extension has to restart it.
pub const RELEASED_FOR_MS: f64 = 60_000.0;

/// How long Chrome's choice of a download's file name is held while the app
/// is asked about it: the handshake, the settings and the hand-off, with a
/// little to spare. Past it the browser carries on whatever happens.
pub const DETERMINE_WITHIN_MS: f64 =
    HELLO_WITHIN_MS + SETTINGS_WITHIN_MS + TAKEN_WITHIN_MS + 5_000.0;
// Held for longer than the app is given to answer and to take it, or the
// browser would ask where to save a download EDM was about to take.
const _: () = assert!(DETERMINE_WITHIN_MS > HELLO_WITHIN_MS + TAKEN_WITHIN_MS);

/// How often the toolbar popup asks again for an app that did not answer, while it is open (gaps
/// row U18): as often as an absent app's answer is kept ([`ABSENT_FOR_MS`]), so each ask is new.
pub const POPUP_ASK_AGAIN_MS: i32 = ABSENT_FOR_MS as i32;

/// How long the popup's list of downloads waits for the app before it shows nothing new.
pub const DOWNLOADS_LISTED_WITHIN_MS: f64 = 1_500.0;
