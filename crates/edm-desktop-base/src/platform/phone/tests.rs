// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's seam with no phone, or one that leaves services out; what keeps a phone busy
//! and its words; where a file EDM writes on a phone goes.

use super::*;

/// With no phone handed in, or one that leaves a service out, EDM says so in words and
/// never panics.
#[test]
fn what_the_phone_leaves_out_is_said_in_words() {
    struct Nothing;
    impl PhoneServices for Nothing {}
    let nothing = Nothing;
    assert_eq!(nothing.open("/a"), Err(NOT_YET.to_string()));
    assert_eq!(nothing.reveal("/a"), Err(NOT_YET.to_string()));
    assert_eq!(nothing.trash("/a"), Err(NO_TRASH.to_string()));
    assert_eq!(nothing.clipboard(), None);
    nothing.keep_awake(true);
    nothing.busy(None);
    nothing.set_clipboard("x");
    assert_eq!(nothing.publish(std::path::Path::new("/a")), Ok(None));
    assert_eq!(nothing.download_folders(), Err(NO_FOLDERS.to_string()));
    assert_eq!(nothing.save_folder(), Err(NO_SAVE.to_string()));
}

fn row(id: u64, state: edm_core::State, downloaded: u64, speed: f64) -> Arc<edm_core::Entry> {
    let row: edm_core::Entry = serde_json::from_value(serde_json::json!({
        "id": id,
        "url": "https://files.example/a.zip",
        "state": serde_json::to_value(state).unwrap(),
        "progress": {"downloaded": downloaded, "total": 100, "speed": speed, "eta": null},
        "filename": "a.zip",
    }))
    .unwrap();
    Arc::new(row)
}

/// Running, queued while due, or sharing keeps EDM busy; queued rows the schedule holds,
/// finished and stopped ones do not.
#[test]
fn what_keeps_a_phone_busy() {
    use edm_core::State;
    let rows = vec![
        row(1, State::Running, 10, 1000.0),
        row(2, State::Running, 100, 5.0),
        row(3, State::Queued, 0, 0.0),
        row(4, State::Completed, 100, 0.0),
        row(5, State::Paused, 3, 0.0),
    ];
    assert_eq!(
        Busy::of(&rows, true, false),
        Some(Busy {
            running: 1,
            finishing: 1,
            waiting: 1,
            seeding: false,
            bytes_per_second: 1000.0,
        })
    );
    assert_eq!(Busy::of(&rows, false, false).map(|b| b.waiting), Some(0));
    let queued = vec![row(3, State::Queued, 0, 0.0)];
    assert_eq!(Busy::of(&queued, false, false), None);
    assert_eq!(Busy::of(&queued, true, false).map(|b| b.waiting), Some(1));
    let done = vec![
        row(4, State::Completed, 100, 0.0),
        row(5, State::Paused, 3, 0.0),
    ];
    assert_eq!(Busy::of(&done, true, false), None);
    assert_eq!(
        Busy::of(&done, true, true),
        Some(Busy {
            seeding: true,
            ..Busy::default()
        })
    );
}

/// The notification's words are the status bar's, Exit's question's and the tray's.
#[test]
fn a_busy_phone_says_it_in_dms_words() {
    crate::i18n::set("en");
    let busy = |running, finishing, waiting, seeding| Busy {
        running,
        finishing,
        waiting,
        seeding,
        bytes_per_second: 2048.0,
    };
    let rate = crate::format::rate(2048.0);
    assert_eq!(
        busy(2, 0, 1, false).words(),
        (
            format!("2 downloading at {rate}"),
            Some("1 more is waiting to start.".into())
        )
    );
    assert_eq!(
        busy(1, 1, 0, true).words(),
        (format!("1 downloading at {rate}"), None)
    );
    assert_eq!(busy(0, 2, 0, false).words(), ("2 finishing".into(), None));
    assert_eq!(
        busy(0, 0, 3, false).words(),
        ("3 downloads are waiting to start.".into(), None)
    );
    assert_eq!(
        busy(0, 0, 0, true).words(),
        (crate::tray::words()[4].to_string(), None)
    );
}

/// A file EDM writes on the phone goes into the save folder, made where it is missing,
/// and never over a file already there.
#[test]
fn a_saved_file_takes_a_free_name() {
    let dir = std::env::temp_dir().join(format!("edm-phone-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let folder = dir.join("Download").join("EDM");
    let first = save_place_in(&folder, "downloads.dmlist").unwrap();
    assert_eq!(first, folder.join("downloads.dmlist"));
    std::fs::write(&first, b"x").unwrap();
    let second = save_place_in(&folder, "downloads.dmlist").unwrap();
    assert_eq!(second, folder.join("downloads (1).dmlist"));
    assert_eq!(save_place_in(&folder, "").unwrap(), folder.join("EDM"));
    let _ = std::fs::remove_dir_all(&dir);
}
