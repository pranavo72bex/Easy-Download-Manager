// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[test]
fn the_mac_network_settings_are_read_for_a_proxy() {
    // This Mac's own answer with no proxy: nothing in force.
    let none =
        "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n  }\n  FTPPassive : 1\n}\n";
    assert_eq!(
        read_scutil(none),
        Settings {
            address: None,
            in_force: false
        }
    );
    let socks = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 8080\n  HTTPProxy : web.example\n  SOCKSEnable : 1\n  SOCKSPort : 1080\n  SOCKSProxy : 10.0.0.2\n}\n";
    assert_eq!(
        read_scutil(socks).address.as_deref(),
        Some("socks5h://10.0.0.2:1080")
    );
    let http = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 8080\n  HTTPProxy : web.example\n  SOCKSEnable : 0\n}\n";
    assert_eq!(
        read_scutil(http).address.as_deref(),
        Some("http://web.example:8080")
    );
    // A script alone: in force, and no address dm's own connections could use.
    let pac = "<dictionary> {\n  ProxyAutoConfigEnable : 1\n  ProxyAutoConfigURLString : http://wpad/wpad.dat\n}\n";
    assert_eq!(
        read_scutil(pac),
        Settings {
            address: None,
            in_force: true
        }
    );
    // Turned on with no host: not an address.
    let half = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 8080\n}\n";
    assert_eq!(
        read_scutil(half),
        Settings {
            address: None,
            in_force: false
        }
    );
}

use super::*;

#[test]
fn no_proxy_is_configured_by_default() {
    assert_eq!(Proxy::default().url(), None);
}

#[test]
fn a_proxy_that_is_off_is_not_used_even_when_filled_in() {
    // Turning it off is how people test whether it is the problem.
    let proxy = Proxy {
        enabled: false,
        host: "127.0.0.1".into(),
        port: 3128,
        ..Default::default()
    };
    assert_eq!(proxy.url(), None);
}

