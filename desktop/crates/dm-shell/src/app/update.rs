// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Whether a newer dm has been published, and the site rules that ride along.
//!
//! Asked for rather than acted on: a download manager that restarted itself
//! mid-transfer to install a fix would be worse than the bug. The release's
//! own `latest.json` says what is current and where each installer is, so
//! there is no server to run. When a newer one is out, a presentation offers
//! to download the installer for this machine, as any other download in its
//! own queue; the person opens it when they choose. Reading the manifest,
//! the version, the installer and its digest, and the rules are the
//! application's; what is said of an [`Update`] is the presentation's.

use std::sync::{Arc, Mutex};

/// Where a release's notes may be: its page on the project's releases.
pub const NOTES: &str = "https://github.com/pranavo72bex/Easy-Download-Manager/releases/tag/";

/// The newest release's notes, from the last check, where it named a page
/// under `NOTES`: what About's "What's new" opens.
pub static LATEST_NOTES: Mutex<Option<String>> = Mutex::new(None);

/// A release's notes page from its `latest.json`, only one of the
/// project's own: a manifest naming anywhere else is not followed.
pub fn notes_in(manifest: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(manifest).ok()?;
    let notes = value.get("notes")?.as_str()?;
    (notes.starts_with(NOTES) && !notes.contains(['"', ' ', '\n'])).then(|| notes.to_owned())
}

/// Where every installer dm offers must come from.
pub const RELEASES: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/releases/download/";

/// Where the update check reads the latest release's manifest (`latest.json`).
pub const ENDPOINT: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/releases/latest/download/latest.json";

/// What the update check has found so far.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// The check has not been asked yet.
    NotAsked,
    /// The check is under way.
    Asking,
    /// This is the newest version.
    Current,
    /// A later version, and the installer for this machine when the
    /// release lists one with its digest.
    Newer(String, Option<Installer>),
    /// The update server answered that it has no release at all: nothing
    /// to update to, which is not a failure to reach it.
    NoRelease,
    /// The check failed, with the reason.
    Failed(String),
}

/// Whether `published` is later than `running`, comparing dotted numbers.
pub fn newer(published: &str, running: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['.', '-', '+'])
            .map_while(|p| p.parse().ok())
            .collect()
    };
    parts(published) > parts(running)
}

/// An installer a release offers for this machine, and the SHA-256 the
/// release published for it, which the download is checked against: a
/// file that does not match is removed, never offered to open.
#[derive(Debug, Clone, PartialEq)]
pub struct Installer {
    /// The installer's address, always under [`RELEASES`].
    pub url: String,
    /// The SHA-256 the release published for the installer, in lower-case hex.
    pub sha256: String,
}

/// The SHA-256 a `latest.json` lists for the installer at `url`, under
/// `sha256` by its file name; only a whole, well-formed digest.
pub fn digest_in(manifest: &str, url: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(manifest).ok()?;
    let name = url.rsplit('/').next()?;
    let digest = value
        .get("sha256")?
        .get(name)?
        .as_str()?
        .trim()
        .to_ascii_lowercase();
    (digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit())).then_some(digest)
}

/// The version a `latest.json` names.
pub fn version_in(manifest: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(manifest).ok()?;
    value.get("version")?.as_str().map(str::to_owned)
}

/// The installer for `os` and `arch` (as `std::env::consts` names them)
/// among the addresses a `latest.json` lists under `installers`, by the
/// names cargo-packager gives them: `dm_1.2.3_aarch64.dmg`,
/// `dm_1.2.3_x64-setup.exe`, `dm_1.2.3_amd64.deb`, `dm_1.2.3_x86_64.AppImage`.
/// On Linux only for a dm that is itself an AppImage (`appimage`): one
/// installed from a .deb, an .rpm or the Arch package is updated through
/// the package manager, and an AppImage beside it would be a second copy
/// that takes the browser's hand-offs over from the installed one.
pub fn installer_in(manifest: &str, os: &str, arch: &str, appimage: bool) -> Option<String> {
    if os == "linux" && !appimage {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(manifest).ok()?;
    let all: Vec<&str> = value
        .get("installers")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str())
        // Only the project's own releases: the list is fetched from there,
        // and an installer from anywhere else is not dm's to offer.
        .filter(|u| u.starts_with(RELEASES))
        .collect();
    let arches: &[&str] = match arch {
        "aarch64" => &["aarch64", "arm64"],
        "x86_64" => &["x86_64", "x64", "amd64"],
        _ => return None,
    };
    let kind = match os {
        "macos" => ".dmg",
        "windows" => "-setup.exe",
        "linux" => ".AppImage",
        _ => return None,
    };
    all.into_iter()
        .find(|u| {
            let name = u.rsplit('/').next().unwrap_or_default();
            name.ends_with(kind) && arches.iter().any(|a| name.contains(&format!("_{a}")))
        })
        .map(str::to_owned)
}

/// The site-rules document a `latest.json` lists under `rules`: where it is, the SHA-256 it must
/// have and its size. Only from under `prefix` (the project's releases), only a whole digest, and
/// never larger than [`RULES_CAP`].
#[derive(Debug, Clone, PartialEq)]
pub struct RulesEntry {
    /// The rules document's address, under the project's releases.
    pub url: String,
    /// The SHA-256 the document must have, in lower-case hex.
    pub sha256: String,
    /// The document's size in bytes.
    pub size: u64,
}

/// The most bytes of a rules document the update check fetches (the same cap the document has).
pub const RULES_CAP: u64 = 256 * 1024;

