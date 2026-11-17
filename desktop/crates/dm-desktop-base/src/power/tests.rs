// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

#[test]
fn a_sound_file_is_handed_to_the_players_program_only_when_it_is_one_an_ear_can_expect() {
    assert_eq!(
        super::file_program("/s/a.WAV", "macos"),
        Some(("afplay", vec!["/s/a.WAV".to_string()]))
    );
    assert_eq!(
        super::file_program("/s/a.ogg", "linux"),
        Some(("paplay", vec!["/s/a.ogg".to_string()]))
    );
    assert_eq!(super::file_program(r"C:s.wav", "windows"), None);
    for refused in ["", "  ", "/s/a.exe", "/s/a.sh", "/s/wav"] {
        assert_eq!(super::file_program(refused, "linux"), None, "{refused:?}");
    }
    assert!(!super::play_file("/no/such/dir/a.wav"));
}

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

/// Sleep on Windows is `SetSuspendState(FALSE, …)`, not rundll32,
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

/// No PowerShell for a sound on Windows; two different sounds
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
