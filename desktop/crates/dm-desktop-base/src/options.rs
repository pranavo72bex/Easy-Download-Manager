// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' rules, for every window: its six pages (B43) and the settings
//! each puts back with Restore defaults, the lists a person types as words,
//! what OK keeps (only what Options changed, over what is in force now) and
//! what it refuses, with the page to show the reason on. The pages' fields
//! and their layout are each window's.

use dm_shell::settings::Settings;

use crate::i18n::{t, tf};

/// The pages of Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    /// The General page.
    General,
    /// The File types page.
    FileTypes,
    /// The Save to page.
    SaveTo,
    /// The Downloads page.
    Downloads,
    /// The expert settings, in one page: the connection's and the proxy's.
    Advanced,
    /// The Sign-ins page.
    SiteLogins,
}

/// Every page of Options, with the name it shows.
pub const TABS: [(Tab, &str); 6] = [
    (Tab::General, "General"),
    (Tab::FileTypes, "Browser"),
    (Tab::SaveTo, "Folders"),
    (Tab::Downloads, "Downloads"),
    (Tab::Advanced, "Advanced"),
    (Tab::SiteLogins, "Sign-ins"),
];

/// Options › General's looks: the setting's word, and what the choice says.
pub const THEMES: [(&str, &str); 4] = [
    ("system", "As the computer is"),
    ("light", "Light"),
    ("dark", "Dark"),
    ("classic", "Classic"),
];

/// Options › Downloads' connection presets (B56): the words, and the count.
pub const CONNECTION_PRESETS: [(&str, u32); 4] = [
    ("Slow (2)", 2),
    ("Normal (8)", 8),
    ("Fast (16)", 16),
    ("Maximum (32)", 32),
];

/// The events a sound file can be chosen for, by the settings' key.
pub const SOUND_EVENTS: [&str; 4] = ["complete", "failed", "queue_started", "queue_done"];

/// What an event is called beside its sound.
pub fn sound_words(event: &str) -> &'static str {
    match event {
        "complete" => t("Download complete"),
        "failed" => t("Download failed"),
        "queue_started" => t("Queue started"),
        _ => t("Queue finished"),
    }
}

/// The settings each tab of Options edits, by the names they are saved under:
/// what its Restore defaults puts back. Sign-ins hold a person's own logins,
/// which are not a default to go back to.
pub fn tab_settings(tab: Tab) -> &'static [&'static str] {
    match tab {
        Tab::General => &[
            "language",
            "theme",
            "start_on_login",
            "allow_command_line",
            "allow_mcp",
            "check_updates",
            "sound_on_complete",
            "sound_on_failure",
            "mica",
            "show_progress",
            "show_complete",
            "watch_clipboard",
            "ask_panel_downloads",
            "ask_caught_downloads",
            "offer_new_link",
            "server_dates",
            "double_click",
            "on_close",
        ],
        Tab::FileTypes => &["catch"],
        Tab::SaveTo => &[
            "categorize",
            "category_folders",
            "download_dir",
            "parts_dir",
        ],
        Tab::Downloads => &[
            "concurrency",
            "connections",
            "fetch_tools",
            "on_complete",
            "queue_limits",
            "queue_starts",
            "recheck_hours",
            "record_minutes",
            "remux_ts",
            "start_queue_on_launch",
            "stream_quality",
            "subtitles",
            "watch_folder",
        ],
        Tab::Advanced => &[
            "attempts",
            "cookies_file",
            "dialup",
            "host_limits",
            "proxy",
            "read_timeout",
            "retry_wait",
            "site_rules",
            "speed_limit",
        ],
        Tab::SiteLogins => &[],
    }
}

/// The settings in force now, with only the settings Options changed taken
/// from its draft: each top-level setting the draft holds differently from
/// when the dialog opened. Whatever else changed meanwhile stands.
pub fn changed_only(
    opened: &Settings,
    draft: &Settings,
    now: &Settings,
) -> Result<Settings, String> {
    let object = |s: &Settings| match serde_json::to_value(s) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        _ => Err("the settings could not be read to save them".to_string()),
    };
    let (opened, draft, mut merged) = (object(opened)?, object(draft)?, object(now)?);
    for (key, value) in draft {
        if opened.get(&key) != Some(&value) {
            merged.insert(key, value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(merged))
        .map_err(|e| format!("the settings could not be saved: {e}"))
}

/// The words of a list as typed: split on spaces, commas and semicolons
/// ("xlsx;" was kept as a type), each without a leading dot, in lower case.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(|w| w.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Sites as the rules match them, by host name: an address pasted whole,
/// scheme and path and all, was kept as it was and matched nothing.
/// A bare name, or one with a leading dot, is kept.
pub fn sites(words: &[String]) -> Vec<String> {
    words
        .iter()
        .map(|w| {
            let host = if w.contains("://") {
                url::Url::parse(w)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
            } else {
                // "example.com/path" or "example.com:8080": the name alone.
                w.split(['/', ':']).next().map(str::to_string)
            };
            host.filter(|h| !h.is_empty())
                .unwrap_or_else(|| w.clone())
                .to_ascii_lowercase()
        })
        .collect()
}

/// The lists Options shows as words typed: the file types taken, the sites
/// never taken from, the sites whose links work once, the proxy's bypass
/// list, and a person's own categories (name, file types, sites).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Typed {
    /// File types, separated by spaces.
    pub types: String,
    /// The hosts to skip.
    pub skip: String,
    /// Sites whose links work once.
    pub once: String,
    /// The proxy's bypass list.
    pub bypass: String,
    /// A person's own categories: name, file types, sites.
    pub custom: Vec<[String; 3]>,
}

