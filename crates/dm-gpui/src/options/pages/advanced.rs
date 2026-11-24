// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' Advanced page, with its proxy's settings: its sections in the
//! page's order, each adding its rows under the one before.

use super::*;

pub(super) fn advanced(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    network(o, p, window, cx);
    only_through(o, p);
    second_network(o, p, cx);
    site_limits(o, p, window, cx);
    site_rules(o, p, window, cx);
    cookies(o, p, window, cx);
    identity(o, p, cx);
    // Where a proxy that cannot be one was said, the page shows it.
    let proxy_at = p.items.len();
    p.section(t("Proxy"));
    proxy(o, p, window, cx);
    if std::mem::take(&mut o.to_proxy) {
        // Its heading at the top of the page, its settings under it.
        // A phone's page holds the search and the pages' strip first.
        o.scroll
            .scroll_to_top_of_item(proxy_at + usize::from(crate::platform::ON_A_PHONE));
    }
    // A dial-up or VPN connection every download waits for, in reference
    // A's words and order (contract B43). Windows
    // only: the connections are the ones Windows knows.
    if cfg!(windows) {
        dialup::dialup(o, p, window, cx);
    }
    bittorrent(o, p, window, cx);
}

/// Network: the speed limit, the attempts and the waits.
fn network(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    p.section(t("Network"));
    for (key, words, hint) in [
        (
            "speed_limit",
            "Speed limit, KB/s (0 for none)",
            "Keeps dm from using all of your connection.",
        ),
        (
            "attempts",
            "Attempts per connection (0 to keep trying)",
            "How many times dm tries again after a connection fails.",
        ),
        (
            "retry_wait",
            "Wait between attempts (seconds, 0 for automatic)",
            "How long dm waits before it tries again.",
        ),
        (
            "read_timeout",
            "Time out a silent connection after (seconds)",
            "How long a connection may say nothing before dm gives up on it.",
        ),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            NUMBER,
            true,
            false,
            move |o| match key {
                "speed_limit" => o.draft.advanced.speed_limit.to_string(),
                "attempts" => o.draft.advanced.attempts.to_string(),
                "retry_wait" => o.draft.advanced.retry_wait.to_string(),
                _ => o.draft.advanced.read_timeout.to_string(),
            },
        );
        p.setting(t(words), true, field);
        p.note(t(hint));
    }
}

/// A second network beside the system's, and how to use a phone as one.
fn second_network(o: &mut Options, p: &mut Page, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // A second network beside the one the system routes by, from the networks `only_through`
    // listed.
    let network_words = |o: &Options| -> Vec<SharedString> {
        std::iter::once(SharedString::from(t("None")))
            .chain(o.networks.iter().map(|found| {
                match dm_core::system_name(&found.name) {
                    Some(named) => format!("{named} ({})", found.name),
                    None => format!("{} ({})", found.name, found.address),
                }
                .into()
            }))
            .collect()
    };
    let second = o.draft.second_path.clone();
    let at = if second.is_empty() {
        Some(0)
    } else {
        o.networks
            .iter()
            .position(|n| n.name == second)
            .map(|i| i + 1)
    };
    // Not beside the one network downloads are held to: greyed while one is (gaps row W12).
    p.setting(
        t("Second network"),
        o.draft.advanced.only_through.is_empty(),
        popup::button(
            "network",
            if second.is_empty() {
                t("None").to_string()
            } else {
                second.clone()
            }
            .into(),
            network_words(o),
            at,
            theme,
            &screen,
        ),
    );
    p.note(t("Use a second connection too, such as your phone over USB or its hotspot. Half of each download's connections go over it. Not used while a proxy is on."));
    // How to have a phone there, step by step, shown on a click.
    let phone = o.phone;
    p.add(
        div()
            .id("phone-steps")
            .role(gpui::Role::Button)
            .aria_label(SharedString::from(t("How to use your phone")))
            .aria_expanded(phone)
            .flex()
            .items_center()
            .gap(px(space::XS))
            .text_color(theme.accent)
            .child(screen.label(if phone { "▾" } else { "▸" }))
            .child(screen.label(t("How to use your phone")))
            .on_click(cx.listener(|o, _, _, cx| {
                o.phone = !o.phone;
                cx.notify();
            })),
    );
    if phone {
        for step in [
            t("1. Connect your phone to this computer with its USB cable. If it asks whether to trust this computer, say yes."),
            t("2. Turn on its hotspot: on an iPhone, Settings › Personal Hotspot; on Android, USB tethering."),
            t("3. Choose it above. Keep Wi-Fi as this computer's own connection, so only downloads use the phone's data."),
        ] {
            p.hint(step);
        }
    }
}

