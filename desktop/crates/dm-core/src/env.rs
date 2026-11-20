// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every `DM_` switch dm reads from its environment, in one table (gaps row A11): its name, what
//! it is for, and whether only a test or a check sets it. A switch is read with [`get`] or
//! [`get_os`], which every crate above this one calls. `cargo xtask layers` refuses a `DM_`
//! variable read anywhere else in the crates' code, and a `DM_` name there that [`SWITCHES`] does
//! not hold.

use std::ffi::OsString;

/// One `DM_` switch: a variable of the environment that changes what dm does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switch {
    /// The variable's name.
    pub name: &'static str,
    /// What setting it does, in one sentence.
    pub purpose: &'static str,
    /// Whether only a test or a check sets it: a person running dm never needs to.
    pub test_only: bool,
}

/// Every `DM_` switch, a person's first and then the tests' (each group by name). `cargo xtask
/// layers` reads the names from this table's `name:` lines.
pub const SWITCHES: &[Switch] = &[
    Switch {
        name: "DM_CONTROL",
        purpose: "The local port the window opens for scripts and assistants to drive it, as \
                  `--control <port>` gives.",
        test_only: false,
    },
    Switch {
        name: "DM_ENGINE",
        purpose: "The program `--register` names in the browsers' manifests, in place of this \
                  one.",
        test_only: false,
    },
    Switch {
        name: "DM_FINISH_TRACE",
        purpose: "A file each stage of a download's finish (and of the window's start) is \
                  appended to with its time, for finding where the time goes.",
        test_only: false,
    },
    Switch {
        name: "DM_HOME",
        purpose: "A folder of dm's own for this run's settings, queue and inbox, so a test, a \
                  script or a portable copy keeps clear of the installed one.",
        test_only: false,
    },
    Switch {
        name: "DM_NO_CHUNK",
        purpose: "Switches off the cap a video page puts on each request, for timing a long \
                  video with and without it.",
        test_only: false,
    },
    Switch {
        name: "DM_BENCH_CA",
        purpose: "A certificate file (PEM) dm trusts beside the public roots, for the \
                  benchmark's `wan` case served over TLS with a certificate of its own \
                  (bench/README.md).",
        test_only: true,
    },
    Switch {
        name: "DM_BENCH_SMALL",
        purpose: "Leaves out `tests/throughput.rs`'s half-gigabyte cases, which take minutes in a \
                  debug build.",
        test_only: true,
    },
    Switch {
        name: "DM_CODEC",
        purpose: "The codec `examples/youtube_read.rs` asks a video's picture in.",
        test_only: true,
    },
    Switch {
        name: "DM_FETCH_FIRST",
        purpose: "Makes `examples/youtube_read.rs` fetch the first bytes of what it read.",
        test_only: true,
    },
    Switch {
        name: "DM_FILE_HOST_STAND_IN",
        purpose: "A test's server that stands in for every file-storage service.",
        test_only: true,
    },
    Switch {
        name: "DM_FTPS_CA",
        purpose: "The certificate authority's file for the real FTPS server a test is named.",
        test_only: true,
    },
    Switch {
        name: "DM_FTPS_CHECK",
        purpose: "The address of a real FTPS server for a test to download from.",
        test_only: true,
    },
    Switch {
        name: "DM_FTPS_SHA256",
        purpose: "The SHA-256 of the file on the real FTPS server a test is named.",
        test_only: true,
    },
    Switch {
        name: "DM_HIGH_CONTRAST",
        purpose: "`black` or `white`: a debug build off Windows shows that high-contrast \
                  palette, for checking it.",
        test_only: true,
    },
    Switch {
        name: "DM_INTEGRITY_FOLDER",
        purpose: "The folder `tests/integrity.rs`'s one case run alone writes into.",
        test_only: true,
    },
    Switch {
        name: "DM_INTEGRITY_RUN_ONE",
        purpose: "The one case `tests/integrity.rs` runs alone, in a process of its own.",
        test_only: true,
    },
    Switch {
        name: "DM_JS_FILE",
        purpose: "A player script `dm-js`'s `tests/real_scripts.rs` reads (that crate reads it \
                  itself: it is below dm-core).",
        test_only: true,
    },
    Switch {
        name: "DM_JS_PLAYER",
        purpose: "A file of YouTube's live player for a test to run in dm's own JavaScript.",
        test_only: true,
    },
    Switch {
        name: "DM_NO_SECRET_STORE",
        purpose: "Keeps the system's store of secrets out of a test's or a check's run.",
        test_only: true,
    },
    Switch {
        name: "DM_NO_SWARM",
        purpose: "Holds torrents off the network in a check's run, so no firewall question \
                  is raised.",
        test_only: true,
    },
    Switch {
        name: "DM_RAISE_FLOOR",
        purpose: "Lets `tests/zoo.rs` raise its kept floor of passing sites when more pass.",
        test_only: true,
    },
    Switch {
        name: "DM_REGISTRY_TEST",
        purpose: "Lets `dm-gpui`'s `tests/magnet_registry.rs` write Windows' registry keys for \
                  magnet links.",
        test_only: true,
    },
    Switch {
        name: "DM_SECRET_STORE_TEST",
        purpose: "Runs the tests that write into the system's real store of secrets.",
        test_only: true,
    },
    Switch {
        name: "DM_SEED_OUT",
        purpose: "A folder `tests/start_timing.rs` leaves its seeded home in, to time a start by \
                  hand.",
        test_only: true,
    },
    Switch {
        name: "DM_SHOW_ADDRESS",
        purpose: "Makes `examples/youtube_read.rs` print the addresses it read.",
        test_only: true,
    },
    Switch {
        name: "DM_STAGE_BY_RANGE",
        purpose: "`1` or `0`: each range in a file of its own or not, whatever the disks, for \
                  tests and for measuring the two.",
        test_only: true,
    },
    Switch {
        name: "DM_STATE_FILE",
        purpose: "A file a check writes the computer's state in (`metered`, `battery <percent>`, \
                  a line each), which a debug build reads in place of the system's (gaps row \
                  W11).",
        test_only: true,
    },
    Switch {
        name: "DM_STRESS_TORRENT_DIR",
        purpose: "The folder `tests/torrent_stress.rs` checks paths outside of.",
        test_only: true,
    },
    Switch {
        name: "DM_TEST_<NAME>",
        purpose: "A login or secret a test uses, in place of one made up for the run.",
        test_only: true,
    },
    Switch {
        name: "DM_WRITE_ZOO",
        purpose: "Makes `tests/zoo.rs` write its table of sites out.",
        test_only: true,
    },
    Switch {
        name: "DM_YOUTUBE_HOST",
        purpose: "A test's server that stands in for YouTube.",
        test_only: true,
    },
];

