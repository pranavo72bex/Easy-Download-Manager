// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm does when the queue runs dry: Downloads › When done.
//!
//! Chosen for this session only, from the Downloads menu, never saved: a
//! "shut down when done" set yesterday and forgotten would shut the machine
//! down under someone today. Sleep and shut down are counted down in a
//! window that can be cancelled, and run through the system's
//! own commands, which need no privileges dm does not already have.

pub use dm_shell::app::power::{Report, WhenDone, COUNTDOWN_SECS};

use crate::Error;

/// How the machine is put to sleep or shut down on one system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    /// A program of the system's, with its arguments.
    Run(&'static str, Vec<&'static str>),
    /// Windows' `SetSuspendState`, called directly, `hibernate` its first
    /// argument. Through `rundll32 powrprof.dll,SetSuspendState 0,1,0` it
    /// was not: rundll32 hands the function its window handle where the
    /// first argument goes, never zero, so Sleep hibernated wherever
    /// hibernation was on.
    Suspend {
        /// Hibernate rather than sleep.
        hibernate: bool,
    },
}

/// How `action` is done on `os` (as `std::env::consts::OS` names it).
/// `None` for anything else, and the menu greys it.
pub fn way(action: WhenDone, os: &str) -> Option<Way> {
    match (action, os) {
        (WhenDone::Sleep, "macos") => Some(Way::Run("pmset", vec!["sleepnow"])),
        (WhenDone::ShutDown, "macos") => Some(Way::Run(
            "osascript",
            vec!["-e", "tell application \"System Events\" to shut down"],
        )),
        (WhenDone::Sleep, "linux") => Some(Way::Run("systemctl", vec!["suspend"])),
        (WhenDone::Hibernate, "linux") => Some(Way::Run("systemctl", vec!["hibernate"])),
        (WhenDone::ShutDown, "linux") => Some(Way::Run("systemctl", vec!["poweroff"])),
        (WhenDone::Sleep, "windows") => Some(Way::Suspend { hibernate: false }),
        (WhenDone::Hibernate, "windows") => Some(Way::Suspend { hibernate: true }),
        (WhenDone::ShutDown, "windows") => Some(Way::Run("shutdown", vec!["/s", "/t", "0"])),
        _ => None,
    }
}

/// Whether `action` can be done here: this system has a way, and where the
/// way is a program, the program is there. Sleep was offered without
/// `systemctl`, and failed when the countdown ran out.
pub fn available(action: WhenDone) -> bool {
    match way(action, std::env::consts::OS) {
        Some(Way::Run(program, _)) => installed(program),
        Some(Way::Suspend { .. }) => true,
        None => false,
    }
}

/// Whether `program` is on the PATH or in the system's own folders, which
/// a launch from a desktop may leave off the PATH. Looked for once a run
/// for each program: the menus ask each time they are built, and the
/// system's own programs do not come and go while dm runs.
fn installed(program: &'static str) -> bool {
    static FOUND: std::sync::Mutex<Vec<(&str, bool)>> = std::sync::Mutex::new(Vec::new());
    if let Some(&(_, found)) = dm_core::lock(&FOUND).iter().find(|(p, _)| *p == program) {
        return found;
    }
    let found = look_for(program);
    dm_core::lock(&FOUND).push((program, found));
    found
}

/// [`installed`]'s look, on the disk.
fn look_for(program: &str) -> bool {
    let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(std::path::PathBuf::from));
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{program}.exe"), program.to_string()]
    } else {
        vec![program.to_string()]
    };
    dirs.iter()
        .any(|d| names.iter().any(|n| d.join(n).is_file()))
}

/// Does `action` on this machine. That it could not be started is the
/// answer; a refusal it gives later goes to `report`.
pub fn run(action: WhenDone, report: Report) -> Result<(), Error> {
    match way(action, std::env::consts::OS)
        .ok_or_else(|| format!("dm cannot {} on this system", action.verb()))?
    {
        Way::Run(program, args) => {
            let mut command = std::process::Command::new(program);
            dm_core::no_window(&mut command).args(args);
            watch(action, &mut command, report)
        }
        Way::Suspend { hibernate } => suspend(action, hibernate, report),
    }
}

/// `SetSuspendState` on a thread of its own: it returns only on waking.
#[cfg(windows)]
fn suspend(action: WhenDone, hibernate: bool, report: Report) -> Result<(), Error> {
    std::thread::Builder::new()
        .name("dm-suspend".into())
        .spawn(move || {
            // SAFETY: three booleans in, one out; no memory of ours is
            // touched. Not forced, wake events left on.
            let done = unsafe {
                windows_sys::Win32::System::Power::SetSuspendState(hibernate, false, false)
            };
            if !done {
                report(format!(
                    "could not {}: {}",
                    action.verb(),
                    std::io::Error::last_os_error()
                ));
            }
        })
        .map(|_| ())
        .map_err(|e| Error::from(format!("could not {}: {e}", action.verb())))
}

