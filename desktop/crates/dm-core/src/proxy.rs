// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Sending downloads through a proxy.
//!
//! Kept as settings plus one constructor rather than threaded through the
//! download path, because a proxy is a property of the client and every
//! request already goes through one of those.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
/// What kind of proxy, in the vocabulary the settings file uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A plain HTTP proxy.
    Http,
    /// An HTTP proxy reached over TLS.
    Https,
    /// A SOCKS4 proxy.
    Socks4,
    /// A SOCKS5 proxy.
    Socks5,
    /// Whatever the computer is set to: the environment's `HTTP_PROXY`,
    /// `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`, and on macOS the system's
    /// network settings. No host or port of dm's own.
    System,
}

impl Kind {
    pub(crate) fn scheme(self) -> &'static str {
        match self {
            Kind::Http => "http",
            Kind::Https => "https",
            Kind::Socks4 => "socks4",
            /* `socks5h`: the proxy resolves the name. Resolved here, a name
             * only the proxy's side knows fails, and the lookup itself goes
             * out around the proxy, telling the local resolver every site. */
            Kind::Socks5 => "socks5h",
            Kind::System => "",
        }
    }
}

/// Where downloads connect through, if anywhere.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Proxy {
    /// Whether the proxy is used.
    pub enabled: bool,
    /// What kind of proxy it is.
    #[serde(rename = "type")]
    pub kind: Kind,
    /// The proxy's host name or address.
    pub host: String,
    /// The proxy's port.
    pub port: u16,
    /// Optional, and empty when not in use. Stored beside the host because
    /// that is where a proxy that needs them is configured.
    #[serde(default)]
    pub username: String,
    /// The password for the proxy, empty if none.
    #[serde(default)]
    pub password: String,
    /// Sites the proxy is not used for: a host, a domain with its
    /// subdomains (`example.com`, or `.example.com`), an address, or `*`.
    /// Downloads from these go out directly. A list, not a text, because
    /// the engine's own FTP client reads it as well as reqwest.
    #[serde(default)]
    pub bypass: Vec<String>,
    /// A proxy of its own for downloads of one scheme (`http`, `https`), in place of the one above;
    /// a scheme with no entry here goes through the one above. Not for the system kind.
    #[serde(default)]
    pub per_scheme: std::collections::BTreeMap<String, SchemeProxy>,
    /// A proxy configuration script's address (a web address, a `file:` one, or a path), empty
    /// for none. Filled in, it decides the way to each site in place of the proxies above
    /// (`pac::way_to`); the sites on the bypass list still go directly.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub script: String,
}

/// The proxy for one scheme (`Proxy::per_scheme`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SchemeProxy {
    /// Its kind; the system kind is not one for a scheme.
    #[serde(default = "default_kind", rename = "type")]
    pub kind: Kind,
    /// Its host name or address.
    #[serde(default)]
    pub host: String,
    /// Its port.
    #[serde(default)]
    pub port: u16,
    /// Its login, empty if none.
    #[serde(default)]
    pub username: String,
    /// Its password, empty if none.
    #[serde(default)]
    pub password: String,
}

fn default_kind() -> Kind {
    Kind::Http
}

impl SchemeProxy {
    /// Its address with the login inside it (percent-encoded by the URL type), or `None` when it
    /// names no usable proxy.
    fn url(&self) -> Option<url::Url> {
        if self.kind == Kind::System || self.host.trim().is_empty() || self.port == 0 {
            return None;
        }
        let mut url = url::Url::parse(&format!(
            "{}://{}:{}",
            self.kind.scheme(),
            self.host.trim(),
            self.port
        ))
        .ok()?;
        if !self.username.is_empty() {
            url.set_username(&self.username).ok()?;
            url.set_password(Some(&self.password)).ok()?;
        }
        Some(url)
    }
}

impl Default for Proxy {
    fn default() -> Self {
        Proxy {
            enabled: false,
            kind: Kind::Http,
            host: String::new(),
            port: 8080,
            username: String::new(),
            password: String::new(),
            bypass: Vec::new(),
            per_scheme: Default::default(),
            script: String::new(),
        }
    }
}

impl Proxy {
    /// The URL reqwest wants, or `None` when there is no usable proxy here.
    ///
    /// Credentials go through reqwest's own setter rather than into the URL,
    /// so a password containing `@` or `:` does not silently produce a
    /// different host.
    pub fn url(&self) -> Option<String> {
        if !self.enabled
            || self.kind == Kind::System
            || self.host.trim().is_empty()
            || self.port == 0
        {
            return None;
        }
        Some(format!(
            "{}://{}:{}",
            self.kind.scheme(),
            self.host.trim(),
            self.port
        ))
    }
}

