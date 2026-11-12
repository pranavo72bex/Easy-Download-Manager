// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The ways dm runs with no window, for every window's `main`: the browser's
//! host (framed JSON on stdin and stdout), the warm page reader behind the
//! panel's list, `--version`, `--help`, the command-line client of a running
//! dm (`list`, `status`, `pause`, `resume`, `cancel`), the MCP server,
//! `--register` and `--unregister`, and a word that is no link, file or
//! command. Each says and does exactly what it did in the egui window's own
//! `main`, which these were taken from.

use dm_shell::{host, settings, shell};

/// What `dm-desktop --help` prints.
pub const USAGE: &str = "\
dm — a download manager

Usage:
  dm-desktop [options] [LINK ...]

Each link (http, https, ftp, magnet, or a .torrent file) is downloaded by the
dm already running, or by one started for it. Options apply to the link that
follows them:
  --save-to FOLDER   save the next link into FOLDER
  --name NAME        save the next link as NAME
  --later            add the next link to the queue without starting it
  --quiet            ask nothing (no Download details) for every link after it
  --start-queue      start the main queue
  --new-download     open Add URL (what the taskbar's jump list does)
  --when-done WHAT   once the downloads are done: nothing, exit, sleep,
                     hibernate or shutdown (for this session)

Other commands:
  --hidden           start in the tray, with no window
  --headless         run with no window at all
  --register [ID]    connect dm to the installed browsers (ID: an extension id)
  --unregister       disconnect dm from the browsers and remove its login item
  mcp                serve dm's commands to an AI client (Model Context Protocol)
  list [--json]      what dm is downloading (dm must run with --control 0)
  status ID          one download
  pause|resume|cancel ID…   act on downloads by their numbers from `list`
  --version          print dm's version
  --help             print this

Examples:
  dm-desktop https://example.com/file.zip
  dm-desktop --save-to ~/Videos --name talk.mp4 https://example.com/v.mp4
  dm-desktop --later https://example.com/a.iso --later https://example.com/b.iso
  dm-desktop --start-queue --when-done shutdown

Portable: an empty file named \"portable\" beside the program keeps dm's
settings and list in a \"dm-data\" folder beside it, and leaves the
browsers and the login items to an installed dm.
";

/// On Windows a release build is a GUI program with no console, so what
/// `--register` and `--unregister` print went nowhere when run from cmd or
/// PowerShell. Attached to the console of whatever started it,
/// `println!` reaches the terminal; with none (started from Explorer)
/// nothing changes.
pub fn attach_console() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        // SAFETY: no pointers; it fails harmlessly when there is no parent
        // console or one is already attached.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// Runs the mode `args` ask for that has no window: `Some(code)` to end the
/// program with, or `None` for the window's own start (the instance lock,
/// the window). Call it first, before any thread starts, once
/// `settings::enter_portable` has run.
pub fn without_window(args: &[String]) -> Option<i32> {
    // The browser's host asking for a page to be read behind the panel's
    // list, in a process that outlives it: one page on stdin, nothing shown.
    if args.len() == 1 && args.first().map(String::as_str) == Some(host::WARM_FLAG) {
        host::warm_main(&settings::home());
        return Some(0);
    }

    // Launched by a browser rather than a person: framed JSON on stdin and
    // stdout, no window, nothing else printed.
    if shell::browser_launch(args) {
        // For an extension dm knows, or not at all: the browser's own check
        // (its manifest names the extensions) is the one that counts, and
        // this holds where something else started dm in a browser's name.
        if !shell::launcher_known(args) {
            eprintln!("dm: not started for an extension dm knows ({})", args[0]);
            return Some(0);
        }
        host::run(&settings::home());
        return Some(0);
    }

    // `--version`: the version, and nothing else. Without it, `dm --version`
    // opened the window (found checking an installed AppImage, 0.2.0).
    if matches!(args.first().map(String::as_str), Some("--version" | "-V")) {
        println!("dm {}", env!("CARGO_PKG_VERSION"));
        return Some(0);
    }

    // `--help`: how to use dm from a command line, and nothing else. It
    // opened the window.
    if matches!(
        args.first().map(String::as_str),
        Some("--help" | "-h" | "/?")
    ) {
        attach_console();
        print!("{USAGE}");
        return Some(0);
    }

    // `list`, `status`, `pause`, `resume`, `cancel`: a client of the running
    // dm's control protocol, like `mcp`. Takes no lock.
    if args.first().is_some_and(|a| crate::cli::is_command(a)) {
        attach_console();
        return Some(crate::cli::main(args, &settings::home()));
    }

    // `mcp`: a Model Context Protocol server on stdin and stdout, relaying
    // to the running dm's control protocol. Takes no lock: it is a client.
    if args.first().map(String::as_str) == Some("mcp") {
        return Some(crate::mcp::main(&args[1..], &settings::home()));
    }

    // `--register [id…]` / `--unregister`: the browsers' host manifests,
    // written or removed, and nothing else. No window, no lock.
    if matches!(
        args.first().map(String::as_str),
        Some("--register" | "--unregister")
    ) {
        attach_console();
    }
    if args.first().map(String::as_str) == Some("--unregister") {
        crate::login::remove_login_item();
        crate::login::unregister_the_rest();
    }
    if let Some(code) = dm_shell::register::from_args(args) {
        return Some(code);
    }

    // A word that is no link, file or option is a typo, not a request to
    // open the window: say so and stop (`dm lst` used to open dm silently).
    let strays = shell::unrecognised(args);
    if !strays.is_empty() {
        attach_console();
        for word in &strays {
            eprintln!("dm: {word:?} is not a link, a file or a command (see --help)");
        }
        return Some(2);
    }
    None
}
