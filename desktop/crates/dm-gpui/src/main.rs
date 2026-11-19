// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's window in GPUI. It starts dm (`dm_desktop_base::boot`: one engine,
//! one queue), reads the queue once each time it is woken, and draws its
//! rows.
//!
//! What the command line asks for is one `dm_desktop_base::modes::Mode`:
//! each mode with no window runs there and ends the program, and
//! `--headless` runs dm with no window and no GPUI at all
//! (`dm_desktop_base::headless`).

// A release opens no console window beside dm's own on Windows; the
// command-line modes attach to the terminal they were started from
// (`dm_desktop_base::modes`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// As every dm crate: no unwrap, expect or panic in shipped code (the rest of the
// lints are the workspace's, `desktop/Cargo.toml` `[workspace.lints]`).
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

mod assets;
mod dialogs;
mod error;
mod options;
mod platform;
mod port;
mod probe;
mod widgets;
mod window;

use std::sync::Arc;

/// On macOS a large block goes back to the system when it is freed: the
/// system's allocator kept a download's buffers long after it ended
/// (`dm_core::memory`, gaps row 175).
#[cfg(target_os = "macos")]
#[global_allocator]
static ALLOCATOR: dm_core::memory::Allocator = dm_core::memory::Allocator;

use dm_desktop_base::boot::{self, Wake};
use dm_desktop_base::{i18n, modes::Mode};
use gpui::{
    point, prelude::*, px, size, App, Bounds, TitlebarOptions, WindowBounds, WindowOptions,
};

fn main() {
    // The start's stages, where DM_FINISH_TRACE names a file (gaps row S16).
    dm_core::finish_trace::mark("start: main");
    // A portable copy's home, before any thread starts.
    dm_shell::settings::enter_portable();

    let args = dm_shell::shell::args();
    // What the command line asks for, read once (`dm_desktop_base::modes`).
    // Every way dm runs with no window ends here: the browser's host, the
    // warm page reader, --version, --help, the command-line client, MCP,
    // --register and --unregister, a typo.
    let mode = Mode::parse(&args);
    if let Some(code) = mode.run(&args) {
        if code == 0 {
            return;
        }
        std::process::exit(code);
    }

    let handed = dm_shell::shell::orders(&args);
    let _running = match boot::claim(&handed) {
        boot::Claimed::Running(lock) => lock,
        boot::Claimed::HandedOff => return,
        boot::Claimed::Refused(why) => {
            dm_core::note!(error, "dm: {why}");
            std::process::exit(1);
        }
    };

    // A magnet link or a .torrent file the system opens with dm arrives as an
    // Apple event, not a command line: handed to the running dm as one.
    #[cfg(target_os = "macos")]
    dm_desktop_base::opened::listen(|words| {
        dm_shell::shell::opened_by_system(&dm_shell::settings::home(), &words);
    });

    // `--headless`: no window and no GPUI at all, so it runs where there is
    // no display (`dm_desktop_base::headless`).
    let headless = mode == Mode::Headless;
    boot_and_run(&args, &handed, headless);
}

