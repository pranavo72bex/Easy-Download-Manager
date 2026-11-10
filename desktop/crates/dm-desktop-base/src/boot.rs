// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm started for a person: one dm at a time, the queue read back and run,
//! the browser's hand-offs watched for, the computer kept awake and woken for
//! the schedule, and the application's `Backend` made. Every window starts
//! dm this way, so there is one engine and one way to start it; what a window
//! adds (its own wake-up, the dialogs it shows) it passes in.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dm_shell::app::backend::{Backend, Platform};
use dm_shell::{instance, settings, shell};

/// What a window is asked to do when something it shows may have changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// The queue changed: draw again within a quarter of a second. While
    /// downloads run the queue changes many times a second, and a frame for
    /// each drew the window far more often than it is drawn anyway (10
    /// downloads: 9.7 % of a core became 14.6 %); the window coalesces.
    Soon,
    /// Something to act on now: a hand-off, an order from another dm, a
    /// signal to quit.
    Now,
}

/// Whether this launch is the dm that runs.
pub enum Claimed {
    /// It is: the lock is held for as long as this is kept.
    Running(std::fs::File),
    /// Another dm runs, and was handed this launch's orders.
    HandedOff,
    /// Whether another runs cannot be told: said, and dm does not start.
    Refused(String),
}

/// One dm at a time. A launch while another runs hands its orders to it (and
/// registers with the browsers first: one installed while dm ran was never
/// told where dm is). The running dm's orders from the command line
/// (`--when-done`, `--new-download`) are left for its window.
pub fn claim(handed: &shell::Orders) -> Claimed {
    match instance::claim(&settings::home()) {
        Ok(Some(lock)) => {
            shell::leave_when_done(&settings::home(), handed);
            shell::leave_new_download(&settings::home(), handed);
            Claimed::Running(lock)
        }
        Ok(None) => {
            if let Some(why) = shell::hand_off(
                &settings::home(),
                handed,
                dm_shell::register::ensure_if_claimed,
            ) {
                eprintln!("dm: your browsers could not be told where dm is: {why}");
            }
            Claimed::HandedOff
        }
        // Not taken for a running dm, which it used to be: dm exited without
        // a word, every time. Nor started without it: a second dm started
        // the same way would write the same queue.
        Err(e) => Claimed::Refused(format!(
            "dm cannot tell whether another dm is running, so it has not started: {e}. \
             Remove that file, or make it yours, and start dm again."
        )),
    }
}

/// dm, started.
pub struct Booted {
    /// The engine's runtime: kept for as long as dm runs.
    pub runtime: tokio::runtime::Runtime,
    /// The application, as every window asks it.
    pub backend: Backend,
    /// Set when another dm or the browser asked for the window to be shown.
    pub show_asked: Arc<AtomicBool>,
    /// Launched by the login item: into the tray, no window yet.
    pub hidden: bool,
    /// The control port to open, if any (`--control <port>`, `DM_CONTROL`,
    /// or the person's own setting, on a port the system picks).
    pub control: Option<u16>,
    /// The port was opened only because AI assistants are allowed: it
    /// answers them and nothing else.
    pub assistants_only: bool,
    /// The settings dm started with.
    pub started_with: dm_shell::Settings,
}

