// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The entry names the icon as the package installed it.
#[test]
fn the_entry_names_the_icon_the_package_installed() {
    let deb = tempfile::tempdir().unwrap();
    let apps = deb.path().join("icons/hicolor/128x128/apps");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("edm-desktop.png"), b"png").unwrap();
    assert_eq!(icon_in(&[deb.path().to_path_buf()]), "edm-desktop");
    // The GPUI window's .deb names it after its binary too.
    let gpui = tempfile::tempdir().unwrap();
    let apps = gpui.path().join("icons/hicolor/128x128/apps");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("edm-gpui.png"), b"png").unwrap();
    assert_eq!(icon_in(&[gpui.path().to_path_buf()]), "edm-gpui");
    assert_eq!(
        icon_in(&[gpui.path().into(), deb.path().into()]),
        "edm-gpui"
    );
    let rpm = tempfile::tempdir().unwrap();
    let apps = rpm.path().join("icons/hicolor/512x512/apps");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("EDM.png"), b"png").unwrap();
    assert_eq!(icon_in(&[rpm.path().to_path_buf()]), "EDM");
    assert_eq!(icon_in(&[deb.path().into(), rpm.path().into()]), "EDM");
    assert_eq!(icon_in(&[]), "EDM");
}

#[test]
fn a_plain_path_is_written_as_it_is() {
    assert!(entry(Path::new("/usr/bin/EDM")).contains("\nExec=/usr/bin/EDM --hidden\n"));
}

#[test]
fn a_path_with_a_space_is_one_argument() {
    assert!(entry(Path::new("/home/a/My Apps/EDM.AppImage"))
        .contains("\nExec=\"/home/a/My Apps/EDM.AppImage\" --hidden\n"));
}

#[test]
fn reserved_characters_are_escaped_twice_and_percent_doubled() {
    // `$` escaped inside the quotes, then that backslash doubled.
    assert_eq!(exec_arg("/opt/$x/EDM"), "\"/opt/\\\\$x/EDM\"");
    assert_eq!(exec_arg("/opt/100%/EDM"), "/opt/100%%/EDM");
    assert_eq!(exec_arg("/opt/a\"b"), "\"/opt/a\\\\\"b\"");
}

#[test]
fn an_entry_for_a_program_that_is_gone_is_rewritten_when_this_copy_may_claim_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("autostart").join("EDM.desktop");
    let old = Path::new("/home/a/Old Place/EDM.AppImage");
    let new = Path::new("/home/a/Apps/EDM.AppImage");
    let gone = |_: &Path| false;
    assert!(set_at(&path, true, old, true, gone).unwrap());
    assert!(
        !set_at(&path, true, old, true, gone).unwrap(),
        "rewritten for nothing"
    );

    // A development build leaves the installed copy's entry alone.
    assert!(!set_at(&path, true, new, false, gone).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(old));

    // Another installed copy that is still there keeps it, so
    // two copies do not take it from each other at every launch.
    assert!(!set_at(&path, true, new, true, |_| true).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(old));

    // The installed copy, moved, takes it back.
    assert!(set_at(&path, true, new, true, gone).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(new));

    // An older EDM's entry, without the mark, is replaced either way.
    std::fs::write(&path, "[Desktop Entry]\nExec=/tmp/.mount_x/EDM\n").unwrap();
    assert!(set_at(&path, true, new, false, |_| true).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(new));

    assert!(set_at(&path, false, Path::new(""), true, gone).unwrap());
    assert!(!path.exists());
    assert!(!set_at(&path, false, Path::new(""), true, gone).unwrap());
}

/// Uninstalling one copy leaves another copy's login item.
#[test]
fn unregister_removes_only_this_copys_entry_or_a_dead_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("EDM.desktop");
    let main = Path::new("/usr/bin/EDM");
    let other = Path::new("/home/a/EDM.AppImage");
    assert_eq!(remove_at(&path, other, |_| true).unwrap(), Removal::None);

    std::fs::write(&path, entry(main)).unwrap();
    assert_eq!(
        remove_at(&path, other, |_| true).unwrap(),
        Removal::Other(main.to_path_buf())
    );
    assert!(path.exists(), "another copy's login item was removed");

    assert_eq!(
        remove_at(&path, other, |_| false).unwrap(),
        Removal::Removed(path.display().to_string()),
        "a login item whose program is gone is removed"
    );
    std::fs::write(&path, entry(main)).unwrap();
    assert!(matches!(
        remove_at(&path, main, |_| true).unwrap(),
        Removal::Removed(_)
    ));
    assert!(!path.exists());
}

#[test]
fn the_program_an_entry_starts_is_read_back_as_it_was_written() {
    for program in [
        "/usr/bin/EDM",
        "/home/a/My Apps/EDM.AppImage",
        "/opt/$x/100%/a\"b`c\\d/EDM",
    ] {
        assert_eq!(
            entry_program(&entry(Path::new(program))).as_deref(),
            Some(Path::new(program)),
            "{program}"
        );
    }
    assert_eq!(entry_program("[Desktop Entry]\nName=EDM\n"), None);
}

