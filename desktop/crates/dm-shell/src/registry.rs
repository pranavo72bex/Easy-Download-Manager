// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows' registry, written and read through its own API (gaps row P8). The registration code
//! (`register`) says what it wants in `reg.exe`'s words, which its tests read on every system;
//! [`parse`] turns those words into a [`Call`], and on Windows [`run`] carries it out with no
//! program started. `export` and `import` still run `reg.exe`: the copy dm keeps of another
//! program's key is a `.reg` file in Windows' own format, which a person can open, and only
//! `reg.exe` writes and reads that format whole.

use crate::Error;

pub mod values;

/// The part of the registry a key is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// `HKCU`, the person's own.
    CurrentUser,
    /// `HKLM`, the computer's.
    LocalMachine,
}

/// One value of a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// The key's unnamed value (`/ve`).
    Default,
    /// A named one (`/v name`).
    Named(String),
}

/// What one `reg.exe` argument list asks. Each names its key by its root and the path under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// `add` with no value: the key made where it is missing.
    Make {
        /// The key's root.
        root: Root,
        /// The key's path under its root.
        key: String,
    },
    /// `add` with a value: a string value set, the key made where it is missing.
    Set {
        /// The key's root.
        root: Root,
        /// The key's path under its root.
        key: String,
        /// The value set.
        value: Value,
        /// Its text (`/d`; empty where none is given, as `reg.exe` does).
        data: String,
    },
    /// `delete` with no value: the key and everything under it removed.
    DeleteKey {
        /// The key's root.
        root: Root,
        /// The key's path under its root.
        key: String,
    },
    /// `delete` with a value: that value removed.
    DeleteValue {
        /// The key's root.
        root: Root,
        /// The key's path under its root.
        key: String,
        /// The value removed.
        value: Value,
    },
    /// `query`: whether the key, or one of its values, is there.
    Query {
        /// The key's root.
        root: Root,
        /// The key's path under its root.
        key: String,
        /// The value asked about; `None` asks about the key.
        value: Option<Value>,
    },
    /// `export` or `import`, left to `reg.exe` (see the module's note).
    Program,
}

/// A full key's root and the path under it: `HKCU\Software\x` is the person's `Software\x`.
pub fn split_root(full: &str) -> Result<(Root, String), Error> {
    let (head, rest) = full.split_once('\\').unwrap_or((full, ""));
    let root = match head.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => Root::CurrentUser,
        "HKLM" | "HKEY_LOCAL_MACHINE" => Root::LocalMachine,
        _ => return Err(format!("{full} is not under HKCU or HKLM").into()),
    };
    Ok((root, rest.trim_end_matches('\\').to_string()))
}

/// What `args` (`reg.exe`'s own arguments, the verb first) ask. Only string values are written,
/// as only they are asked for; anything else is refused in words, not guessed at.
pub fn parse(args: &[String]) -> Result<Call, Error> {
    let verb = args.first().map(String::as_str).unwrap_or_default();
    if matches!(verb, "export" | "import") {
        return Ok(Call::Program);
    }
    if !matches!(verb, "add" | "delete" | "query") {
        return Err(format!("\"{verb}\" is not a registry change dm makes").into());
    }
    let (root, key) = split_root(args.get(1).map_or("", String::as_str))?;
    // A root alone is never meant: removing its empty path would empty the whole root.
    if key.is_empty() {
        return Err("a key under HKCU or HKLM is to be named, not the root itself".into());
    }
    let (mut value, mut data) = (None, None);
    let mut rest = args.iter().skip(2);
    while let Some(flag) = rest.next() {
        let mut next = || {
            rest.next()
                .cloned()
                .ok_or_else(|| Error::from(format!("{flag} names nothing")))
        };
        match flag.to_ascii_lowercase().as_str() {
            "/ve" => value = Some(Value::Default),
            "/v" => value = Some(Value::Named(next()?)),
            "/d" => data = Some(next()?),
            "/t" => {
                let kind = next()?;
                if !kind.eq_ignore_ascii_case("REG_SZ") {
                    return Err(format!("{kind} values are not written, only REG_SZ").into());
                }
            }
            "/f" => {}
            _ => return Err(format!("{flag} is not a flag dm uses").into()),
        }
    }
    Ok(match (verb, value) {
        ("add", None) => Call::Make { root, key },
        ("add", Some(value)) => Call::Set {
            root,
            key,
            value,
            data: data.unwrap_or_default(),
        },
        ("delete", None) => Call::DeleteKey { root, key },
        ("delete", Some(value)) => Call::DeleteValue { root, key, value },
        (_, value) => Call::Query { root, key, value },
    })
}

