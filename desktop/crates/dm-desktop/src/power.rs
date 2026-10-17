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

/// Where a refusal that comes back later is told: the window's list of
/// problems.
pub type Report = Box<dyn FnOnce(String) + Send>;

/// Runs the system's command for `action` on this machine. That it could
/// not be started is the answer; a refusal it gives later goes to `report`.
pub fn run(action: WhenDone, report: Report) -> Result<(), String> {
    let (program, args) = command(action, std::env::consts::OS)
        .ok_or_else(|| format!("dm cannot {} on this system", action.verb()))?;
    let mut command = std::process::Command::new(program);
    dm_core::no_window(&mut command).args(args);
    watch(action, &mut command, report)
}

/// Starts `command` and waits for it on a thread of its own, telling
/// `report` if it fails, whenever that is. A refusal — no permission to
/// shut down, a system that will not suspend — used to be ignored: the
/// countdown ended, nothing happened, and nothing said so. Waiting on the
/// window's thread would hold the window, and a suspend that returns only
/// on waking would hold it for the night.
fn watch(
    action: WhenDone,
    command: &mut std::process::Command,
    report: Report,
) -> Result<(), String> {
    let child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not {}: {e}", action.verb()))?;
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

    #[cfg(unix)]
    #[test]
    fn a_refusal_is_told_whenever_it_comes_without_holding_the_caller() {
        use std::sync::mpsc;
        let told = |script: &str| {
            let (send, receive) = mpsc::channel();
            let mut command = std::process::Command::new("sh");
            command.args(["-c", script]);
            let at = std::time::Instant::now();
            watch(
                WhenDone::ShutDown,
                &mut command,
                Box::new(move |why| {
                    let _ = send.send(why);
                }),
            )
            .unwrap();
            assert!(
                at.elapsed() < std::time::Duration::from_millis(500),
                "held the caller"
            );
            receive.recv_timeout(std::time::Duration::from_secs(10))
        };
        // Later than the three seconds the first version waited.
        let late = told("sleep 4; echo 'Access denied' >&2; exit 1").unwrap();
        assert!(late.contains("Access denied"), "{late}");
        assert!(told("exit 0").is_err(), "a success is not a problem");
    }

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
