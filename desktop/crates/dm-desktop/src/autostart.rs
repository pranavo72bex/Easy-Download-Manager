//! Start on login, on Linux: an XDG autostart entry, which every desktop
//! that runs a session (GNOME, KDE, Xfce, Cinnamon, MATE, LXQt, and the
//! tools that run one for a bare window manager) starts at login.
//!
//! Written here rather than by the crate that does it for macOS and
//! Windows, which pointed the entry at the running binary unquoted: an
//! AppImage runs from a mount that is gone by the next login, and a path
//! with a space in it was two words.
//!
//! Also the helpers `main` uses to tell whether the other systems' login
//! items (written by `auto-launch`) still start this program.

use std::path::{Path, PathBuf};

/// Where the entry goes: `$XDG_CONFIG_HOME/autostart`, or `~/.config/autostart`.
pub fn entry_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart").join("dm.desktop"))
}

/// The program a login should start: the AppImage itself when run from
/// one (its own binary lives on a mount that ends with it), else this one.
pub fn program() -> std::io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        Some(image) => Ok(PathBuf::from(image)),
        None => std::env::current_exe(),
    }
}

/// The entry for `program`, launched into the tray.
pub fn entry(program: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=dm\n\
         {MARK}\
         Exec={} --hidden\n\
         Icon={}\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_arg(&program.to_string_lossy()),
        icon_in(&data_dirs())
    )
}

/// The name dm's icon is installed under: `dm` by the .rpm and Arch
/// packages, `dm-desktop` (the binary's name) by the .deb. The entry named
/// `dm` on every system, and a startup-applications list showed the .deb's
/// without an icon (round 12). `dm` where neither is found (an AppImage).
pub fn icon_in(data_dirs: &[PathBuf]) -> &'static str {
    let has = |name: &str| {
        data_dirs.iter().any(|dir| {
            let themed = dir.join("icons").join("hicolor");
            std::fs::read_dir(&themed).is_ok_and(|sizes| {
                sizes.flatten().any(|size| {
                    size.path()
                        .join("apps")
                        .join(format!("{name}.png"))
                        .is_file()
                })
            }) || dir.join("pixmaps").join(format!("{name}.png")).is_file()
        })
    };
    if !has("dm") && has("dm-desktop") {
        "dm-desktop"
    } else {
        "dm"
    }
}

/// Where icons are looked for, as the XDG base-directory spec says.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .into_iter()
        .collect();
    let system = std::env::var_os("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&system));
    dirs
}

/// One argument as the desktop-entry spec has an `Exec` line read it:
/// quoted when it has to be, with `"`, `` ` ``, `$` and `\` escaped inside
/// the quotes, then every `\` doubled again (the value is itself an escaped
/// string), and `%` written `%%` so it is not taken for a field code.
fn exec_arg(arg: &str) -> String {
    let needs_quotes = arg.is_empty()
        || arg.chars().any(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\''
                        | '\\'
                        | '>'
                        | '<'
                        | '~'
                        | '|'
                        | '&'
                        | ';'
                        | '$'
                        | '*'
                        | '?'
                        | '#'
                        | '('
                        | ')'
                        | '`'
                )
        });
    let arg = if needs_quotes {
        let mut quoted = String::from('"');
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted
    } else {
        arg.to_string()
    };
    arg.replace('\\', "\\\\").replace('%', "%%")
}

/// Marks an entry as written by this module: one without it came from an
/// older dm, possibly pointing into an AppImage's vanished mount, and is
/// replaced.
const MARK: &str = "Comment=Download manager, waiting in the tray\n";

/// Writes or removes the entry, as `wanted` says. See [`set_at`].
pub fn set(wanted: bool) -> Result<(), String> {
    let path = entry_path().ok_or("no home directory to put a login entry in")?;
    let program = if wanted {
        program().map_err(|e| e.to_string())?
    } else {
        PathBuf::new()
    };
    set_at(&path, wanted, &program, may_claim(), |p| p.exists()).map(|_| ())
}

/// Whether this copy of dm may point the login item at itself when one
/// already names another copy that is gone: an installed, release dm — not
/// a development build or a test's dm (`DM_HOME`), which must not take the
/// login item from the installed one. The rule `register::ensure_this_copy`
/// follows for the browsers' manifests.
pub fn may_claim() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("DM_HOME").is_none()
}