/// Carries out `args` (`reg.exe`'s own arguments): `add`, `delete` and `query` through the
/// registry's API, `export` and `import` by `reg.exe`. An error says the verb and Windows' own
/// words, as `reg.exe`'s did.
#[cfg(windows)]
pub fn run(args: &[String]) -> Result<(), Error> {
    let verb = args.first().map(String::as_str).unwrap_or_default();
    parse(args)
        .and_then(|call| match call {
            Call::Program => program(args),
            call => api::run(&call),
        })
        .map_err(|why| Error::from(format!("reg {verb}: {why}")))
}

/// The string values of a key (`HKLM\…`), in the registry's order; values of other kinds are
/// left out.
#[cfg(windows)]
pub fn strings(full: &str) -> Result<Vec<String>, Error> {
    let (root, key) = split_root(full)?;
    api::strings(root, &key)
}

/// Windows' own `reg.exe`, for the `.reg` files.
#[cfg(windows)]
fn program(args: &[String]) -> Result<(), Error> {
    let out = dm_core::no_window(&mut std::process::Command::new("reg"))
        .args(args)
        .output()
        .map_err(|e| format!("reg.exe could not be run: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().into())
    }
}

#[cfg(windows)]
mod api {
    use super::{Call, Error, Root, Value};
    use windows_sys::Win32::Foundation::{
        ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegEnumValueW,
        RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
        REG_SAM_FLAGS, REG_SZ,
    };

    /// A name as the API takes it: UTF-16 with its closing NUL.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// The value's name, `None` for the key's unnamed one (a null pointer to the API).
    fn value_name(value: &Value) -> Option<Vec<u16>> {
        match value {
            Value::Default => None,
            Value::Named(name) => Some(wide(name)),
        }
    }

    fn pointer(name: Option<&[u16]>) -> *const u16 {
        name.map_or(std::ptr::null(), <[u16]>::as_ptr)
    }

    /// Windows' words for a status other than success.
    fn checked(status: WIN32_ERROR) -> Result<(), Error> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(status as i32)
                .to_string()
                .into())
        }
    }

    fn handle(root: Root) -> HKEY {
        match root {
            Root::CurrentUser => HKEY_CURRENT_USER,
            Root::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    /// An open key, closed when dropped.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by `open` or `create` and is closed once, here.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    impl Key {
        fn open(root: Root, key: &str, access: REG_SAM_FLAGS) -> Result<Key, Error> {
            let name = wide(key);
            let mut opened: HKEY = std::ptr::null_mut();
            // SAFETY: `name` is NUL-terminated and outlives the call; `opened` is ours to write.
            checked(unsafe { RegOpenKeyExW(handle(root), name.as_ptr(), 0, access, &mut opened) })?;
            Ok(Key(opened))
        }

        /// The key, made (with any keys above it) where it is missing.
        fn create(root: Root, key: &str) -> Result<Key, Error> {
            let name = wide(key);
            let mut opened: HKEY = std::ptr::null_mut();
            // SAFETY: `name` is NUL-terminated and outlives the call; no class, no security
            // attributes and no disposition are asked for (null), and `opened` is ours to write.
            checked(unsafe {
                RegCreateKeyExW(
                    handle(root),
                    name.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    std::ptr::null(),
                    &mut opened,
                    std::ptr::null_mut(),
                )
            })?;
            Ok(Key(opened))
        }
    }

    pub(super) fn run(call: &Call) -> Result<(), Error> {
        match call {
            Call::Make { root, key } => Key::create(*root, key).map(drop),
            Call::Set {
                root,
                key,
                value,
                data,
            } => {
                let opened = Key::create(*root, key)?;
                let name = value_name(value);
                let data = wide(data);
                let bytes = u32::try_from(data.len() * 2).map_err(|e| e.to_string())?;
                // SAFETY: the key is open for setting values; the name is null or
                // NUL-terminated, and `data` holds `bytes` bytes, its NUL among them.
                checked(unsafe {
                    RegSetValueExW(
                        opened.0,
                        pointer(name.as_deref()),
                        0,
                        REG_SZ,
                        data.as_ptr().cast(),
                        bytes,
                    )
                })
            }
            Call::DeleteKey { root, key } => {
                let name = wide(key);
                // SAFETY: `name` is NUL-terminated and outlives the call.
                checked(unsafe { RegDeleteTreeW(handle(*root), name.as_ptr()) })
            }
            Call::DeleteValue { root, key, value } => {
                let opened = Key::open(*root, key, KEY_SET_VALUE)?;
                let name = value_name(value);
                // SAFETY: the key is open for setting values; the name is null or
                // NUL-terminated and outlives the call.
                checked(unsafe { RegDeleteValueW(opened.0, pointer(name.as_deref())) })
            }
            Call::Query { root, key, value } => match value {
                None => Key::open(*root, key, KEY_READ).map(drop),
                Some(value) => {
                    let opened = Key::open(*root, key, KEY_QUERY_VALUE)?;
                    let name = value_name(value);
                    // SAFETY: only whether the value is there is asked: no type, no data and
                    // no size (all null, which the API allows together).
                    checked(unsafe {
                        RegQueryValueExW(
                            opened.0,
                            pointer(name.as_deref()),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        )
                    })
                }
            },
            Call::Program => Err("export and import are reg.exe's".into()),
        }
    }

    /// Every string value of a key, read one by one until the registry says there are no more.
    pub(super) fn strings(root: Root, key: &str) -> Result<Vec<String>, Error> {
        let opened = Key::open(root, key, KEY_QUERY_VALUE)?;
        // A value's name is at most 16,383 characters; data grows as a value asks.
        let mut name = vec![0u16; 16_384];
        let mut data = vec![0u8; 4096];
        let mut found = Vec::new();
        let mut index = 0;
        let mut grown = 0;
        loop {
            let mut name_len = name.len() as u32;
            let mut data_len = data.len() as u32;
            let mut kind = 0u32;
            // SAFETY: `name` holds `name_len` characters and `data` `data_len` bytes, both ours;
            // the API writes no more than each says.
            let status = unsafe {
                RegEnumValueW(
                    opened.0,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    &mut kind,
                    data.as_mut_ptr(),
                    &mut data_len,
                )
            };
            match status {
                ERROR_NO_MORE_ITEMS => return Ok(found),
                // The value is larger than the buffer: the same one asked again with room,
                // a few times, as it can grow between two asks.
                ERROR_MORE_DATA if grown < 4 => {
                    grown += 1;
                    data.resize((data_len as usize).max(data.len() * 2), 0);
                    continue;
                }
                ERROR_SUCCESS => {
                    if kind == REG_SZ {
                        let units: Vec<u16> = data[..data_len as usize]
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| u16::from_le_bytes(*pair))
                            .collect();
                        let text = String::from_utf16_lossy(&units);
                        found.push(text.trim_end_matches('\0').to_string());
                    }
                }
                other => checked(other)?,
            }
            index += 1;
            grown = 0;
        }
    }
}

#[cfg(test)]
mod tests;
