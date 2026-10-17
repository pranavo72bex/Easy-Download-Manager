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
         Icon=dm\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_arg(&program.to_string_lossy())
    )
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

/// The program `reg query`'s output for Windows' Run key shows dm's value
/// starting: the value is `<path> --hidden`, unquoted.
pub fn run_key_program(query: &str) -> Option<PathBuf> {
    query.lines().find_map(|line| {
        let (_, value) = line.split_once("REG_SZ")?;
        let value = value.trim();
        let path = value.strip_suffix(" --hidden").unwrap_or(value).trim();
        (!path.is_empty()).then(|| PathBuf::from(path))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let query =
            "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    \
                     dm    REG_SZ    C:\\Program Files\\dm\\dm-desktop.exe --hidden\r\n";
        assert_eq!(
            run_key_program(query).as_deref(),
            Some(Path::new(r"C:\Program Files\dm\dm-desktop.exe"))
        );
        assert_eq!(run_key_program("ERROR: not found"), None);
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
