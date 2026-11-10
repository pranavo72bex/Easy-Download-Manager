// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's window in GPUI: the Phase B shell (docs/gaps.md G6). It starts dm as
//! the egui window does (`dm_desktop_base::boot`: one engine, one queue),
//! reads the queue once each time it is woken, and draws its rows; each check
//! of docs/migration/GPUI_MIGRATION_PLAN.md Phase B is made on it.
//!
//! `--accessibility` prints what VoiceOver is given of each window and
//! quits; `--churn` first opens and closes a dialog window 50 times.

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
mod appkit;
mod assets;
mod bars;
mod cliplinks;
mod clipwatch;
mod complete;
mod details;
mod drops;
mod droptarget;
mod fit;
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
mod scheduler;
mod screen;
mod series;
mod services;
mod sidebar;
mod statusbar;
mod table;
mod theme;
mod tick;
mod titlebar;
mod toolbar;
mod tray;
mod welcome;

use std::sync::Arc;
use std::time::Duration;

use dm_desktop_base::boot::{self, Wake};
use gpui::{
    div, point, prelude::*, px, size, App, Bounds, Context, Role, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowKind, WindowOptions,
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
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(dialog)),
            kind: WindowKind::Dialog,
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("Delete");
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

    // Woken from any thread; read on the window's own, in `list::Downloads`.
    let (wakes, woken) = async_channel::unbounded::<Wake>();
    let wake = {
        let wakes = wakes.clone();
        Arc::new(move |wake: Wake| {
            let _ = wakes.try_send(wake);
        })
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

    // The control port, where asked for (`--control`, DM_CONTROL, or the
    // person's setting), as the egui window opens it.
    let requests = control.and_then(|port| match port::open(port, wakes) {
        Ok(requests) => Some(requests),
        Err(e) => {
            backend.note(format!("the control protocol could not start: {e}"));
            None
        }
    });

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
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(640.0), px(400.0))),
                // A login launch goes into the tray, with no window yet.
                show: !hidden,
                // A unified title bar, as macOS programs have: the toolbar is
                // drawn in it, beside the system's own traffic lights.
                titlebar: Some(TitlebarOptions {
                    title: Some("dm".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(
                        px(titlebar::TRAFFIC_LIGHTS_AT.0),
                        px(titlebar::TRAFFIC_LIGHTS_AT.1),
                    )),
                }),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("dm");
                cx.new(|cx| {
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
                })
            },
        );
        let main = match opened {
            Ok(main) => main,
            Err(e) => {
                eprintln!("dm-gpui: the window could not be opened: {e}");
                cx.quit();
                return;
            }
        };
        // dm's menus in the system's menu bar, and every command's keys.
        menus::bind_keys(cx);
        let queues = main
            .update(cx, |view, _, _| view.backend.queues())
            .unwrap_or_default();
        menus::set(cx, &queues);
        // Quit is heard from any window in front (a dialog's too), as in
        // every macOS program: the main window's view ends dm.
        if let Ok(view) = main.update(cx, |_, _, cx| cx.entity().downgrade()) {
            cx.on_action(move |_: &menus::Quit, cx| match view.upgrade() {
                Some(view) => view.update(cx, |view, cx| view.ask_quit(cx)),
                None => cx.quit(),
            });
        }
        // The commands themselves are the window's view's (`Downloads`'
        // root element handles `menus::Run`), reached through the window's
        // own dispatch, as a key or the menu bar sends them.
        let _ = main;
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