/// The one network every download is held to, such as a VPN's (gaps row W12): the computer's
/// networks in a pop-up, as the second network's, "Any network" first.
fn only_through(o: &mut Options, p: &mut Page) {
    let (theme, screen) = (p.theme, p.screen.clone());
    o.networks = dm_core::interfaces();
    let only = o.draft.advanced.only_through.clone();
    let words: Vec<SharedString> = std::iter::once(SharedString::from(t("Any network")))
        .chain(o.networks.iter().map(|found| {
            match dm_core::system_name(&found.name) {
                Some(named) => format!("{named} ({})", found.name),
                None => format!("{} ({})", found.name, found.address),
            }
            .into()
        }))
        .collect();
    // One chosen and not connected now is still the one chosen, by its name.
    let at = if only.is_empty() {
        Some(0)
    } else {
        o.networks
            .iter()
            .position(|n| n.name == only)
            .map(|i| i + 1)
    };
    let shown = if only.is_empty() {
        t("Any network").to_string()
    } else {
        only
    };
    p.setting(
        t("Only through"),
        true,
        popup::button("only-through", shown.into(), words, at, theme, &screen),
    );
    p.note(t("Every download goes out over this network alone, such as your VPN's. While it is not connected, downloads wait and say so, and go on by themselves when it is back. Live streams do not start while one is chosen (on Windows, torrents too), and the second network is not used."));
}

/// Sites with their own connection limit, and the button adding one.
fn site_limits(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let screen = p.screen.clone();
    // Sites with their own connection limit (B57).
    p.words(t("Sites with their own connection limit:"));
    for i in 0..o.draft.advanced.host_limits.len() {
        let host = input(
            o,
            window,
            cx,
            &format!("host_limits.{i}.host"),
            "Site",
            200.0,
            true,
            false,
            move |o| o.draft.advanced.host_limits[i].host.clone(),
        );
        let count = input(
            o,
            window,
            cx,
            &format!("host_limits.{i}.connections"),
            "Connections",
            NUMBER,
            true,
            false,
            move |o| o.draft.advanced.host_limits[i].connections.to_string(),
        );
        let delete = button(p, ("delete-site-limit", i), t("Delete site limit")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.advanced.host_limits.len() {
                    o.draft.advanced.host_limits.remove(i);
                    o.forget("host_limits");
                }
                cx.notify();
            }),
        );
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Site")))
                .child(host)
                .child(screen.label(t("Connections")))
                .child(count)
                .child(delete),
        );
    }
    let add_limit =
        button(p, "add-site-limit", t("Add a site limit")).on_click(cx.listener(|o, _, _, cx| {
            o.draft.advanced.host_limits.push(dm_core::HostLimit {
                host: String::new(),
                connections: 1,
            });
            cx.notify();
        }));
    p.add(div().flex().child(add_limit));
}