#[test]
fn the_other_systems_login_items_are_read_for_the_program_they_start() {
    let plist = "<dict><key>Label</key><string>EDM</string><key>ProgramArguments</key>\
                 <array><string>/Applications/EDM.app/Contents/MacOS/edm-desktop</string>\
                 <string>--hidden</string></array></dict>";
    assert_eq!(
        plist_program(plist).as_deref(),
        Some(Path::new(
            "/Applications/EDM.app/Contents/MacOS/edm-desktop"
        ))
    );
    assert_eq!(plist_program("<dict></dict>"), None);

    assert_eq!(
        run_value_program(r"C:\Program Files\EDM\edm-desktop.exe --hidden").as_deref(),
        Some(Path::new(r"C:\Program Files\EDM\edm-desktop.exe"))
    );
    assert_eq!(
        run_value_program(r#""C:\Program Files\EDM\edm-desktop.exe" --hidden"#).as_deref(),
        Some(Path::new(r"C:\Program Files\EDM\edm-desktop.exe"))
    );
    assert_eq!(run_value_program("  "), None);
}

/// The Run value is read as UTF-16, so a user name outside the
/// console's code page comes back whole.
#[test]
fn a_registry_string_is_decoded_from_utf16_whatever_the_user_is_called() {
    let path = r"C:\Users\Zoë Ñúñez 李\AppData\Local\EDM\edm-desktop.exe --hidden";
    let mut bytes: Vec<u8> = path.encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes.extend([0, 0]);
    let value = utf16_value(&bytes);
    assert_eq!(value, path);
    assert_eq!(
        run_value_program(&value).as_deref(),
        Some(Path::new(
            r"C:\Users\Zoë Ñúñez 李\AppData\Local\EDM\edm-desktop.exe"
        ))
    );
    assert_eq!(utf16_value(&[]), "");
    assert_eq!(utf16_value(&[0x61]), "", "an odd byte is not half a letter");
}

/// An item Task Manager turned off is left off, and the setting
/// follows it; one turned on, or never touched there, follows the
/// setting.
#[test]
fn a_login_item_turned_off_in_task_manager_is_left_off() {
    let on = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let off = [
        0x03, 0, 0, 0, 0x12, 0x9c, 0x3e, 0x5a, 0x1f, 0xd9, 0xda, 0x01,
    ];
    assert!(!turned_off_in_task_manager(&on));
    assert!(!turned_off_in_task_manager(&[0x06, 0, 0, 0]));
    assert!(turned_off_in_task_manager(&off));
    assert!(turned_off_in_task_manager(&[0x07, 0, 0, 0]));
    assert!(!turned_off_in_task_manager(&[]), "never touched there");

    assert_eq!(at_launch(true, true), AtLaunch::TurnedOffOutside);
    assert_eq!(at_launch(true, false), AtLaunch::Follow(true));
    assert_eq!(at_launch(false, true), AtLaunch::Follow(false));
    assert_eq!(at_launch(false, false), AtLaunch::Follow(false));
}

#[test]
fn another_copy_is_taken_over_only_when_it_is_gone() {
    let this = Path::new("/Applications/EDM.app/Contents/MacOS/edm-desktop");
    let other = Path::new("/Users/a/Apps/EDM.app/Contents/MacOS/edm-desktop");
    assert!(!takes_over(this, this, true, |_| false));
    assert!(!takes_over(other, this, true, |_| true));
    assert!(!takes_over(other, this, false, |_| false));
    assert!(takes_over(other, this, true, |_| false));
}

/// A copy run from ~/Downloads (not quarantined, so not
/// translocated) may not become the login item's program.
#[test]
fn a_copy_in_a_protected_folder_or_a_disk_image_is_refused_at_login_on_macos() {
    let home = Path::new("/Users/a");
    let downloads = Path::new("/Users/a/Downloads/EDM.app/Contents/MacOS/edm-desktop");
    let image = Path::new("/Volumes/EDM/EDM.app/Contents/MacOS/edm-desktop");
    let installed = Path::new("/Applications/EDM.app/Contents/MacOS/edm-desktop");
    assert!(refused_at_login(downloads, home, true, |_| false).is_some());
    assert!(refused_at_login(image, home, true, |_| true).is_some());
    assert!(refused_at_login(installed, home, true, |_| false).is_none());
    assert!(refused_at_login(downloads, home, false, |_| false).is_none());
}

#[test]
fn it_is_launched_into_the_tray() {
    let text = entry(Path::new("/usr/bin/EDM"));
    assert!(text.starts_with("[Desktop Entry]\n"));
    assert!(text.contains("Type=Application\n"));
    assert!(text.contains(" --hidden\n"));
}
