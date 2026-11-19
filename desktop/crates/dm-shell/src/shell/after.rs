// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Running a program a person named: once the scheduled downloads are done, or on a finished file.

use super::*;

/// The words of a command line, as a shell would split them: spaces apart,
/// except inside single or double quotes, and a backslash escaping the next
/// character outside single quotes.
pub fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => word.push(c),
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
                started = true;
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// Runs `command` as it is, with no file: started and left running, no shell, its
/// words split as `run_after`'s are. An error says why it could not start.
pub fn run_program(command: &str) -> Result<(), String> {
    let mut parts = words(command);
    if parts.is_empty() {
        return Ok(());
    }
    let program = parts.remove(0);
    std::process::Command::new(&program)
        .args(parts)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            format!(
                "\"{program}\", to run when the scheduled downloads are done, could not start: {e}"
            )
        })
}

/// Runs `command` on a finished file, its path added as the last argument
/// rather than pasted into the line, so no file name can become a command.
/// Started and left running; an error says why it could not start.
pub fn run_after(command: &str, file: &Path) -> Result<(), String> {
    let mut parts = words(command);
    if parts.is_empty() {
        return Ok(());
    }
    let program = parts.remove(0);
    std::process::Command::new(&program)
        .args(parts)
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            format!(
                "\"{program}\", to run after {}, could not start: {e}",
                file.display()
            )
        })
}
