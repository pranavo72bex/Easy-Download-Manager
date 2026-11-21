// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The browsers dm knows: where each reads host manifests from, how it is told to be
//! installed, and which store its extension comes from.

use std::path::{Path, PathBuf};

use dm_protocol::NATIVE_HOST;

/// Which kind of manifest a directory wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Chrome, Edge, Brave, Vivaldi, Opera and Chromium: `allowed_origins`.
    Chromium,
    /// Firefox: `allowed_extensions`.
    Firefox,
}

/// A directory one browser reads host manifests from.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// The directory the browser reads host manifests from.
    pub dir: PathBuf,
    /// Which kind of manifest it wants there.
    pub family: Family,
    /// What says the browser is installed though it has never been opened:
    /// its programs on `PATH` (Linux), or its bundles in an Applications
    /// folder (macOS).
    pub programs: &'static [&'static str],
}

/// Where installed browsers' programs are looked for.
/// A browser installed but never opened has no profile folder, and was not
/// registered: on a fresh desktop, in the order the README gives (install
/// dm, open it, then open the browser and add the extension), the extension
/// found no app until dm's next launch.
#[derive(Debug, Clone, Default)]
pub struct Programs {
    /// The directories of `PATH`, where Linux browsers put their programs.
    pub path: Vec<PathBuf>,
    /// The Applications folders, where macOS keeps app bundles.
    pub applications: Vec<PathBuf>,
    /// Where a browser unpacked from an archive lives on Linux, in a folder
    /// of its own named after it (`/opt/firefox/firefox`): one never run
    /// and not on `PATH` was not registered.
    pub unpacked: Vec<PathBuf>,
}

impl Programs {
    /// This computer's: `PATH`, `/Applications` and `~/Applications`.
    pub fn here(home: &Path) -> Self {
        Programs {
            path: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            applications: vec![PathBuf::from("/Applications"), home.join("Applications")],
            unpacked: vec![
                PathBuf::from("/opt"),
                PathBuf::from("/usr/local"),
                home.to_path_buf(),
                home.join(".local/share"),
                home.join("Applications"),
            ],
        }
    }

    /// Whether any of `names` is installed: an app bundle on macOS, a file
    /// on `PATH` elsewhere.
    fn has(&self, names: &[&str]) -> bool {
        let (dirs, bundle) = if cfg!(target_os = "macos") {
            (&self.applications, true)
        } else {
            (&self.path, false)
        };
        let on_path = names.iter().any(|name| {
            dirs.iter().any(|dir| {
                let found = dir.join(name);
                if bundle {
                    found.is_dir()
                } else {
                    found.is_file()
                }
            })
        });
        on_path || (!bundle && self.unpacked_has(names))
    }

    /// Whether a folder in `unpacked` named after one of `names`
    /// (`firefox`, `firefox-esr`, `firefox-128.0`) holds that program.
    pub(super) fn unpacked_has(&self, names: &[&str]) -> bool {
        self.unpacked.iter().any(|root| {
            let Ok(entries) = std::fs::read_dir(root) else {
                return false;
            };
            entries.flatten().any(|entry| {
                let folder = entry.file_name().to_string_lossy().to_lowercase();
                names
                    .iter()
                    .any(|name| folder.starts_with(name) && entry.path().join(name).is_file())
            })
        })
    }
}

impl Place {
    /// The browser's own directory, which exists when it is installed.
    pub(super) fn browser(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    /// Whether the browser is installed: it has run (its folder is there)
    /// or its program is in `programs`. Firefox as a Snap (Ubuntu's own)
    /// keeps its profile under ~/snap/firefox, so a fresh Ubuntu has no
    /// ~/.mozilla at all; its native messaging still reads the manifest
    /// from ~/.mozilla/native-messaging-hosts.
    pub(super) fn installed(&self, programs: &Programs) -> bool {
        let browser = self.browser();
        browser.is_dir()
            || programs.has(self.programs)
            || (self.family == Family::Firefox
                && browser.file_name() == Some(std::ffi::OsStr::new(".mozilla"))
                && browser
                    .parent()
                    .is_some_and(|home| home.join("snap/firefox").is_dir()))
    }

    /// Where dm's host manifest goes in this directory.
    pub fn manifest(&self) -> PathBuf {
        self.dir.join(format!("{NATIVE_HOST}.json"))
    }
}

/// Where a browser's extension comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    /// Microsoft Edge Add-ons.
    Edge,
    /// Firefox Browser Add-ons (addons.mozilla.org).
    Firefox,
    /// The Chrome Web Store, which Brave, Vivaldi, Opera and Chromium use.
    Chrome,
}

/// A browser found on this computer, to offer it dm's extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Installed {
    /// The browser's name as people know it ("Brave").
    pub name: &'static str,
    /// Where its extension is installed from.
    pub store: Store,
}

