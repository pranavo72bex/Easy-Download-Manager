// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What `Downloads::new` does once the view is made, in its order: the settings it starts
//! with, what it listens to, and the task that reads the queue each time dm is woken.

use super::*;

/// The view `this` as the settings say it was left, and the window it is in made ready: its
/// tray, its close button, its links dropped, and the queue read.
pub(super) fn first_settings(
    this: &mut Downloads,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) {
    // The sort the person left the list in.
    let settings = this.backend.settings();
    this.list.sort = dm_desktop_base::columns::kept_sort(&settings);
    // And the side panel and toolbar as they were left (gaps row U29).
    this.layout.show_sidebar = settings.show_categories;
    this.layout.show_toolbar = settings.show_toolbar;
    this.layout.show_details = settings.show_details;
    // The taskbar button's jump list (Windows): Add URL and the folder.
    dm_desktop_base::jumplist::publish(
        dm_desktop_base::i18n::t("Add URL"),
        dm_desktop_base::i18n::t("Open downloads folder"),
        &settings.folders.download_dir,
    );
    // The clipboard watched, and the look asked for, as Options says.
    this.watch_clipboard(settings.general.watch_clipboard);
    crate::window::theme::follow(&settings.general.theme, cx);
    window.focus(&this.focus, cx);
    // The tray, and the close button that keeps dm in it.
    this.start_tray(window, cx);
    // A new install's first window, once.
    this.welcome_if_new(cx);
    // A link dragged from a browser is taken, as files are.
    crate::platform::drops::take_links(window);
    let me = cx.entity().downgrade();
    window.on_window_should_close(cx, move |window, cx| {
        me.update(cx, |view, cx| view.close_asked(window, cx))
            .unwrap_or(true)
    });
    crate::window::screen::Screens::register(
        window.window_handle(),
        this.screen.clone(),
        false,
        cx,
    );
    // A finished row's file is looked for off this thread from now on
    // (`look_for_files`), not asked of the disk as it is drawn.
    dm_desktop_base::rows::set_looked(Vec::new());
    this.read();
}

/// What the view `this` listens to in its window: the search box, the window in front, put
/// away or moved, the application quitting, and the system's colours. `hidden` for a login
/// launch into the tray.
pub(super) fn listen(
    this: &Downloads,
    hidden: bool,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) {
    // One task on the window's thread: each wake is one read, a burst of
    // queue changes one read a quarter of a second later.
    // Redrawn in the system's colours when the person switches between
    // light and dark.
    // The list narrowed as one types in the top bar's search box;
    // Return there chooses the first row it leaves.
    cx.subscribe_in(
        &this.list.filter,
        window,
        |this: &mut Downloads, field, event, window, cx| match event {
            gpui_component::input::InputEvent::Change => {
                this.list.filter_text = field.read(cx).value().to_string();
                this.reshow();
                cx.notify();
            }
            gpui_component::input::InputEvent::PressEnter { .. } => {
                this.find_step(true);
                window.focus(&this.focus, cx);
                cx.notify();
            }
            _ => {}
        },
    )
    .detach();
    // A question waiting for the main window is asked once it is in
    // front again.
    cx.observe_window_activation(window, |this, window, cx| {
        if window.is_window_active() {
            // The system's less-motion setting, changed while dm was behind.
            crate::window::follow_motion(cx);
            this.show_sheets(cx);
            // Back from a file manager, a file deleted there is said.
            this.known.files_looked = None;
            this.look_for_files(cx);
        }
    })
    .detach();
    // A login launch into the tray: GPUI's X11 windows are shown however
    // they were asked to open, so it is put away there once its first
    // frame is drawn (GPUI has mapped it on its own connection by then).
    if hidden && cfg!(all(unix, not(target_os = "macos"))) {
        window.on_next_frame(|window, _| crate::platform::syswindow::hide(window));
    }
    // A login launch waits in the tray; where nothing is showing a tray
    // a while later (a panel that never came), its window opens rather
    // than dm running unseen.
    if hidden {
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(HIDDEN_WAIT).await;
            let _ = this.update(cx, |this, cx| {
                if this.system.tray.is_none() || !dm_desktop_base::tray::host_showing() {
                    this.show_main(cx);
                }
            });
        })
        .detach();
    }
    // Its frame as it moves and is resized, kept as dm quits.
    cx.observe_window_bounds(window, |this, window, cx| {
        this.windows.frame = window.window_bounds();
        this.windows.display = window.display(cx).map(|d| u64::from(d.id()));
    })
    .detach();
    // GPUI ending dm without Exit: on Windows, the session ending (a
    // logoff, a shutdown or restart) quits GPUI from its own window, which
    // ends the process as soon as this returns. The downloads are stopped
    // and awaited here first, as Exit stops them (gaps row 186).
    cx.on_app_quit(|this, _| {
        if !this.questions.quit_asked {
            this.wind_down();
            this.questions.quit_asked = true;
        }
        async {}
    })
    .detach();
    cx.observe_window_appearance(window, |_, window, cx| {
        // The text fields and menus (gpui-component's) follow too.
        gpui_component::Theme::sync_system_appearance(Some(window), cx);
        crate::window::follow_motion(cx);
        cx.notify();
    })
    .detach();
}

/// The task on the window's thread that reads the queue each time `woken` says, and on the
/// view's clock.
pub(super) fn read_on_wakes(
    woken: dm_desktop_base::wakes::Woken,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) {
    cx.spawn_in(window, async move |this, cx| {
        // When the window last looked: its clock runs from there. The first look is at once,
        // so the tick's work due at opening (finished downloads older than Options' days
        // forgotten, B70) is not left for the idle clock's first look, 30 s on.
        let mut looked = Instant::now()
            .checked_sub(super::IDLE)
            .unwrap_or_else(Instant::now);
        loop {
            // Nothing tells the window of a running download's progress
            // or of a clock (the schedules, When done's countdown): it
            // looks of its own accord (`clock_every`).
            let every = this
                .read_with(cx, |this, _| this.clock_every())
                .unwrap_or(IDLE);
            let timer = cx
                .background_executor()
                .timer((looked + every).saturating_duration_since(Instant::now()));
            let wake = match woke(&woken, timer).await {
                Woke::By(wake) => wake,
                Woke::Clock => Wake::Now,
                Woke::Gone => return,
            };
            // A change to the queue is read a quarter of a second later,
            // the burst within it as one; but something to act on now (a
            // browser's hand-off, another dm's order) ends that wait:
            // behind a running download's
            // changes, a hand-off waited the quarter out before its
            // download began.
            if wake == Wake::Soon {
                // No later than the clock's own look, which reads it.
                let until = (Instant::now() + SOON).min(looked + every);
                loop {
                    let timer = cx
                        .background_executor()
                        .timer(until.saturating_duration_since(Instant::now()));
                    match woke(&woken, timer).await {
                        Woke::By(Wake::Soon) => {}
                        Woke::By(Wake::Now) | Woke::Clock | Woke::Gone => break,
                    }
                }
            }
            while woken.try_recv().is_ok() {}
            // Asked to end (a logout, `kill`, an uninstall): as Exit does,
            // the queue written down first.
            if dm_desktop_base::signals::SIGNALLED.load(Ordering::SeqCst) {
                let _ = in_own_window(&this, cx, |this, _, cx| this.quit(cx));
                return;
            }
            let done = in_own_window(&this, cx, |this, window, cx| this.wake(window, cx));
            if done.is_none() {
                return;
            }
            looked = Instant::now();
        }
    })
    .detach();
}
