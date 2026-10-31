//! Telling the browsers where dm is: `dm-desktop --register` and
//! `--unregister`.
//!
//! A browser only launches a native messaging host it has a manifest for, in
//! a directory it looks in, naming the extensions allowed to talk to it. That
//! last part is the security boundary — without it any extension could start
//! a program on this machine — so a manifest names dm's extension and no
//! other. The unpacked Chrome build's id is fixed by the key in its manifest
//! and Firefox's by `browser_specific_settings`, so both are registered with
//! no id typed; a store build's id is named on the command line.
//!
//! The binary registers itself: the path written is the running program's
//! (or `DM_ENGINE`'s), which is the binary the browser will start.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use std::path::{Path, PathBuf};

use dm_protocol::{CHROME_EXTENSION_ID, EDGE_EXTENSION_ID, FIREFOX_EXTENSION_ID, NATIVE_HOST};

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
    fn unpacked_has(&self, names: &[&str]) -> bool {
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
    fn browser(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    /// Whether the browser is installed: it has run (its folder is there)
    /// or its program is in `programs`. Firefox as a Snap (Ubuntu's own)
    /// keeps its profile under ~/snap/firefox, so a fresh Ubuntu has no
    /// ~/.mozilla at all; its native messaging still reads the manifest
    /// from ~/.mozilla/native-messaging-hosts.
    fn installed(&self, programs: &Programs) -> bool {
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
    let windows_bases: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
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
        let support = home.join("Library/Application Support");
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

/// Where browsers on Windows look for a host: a key under
/// `HKEY_CURRENT_USER` whose default value is the manifest's path. Windows
/// reads no manifest directories, so a key is written for each browser
/// whether or not it is installed yet; one installed later then finds dm.
pub const WINDOWS_KEYS: &[(&str, Family)] = &[
    (
        r"Software\Google\Chrome\NativeMessagingHosts",
        Family::Chromium,
    ),
    (r"Software\Chromium\NativeMessagingHosts", Family::Chromium),
    (
        r"Software\Microsoft\Edge\NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
        Family::Chromium,
    ),
    (r"Software\Vivaldi\NativeMessagingHosts", Family::Chromium),
    (r"Software\Mozilla\NativeMessagingHosts", Family::Firefox),
];

/// The manifest file of each family on Windows, under `data` (dm's folder in
/// `%LOCALAPPDATA%`). The two families read different keys, so they are two
/// files.
pub fn windows_manifest(data: &Path, family: Family) -> PathBuf {
    let sub = match family {
        Family::Chromium => "chromium",
        Family::Firefox => "firefox",
    };
    data.join("NativeMessagingHosts")
        .join(sub)
        .join(format!("{NATIVE_HOST}.json"))
}

/// The full registry key of dm's host under one browser's key.
fn windows_key(key: &str) -> String {
    format!(r"HKCU\{key}\{NATIVE_HOST}")
}

/// `reg.exe` arguments that point one browser at dm's manifest.
pub fn reg_add(key: &str, manifest: &Path) -> Vec<String> {
    [
        "add",
        &windows_key(key),
        "/ve",
        "/t",
        "REG_SZ",
        "/d",
        &manifest.to_string_lossy(),
        "/f",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// `reg.exe` arguments that remove dm from one browser.
pub fn reg_delete(key: &str) -> Vec<String> {
    ["delete", &windows_key(key), "/f"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Registers on Windows: both manifests written under `data`, then a key
/// for every browser. `reg` runs `reg.exe` with the arguments given; a test
/// passes one that records them. Returns what was written, files then keys.
pub fn register_windows(
    data: &Path,
    exe: &Path,
    allowed: &Allowed,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<Vec<PathBuf>, String> {
    if !exe.is_absolute() || !exe.is_file() {
        return Err(format!(
            "{} is not a program a browser could start",
            exe.display()
        ));
    }
    let mut written = Vec::new();
    for family in [Family::Chromium, Family::Firefox] {
        let file = windows_manifest(data, family);
        let text = serde_json::to_string_pretty(&manifest(family, exe, allowed))
            .map_err(|e| e.to_string())?;
        file.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| dm_core::write_atomic(&file, format!("{text}\n").as_bytes()))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        written.push(file);
    }
    for (key, family) in WINDOWS_KEYS {
        reg(&reg_add(key, &windows_manifest(data, *family)))?;
        written.push(PathBuf::from(windows_key(key)));
    }
    Ok(written)
}

/// Removes dm from every browser on Windows. Returns what was removed.
pub fn unregister_windows(
    data: &Path,
    mut reg: impl FnMut(&[String]) -> Result<(), String>,
) -> Vec<PathBuf> {
    let keys = WINDOWS_KEYS
        .iter()
        .filter(|(key, _)| reg(&reg_delete(key)).is_ok())
        .map(|(key, _)| PathBuf::from(windows_key(key)));
    let files = [Family::Chromium, Family::Firefox]
        .into_iter()
        .map(|family| windows_manifest(data, family))
        .filter(|file| std::fs::remove_file(file).is_ok());
    keys.collect::<Vec<_>>().into_iter().chain(files).collect()
}

/// Runs Windows' own `reg.exe`, which every Windows has: no registry crate
/// and no `unsafe` for six keys.
pub fn run_reg(args: &[String]) -> Result<(), String> {
    let out = dm_core::no_window(&mut std::process::Command::new("reg"))
        .args(args)
        .output()
        .map_err(|e| format!("reg.exe could not be run: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "reg {}: {}",
            args.first().map(String::as_str).unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// dm's folder in `%LOCALAPPDATA%`, where its Windows manifests live.
fn windows_data(home: &Path) -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| home.join("AppData").join("Local"))
        .join("dm")
}

/// Which extensions may talk to dm: the two built from this repository, the
/// Edge Add-ons listing's, and any named — a Chrome Web Store id, or a
/// signed Firefox add-on's.
#[derive(Debug, Clone, PartialEq)]
pub struct Allowed {
    /// The `chrome-extension://…/` origins Chromium browsers may connect from.
    pub chromium: Vec<String>,
    /// The add-on ids Firefox may connect from.
    pub firefox: Vec<String>,
}

impl Allowed {
    /// The extensions built here and the Edge listing's, and those `named`:
    /// a 32-letter Chrome Web Store id or a Firefox add-on id (`name@host` or
    /// a `{uuid}`). Anything else is refused in words.
    pub fn with(named: &[String]) -> Result<Self, String> {
        let mut allowed = Allowed {
            chromium: vec![
                CHROME_EXTENSION_ID.to_string(),
                EDGE_EXTENSION_ID.to_string(),
            ],
            firefox: vec![FIREFOX_EXTENSION_ID.to_string()],
        };
        for id in named {
            let id = id.trim();
            let list = if id.contains('@') || (id.starts_with('{') && id.ends_with('}')) {
                // Firefox ids are an address or a braced UUID.
                &mut allowed.firefox
            } else if id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
                // Chrome's are 32 letters from a to p, and nothing else.
                &mut allowed.chromium
            } else {
                return Err(format!(
                    "{id:?} is not an extension id. Chrome's is 32 letters from a to p, shown \
                     on chrome://extensions with Developer mode on; Firefox's looks like an \
                     address."
                ));
            };
            if !list.iter().any(|known| known == id) {
                list.push(id.to_string());
            }
        }
        Ok(allowed)
    }
}

/// The manifest one family reads.
pub fn manifest(family: Family, exe: &Path, allowed: &Allowed) -> serde_json::Value {
    let mut manifest = serde_json::json!({
        "name": NATIVE_HOST,
        "description": "dm download manager",
        "path": exe,
        "type": "stdio",
    });
    let (key, value): (&str, serde_json::Value) = match family {
        Family::Chromium => (
            "allowed_origins",
            allowed
                .chromium
                .iter()
                .map(|id| format!("chrome-extension://{id}/"))
                .collect(),
        ),
        Family::Firefox => ("allowed_extensions", allowed.firefox.clone().into()),
    };
    if let Some(fields) = manifest.as_object_mut() {
        fields.insert(key.to_string(), value);
    }
    manifest
}

/// Whether a browser on macOS will be refused when it runs `exe`.
///
/// Downloads, Desktop and Documents are behind a folder permission browsers
/// do not have. The failure is silent and looks exactly like a bug in dm: the
/// host hangs in the loader opening its own executable while the system
/// waits on a prompt nobody sees, and the extension reports that dm did not
/// answer. Found by the Chrome hand-off check, with this repository under
/// ~/Downloads.
pub fn protected(exe: &Path, home: &Path, macos: bool) -> bool {
    macos
        && ["Downloads", "Desktop", "Documents"]
            .iter()
            .any(|folder| exe.starts_with(home.join(folder)))
}

/// Whether macOS is running `exe` from a place that will not be there next
/// time: a mounted disk image, or AppTranslocation — the randomised
/// read-only copy macOS runs a quarantined app from when it was opened
/// where it was downloaded rather than moved to Applications. A browser
/// manifest or login item naming either starts nothing later.
/// A disk image is told by being mounted read-only under `/Volumes`
/// (`read_only` says whether the mount holding a path is). An external
/// disk is mounted there too, writable, and an install on it is a place
/// like any other — refusing it left that install with no browser
/// registration and no login item.
pub fn transient_on(exe: &Path, macos: bool, read_only: impl FnOnce(&Path) -> bool) -> bool {
    macos
        && (exe.to_string_lossy().contains("/AppTranslocation/")
            || (exe.starts_with("/Volumes") && read_only(exe)))
}

/// [`transient_on`] for the system this runs on, the mount's flags read.
pub fn transient(exe: &Path) -> bool {
    transient_on(exe, cfg!(target_os = "macos"), mounted_read_only)
}

/// Whether the file system holding `path` is mounted read-only. Unreadable
/// is taken as writable: the answer that refuses nothing.
pub fn mounted_read_only(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
        // SAFETY: `c` is a valid NUL-terminated string and `stat` is
        // writable memory of the right size, filled only on success.
        let done = unsafe { libc::statvfs(c.as_ptr(), stat.as_mut_ptr()) };
        if done != 0 {
            return false;
        }
        // SAFETY: statvfs returned 0, so it filled `stat`.
        let stat = unsafe { stat.assume_init() };
        (stat.f_flag & libc::ST_RDONLY) != 0
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// Writes a manifest for every installed browser, naming `exe` and the
/// extensions `allowed`. Returns the files written. A browser counts as
/// installed when it has run or its program is in `programs`; a folder is
/// made for no other.
pub fn register(
    home: &Path,
    exe: &Path,
    allowed: &Allowed,
    programs: &Programs,
) -> Result<Vec<PathBuf>, String> {
    if cfg!(target_os = "windows") {
        return register_windows(&windows_data(home), exe, allowed, run_reg);
    }
    if !exe.is_absolute() || !exe.is_file() {
        return Err(format!(
            "{} is not a program a browser could start",
            exe.display()
        ));
    }
    // Before anything is written: a manifest pointing at a binary the browser
    // may not run is the failure this exists to prevent.
    if transient(exe) {
        return Err(format!(
            "{}\nis a copy macOS runs from a disk image or from a temporary copy it made \
             (AppTranslocation), gone by the next launch. Drag dm to Applications (or any \
             folder on a disk you can write to) and open it from there, so your browser can \
             hand it downloads.",
            exe.display()
        ));
    }
    if protected(exe, home, cfg!(target_os = "macos")) {
        return Err(format!(
            "{}\nis inside a folder macOS protects, and a browser will not be allowed to run it. \
             The browser would open the connection, dm would never start, and nothing would say \
             why.\n\nInstall dm (make install, or the app in /Applications) and register that copy, \
             or set DM_ENGINE to a copy outside ~/Downloads, ~/Desktop and ~/Documents.",
            exe.display()
        ));
    }

    let mut written = Vec::new();
    for place in places(home) {
        if !place.installed(programs) {
            continue; // that browser is not installed
        }
        let text = serde_json::to_string_pretty(&manifest(place.family, exe, allowed))
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&place.dir)
            .and_then(|()| dm_core::write_atomic(&place.manifest(), format!("{text}\n").as_bytes()))
            .map_err(|e| format!("{}: {e}", place.manifest().display()))?;
        written.push(place.manifest());
    }
    if written.is_empty() {
        return Err("no browser was found to register with".into());
    }
    Ok(written)
}

/// Removes dm's manifest from every browser, installed or not. Returns the
/// files removed.
pub fn unregister(home: &Path) -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        return unregister_windows(&windows_data(home), run_reg);
    }
    places(home)
        .into_iter()
        .map(|place| place.manifest())
        .filter(|file| std::fs::remove_file(file).is_ok())
        .collect()
}

/// Registers this copy of dm with the browsers if they do not already start
/// it, so an installed app works with the extension without a terminal.
///
/// Run each time the installed app opens. A browser whose manifest already
/// names `exe` and allows this version's extension is left alone; one that
/// names another copy (the app moved, or an older install), none, or does
/// not allow this version's extension id (an upgrade across an id change:
/// 0.1.0's Firefox id was `dm@pranavo72bex.github.io`) is written, keeping
/// any extension ids already allowed there — the retired one included,
/// which is harmless and keeps an old copy of the extension working until
/// it is replaced. `None` when nothing needed doing, including when no
/// browser is installed.
pub fn ensure(
    home: &Path,
    exe: &Path,
    programs: &Programs,
) -> Result<Option<Vec<PathBuf>>, String> {
    let files: Vec<(PathBuf, Family)> = if cfg!(target_os = "windows") {
        let data = windows_data(home);
        [Family::Chromium, Family::Firefox]
            .into_iter()
            .map(|family| (windows_manifest(&data, family), family))
            .collect()
    } else {
        places(home)
            .into_iter()
            .filter(|place| place.installed(programs))
            .map(|place| (place.manifest(), place.family))
            .collect()
    };
    if files.is_empty() {
        return Ok(None);
    }
    let mut stale = false;
    let mut known = Vec::new();
    for (file, family) in &files {
        let manifest = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        let Some(manifest) = manifest else {
            stale = true;
            continue;
        };
        if manifest.get("path").and_then(|p| p.as_str()) != exe.to_str() {
            stale = true;
        }
        // Every id dm allows of itself: a manifest written before the Edge
        // Add-ons listing had its id refused the extension from that store.
        let (key, current): (&str, Vec<String>) = match family {
            Family::Chromium => (
                "allowed_origins",
                [CHROME_EXTENSION_ID, EDGE_EXTENSION_ID]
                    .iter()
                    .map(|id| format!("chrome-extension://{id}/"))
                    .collect(),
            ),
            Family::Firefox => ("allowed_extensions", vec![FIREFOX_EXTENSION_ID.to_string()]),
        };
        let allows_current = manifest
            .get(key)
            .and_then(|v| v.as_array())
            .is_some_and(|ids| {
                current
                    .iter()
                    .all(|want| ids.iter().any(|id| id.as_str() == Some(want.as_str())))
            });
        if !allows_current {
            stale = true;
        }
        for id in ids_allowed_by(&manifest) {
            if !known.contains(&id) {
                known.push(id);
            }
        }
    }
    if !stale {
        return Ok(None);
    }
    // An id this version would not accept is dropped rather than failing
    // the registration of the ones it would.
    known.retain(|id| Allowed::with(std::slice::from_ref(id)).is_ok());
    let allowed = Allowed::with(&known)?;
    register(home, exe, &allowed, programs).map(Some)
}

/// [`ensure`] for this user, for the copy of dm that is running — or, in an
/// AppImage, the AppImage file, since the program inside it is mounted at a
/// new place every launch.
pub fn ensure_this_copy() -> Result<Option<Vec<PathBuf>>, String> {
    let exe = match std::env::var_os("APPIMAGE") {
        Some(image) => PathBuf::from(image),
        None => std::env::current_exe()
            .and_then(|exe| exe.canonicalize())
            .map(dm_core::plain_path)
            .map_err(|e| format!("could not tell where this program is: {e}"))?,
    };
    if transient(&exe) {
        return Err(
            "dm is running from its disk image or a temporary copy macOS made of it. Drag dm \
             to Applications and open it from there, so your browser can hand it downloads"
                .into(),
        );
    }
    let home = dirs::home_dir().unwrap_or_default();
    ensure(&home, &exe, &Programs::here(&home))
}

/// The extension ids a manifest allows, as ids: a Chromium manifest names
/// them as origins (`chrome-extension://<id>/`), a Firefox one as ids.
fn ids_allowed_by(manifest: &serde_json::Value) -> Vec<String> {
    ["allowed_origins", "allowed_extensions"]
        .iter()
        .flat_map(|key| {
            manifest
                .get(*key)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
        })
        .filter_map(|v| v.as_str())
        .map(|id| {
            id.trim_start_matches("chrome-extension://")
                .trim_end_matches('/')
                .to_string()
        })
        .collect()
}

/// Every extension that may start dm's host: the ids built into this
/// version, and any the manifests dm wrote allow (read from the files,
/// whether or not their browser is installed, so a manifest a person edited
/// to allow a build of their own counts).
pub fn allowed_ids(home: &Path) -> Vec<String> {
    let mut ids: Vec<String> = [CHROME_EXTENSION_ID, EDGE_EXTENSION_ID, FIREFOX_EXTENSION_ID]
        .iter()
        .map(|id| id.to_string())
        .collect();
    let files: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let data = windows_data(home);
        [Family::Chromium, Family::Firefox]
            .into_iter()
            .map(|family| windows_manifest(&data, family))
            .collect()
    } else {
        places(home)
            .into_iter()
            .map(|place| place.manifest())
            .collect()
    };
    for file in files {
        let manifest = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        for id in manifest.as_ref().map(ids_allowed_by).unwrap_or_default() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Whether this copy of dm may write the browsers' registration: an
/// installed, release dm — not a development build or a test's dm
/// (`DM_HOME`), which must not take the browsers from the installed one.
pub fn may_claim() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("DM_HOME").is_none()
}

/// [`ensure_this_copy`] when this copy [`may_claim`] the browsers, and
/// nothing otherwise. Run at every launch — the one that hands off to a
/// running dm too, before it hands off — and each time the running
/// dm is asked to show itself, so a browser installed since is registered
/// the next time the person opens dm.
pub fn ensure_if_claimed() -> Result<Option<Vec<PathBuf>>, String> {
    if may_claim() {
        ensure_this_copy()
    } else {
        Ok(None)
    }
}

/// What to say about a registration check, given what was last said: a
/// failure is said once, not again at every opening of the window while it
/// stays the same; one that clears is forgotten, so it is said again if it
/// comes back.
pub fn worth_saying(
    last: &mut Option<String>,
    result: Result<Option<Vec<PathBuf>>, String>,
) -> Option<String> {
    match result {
        Ok(_) => {
            *last = None;
            None
        }
        Err(why) if last.as_deref() == Some(why.as_str()) => None,
        Err(why) => {
            *last = Some(why.clone());
            Some(why)
        }
    }
}

/// `--register [id…]` or `--unregister`, run and finished with an exit code;
/// `None` when neither was asked for.
pub fn from_args(args: &[String]) -> Option<i32> {
    let home = dirs::home_dir().unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("--unregister") => {
            let removed = unregister(&home);
            for file in &removed {
                println!("removed {}", file.display());
            }
            if removed.is_empty() {
                println!("dm was not registered with any browser");
            }
            Some(0)
        }
        Some("--register") => {
            let allowed = match Allowed::with(args.get(1..).unwrap_or_default()) {
                Ok(allowed) => allowed,
                Err(why) => {
                    eprintln!("dm: {why}");
                    return Some(2);
                }
            };
            let exe = match std::env::var_os("DM_ENGINE") {
                Some(named) => PathBuf::from(named),
                None => match std::env::current_exe()
                    .and_then(|e| e.canonicalize())
                    .map(dm_core::plain_path)
                {
                    Ok(exe) => exe,
                    Err(e) => {
                        eprintln!("dm: could not tell where this program is: {e}");
                        return Some(1);
                    }
                },
            };
            match register(&home, &exe, &allowed, &Programs::here(&home)) {
                Ok(written) => {
                    for file in &written {
                        println!("registered {}", file.display());
                    }
                    println!(
                        "\nThe browser starts {} for these extensions:\n  {}\n\nRestart the \
                         browser. A download it catches is handed to dm.",
                        exe.display(),
                        allowed
                            .chromium
                            .iter()
                            .chain(&allowed.firefox)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n  ")
                    );
                    Some(0)
                }
                Err(why) => {
                    eprintln!("dm: {why}");
                    Some(1)
                }
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod installed_tests {
    use super::*;

    #[test]
    fn the_browsers_found_are_offered_with_their_stores() {
        let found = installed_where(|bundle, on_path, _| {
            bundle == "Firefox.app" || on_path.contains(&"brave-browser")
        });
        assert_eq!(
            found,
            [
                Installed {
                    name: "Firefox",
                    store: Store::Firefox
                },
                Installed {
                    name: "Brave",
                    store: Store::Chrome
                },
            ]
        );
        assert!(installed_where(|_, _, _| false).is_empty());
        // Windows paths are written with single separators.
        let edge = installed_where(|_, _, under| under == r"Microsoft\Edge\Application\msedge.exe");
        assert_eq!(edge.first().map(|b| b.store), Some(Store::Edge));
    }
}

#[cfg(test)]
mod tests;
