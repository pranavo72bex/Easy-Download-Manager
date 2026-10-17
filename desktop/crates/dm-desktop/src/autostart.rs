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
    set_at(&path, wanted, &program, may_claim()).map(|_| ())
}

/// Whether this copy of dm may point the login item at itself when one
/// already names another copy: an installed, release dm — not a development
/// build or a test's dm (`DM_HOME`), which must not take the login item from
/// the installed one. The rule `register::ensure_this_copy` follows for the
/// browsers' manifests.
pub fn may_claim() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("DM_HOME").is_none()
}

/// Writes or removes the entry at `path`. Returns whether anything changed.
///
/// One of this module's own that starts `program` is left as it is. One
/// that starts another program — the AppImage moved, or dm was reinstalled
/// elsewhere — is rewritten when `claim` says this copy may take it (see
/// [`may_claim`]), and kept otherwise, so a debug build's launch does not
/// retarget it. One without the mark came from an older dm and is replaced
/// whatever `claim` says.
pub fn set_at(path: &Path, wanted: bool, program: &Path, claim: bool) -> Result<bool, String> {
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
        Ok(now) if now.contains(MARK) && !claim => return Ok(false),
        _ => {}
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    dm_core::write_atomic(path, text.as_bytes())
        .map(|()| true)
        .map_err(|e| e.to_string())
}

/// Whether a macOS launch agent's plist starts `program` — the path is one
/// `<string>` of its `ProgramArguments`, whole, so a copy whose path merely
/// contains this one's is not taken for it.
pub fn plist_starts(plist: &str, program: &Path) -> bool {
    plist.contains(&format!("<string>{}</string>", program.to_string_lossy()))
}

/// Whether `reg query`'s output for Windows' Run key shows dm's value
/// starting `program`: the value is `<path> --hidden`, unquoted.
pub fn run_key_starts(query: &str, program: &Path) -> bool {
    let wanted = format!("{} --hidden", program.to_string_lossy());
    query.lines().any(|line| {
        line.split_once("REG_SZ")
            .is_some_and(|(_, value)| value.trim().eq_ignore_ascii_case(&wanted))
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
    fn an_entry_for_another_program_is_rewritten_when_this_copy_may_claim_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("autostart").join("dm.desktop");
        let old = Path::new("/home/a/Old Place/dm.AppImage");
        let new = Path::new("/home/a/Apps/dm.AppImage");
        assert!(set_at(&path, true, old, true).unwrap());
        assert!(
            !set_at(&path, true, old, true).unwrap(),
            "rewritten for nothing"
        );

        // A development build leaves the installed copy's entry alone.
        assert!(!set_at(&path, true, new, false).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(old));

        // The installed copy, moved, takes it back.
        assert!(set_at(&path, true, new, true).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(new));

        // An older dm's entry, without the mark, is replaced either way.
        std::fs::write(&path, "[Desktop Entry]\nExec=/tmp/.mount_x/dm\n").unwrap();
        assert!(set_at(&path, true, new, false).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), entry(new));

        assert!(set_at(&path, false, Path::new(""), true).unwrap());
        assert!(!path.exists());
        assert!(!set_at(&path, false, Path::new(""), true).unwrap());
    }

    #[test]
    fn the_other_systems_login_items_are_read_for_the_program_they_start() {
        let plist = "<array><string>/Applications/dm.app/Contents/MacOS/dm-desktop</string>\
                     <string>--hidden</string></array>";
        assert!(plist_starts(
            plist,
            Path::new("/Applications/dm.app/Contents/MacOS/dm-desktop")
        ));
        assert!(!plist_starts(
            plist,
            Path::new("/Users/a/Applications/dm.app/Contents/MacOS/dm-desktop")
        ));
        assert!(!plist_starts(plist, Path::new("/Applications/dm.app")));

        let query =
            "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    \
                     dm    REG_SZ    C:\\Program Files\\dm\\dm-desktop.exe --hidden\r\n";
        assert!(run_key_starts(
            query,
            Path::new(r"C:\Program Files\dm\dm-desktop.exe")
        ));
        assert!(!run_key_starts(query, Path::new(r"D:\dm\dm-desktop.exe")));
    }

    #[test]
    fn it_is_launched_into_the_tray() {
        let text = entry(Path::new("/usr/bin/dm"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("Type=Application\n"));
        assert!(text.contains(" --hidden\n"));
    }
}
