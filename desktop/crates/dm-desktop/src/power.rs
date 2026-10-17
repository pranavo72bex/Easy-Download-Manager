//! What dm does when the queue runs dry: Downloads › When done.
//!
//! Chosen for this session only, from the Downloads menu, never saved: a
//! "shut down when done" set yesterday and forgotten would shut the machine
//! down under someone today. Sleep and shut down are counted down in a
//! window that can be cancelled, and run through the system's
//! own commands, which need no privileges dm does not already have.

/// The action taken when the last download settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhenDone {
    #[default]
    Nothing,
    Quit,
    Sleep,
    /// To disk and off: Linux and Windows have it; macOS does not offer it
    /// as a command, so the menu greys it there.
    Hibernate,
    ShutDown,
}

/// How long a sleep or shut down waits for someone to cancel it.
pub const COUNTDOWN_SECS: u64 = 30;

impl WhenDone {
    pub const ALL: [WhenDone; 5] = [
        WhenDone::Nothing,
        WhenDone::Quit,
        WhenDone::Sleep,
        WhenDone::Hibernate,
        WhenDone::ShutDown,
    ];

    /// What the countdown window says is about to happen.
    pub fn verb(self) -> &'static str {
        match self {
            WhenDone::Nothing => "do nothing",
            WhenDone::Quit => "quit",
            WhenDone::Sleep => "put the computer to sleep",
            WhenDone::Hibernate => "hibernate the computer",
            WhenDone::ShutDown => "shut the computer down",
        }
    }
}

/// The program and arguments that put the machine to sleep or shut it down
/// on `os` (as `std::env::consts::OS` names it). `None` for anything else.
pub fn command(action: WhenDone, os: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match (action, os) {
        (WhenDone::Sleep, "macos") => Some(("pmset", vec!["sleepnow"])),
        (WhenDone::ShutDown, "macos") => Some((
            "osascript",
            vec!["-e", "tell application \"System Events\" to shut down"],
        )),
        (WhenDone::Sleep, "linux") => Some(("systemctl", vec!["suspend"])),
        (WhenDone::Hibernate, "linux") => Some(("systemctl", vec!["hibernate"])),
        (WhenDone::ShutDown, "linux") => Some(("systemctl", vec!["poweroff"])),
        (WhenDone::Sleep, "windows") => Some((
            "rundll32.exe",
            vec!["powrprof.dll,SetSuspendState", "0,1,0"],
        )),
        (WhenDone::Hibernate, "windows") => Some(("shutdown", vec!["/h"])),
        (WhenDone::ShutDown, "windows") => Some(("shutdown", vec!["/s", "/t", "0"])),
        _ => None,
    }
}

/// Runs the system's command for `action` on this machine.
pub fn run(action: WhenDone) -> Result<(), String> {
    let (program, args) = command(action, std::env::consts::OS)
        .ok_or_else(|| format!("dm cannot {} on this system", action.verb()))?;
    dm_core::no_window(&mut std::process::Command::new(program))
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not {}: {e}", action.verb()))
}

/// Plays the system's own short sound for a failed download: a different
/// one from the finish, so the ear tells them apart across the room.
pub fn alert() {
    let (program, args): (&str, Vec<&str>) = match std::env::consts::OS {
        "macos" => ("afplay", vec!["/System/Library/Sounds/Basso.aiff"]),
        "windows" => (
            "powershell",
            vec![
                "-NoProfile",
                "-Command",
                "[System.Media.SystemSounds]::Hand.Play()",
            ],
        ),
        _ => (
            "canberra-gtk-play",
            vec!["-i", "dialog-error", "-d", "dm download failed"],
        ),
    };
    let _ = dm_core::no_window(&mut std::process::Command::new(program))
        .args(args)
        .spawn();
}

/// Plays the system's own short sound for a finished download.
pub fn chime() {
    let (program, args): (&str, Vec<&str>) = match std::env::consts::OS {
        "macos" => ("afplay", vec!["/System/Library/Sounds/Glass.aiff"]),
        "windows" => (
            "powershell",
            vec![
                "-NoProfile",
                "-Command",
                "[System.Media.SystemSounds]::Asterisk.Play()",
            ],
        ),
        _ => (
            "canberra-gtk-play",
            vec!["-i", "complete", "-d", "dm download finished"],
        ),
    };
    // A sound that cannot be played is not worth a word about.
    let _ = dm_core::no_window(&mut std::process::Command::new(program))
        .args(args)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_system_is_asked_with_its_own_command_and_nothing_is_not_run() {
        assert_eq!(
            command(WhenDone::Sleep, "macos"),
            Some(("pmset", vec!["sleepnow"]))
        );
        assert_eq!(
            command(WhenDone::ShutDown, "linux"),
            Some(("systemctl", vec!["poweroff"]))
        );
        assert_eq!(
            command(WhenDone::ShutDown, "windows"),
            Some(("shutdown", vec!["/s", "/t", "0"]))
        );
        assert_eq!(
            command(WhenDone::Hibernate, "linux"),
            Some(("systemctl", vec!["hibernate"]))
        );
        assert_eq!(
            command(WhenDone::Hibernate, "windows"),
            Some(("shutdown", vec!["/h"]))
        );
        assert_eq!(
            command(WhenDone::Hibernate, "macos"),
            None,
            "no such command"
        );
        assert_eq!(command(WhenDone::Nothing, "macos"), None);
        assert_eq!(command(WhenDone::Quit, "linux"), None, "dm quits itself");
        assert_eq!(command(WhenDone::Sleep, "freebsd"), None);
    }
}