impl Proxy {
    /// The same address, with the credentials inline, for a program dm runs
    /// that takes its proxy as one argument. Each part is
    /// percent-encoded, so a password holding `@` or `:` still names the
    /// right host, which is the reason `url` keeps them out.
    pub fn url_for_tools(&self) -> Option<String> {
        if let Some(script) = self.script() {
            return Some(format!("{SCRIPT_MARK}{script}"));
        }
        if self.enabled && self.kind == Kind::System {
            return system_proxy_url();
        }
        let url = self.url()?;
        if self.username.is_empty() {
            return Some(url);
        }
        let (scheme, rest) = url.split_once("://")?;
        let encode = |s: &str| {
            percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
        };
        Some(format!(
            "{scheme}://{}:{}@{rest}",
            encode(&self.username),
            encode(&self.password)
        ))
    }
}

/// What a tool's proxy address starts with when a configuration script chooses it: the script's
/// address follows, and [`route_for`] asks it about each site.
pub const SCRIPT_MARK: &str = "script:";

/// The proxy a tool's request to `url` goes through, given the address `url_for_tools` made:
/// that address, or what the script it names says for the site. A site the script gives no way
/// to is sent where nothing answers ([`crate::pac::NO_WAY`]), so it fails rather than going out
/// directly.
pub fn route_for(proxy: Option<&str>, url: &str) -> Option<String> {
    let proxy = proxy?;
    let Some(script) = proxy.strip_prefix(SCRIPT_MARK) else {
        return Some(proxy.to_string());
    };
    let Ok(target) = url::Url::parse(url) else {
        return Some(crate::pac::NO_WAY.into());
    };
    match crate::pac::way_to(script, &target) {
        Ok(way) => way.map(|u| u.to_string().trim_end_matches('/').to_string()),
        Err(_) => Some(crate::pac::NO_WAY.into()),
    }
}

impl Proxy {
    /// The configuration script's address, where the proxy is on and one is named.
    pub fn script(&self) -> Option<&str> {
        let script = self.script.trim();
        (self.enabled && !script.is_empty()).then_some(script)
    }

    /// Whether downloads from `host` go out directly, past the proxy.
    ///
    /// The same rule reqwest applies to its `NO_PROXY`: a name covers
    /// itself and its subdomains, a leading dot is the same thing, `*` is
    /// everything, and an address matches itself.
    pub fn bypasses(&self, host: &str) -> bool {
        let host = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
        self.bypass.iter().any(|entry| {
            let entry = entry.trim().trim_start_matches('.').to_ascii_lowercase();
            !entry.is_empty()
                && (entry == "*" || host == entry || host.ends_with(&format!(".{entry}")))
        })
    }

    /// The bypass list as reqwest reads it.
    fn no_proxy(&self) -> Option<reqwest::NoProxy> {
        let list = self
            .bypass
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        if list.is_empty() {
            None
        } else {
            reqwest::NoProxy::from_string(&list)
        }
    }
}

/// The proxy the computer names, for a tool dm runs under system settings
/// and for dm's own FTP and torrent connections, which need it as an
/// address: the first of the usual environment variables that is set, then,
/// on macOS, the network settings' SOCKS, HTTPS or HTTP proxy. `None` when
/// the computer names none.
fn system_proxy_url() -> Option<String> {
    [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]
    .iter()
    .find_map(|name| std::env::var(name).ok())
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
    .or_else(|| system_settings().and_then(|s| s.address))
}

/// Whether the computer has a proxy in force at all: an environment
/// variable, or (macOS) a proxy, a configuration script or discovery turned
/// on in the network settings. With none, dm's own connections go direct,
/// as the computer's own programs do.
pub fn system_proxy_in_force() -> bool {
    system_proxy_url().is_some() || system_settings().is_some_and(|s| s.in_force)
}

/// What the computer's network settings say of proxies.
#[derive(Debug, PartialEq, Eq)]
struct Settings {
    /// A SOCKS, HTTPS or HTTP proxy's address, in that order of choice.
    address: Option<String>,
    /// Any proxy kind, script or discovery turned on.
    in_force: bool,
}