impl Typed {
    /// The lists of `settings`, as Options shows them.
    pub fn of(settings: &Settings) -> Typed {
        Typed {
            types: settings.catch.extensions.join(" "),
            skip: settings.catch.skip_hosts.join(" "),
            once: settings.catch.once_hosts.join(" "),
            bypass: settings.proxy.bypass.join(" "),
            custom: settings
                .custom_categories
                .iter()
                .map(|c| [c.name.clone(), c.extensions.join(" "), c.sites.join(" ")])
                .collect(),
        }
    }
}

/// Restore defaults on `tab`: the draft with that page's settings put back
/// to what a new install has. OK keeps them, Cancel leaves everything as it
/// was; the words typed for the lists are read again from what they hold.
pub fn restore_defaults(tab: Tab, draft: &Settings) -> Settings {
    let (Ok(serde_json::Value::Object(mut now)), Ok(serde_json::Value::Object(fresh))) = (
        serde_json::to_value(draft),
        serde_json::to_value(Settings::default()),
    ) else {
        return draft.clone();
    };
    for key in tab_settings(tab) {
        if let Some(value) = fresh.get(*key) {
            now.insert((*key).to_string(), value.clone());
        }
    }
    serde_json::from_value(serde_json::Value::Object(now)).unwrap_or_else(|_| draft.clone())
}

/// Why OK kept nothing, and the page to say it on.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    /// What is wrong, in the language in force.
    pub says: String,
    /// The page that holds what is wrong; `None` where it is on no page
    /// (the settings could not be written), and the page stays as it is.
    pub tab: Option<Tab>,
    /// Advanced is to open at its proxy section.
    pub to_proxy: bool,
}

/// OK: the draft with the lists typed read into it, held to the rules, and
/// only what Options changed taken over what is in force `now`. Refused
/// where a proxy is on with nowhere to connect to, or a category made or
/// renamed here cannot be a folder on every computer.
pub fn ok(
    opened: &Settings,
    draft: &Settings,
    typed: &Typed,
    now: &Settings,
) -> Result<Settings, Refused> {
    let mut wanted = draft.clone();
    wanted.catch.extensions = words(&typed.types);
    wanted.catch.skip_hosts = sites(&words(&typed.skip));
    wanted.catch.once_hosts = sites(&words(&typed.once));
    wanted.proxy.bypass = words(&typed.bypass);
    // A sign-in's host as the rules read it: an address pasted whole
    // matched nothing.
    for login in &mut wanted.site_logins {
        if let Some(host) = sites(&[login.host.trim().to_string()]).pop() {
            login.host = host;
        }
    }
    wanted.custom_categories = typed
        .custom
        .iter()
        .filter(|[name, ..]| !name.trim().is_empty())
        .map(|[name, types, only]| dm_core::Custom {
            name: name.trim().to_string(),
            extensions: words(types),
            sites: sites(&words(only)),
        })
        .collect();
    // A proxy turned on with nowhere to connect to was taken, and every
    // download then went out directly without a word.
    if let Some(says) = proxy_refused(&wanted.proxy) {
        return Err(Refused {
            says,
            tab: Some(Tab::Advanced),
            to_proxy: true,
        });
    }
    // A category's name is its folder: one that is not one folder would be
    // saved and then sort nothing, without a word. Only those this edit made
    // or renamed: one an older dm saved is left out as the settings load,
    // and must not fail every OK after. Held to every computer's rules,
    // since settings travel. The built-in kinds' folders likewise, where
    // this edit renamed one; empty leaves that kind unsorted.
    let new_custom = wanted
        .custom_categories
        .iter()
        .map(|c| c.name.clone())
        .filter(|name| !opened.custom_categories.iter().any(|o| o.name == *name));
    let renamed_kinds = wanted
        .category_folders
        .iter()
        .enumerate()
        .filter(|(i, name)| {
            !name.trim().is_empty()
                && opened.category_folders.get(*i).map(|o| o.trim()) != Some(name.trim())
        })
        .map(|(_, name)| name.trim().to_string());
    if let Some(bad) = new_custom
        .chain(renamed_kinds)
        .find(|name| dm_core::portable_folder(name).is_err())
    {
        return Err(Refused {
            says: tf(
                "\"{}\" can't be a category, because its name is also its folder's. Leave out / \\ : * ? \" < > |, names like CON, and a dot or space at the end.",
                &[&bad],
            ),
            tab: Some(Tab::SaveTo),
            to_proxy: false,
        });
    }
    changed_only(opened, &wanted, now).map_err(|says| Refused {
        says,
        tab: None,
        to_proxy: false,
    })
}

