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

/// How the machine is put to sleep or shut down on one system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    /// A program of the system's, with its arguments.
    Run(&'static str, Vec<&'static str>),
    /// Windows' `SetSuspendState`, called directly, `hibernate` its first
    /// argument. Through `rundll32 powrprof.dll,SetSuspendState 0,1,0` it
    /// was not: rundll32 hands the function its window handle where the
    /// first argument goes, never zero, so Sleep hibernated wherever
    /// hibernation was on (gap 42).
    Suspend { hibernate: bool },
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
/// `systemctl`, and failed when the countdown ran out (UI round 13).
pub fn available(action: WhenDone) -> bool {
    match way(action, std::env::consts::OS) {
        Some(Way::Run(program, _)) => installed(program),
        Some(Way::Suspend { .. }) => true,
        None => false,
    }
}

/// Whether `program` is on the PATH or in the system's own folders, which
/// a launch from a desktop may leave off the PATH.
fn installed(program: &str) -> bool {
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

/// Where a refusal that comes back later is told: the window's list of
/// problems.
pub type Report = Box<dyn FnOnce(String) + Send>;

/// Does `action` on this machine. That it could not be started is the
/// answer; a refusal it gives later goes to `report`.
pub fn run(action: WhenDone, report: Report) -> Result<(), String> {
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
fn suspend(action: WhenDone, hibernate: bool, report: Report) -> Result<(), String> {
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
        .map_err(|e| format!("could not {}: {e}", action.verb()))
}

#[cfg(not(windows))]
fn suspend(action: WhenDone, _hibernate: bool, _report: Report) -> Result<(), String> {
    Err(format!("dm cannot {} on this system", action.verb()))
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

/// Which sound: a failure has a different one from the finish, so the
/// ear tells them apart across the room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Finished,
    Failed,
}

/// The program that plays `sound` on `os`. `None` on Windows, where
/// `MessageBeep` plays it in this process: PowerShell's
/// `[SystemSounds]::X.Play()` took about a second to start and played
/// asynchronously, so its own exit could cut the sound off (gap 100).
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

/// Plays the system's own short sound for a failed download.
pub fn alert() {
    play(Sound::Failed);
}

/// Plays the system's own short sound for a finished download.
pub fn chime() {
    play(Sound::Finished);
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(unix)]
    fn a_program_counts_only_where_it_is_installed() {
        assert!(super::installed("sh"));
        assert!(!super::installed("dm-no-such-power-program"));
    }

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
            way(WhenDone::Sleep, "macos"),
            Some(Way::Run("pmset", vec!["sleepnow"]))
        );
        assert_eq!(
            way(WhenDone::ShutDown, "linux"),
            Some(Way::Run("systemctl", vec!["poweroff"]))
        );
        assert_eq!(
            way(WhenDone::ShutDown, "windows"),
            Some(Way::Run("shutdown", vec!["/s", "/t", "0"]))
        );
        assert_eq!(
            way(WhenDone::Hibernate, "linux"),
            Some(Way::Run("systemctl", vec!["hibernate"]))
        );
        assert_eq!(way(WhenDone::Hibernate, "macos"), None, "no such command");
        assert_eq!(way(WhenDone::Nothing, "macos"), None);
        assert_eq!(way(WhenDone::Quit, "linux"), None, "dm quits itself");
        assert_eq!(way(WhenDone::Sleep, "freebsd"), None);
    }

    /// Gap 42: Sleep on Windows is `SetSuspendState(FALSE, …)`, not rundll32,
    /// which passed its window handle as "hibernate".
    #[test]
    fn windows_sleeps_and_hibernates_by_its_own_call_not_rundll32() {
        assert_eq!(
            way(WhenDone::Sleep, "windows"),
            Some(Way::Suspend { hibernate: false })
        );
        assert_eq!(
            way(WhenDone::Hibernate, "windows"),
            Some(Way::Suspend { hibernate: true })
        );
        for action in WhenDone::ALL {
            for os in ["windows", "macos", "linux"] {
                if let Some(Way::Run(program, _)) = way(action, os) {
                    assert_ne!(program, "rundll32.exe");
                }
            }
        }
    }

    /// Gap 100: no PowerShell for a sound on Windows; two different sounds
    /// elsewhere.
    #[test]
    fn windows_plays_its_sounds_without_a_program_and_others_tell_them_apart() {
        assert_eq!(sound_program(Sound::Finished, "windows"), None);
        assert_eq!(sound_program(Sound::Failed, "windows"), None);
        for os in ["macos", "linux"] {
            let finished = sound_program(Sound::Finished, os);
            assert!(finished.is_some());
            assert_ne!(finished, sound_program(Sound::Failed, os), "{os}");
        }
    }
}
