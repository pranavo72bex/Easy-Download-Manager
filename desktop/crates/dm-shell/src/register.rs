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

use dm_protocol::{CHROME_EXTENSION_ID, FIREFOX_EXTENSION_ID, NATIVE_HOST};

/// Which kind of manifest a directory wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Chromium,
    Firefox,
}

/// A directory one browser reads host manifests from.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub dir: PathBuf,
    pub family: Family,
    /// What says the browser is installed though it has never been opened:
    /// its programs on `PATH` (Linux), or its bundles in an Applications
    /// folder (macOS).
    pub programs: &'static [&'static str],
}

/// Where installed browsers' programs are looked for.
///
/// A browser installed but never opened has no profile folder, and was not
/// registered: on a fresh desktop, in the order the README gives (install
/// dm, open it, then open the browser and add the extension), the extension
/// found no app until dm's next launch (gap 113).
#[derive(Debug, Clone, Default)]
pub struct Programs {
    /// The directories of `PATH`, where Linux browsers put their programs.
    pub path: Vec<PathBuf>,
    /// The Applications folders, where macOS keeps app bundles.
    pub applications: Vec<PathBuf>,
    /// Where a browser unpacked from an archive lives on Linux, in a folder
    /// of its own named after it (`/opt/firefox/firefox`): one never run
    /// and not on `PATH` was not registered (gap 46).
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

    pub fn manifest(&self) -> PathBuf {
        self.dir.join(format!("{NATIVE_HOST}.json"))
    }
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

/// Which extensions may talk to dm: the two built from this repository, and
/// any named — a Chrome Web Store id, or a signed Firefox add-on's.
#[derive(Debug, Clone, PartialEq)]
pub struct Allowed {
    pub chromium: Vec<String>,
    pub firefox: Vec<String>,
}

impl Allowed {
    pub fn with(named: &[String]) -> Result<Self, String> {
        let mut allowed = Allowed {
            chromium: vec![CHROME_EXTENSION_ID.to_string()],
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
///
/// A disk image is told by being mounted read-only under `/Volumes`
/// (`read_only` says whether the mount holding a path is). An external
/// disk is mounted there too, writable, and an install on it is a place
/// like any other — refusing it left that install with no browser
/// registration and no login item (gap 88).
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
        let (key, current) = match family {
            Family::Chromium => (
                "allowed_origins",
                format!("chrome-extension://{CHROME_EXTENSION_ID}/"),
            ),
            Family::Firefox => ("allowed_extensions", FIREFOX_EXTENSION_ID.to_string()),
        };
        let allows_current = manifest
            .get(key)
            .and_then(|v| v.as_array())
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&current)));
        if !allows_current {
            stale = true;
        }
        for key in ["allowed_origins", "allowed_extensions"] {
            let ids = manifest
                .get(key)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten();
            for id in ids.filter_map(|v| v.as_str()) {
                let id = id
                    .trim_start_matches("chrome-extension://")
                    .trim_end_matches('/')
                    .to_string();
                if !known.contains(&id) {
                    known.push(id);
                }
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

/// Whether this copy of dm may write the browsers' registration: an
/// installed, release dm — not a development build or a test's dm
/// (`DM_HOME`), which must not take the browsers from the installed one.
pub fn may_claim() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("DM_HOME").is_none()
}

/// [`ensure_this_copy`] when this copy [`may_claim`] the browsers, and
/// nothing otherwise. Run at every launch — the one that hands off to a
/// running dm too, before it hands off (gap 131) — and each time the running
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
mod tests {
    use super::*;

    /* Found by profile folder alone, as before: these tests run on
     * computers with real browsers on PATH and in /Applications. */
    fn register(home: &Path, exe: &Path, allowed: &Allowed) -> Result<Vec<PathBuf>, String> {
        super::register(home, exe, allowed, &Programs::default())
    }

    fn ensure(home: &Path, exe: &Path) -> Result<Option<Vec<PathBuf>>, String> {
        super::ensure(home, exe, &Programs::default())
    }

    /// A home with Chrome and Firefox installed, and nothing else.
    fn home_with_browsers() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        for place in places(home.path()) {
            let wanted = place.dir.to_string_lossy();
            if wanted.contains("Google/Chrome/")
                || wanted.contains("google-chrome/")
                || place.family == Family::Firefox
            {
                std::fs::create_dir_all(place.browser()).unwrap();
            }
        }
        home
    }

    fn program(dir: &Path) -> PathBuf {
        let exe = dir.join("bin").join("dm-desktop");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        exe
    }

    fn read(file: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn an_installed_app_registers_itself_once_and_keeps_extra_ids() {
        let home = home_with_browsers();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());

        let written = ensure(home.path(), &exe)
            .unwrap()
            .expect("nothing was registered");
        assert!(!written.is_empty());
        assert_eq!(
            ensure(home.path(), &exe).unwrap(),
            None,
            "registered again for nothing"
        );

        // A Web Store id added by hand survives the app moving.
        let store = "abcdefghijklmnopabcdefghijklmnop".to_string();
        register(
            home.path(),
            &exe,
            &Allowed::with(std::slice::from_ref(&store)).unwrap(),
        )
        .unwrap();
        let moved = program(&elsewhere.path().join("moved"));
        ensure(home.path(), &moved)
            .unwrap()
            .expect("a moved app was not registered");
        for file in &written {
            let manifest = read(file);
            assert_eq!(manifest["path"], moved.to_str().unwrap());
            if let Some(origins) = manifest.get("allowed_origins") {
                assert!(origins.to_string().contains(&store), "{origins}");
            }
        }
    }

    /// Gap 131: dm registered with Chrome, then Firefox installed. The next
    /// check (a second launch, or the running dm asked to show itself)
    /// writes Firefox's manifest and leaves Chrome's as it was.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_browser_installed_after_dm_registered_is_registered_at_the_next_check() {
        let home = tempfile::tempdir().unwrap();
        let chrome: Vec<Place> = places(home.path())
            .into_iter()
            .filter(|p| p.family == Family::Chromium)
            .collect();
        std::fs::create_dir_all(chrome[0].browser()).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        let first = ensure(home.path(), &exe).unwrap().expect("Chrome");
        assert!(first
            .iter()
            .all(|f| !f.to_string_lossy().contains("ozilla")));

        let firefox = places(home.path())
            .into_iter()
            .find(|p| p.family == Family::Firefox)
            .unwrap();
        std::fs::create_dir_all(firefox.browser()).unwrap();
        let second = ensure(home.path(), &exe)
            .unwrap()
            .expect("Firefox, installed since, was not registered");
        assert!(second.contains(&firefox.manifest()), "{second:?}");
        assert_eq!(read(&firefox.manifest())["path"], exe.to_str().unwrap());
        assert_eq!(ensure(home.path(), &exe).unwrap(), None);
    }

