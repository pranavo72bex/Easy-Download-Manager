// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options › Advanced's Dial Up / VPN section (Windows).

use super::*;

/// How long the connections read once are kept before they are read again.
const CONNECTIONS_FOR: std::time::Duration = std::time::Duration::from_secs(2);

/// The connections Windows knows (none elsewhere), read again at most every
/// two seconds: the page is drawn every frame it is open.
pub(super) fn known_connections() -> Vec<String> {
    static READ: std::sync::Mutex<Option<(std::time::Instant, Vec<String>)>> =
        std::sync::Mutex::new(None);
    let mut read = edm_core::lock(&READ);
    match &*read {
        Some((at, names)) if at.elapsed() < CONNECTIONS_FOR => names.clone(),
        _ => {
            let names = edm_shell::dialup::connections();
            *read = Some((std::time::Instant::now(), names.clone()));
            names
        }
    }
}

/// Advanced's Dial Up / VPN section (Windows): reference A's switch, then
/// the connection with its sign-in, then the redials, which wait for a
/// connection chosen.
pub(super) fn dialup(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let dial = o.draft.advanced.dialup.clone();
    p.section(t("Dial Up / VPN"));
    check(
        p,
        cx,
        "dialup-enabled",
        t("Use Windows Dial Up / VPN Networking"),
        dial.enabled,
        true,
        |s| s.advanced.dialup.enabled = !s.advanced.dialup.enabled,
    );
    let on = dial.enabled;
    let known = known_connections();
    let choices: Vec<SharedString> = std::iter::once(t("None").into())
        .chain(known.iter().map(|name| SharedString::from(name.clone())))
        .collect();
    let (now, chosen) = if dial.connection.is_empty() {
        (t("None").to_string(), Some(0))
    } else {
        (
            dial.connection.clone(),
            known
                .iter()
                .position(|name| *name == dial.connection)
                .map(|i| i + 1),
        )
    };
    p.labelled(
        t("Connection"),
        on,
        popup::button(
            "dialup-connection",
            now.into(),
            choices,
            chosen,
            theme,
            &screen,
        ),
    );
    let chosen_one = on && !dial.connection.is_empty();
    for (key, words, masked) in [
        ("dialup.username", "User name", false),
        ("dialup.password", "Password", true),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            200.0,
            chosen_one,
            masked,
            move |o| match key {
                "dialup.username" => o.draft.advanced.dialup.username.clone(),
                _ => o.draft.advanced.dialup.password.clone(),
            },
        );
        p.labelled(t(words), chosen_one, field);
    }
    check(
        p,
        cx,
        "dialup-save-password",
        t("Save password"),
        dial.save_password,
        chosen_one,
        |s| s.advanced.dialup.save_password = !s.advanced.dialup.save_password,
    );
    for (key, words) in [
        ("dialup.attempts", "Redial attempts (zero if endlessly)"),
        (
            "dialup.wait_seconds",
            "Time between redial attempts (seconds)",
        ),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            NUMBER,
            chosen_one,
            false,
            move |o| match key {
                "dialup.attempts" => o.draft.advanced.dialup.attempts.to_string(),
                _ => o.draft.advanced.dialup.wait_seconds.to_string(),
            },
        );
        p.labelled(t(words), chosen_one, field);
    }
    p.hint(t("A connection Windows knows (Settings › Network & internet › Dial-up or VPN). A download waits while EDM connects it, and stops if it does not come up. With no user name or password here, EDM uses the ones Windows keeps; without Save password, the one typed is forgotten when EDM closes."));
}
