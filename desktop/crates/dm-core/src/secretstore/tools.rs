// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use base64::Engine as _;

use super::SecretStore;

pub struct ToolStore {
    /// macOS: the keychain file every command names, where it is not the account's default
    /// (a test's own). Unused on Linux, where the session's Secret Service is the store.
    keychain: Option<String>,
}

impl ToolStore {
    /// The account's own store.
    pub const SYSTEM: ToolStore = ToolStore { keychain: None };
}

#[cfg(target_os = "macos")]
const TOOL: &str = "security";
#[cfg(target_os = "linux")]
const TOOL: &str = "secret-tool";

/// Whether the tool can be started at all (a lookup of a name nobody keeps).
pub fn present() -> bool {
    run(&ToolStore::SYSTEM.lookup_args("dm:probe"), None).is_ok()
}

impl ToolStore {
    /// `args`, then the keychain named, if one is.
    #[cfg(target_os = "macos")]
    fn in_keychain(&self, args: &[&str]) -> Vec<String> {
        let mut all: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        all.extend(self.keychain.clone());
        all
    }

    #[cfg(target_os = "macos")]
    fn lookup_args(&self, name: &str) -> Vec<String> {
        self.in_keychain(&[
            "find-generic-password",
            "-a",
            "dm",
            "-s",
            &keyed(name),
            "-w",
        ])
    }
    #[cfg(target_os = "linux")]
    fn lookup_args(&self, name: &str) -> Vec<String> {
        let _ = &self.keychain;
        ["lookup", "service", "dm", "name", name]
            .map(String::from)
            .to_vec()
    }
}

/// A word `security -i` cannot mis-split: no space, line break or quote.
#[cfg(target_os = "macos")]
fn plain(word: &str) -> bool {
    !word
        .chars()
        .any(|c| c.is_whitespace() || "\"'\\".contains(c))
}

/// The name an item is kept under in the keychain: dm's own name where it is plain, and
/// otherwise the name spelt in base64url. A Sign-in's name holds a line break between the
/// site and the user (`settings/secrets.rs`), which `security -i` would split, so every
/// Sign-in's password was refused and stayed in the settings file (security review,
/// 2026-11-07). Plain names, the proxy's among them, are kept as they always were.
#[cfg(target_os = "macos")]
fn keyed(name: &str) -> std::borrow::Cow<'_, str> {
    if plain(name) {
        name.into()
    } else {
        format!(
            "dm:b64:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(name)
        )
        .into()
    }
}

/// Runs the tool, `stdin` fed to it, and gives what it printed and how it ended.
fn run(args: &[String], stdin: Option<&[u8]>) -> Result<Output, String> {
    let mut command = Command::new(TOOL);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("{TOOL}: {e}"))?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes).map_err(|e| format!("{TOOL}: {e}"))?;
    }
    child.wait_with_output().map_err(|e| format!("{TOOL}: {e}"))
}

fn said(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let text = text.trim();
    if text.is_empty() {
        format!("{TOOL} ended with {}", output.status)
    } else {
        text.chars().take(200).collect()
    }
}

impl SecretStore for ToolStore {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let out = run(&self.lookup_args(name), None)?;
        let text = String::from_utf8_lossy(&out.stdout);
        let text = text.trim();
        if !out.status.success() || text.is_empty() {
            // Both tools end unsuccessfully, saying nothing, for a name not kept; an
            // error message on stderr means a store that could not be used.
            return if out.stderr.iter().all(u8::is_ascii_whitespace)
                || String::from_utf8_lossy(&out.stderr).contains("could not be found")
            {
                Ok(None)
            } else {
                Err(said(&out))
            };
        }
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map(Some)
            .map_err(|_| format!("{TOOL} gave back a value dm did not keep"))
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), String> {
        let text = base64::engine::general_purpose::STANDARD.encode(value);
        // `secret-tool store` reads the secret from standard input. So does `security -i`
        // (its interactive mode reads commands from standard input), so the secret never
        // appears in a command line, where any program of the account could read it.
        #[cfg(target_os = "linux")]
        let out = run(
            &["store", "--label=dm", "service", "dm", "name", name].map(String::from),
            Some(text.as_bytes()),
        )?;
        #[cfg(target_os = "macos")]
        let out = {
            // The line is split on spaces and quotes by `security`; the name as kept
            // (`keyed`) and the base64 text have none, and a keychain path with any is
            // refused rather than mis-split.
            let name = keyed(name);
            if !self.keychain.as_deref().is_none_or(plain) {
                return Err("a keychain path dm does not use".to_string());
            }
            let line = format!(
                "add-generic-password -U -a dm -s {name} -w {text}{}\n",
                self.keychain
                    .as_deref()
                    .map(|k| format!(" {k}"))
                    .unwrap_or_default()
            );
            run(&["-i".to_string()], Some(line.as_bytes()))?
        };
        if out.status.success() {
            Ok(())
        } else {
            Err(said(&out))
        }
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        let args = ["clear", "service", "dm", "name", name].map(String::from);
        #[cfg(target_os = "macos")]
        let args = self.in_keychain(&["delete-generic-password", "-a", "dm", "-s", &keyed(name)]);
        let out = run(&args, None)?;
        // Forgetting what is not there is not an error: the tools end unsuccessfully for it.
        let _ = out;
        Ok(())
    }
}