/// Why a proxy that is on cannot be used as it is set, or `None`.
fn proxy_refused(p: &dm_core::Proxy) -> Option<String> {
    use dm_core::ProxyKind as Kind;
    if !p.enabled || p.kind == Kind::System {
        return None;
    }
    let host = p.host.trim();
    if host.is_empty() {
        Some(t("Enter the proxy's host, or untick \"Connect through a proxy\".").to_string())
    } else if host.contains("://")
        || host
            .chars()
            .any(|c| c.is_whitespace() || "/@?#".contains(c))
    {
        Some(tf(
            "\"{}\" isn't a valid proxy host. Enter a name like proxy.example.com; the port goes in its own field.",
            &[&host],
        ))
    } else if p.port == 0 {
        Some(t("A proxy needs a port from 1 to 65535.").to_string())
    } else {
        None
    }
}

/// A proxy kind's port where the port was the last kind's usual one: SOCKS5
/// kept 8080. A port typed in stays.
pub fn port_for_kind(was: dm_core::ProxyKind, now: dm_core::ProxyKind, port: u16) -> u16 {
    use dm_core::ProxyKind as Kind;
    let usual = |k: Kind| match k {
        Kind::Socks4 | Kind::Socks5 => Some(1080),
        Kind::Http | Kind::Https => Some(8080),
        Kind::System => None,
    };
    if usual(was).is_some_and(|usual| usual == port) || port == 0 {
        usual(now).unwrap_or(port)
    } else {
        port
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_writes_only_what_it_changed() {
        let opened = Settings::default();
        // Options changed the connections.
        let mut draft = opened.clone();
        draft.connections = 4;
        // Meanwhile View › Language and the Scheduler changed others.
        let mut now = opened.clone();
        now.language = "de".into();
        now.schedule.enabled = true;
        let wanted = changed_only(&opened, &draft, &now).unwrap();
        assert_eq!(wanted.connections, 4, "Options' own change");
        assert_eq!(wanted.language, "de", "the language picked meanwhile");
        assert!(wanted.schedule.enabled, "the schedule set meanwhile");
    }

    #[test]
    fn the_lists_typed_are_read_as_words_and_sites() {
        assert_eq!(words(" .ZIP; pdf,iso "), ["zip", "pdf", "iso"]);
        assert_eq!(
            sites(&[
                "https://Example.com/a".into(),
                "b.org:8080".into(),
                ".c.net".into()
            ]),
            ["example.com", "b.org", ".c.net"]
        );
        let opened = Settings::default();
        let typed = Typed {
            types: "zip pdf".into(),
            skip: "https://skip.example/page".into(),
            ..Typed::of(&opened)
        };
        let kept = ok(&opened, &opened, &typed, &opened).unwrap();
        assert_eq!(kept.catch.extensions, ["zip", "pdf"]);
        assert_eq!(kept.catch.skip_hosts, ["skip.example"]);
    }

    #[test]
    fn a_proxy_with_nowhere_to_go_is_refused_on_advanced() {
        let opened = Settings::default();
        let mut draft = opened.clone();
        draft.proxy.enabled = true;
        draft.proxy.kind = dm_core::ProxyKind::Http;
        draft.proxy.host = String::new();
        let refused = ok(&opened, &draft, &Typed::of(&draft), &opened).unwrap_err();
        assert_eq!(refused.tab, Some(Tab::Advanced));
        assert!(refused.to_proxy);
        draft.proxy.host = "http://proxy.example".into();
        assert!(ok(&opened, &draft, &Typed::of(&draft), &opened).is_err());
        draft.proxy.host = "proxy.example".into();
        draft.proxy.port = 3128;
        assert!(ok(&opened, &draft, &Typed::of(&draft), &opened).is_ok());
    }

    #[test]
    fn a_category_that_cannot_be_a_folder_is_refused_on_folders() {
        let opened = Settings::default();
        let typed = Typed {
            custom: vec![["a/b".into(), "zip".into(), String::new()]],
            ..Typed::of(&opened)
        };
        let refused = ok(&opened, &opened, &typed, &opened).unwrap_err();
        assert_eq!(refused.tab, Some(Tab::SaveTo));
    }

    #[test]
    fn restore_defaults_puts_back_one_page_alone() {
        let draft = Settings {
            connections: 3,
            speed_limit: 50,
            ..Settings::default()
        };
        let restored = restore_defaults(Tab::Downloads, &draft);
        assert_eq!(restored.connections, Settings::default().connections);
        assert_eq!(restored.speed_limit, 50, "Advanced's, not Downloads'");
    }

    #[test]
    fn a_kinds_usual_port_follows_the_kind_and_a_typed_one_stays() {
        use dm_core::ProxyKind as Kind;
        assert_eq!(port_for_kind(Kind::Http, Kind::Socks5, 8080), 1080);
        assert_eq!(port_for_kind(Kind::Http, Kind::Socks5, 3128), 3128);
        assert_eq!(port_for_kind(Kind::System, Kind::Http, 0), 8080);
    }
}