/// Why `exe` may not be the login item's program, if it may not: on macOS a
/// copy in a folder browsers and launchd are kept out of (Downloads,
/// Desktop, Documents: the same rule as the browsers' manifests, gap 89), a
/// disk image or an AppTranslocation copy, gone by the next login.
pub fn refused_at_login(
    exe: &Path,
    home: &Path,
    macos: bool,
    read_only: impl FnOnce(&Path) -> bool,
) -> Option<String> {
    if dm_shell::register::transient_on(exe, macos, read_only) {
        return Some(format!(
            "{} is a copy macOS runs from a disk image or a temporary copy it made, gone by \
             the next login. Move dm to Applications and open it from there",
            exe.display()
        ));
    }
    if dm_shell::register::protected(exe, home, macos) {
        return Some(format!(
            "{} is inside Downloads, Desktop or Documents, which macOS keeps programs started \
             at login out of. Move dm to Applications and open it from there",
            exe.display()
        ));
    }
    None
}

/// Whether the login item, found starting `found`, should be pointed at
/// `this` instead. Only when this copy may claim it and the program it
/// starts is gone — the app moved, or was removed and installed elsewhere.
/// Not merely because it differs: two installed copies (a .deb and an
/// AppImage) would otherwise take it from each other at every launch
/// (gap 93).
pub fn takes_over(
    found: &Path,
    this: &Path,
    claim: bool,
    exists: impl FnOnce(&Path) -> bool,
) -> bool {
    !same_program(found, this) && claim && !exists(found)
}

/// Whether `--unregister` of `this` copy removes a login item found
/// starting `found` (`None`: its program could not be read). Its own, one
/// whose program is gone, or one it cannot read; not another copy's that is
/// still there, which uninstalling this one should not take away (gap 91).
pub fn removes(found: Option<&Path>, this: &Path, exists: impl FnOnce(&Path) -> bool) -> bool {
    match found {
        None => true,
        Some(found) => same_program(found, this) || !exists(found),
    }
}

