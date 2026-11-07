// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Who dm says it is to a server, and what each server was found to refuse.

use crate::lock;

/// What dm says it is. Wikimedia, as its policy says, refuses an agent with
/// no way to reach whoever runs it: a bare `dm/0.1.0` got 403 for every
/// Wikipedia image, and the same agent with the project's address got the
/// file.
pub const USER_AGENT: &str = concat!(
    "dm/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/pranavo72bex/Easy-Download-Manager)"
);

/// dm's name alone. Some sites refuse an agent that names a code host as a
/// bot's (W3Schools refused anything with "github" in it, with 403), and
/// Wikimedia refuses this one; so a site that refuses [`USER_AGENT`] is
/// asked again with this, once, and if it answers it is asked with this
/// from then on.
pub const PLAIN_AGENT: &str = concat!("dm/", env!("CARGO_PKG_VERSION"));

/// The last agent asked with: a browser's, naming dm. A site that refuses
/// both of dm's own (W3Schools did on one network, though a browser there
/// played the same video) is asked once more as browsers ask.
pub const BROWSER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0 dm/",
    env!("CARGO_PKG_VERSION")
);

pub(crate) fn plain_hosts(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, &'static str>> {
    static HOSTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, &'static str>>,
    > = std::sync::OnceLock::new();
    HOSTS.get_or_init(Default::default)
}

pub(crate) fn bare_hosts() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static HOSTS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    HOSTS.get_or_init(Default::default)
}

/// From now on, `url`'s server is asked with no `Referer` of dm's own
/// making: it refused one (thinkbroadband answers 403 to any `Referer` but
/// its own pages) and served the same request without it.
pub fn drop_made_up_referer(url: &str) {
    if let Some(host) = server_of(url) {
        lock(bare_hosts()).insert(host);
    }
}

/// Undoes [`drop_made_up_referer`]: the server refused without it too, so
/// the `Referer` was not why.
pub fn forget_dropped_referer(url: &str) {
    if let Some(host) = server_of(url) {
        lock(bare_hosts()).remove(&host);
    }
}

/// Whether `url`'s server refused the `Referer` dm makes up.
pub fn made_up_referer_refused(url: &str) -> bool {
    server_of(url).is_some_and(|host| lock(bare_hosts()).contains(&host))
}

/// The server a URL names, host and port: two servers on one host are two
/// servers, each with its own answer about which agent it takes.
pub(crate) fn server_of(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// The agent a download goes out with first: Internet Explorer 11's, as
/// reference A sends it (maintainer, 2026-11-01), so a site that takes only
/// browsers answers the first request. A site that refuses it is asked again
/// with dm's own agents ([`FALLBACK_AGENTS`]), which the sites that refuse
/// browsers' (Wikimedia asks for a contact address) take.
pub const IE_AGENT: &str = "Mozilla/5.0 (Windows NT 6.1; Trident/7.0; rv:11.0) like Gecko";

/// The agent a request to `url` goes out with: the one its site last
/// answered, else [`first_agent`].
pub(crate) static IDENTIFY_AS_DM: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether dm sends its own name first, not the browser-like one: Options ›
/// Advanced › Identify as dm only. A site that refused the first name is still
/// asked again with the others (`prefer_agent`).
pub fn set_identify_as_dm(only: bool) {
    IDENTIFY_AS_DM.store(only, std::sync::atomic::Ordering::Relaxed);
}

/// The agent a request goes out with before any site has refused it:
/// [`IE_AGENT`], or dm's own name where the person asked for that.
pub fn first_agent() -> &'static str {
    if IDENTIFY_AS_DM.load(std::sync::atomic::Ordering::Relaxed) {
        USER_AGENT
    } else {
        IE_AGENT
    }
}

/// The identity (`User-Agent`) dm presents to `url`'s host.
#[must_use]
pub fn agent_for(url: &str) -> &'static str {
    server_of(url)
        .and_then(|host| lock(plain_hosts()).get(&host).copied())
        .unwrap_or_else(first_agent)
}

/// The agents a refused probe tries, in order, after [`IE_AGENT`].
pub const FALLBACK_AGENTS: [&str; 3] = [USER_AGENT, PLAIN_AGENT, BROWSER_AGENT];

/// From now on, `url`'s site is asked with `agent`.
pub fn prefer_agent(url: &str, agent: &'static str) {
    if let Some(host) = server_of(url) {
        lock(plain_hosts()).insert(host, agent);
    }
}

/// `url`'s server answered "429 Too many requests" to a request in the
/// browser's name: from then on it is asked by dm's own. Wikimedia's media
/// servers (2026-11-07) limit browsers' names from a program, now on the
/// first request and now only on the ranges after it, and serve dm's name;
/// held to the browser's, the ranges were refused for as long as dm tried. A
/// server that limits every name limits dm's too and is waited out as before;
/// one where the person chose dm's name first (Identify as dm only) is never
/// moved to another by a limit.
pub(crate) fn limited(url: &str) {
    if agent_for(url) == IE_AGENT {
        prefer_agent(url, USER_AGENT);
    }
}