/// Reads the optional `rules` entry of a `latest.json`, giving it only if it is under `prefix`, has a whole digest and a size within [`RULES_CAP`].
pub fn rules_in(manifest: &str, prefix: &str) -> Option<RulesEntry> {
    let value: serde_json::Value = serde_json::from_str(manifest).ok()?;
    let entry = value.get("rules")?;
    let url = entry.get("url")?.as_str()?;
    if !url.starts_with(prefix) || url.contains(['"', ' ', '\n']) {
        return None;
    }
    let sha256 = entry.get("sha256")?.as_str()?.trim().to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let size = entry.get("size")?.as_u64()?;
    (size > 0 && size <= RULES_CAP).then(|| RulesEntry {
        url: url.to_owned(),
        sha256,
        size,
    })
}

/// The lower-case hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Fetches the rules document `entry` names and keeps it in `home` when it is the digest the
/// manifest gave, within the size it gave, a document this side reads and newer than what is
/// served. `Ok(true)` when kept. Any refusal leaves the previous file as it was.
pub async fn fetch_rules(
    client: &dm_core::reqwest::Client,
    entry: &RulesEntry,
    home: &std::path::Path,
) -> Result<bool, String> {
    let mut res = dm_core::redirects::send(client.get(&entry.url))
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("the server answered {}", res.status()));
    }
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        if (body.len() + chunk.len()) as u64 > entry.size {
            return Err("more than the manifest said".into());
        }
        body.extend_from_slice(&chunk);
    }
    if body.len() as u64 != entry.size {
        return Err("not the size the manifest said".into());
    }
    if sha256_hex(&body) != entry.sha256 {
        return Err("not the digest the manifest said".into());
    }
    let text = String::from_utf8(body).map_err(|e| e.to_string())?;
    let kept = crate::host::served::adopt(home, &text)?;
    if kept {
        // Its YouTube clients in force from now on, with no restart.
        crate::host::served::apply(home);
    }
    Ok(kept)
}

/// How often the weekly check looks.
pub const WEEK: u64 = 7 * 24 * 3600;

/// Whether a weekly check last done at `last` is due at `now` (both in
/// seconds since 1970). A clock set back is due, rather than never.
pub fn due(last: u64, now: u64) -> bool {
    now < last || now - last >= WEEK
}

/// The settings with the weekly check stamped at `now`, when the person
/// wants it and it is due: to be saved as the check is made, so the next is
/// a week after this one however long dm runs. `None` when no check is due.
pub fn weekly(settings: &crate::Settings, now: u64) -> Option<crate::Settings> {
    if !settings.check_updates || !due(settings.update_checked, now) {
        return None;
    }
    let mut wanted = settings.clone();
    wanted.update_checked = now;
    Some(wanted)
}

/// Asks in the background, with the application's client (`Backend::client`:
/// through the proxy downloads go through; a plain client went out directly,
/// past a proxy a person had set so that nothing would), whether a version
/// later than `running` is published, and keeps the answer in `into`. Newer
/// site rules are kept as they arrive. `wake` is called once the answer is
/// in, for the presentation to show it.
pub fn check(
    backend: &crate::app::backend::Backend,
    running: &'static str,
    into: Arc<Mutex<Update>>,
    wake: impl FnOnce() + Send + 'static,
) {
    *dm_core::lock(&into) = Update::Asking;
    let client = backend.client();
    backend.runtime.spawn(async move {
        let outcome = async {
            let res = dm_core::redirects::send(client.get(ENDPOINT))
                .await
                .map_err(|e| e.to_string())?;
            if res.status() == dm_core::reqwest::StatusCode::NOT_FOUND {
                return Ok(Update::NoRelease);
            }
            if !res.status().is_success() {
                return Err(format!("the server answered {}", res.status()));
            }
            let bytes = res.bytes().await.map_err(|e| e.to_string())?;
            let text = String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())?;
            let published = version_in(&text).ok_or("the release names no version")?;
            *dm_core::lock(&LATEST_NOTES) = notes_in(&text);
            // Newer site rules ride along, whether or not dm itself is newer. A refusal
            // keeps the rules in force and does not fail the check.
            if let Some(entry) = rules_in(&text, RELEASES) {
                let _ = fetch_rules(&client, &entry, &crate::settings::home()).await;
            }
            Ok(if newer(&published, running) {
                // Offered only with its digest: an installer that cannot be
                // checked is left to the releases page.
                let installer = installer_in(
                    &text,
                    std::env::consts::OS,
                    std::env::consts::ARCH,
                    std::env::var_os("APPIMAGE").is_some(),
                )
                .and_then(|url| digest_in(&text, &url).map(|sha256| Installer { url, sha256 }));
                Update::Newer(published, installer)
            } else {
                Update::Current
            })
        }
        .await;
        *dm_core::lock(&into) = outcome.unwrap_or_else(Update::Failed);
        wake();
    });
}

/// Asks in the background, with the application's client, for the site rules the release
/// manifest names, and keeps them where newer (`fetch_rules`), with no update offered: for a
/// YouTube player whose checks dm could not answer (`dm_core::youtube::take_unsolved_player`),
/// whose fix may be in newer rules, out of the weekly turn (gaps row Y3). Any failure keeps the
/// rules in force and says nothing: the reading has already said why it failed.
pub fn fresh_rules(backend: &crate::app::backend::Backend) {
    let client = backend.client();
    backend.runtime.spawn(async move {
        let Ok(res) = dm_core::redirects::send(client.get(ENDPOINT)).await else {
            return;
        };
        if !res.status().is_success() {
            return;
        }
        let Ok(text) = res.text().await else {
            return;
        };
        if let Some(entry) = rules_in(&text, RELEASES) {
            let _ = fetch_rules(&client, &entry, &crate::settings::home()).await;
        }
    });
}

#[cfg(test)]
mod tests;