#[test]
fn an_enabled_proxy_with_no_host_is_not_a_proxy() {
    let proxy = Proxy {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(proxy.url(), None);
}

#[test]
fn a_tool_gets_the_credentials_inline_and_encoded() {
    // With `:`, `/` and `@` in it, each of which would end the login.
    let password = format!("{}/w", crate::test_secrets::marked("password"));
    let encoded = crate::test_secrets::escaped(&password).replace('/', "%2F");
    let proxy = Proxy {
        enabled: true,
        kind: Kind::Socks5,
        host: "h".into(),
        port: 1080,
        username: "me@work".into(),
        password: password.clone(),
        bypass: Vec::new(),
        per_scheme: Default::default(),
        script: String::new(),
    };
    assert_eq!(
        proxy.url_for_tools().as_deref(),
        Some(format!("socks5h://me%40work:{encoded}@h:1080").as_str())
    );
    // Without a user there is nothing to add, and no stray `:@`.
    let plain = Proxy {
        username: String::new(),
        ..proxy
    };
    assert_eq!(plain.url_for_tools().as_deref(), Some("socks5h://h:1080"));
}

#[test]
fn each_kind_gets_its_own_scheme() {
    for (kind, expected) in [
        (Kind::Http, "http://h:8080"),
        (Kind::Https, "https://h:8080"),
        (Kind::Socks5, "socks5h://h:8080"),
        (Kind::Socks4, "socks4://h:8080"),
    ] {
        let proxy = Proxy {
            enabled: true,
            kind,
            host: "h".into(),
            port: 8080,
            ..Default::default()
        };
        assert_eq!(proxy.url().as_deref(), Some(expected));
    }
}

#[test]
fn a_password_with_punctuation_in_it_does_not_change_the_host() {
    /* Credentials in the URL are how a password with `@` and `:` becomes a request to a
     * host called `word`. They go through reqwest's setter instead. */
    let proxy = Proxy {
        enabled: true,
        host: "proxy.internal".into(),
        port: 3128,
        username: "user".into(),
        password: crate::test_secrets::marked("password"),
        ..Default::default()
    };
    assert_eq!(proxy.url().as_deref(), Some("http://proxy.internal:3128"));
    assert!(client_with(&proxy).is_ok());
}

#[test]
fn a_client_is_still_built_when_no_proxy_is_set() {
    assert!(client_with(&Proxy::default()).is_ok());
}

#[test]
fn the_systems_proxy_names_no_address_of_dms_own_and_builds() {
    let system = Proxy {
        enabled: true,
        kind: Kind::System,
        host: "ignored".into(),
        port: 1,
        ..Default::default()
    };
    assert_eq!(system.url(), None);
    assert!(client_with(&system).is_ok());
}

#[test]
fn the_bypass_list_covers_names_their_subdomains_addresses_and_everything() {
    let proxy = Proxy {
        bypass: vec![
            "Example.com".into(),
            ".intranet".into(),
            "10.0.0.5".into(),
            "  ".into(),
        ],
        ..Default::default()
    };
    for direct in [
        "example.com",
        "dl.EXAMPLE.com",
        "files.intranet",
        "10.0.0.5",
    ] {
        assert!(proxy.bypasses(direct), "{direct}");
    }
    for proxied in ["notexample.com", "intranet.example.org", "10.0.0.50", ""] {
        assert!(!proxy.bypasses(proxied), "{proxied}");
    }
    let all = Proxy {
        bypass: vec!["*".into()],
        ..Default::default()
    };
    assert!(all.bypasses("anything.example"));
    assert!(!Proxy::default().bypasses("anything.example"));
}

#[test]
fn a_proxy_turned_on_with_no_host_or_port_is_refused_not_gone_around() {
    let no_host = Proxy {
        enabled: true,
        ..Default::default()
    };
    assert!(client_with(&no_host).is_err());
    let no_port = Proxy {
        enabled: true,
        host: "proxy.example".into(),
        port: 0,
        ..Default::default()
    };
    assert!(client_with(&no_port).is_err());
    // Off, or the computer's own settings, needs neither.
    assert!(client_with(&Proxy::default()).is_ok());
    let system = Proxy {
        enabled: true,
        kind: Kind::System,
        ..Default::default()
    };
    assert!(client_with(&system).is_ok());
}

#[test]
fn an_unusable_proxy_is_reported_rather_than_ignored() {
    // Downloading directly when a proxy was asked for is the outcome
    // someone would most want to hear about.
    let proxy = Proxy {
        enabled: true,
        host: "not a host name".into(),
        port: 8080,
        ..Default::default()
    };
    assert!(client_with(&proxy).is_err());
}

#[test]
fn a_proxy_per_scheme_is_used_for_its_scheme_and_the_main_one_for_the_rest() {
    let mut proxy = Proxy {
        enabled: true,
        host: "main.example".into(),
        port: 3128,
        ..Proxy::default()
    };
    proxy.per_scheme.insert(
        "https".into(),
        SchemeProxy {
            kind: Kind::Socks5,
            host: "secure.example".into(),
            port: 1080,
            username: "u@x".into(),
            password: "p:w".into(),
        },
    );
    // The client is built (the closure is only run at connect time, so also test its parts).
    assert!(client_with(&proxy).is_ok());
    let entry = proxy
        .per_scheme
        .get("https")
        .and_then(SchemeProxy::url)
        .expect("usable");
    assert_eq!(entry.scheme(), "socks5h");
    assert_eq!(entry.host_str(), Some("secure.example"));
    assert_eq!(entry.port(), Some(1080));
    assert_eq!(entry.username(), "u%40x");
    assert_eq!(proxy.url().as_deref(), Some("http://main.example:3128"));
}

#[test]
fn a_scheme_proxy_with_no_host_is_refused_not_gone_around() {
    let mut proxy = Proxy {
        enabled: true,
        ..Proxy::default()
    };
    proxy.per_scheme.insert(
        "http".into(),
        SchemeProxy {
            kind: Kind::Http,
            host: String::new(),
            port: 8080,
            username: String::new(),
            password: String::new(),
        },
    );
    let why = client_with(&proxy).expect_err("refused");
    assert!(why.contains("http proxy"), "{why}");
}