#[cfg(target_os = "macos")]
fn system_settings() -> Option<Settings> {
    let out = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| read_scutil(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(not(target_os = "macos"))]
fn system_settings() -> Option<Settings> {
    None
}

/// `scutil --proxy`'s dictionary, read for its proxies.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn read_scutil(text: &str) -> Settings {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let (k, v) = line.trim().split_once(" : ")?;
            (k == key).then(|| v.trim().to_string())
        })
    };
    let on = |key: &str| value(key).as_deref() == Some("1");
    let address = [("SOCKS", "socks5h"), ("HTTPS", "http"), ("HTTP", "http")]
        .iter()
        .find_map(|(kind, scheme)| {
            if !on(&format!("{kind}Enable")) {
                return None;
            }
            let host = value(&format!("{kind}Proxy")).filter(|h| !h.is_empty())?;
            let port = value(&format!("{kind}Port"))?.parse::<u16>().ok()?;
            Some(format!("{scheme}://{host}:{port}"))
        });
    let in_force = address.is_some()
        || [
            "FTPEnable",
            "RTSPEnable",
            "ProxyAutoConfigEnable",
            "ProxyAutoDiscoveryEnable",
        ]
        .iter()
        .any(|k| on(k));
    Settings { address, in_force }
}

/// A client that honours these settings.
///
/// A proxy that cannot be parsed is reported rather than ignored: downloading
/// directly when someone asked for a proxy is the one outcome they would
/// most want to be told about, and the usual reason to set one is that the
/// direct route is not acceptable.
pub fn client_with(proxy: &Proxy) -> Result<reqwest::Client, String> {
    client_with_timeout(proxy, crate::READ_TIMEOUT)
}

/// [`client_with`], giving up on a silent connection after `read_timeout`.
pub fn client_with_timeout(
    proxy: &Proxy,
    read_timeout: std::time::Duration,
) -> Result<reqwest::Client, String> {
    /* The system's settings — the environment's variables and, on macOS,
     * the network settings — are read when the client is built, and only
     * for the kind that asks for them; off, or a proxy of dm's own, goes
     * out as told and not as the environment says. */
    let script = proxy.script().map(str::to_string);
    let system = proxy.enabled && proxy.kind == Kind::System && script.is_none();
    // Turned on with no host or port is a proxy asked for and not usable:
    // building a client without one sent every download out directly, and
    // a saved one like it went direct at every launch, unsaid.
    if proxy.enabled
        && !system
        && script.is_none()
        && proxy.per_scheme.is_empty()
        && proxy.url().is_none()
    {
        return Err(if proxy.host.trim().is_empty() {
            "the proxy has no host".into()
        } else {
            "the proxy has no port".into()
        });
    }
    let mut builder = crate::builder_with(read_timeout, system);

    if let Some(script) = script {
        // The script decides each site's way, asked the first time a request goes there (a
        // redirect's new site too) and kept; settings saved again ask it afresh. A site it
        // gives no way to is sent where nothing answers, never directly.
        crate::pac::forget_all();
        let chosen = proxy.clone();
        builder = builder.proxy(reqwest::Proxy::custom(move |target| {
            if chosen.bypasses(target.host_str().unwrap_or_default()) {
                return None;
            }
            match crate::pac::way_to(&script, target) {
                Ok(way) => way.map(|mut u| {
                    if !chosen.username.is_empty() {
                        let _ = u.set_username(&chosen.username);
                        let _ = u.set_password(Some(&chosen.password));
                    }
                    u
                }),
                Err(_) => url::Url::parse(crate::pac::NO_WAY).ok(),
            }
        }));
    } else if proxy.enabled && !system && !proxy.per_scheme.is_empty() {
        // A scheme named here is refused when its entry is unusable, as the main proxy is: going
        // direct when a proxy was asked for is the outcome no one wants.
        for (scheme, entry) in &proxy.per_scheme {
            if entry.url().is_none() {
                return Err(format!("the {scheme} proxy has no usable host or port"));
            }
        }
        let chosen = proxy.clone();
        let main = proxy
            .url()
            .and_then(|u| url::Url::parse(&u).ok())
            .map(|mut u| {
                if !proxy.username.is_empty() {
                    let _ = u.set_username(&proxy.username);
                    let _ = u.set_password(Some(&proxy.password));
                }
                u
            });
        builder = builder.proxy(reqwest::Proxy::custom(move |target| {
            if chosen.bypasses(target.host_str().unwrap_or_default()) {
                return None;
            }
            chosen
                .per_scheme
                .get(target.scheme())
                .and_then(SchemeProxy::url)
                .or_else(|| main.clone())
        }));
    } else if let Some(url) = proxy.url() {
        let mut configured = reqwest::Proxy::all(&url)
            .map_err(|e| format!("that proxy address is not usable: {e}"))?;
        if !proxy.username.is_empty() {
            configured = configured.basic_auth(&proxy.username, &proxy.password);
        }
        configured = configured.no_proxy(proxy.no_proxy());
        builder = builder.proxy(configured);
    }

    builder
        .build()
        .map_err(|e| format!("a client could not be built: {e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_mac_network_settings_are_read_for_a_proxy() {
        // This Mac's own answer with no proxy: nothing in force.
        let none = "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n  }\n  FTPPassive : 1\n}\n";
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
}
