// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The rules the app serves to the extension in its hello, so that a fix for one site needs a new
//! app (or a new `rules.json` in dm's home) and not a new release in a browser store.
//!
//! The document is versioned twice: `format` says how it is written (a side that does not read the
//! format ignores the document and keeps its compiled tables), and `revision` says how new it is
//! (the app serves the newest of the one it ships and the one in its home folder). The extension
//! adds what it is served to what it was built with; nothing served can take a compiled rule away,
//! so a bad or old document costs nothing.

use serde::{Deserialize, Serialize};

/// The format of the rules document this side reads.
pub const RULES_FORMAT: u64 = 1;

/// The largest rules document read, in bytes: the hello travels in one native-messaging frame,
/// which a browser limits to 1 MiB, and this leaves the rest of the frame room.
pub const MAX_RULES_BYTES: usize = 256 * 1024;

/// The most entries kept in any one list of a document.
const MAX_ENTRIES: usize = 2000;
/// The most site profiles, answer rules and keys kept.
const MAX_PROFILES: usize = 500;
const MAX_ANSWERS: usize = 64;
const MAX_FINDERS: usize = 16;
const MAX_FINDER_TEXT: usize = 512;
const MAX_KEYS: usize = 64;
/// The largest floor for a Windows media stub a document may set: 10 MiB.
const MAX_MEDIA_FLOOR: u64 = 10 * 1024 * 1024;

/// A versioned set of rules: what is never caught, and what to read on which site.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Rules {
    /// How the document is written; [`RULES_FORMAT`] is the one this side reads.
    pub format: u64,
    /// How new the document is: the larger revision wins.
    pub revision: u64,
    /// Addresses and responses nobody came to download.
    pub exclude: ExcludeRules,
    /// Sites whose pages build their streams from the answers of their own requests.
    pub sites: Vec<SiteProfile>,
}

/// Names added to the extension's built-in list of what is never caught (the same classes; see the
/// extension's `excluded.rs`). Every entry is lower case.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct ExcludeRules {
    /// Advertising networks, by the end of their host name.
    pub advert_hosts: Vec<String>,
    /// Measuring and tracking services, by the end of their host name.
    pub tracker_hosts: Vec<String>,
    /// Whole path segments that mean an advert wherever they stand.
    pub advert_segments: Vec<String>,
    /// File names of tracking pixels.
    pub tracker_files: Vec<String>,
    /// Names a page's interface sounds go by.
    pub sound_names: Vec<String>,
    /// Folders a page keeps its interface sounds in.
    pub sound_folders: Vec<String>,
    /// Extensions of those sounds.
    pub sound_extensions: Vec<String>,
    /// Folders a page keeps its furniture in.
    pub furniture_folders: Vec<String>,
    /// Names of a page's furniture.
    pub furniture_names: Vec<String>,
    /// Words in a `Server` header that name an ad server.
    pub ad_server_names: Vec<String>,
    /// A Windows media file of this many bytes or fewer is an ad's stub, where the built-in floor
    /// is not the right one. Never lowers the built-in floor.
    pub windows_media_floor: Option<u64>,
}

/// What to read on one site: its host, and which of its requests answer with streams.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct SiteProfile {
    /// The site, such as `example.com`; it covers its sub-domains.
    pub host: String,
    /// The requests whose answers are read.
    pub answers: Vec<AnswerRule>,
    /// How to tell which video a player on a page is (a feed lists many).
    pub finders: Vec<Finder>,
    /// Private copies of `fetch` a site's script keeps, to be read like the global one, each
    /// `<global>|<property path>|<name>`: the object `global` holds, the path to the object that
    /// owns the function, and the function's name.
    pub private_fetches: Vec<String>,
    /// A CSS selector for the element the site keeps a video's title in, where its page's own
    /// title says less (a feed's "Home", a count in front): the download is named from that
    /// element's words, as reference A's site rules name a title element (gaps row 124).
    pub title: String,
}

/// How to find which video one player is, in a site's pages. The languages are small and read
/// only: a selector chain (see the extension's `document.js`) and a dotted property path, so a fix
/// for a site needs a new rules document and not a new release.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Finder {
    /// The selector chain that says a player element is one this finder reads.
    pub player: String,
    /// A dotted property path from the player element to its video's id. A leading `*` means
    /// "the first own property of this object for which the rest of the path exists".
    pub id_path: String,
    /// The player element's attribute that holds the id.
    pub id_attr: String,
    /// A regular expression whose first group is the id in the page's address.
    pub id_from_url: String,
    /// The selector chain to the element that holds a feed's players, watched for new ones.
    pub container: String,
}

