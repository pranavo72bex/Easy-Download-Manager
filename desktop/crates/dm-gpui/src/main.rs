// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's window in GPUI: the Phase B shell (docs/gaps.md G6). It starts dm as
//! the egui window does (`dm_desktop_base::boot`: one engine, one queue),
//! reads the queue once each time it is woken, and draws its rows; each check
//! of docs/migration/GPUI_MIGRATION_PLAN.md Phase B is made on it.
//!
//! `--headless` runs dm with no window and no GPUI at all
//! (`dm_desktop_base::headless`), as the egui binary's does.
//! `--accessibility` prints what VoiceOver is given of each window and
//! quits; `--churn` first opens and closes a dialog window 50 times.

// A release opens no console window beside dm's own on Windows, as the egui
// binary's does; the command-line modes attach to the terminal they were
// started from (`dm_desktop_base::modes`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// As every dm crate: no unwrap, expect or panic in shipped code, an `unsafe`
// block says why it is sound, a string is built with `format!`.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::string_add, clippy::needless_for_each)]

mod about;
mod actions;
mod addurl;
mod assets;
mod bars;
mod cliplinks;
mod clipwatch;
mod complete;
mod details;
mod dragout;
mod drops;
mod droptarget;
mod fit;
mod frame;
mod grabber;
mod limiter;
mod links;
mod list;
mod menus;
mod native;
#[cfg(not(windows))]
mod notify;
mod options;
mod options_pages;
mod playlist;
mod popup;
mod port;
mod probe;
mod progress;
mod properties;
mod question;
mod rowdialogs;
mod rowmenu;
mod savefolder;
mod scheduler;
mod screen;
mod series;
mod services;
mod sidebar;
mod statusbar;
mod syswindow;
mod table;
mod theme;
mod tick;
mod titlebar;
mod toolbar;
mod tray;
mod viewmenu;
mod wakes;
mod welcome;
mod zoom;

use std::sync::Arc;
use std::time::Duration;

use dm_desktop_base::boot::{self, Wake};
use gpui::{
    div, point, prelude::*, px, size, App, Bounds, Context, Role, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions,
};

/// A dialog in a window of its own, as dm's are (B42): a question, OK and
/// Cancel. Phase B's first check is whether a screen reader reaches both.
struct Question;

/// A button a screen reader can find: an id, the button role, its words.
fn button(id: &'static str, words: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(words)
        .px_3()
        .py_1()
        .border_1()
        .rounded_md()
        .child(words)
}

impl Render for Question {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("question")
            .role(Role::Dialog)
            .aria_label("Delete the file?")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child("Delete the file?")
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button("ok", "OK").into_any_element())
                    .child(button("cancel", "Cancel").into_any_element()),
            )
    }
}

/// Opens the question in a dialog window of its own, over the screen's middle.
fn open_question(cx: &mut App) -> Option<WindowHandle<Question>> {
    let dialog = Bounds::centered(None, size(px(360.0), px(140.0)), cx);
    crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(dialog)),
            kind: crate::question::KIND,
            ..Default::default()
        },
        |window, cx| {
            crate::screen::set_title(window, "Delete", cx);
            cx.new(|_| Question)
        },
    )
    .ok()
}

/// The shell's own words on the command line, taken out before dm reads
/// the rest.
const OWN_FLAGS: [&str; 2] = ["--accessibility", "--churn"];

