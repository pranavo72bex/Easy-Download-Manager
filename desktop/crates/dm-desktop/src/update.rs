//! Whether a newer dm has been published.
//!
//! Asked for rather than acted on: a download manager that restarted itself
//! mid-transfer to install a fix would be worse than the bug. The release's
//! own `latest.json` says what is current and where each installer is, so
//! there is no server to run. When a newer one is out, dm offers to download
//! the installer for this machine, as any other download in its own queue;
//! the person opens it when they choose.

use std::sync::{Arc, Mutex};

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

impl Update {
    /// What About says, in the language in force.
    pub fn shown(&self) -> String {
        let running = env!("CARGO_PKG_VERSION");
        match self {
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
    dm_shell::host::served::adopt(home, &text)
}

/// How often the weekly check looks.
pub const WEEK: u64 = 7 * 24 * 3600;

/// Whether a weekly check last done at `last` is due at `now` (both in
/// seconds since 1970). A clock set back is due, rather than never.
pub fn due(last: u64, now: u64) -> bool {
    now < last || now - last >= WEEK
}

/// Asks, with `client`: the one built from the settings, so the question
/// goes through the proxy downloads go through. A plain client went out
/// directly, past a proxy a person had set so that nothing would.
pub fn check(
    runtime: &tokio::runtime::Handle,
    client: dm_core::reqwest::Client,
    into: Arc<Mutex<Update>>,
    ctx: egui::Context,
) {
    *dm_core::lock(&into) = Update::Asking;
    runtime.spawn(async move {
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
                let _ = fetch_rules(&client, &entry, &dm_shell::settings::home()).await;
            }
            Ok(if newer(&published, env!("CARGO_PKG_VERSION")) {
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
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_releases_notes_are_followed_only_to_the_projects_own_page() {
        let page = format!("{NOTES}desktop-v0.2.0");
        let manifest = |notes: &str| format!("{{\"version\":\"0.2.0\",\"notes\":\"{notes}\"}}");
        assert_eq!(notes_in(&manifest(&page)), Some(page));
        assert_eq!(notes_in(&manifest("https://elsewhere.example/notes")), None);
        assert_eq!(notes_in("{\"version\":\"0.2.0\"}"), None);
        assert_eq!(notes_in("not json"), None);
    }

    fn rules_doc(revision: u64, host: &str) -> String {
        format!(
            r#"{{ "format": 1, "revision": {revision}, "exclude": {{ "advert_hosts": ["{host}"] }} }}"#
        )
    }

    /// A server that answers `body` at /rules.json; its address.
    async fn serve(body: Vec<u8>) -> String {
        let app = axum::Router::new().route(
            "/rules.json",
            axum::routing::get(move || {
                let body = body.clone();
                async move { body }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{at}/")
    }

    fn entry_for(base: &str, body: &[u8]) -> RulesEntry {
        RulesEntry {
            url: format!("{base}rules.json"),
            sha256: sha256_hex(body),
            size: body.len() as u64,
        }
    }

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn the_manifests_rules_entry_is_read_only_whole_and_from_the_projects_releases() {
        let url = format!("{RELEASES}desktop-v1/rules.json");
        let good = "ab".repeat(32);
        let manifest = |url: &str, sha: &str, size: u64| {
            serde_json::json!({ "version": "1", "rules": { "url": url, "sha256": sha, "size": size } })
                .to_string()
        };
        let got = rules_in(&manifest(&url, &good.to_uppercase(), 100), RELEASES).unwrap();
        assert_eq!(got.sha256, good);
        assert_eq!((got.url.as_str(), got.size), (url.as_str(), 100));
        assert_eq!(
            rules_in(
                &manifest("https://elsewhere.example/r.json", &good, 100),
                RELEASES
            ),
            None
        );
        assert_eq!(rules_in(&manifest(&url, "abc", 100), RELEASES), None);
        assert_eq!(rules_in(&manifest(&url, &good, 0), RELEASES), None);
        assert_eq!(
            rules_in(&manifest(&url, &good, RULES_CAP + 1), RELEASES),
            None
        );
        assert_eq!(
            rules_in(r#"{"version":"1"}"#, RELEASES),
            None,
            "an older manifest"
        );
        assert_eq!(rules_in("not json", RELEASES), None);
    }

    #[tokio::test]
    async fn newer_rules_are_kept_and_older_or_equal_ones_are_not() {
        let dir = home();
        let client = dm_core::reqwest::Client::new();
        let newer = rules_doc(900, "ads.example").into_bytes();
        let base = serve(newer.clone()).await;
        assert_eq!(
            fetch_rules(&client, &entry_for(&base, &newer), dir.path()).await,
            Ok(true)
        );
        let kept = std::fs::read_to_string(dir.path().join(dm_shell::host::served::FILE)).unwrap();
        assert_eq!(kept.as_bytes(), newer.as_slice());

        let older = rules_doc(10, "old.example").into_bytes();
        let base = serve(older.clone()).await;
        assert_eq!(
            fetch_rules(&client, &entry_for(&base, &older), dir.path()).await,
            Ok(false)
        );
        let kept = std::fs::read_to_string(dir.path().join(dm_shell::host::served::FILE)).unwrap();
        assert_eq!(kept.as_bytes(), newer.as_slice());
    }

    #[tokio::test]
    async fn rules_with_a_wrong_digest_size_or_content_are_refused_and_change_nothing() {
        let dir = home();
        let client = dm_core::reqwest::Client::new();
        let file = dir.path().join(dm_shell::host::served::FILE);
        let doc = rules_doc(900, "ads.example").into_bytes();
        let base = serve(doc.clone()).await;

        let mut wrong_digest = entry_for(&base, &doc);
        wrong_digest.sha256 = "0".repeat(64);
        assert!(fetch_rules(&client, &wrong_digest, dir.path())
            .await
            .is_err());

        let mut too_small = entry_for(&base, &doc);
        too_small.size -= 1;
        assert!(fetch_rules(&client, &too_small, dir.path()).await.is_err());

        let mut too_big = entry_for(&base, &doc);
        too_big.size += 1;
        assert!(fetch_rules(&client, &too_big, dir.path()).await.is_err());

        // Right digest of something that is not a rules document.
        let garbage = b"<html>not rules</html>".to_vec();
        let base = serve(garbage.clone()).await;
        assert!(
            fetch_rules(&client, &entry_for(&base, &garbage), dir.path())
                .await
                .is_err()
        );

        // Nothing there at all.
        let mut missing = entry_for(&base, &doc);
        missing.url = format!("{base}absent.json");
        assert!(fetch_rules(&client, &missing, dir.path()).await.is_err());
        assert!(!file.exists());
    }

    #[test]
    fn the_weekly_check_is_due_after_a_week_and_not_before() {
        assert!(due(0, WEEK), "never checked");
        assert!(!due(1_000_000, 1_000_000 + WEEK - 1));
        assert!(due(1_000_000, 1_000_000 + WEEK));
        assert!(due(1_000_000, 10), "a clock set back");
    }

    #[test]
    fn a_later_version_is_newer_and_the_same_one_is_not() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("v0.1.10", "0.1.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        assert_eq!(
            version_in(r#"{"version":"1.2.3","platforms":{}}"#).as_deref(),
            Some("1.2.3")
        );
        assert_eq!(version_in("not json"), None);
    }

    #[test]
    fn an_installers_digest_is_read_by_its_name_and_only_whole() {
        let url = format!("{RELEASES}desktop-v9.9.9/dm_9.9.9_aarch64.dmg");
        let good = "AB".repeat(32);
        let manifest = serde_json::json!({
            "version": "9.9.9",
            "installers": [url],
            "sha256": {
                "dm_9.9.9_aarch64.dmg": good,
                "dm_9.9.9_x64-setup.exe": "abc",
            },
        })
        .to_string();
        assert_eq!(digest_in(&manifest, &url), Some(good.to_lowercase()));
        // Too short to be one, or not listed: none.
        let exe = format!("{RELEASES}desktop-v9.9.9/dm_9.9.9_x64-setup.exe");
        assert_eq!(digest_in(&manifest, &exe), None);
        assert_eq!(digest_in(r#"{"version":"9.9.9"}"#, &url), None);
    }

    #[test]
    fn the_installer_for_this_machine_is_picked_by_its_name() {
        let base = format!("{RELEASES}desktop-v1.2.3");
        let manifest = serde_json::json!({
            "version": "1.2.3",
            "installers": [
                format!("{base}/dm_1.2.3_aarch64.dmg"),
                format!("{base}/dm_1.2.3_x64.dmg"),
                format!("{base}/dm_1.2.3_x64-setup.exe"),
                format!("{base}/dm_1.2.3_amd64.deb"),
                format!("{base}/dm_1.2.3_x86_64.AppImage"),
                "http://insecure.example/dm_1.2.3_aarch64.AppImage",
                "https://elsewhere.example/dm_1.2.3_arm64-setup.exe",
            ],
        })
        .to_string();
        let pick = |os, arch| installer_in(&manifest, os, arch, true);
        assert_eq!(
            pick("macos", "aarch64"),
            Some(format!("{base}/dm_1.2.3_aarch64.dmg"))
        );
        assert_eq!(
            pick("macos", "x86_64"),
            Some(format!("{base}/dm_1.2.3_x64.dmg"))
        );
        assert_eq!(
            pick("windows", "x86_64"),
            Some(format!("{base}/dm_1.2.3_x64-setup.exe"))
        );
        assert_eq!(
            pick("linux", "x86_64"),
            Some(format!("{base}/dm_1.2.3_x86_64.AppImage"))
        );
        assert_eq!(pick("linux", "aarch64"), None, "never over plain http");
        assert_eq!(
            pick("windows", "aarch64"),
            None,
            "only from dm's own releases"
        );
        assert_eq!(pick("freebsd", "x86_64"), None);
        assert_eq!(
            installer_in(&manifest, "linux", "x86_64", false),
            None,
            "a packaged dm is updated by its package manager"
        );
        assert_eq!(
            installer_in(r#"{"version":"1"}"#, "macos", "aarch64", false),
            None,
            "an older manifest"
        );
    }
}
