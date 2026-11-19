// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' Sign-ins page.

use super::*;

pub(super) fn sign_ins(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let screen = p.screen.clone();
    p.words(t(
        "A user and password used for every download from a host.",
    ));
    p.note(t(
        "Sent as a Basic login, or as a Digest where the server asks for one.",
    ));
    for i in 0..o.draft.site_logins.len() {
        let host = input(
            o,
            window,
            cx,
            &format!("site_logins.{i}.host"),
            "Host",
            150.0,
            true,
            false,
            move |o| o.draft.site_logins[i].host.clone(),
        );
        let user = input(
            o,
            window,
            cx,
            &format!("site_logins.{i}.username"),
            "User",
            110.0,
            true,
            false,
            move |o| o.draft.site_logins[i].username.clone(),
        );
        let password = input(
            o,
            window,
            cx,
            &format!("site_logins.{i}.password"),
            "Password",
            110.0,
            true,
            true,
            move |o| o.draft.site_logins[i].password.clone(),
        );
        let delete = button(p, ("delete-login", i), t("Delete")).on_click(cx.listener(
            move |o, _, _, cx| {
                if i < o.draft.site_logins.len() {
                    o.draft.site_logins.remove(i);
                    o.forget("site_logins");
                }
                cx.notify();
            },
        ));
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Host")))
                .child(host)
                .child(screen.label(t("User")))
                .child(user)
                .child(screen.label(t("Password")))
                .child(password)
                .child(delete),
        );
    }
    let add = button(p, "add-login", t("Add a login")).on_click(cx.listener(|o, _, _, cx| {
        o.draft
            .site_logins
            .push(dm_shell::settings::SiteLogin::default());
        cx.notify();
    }));
    p.add(div().flex().child(add));
    p.note(t(
        "Kept in dm's settings file on this computer, readable by your account.",
    ));
}
