//! Start on login, on Linux: an XDG autostart entry, which every desktop
//! that runs a session (GNOME, KDE, Xfce, Cinnamon, MATE, LXQt, and the
//! tools that run one for a bare window manager) starts at login.
//!
//! Written here rather than by the crate that does it for macOS and
//! Windows, which pointed the entry at the running binary unquoted: an
//! AppImage runs from a mount that is gone by the next login, and a path
//! with a space in it was two words.

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

/// Writes or removes the entry, as `wanted` says. One of this module's own
/// is left as it is, so a launch of another copy of dm (a debug build) does
/// not retarget it without the setting being changed.
pub fn set(wanted: bool) -> Result<(), String> {
    let path = entry_path().ok_or("no home directory to put a login entry in")?;
    if !wanted {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    if std::fs::read_to_string(&path).is_ok_and(|text| text.contains(MARK)) {
        return Ok(());
    }
    let text = entry(&program().map_err(|e| e.to_string())?);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    dm_core::write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())
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
    fn it_is_launched_into_the_tray() {
        let text = entry(Path::new("/usr/bin/dm"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("Type=Application\n"));
        assert!(text.contains(" --hidden\n"));
    }
}