/// Each browser dm offers its extension to, its store, and where it is
/// found: its app bundle on macOS, its program on Linux's `PATH`, its
/// program under the program folders on Windows.
const BROWSERS: [(&str, Store, &str, &[&str], &str); 8] = [
    (
        "Microsoft Edge",
        Store::Edge,
        "Microsoft Edge.app",
        &["microsoft-edge", "microsoft-edge-stable"],
        r"Microsoft\Edge\Application\msedge.exe",
    ),
    (
        "Firefox",
        Store::Firefox,
        "Firefox.app",
        &["firefox"],
        r"Mozilla Firefox\firefox.exe",
    ),
    (
        "Google Chrome",
        Store::Chrome,
        "Google Chrome.app",
        &["google-chrome", "google-chrome-stable"],
        r"Google\Chrome\Application\chrome.exe",
    ),
    (
        "Brave",
        Store::Chrome,
        "Brave Browser.app",
        &["brave-browser", "brave"],
        r"BraveSoftware\Brave-Browser\Application\brave.exe",
    ),
    (
        "Vivaldi",
        Store::Chrome,
        "Vivaldi.app",
        &["vivaldi", "vivaldi-stable"],
        r"Vivaldi\Application\vivaldi.exe",
    ),
    (
        "Opera",
        Store::Chrome,
        "Opera.app",
        &["opera"],
        r"Programs\Opera\opera.exe",
    ),
    (
        "Chromium",
        Store::Chrome,
        "Chromium.app",
        &["chromium", "chromium-browser"],
        r"Chromium\Application\chrome.exe",
    ),
    (
        "Arc",
        Store::Chrome,
        "Arc.app",
        &[],
        r"Arc\Application\Arc.exe",
    ),
];

/// The browsers installed on this computer, in the order they are offered.
pub fn installed_browsers(home: &Path) -> Vec<Installed> {
    let programs = Programs::here(home);
    let windows_bases = dm_core::folders::windows_programs();
    installed_where(|bundle, on_path, under| {
        if cfg!(target_os = "macos") {
            programs
                .applications
                .iter()
                .any(|dir| dir.join(bundle).is_dir())
        } else if cfg!(windows) {
            windows_bases.iter().any(|base| base.join(under).is_file())
        } else {
            programs
                .path
                .iter()
                .any(|dir| on_path.iter().any(|p| dir.join(p).is_file()))
        }
    })
}

/// The browsers `found` says are there, asked with each one's bundle, its
/// programs and its path under a program folder.
pub fn installed_where(found: impl Fn(&str, &[&str], &str) -> bool) -> Vec<Installed> {
    BROWSERS
        .iter()
        .filter(|(_, _, bundle, on_path, under)| found(bundle, on_path, under))
        .map(|(name, store, ..)| Installed {
            name,
            store: *store,
        })
        .collect()
}

/// Every directory a browser on this system reads host manifests from, under
/// the home directory `home`. A manifest is written to each one whose
/// browser is installed, because someone with Chrome and Brave means both.
pub fn places(home: &Path) -> Vec<Place> {
    type Found = Vec<(PathBuf, &'static [&'static str])>;
    let (chromium, firefox): (Found, Found) = if cfg!(target_os = "macos") {
        let support = dm_core::folders::application_support(home);
        (
            [
                ("Google/Chrome", &["Google Chrome.app"][..]),
                ("Google/Chrome Beta", &["Google Chrome Beta.app"]),
                ("Google/Chrome Dev", &["Google Chrome Dev.app"]),
                ("Google/Chrome Canary", &["Google Chrome Canary.app"]),
                (
                    "Google/Chrome for Testing",
                    &["Google Chrome for Testing.app"],
                ),
                ("Chromium", &["Chromium.app"]),
                ("BraveSoftware/Brave-Browser", &["Brave Browser.app"]),
                ("Microsoft Edge", &["Microsoft Edge.app"]),
                ("Vivaldi", &["Vivaldi.app"]),
                ("com.operasoftware.Opera", &["Opera.app"]),
                ("Arc/User Data", &["Arc.app"]),
            ]
            .iter()
            .map(|(b, programs)| (support.join(b).join("NativeMessagingHosts"), *programs))
            .collect(),
            vec![(
                support.join("Mozilla/NativeMessagingHosts"),
                &[
                    "Firefox.app",
                    "Firefox Developer Edition.app",
                    "Firefox Nightly.app",
                ][..],
            )],
        )
    } else if cfg!(target_os = "windows") {
        // Windows reads no manifest directories: see `WINDOWS_KEYS`.
        (Vec::new(), Vec::new())
    } else {
        let config = home.join(".config");
        (
            [
                (
                    "google-chrome",
                    &["google-chrome", "google-chrome-stable"][..],
                ),
                ("google-chrome-beta", &["google-chrome-beta"]),
                ("google-chrome-unstable", &["google-chrome-unstable"]),
                ("google-chrome-for-testing", &[]),
                ("chromium", &["chromium", "chromium-browser"]),
                (
                    "BraveSoftware/Brave-Browser",
                    &["brave-browser", "brave-browser-stable", "brave"],
                ),
                (
                    "microsoft-edge",
                    &["microsoft-edge", "microsoft-edge-stable"],
                ),
                ("vivaldi", &["vivaldi", "vivaldi-stable"]),
                ("opera", &["opera"]),
            ]
            .iter()
            .map(|(b, programs)| (config.join(b).join("NativeMessagingHosts"), *programs))
            .collect(),
            vec![(
                home.join(".mozilla/native-messaging-hosts"),
                &["firefox", "firefox-esr"][..],
            )],
        )
    };
    chromium
        .into_iter()
        .map(|(dir, programs)| Place {
            dir,
            family: Family::Chromium,
            programs,
        })
        .chain(firefox.into_iter().map(|(dir, programs)| Place {
            dir,
            family: Family::Firefox,
            programs,
        }))
        .collect()
}