/// Against a real store, and only where asked (`DM_SECRET_STORE_TEST=1`): on Linux the
/// session's Secret Service, which CI's machines have none of; on macOS a keychain file of the
/// test's own, named on every command and left off the account's search list, so the
/// person's login keychain is never read or written.
#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch {
        store: ToolStore,
        #[cfg(target_os = "macos")]
        _dir: tempfile::TempDir,
    }

    #[cfg(target_os = "macos")]
    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Some(keychain) = &self.store.keychain {
                let _ = run(&["delete-keychain".into(), keychain.clone()], None);
            }
        }
    }

    fn scratch() -> Option<Scratch> {
        if std::env::var_os("DM_SECRET_STORE_TEST").is_none_or(|v| v.is_empty()) {
            eprintln!("not run: DM_SECRET_STORE_TEST is not set");
            return None;
        }
        #[cfg(target_os = "linux")]
        {
            Some(Scratch {
                store: ToolStore::SYSTEM,
            })
        }
        #[cfg(target_os = "macos")]
        {
            let dir = tempfile::tempdir().unwrap();
            let keychain = dir.path().join("dm-test.keychain-db").display().to_string();
            let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
            // `create-keychain` puts the new one on the search list: the list is put
            // back as it was at once.
            let listed = run(&s(&["list-keychains", "-d", "user"]), None).unwrap();
            let before: Vec<String> = String::from_utf8_lossy(&listed.stdout)
                .lines()
                .map(|l| l.trim().trim_matches('"').to_string())
                .filter(|l| !l.is_empty())
                .collect();
            let made = run(&s(&["create-keychain", "-p", "dm-test", &keychain]), None).unwrap();
            let mut back = s(&["list-keychains", "-d", "user", "-s"]);
            back.extend(before);
            let restored = run(&back, None).unwrap();
            assert!(made.status.success(), "{}", said(&made));
            assert!(restored.status.success(), "{}", said(&restored));
            let opened = run(&s(&["unlock-keychain", "-p", "dm-test", &keychain]), None).unwrap();
            assert!(opened.status.success(), "{}", said(&opened));
            Some(Scratch {
                store: ToolStore {
                    keychain: Some(keychain),
                },
                _dir: dir,
            })
        }
    }

    fn unique(what: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("dm:test:{what}:{}:{nanos}", std::process::id())
    }

    #[test]
    fn a_secret_is_kept_read_replaced_and_forgotten_in_the_real_store() {
        let Some(scratch) = scratch() else { return };
        let store = &scratch.store;
        let name = unique("round");
        assert_eq!(store.get(&name).unwrap(), None, "nothing there yet");
        store.put(&name, b"first").unwrap();
        assert_eq!(store.get(&name).unwrap().as_deref(), Some(&b"first"[..]));
        store.put(&name, b"second, longer").unwrap();
        assert_eq!(
            store.get(&name).unwrap().as_deref(),
            Some(&b"second, longer"[..])
        );
        store.delete(&name).unwrap();
        assert_eq!(store.get(&name).unwrap(), None, "forgotten");
        store.delete(&name).unwrap();
    }

    /// A Sign-in's name: the site and the user with a line break between, as
    /// `settings/secrets.rs` makes it, and a user name with a space and a quote.
    #[test]
    fn a_sign_ins_name_with_a_line_break_is_kept_and_forgotten_in_the_real_store() {
        let Some(scratch) = scratch() else { return };
        let store = &scratch.store;
        let name = format!("{}:example.com\nann o'neil", unique("login"));
        store.put(&name, b"pw").unwrap();
        assert_eq!(store.get(&name).unwrap().as_deref(), Some(&b"pw"[..]));
        store.delete(&name).unwrap();
        assert_eq!(store.get(&name).unwrap(), None, "forgotten");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_name_security_would_split_is_kept_under_a_plain_one_and_a_plain_one_as_it_is() {
        assert_eq!(keyed("dm:proxy:host:3128:me"), "dm:proxy:host:3128:me");
        let odd = keyed("dm:login:x:example.com\nann o'neil");
        assert!(odd.starts_with("dm:b64:") && plain(&odd), "{odd}");
        assert_ne!(odd, keyed("dm:login:x:example.com\nann o'neim"));
    }

    #[test]
    fn a_key_of_32_bytes_with_zeros_in_it_comes_back_whole() {
        let Some(scratch) = scratch() else { return };
        let store = &scratch.store;
        let name = unique("key");
        let key: Vec<u8> = (0..32u8).map(|n| if n % 5 == 0 { 0 } else { n }).collect();
        store.put(&name, &key).unwrap();
        assert_eq!(store.get(&name).unwrap(), Some(key));
        store.delete(&name).unwrap();
    }

    #[test]
    fn a_file_of_secrets_is_sealed_with_a_key_the_real_store_keeps() {
        let Some(scratch) = scratch() else { return };
        let store = &scratch.store;
        let key_name = unique("sealkey");
        let plain = br#"[{"id":1,"username":"me","password":"hunter2"}]"#;
        let sealed = super::super::seal_with(store, &key_name, plain).unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("hunter2"));
        assert_eq!(
            super::super::open_with(store, &sealed),
            super::super::Opened::Opened(plain.to_vec())
        );
        store.delete(&key_name).unwrap();
        assert!(matches!(
            super::super::open_with(store, &sealed),
            super::super::Opened::Failed(_)
        ));
    }
}
