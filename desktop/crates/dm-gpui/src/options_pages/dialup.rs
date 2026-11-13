// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options › Advanced's Dial Up / VPN section (Windows).

use super::*;

/// The connections Windows knows (none elsewhere), read again at most every
/// two seconds: the page is drawn every frame it is open.
pub(super) fn known_connections() -> Vec<String> {
    static READ: std::sync::Mutex<Option<(std::time::Instant, Vec<String>)>> =
        std::sync::Mutex::new(None);
    let mut read = dm_core::lock(&READ);
    match &*read {
        Some((at, names)) if at.elapsed() < std::time::Duration::from_secs(2) => names.clone(),
        _ => {
            let names = dm_shell::dialup::connections();
            *read = Some((std::time::Instant::now(), names.clone()));
            names
        }
    }
}

/// Advanced's Dial Up / VPN section (Windows), as the egui window's:
/// reference A's switch, then the connection with its sign-in, then the
/// redials, which wait for a connection chosen.
pub(super) fn dialup(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let dial = o.draft.dialup.clone();
    p.section(t("Dial Up / VPN"));
    check(
        p,
        cx,
        "dialup-enabled",
        t("Use Windows Dial Up / VPN Networking"),
        dial.enabled,
        true,
        |s| s.dialup.enabled = !s.dialup.enabled,
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
                "dialup.username" => o.draft.dialup.username.clone(),
                _ => o.draft.dialup.password.clone(),
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
        |s| s.dialup.save_password = !s.dialup.save_password,
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
                "dialup.attempts" => o.draft.dialup.attempts.to_string(),
                _ => o.draft.dialup.wait_seconds.to_string(),
            },
        );
        p.labelled(t(words), chosen_one, field);
    }
    p.note(t("A connection Windows knows (Settings › Network & internet › Dial-up or VPN). A download waits while dm connects it, and stops if it does not come up. With no user name or password here, dm uses the ones Windows keeps; without Save password, the one typed is forgotten when dm closes."));
}
