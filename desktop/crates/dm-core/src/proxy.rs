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
    Http,
    Https,
    Socks4,
    Socks5,
    /// Whatever the computer is set to: the environment's `HTTP_PROXY`,
    /// `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`, and on macOS the system's
    /// network settings. No host or port of dm's own.
    System,
}

impl Kind {
    fn scheme(self) -> &'static str {
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
    pub enabled: bool,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub host: String,
    pub port: u16,
    /// Optional, and empty when not in use. Stored beside the host because
    /// that is where a proxy that needs them is configured.
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Sites the proxy is not used for: a host, a domain with its
    /// subdomains (`example.com`, or `.example.com`), an address, or `*`.
    /// Downloads from these go out directly. A list, not a text, because
    /// the engine's own FTP client reads it as well as reqwest.
    #[serde(default)]
    pub bypass: Vec<String>,
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
    /// that takes its proxy as one argument — yt-dlp does. Each part is
    /// percent-encoded, so a password holding `@` or `:` still names the
    /// right host, which is the reason `url` keeps them out.
    pub fn url_for_tools(&self) -> Option<String> {
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

impl Proxy {
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

/// The proxy the environment names, for a tool dm runs under system
/// settings: the tool reads the same variables itself, but a torrent's
/// swarm needs it as an address. The first of the usual names that is set.
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
    let system = proxy.enabled && proxy.kind == Kind::System;
    // Turned on with no host or port is a proxy asked for and not usable:
    // building a client without one sent every download out directly, and
    // a saved one like it went direct at every launch, unsaid.
    if proxy.enabled && !system && proxy.url().is_none() {
        return Err(if proxy.host.trim().is_empty() {
            "the proxy has no host".into()
        } else {
            "the proxy has no port".into()
        });
    }
    let mut builder = crate::builder_with(read_timeout, system);

    if let Some(url) = proxy.url() {
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
}