/// One kind of request a site's player makes whose answer names a stream.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct AnswerRule {
    /// A request is read when its address contains this (compared in lower case).
    pub url_contains: String,
    /// The JSON keys whose strings are addresses (compared exactly); empty means every string of
    /// the answer.
    pub keys: Vec<String>,
}

impl Rules {
    /// The rules of a document, or why it is not one. Reads JSON only, nothing over
    /// [`MAX_RULES_BYTES`], and only the format this side reads; the lists are tidied (see
    /// [`Rules::tidied`]).
    pub fn parse(text: &str) -> Result<Rules, String> {
        if text.len() > MAX_RULES_BYTES {
            return Err(format!("more than {MAX_RULES_BYTES} bytes"));
        }
        // An object only: serde would read an array as the struct's fields in order,
        // and `[1, 2]` as a format and a revision.
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|why| format!("not a rules document: {why}"))?;
        if !value.is_object() {
            return Err("not a rules document: it is not a JSON object".into());
        }
        let rules: Rules =
            serde_json::from_value(value).map_err(|why| format!("not a rules document: {why}"))?;
        if rules.format != RULES_FORMAT {
            return Err(format!(
                "format {} is not one this side reads (it reads {RULES_FORMAT})",
                rules.format
            ));
        }
        Ok(rules.tidied())
    }

    /// The newer of the rules an app ships and the ones found beside it; the shipped ones when the
    /// found ones are not newer, or are not there.
    pub fn newest(shipped: Rules, found: Option<Rules>) -> Rules {
        match found {
            Some(found) if found.revision > shipped.revision => found,
            _ => shipped,
        }
    }

    /// The rules with every list trimmed, lower-cased where a name is compared in lower case,
    /// stripped of entries that cannot be what they claim to be (empty, with a space, a host with
    /// a slash) and cut to a sane length.
    pub fn tidied(mut self) -> Rules {
        let hosts = |list: &mut Vec<String>| {
            tidy(list, |entry| {
                !entry.contains(['/', ':', '?', '#', '@']) && !entry.starts_with('.')
            })
        };
        hosts(&mut self.exclude.advert_hosts);
        hosts(&mut self.exclude.tracker_hosts);
        let plain = |list: &mut Vec<String>| tidy(list, |entry| !entry.contains(['?', '#']));
        plain(&mut self.exclude.advert_segments);
        plain(&mut self.exclude.tracker_files);
        plain(&mut self.exclude.sound_names);
        plain(&mut self.exclude.sound_folders);
        plain(&mut self.exclude.sound_extensions);
        plain(&mut self.exclude.furniture_folders);
        plain(&mut self.exclude.furniture_names);
        plain(&mut self.exclude.ad_server_names);
        self.exclude.windows_media_floor = self
            .exclude
            .windows_media_floor
            .map(|floor| floor.min(MAX_MEDIA_FLOOR));
        self.sites.truncate(MAX_PROFILES);
        for site in &mut self.sites {
            site.host = site.host.trim().trim_matches('.').to_ascii_lowercase();
            site.answers.truncate(MAX_ANSWERS);
            for answer in &mut site.answers {
                answer.url_contains = answer.url_contains.trim().to_ascii_lowercase();
                answer.keys.retain(|key| !key.is_empty());
                answer.keys.truncate(MAX_KEYS);
            }
            site.answers
                .retain(|answer| !answer.url_contains.is_empty());
            site.finders.truncate(MAX_FINDERS);
            for finder in &mut site.finders {
                for text in [
                    &mut finder.player,
                    &mut finder.id_path,
                    &mut finder.id_attr,
                    &mut finder.id_from_url,
                    &mut finder.container,
                ] {
                    *text = text.trim().to_string();
                    if text.len() > MAX_FINDER_TEXT {
                        text.clear();
                    }
                }
            }
            site.finders.retain(|finder| {
                !finder.player.is_empty()
                    && (!finder.id_path.is_empty()
                        || !finder.id_attr.is_empty()
                        || !finder.id_from_url.is_empty())
            });
            site.private_fetches.truncate(MAX_FINDERS);
            site.private_fetches.retain(|path| {
                path.len() <= MAX_FINDER_TEXT
                    && path.split('|').count() == 3
                    && path.split('|').all(|part| !part.trim().is_empty())
            });
            // A title selector of a sane length, kept as written.
            site.title = site.title.trim().to_string();
            if site.title.len() > MAX_FINDER_TEXT {
                site.title.clear();
            }
        }
        self.sites.retain(|site| {
            !site.host.is_empty()
                && !site.host.contains(['/', ':', ' '])
                && (!site.answers.is_empty()
                    || !site.finders.is_empty()
                    || !site.private_fetches.is_empty()
                    || !site.title.is_empty())
        });
        self
    }

    /// The answer rules of the site a page on `host` belongs to: those of every profile whose host
    /// is `host` or one `host` is a sub-domain of.
    pub fn answers_for(&self, host: &str) -> Vec<&AnswerRule> {
        self.profiles_for(host)
            .flat_map(|site| site.answers.iter())
            .collect()
    }

    /// The finders of the site a page on `host` belongs to.
    pub fn finders_for(&self, host: &str) -> Vec<&Finder> {
        self.profiles_for(host)
            .flat_map(|site| site.finders.iter())
            .collect()
    }

    /// The selector for the element a page on `host` keeps its video's title in, where the
    /// rules name one.
    pub fn title_for(&self, host: &str) -> Option<&str> {
        self.profiles_for(host)
            .map(|site| site.title.trim())
            .find(|title| !title.is_empty())
    }

    /// The private `fetch` paths of the site a page on `host` belongs to.
    pub fn private_fetches_for(&self, host: &str) -> Vec<&String> {
        self.profiles_for(host)
            .flat_map(|site| site.private_fetches.iter())
            .collect()
    }

    fn profiles_for<'a>(&'a self, host: &str) -> impl Iterator<Item = &'a SiteProfile> {
        let host = host.trim_matches('.').to_ascii_lowercase();
        self.sites.iter().filter(move |site| {
            host == site.host
                || host
                    .strip_suffix(site.host.as_str())
                    .is_some_and(|before| before.ends_with('.'))
        })
    }
}

