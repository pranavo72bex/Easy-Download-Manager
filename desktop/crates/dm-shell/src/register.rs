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
}

impl Place {
    /// The browser's own directory, which exists when it is installed.
    fn browser(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    pub fn manifest(&self) -> PathBuf {
        self.dir.join(format!("{NATIVE_HOST}.json"))
    }
}

/// Every directory a browser on this system reads host manifests from, under
/// the home directory `home`. A manifest is written to each one whose
/// browser is installed, because someone with Chrome and Brave means both.
pub fn places(home: &Path) -> Vec<Place> {
    let (chromium, firefox): (Vec<PathBuf>, Vec<PathBuf>) = if cfg!(target_os = "macos") {
        let support = home.join("Library/Application Support");
        (
            [
                "Google/Chrome",
                "Google/Chrome Beta",
                "Google/Chrome Dev",
                "Google/Chrome Canary",
                "Google/Chrome for Testing",
                "Chromium",
                "BraveSoftware/Brave-Browser",
                "Microsoft Edge",
                "Vivaldi",
                "com.operasoftware.Opera",
            ]
            .iter()
            .map(|b| support.join(b).join("NativeMessagingHosts"))
            .collect(),
            vec![support.join("Mozilla/NativeMessagingHosts")],
        )
    } else if cfg!(target_os = "windows") {
        // Windows reads no manifest directories: see `WINDOWS_KEYS`.
        (Vec::new(), Vec::new())
    } else {
        let config = home.join(".config");
        (
            [
                "google-chrome",
                "google-chrome-beta",
                "google-chrome-unstable",
                "google-chrome-for-testing",
                "chromium",
                "BraveSoftware/Brave-Browser",
                "microsoft-edge",
                "vivaldi",
                "opera",
            ]
            .iter()
            .map(|b| config.join(b).join("NativeMessagingHosts"))
            .collect(),
            vec![home.join(".mozilla/native-messaging-hosts")],
        )
    };
    chromium
        .into_iter()
        .map(|dir| Place {
            dir,
            family: Family::Chromium,
        })
        .chain(firefox.into_iter().map(|dir| Place {
            dir,
            family: Family::Firefox,
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

/// Writes a manifest for every installed browser, naming `exe` and the
/// extensions `allowed`. Returns the files written.
pub fn register(home: &Path, exe: &Path, allowed: &Allowed) -> Result<Vec<PathBuf>, String> {
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
        if !place.browser().is_dir() {
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
/// names `exe` is left alone; one that names another copy (the app moved,
/// or an older install) or none is written, keeping any extension ids
/// already allowed there. `None` when nothing needed doing, including when
/// no browser is installed.
pub fn ensure(home: &Path, exe: &Path) -> Result<Option<Vec<PathBuf>>, String> {
    let files: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let data = windows_data(home);
        [Family::Chromium, Family::Firefox]
            .into_iter()
            .map(|family| windows_manifest(&data, family))
            .collect()
    } else {
        places(home)
            .into_iter()
            .filter(|place| place.browser().is_dir())
            .map(|place| place.manifest())
            .collect()
    };
    if files.is_empty() {
        return Ok(None);
    }
    let mut stale = false;
    let mut known = Vec::new();
    for file in &files {
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
    register(home, exe, &allowed).map(Some)
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
    if cfg!(target_os = "macos") && exe.starts_with("/Volumes") {
        return Err(
            "dm is running from its disk image. Drag dm to Applications and open it \
                    from there, so your browser can hand it downloads"
                .into(),
        );
    }
    ensure(&dirs::home_dir().unwrap_or_default(), &exe)
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
            match register(&home, &exe, &allowed) {
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
            if !place.browser().is_dir() {
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