/// Whether [`SWITCHES`] holds a switch called `name`.
pub fn known(name: &str) -> bool {
    SWITCHES.iter().any(|s| s.name == name)
}

/// The switch `name` as text, where it is set and is Unicode (as `std::env::var` reads it). A
/// name the table does not hold is a mistake a debug build stops at.
pub fn get(name: &str) -> Option<String> {
    debug_assert!(known(name), "{name} is not in dm_core::env::SWITCHES");
    std::env::var(name).ok()
}

/// The switch `name` as the system has it, where it is set (as `std::env::var_os` reads it). A
/// name the table does not hold is a mistake a debug build stops at.
pub fn get_os(name: &str) -> Option<OsString> {
    debug_assert!(known(name), "{name} is not in dm_core::env::SWITCHES");
    std::env::var_os(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_switch_is_named_once_with_dms_prefix_and_says_what_it_is_for() {
        for (i, switch) in SWITCHES.iter().enumerate() {
            assert!(switch.name.starts_with("DM_"), "{}", switch.name);
            assert!(!switch.purpose.trim().is_empty(), "{}", switch.name);
            assert!(
                SWITCHES[..i].iter().all(|s| s.name != switch.name),
                "{} is named twice",
                switch.name
            );
        }
    }

    #[test]
    fn a_switch_in_the_table_is_known_and_one_outside_it_is_not() {
        assert!(known("DM_HOME"));
        assert!(!known("PATH"));
        assert!(!known(""));
    }
}