    #[test]
    fn a_registration_failure_is_said_once_until_it_changes_or_clears() {
        let mut last = None;
        assert_eq!(
            worth_saying(&mut last, Err("a".into())).as_deref(),
            Some("a")
        );
        assert_eq!(worth_saying(&mut last, Err("a".into())), None);
        assert_eq!(
            worth_saying(&mut last, Err("b".into())).as_deref(),
            Some("b")
        );
        assert_eq!(worth_saying(&mut last, Ok(Some(vec![]))), None);
        assert_eq!(
            worth_saying(&mut last, Err("b".into())).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn a_development_build_does_not_claim_the_browsers() {
        // A debug build (these tests, mostly): the installed dm's
        // registration is kept, and nothing is written.
        if cfg!(debug_assertions) {
            assert!(!may_claim());
            assert_eq!(ensure_if_claimed(), Ok(None));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn firefox_as_a_snap_is_registered_though_it_has_no_mozilla_folder() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("snap/firefox/common")).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        let written = register(home.path(), &exe, &Allowed::with(&[]).unwrap()).unwrap();
        assert_eq!(
            written,
            [home
                .path()
                .join(".mozilla/native-messaging-hosts")
                .join(format!("{NATIVE_HOST}.json"))]
        );
    }

    /// Gap 46: a Firefox unpacked from its archive into /opt, never run and
    /// not on `PATH`, is registered.
    #[cfg(target_os = "linux")]
    #[test]
    fn firefox_unpacked_into_opt_is_registered() {
        let home = tempfile::tempdir().unwrap();
        let opt = tempfile::tempdir().unwrap();
        let firefox = opt.path().join("firefox-128.0");
        std::fs::create_dir_all(&firefox).unwrap();
        std::fs::write(firefox.join("firefox"), b"").unwrap();
        let programs = Programs {
            unpacked: vec![opt.path().to_path_buf()],
            ..Programs::default()
        };
        let mozilla = places(home.path())
            .into_iter()
            .find(|p| p.family == Family::Firefox)
            .unwrap();
        assert!(mozilla.installed(&programs));
        // A folder of that name without the program is not Firefox.
        std::fs::remove_file(firefox.join("firefox")).unwrap();
        assert!(!mozilla.installed(&programs));
        assert!(!mozilla.installed(&Programs::default()));
    }

    #[test]
    fn with_no_browser_installed_there_is_nothing_to_do() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        if !cfg!(target_os = "windows") {
            assert_eq!(ensure(home.path(), &exe).unwrap(), None);
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn each_installed_browser_gets_the_manifest_its_family_reads() {
        let home = home_with_browsers();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        let allowed = Allowed::with(&[]).unwrap();

        let written = register(home.path(), &exe, &allowed).unwrap();
        assert_eq!(written.len(), 2, "{written:?}");

        for place in places(home.path()) {
            let file = place.manifest();
            if !place.installed(&Programs::default()) {
                assert!(!file.exists(), "{} is not installed", place.dir.display());
                continue;
            }
            let manifest = read(&file);
            assert_eq!(manifest["name"], NATIVE_HOST);
            assert_eq!(manifest["type"], "stdio");
            assert_eq!(manifest["path"], exe.to_string_lossy().as_ref());
            match place.family {
                Family::Chromium => assert_eq!(
                    manifest["allowed_origins"],
                    serde_json::json!([format!("chrome-extension://{CHROME_EXTENSION_ID}/")])
                ),
                Family::Firefox => assert_eq!(
                    manifest["allowed_extensions"],
                    serde_json::json!([FIREFOX_EXTENSION_ID])
                ),
            }
        }

        // And it all goes again, whichever family it was.
        let removed = unregister(home.path());
        assert_eq!(removed.len(), 2);
        assert!(written.iter().all(|file| !file.exists()));
        assert!(unregister(home.path()).is_empty());
    }

    #[cfg(not(target_os = "windows"))]
    /// A browser installed but never opened has no profile folder. It is
    /// registered all the same, found by its program, and no folder is made
    /// for a browser that is not there at all (gap 113).
    #[test]
    fn a_browser_installed_but_never_opened_is_registered_by_its_program() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        // Chromium and Firefox installed: on PATH as Linux has them, and as
        // bundles in Applications as macOS has them.
        let bin = elsewhere.path().join("path");
        let apps = elsewhere.path().join("Applications");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["chromium-browser", "firefox"] {
            std::fs::write(bin.join(name), "").unwrap();
        }
        for name in ["Chromium.app", "Firefox.app"] {
            std::fs::create_dir_all(apps.join(name)).unwrap();
        }
        // A folder named like a program is not one.
        std::fs::create_dir_all(bin.join("google-chrome")).unwrap();
        let programs = Programs {
            path: vec![bin],
            applications: vec![apps],
            unpacked: Vec::new(),
        };

        // Nothing has run, and nothing is found without looking for programs.
        assert_eq!(ensure(home.path(), &exe).unwrap(), None);

        let written = super::ensure(home.path(), &exe, &programs)
            .unwrap()
            .expect("an installed browser was not registered");
        assert_eq!(written.len(), 2, "{written:?}");
        let families: Vec<Family> = places(home.path())
            .into_iter()
            .filter(|p| written.contains(&p.manifest()))
            .map(|p| p.family)
            .collect();
        assert_eq!(families, [Family::Chromium, Family::Firefox]);
        for place in places(home.path()) {
            if !written.contains(&place.manifest()) {
                assert!(
                    !place.browser().exists(),
                    "{} was made for a browser that is not installed",
                    place.browser().display()
                );
            }
        }
        assert_eq!(super::ensure(home.path(), &exe, &programs).unwrap(), None);
    }

    #[test]
    fn with_no_browser_installed_nothing_is_written_and_that_is_said() {
        let home = tempfile::tempdir().unwrap();
        let exe = program(home.path());
        let why = register(home.path(), &exe, &Allowed::with(&[]).unwrap()).unwrap_err();
        assert!(why.contains("no browser"), "{why}");
    }

    #[test]
    fn a_program_that_is_not_there_is_not_registered() {
        let home = home_with_browsers();
        let why = register(
            home.path(),
            &home.path().join("nowhere/dm-desktop"),
            &Allowed::with(&[]).unwrap(),
        )
        .unwrap_err();
        assert!(
            why.contains("not a program") || why.contains("Windows"),
            "{why}"
        );
        assert!(places(home.path()).iter().all(|p| !p.manifest().exists()));
    }

    #[test]
    fn a_store_id_is_added_and_anything_else_refused() {
        let store = "abcdefghijklmnopabcdefghijklmnop".to_string();
        let allowed = Allowed::with(&[store.clone(), "dm@example.org".into()]).unwrap();
        assert_eq!(
            allowed.chromium,
            vec![CHROME_EXTENSION_ID.to_string(), store]
        );
        assert_eq!(
            allowed.firefox,
            vec![FIREFOX_EXTENSION_ID.to_string(), "dm@example.org".into()]
        );

        // Naming the built-in ones again does not list them twice.
        let again = Allowed::with(&[CHROME_EXTENSION_ID.into(), FIREFOX_EXTENSION_ID.into()]);
        assert_eq!(again.unwrap(), Allowed::with(&[]).unwrap());

        for wrong in ["", "short", "ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP", "../../x"] {
            assert!(Allowed::with(&[wrong.into()]).is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn the_folders_macos_keeps_from_browsers_are_refused_there_only() {
        let home = Path::new("/Users/someone");
        for refused in [
            "/Users/someone/Downloads/dm/desktop/target/release/dm-desktop",
            "/Users/someone/Desktop/dm-desktop",
            "/Users/someone/Documents/dm/dm-desktop",
        ] {
            assert!(protected(Path::new(refused), home, true), "{refused}");
            assert!(!protected(Path::new(refused), home, false), "{refused}");
        }
        for fine in [
            "/Applications/dm.app/Contents/MacOS/dm-desktop",
            "/Users/someone/.local/bin/dm",
            "/Users/someone/DownloadsArchive/dm-desktop",
        ] {
            assert!(!protected(Path::new(fine), home, true), "{fine}");
        }
    }

    /// Gap 85: a quarantined dm.app opened where it was downloaded runs from
    /// a randomised AppTranslocation copy that is gone by the next launch.
    /// Gap 88: an install on an external disk, mounted writable under
    /// /Volumes, is not a disk image and is not refused.
    #[test]
    fn a_translocated_copy_or_a_disk_image_is_refused_on_macos_only() {
        let translocated =
            Path::new("/private/var/folders/xy/abc/T/AppTranslocation/1F2E-3D4C/d/dm.app/Contents/MacOS/dm-desktop");
        let image = Path::new("/Volumes/dm/dm.app/Contents/MacOS/dm-desktop");
        let external = Path::new("/Volumes/External/Applications/dm.app/Contents/MacOS/dm-desktop");
        assert!(transient_on(translocated, true, |_| false));
        assert!(
            transient_on(image, true, |_| true),
            "a read-only disk image"
        );
        assert!(
            !transient_on(external, true, |_| false),
            "a writable external disk"
        );
        assert!(!transient_on(translocated, false, |_| true));
        assert!(!transient_on(image, false, |_| true));
        assert!(!transient_on(
            Path::new("/Applications/dm.app/Contents/MacOS/dm-desktop"),
            true,
            |_| true
        ));
        // The real flags: this test's own folder is writable.
        assert!(!mounted_read_only(&std::env::temp_dir()));
    }

    /// Gap 84: 0.1.0 allowed Firefox's old id. Upgraded in place, the path
    /// was right, so nothing was rewritten and the extension from Firefox
    /// Add-ons, under its new id, was refused.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_manifest_allowing_only_the_old_firefox_id_is_rewritten() {
        let home = home_with_browsers();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        let old = "dm@pranavo72bex.github.io";
        let written = register(
            home.path(),
            &exe,
            &Allowed {
                chromium: vec![CHROME_EXTENSION_ID.into()],
                firefox: vec![old.into()],
            },
        )
        .unwrap();

        let again = ensure(home.path(), &exe)
            .unwrap()
            .expect("a manifest without the current Firefox id was left as it was");
        assert_eq!(again.len(), written.len());
        let firefox = places(home.path())
            .into_iter()
            .find(|p| p.family == Family::Firefox && p.installed(&Programs::default()))
            .unwrap()
            .manifest();
        let allowed = read(&firefox)["allowed_extensions"].clone();
        assert_eq!(
            allowed,
            serde_json::json!([FIREFOX_EXTENSION_ID, old]),
            "the current id added, the old one kept"
        );
        assert_eq!(ensure(home.path(), &exe).unwrap(), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_program_under_downloads_is_refused_before_anything_is_written() {
        let home = home_with_browsers();
        let exe = program(&home.path().join("Downloads"));
        let why = register(home.path(), &exe, &Allowed::with(&[]).unwrap()).unwrap_err();
        assert!(why.contains("protects"), "{why}");
        assert!(places(home.path()).iter().all(|p| !p.manifest().exists()));
    }

    /// Windows, on any machine: the manifests are written and each browser's
    /// key points at its family's, through the `reg.exe` arguments recorded.
    #[test]
    fn on_windows_every_browser_gets_a_key_naming_its_familys_manifest() {
        let data = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        let allowed = Allowed::with(&[]).unwrap();
        let mut ran: Vec<Vec<String>> = Vec::new();

        let written = register_windows(data.path(), &exe, &allowed, |args| {
            ran.push(args.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(written.len(), 2 + WINDOWS_KEYS.len(), "{written:?}");

        let chromium = windows_manifest(data.path(), Family::Chromium);
        let firefox = windows_manifest(data.path(), Family::Firefox);
        assert_eq!(read(&chromium)["path"], exe.to_string_lossy().as_ref());
        assert!(read(&chromium)["allowed_origins"].is_array());
        assert!(read(&firefox)["allowed_extensions"].is_array());

        assert_eq!(ran.len(), WINDOWS_KEYS.len());
        let chrome = &ran[0];
        assert_eq!(
            chrome,
            &reg_add(r"Software\Google\Chrome\NativeMessagingHosts", &chromium)
        );
        assert_eq!(
            chrome[1],
            format!(r"HKCU\Software\Google\Chrome\NativeMessagingHosts\{NATIVE_HOST}")
        );
        assert_eq!(
            &chrome[2..],
            [
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &chromium.to_string_lossy(),
                "/f"
            ]
        );
        let mozilla = ran.iter().find(|a| a[1].contains("Mozilla")).unwrap();
        assert_eq!(mozilla[6], firefox.to_string_lossy());
    }

    #[test]
    fn on_windows_a_key_that_cannot_be_written_is_said_and_unregistering_removes_all() {
        let data = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let exe = program(elsewhere.path());
        let allowed = Allowed::with(&[]).unwrap();

        let refused =
            register_windows(data.path(), &exe, &allowed, |_| Err("access denied".into()));
        assert_eq!(refused.unwrap_err(), "access denied");

        register_windows(data.path(), &exe, &allowed, |_| Ok(())).unwrap();
        let mut deleted = Vec::new();
        let removed = unregister_windows(data.path(), |args| {
            deleted.push(args.to_vec());
            Ok(())
        });
        assert_eq!(removed.len(), WINDOWS_KEYS.len() + 2, "{removed:?}");
        assert!(deleted.iter().all(|a| a[0] == "delete" && a[2] == "/f"));
        assert!(!windows_manifest(data.path(), Family::Chromium).exists());
        assert!(!windows_manifest(data.path(), Family::Firefox).exists());

        let missing = register_windows(data.path(), Path::new("dm.exe"), &allowed, |_| Ok(()));
        assert!(missing.is_err(), "a relative program is not registered");
    }

    #[test]
    fn only_the_two_flags_are_taken_as_registering() {
        assert_eq!(from_args(&[]), None);
        assert_eq!(from_args(&["--hidden".into()]), None);
        assert_eq!(from_args(&["https://e.com/f.zip".into()]), None);
        // A bad id stops before anything is looked at.
        assert_eq!(
            from_args(&["--register".into(), "not-an-id".into()]),
            Some(2)
        );
    }
}