#[cfg(not(windows))]
fn suspend(action: WhenDone, _hibernate: bool, _report: Report) -> Result<(), Error> {
    Err(format!("dm cannot {} on this system", action.verb()).into())
}

/// Starts `command` and waits for it on a thread of its own, telling
/// `report` if it fails, whenever that is. A refusal — no permission to
/// shut down, a system that will not suspend — is told: ignored, the
/// countdown would end, nothing happen, and nothing say so. Waiting on the
/// window's thread would hold the window, and a suspend that returns only
/// on waking would hold it for the night.
fn watch(
    action: WhenDone,
    command: &mut std::process::Command,
    report: Report,
) -> Result<(), Error> {
    let child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::from(format!("could not {}: {e}", action.verb())))?;
    std::thread::spawn(move || match child.wait_with_output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let said = String::from_utf8_lossy(&out.stderr);
            let said = said.trim();
            report(if said.is_empty() {
                format!(
                    "could not {}: the system's command ended with {}",
                    action.verb(),
                    out.status
                )
            } else {
                format!("could not {}: {said}", action.verb())
            });
        }
        Err(e) => report(format!("could not {}: {e}", action.verb())),
    });
    Ok(())
}

/// Which sound: a failure has a different one from the finish, so the
/// ear tells them apart across the room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// The sound of a download finishing.
    Finished,
    /// The sound of a download failing.
    Failed,
}

/// The program that plays `sound` on `os`. `None` on Windows, where
/// `MessageBeep` plays it in this process: PowerShell's
/// `[SystemSounds]::X.Play()` took about a second to start and played
/// asynchronously, so its own exit could cut the sound off.
pub fn sound_program(sound: Sound, os: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match (os, sound) {
        ("windows", _) => None,
        ("macos", Sound::Finished) => Some(("afplay", vec!["/System/Library/Sounds/Glass.aiff"])),
        ("macos", Sound::Failed) => Some(("afplay", vec!["/System/Library/Sounds/Basso.aiff"])),
        (_, Sound::Finished) => Some((
            "canberra-gtk-play",
            vec!["-i", "complete", "-d", "dm download finished"],
        )),
        (_, Sound::Failed) => Some((
            "canberra-gtk-play",
            vec!["-i", "dialog-error", "-d", "dm download failed"],
        )),
    }
}

/// Plays `sound`. One that cannot be played is not worth a word about.
pub fn play(sound: Sound) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONHAND, MB_OK};
        let kind = match sound {
            Sound::Finished => MB_OK,
            Sound::Failed => MB_ICONHAND,
        };
        // SAFETY: an integer in, an integer out. It queues the system's
        // sound and returns; the sound plays to its end whatever dm does.
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(kind);
        }
    }
    if let Some((program, args)) = sound_program(sound, std::env::consts::OS) {
        let _ = dm_core::no_window(&mut std::process::Command::new(program))
            .args(args)
            .spawn();
    }
}

/// The program that plays the sound file `path` on `os`; `None` on Windows, which plays it in this
/// process. Only the files an ear can expect (`.wav`, `.aiff`, `.mp3`, `.ogg`) are handed on.
pub fn file_program(path: &str, os: &str) -> Option<(&'static str, Vec<String>)> {
    let lower = path.to_ascii_lowercase();
    if path.trim().is_empty()
        || ![".wav", ".aiff", ".mp3", ".ogg"]
            .iter()
            .any(|e| lower.ends_with(e))
    {
        return None;
    }
    match os {
        "windows" => None,
        "macos" => Some(("afplay", vec![path.to_string()])),
        _ => Some(("paplay", vec![path.to_string()])),
    }
}

/// Plays the sound file `path`. False when it is not a file dm can play, so the caller can fall back
/// to the system's sound.
pub fn play_file(path: &str) -> bool {
    if !std::path::Path::new(path).is_file() {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    #[cfg(windows)]
    {
        if !lower.ends_with(".wav") {
            return false;
        }
        use windows_sys::Win32::Media::Audio::{
            PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT,
        };
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a NUL-ended wide string that outlives the call, which queues the sound and returns.
        let played = unsafe {
            PlaySoundW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
            )
        };
        played != 0
    }
    #[cfg(not(windows))]
    {
        let _ = lower;
        match file_program(path, std::env::consts::OS) {
            Some((program, args)) => dm_core::no_window(&mut std::process::Command::new(program))
                .args(args)
                .spawn()
                .is_ok(),
            None => false,
        }
    }
}

/// Plays the system's own short sound for a failed download.
pub fn alert() {
    play(Sound::Failed);
}

/// Plays the system's own short sound for a finished download.
pub fn chime() {
    play(Sound::Finished);
}

#[cfg(test)]
mod tests;
