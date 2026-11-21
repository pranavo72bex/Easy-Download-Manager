// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The proxy and the Advanced page: a bypass list, a script, a proxy refused, and dial-up.

use super::*;

#[test]
fn the_proxys_bypass_list_and_the_recheck_timer_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    h.fill("Host", "proxy.example");
    h.fill(
        "Not for these sites (fetched directly)",
        "Intranet.example 10.0.0.5",
    );
    // The recheck timer lives on the Downloads tab; what was typed on Advanced waits there.
    tab(&mut h, "Downloads");
    h.fill("Recheck every (hours, 0 for manual)", "6");
    ok(&mut h);
    let settings = kept(&mut h);
    let proxy = &settings.advanced.proxy;
    assert!(proxy.enabled);
    assert_eq!(proxy.host, "proxy.example");
    assert_eq!(proxy.bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(settings.downloads.recheck_hours, 6);
    let options = in_force(&mut h, &world);
    assert_eq!(options.proxy_bypass, ["intranet.example", "10.0.0.5"]);
    assert_eq!(options.proxy_for("http://files.intranet.example/a"), None);
    assert!(options.proxy_for("http://outside.example/a").is_some());
    // Turned off again from its switch.
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.reveal("Connect through a proxy");
    h.click("Connect through a proxy");
    ok(&mut h);
    assert!(!kept(&mut h).advanced.proxy.enabled);
}

/// Reference A's per-protocol proxies and its automatic configuration script, in the proxy
/// section (gaps row 80).
#[test]
fn a_proxy_of_its_own_for_https_and_a_configuration_script_are_set_in_options() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    h.fill("Host", "proxy.example");
    h.reveal("A proxy of its own for HTTPS downloads");
    h.click("A proxy of its own for HTTPS downloads");
    h.fill("Its host", "secure-proxy.example");
    ok(&mut h);
    let settings = kept(&mut h);
    let https = &settings.advanced.proxy.per_scheme["https"];
    assert_eq!(https.host, "secure-proxy.example");
    assert_eq!(https.port, 8080);
    assert!(!settings.advanced.proxy.per_scheme.contains_key("http"));
    assert!(settings.advanced.proxy.script.is_empty());

    let script = "Automatic configuration script address (it chooses each site's proxy, in place of the above)";
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.fill(script, "http://wpad.example/proxy.pac");
    ok(&mut h);
    assert_eq!(
        kept(&mut h).advanced.proxy.script,
        "http://wpad.example/proxy.pac"
    );
    let saved = saved_settings();
    assert_eq!(saved["proxy"]["script"], "http://wpad.example/proxy.pac");
    assert_eq!(
        saved["proxy"]["per_scheme"]["https"]["host"],
        "secure-proxy.example"
    );
}

/// OK refuses a proxy turned on with no host, or with an address where its host goes, and
/// says why on the Advanced page, at its proxy section; nothing is put in force.
#[test]
fn options_refuses_a_proxy_with_no_host() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    h.click_as_person("OK", 0);
    h.until("the refusal", |h| h.shows_part("Enter the proxy's host"));
    assert!(h.dialog_open("Options"), "OK took a proxy with no host");
    // Nothing was put in force: the proxy still follows the computer's settings.
    let untouched = |h: &mut Harness| kept(h).advanced.proxy.kind == dm_core::ProxyKind::System;
    assert!(untouched(&mut h));
    h.fill("Host", "http://proxy.example:3128");
    h.click_as_person("OK", 0);
    h.until("the refusal", |h| h.shows_part("isn't a valid proxy host"));
    assert!(untouched(&mut h));
    h.click("Cancel");
}

/// Options has one Advanced page for what were the Connection and Proxy tabs: both groups of
/// settings are on it, and neither tab is in the list any more.
#[test]
fn advanced_holds_the_connection_and_the_proxy_settings_on_one_page() {
    use dm_desktop_base::options::{Tab, TABS};
    assert!(TABS.iter().any(|(tab, _)| *tab == Tab::Advanced));
    assert!(
        !TABS
            .iter()
            .any(|(_, name)| *name == "Network" || *name == "Proxy"),
        "the Connection and Proxy tabs are still in the list"
    );
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    for name in [
        "Speed limit, KB/s (0 for none)",
        "Attempts per connection (0 to keep trying)",
        "Wait between attempts (seconds, 0 for automatic)",
        "Time out a silent connection after (seconds)",
        // The one network every download is held to (gaps row W12).
        "Only through",
        "Second network",
        "Connect through a proxy",
    ] {
        h.reveal(name);
    }
    h.click("Cancel");
}