fn main() {
    // As the egui binary: dm-core's JavaScript host, then a portable copy's
    // home, before any thread starts.
    let mut raw = std::env::args_os().skip(1);
    if let (Some(flag), None) = (raw.next(), raw.next()) {
        if flag == dm_core::javascript::HOST_FLAG {
            std::process::exit(dm_core::javascript::host_main());
        }
    }
    dm_core::javascript::host_is_this_program();
    dm_shell::settings::enter_portable();

    let all = dm_shell::shell::args();
    let dump_accessibility = all.iter().any(|a| a == "--accessibility");
    let churn = all.iter().any(|a| a == "--churn");
    let args: Vec<String> = all
        .into_iter()
        .filter(|a| !OWN_FLAGS.contains(&a.as_str()))
        .collect();
    // Every way dm runs with no window (`dm_desktop_base::modes`): the
    // browser's host, the warm page reader, --version, --help, the
    // command-line client, MCP, --register and --unregister, a typo.
    if let Some(code) = dm_desktop_base::modes::without_window(&args) {
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
            eprintln!("dm: {why}");
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
    let headless = args.iter().any(|a| a == "--headless");
    // Woken from any thread; read on the window's own, in `list::Downloads`,
    // or by the headless loop.
    let (wakes, woken) = wakes::pair();
    let (headless_wakes, headless_woken) = std::sync::mpsc::channel::<Wake>();
    let wake: Arc<dyn Fn(Wake) + Send + Sync> = if headless {
        Arc::new(move |wake: Wake| {
            let _ = headless_wakes.send(wake);
        })
    } else {
        Arc::new(move |wake: Wake| wakes.send(wake))
    };
    let platform: Arc<dyn dm_shell::app::backend::Platform> =
        Arc::new(dm_desktop_base::desktop::Desktop {
            window: services::GpuiServices,
        });
    let boot::Booted {
        runtime: _runtime,
        backend,
        control,
        assistants_only,
        show_asked,
        hidden,
        ..
    } = match boot::start(&args, &handed, platform, wake.clone()) {
        Ok(booted) => booted,
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    };

    // Every word in the language the person chose (or the system's), from
    // the first frame, as the egui window starts: the window, its menus, the
    // tray and the jump list are made on this thread, where the language in
    // force is kept. Without it the window spoke English after every start
    // until Options was next applied.
    dm_desktop_base::i18n::set(dm_desktop_base::i18n::chosen(&backend.settings().language));

    // What dm's own drop targets take wakes the window to read it.
    {
        let wake = Arc::clone(&wake);
        dm_desktop_base::drops::on_arrival(move || wake(Wake::Now));
    }

    // The control port, where asked for (`--control`, DM_CONTROL, or the
    // person's setting), as the egui window opens it; the list read on the
    // port's own thread.
    let requests = control.and_then(|port| {
        match port::open(port, Arc::clone(&wake), backend.clone(), assistants_only) {
            Ok(requests) => {
                // What it reads and clicks: the words as drawn.
                screen::record();
                Some(requests)
            }
            Err(e) => {
                backend.note(format!("the control protocol could not start: {e}"));
                None
            }
        }
    });

    // `--headless`: the queue, the browser's hand-offs and the control
    // protocol with no window, for a server, a script or an agent, until
    // Exit over the port, When done or a signal ends it.
    if headless {
        #[cfg(not(windows))]
        notify::claim();
        dm_desktop_base::headless::run(backend, requests, assistants_only, &headless_woken);
        return;
    }
    drop(headless_woken);

    let application = gpui_platform::application().with_assets(assets::Assets);
    // A click on the Dock icon with the window put away in the tray asks
    // for it back; without this the click did nothing.
    {
        let (flag, wake) = (Arc::clone(&show_asked), Arc::clone(&wake));
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
        notify::claim();
        // Their colours the system's appearance, light or dark.
        gpui_component::Theme::sync_system_appearance(None, cx);
        // Everything at the text size chosen (View › Text size), from the
        // first frame: the theme's rem with it.
        zoom::start(backend.settings().text_size, cx);
        // Where and as large as it was left, where that is still on a
        // display; else in the middle (`frame.rs`).
        let least = size(px(640.0), px(400.0));
        let displays: Vec<(u64, Bounds<gpui::Pixels>)> = cx
            .displays()
            .iter()
            .map(|display| (u64::from(display.id()), display.bounds()))
            .collect();
        let main_display = cx.primary_display().map(|display| display.bounds());
        let kept = frame::kept(&dm_shell::settings::home()).and_then(|(kept, display)| {
            if frame::DESKTOP_PLACES_IT {
                return frame::sized(kept, main_display?, least).map(|kept| (kept, None));
            }
            frame::on_a_display(kept, display, &displays, least)
                .map(|kept| (kept, display.map(gpui::DisplayId::from)))
        });
        let (window_bounds, display_id) = kept.unwrap_or_else(|| {
            // The first start's size: on Windows as reference A's window and
            // the system's own programs open, not most of a laptop's screen
            // (900 by 600 took two thirds of 1366 by 768, the maintainer's
            // word of 2026-11-11), as wide as the toolbar's buttons need;
            // macOS's as it was.
            let first = size(
                px(native::pick(900.0, 840.0, 900.0)),
                px(native::pick(600.0, 520.0, 600.0)),
            );
            let middle = Bounds::centered(None, first, cx);
            (WindowBounds::Windowed(middle), None)
        });
        // The list, as the window's build makes it.
        let made: std::rc::Rc<std::cell::RefCell<Option<gpui::Entity<list::Downloads>>>> =
            Default::default();
        let opened = crate::screen::open_main_window(
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
                    appears_transparent: titlebar::DRAWN_BY_DM,
                    traffic_light_position: Some(point(
                        px(titlebar::traffic_lights_at().0),
                        px(titlebar::traffic_lights_at().1),
                    )),
                }),
                window_decorations: (!titlebar::DRAWN_BY_DM)
                    .then_some(gpui::WindowDecorations::Server),
                ..Default::default()
            },
            |window, cx| {
                crate::screen::set_title(window, "dm", cx);
                let view = cx.new(|cx| {
                    list::Downloads::new(
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
                // and without it a click on Tasks or Downloads opened nothing
                // (real input, 2026-11-11).
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            },
        );
        let view = match (opened, made.borrow_mut().take()) {
            (Ok(_), Some(view)) => view,
            (Err(e), _) => {
                eprintln!("dm-gpui: the window could not be opened: {e}");
                cx.quit();
                return;
            }
            (Ok(_), None) => {
                eprintln!("dm-gpui: the window opened without its list");
                cx.quit();
                return;
            }
        };
        // dm's menus in the system's menu bar, and every command's keys.
        menus::bind_keys(cx);
        view.update(cx, |view, cx| view.refresh_menus(cx));
        // Quit is heard from any window in front (a dialog's too), as in
        // every macOS program: the main window's view ends dm.
        let view = view.downgrade();
        let text_view = view.clone();
        cx.on_action(move |_: &menus::Quit, cx| match view.upgrade() {
            Some(view) => view.update(cx, |view, cx| view.ask_quit(cx)),
            None => cx.quit(),
        });
        // View › Text size and its keys, heard from any window in front, a
        // dialog's too, as the egui window's zoom was: kept by the list,
        // which keeps the settings.
        let text_size = move |up: Option<bool>| {
            let view = text_view.clone();
            move |cx: &mut App| {
                if let Some(view) = view.upgrade() {
                    view.update(cx, |view, cx| view.step_text_size(up, cx));
                }
            }
        };
        let larger = text_size(Some(true));
        cx.on_action(move |_: &viewmenu::TextLarger, cx| larger(cx));
        let smaller = text_size(Some(false));
        cx.on_action(move |_: &viewmenu::TextSmaller, cx| smaller(cx));
        let normal = text_size(None);
        cx.on_action(move |_: &viewmenu::TextNormal, cx| normal(cx));
        // The commands themselves are the list's (`Downloads`' root element
        // handles `menus::Run`), reached through the window's own dispatch,
        // as a key or the menu bar sends them: the list keeps the keyboard.
        if dump_accessibility && open_question(cx).is_none() {
            eprintln!("dm-gpui: the dialog's window could not be opened");
        }
        cx.activate(true);
        if dump_accessibility {
            cx.spawn(async move |cx| dump(cx, churn).await).detach();
        }
    });
}

