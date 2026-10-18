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

/// Where every installer dm offers must come from.
pub const RELEASES: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/releases/download/";

pub const ENDPOINT: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/releases/latest/download/latest.json";

#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    NotAsked,
    Asking,
    Current,
    /// A later version, and the installer for this machine when the
    /// release lists one.
    Newer(String, Option<String>),
    /// The update server answered that it has no release at all: nothing
    /// to update to, which is not a failure to reach it (round 13).
    NoRelease,
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
                "dm {} is out (this is {}). Its installer can be downloaded in dm, and opened when it has finished.",
                &[v, &running],
            ),
            Update::Newer(v, None) => tf(
                "dm {} is out (this is {}). It is on the project's releases page.",
                &[v, &running],
            ),
            Update::NoRelease => tf(
                "No release of dm has been published yet, so there is nothing to update to (this is {}).",
                &[&running],
            ),
            Update::Failed(why) => tf("Could not reach the update server: {}", &[why]),
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
            let res = client
                .get(ENDPOINT)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if res.status() == dm_core::reqwest::StatusCode::NOT_FOUND {
                return Ok(Update::NoRelease);
            }
            if !res.status().is_success() {
                return Err(format!("the server answered {}", res.status()));
            }
            let text = res.text().await.map_err(|e| e.to_string())?;
            let published = version_in(&text).ok_or("the release names no version")?;
            Ok(if newer(&published, env!("CARGO_PKG_VERSION")) {
                let installer = installer_in(
                    &text,
                    std::env::consts::OS,
                    std::env::consts::ARCH,
                    std::env::var_os("APPIMAGE").is_some(),
                );
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
