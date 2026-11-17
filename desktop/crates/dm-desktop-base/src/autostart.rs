// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
/// packages, the binary's name by the .deb (`dm-gpui` since the GPUI window,
/// `dm-desktop` before). The entry named `dm` on every system, and a
/// startup-applications list showed the .deb's without an icon. `dm` where
/// none is found (an AppImage).
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
    if has("dm") {
        "dm"
    } else if has("dm-gpui") {
        "dm-gpui"
    } else if has("dm-desktop") {
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
/// Desktop, Documents: the same rule as the browsers' manifests), a
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
/// AppImage) would otherwise take it from each other at every launch.
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
/// still there, which uninstalling this one should not take away.
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
/// a user name outside it.
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
    /// enable it again at every launch, over the person's choice.
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

/// The current user's registry, read through its own API.
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
mod tests;
