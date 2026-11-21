// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The project's pages that About and Help link to, what About says of the
//! update check, Help's tips and the email that tells a friend about dm:
//! the same in every window. The check itself (the manifest, the version,
//! the installer and its digest) is the application's
//! (`dm_shell::app::update`).

use dm_shell::app::update::Update;

use crate::i18n::{t, tf};

/// Where dm's documentation is: the project's README.
pub const HELP: &str = "https://github.com/pranavo72bex/Easy-Download-Manager#readme";

/// The project's source, licence, privacy statement and code signing
/// policy, as About dm links them.
pub const SOURCE: &str = "https://github.com/pranavo72bex/Easy-Download-Manager";
/// The guide to dm's parts, where Help's topics point (`docs/user-guide.md`).
pub const GUIDE: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/blob/main/docs/user-guide.md";
/// Where a person asks for help or reports a fault.
pub const SUPPORT: &str = "https://github.com/pranavo72bex/Easy-Download-Manager/issues";
/// The project's licence.
pub const LICENCE: &str = "https://github.com/pranavo72bex/Easy-Download-Manager/blob/main/LICENSE";
/// The project's privacy statement.
pub const PRIVACY: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/blob/main/PRIVACY.md";
/// The project's code signing policy.
pub const SIGNING: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager#code-signing-policy";

/// Where the browser extension is listed for Microsoft Edge.
pub const EXTENSION_EDGE: &str =
    "https://microsoftedge.microsoft.com/addons/detail/ooafpfjpliofcdlnbdkjhonaoebcpnej";
/// Where the add-on is listed for Firefox.
pub const EXTENSION_FIREFOX: &str = "https://addons.mozilla.org/firefox/addon/dm-download-manager/";
/// Where the add-on is listed for Chrome.
pub const EXTENSION_CHROME: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager#chrome-brave-vivaldi-and-opera";

/// The browsers Options › Browser lists, each with its extension's page
/// and the browsers its extension speaks for, by the names the host knows.
pub fn extension_browsers() -> [(&'static str, &'static str, &'static [&'static str]); 3] {
    [
        ("Microsoft Edge", EXTENSION_EDGE, &["Microsoft Edge"]),
        ("Firefox", EXTENSION_FIREFOX, &["Firefox"]),
        (
            t("Chrome and others"),
            EXTENSION_CHROME,
            &[
                "Google Chrome",
                "Brave",
                "Vivaldi",
                "Opera",
                "Arc",
                "Chromium",
            ],
        ),
    ]
}

/// What About says of the update check, in the language in force, for the
/// dm whose version is `running`.
pub fn update_words(update: &Update, running: &str) -> String {
    match update {
        Update::NotAsked => t("Not checked yet.").to_owned(),
        Update::Asking => t("Checking…").to_owned(),
        Update::Current => tf("This is the newest version ({}).", &[&running]),
        Update::Newer(v, Some(_)) => tf(
            "dm {} is available (you have {}). Download its installer here.",
            &[v, &running],
        ),
        Update::Newer(v, None) => tf(
            "dm {} is out (this is {}). It is on the project's releases page.",
            &[v, &running],
        ),
        Update::NoRelease => tf("No newer release yet (you have {}).", &[&running]),
        // In plain words where dm knows them (the raw
        // "error sending request for url (…)" offline).
        Update::Failed(why) => tf(
            "Could not reach the update server: {}",
            &[&crate::format::reason(why).map(t).unwrap_or(why.as_str())],
        ),
    }
}

/// Help's topics, each a page of dm's guide, by the words the menu gives
/// them.
pub fn guide_topics() -> [(&'static str, String); 3] {
    [
        (t("Tutorials"), GUIDE.to_string()),
        (
            t("Queues and the Scheduler"),
            format!("{GUIDE}#queues-and-the-scheduler"),
        ),
        (t("Grabber help"), format!("{GUIDE}#grabber")),
    ]
}

/// Tips about dm, one shown at a time in Tip of the day (gaps row 91).
pub fn tips() -> [&'static str; 8] {
    [
        t("Hold Alt while you click a link to leave that download with the browser."),
        t("Right-click a page and choose Download all links with dm to pick from every link on it."),
        t("Downloads › Scheduler starts a queue at set hours, and can shut the computer down when it is done."),
        t("Move the mouse over a playing video and click Download video to choose a quality."),
        t("Drag a link from the browser onto dm's window to download it."),
        t("The Grabber lists the files a website links to, by type and size; save a search to run it again."),
        t("Options › Advanced › Network sets a speed limit and the number of attempts for a failed download."),
        t("Properties shows a finished file's SHA-256 and checks it against a digest you were given."),
    ]
}

/// An email that tells someone about dm, written in the person's own mail
/// program for them to send or not: nothing leaves the computer unless they
/// send it.
pub fn tell_a_friend() -> String {
    let subject = t("A free download manager");
    let body = tf("I use dm to download files. It is free: {}", &[&SOURCE]);
    // Spaces as %20, which every mail program reads; `+` is a plus sign in a
    // mailto body.
    let encode = |s: &str| {
        url::form_urlencoded::byte_serialize(s.as_bytes())
            .collect::<String>()
            .replace('+', "%20")
    };
    format!("mailto:?subject={}&body={}", encode(subject), encode(&body))
}

/// Why a numbered series cannot be made, in the language in force.
pub fn series_error(e: dm_shell::lists::SeriesError) -> &'static str {
    use dm_shell::lists::SeriesError;
    match e {
        SeriesError::Star => t("The address needs exactly one * to fill in."),
        SeriesError::Backwards => t("The range runs backwards."),
        SeriesError::TooMany => t("Ten thousand addresses is more than a batch."),
        SeriesError::Ends => t("From and To must both be numbers, or both letters of one case."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_shell::lists::{series, SeriesError};

    #[test]
    fn each_reason_a_series_cannot_be_made_is_said() {
        let said = |e| series_error(e).to_string();
        let all = [
            SeriesError::Star,
            SeriesError::Backwards,
            SeriesError::TooMany,
            SeriesError::Ends,
        ]
        .map(said);
        // Four reasons, four sentences.
        let distinct: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(distinct.len(), 4);
        assert!(all.iter().all(|s| s.ends_with('.')));
        // As the series reads them.
        let reason = |t: &str, f: &str, to: &str| series(t, f, to, 0).err().map(said);
        assert_eq!(reason("h/*", "1", "20000"), Some(all[2].clone()));
        assert_eq!(reason("h/*", "1", "b"), Some(all[3].clone()));
        assert_eq!(reason("h/*", "9", "1"), Some(all[1].clone()));
        assert_eq!(reason("h/x", "1", "2"), Some(all[0].clone()));
    }

    #[test]
    fn the_update_check_is_said_with_the_version_running() {
        assert_eq!(update_words(&Update::NotAsked, "1.0.0"), "Not checked yet.");
        assert_eq!(
            update_words(&Update::Current, "1.0.0"),
            "This is the newest version (1.0.0)."
        );
        assert_eq!(
            update_words(&Update::NoRelease, "1.0.0"),
            "No newer release yet (you have 1.0.0)."
        );
    }

    #[test]
    fn telling_a_friend_writes_an_email_and_sends_nothing() {
        let mail = tell_a_friend();
        assert!(mail.starts_with("mailto:?subject="));
        assert!(!mail.contains('+'));
        assert!(mail.contains("Easy-Download-Manager"));
    }
}