/// A proxy that cannot be one, found by OK on another tab, is shown where it is said:
/// Advanced opens at its proxy section.
#[test]
fn a_proxy_that_is_wrong_opens_advanced_at_the_proxy_section() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    own_proxy(&mut h);
    // Another tab, as a person leaves the proxy for another page and presses OK.
    tab(&mut h, "Downloads");
    h.click_as_person("OK", 0);
    h.until("Advanced at its proxy section", |h| {
        h.dialog(|o: &mut Options, _, _| o.tab == dm_desktop_base::options::Tab::Advanced)
            && h.shows("Connect through a proxy")
    });
    assert!(h.dialog_open("Options"), "OK took a proxy with no host");
    assert!(h.shows_part("Enter the proxy's host"));
    h.click("Cancel");
}

/// OK keeps every setting the Advanced page holds, as the two tabs did.
#[test]
fn ok_keeps_every_setting_of_the_advanced_page() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    h.fill("Speed limit, KB/s (0 for none)", "321");
    h.fill("Attempts per connection (0 to keep trying)", "7");
    h.fill("Wait between attempts (seconds, 0 for automatic)", "9");
    h.fill("Time out a silent connection after (seconds)", "45");
    own_proxy(&mut h);
    h.fill("Host", "proxy.example");
    h.fill("Port", "3128");
    h.fill("Username, if it needs one", "someone");
    ok(&mut h);
    let s = kept(&mut h).advanced;
    assert_eq!(
        (s.speed_limit, s.attempts, s.retry_wait, s.read_timeout),
        (321, 7, 9, 45)
    );
    assert!(s.proxy.enabled);
    assert_eq!(
        (
            s.proxy.host.as_str(),
            s.proxy.port,
            s.proxy.username.as_str()
        ),
        ("proxy.example", 3128, "someone")
    );
    let saved = saved_settings();
    assert_eq!(saved["speed_limit"], 321);
    assert_eq!(saved["proxy"]["host"], "proxy.example");
}

/// Advanced's Dial Up / VPN section (Windows), reference A's last tab in its words and order:
/// off as a new dm starts, the sign-in and the redials once a connection is chosen, and OK
/// keeping them (gaps row 93, G84). Saved with the switch off: on, the settings in force would
/// hold every download in this program for the connection.
#[cfg(windows)]
#[test]
fn advanced_has_the_dial_up_section_of_reference_a_on_windows() {
    let _file = settings_file();
    let world = world();
    let mut h = world.harness();
    open_options(&mut h);
    tab(&mut h, "Advanced");
    for name in [
        "Dial Up / VPN",
        "Use Windows Dial Up / VPN Networking",
        "User name",
        "Save password",
        "Redial attempts (zero if endlessly)",
        "Time between redial attempts (seconds)",
    ] {
        h.reveal(name);
    }
    // Its fields wait for the switch and a connection, as a person finds them: switched on, a
    // connection Windows knows chosen (as its pop-up gives it: this computer may know none),
    // filled in, and switched off again, which keeps what was typed.
    h.click("Use Windows Dial Up / VPN Networking");
    h.dialog(|o: &mut Options, _, _| o.draft.advanced.dialup.connection = "Office VPN".into());
    h.fill("User name", "me");
    h.fill("Redial attempts (zero if endlessly)", "3");
    h.fill("Time between redial attempts (seconds)", "10");
    h.click("Use Windows Dial Up / VPN Networking");
    ok(&mut h);
    let s = kept(&mut h);
    assert!(!s.advanced.dialup.enabled, "a new dm dials nothing");
    assert_eq!(
        (
            s.advanced.dialup.connection.as_str(),
            s.advanced.dialup.username.as_str(),
            s.advanced.dialup.attempts,
            s.advanced.dialup.wait_seconds
        ),
        ("Office VPN", "me", 3, 10)
    );
    let saved = saved_settings();
    assert_eq!(saved["dialup"]["connection"], "Office VPN");
    assert_eq!(saved["dialup"]["attempts"], 3);
}