/// Lower-cases, trims, drops what is empty, has whitespace or fails `fits`, drops repeats and cuts
/// to [`MAX_ENTRIES`].
fn tidy(list: &mut Vec<String>, fits: impl Fn(&str) -> bool) {
    for entry in list.iter_mut() {
        *entry = entry.trim().to_ascii_lowercase();
    }
    list.retain(|entry| {
        !entry.is_empty() && !entry.contains(char::is_whitespace) && fits(entry.as_str())
    });
    let mut seen = std::collections::HashSet::new();
    list.retain(|entry| seen.insert(entry.clone()));
    list.truncate(MAX_ENTRIES);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(extra: &str) -> String {
        format!(r#"{{ "format": 1, "revision": 3, {extra} }}"#)
    }

    #[test]
    fn a_document_is_read_and_its_lists_tidied() {
        let text = document(
            r#""exclude": { "advert_hosts": [" Ads.Example.COM ", "", "a b", "bad/host", "ads.example.com", ".lead.dot"],
                             "windows_media_floor": 999999999999 },
               "sites": [ { "host": " Video.Example ", "answers": [
                    { "url_contains": " /API/Play ", "keys": ["hlsUrl", ""] },
                    { "url_contains": "   " } ] },
                  { "host": "", "answers": [ { "url_contains": "x" } ] },
                  { "host": "no.answers.example" } ]"#,
        );
        let rules = Rules::parse(&text).expect("a document");
        assert_eq!(rules.revision, 3);
        assert_eq!(rules.exclude.advert_hosts, vec!["ads.example.com"]);
        assert_eq!(rules.exclude.windows_media_floor, Some(MAX_MEDIA_FLOOR));
        assert_eq!(rules.sites.len(), 1);
        assert_eq!(rules.sites[0].host, "video.example");
        assert_eq!(rules.sites[0].answers.len(), 1);
        assert_eq!(rules.sites[0].answers[0].url_contains, "/api/play");
        assert_eq!(rules.sites[0].answers[0].keys, vec!["hlsUrl"]);
    }

    /// A site's rule can name the element its pages keep a video's title in (gaps row 124): a
    /// site with that alone is kept, found by its sub-domains, and an over-long one is dropped.
    #[test]
    fn a_title_selector_is_read_and_found_by_site() {
        let long = "x".repeat(MAX_FINDER_TEXT + 1);
        let rules = Rules::parse(&document(&format!(
            r#""sites": [ {{ "host": "videos.example", "title": " h1.video-title " }},
                          {{ "host": "long.example", "title": "{long}" }} ]"#
        )))
        .expect("a document");
        assert_eq!(
            rules.title_for("www.videos.example"),
            Some("h1.video-title")
        );
        assert_eq!(rules.title_for("other.example"), None);
        assert_eq!(rules.title_for("long.example"), None);
        assert_eq!(rules.sites.len(), 1, "a site with nothing left is dropped");
    }

    #[test]
    fn finders_and_private_fetches_are_read_tidied_and_found_by_site() {
        let rules = Rules::parse(&document(
            r#""sites": [ { "host": "feed.example", "finders": [
                    { "player": " div.vp ", "id_path": "*.return.props.videoID" },
                    { "player": "", "id_path": "x" },
                    { "player": "video" } ],
                  "private_fetches": ["w|a.b|fetch", "bad", "a||c"] } ]"#,
        ))
        .expect("a document");
        assert_eq!(rules.sites.len(), 1, "a site with finders only is kept");
        let finders = rules.finders_for("www.feed.example");
        assert_eq!(finders.len(), 1);
        assert_eq!(finders[0].player, "div.vp");
        assert_eq!(rules.private_fetches_for("feed.example"), ["w|a.b|fetch"]);
        assert!(rules.finders_for("other.example").is_empty());
    }

    #[test]
    fn a_document_that_is_not_one_is_refused() {
        assert!(Rules::parse("not json").is_err());
        assert!(Rules::parse("[1, 2]").is_err());
        assert!(Rules::parse("{}").is_err(), "no format");
        assert!(
            Rules::parse(r#"{ "format": 2, "revision": 9 }"#).is_err(),
            "a format this side does not read"
        );
        let huge = format!(
            r#"{{ "format": 1, "revision": 1, "exclude": {{ "advert_hosts": ["{}"] }} }}"#,
            "a".repeat(MAX_RULES_BYTES)
        );
        assert!(Rules::parse(&huge).is_err(), "over the size cap");
    }

    #[test]
    fn a_field_this_side_does_not_know_is_ignored() {
        let rules = Rules::parse(&document(r#""future": { "x": 1 }"#)).expect("a document");
        assert_eq!(rules.revision, 3);
        assert!(rules.sites.is_empty());
    }

    #[test]
    fn the_newer_revision_wins_and_a_tie_keeps_the_shipped_one() {
        let shipped = Rules {
            format: 1,
            revision: 5,
            ..Rules::default()
        };
        let newer = Rules {
            revision: 6,
            ..shipped.clone()
        };
        let older = Rules {
            revision: 4,
            ..shipped.clone()
        };
        let same = Rules {
            exclude: ExcludeRules {
                advert_hosts: vec!["x.example".into()],
                ..ExcludeRules::default()
            },
            ..shipped.clone()
        };
        assert_eq!(Rules::newest(shipped.clone(), Some(newer.clone())), newer);
        assert_eq!(Rules::newest(shipped.clone(), Some(older)), shipped);
        assert_eq!(Rules::newest(shipped.clone(), Some(same)), shipped);
        assert_eq!(Rules::newest(shipped.clone(), None), shipped);
    }

    #[test]
    fn a_page_gets_the_answer_rules_of_its_site_and_the_sites_it_is_part_of() {
        let rules = Rules::parse(&document(
            r#""sites": [ { "host": "video.example", "answers": [ { "url_contains": "/play" } ] },
                          { "host": "other.example", "answers": [ { "url_contains": "/o" } ] } ]"#,
        ))
        .expect("a document");
        assert_eq!(rules.answers_for("video.example").len(), 1);
        assert_eq!(rules.answers_for("WWW.video.example").len(), 1);
        assert_eq!(rules.answers_for("m.video.example").len(), 1);
        assert!(rules.answers_for("notvideo.example").is_empty());
        assert!(rules.answers_for("example").is_empty());
    }

    #[test]
    fn a_hello_carries_the_rules_and_an_old_hello_has_none() {
        let hello: crate::Hello =
            serde_json::from_str(r#"{ "engineVersion": "1.0.0" }"#).expect("an old hello");
        assert_eq!(hello.rules, None);
        let with = crate::Hello {
            rules: Some(Rules {
                format: 1,
                revision: 2,
                ..Rules::default()
            }),
            ..crate::Hello::default()
        };
        let text = serde_json::to_string(&with).expect("text");
        let back: crate::Hello = serde_json::from_str(&text).expect("a hello");
        assert_eq!(back, with);
        let none = serde_json::to_string(&crate::Hello::default()).expect("text");
        assert!(!none.contains("rules"), "{none}");
    }
}
