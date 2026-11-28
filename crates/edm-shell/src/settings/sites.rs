// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a site's downloads get: its own rule, and the sign-in Options keeps for it.

use edm_core::{AddOptions, Schedule};
use serde::{Deserialize, Serialize};

use super::Settings;

/// What a site's downloads get when they are added: a folder of their own,
/// a speed limit of their own, and a `User-Agent` and request headers of
/// their own. The host covers its subdomains, as a site's connection limit
/// does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteRule {
    /// The site, which covers its subdomains.
    pub host: String,
    /// Where its downloads are saved, instead of the folder their kind has;
    /// empty keeps the kind's.
    pub folder: String,
    /// KB/s for each of its downloads; zero for none.
    pub speed: u64,
    /// The `User-Agent` its downloads are asked with, in place of the one EDM
    /// picks; empty keeps EDM's.
    pub agent: String,
    /// Request headers its downloads carry, as typed: `Name: value`, each
    /// apart by `|` (`edm_core::cookies::headers_from_text`).
    pub headers: String,
}

impl SiteRule {
    /// `given` with this rule's `User-Agent` and headers after it, each name `given` has already
    /// left out.
    pub fn headers_onto(&self, mut given: Vec<(String, String)>) -> Vec<(String, String)> {
        let mut own = edm_core::cookies::headers_from_text(&self.headers);
        if !self.agent.trim().is_empty() {
            own.retain(|(name, _)| !name.eq_ignore_ascii_case("user-agent"));
            own.push(("User-Agent".to_string(), self.agent.trim().to_string()));
        }
        for (name, value) in own {
            if !given.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                given.push((name, value));
            }
        }
        given
    }
}

/// One entry in Options › Sign-ins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteLogin {
    /// The host, exactly: `example.com` does not cover `dl.example.com`.
    /// A leading dot (`.example.com`) also covers its subdomains.
    pub host: String,
    /// The user name sent to that host.
    pub username: String,
    /// The password sent with it, kept in this account's own file (0600).
    pub password: String,
}

impl Settings {
    /// `url` with a site login's user and password in it, where one applies
    /// and the address carries none of its own. HTTPS only (plain HTTP only
    /// to this computer), and only to the exact host unless the saved host
    /// begins with a dot.
    /// The rule of the site `url` is on, the most specific host that covers
    /// it.
    pub fn site_rule_for(&self, url: &str) -> Option<&SiteRule> {
        let host = edm_core::host_of(url);
        self.advanced
            .site_rules
            .iter()
            .filter(|rule| {
                let want = rule
                    .host
                    .trim()
                    .trim_start_matches('.')
                    .to_ascii_lowercase();
                !want.is_empty() && (host == want || host.ends_with(&format!(".{want}")))
            })
            .max_by_key(|rule| rule.host.trim().len())
    }

    /// The schedule that governs the queue called `name`: its own, or the
    /// one every queue shares.
    pub fn schedule_for(&self, name: &str) -> &Schedule {
        self.queue_schedules.get(name).unwrap_or(&self.schedule)
    }

    /// `with`, given its site's folder and speed limit where it was not given
    /// its own: what a person chose for this one download stands.
    pub fn with_site_rule(&self, url: &str, mut with: AddOptions) -> AddOptions {
        let Some(rule) = self.site_rule_for(url) else {
            return with;
        };
        if with.save_dir.as_deref().is_none_or(|d| d.trim().is_empty()) && !rule.folder.is_empty() {
            with.save_dir = Some(rule.folder.clone());
        }
        if with.speed_limit == 0 {
            with.speed_limit = rule.speed;
        }
        with.headers = rule.headers_onto(std::mem::take(&mut with.headers));
        with
    }

    /// `given` with `url`'s site rule's `User-Agent` and headers added, a header already given
    /// (by the browser, or the person for this one download) standing over the rule's.
    pub fn site_headers(&self, url: &str, given: &[(String, String)]) -> Vec<(String, String)> {
        match self.site_rule_for(url) {
            Some(rule) => rule.headers_onto(given.to_vec()),
            None => given.to_vec(),
        }
    }

    /// `url` with the login Options › Sign-ins keeps for its host, where it
    /// has none of its own; else as it is.
    pub fn with_login(&self, url: &str) -> String {
        let Ok(mut parsed) = url::Url::parse(url) else {
            return url.to_string();
        };
        if !parsed.username().is_empty() {
            return url.to_string();
        }
        let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
        let on_this_computer = host == "localhost" || host == "127.0.0.1" || host == "[::1]";
        match parsed.scheme() {
            "https" => {}
            "http" if on_this_computer => {}
            _ => return url.to_string(),
        }
        let found = self.site_logins.iter().find(|login| {
            // A host saved as a whole address names its host (one pasted in matched nothing, and the download got 401).
            let given = login.host.trim();
            let given = if given.contains("://") {
                url::Url::parse(given)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
                    .unwrap_or_default()
            } else {
                given.split(['/', ':']).next().unwrap_or("").to_string()
            };
            let with_subdomains = given.starts_with('.');
            let want = given.trim_start_matches('.').to_ascii_lowercase();
            !want.is_empty()
                && (host == want || (with_subdomains && host.ends_with(&format!(".{want}"))))
        });
        match found {
            Some(login)
                if parsed.set_username(&login.username).is_ok()
                    && parsed.set_password(Some(&login.password)).is_ok() =>
            {
                parsed.to_string()
            }
            _ => url.to_string(),
        }
    }
}
