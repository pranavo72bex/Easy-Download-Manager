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

/// Dm registered with Chrome, then Firefox installed. The next
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

/// A Firefox unpacked from its archive into /opt, never run and
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
                serde_json::json!([
                    format!("chrome-extension://{CHROME_EXTENSION_ID}/"),
                    format!("chrome-extension://{EDGE_EXTENSION_ID}/")
                ])
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
/// for a browser that is not there at all.
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

/// Not on Windows, whose keys are written for every browser it knows
/// whether or not it is there (`WINDOWS_KEYS`): a key costs nothing and
/// a browser installed later finds it.
#[cfg(not(windows))]
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
        vec![
            CHROME_EXTENSION_ID.to_string(),
            EDGE_EXTENSION_ID.to_string(),
            store
        ]
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

/// A quarantined dm.app opened where it was downloaded runs from
/// a randomised AppTranslocation copy that is gone by the next launch.
/// An install on an external disk, mounted writable under
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

/// A manifest written before the Edge Add-ons listing allowed only the
/// unpacked build: the next launch adds the store's id.
#[cfg(not(target_os = "windows"))]
#[test]
fn a_manifest_without_the_edge_stores_id_is_rewritten() {
    let home = home_with_browsers();
    let elsewhere = tempfile::tempdir().unwrap();
    let exe = program(elsewhere.path());
    register(
        home.path(),
        &exe,
        &Allowed {
            chromium: vec![CHROME_EXTENSION_ID.into()],
            firefox: vec![FIREFOX_EXTENSION_ID.into()],
        },
    )
    .unwrap();
    ensure(home.path(), &exe)
        .unwrap()
        .expect("left without the Edge store's id");
    let chromium = places(home.path())
        .into_iter()
        .find(|p| p.family == Family::Chromium && p.installed(&Programs::default()))
        .unwrap()
        .manifest();
    let allowed = read(&chromium)["allowed_origins"].to_string();
    assert!(allowed.contains(EDGE_EXTENSION_ID), "{allowed}");
    assert!(allowed.contains(CHROME_EXTENSION_ID), "{allowed}");
    assert_eq!(ensure(home.path(), &exe).unwrap(), None);
}

/// 0.1.0 allowed Firefox's old id. Upgraded in place, the path
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

    let refused = register_windows(data.path(), &exe, &allowed, |_| Err("access denied".into()));
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
