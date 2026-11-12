// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm, with its window drawn in egui (the window before GPUI's).
//!
//! The same startup, browser host, lock, inbox and queue as the GPUI window, all from dm-shell.
//! What lives here is only what a real desktop has and a test must not touch: the tray, the
//! login item, notifications, the clipboard.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use dm_desktop::app::DmApp;
use dm_desktop::backend::Platform;
use dm_shell::{settings, shell};

#[cfg(target_os = "macos")]
mod dock;

mod hidden;
mod platform;
mod tray;
mod window;

use dm_desktop_base::signals::*;
use hidden::*;
use platform::*;
use tray::*;
use window::*;

const ICON: &[u8] = include_bytes!("../icons/128x128.png");

/// What the tray asks for.
enum Tray {
    Show,
    Quit,
    /// The tray carries the speed limiter; the Scheduler opens over the
    /// window.
    Run(dm_desktop::commands::Command, bool),
}

/// The window, with the tray and the close-into-the-tray behaviour around it.
struct Window {
    app: DmApp,
    tray: mpsc::Receiver<Tray>,
    /// Whether there is a tray to hide into. Without one, closing the window
    /// has to quit: hiding it would leave no way back.
    has_tray: bool,
    /// A login launch, hidden from the start: if no tray is showing by
    /// this moment, the window opens rather than leave dm unreachable.
    hidden_until: Option<std::time::Instant>,
    quitting: bool,
    /// The window is in the tray: nothing is drawn, and dm wakes only as
    /// often as its timers need.
    hidden: bool,
    /// The window is on a Wayland display, where a window cannot be hidden
    /// or unminimized by its own program (winit's `set_visible` does
    /// nothing there). Learnt from the first frame's display handle.
    wayland: bool,
    /// Set by a second launch through the inbox, or a click on the Dock
    /// icon, asking for the window back.
    show_asked: Arc<AtomicBool>,
    /// The look last told to the window's title bar (Windows).
    title_bar_in_force: Option<(bool, egui::Color32, egui::Color32)>,
    taskbar: dm_desktop::taskbar::Taskbar,
    /// Kept alive for as long as the window: dropping the icon removes it.
    /// With its menu's items, relabelled when the language changes.
    icon: Option<TrayMade>,
    /// The language the tray's words are in.
    tray_language: String,
    /// What the tray's tooltip says now, so it is set only when it changes.
    tray_tip: String,
    /// Where the window was when it went to the tray: it comes back there.
    /// A window manager places a window shown again as it likes, which was
    /// somewhere else each time.
    hidden_at: Option<egui::Pos2>,
    /// Whether it was maximized then: shown again it came back at its
    /// restored size in a corner.
    hidden_maximized: bool,
    /// The last registration failure said in the window, so opening dm
    /// again does not say it again.
    browsers_said: Option<String>,
}

/// Where the window's size and place are kept: under a `DM_HOME` when one
/// is set, beside that run's own settings and queue, so a test's or a
/// screenshot's dm does not write the person's real window state; the
/// system's place for dm otherwise.
fn window_state_file(home: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    home.filter(|h| !h.is_empty())
        .map(|h| std::path::PathBuf::from(h).join("window.ron"))
}

/// Starts the control protocol and says where in dm's home.
fn start_control(app: &mut DmApp, port: u16, ctx: &egui::Context, assistants_only: bool) {
    let home = settings::home();
    let started = dm_desktop::control::fresh_token(&home).and_then(|token| {
        let (to_window, requests) = mpsc::channel();
        let wake = ctx.clone();
        let port =
            dm_desktop::control::listen(port, token, to_window, move || wake.request_repaint())?;
        dm_desktop::control::write_port(&home, port)?;
        Ok((port, requests))
    });
    match started {
        Ok((_, requests)) => {
            app.attach_control(requests);
            app.restrict_control_to_assistants(assistants_only);
        }
        Err(e) => app
            .backend
            .note(format!("the control protocol could not start: {e}")),
    }
}

fn window_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(ICON).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