/// Sites with their own folder, speed and headers, and the button adding
/// one.
fn site_rules(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let screen = p.screen.clone();
    // Sites with their own folder, speed, User-Agent and request headers (gaps row F1).
    p.words(t("Sites with their own folder, speed and headers:"));
    for i in 0..o.draft.advanced.site_rules.len() {
        let host = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.host"),
            "Site",
            160.0,
            true,
            false,
            move |o| o.draft.advanced.site_rules[i].host.clone(),
        );
        let rule_folder = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.folder"),
            "Folder",
            200.0,
            true,
            false,
            move |o| o.draft.advanced.site_rules[i].folder.clone(),
        );
        let browse = button(p, ("site-rule-browse", i), t("Browse…")).on_click(cx.listener(
            move |o, _, _, cx| {
                o.choose_folder(cx, move |o, folder, cx| {
                    if let Some(rule) = o.draft.advanced.site_rules.get_mut(i) {
                        rule.folder = folder;
                    }
                    o.forget_field(&format!("site_rules.{i}.folder"));
                    cx.notify();
                });
            },
        ));
        let speed = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.speed"),
            "Speed limit, KB/s (0 for none)",
            NUMBER,
            true,
            false,
            move |o| o.draft.advanced.site_rules[i].speed.to_string(),
        );
        let agent = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.agent"),
            "User-Agent",
            260.0,
            true,
            false,
            move |o| o.draft.advanced.site_rules[i].agent.clone(),
        );
        let headers = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.headers"),
            "Request headers",
            260.0,
            true,
            false,
            move |o| o.draft.advanced.site_rules[i].headers.clone(),
        );
        let delete = button(p, ("delete-site-rule", i), t("Delete site rule")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.advanced.site_rules.len() {
                    o.draft.advanced.site_rules.remove(i);
                    o.forget("site_rules");
                }
                cx.notify();
            }),
        );
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Site")))
                .child(host)
                .child(screen.label(t("Folder")))
                .child(rule_folder)
                .child(browse)
                .child(screen.label(t("Speed limit, KB/s (0 for none)")))
                .child(speed)
                .child(delete),
        );
        // The names and headers its requests go with, on a line of their own.
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("User-Agent")))
                .child(agent)
                .child(screen.label(t("Request headers")))
                .child(headers),
        );
    }
    let add_rule =
        button(p, "add-site-rule", t("Add a site rule")).on_click(cx.listener(|o, _, _, cx| {
            o.draft
                .advanced
                .site_rules
                .push(dm_shell::settings::SiteRule::default());
            cx.notify();
        }));
    p.add(div().flex().child(add_rule));
}

/// The cookies file for downloads that need a sign-in.
fn cookies(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    p.words(t(
        "For downloads that need a sign-in: a cookies.txt from your browser.",
    ));
    let cookies = input(
        o,
        window,
        cx,
        "cookies_file",
        "Cookies file",
        260.0,
        true,
        false,
        |o| o.draft.advanced.cookies_file.clone(),
    );
    let choose = button(p, "cookies-choose", t("Choose…")).on_click(cx.listener(|_, _, _, cx| {
        crate::platform::services::ask_then(
            crate::platform::services::Asked {
                filter: Some(("cookies.txt", &["txt"])),
                ..Default::default()
            },
            cx,
            |o: &mut Options, file, cx| {
                o.put("cookies_file", &file.display().to_string());
                o.forget_field("cookies_file");
                cx.notify();
            },
        );
    }));
    let has_cookies = !o.draft.advanced.cookies_file.is_empty();
    let clear = button(p, "cookies-clear", t("Clear")).on_click(cx.listener(|o, _, _, cx| {
        o.put("cookies_file", "");
        o.forget_field("cookies_file");
        cx.notify();
    }));
    p.labelled(
        t("Cookies file"),
        true,
        div()
            .flex()
            .items_center()
            .gap(px(native::LABEL_GAP))
            .child(cookies)
            .child(choose)
            .when(has_cookies, |d| d.child(clear)),
    );
}