/// Starts dm for `args`, carrying out `handed` (the launch's own links and
/// orders), with the desktop's services (`platform`) and the window's
/// wake-up (`wake`, called from any thread).
pub fn start(
    args: &[String],
    handed: &shell::Orders,
    platform: Arc<dyn Platform>,
    wake: Arc<dyn Fn(Wake) + Send + Sync>,
) -> Result<Booted, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("dm could not start: {e}"))?;
    crate::signals::on_signal(&runtime, {
        let wake = Arc::clone(&wake);
        move || wake(Wake::Now)
    });
    let shell::Started {
        queue,
        settings: mut saved,
        mut notices,
    } = runtime.block_on(shell::start(handed));
    let hidden = shell::launched_at_login(args);
    // `--control <port>`, or DM_CONTROL: the control protocol, for tests and
    // agents to drive the running window. Off unless asked for.
    let control: Option<u16> = args
        .iter()
        .position(|a| a == "--control")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .or_else(|| std::env::var("DM_CONTROL").ok())
        .and_then(|p| p.parse().ok())
        // Or the person's own setting (Options › General): the command line.
        .or_else(|| (saved.allow_command_line || saved.allow_mcp).then_some(0));
    // Open only because AI assistants are allowed (not asked for by
    // `--control`, `DM_CONTROL` or the command-line setting): then the port
    // answers assistants and nothing else, so the default does not hand the
    // command line to people who never turned it on.
    let assistants_only = !args.iter().any(|a| a == "--control")
        && std::env::var_os("DM_CONTROL").is_none()
        && !saved.allow_command_line;

    // The login item follows the saved setting, so a reinstall lands in the
    // state the person last chose. Said in the window, not fatal. One the
    // person turned off in Task Manager stays off, and so does the setting.
    // Not for a dm run on a home of its own (`DM_HOME`: a test, a script, a
    // portable copy): starting at login is on for a new install, and every
    // test's fresh home would otherwise put a login item on the computer
    // running it. Turning the setting on in Options still registers one.
    let own_home = std::env::var_os("DM_HOME").is_some_and(|h| !h.is_empty());
    let at_launch = (!own_home).then(|| {
        crate::autostart::at_launch(
            saved.start_on_login,
            crate::login::login_item_turned_off_outside(),
        )
    });
    match at_launch {
        None => {}
        Some(crate::autostart::AtLaunch::TurnedOffOutside) => {
            saved.start_on_login = false;
            notices.push(match saved.save() {
                Ok(()) => "start on login was turned off in Task Manager, so dm has turned it \
                           off too; Options › General turns it back on"
                    .to_string(),
                Err(e) => format!(
                    "start on login was turned off in Task Manager and dm leaves it off, \
                     but the setting could not be saved: {e}"
                ),
            });
        }
        Some(crate::autostart::AtLaunch::Follow(wanted)) => {
            if let Err(why) = platform.set_login_item(wanted) {
                notices.push(format!(
                    "could not change start-on-login: {why}; start on login is not in force"
                ));
            }
        }
    }

    // Anything a browser handed over while the app was closed is waiting in
    // the inbox; anything that was mid-flight came back queued.
    let show_asked = Arc::new(AtomicBool::new(false));
    // A change to the queue wakes the window, which then has no need to look
    // every second for one while nothing moves. The queue tells the
    // application (`dm_shell::app::changes`), which marks the change and
    // tells each presentation subscribed.
    dm_shell::app::changes::connect(&queue);
    dm_shell::app::changes::subscribe({
        let wake = Arc::clone(&wake);
        move || wake(Wake::Soon)
    });
    let handed_over: Arc<Mutex<std::collections::VecDeque<dm_shell::host::Request>>> =
        Arc::default();
    {
        let (flag, wake_shown) = (Arc::clone(&show_asked), Arc::clone(&wake));
        let (asked, ask_flag, wake_asked) = (
            Arc::clone(&handed_over),
            Arc::clone(&show_asked),
            Arc::clone(&wake),
        );
        let wake_quit = Arc::clone(&wake);
        runtime.spawn(shell::watch_inbox(
            queue.clone(),
            move || {
                flag.store(true, Ordering::Relaxed);
                wake_shown(Wake::Now);
            },
            // A video from the panel: shown in the window's own dialog.
            move |request| {
                dm_core::lock(&asked).push_back(request);
                ask_flag.store(true, Ordering::Relaxed);
                wake_asked(Wake::Now);
            },
            // `--unregister` of an uninstall: ended as a stop signal ends dm.
            move || {
                crate::signals::SIGNALLED.store(true, Ordering::SeqCst);
                wake_quit(Wake::Now);
            },
        ));
        let queue = queue.clone();
        runtime.spawn(async move { queue.run().await });
    }

    // The computer kept awake for as long as a download is running.
    {
        let (queue, handle) = (queue.clone(), runtime.handle().clone());
        crate::awake::watch(move || {
            handle
                .block_on(queue.list())
                .iter()
                .any(|row| row.state == dm_core::State::Running)
        });
    }
    let started_with = saved.clone();
    let mut backend = Backend::new(runtime.handle().clone(), queue, saved, notices, platform);
    // A sleeping computer woken for the schedule, where the Scheduler says.
    {
        let settings = backend.settings_handle();
        crate::wake::watch(move || {
            let now = dm_core::lock(&settings).clone();
            if !now.schedule.wake {
                return None;
            }
            let starts: Vec<u32> = now
                .queue_start_list()
                .into_iter()
                .map(|(_, at)| at)
                .collect();
            let (weekday, minute) = dm_core::weekday_now();
            dm_core::minutes_until_next_start(&now.schedule, &starts, weekday, minute)
        });
    }
    backend.handed = handed_over;
    Ok(Booted {
        runtime,
        backend,
        show_asked,
        hidden,
        control,
        assistants_only,
        started_with,
    })
}
