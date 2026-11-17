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
mod tests;
