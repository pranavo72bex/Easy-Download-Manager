// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Running a program a person named: once the scheduled downloads are done, or on a finished file.

use super::*;

/// The words of a command line, as this system's shell would split them: spaces apart, except
/// inside single or double quotes; outside single quotes a backslash escapes the next character
/// on macOS and Linux, and on Windows is a backslash, the folder separator
/// (`C:\Tools\scan.exe` was read as `C:Toolsscan.exe`, gaps row H39).
pub fn words(command: &str) -> Vec<String> {
    words_with(command, !cfg!(windows))
}

/// [`words`], with a backslash escaping the next character where `escapes`.
fn words_with(command: &str, escapes: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => word.push(c),
            (_, '\\') if escapes => {
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
pub fn run_program(command: &str) -> Result<(), crate::Error> {
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
            crate::Error::from(format!(
                "\"{program}\", to run when the scheduled downloads are done, could not start: {e}"
            ))
        })
}

/// The characters Windows' command interpreters run as commands or variables when they read
/// their line again: `cmd /c` and PowerShell take the file's path as text of their own.
const INTERPRETED: &[char] = &['&', '|', '^', '%', '!', ';', '$', '(', ')', '<', '>', '`', '\'', '"'];

/// Whether `program` is a command interpreter that would read `file`'s name as commands: `cmd`,
/// `powershell` or `pwsh`, given a name a server chose with `&`, `;` and the rest in it (gaps
/// row H39). Then the file is in `DM_FILE` alone, which `%DM_FILE%` and `$env:DM_FILE` read.
fn interpreter_would_run(program: &str, file: &Path) -> bool {
    // The last part after either separator, whatever system reads it.
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let stem = name
        .rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_lowercase();
    matches!(stem.as_str(), "cmd" | "powershell" | "pwsh")
        && file
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains(INTERPRETED))
}

/// Runs `command` on a finished file, its path added as the last argument rather than pasted
/// into the line, and in `DM_FILE`. To a command interpreter (`cmd /c`, PowerShell) a name
/// holding what it would run as a command is not added as an argument, only kept in
/// `DM_FILE`: it reads its own line again, so `a&calc.txt` ran `calc` (gaps row H39).
/// Started and left running; an error says why it could not start.
pub fn run_after(command: &str, file: &Path) -> Result<(), crate::Error> {
    let mut parts = words(command);
    if parts.is_empty() {
        return Ok(());
    }
    let program = parts.remove(0);
    let as_argument = !interpreter_would_run(&program, file);
    std::process::Command::new(&program)
        .args(parts)
        .args(as_argument.then_some(file))
        .env("DM_FILE", file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            crate::Error::from(format!(
                "\"{program}\", to run after {}, could not start: {e}",
                file.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Windows path keeps its backslashes, where they are the folder separator (gaps row
    /// H39); elsewhere a backslash escapes as a shell's does.
    #[test]
    fn a_windows_path_keeps_its_backslashes() {
        assert_eq!(
            words_with(r#"C:\Tools\scan.exe "C:\My Files\x" /q"#, false),
            [r"C:\Tools\scan.exe", r"C:\My Files\x", "/q"]
        );
        assert_eq!(words_with(r"a\ b", true), ["a b"]);
    }

    /// A file name a command interpreter would run part of is not handed to it as an argument;
    /// a plain name is, and any name is to a program that reads no command line again.
    #[test]
    fn a_name_an_interpreter_would_run_is_kept_from_its_line() {
        let bad = Path::new("downloads/a&calc.txt");
        let plain = Path::new("downloads/report 2026.pdf");
        for interpreter in ["cmd", "C:\\Windows\\System32\\cmd.exe", "powershell.exe", "PWSH"] {
            assert!(interpreter_would_run(interpreter, bad), "{interpreter}");
            assert!(!interpreter_would_run(interpreter, plain), "{interpreter}");
        }
        assert!(!interpreter_would_run("clamscan", bad));
        assert!(interpreter_would_run("cmd", Path::new("a;b.txt")));
        assert!(interpreter_would_run("cmd", Path::new("%PATH%.txt")));
    }
}