/// Starts dm (`boot::start`) once the command line has asked for its window or `--headless`,
/// and runs it until it ends: with no window for `--headless`, else in its window.
fn boot_and_run(args: &[String], handed: &dm_shell::shell::Orders, headless: bool) {
    // Woken from any thread, through the one wake channel; read on the window's own, in
    // `window::list::Downloads`, or by the headless loop.
    let (wakes, woken) = dm_desktop_base::wakes::pair();
    let wake: Arc<dyn Fn(Wake) + Send + Sync> = Arc::new(move |wake: Wake| wakes.send(wake));
    let platform: Arc<dyn dm_shell::app::backend::Platform> =
        Arc::new(dm_desktop_base::desktop::Desktop {
            window: platform::services::GpuiServices,
        });
    let boot::Booted {
        runtime: _runtime,
        backend,
        control,
        assistants_only,
        show_asked,
        hidden,
        login_item,
        ..
    } = match boot::start(args, handed, platform, wake.clone()) {
        Ok(booted) => {
            dm_core::finish_trace::mark("start: settings, client and queue read");
            booted
        }
        Err(why) => {
            dm_core::note!(error, "{why}");
            std::process::exit(1);
        }
    };

    // Windows ending the session (a logoff, a shutdown or restart) ends dm
    // as Exit does, the downloads stopped and awaited before it is answered,
    // with the window or without (gaps row 186).
    dm_desktop_base::signals::on_session_end(&backend, {
        let wake = Arc::clone(&wake);
        move || wake(Wake::Now)
    });

    // Every word in the language the person chose (or the system's), from
    // the first frame: the window, its menus, the tray and the jump list
    // are made on this thread, where the language in force is kept. Without
    // it the window spoke English after every start until Options was next
    // applied.
    i18n::set(i18n::chosen(&backend.settings().general.language));

    // Mica behind the window where it was chosen and Windows 11 can draw it
    // (Options › General, from this start: gaps row G11).
    let mica = dm_desktop_base::mica::use_it(backend.settings().general.mica);

    // What dm's own drop targets take wakes the window to read it.
    {
        let wake = Arc::clone(&wake);
        dm_desktop_base::drops::on_arrival(move || wake(Wake::Now));
    }

    // The control port, where asked for (`--control`, DM_CONTROL, or the
    // person's setting); the list read on the port's own thread.
    let requests = control.and_then(|port| {
        match port::open(port, Arc::clone(&wake), backend.clone(), assistants_only) {
            Ok(requests) => {
                // What it reads and clicks: the words as drawn.
                window::screen::record();
                Some(requests)
            }
            Err(e) => {
                backend.note(dm_desktop_base::i18n::tf(
                    "the control protocol could not start: {}",
                    &[&e],
                ));
                None
            }
        }
    });

    // `--headless`: the queue, the browser's hand-offs and the control
    // protocol with no window, for a server, a script or an agent, until
    // Exit over the port, When done or a signal ends it.
    if headless {
        #[cfg(not(windows))]
        platform::notify::claim();
        if let Some(item) = login_item {
            item.follow(&backend, Arc::clone(&wake));
        }
        dm_desktop_base::headless::run(backend, requests, assistants_only, &woken);
        return;
    }

    // An installed dm tells the browsers where it is, so the extension works without a
    // terminal and no browser is left pointing at an old copy of dm (not a development build
    // or a test's dm: `register::may_claim`). Off the window's thread: it writes a file for
    // each browser, and the first frame is not held back for them.
    {
        let backend = backend.clone();
        let _ = std::thread::Builder::new()
            .name("dm-register".into())
            .spawn(move || {
                if let Some(words) = dm_shell::register::ensure_and_say() {
                    backend.note(words);
                }
            });
    }

    windowed(
        Ready {
            backend,
            woken,
            requests,
            assistants_only,
            show_asked,
            hidden,
            mica,
            login_item,
        },
        &wake,
    );
}

/// What `boot_and_run` hands the window's run: the application, and what the window is made
/// with.
struct Ready {
    /// The application, as every window asks it.
    backend: dm_shell::app::backend::Backend,
    /// Where the window hears it is woken.
    woken: dm_desktop_base::wakes::Woken,
    /// The control port's requests, where it is open.
    requests: Option<std::sync::mpsc::Receiver<dm_desktop_base::control::Request>>,
    /// The port answers AI assistants and nothing else.
    assistants_only: bool,
    /// Set when another dm or the browser asked for the window to be shown.
    show_asked: Arc<std::sync::atomic::AtomicBool>,
    /// Launched by the login item: into the tray, no window yet.
    hidden: bool,
    /// Mica behind the window.
    mica: bool,
    /// The login item, put in step once the window is up.
    login_item: Option<boot::LoginItem>,
}