/// One program, named twice. Windows' paths are compared without case.
pub fn same_program(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

/// Writes or removes the entry at `path`. Returns whether anything changed.
///
/// One of this module's own that starts `program` is rewritten only when
/// its text is not this version's. One that starts another program is
/// rewritten when [`takes_over`] says so, and kept otherwise. One without
/// the mark came from an older dm and is replaced.
pub fn set_at(
    path: &Path,
    wanted: bool,
    program: &Path,
    claim: bool,
    exists: impl FnOnce(&Path) -> bool,
) -> Result<bool, String> {
    if !wanted {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(format!("{}: {e}", path.display())),
        };
    }
    let text = entry(program);
    match std::fs::read_to_string(path) {
        Ok(now) if now == text => return Ok(false),
        Ok(now) if now.contains(MARK) => match entry_program(&now) {
            Some(found) if same_program(&found, program) => {}
            Some(found) if !takes_over(&found, program, claim, exists) => return Ok(false),
            Some(_) => {}
            None if !claim => return Ok(false),
            None => {}
        },
        _ => {}
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    dm_core::write_atomic(path, text.as_bytes())
        .map(|()| true)
        .map_err(|e| e.to_string())
}

/// What `--unregister` did with the login item.
#[derive(Debug, PartialEq)]
pub enum Removal {
    /// There was none.
    None,
    /// Removed; what it was.
    Removed(String),
    /// It starts another copy of dm, still there, and was left alone.
    Other(PathBuf),
}

/// `--unregister` on Linux: the entry at `path`, removed if [`removes`]
/// says so.
pub fn remove_at(
    path: &Path,
    this: &Path,
    exists: impl FnOnce(&Path) -> bool,
) -> Result<Removal, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Removal::None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let found = entry_program(&text);
    if let Some(found) = found.as_deref().filter(|f| !removes(Some(f), this, exists)) {
        return Ok(Removal::Other(found.to_path_buf()));
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(Removal::Removed(path.display().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Removal::None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The program a desktop entry's `Exec` line starts: [`exec_arg`] undone
/// on its first argument.
pub fn entry_program(text: &str) -> Option<PathBuf> {
    let line = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
    // The value's own escapes, then `%%`.
    let mut value = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => value.push(chars.next()?),
            '%' if chars.peek() == Some(&'%') => {
                chars.next();
                value.push('%');
            }
            c => value.push(c),
        }
    }
    let mut chars = value.chars();
    let mut arg = String::new();
    if value.starts_with('"') {
        chars.next();
        loop {
            match chars.next()? {
                '\\' => arg.push(chars.next()?),
                '"' => break,
                c => arg.push(c),
            }
        }
    } else {
        arg = value.split(' ').next()?.to_string();
    }
    (!arg.is_empty()).then(|| PathBuf::from(arg))
}

/// The program a macOS launch agent's plist starts: the first `<string>` of
/// its `ProgramArguments`, as `auto-launch` writes it.
pub fn plist_program(plist: &str) -> Option<PathBuf> {
    let (_, after) = plist.split_once("<key>ProgramArguments</key>")?;
    let (_, after) = after.split_once("<string>")?;
    let (path, _) = after.split_once("</string>")?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Where Windows keeps the login items, under the current user's hive.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Where Task Manager's Startup tab records an item turned on or off.
pub const STARTUP_APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The program Windows' Run value for dm starts: `<path> --hidden`, as
/// `auto-launch` writes it (unquoted), or quoted.
pub fn run_value_program(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let path = value.strip_suffix("--hidden").unwrap_or(value).trim();
    let path = path
        .strip_prefix('"')
        .and_then(|p| p.strip_suffix('"'))
        .unwrap_or(path);
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// A registry string as the registry API hands it over: UTF-16, little
/// endian, usually ending in a NUL. Read this way rather than from
/// `reg query`'s text, which comes in the console's code page and garbled
/// a user name outside it (gap 99).
pub fn utf16_value(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(units.get(..end).unwrap_or_default())
}

/// Whether Task Manager's StartupApproved value says the item was turned
/// off: its first byte is 2 or 6 when on, 3 or 7 (odd) when off. No value
/// means never touched there, which is on.
pub fn turned_off_in_task_manager(approved: &[u8]) -> bool {
    approved.first().is_some_and(|first| first & 1 == 1)
}

/// What the login item does at launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtLaunch {
    /// Made to match the saved setting.
    Follow(bool),
    /// The setting is on, but the person turned the item off outside dm
    /// (Task Manager): left off, and the setting turned off to say so.
    /// `auto-launch` reads such an item as not enabled, and dm used to
    /// enable it again at every launch, over the person's choice (gap 96).
    TurnedOffOutside,
}

/// The login item's course at launch, from the saved setting and whether
/// the system says it was turned off outside dm.
pub fn at_launch(wanted: bool, turned_off_outside: bool) -> AtLaunch {
    if wanted && turned_off_outside {
        AtLaunch::TurnedOffOutside
    } else {
        AtLaunch::Follow(wanted)
    }
}

/// The current user's registry, read through its own API (gaps 96, 99).
#[cfg(windows)]
pub mod registry {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS,
        RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// `name` under `root`\`subkey`, of the type `flags` allows. `None`
    /// when it is not there or cannot be read.
    fn read(root: HKEY, subkey: &str, name: &str, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
        let (subkey, name) = (wide(subkey), wide(name));
        let mut size: u32 = 0;
        // A value that grows between asking its size and reading it is
        // asked about again, a few times.
        for _ in 0..4 {
            let mut buffer = vec![0u8; size as usize];
            let data = if buffer.is_empty() {
                std::ptr::null_mut()
            } else {
                buffer.as_mut_ptr().cast()
            };
            // SAFETY: both names are NUL-terminated and outlive the call;
            // `data` is null (asking only the size) or `size` bytes we own.
            let status = unsafe {
                RegGetValueW(
                    root,
                    subkey.as_ptr(),
                    name.as_ptr(),
                    flags,
                    std::ptr::null_mut(),
                    data,
                    &mut size,
                )
            };
            match status {
                ERROR_SUCCESS if !buffer.is_empty() || size == 0 => {
                    buffer.truncate(size as usize);
                    return Some(buffer);
                }
                ERROR_SUCCESS | ERROR_MORE_DATA => continue,
                _ => return None,
            }
        }
        None
    }

    /// A string value, decoded from UTF-16.
    pub fn string(subkey: &str, name: &str) -> Option<String> {
        read(HKEY_CURRENT_USER, subkey, name, RRF_RT_REG_SZ).map(|bytes| super::utf16_value(&bytes))
    }

    /// A string value of the computer's own (HKLM), decoded from UTF-16.
    pub fn machine_string(subkey: &str, name: &str) -> Option<String> {
        read(HKEY_LOCAL_MACHINE, subkey, name, RRF_RT_REG_SZ)
            .map(|bytes| super::utf16_value(&bytes))
    }

    /// A binary value's bytes.
    pub fn binary(subkey: &str, name: &str) -> Option<Vec<u8>> {
        read(HKEY_CURRENT_USER, subkey, name, RRF_RT_REG_BINARY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry names the icon as the package installed it.
    #[test]
    fn the_entry_names_the_icon_the_package_installed() {
        let deb = tempfile::tempdir().unwrap();
        let apps = deb.path().join("icons/hicolor/128x128/apps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("dm-desktop.png"), b"png").unwrap();
        assert_eq!(icon_in(&[deb.path().to_path_buf()]), "dm-desktop");
        let rpm = tempfile::tempdir().unwrap();
        let apps = rpm.path().join("icons/hicolor/512x512/apps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("dm.png"), b"png").unwrap();
        assert_eq!(icon_in(&[rpm.path().to_path_buf()]), "dm");
        assert_eq!(icon_in(&[deb.path().into(), rpm.path().into()]), "dm");
        assert_eq!(icon_in(&[]), "dm");
    }

    #[test]
    fn a_plain_path_is_written_as_it_is() {
        assert!(entry(Path::new("/usr/bin/dm")).contains("\nExec=/usr/bin/dm --hidden\n"));
    }

    #[test]
    fn a_path_with_a_space_is_one_argument() {
        assert!(entry(Path::new("/home/a/My Apps/dm.AppImage"))
            .contains("\nExec=\"/home/a/My Apps/dm.AppImage\" --hidden\n"));
    }

    #[test]
    fn reserved_characters_are_escaped_twice_and_percent_doubled() {
        // `$` escaped inside the quotes, then that backslash doubled.
        assert_eq!(exec_arg("/opt/$x/dm"), "\"/opt/\\\\$x/dm\"");
        assert_eq!(exec_arg("/opt/100%/dm"), "/opt/100%%/dm");
        assert_eq!(exec_arg("/opt/a\"b"), "\"/opt/a\\\\\"b\"");
    }

    #[test]
    fn an_entry_for_a_program_that_is_gone_is_rewritten_when_this_copy_may_claim_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("autostart").join("dm.desktop");
        let old = Path::new("/home/a/Old Place/dm.AppImage");
        let new = Path::new("/home/a/Apps/dm.AppImage");
        let gone = |_: &Path| false;
        assert!(set_at(&path, true, old, true, gone).unwrap());
        assert!(
            !set_at(&path, true, old, true, gone).unwrap(),
            "rewritten for nothing"
        );

        // A development build leaves the installed copy's entry alone.
        assert!(!set_at(&path, true, new, false, gone).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(old));

        // Gap 93: another installed copy that is still there keeps it, so
        // two copies do not take it from each other at every launch.
        assert!(!set_at(&path, true, new, true, |_| true).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(old));

        // The installed copy, moved, takes it back.
        assert!(set_at(&path, true, new, true, gone).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(new));

        // An older dm's entry, without the mark, is replaced either way.
        std::fs::write(&path, "[Desktop Entry]\nExec=/tmp/.mount_x/dm\n").unwrap();
        assert!(set_at(&path, true, new, false, |_| true).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(new));

        assert!(set_at(&path, false, Path::new(""), true, gone).unwrap());
        assert!(!path.exists());
        assert!(!set_at(&path, false, Path::new(""), true, gone).unwrap());
    }

    /// Gap 91: uninstalling one copy leaves another copy's login item.
    #[test]
    fn unregister_removes_only_this_copys_entry_or_a_dead_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dm.desktop");
        let main = Path::new("/usr/bin/dm");
        let other = Path::new("/home/a/dm.AppImage");
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
            "/usr/bin/dm",
            "/home/a/My Apps/dm.AppImage",
            "/opt/$x/100%/a\"b`c\\d/dm",
        ] {
            assert_eq!(
                entry_program(&entry(Path::new(program))).as_deref(),
                Some(Path::new(program)),
                "{program}"
            );
        }
        assert_eq!(entry_program("[Desktop Entry]\nName=dm\n"), None);
    }

    #[test]
    fn the_other_systems_login_items_are_read_for_the_program_they_start() {
        let plist = "<dict><key>Label</key><string>dm</string><key>ProgramArguments</key>\
                     <array><string>/Applications/dm.app/Contents/MacOS/dm-desktop</string>\
                     <string>--hidden</string></array></dict>";
        assert_eq!(
            plist_program(plist).as_deref(),
            Some(Path::new("/Applications/dm.app/Contents/MacOS/dm-desktop"))
        );
        assert_eq!(plist_program("<dict></dict>"), None);

        assert_eq!(
            run_value_program(r"C:\Program Files\dm\dm-desktop.exe --hidden").as_deref(),
            Some(Path::new(r"C:\Program Files\dm\dm-desktop.exe"))
        );
        assert_eq!(
            run_value_program(r#""C:\Program Files\dm\dm-desktop.exe" --hidden"#).as_deref(),
            Some(Path::new(r"C:\Program Files\dm\dm-desktop.exe"))
        );
        assert_eq!(run_value_program("  "), None);
    }

    /// Gap 99: the Run value is read as UTF-16, so a user name outside the
    /// console's code page comes back whole.
    #[test]
    fn a_registry_string_is_decoded_from_utf16_whatever_the_user_is_called() {
        let path = r"C:\Users\Zoë Ñúñez 李\AppData\Local\dm\dm-desktop.exe --hidden";
        let mut bytes: Vec<u8> = path.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes.extend([0, 0]);
        let value = utf16_value(&bytes);
        assert_eq!(value, path);
        assert_eq!(
            run_value_program(&value).as_deref(),
            Some(Path::new(
                r"C:\Users\Zoë Ñúñez 李\AppData\Local\dm\dm-desktop.exe"
            ))
        );
        assert_eq!(utf16_value(&[]), "");
        assert_eq!(utf16_value(&[0x61]), "", "an odd byte is not half a letter");
    }

    /// Gap 96: an item Task Manager turned off is left off, and the setting
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
        let this = Path::new("/Applications/dm.app/Contents/MacOS/dm-desktop");
        let other = Path::new("/Users/a/Apps/dm.app/Contents/MacOS/dm-desktop");
        assert!(!takes_over(this, this, true, |_| false));
        assert!(!takes_over(other, this, true, |_| true));
        assert!(!takes_over(other, this, false, |_| false));
        assert!(takes_over(other, this, true, |_| false));
    }

    /// Gap 89: a copy run from ~/Downloads (not quarantined, so not
    /// translocated) may not become the login item's program.
    #[test]
    fn a_copy_in_a_protected_folder_or_a_disk_image_is_refused_at_login_on_macos() {
        let home = Path::new("/Users/a");
        let downloads = Path::new("/Users/a/Downloads/dm.app/Contents/MacOS/dm-desktop");
        let image = Path::new("/Volumes/dm/dm.app/Contents/MacOS/dm-desktop");
        let installed = Path::new("/Applications/dm.app/Contents/MacOS/dm-desktop");
        assert!(refused_at_login(downloads, home, true, |_| false).is_some());
        assert!(refused_at_login(image, home, true, |_| true).is_some());
        assert!(refused_at_login(installed, home, true, |_| false).is_none());
        assert!(refused_at_login(downloads, home, false, |_| false).is_none());
    }

    #[test]
    fn it_is_launched_into_the_tray() {
        let text = entry(Path::new("/usr/bin/dm"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("Type=Application\n"));
        assert!(text.contains(" --hidden\n"));
    }
}