/// The name dm gives servers, and the switch for dm's own.
fn identity(o: &mut Options, p: &mut Page, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // The name dm gives servers first, and the switch for dm's own (B51).
    p.words(t("dm identifies itself to servers as:"));
    let agent = if o.draft.identify_as_dm {
        dm_core::USER_AGENT
    } else {
        dm_core::IE_AGENT
    };
    p.add(
        div()
            .font_family(crate::widgets::native::mono(cx))
            .text_size(px(native::SMALL_TEXT))
            .text_color(theme.muted)
            .child(screen.label(agent)),
    );
    check(
        p,
        cx,
        "identify-as-dm",
        t("Identify as dm only"),
        o.draft.identify_as_dm,
        true,
        |s| s.identify_as_dm = !s.identify_as_dm,
    );
    p.hint(t(
        "Sends dm's own name first. A site that refuses it is asked again with another name.",
    ));
}

/// BitTorrent: the peers' blocklist and sharing a finished torrent.
fn bittorrent(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    p.section(t("BitTorrent"));
    let blocklist = input(
        o,
        window,
        cx,
        "peer_blocklist",
        "Block the peers listed at this address (optional)",
        360.0,
        true,
        false,
        |o| o.draft.peer_blocklist.clone(),
    );
    p.labelled(
        t("Block the peers listed at this address (optional)"),
        true,
        blocklist,
    );
    p.note(t("A list in the P2P format, plain or gzipped, on the web or on this computer. One on the web is fetched through the proxy and read again once a day; a torrent stops, saying why, if the list cannot be read."));
    // Off unless turned on: sharing sends from the person's connection.
    let share = o.draft.torrent_share;
    check(
        p,
        cx,
        "torrent-share",
        t("Keep sharing a finished torrent"),
        share,
        true,
        |s| s.torrent_share = !s.torrent_share,
    );
    p.hint(t("Its files stay where they are, and other peers are sent them until it has sent the times its size below, or for the minutes below, whichever comes first. Off, a torrent stops once its files are whole."));
    for (key, words) in [
        ("torrent_share_ratio", "Until it has sent (times its size)"),
        ("torrent_share_minutes", "For at most (minutes)"),
        (
            "torrent_upload_limit",
            "Send at most (KB/s, 0 for no limit)",
        ),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            NUMBER,
            share,
            false,
            move |o| match key {
                "torrent_share_ratio" => o.draft.torrent_share_ratio.to_string(),
                "torrent_share_minutes" => o.draft.torrent_share_minutes.to_string(),
                _ => o.draft.torrent_upload_limit.to_string(),
            },
        );
        p.setting(t(words), share, field);
    }
    // Windows' firewall question alone: no other system asks it.
    if cfg!(windows) {
        p.note(t("The first torrent makes Windows ask whether dm may use public and private networks. Allowed, other peers can reach dm and torrents go faster; refused, dm still downloads, reaching peers itself. Change it later in Windows Security, Firewall, Allow an app through firewall."));
    }
}

/// The proxy's settings, under Advanced's "Proxy".
fn proxy(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    use dm_core::ProxyKind as Kind;
    let (theme, screen) = (p.theme, p.screen.clone());
    let proxy = o.draft.advanced.proxy.clone();
    check(
        p,
        cx,
        "proxy-enabled",
        t("Connect through a proxy"),
        proxy.enabled,
        true,
        |s| s.advanced.proxy.enabled = !s.advanced.proxy.enabled,
    );
    p.hint(t("For a network that reaches the internet through a proxy server. Whoever runs the network can give you its address."));
    let on = proxy.enabled;
    let kind_words = |kind: Kind| match kind {
        Kind::System => t("The computer's proxy settings").to_string(),
        other => format!("{other:?}").to_uppercase(),
    };
    p.setting(
        t("Proxy type"),
        on,
        popup::button(
            "proxy-kind",
            kind_words(proxy.kind).into(),
            proxy_kinds()
                .iter()
                .map(|k| kind_words(*k).into())
                .collect(),
            proxy_kinds().iter().position(|k| *k == proxy.kind),
            theme,
            &screen,
        ),
    );
    let own = on && proxy.kind != Kind::System;
    for (key, words, width, masked) in [
        ("proxy.host", "Host", 260.0, false),
        ("proxy.port", "Port", NUMBER, false),
        ("proxy.username", "Username, if it needs one", 200.0, false),
        ("proxy.password", "Password", 200.0, true),
    ] {
        let field = input(o, window, cx, key, words, width, own, masked, move |o| {
            let p = &o.draft.advanced.proxy;
            match key {
                "proxy.host" => p.host.clone(),
                "proxy.port" => p.port.to_string(),
                "proxy.username" => p.username.clone(),
                _ => p.password.clone(),
            }
        });
        p.labelled(t(words), own, field);
    }
    let bypass = input(
        o,
        window,
        cx,
        "bypass",
        "Not for these sites (fetched directly)",
        260.0,
        own,
        false,
        |o| o.typed.bypass.clone(),
    );
    p.labelled(t("Not for these sites (fetched directly)"), own, bypass);
    per_scheme(o, p, &proxy, own, window, cx);
    script(o, p, on, window, cx);
}