/// dm in its window: the application run, the main window opened in it, until dm ends.
fn windowed(ready: Ready, wake: &Arc<dyn Fn(Wake) + Send + Sync>) {
    let Ready {
        backend,
        woken,
        requests,
        assistants_only,
        show_asked,
        hidden,
        mica,
        login_item,
    } = ready;

    // The login item, put in step once the window is up (gaps row S16):
    // asked of the system before it, it held the first frame back.
    let login_item = login_item.map(|item| (item, backend.clone(), Arc::clone(wake)));

    let application = gpui_platform::application().with_assets(assets::Assets);
    // A click on the Dock icon with the window put away in the tray asks
    // for it back; without this the click would do nothing.
    {
        let (flag, wake) = (Arc::clone(&show_asked), Arc::clone(wake));
        application.on_reopen(move |_| {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            wake(Wake::Now);
        });
    }
    application.run(move |cx: &mut App| {
        // The text fields and the system's own menus (gpui-component).
        gpui_component::init(cx);
        // Notifications are posted as dm's own (macOS: its bundle id),
        // claimed after GPUI's own notification centre has started: the
        // claim before it made the system look for a bundle a bare
        // binary does not have, and dm stopped at launch.
        #[cfg(not(windows))]
        platform::notify::claim();
        // Their colours the system's appearance, light or dark.
        gpui_component::Theme::sync_system_appearance(None, cx);
        // Everything at the text size chosen (View › Text size), from the
        // first frame: the theme's rem with it.
        window::zoom::start(backend.settings().text_size, cx);
        let (least, window_bounds, display_id) = where_it_opens(cx);
        // The list, as the window's build makes it.
        let made: std::rc::Rc<std::cell::RefCell<Option<gpui::Entity<window::list::Downloads>>>> =
            Default::default();
        let opened = crate::window::screen::open_main_window(
            cx,
            WindowOptions {
                window_bounds: Some(window_bounds),
                display_id,
                window_min_size: Some(least),
                // A login launch goes into the tray, with no window yet.
                show: !hidden,
                // The top bar is the title bar on macOS (beside the system's
                // traffic lights) and on Windows (ending with the window's
                // buttons); on Linux the desktop draws its own title bar and
                // the top bar is the row under it, as a code editor's is
                // there with the system's title bar (`titlebar.rs`).
                titlebar: Some(TitlebarOptions {
                    title: Some("dm".into()),
                    appears_transparent: window::titlebar::DRAWN_BY_DM,
                    traffic_light_position: Some(point(
                        px(window::titlebar::traffic_lights_at().0),
                        px(window::titlebar::traffic_lights_at().1),
                    )),
                }),
                window_decorations: (!window::titlebar::DRAWN_BY_DM)
                    .then_some(gpui::WindowDecorations::Server),
                // Under Mica the frame is drawn clear and the backdrop is
                // its colour (`window::theme::frame`); the list stays solid.
                window_background: if mica {
                    gpui::WindowBackgroundAppearance::MicaBackdrop
                } else {
                    gpui::WindowBackgroundAppearance::Opaque
                },
                ..Default::default()
            },
            |window, cx| {
                crate::window::screen::set_title(window, "dm", cx);
                let view = cx.new(|cx| {
                    window::list::Downloads::new(
                        backend,
                        woken,
                        requests,
                        assistants_only,
                        show_asked,
                        hidden,
                        window,
                        cx,
                    )
                });
                *made.borrow_mut() = Some(view.clone());
                // In gpui-component's root, as each dialog's window is: on
                // Linux the top bar's menus are drawn in the overlay it holds,
                // and without it a click on Tasks or Downloads would open
                // nothing (real input).
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            },
        );
        let view = match (opened, made.borrow_mut().take()) {
            (Ok(_), Some(view)) => view,
            (Err(e), _) => {
                dm_core::note!(error, "dm-gpui: the window could not be opened: {e}");
                cx.quit();
                return;
            }
            (Ok(_), None) => {
                dm_core::note!(error, "dm-gpui: the window opened without its list");
                cx.quit();
                return;
            }
        };
        // The window is made (shown, or in the tray for a login launch): the
        // login item now, on a thread of its own.
        if let Some((item, backend, wake)) = login_item {
            item.follow(&backend, wake);
        }
        wire(&view, cx);
        // The commands themselves are the list's (`Downloads`' root element
        // handles `window::menus::Run`), reached through the window's own dispatch,
        // as a key or the menu bar sends them: the list keeps the keyboard.
        cx.activate(true);
    });
}

