//! How long the extension waits for, and believes, each answer.

/// How long an app that answered is believed. Its settings may change, so
/// not for ever; a minute keeps a burst of downloads from starting the host
/// once each.
pub const PRESENT_FOR_MS: f64 = 60_000.0;
/// How long "no app" is believed. Short, so an app installed or registered
/// since is found without restarting the browser.
pub const ABSENT_FOR_MS: f64 = 10_000.0;
/// How long the host has to answer the handshake. Not its reply time, which
/// is milliseconds, but its start: the browser spawns it cold, and a freshly
/// installed binary can be held by the system's first-run checks.
pub const HELLO_WITHIN_MS: f64 = 10_000.0;
/// How long a host that has said hello has to send its settings before an
/// older host that does not know the question is assumed.
pub const SETTINGS_WITHIN_MS: f64 = 3_000.0;
/// How long the host has to take a download. It files it and, if the app is
/// closed, starts it; both are quick, but a slow disk is not an error.
pub const TAKEN_WITHIN_MS: f64 = 20_000.0;
/// How long a URL given back to the browser is left alone if it comes round
/// again, which it does when the extension has to restart it.
pub const RELEASED_FOR_MS: f64 = 60_000.0;

/// How long Chrome's choice of a download's file name is held while the app
/// is asked about it: the handshake, the settings and the hand-off, with a
/// little to spare. Past it the browser carries on whatever happens.
pub const DETERMINE_WITHIN_MS: f64 =
    HELLO_WITHIN_MS + SETTINGS_WITHIN_MS + TAKEN_WITHIN_MS + 5_000.0;
// Held for longer than the app is given to answer and to take it, or the
// browser would ask where to save a download dm was about to take.
const _: () = assert!(DETERMINE_WITHIN_MS > HELLO_WITHIN_MS + TAKEN_WITHIN_MS);