/// A proxy of its own for one kind of download; the rest go through the
/// one above.
fn per_scheme(
    o: &mut Options,
    p: &mut Page,
    proxy: &dm_core::Proxy,
    own: bool,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    use dm_core::ProxyKind as Kind;
    let (theme, screen) = (p.theme, p.screen.clone());
    for (scheme, words) in [
        ("http", t("A proxy of its own for HTTP downloads")),
        ("https", t("A proxy of its own for HTTPS downloads")),
    ] {
        let has = proxy.per_scheme.contains_key(scheme);
        let toggle = native::switch(
            SharedString::from(format!("per-scheme-{scheme}")),
            words.into(),
            has,
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            let p = &mut o.draft.advanced.proxy;
            if p.per_scheme.remove(scheme).is_none() {
                p.per_scheme.insert(
                    scheme.to_string(),
                    dm_core::SchemeProxy {
                        kind: Kind::Http,
                        host: String::new(),
                        port: 8080,
                        username: String::new(),
                        password: String::new(),
                    },
                );
            }
            o.forget("per_scheme");
            cx.notify();
        }));
        p.add(native::enabled(own, toggle));
        if has {
            let host = input(
                o,
                window,
                cx,
                &format!("per_scheme.{scheme}.host"),
                "Its host",
                220.0,
                own,
                false,
                move |o| {
                    let entry = o.draft.advanced.proxy.per_scheme.get(scheme);
                    entry.map(|e| e.host.clone()).unwrap_or_default()
                },
            );
            let port = input(
                o,
                window,
                cx,
                &format!("per_scheme.{scheme}.port"),
                "Its port",
                NUMBER,
                own,
                false,
                move |o| {
                    let entry = o.draft.advanced.proxy.per_scheme.get(scheme);
                    entry.map(|e| e.port.to_string()).unwrap_or_default()
                },
            );
            p.add(native::enabled(
                own,
                div()
                    .pl(px(UNDER_CHECK))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(screen.label(t("Its host")))
                    .child(host)
                    .child(screen.label(t("Its port")))
                    .child(port),
            ));
        }
    }
}

/// The automatic configuration script, and what goes through the proxy.
fn script(o: &mut Options, p: &mut Page, on: bool, window: &mut Window, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let script = input(
        o,
        window,
        cx,
        "proxy.script",
        "Automatic configuration script address (it chooses each site's proxy, in place of the above)",
        WIDE,
        on,
        false,
        |o| o.draft.advanced.proxy.script.clone(),
    );
    p.add(native::enabled(
        on,
        div()
            .flex()
            .flex_col()
            .gap(px(space::XS))
            .child(screen.label(t("Automatic configuration script address (it chooses each site's proxy, in place of the above)")))
            .child(script),
    ));
    p.add(native::enabled(
        on,
        native::note(
            t("Every download, the video tools and a torrent's SOCKS5 connections go through it; FTP through SOCKS or HTTP CONNECT."),
            theme.muted,
            &screen,
        ),
    ));
}