/// What VoiceOver is given of each window, then out (Phase B check 1). The
/// first ask wakes AccessKit, which GPUI answers with a placeholder and the
/// real tree on the next frame: so ask, draw every window again, then read.
/// With `churn`, a dialog is opened, read and closed 50 times first.
async fn dump(cx: &mut gpui::AsyncApp, churn: bool) {
    cx.background_executor().timer(Duration::from_secs(2)).await;
    cx.update(|cx| {
        let _ = dm_desktop_base::a11y_tree::windows();
        cx.refresh_windows();
    });
    cx.background_executor().timer(Duration::from_secs(1)).await;
    for _ in 0..if churn { 50 } else { 0 } {
        let opened = cx.update(open_question);
        cx.background_executor()
            .timer(Duration::from_millis(80))
            .await;
        cx.update(|cx| {
            let _ = dm_desktop_base::a11y_tree::windows();
            if let Some(handle) = opened {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        });
        cx.background_executor()
            .timer(Duration::from_millis(40))
            .await;
    }
    cx.update(|cx| cx.refresh_windows());
    cx.background_executor()
        .timer(Duration::from_millis(500))
        .await;
    cx.update(|cx| {
        match dm_desktop_base::a11y_tree::windows() {
            Ok(tree) => println!("{tree:#}"),
            Err(why) => eprintln!("dm-gpui: {why}"),
        }
        cx.quit();
    });
}

#[cfg(test)]
mod tests {
    /// The release tools read dm's version from the egui crate's manifest
    /// (the Makefile's `APP_VERSION`, the workflows); macOS's package and
    /// its update check are this crate's: one number, or a release names
    /// itself twice.
    #[test]
    fn this_window_carries_the_version_dm_is_released_under() {
        let manifest = include_str!("../../dm-desktop/Cargo.toml");
        let released = manifest
            .lines()
            .find_map(|line| line.strip_prefix("version = \""))
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or_default();
        assert_eq!(env!("CARGO_PKG_VERSION"), released);
    }

    /// The language chosen is in force before the window and the tray are
    /// made: set only by Options' OK, every word was English after each start
    /// (review of 2026-11-11), where the egui window starts in it.
    #[test]
    fn the_language_chosen_is_in_force_before_the_window_opens() {
        let source = include_str!("main.rs");
        let main = source.split("#[cfg(test)]").next().unwrap_or_default();
        let set = main
            .find("i18n::set(dm_desktop_base::i18n::chosen(")
            .expect("main puts the chosen language in force");
        let run = main
            .find("application.run(")
            .expect("main runs the application");
        assert!(set < run, "the language is set after the window is made");
    }

    /// The windows that save a program dm runs (the command after each
    /// download, the scheduled hours' program, the proxy script) save
    /// through `Downloads::apply_settings`, which keeps those programs when
    /// a script was driving the window a moment ago. A direct
    /// `backend.set_settings` there let a control-port client set the
    /// Scheduler's program (security review, 2026-11-11).
    #[test]
    fn windows_that_set_a_program_save_through_the_guard() {
        // Split so this test's own source does not match it.
        let direct = concat!(".set_", "settings(");
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut read = 0;
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let whole = std::fs::read_to_string(&path).unwrap();
            // The window's own code, not its tests (this one names both).
            let source = whole.split("#[cfg(test)]").next().unwrap_or_default();
            read += 1;
            let sets_a_program = [
                ".run_when_done",
                ".on_complete",
                ".proxy.script",
                ".queue_schedules",
            ]
            .iter()
            .any(|field| source.contains(field));
            assert!(
                !(sets_a_program && source.contains(direct)),
                "{} saves around the guard on programs dm runs",
                path.display()
            );
        }
        assert!(
            read > 10,
            "the window's sources were not found in {}",
            src.display()
        );
    }
}