fn main() {
    // dm started as the JavaScript engine's host (dm-core's javascript.rs):
    // one program on stdin, its answer on stdout, nothing else touched or
    // shown. YouTube's checks run there, so an engine that ends its process
    // ends only that one.
    let mut raw = std::env::args_os().skip(1);
    if let (Some(flag), None) = (raw.next(), raw.next()) {
        if flag == dm_core::javascript::HOST_FLAG {
            std::process::exit(dm_core::javascript::host_main());
        }
    }
    // Every other way dm runs reads with the engine in such a process.
    dm_core::javascript::host_is_this_program();

    // First, before any thread starts: a copy with a "portable" file beside
    // it keeps its things beside it, for the host and the window alike.
    settings::enter_portable();
    let args = shell::args();

    // Every way dm runs with no window (`dm_desktop_base::modes`), as every
    // window's `main` runs them.
    if let Some(code) = dm_desktop_base::modes::without_window(&args) {
        if code == 0 {
            return;
        }
        std::process::exit(code);
    }

    let handed = shell::orders(&args);

    // One dm at a time (`dm_desktop_base::boot`, as every window starts
    // dm). The lock is held for the life of this process by living to the
    // end of `main`.
    let headless = args.iter().any(|a| a == "--headless");
    let _running = match dm_desktop_base::boot::claim(&handed) {
        dm_desktop_base::boot::Claimed::Running(lock) => lock,
        dm_desktop_base::boot::Claimed::HandedOff => return,
        // Said, where a person launching from the Dock will see it too.
        dm_desktop_base::boot::Claimed::Refused(why) => {
            eprintln!("dm: {why}");
            if !headless {
                let _ = rfd::MessageDialog::new()
                    .set_title("dm could not start")
                    .set_description(&why)
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }
            std::process::exit(1);
        }
    };
    // A magnet link or a .torrent file the system opens with dm arrives as an
    // Apple event, not a command line: handed to the window as one.
    #[cfg(target_os = "macos")]
    dm_desktop_base::opened::listen(|words| {
        shell::opened_by_system(&settings::home(), &words);
    });

    // Made before dm starts, so a signal or a change during startup still
    // reaches the window once there is one.
    let repaint_early: Arc<std::sync::OnceLock<egui::Context>> = Arc::default();

    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application("com.dm.desktop");

    let platform: Arc<dyn Platform> =
        Arc::new(dm_desktop_base::desktop::Desktop { window: EguiWindow });
    let wake: Arc<dyn Fn(dm_desktop_base::boot::Wake) + Send + Sync> = {
        let repaint = Arc::clone(&repaint_early);
        Arc::new(move |wake: dm_desktop_base::boot::Wake| {
            if let Some(ctx) = repaint.get() {
                match wake {
                    // Requests within the quarter are one.
                    dm_desktop_base::boot::Wake::Soon => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(250))
                    }
                    dm_desktop_base::boot::Wake::Now => ctx.request_repaint(),
                }
            }
        })
    };
    // The engine's runtime lives to the end of `main` (`_runtime`, not `_`,
    // which would drop it here).
    let dm_desktop_base::boot::Booted {
        runtime: _runtime,
        backend,
        show_asked,
        hidden,
        control,
        assistants_only,
        started_with,
        login_item,
    } = match dm_desktop_base::boot::start(&args, &handed, platform, Arc::clone(&wake)) {
        Ok(booted) => booted,
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    };
    // The login item, put in step on a thread of its own (gaps row S16).
    // At once rather than once the window is made: a login launch makes
    // this window only when it is first asked for, and the item must follow
    // the setting all the same. A refusal waits in the banner for the
    // window's first frame.
    if let Some(item) = login_item {
        item.follow(&backend, wake);
    }
    // Read once: Mica is the window's from its making, so a change waits
    // for the next start.
    let mica = dm_desktop::chrome::use_mica(started_with.mica);
    let repaint = repaint_early;

    // `--headless`: the queue, the browser hand-off and the control
    // protocol, with no window — for a server, a script or an agent.
    if headless {
        let ctx = egui::Context::default();
        let mut app = DmApp::new(backend);
        app.headless = true;
        if let Some(port) = control {
            start_control(&mut app, port, &ctx, assistants_only);
        }
        while !app.quit_asked && !SIGNALLED.load(Ordering::SeqCst) {
            app.logic(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        app.backend().shut_down();
        dm_desktop::control::closing();
        dm_desktop::control::forget_port(&settings::home());
        return;
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("dm")
        .with_app_id("dm")
        // Reference A opens 800 by 458; a little more for dm's side panel.
        .with_inner_size([820.0, 500.0])
        // Half the smallest at first: eframe applies this before the saved
        // text size, so at 200% the full one forced a 1440×920 window over
        // the size it was left at. The window sets the real
        // one, divided by the text size, on its first frame.
        .with_min_inner_size([
            dm_desktop::app::MIN_WINDOW.0 / 2.0,
            dm_desktop::app::MIN_WINDOW.1 / 2.0,
        ]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    if mica {
        viewport = viewport.with_transparent(true);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Remembers where the window was and how big, across launches.
        persist_window: true,
        persistence_path: window_state_file(std::env::var_os("DM_HOME")),
        ..Default::default()
    };

    // What the window's creator makes, around the context it is given:
    // eframe's at once, or for a login launch a stand-in's until the window
    // is first asked for.
    let make = move |ctx: &egui::Context| {
        let _ = repaint.set(ctx.clone());
        // A click on the Dock icon of an app whose window is hidden asks
        // for it back; without this the click did nothing.
        #[cfg(target_os = "macos")]
        {
            let (flag, ctx) = (Arc::clone(&show_asked), ctx.clone());
            dock::on_reopen(move || {
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            });
        }
        let (send, receive) = mpsc::channel();
        // The tray's words in the window's language.
        let language = backend.settings().language;
        dm_desktop::i18n::set(dm_desktop::i18n::chosen(&language));
        let built = start_tray(ctx.clone(), send);
        if let Err(why) = &built {
            backend.note(dm_desktop_base::tray::unavailable(why));
        }
        // An installed dm tells the browsers where it is, so the
        // extension works without a terminal. Not a development build
        // or a test's dm (DM_HOME), which must not take the browsers'
        // registration from the installed one.
        let mut browsers_said = None;
        if let Some(why) = dm_shell::register::worth_saying(
            &mut browsers_said,
            dm_shell::register::ensure_if_claimed(),
        ) {
            backend.note(registration_failed(&why));
        }
        let has_tray = built.is_ok();
        let icon = built.ok().flatten();
        let mut app = DmApp::new(backend);
        // The taskbar button's jump list (Windows): Add URL and the folder.
        dm_desktop::jumplist::publish(
            dm_desktop::i18n::t("Add URL"),
            dm_desktop::i18n::t("Open downloads folder"),
            &app.applied.download_dir,
        );
        if let Some(port) = control {
            start_control(&mut app, port, ctx, assistants_only);
        }
        Window {
            app,
            tray: receive,
            has_tray,
            hidden,
            wayland: false,
            hidden_until: hidden
                .then(|| std::time::Instant::now() + std::time::Duration::from_secs(10)),
            quitting: false,
            show_asked,
            title_bar_in_force: None,
            taskbar: dm_desktop::taskbar::Taskbar::default(),
            icon,
            tray_language: language,
            tray_tip: String::new(),
            hidden_at: None,
            hidden_maximized: false,
            browsers_said,
        }
    };
    // The dialogs' own windows take the clicks and keys a control client sends
    // (`dialogs/apart.rs`; `vendor/egui-winit`).
    let _ = egui_winit::OTHER_WINDOWS_INPUT.set(dm_desktop::dialogs::apart::feed);
    let result = if hidden {
        run_hidden(options, make)
    } else {
        eframe::run_native(
            "dm",
            options,
            Box::new(move |cc| Ok(Box::new(make(&cc.egui_ctx)))),
        )
        .map_err(|e| e.to_string())
    };
    dm_desktop::control::closing();
    dm_desktop::control::forget_port(&settings::home());
    if let Err(e) = result {
        eprintln!("dm: the window could not be opened: {e}");
        // Said in the exit status too: a launcher, a login entry or a
        // script asking whether dm started must not be told it did.
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_run_with_its_own_home_keeps_its_window_state_there() {
        assert_eq!(
            super::window_state_file(Some("/tmp/dm-test".into())),
            Some(std::path::PathBuf::from("/tmp/dm-test/window.ron"))
        );
        assert_eq!(super::window_state_file(None), None, "the system's place");
        assert_eq!(super::window_state_file(Some("".into())), None);
    }
}