/// Where the main window opens and how small it may be: where and as large as it was left,
/// where that is still on a display; else in the middle (`frame.rs`).
fn where_it_opens(
    cx: &mut App,
) -> (
    gpui::Size<gpui::Pixels>,
    WindowBounds,
    Option<gpui::DisplayId>,
) {
    let least = size(px(640.0), px(400.0));
    let displays: Vec<(u64, Bounds<gpui::Pixels>)> = cx
        .displays()
        .iter()
        .map(|display| (u64::from(display.id()), display.bounds()))
        .collect();
    let main_display = cx.primary_display().map(|display| display.bounds());
    let kept = window::frame::kept(&dm_shell::settings::home()).and_then(|(kept, display)| {
        if window::frame::DESKTOP_PLACES_IT {
            return window::frame::sized(kept, main_display?, least).map(|kept| (kept, None));
        }
        window::frame::on_a_display(kept, display, &displays, least)
            .map(|kept| (kept, display.map(gpui::DisplayId::from)))
    });
    let (window_bounds, display_id) = kept.unwrap_or_else(|| {
        // The first start's size: on Windows as reference A's window and
        // the system's own programs open, not most of a laptop's screen
        // (900 by 600 takes two thirds of 1366 by 768, the maintainer's
        // word), as wide as the toolbar's buttons need;
        // macOS's as it was.
        let (width, height) = widgets::native::WindowSize::Main.here();
        let first = size(px(width), px(height));
        let middle = Bounds::centered(None, first, cx);
        (WindowBounds::Windowed(middle), None)
    });
    (least, window_bounds, display_id)
}
/// What the application hears beside the window, once the main window is made: dm's menus in
/// the system's menu bar and every command's key, Quit and View › Text size from any window in
/// front. Run by `main` and by the window's tests (`harness.rs`), which start it as dm does.
fn wire(view: &gpui::Entity<window::list::Downloads>, cx: &mut App) {
    // dm's menus in the system's menu bar, and every command's keys.
    window::menus::bind_keys(cx);
    view.update(cx, |view, cx| view.refresh_menus(cx));
    // Quit is heard from any window in front (a dialog's too), as in
    // every macOS program: the main window's view ends dm.
    let view = view.downgrade();
    let text_view = view.clone();
    cx.on_action(move |_: &window::menus::Quit, cx| match view.upgrade() {
        Some(view) => view.update(cx, |view, cx| view.ask_quit(cx)),
        None => cx.quit(),
    });
    // View › Text size and its keys, heard from any window in front, a
    // dialog's too: kept by the list, which keeps the settings.
    let text_size = move |up: Option<bool>| {
        let view = text_view.clone();
        move |cx: &mut App| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |view, cx| view.step_text_size(up, cx));
            }
        }
    };
    let larger = text_size(Some(true));
    cx.on_action(move |_: &window::viewmenu::TextLarger, cx| larger(cx));
    let smaller = text_size(Some(false));
    cx.on_action(move |_: &window::viewmenu::TextSmaller, cx| smaller(cx));
    let normal = text_size(None);
    cx.on_action(move |_: &window::viewmenu::TextNormal, cx| normal(cx));
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod i18n_check;

#[cfg(test)]
mod harness;

/// Whether the window looks in dm's inbox at its ticks: always, but in the tests only a window
/// whose test holds it (`harness::Inbox`), the home being every test's.
#[cfg(test)]
fn reads_inbox() -> bool {
    harness::reads_inbox()
}

/// Whether the window looks in dm's inbox at its ticks: always.
#[cfg(not(test))]
fn reads_inbox() -> bool {
    true
}

#[cfg(test)]
mod window_tests;

#[cfg(test)]
use dm_test_support::secrets as test_secrets;
