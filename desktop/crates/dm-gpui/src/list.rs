// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The download list as the shell draws it: the application read once each
//! time the window is woken (a queue change, coalesced over a quarter of a
//! second as the egui window does; an order or a signal at once; every
//! quarter of a second while a download runs, and once a second for the
//! schedules' and When done's clocks), and only the rows on screen laid out
//! (`uniform_list`), so a list of 10,000 costs what the screenful does. A
//! script's reading of the list is answered on the control port's own
//! thread (`port.rs`), the window neither woken nor drawn for it.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_core::{Entry, State};
use dm_desktop_base::boot::Wake;
use dm_shell::app::backend::Backend;
use dm_shell::app::session::Session;
use gpui::{
    div, prelude::*, Context, FocusHandle, Role, SharedString, UniformListScrollHandle, Window,
};

use crate::screen::Screen;
use crate::theme::Theme;
use dm_desktop_base::columns::Column;
use dm_shell::app::places::{Place, Places};

/// Runs `update` on the view in the window the loop was started in, the
/// main window: not `WeakEntity::update_in`, which runs in whichever window
/// last drew the view (Download details reads it as it draws, and once that
/// dialog closed the view had no window, and the loop ended). `None` once
/// the window or the view is gone.
fn in_own_window<R>(
    this: &gpui::WeakEntity<Downloads>,
    cx: &mut gpui::AsyncWindowContext,
    update: impl FnOnce(&mut Downloads, &mut Window, &mut Context<Downloads>) -> R,
) -> Option<R> {
    cx.update(|window, cx| this.update(cx, |this, cx| update(this, window, cx)).ok())
        .ok()
        .flatten()
}

/// What ended one of the window's waits.
enum Woke {
    /// Something woke it.
    By(Wake),
    /// Its timer ran out first.
    Clock,
    /// Nothing can wake it any more: dm is ending.
    Gone,
}

/// The next wake, or the timer's end, whichever comes first.
async fn woke(woken: &crate::wakes::Woken, timer: gpui::Task<()>) -> Woke {
    futures_lite::future::or(
        async { woken.recv().await.map_or(Woke::Gone, Woke::By) },
        async {
            timer.await;
            Woke::Clock
        },
    )
    .await
}

/// How long the window sleeps with nothing waking it (`clock_every`), with
/// a download `running` or a clock `ticking` (When done's countdown, the
/// clipboard watched).
fn clock(running: bool, ticking: bool) -> Duration {
    if running {
        RUNNING
    } else if ticking {
        Duration::from_secs(1)
    } else {
        IDLE
    }
}

/// When done's countdown as it stands: what dm will do and in how many
/// seconds; `None` once it is over or cancelled.
fn countdown_question(view: &Downloads) -> Option<crate::question::Asked> {
    use dm_desktop_base::i18n::{t, tf};
    let (action, at) = view.session.countdown?;
    let left = at
        .saturating_duration_since(std::time::Instant::now())
        .as_secs();
    Some(crate::question::Asked {
        title: t("Downloads are done").into(),
        lines: vec![tf("dm will {} in {} seconds.", &[&t(action.verb()), &left]).into()],
        named: None,
        detail: None,
        toggle: None,
        ticked: false,
        // Cancel, the default, rightmost: Return and Escape both stop it.
        buttons: vec![t("Cancel").into(), t("Now").into()],
        default: 0,
        escape: 0,
    })
}

/// Where Exit stands in "Exit dm?"'s buttons.
const EXIT: usize = 1;

/// "Exit dm?" as the list stands (`asks::exit`). Keep downloading (Stay,
/// once nothing runs) stands rightmost and Exit beside it, each where it is
/// whatever the words say: a download finishing while it is open changes
/// the words and moves the default to Exit, and moves no button.
fn exit_question(view: &Downloads) -> crate::question::Asked {
    let (busy, waiting) = view.running_and_waiting();
    let ask = dm_desktop_base::asks::exit(busy, waiting);
    crate::question::Asked {
        title: ask.title.into(),
        lines: vec![ask.sentence.into()],
        named: None,
        detail: None,
        toggle: None,
        ticked: false,
        buttons: vec![ask.keep.into(), ask.exit.into()],
        default: if ask.finished { EXIT } else { 0 },
        escape: 0,
    }
}

/// The rows' height, in points.
pub(crate) const ROW_HEIGHT: f32 = 24.0;

/// How often a window with nothing moving wakes of its own accord.
const IDLE: Duration = Duration::from_secs(30);

/// How often the window reads and draws the list while a download runs.
const RUNNING: Duration = Duration::from_millis(250);

/// How long a login launch waits for something to show its tray before its
/// window opens instead.
const HIDDEN_WAIT: Duration = Duration::from_secs(10);

mod commands;
mod keys;
mod later;
mod questions;
mod refresh;
use refresh::clicked;
use refresh::{needs_drawing, placed_alike, NAMES_EVERY};

/// The main window's view: the application, and the rows it last read.
pub struct Downloads {
    pub(crate) backend: Backend,
    pub(crate) session: Session,
    pub(crate) rows: Vec<Arc<Entry>>,
    /// The control port's requests, answered on this thread.
    requests: Option<std::sync::mpsc::Receiver<dm_desktop_base::control::Request>>,
    /// The port was opened only for AI assistants.
    assistants_only: bool,
    /// Where the list is scrolled, and where it is on screen.
    pub(crate) scroll: UniformListScrollHandle,
    /// The words on screen, where the last frame laid them out.
    pub(crate) screen: Screen,
    /// The window's focus: where keys and the menu bar's commands arrive.
    pub(crate) focus: FocusHandle,
    /// The place chosen in the side panel.
    pub(crate) place: Place,
    /// Each row's place and the places' counts (`dm_shell::app::places`).
    pub(crate) places: Places,
    /// The rows the list shows, in the place chosen: their indices in `rows`.
    pub(crate) shown: Vec<usize>,
    /// The column the list is sorted by, and whether up; none: as added.
    pub(crate) sort: Option<(Column, bool)>,
    /// The rows chosen, by id: they stay chosen as the list is read again
    /// and sorted, as a person expects of a selection.
    pub(crate) selected: std::collections::BTreeSet<dm_core::Id>,
    /// Where a ⇧-click's range starts: the row last clicked alone.
    anchor: Option<dm_core::Id>,
    /// A column's width as its heading's edge is dragged, and when the drag's width was last
    /// kept (`table.rs`).
    pub(crate) resizing: Option<(Column, f32)>,
    pub(crate) resize_kept: Option<std::time::Instant>,
    /// The side panel's width as its edge is dragged (`sidebar.rs`), kept as `resize_kept` says.
    pub(crate) panel_resizing: Option<f32>,
    /// The keyboard's place: the row last clicked or moved to, which the
    /// arrows, Home, End and the page keys move from (`keys.rs`).
    cursor: Option<dm_core::Id>,
    /// The row menu, open where it was asked for.
    menu: Option<gpui::Point<gpui::Pixels>>,
    /// The keyboard's place in the open row menu, by entry (`rowmenu::entries`).
    pub(crate) menu_at: Option<usize>,
    /// A place's right-click menu, open where it was asked for (`placemenu`).
    pub(crate) place_menu: Option<(Place, gpui::Point<gpui::Pixels>)>,
    /// What was typed over the list to find a row by its name, and when last (`keys.rs`).
    pub(crate) typed: (String, Option<std::time::Instant>),
    /// A sentence for the status bar, and when it was said: it stands for
    /// a while, as the egui window's does.
    pub(crate) said: Option<(String, std::time::Instant)>,
    /// Whether `said` says a queue waits for the schedule, so the status bar does not say it
    /// twice (`statusbar::parts`).
    pub(crate) said_schedule_wait: bool,
    /// What finished or failed last, said aloud to a screen reader from the status bar and not
    /// drawn (`a11y::heard`, gaps row U23), and when.
    pub(crate) heard: Option<(String, std::time::Instant)>,
    /// Finished rows Remove took off, kept a moment for Undo.
    pub(crate) undo: dm_shell::app::backend::Undo,
    /// The questions the rows raise as they run (B39 to B41), asked one at
    /// a time in the order they came.
    asks: dm_shell::app::questions::Questions,
    /// Names a person chose for rows still downloading, given as each
    /// finishes.
    renames: dm_shell::app::names::PendingRenames,
    /// A question is being asked: the next waits for its answer.
    asking: bool,
    /// The main window: a sheet is opened on it alone.
    main: gpui::AnyWindowHandle,
    /// Questions waiting to be asked as sheets, one at a time.
    sheets: std::collections::VecDeque<(
        crate::question::Asked,
        Option<crate::question::Reword>,
        Option<crate::question::Live>,
        crate::question::Answer,
    )>,
    /// A sheet is open on the main window.
    sheet_open: bool,
    /// The open question's window: on Linux it holds the main window as
    /// Options does (`holder`), since GPUI's X11 and Wayland windows hold
    /// their parent only for a `Dialog`, which a question is not there.
    question_window: Option<gpui::AnyWindowHandle>,
    /// "Exit dm?" is being asked: Exit again does not ask twice.
    exit_asked: bool,
    /// What the reads since the last tick detected (`tick.rs`).
    pub(crate) detected: Vec<dm_shell::app::detect::Detected>,
    /// Problems to show above the list, each until dismissed.
    pub(crate) problems: Vec<String>,
    /// Finished rows' sizes, read once from their files.
    saved: dm_shell::app::snapshot::SavedSizes,
    /// A browser's hand-off is being asked about (Download details or
    /// Download links): the next waits for it.
    pub(crate) handed_open: bool,
    /// Hand-offs cancelled, by site (`handoff::Cancels`): the second asks
    /// whether to leave that site's downloads to the browser.
    cancelled_hand_offs: dm_shell::app::handoff::Cancels,
    /// When done's countdown is being shown.
    countdown_shown: bool,
    /// The top bar's search box: the words that narrow the list.
    pub(crate) filter: gpui::Entity<gpui_component::input::InputState>,
    /// Its words, as last typed.
    pub(crate) filter_text: String,
    /// The speed limit before the limiter was turned off, for turning it on.
    last_limit: u64,
    /// The Recheck queue's counts, once a check of it is done.
    rechecked: std::sync::Arc<std::sync::Mutex<Option<(usize, usize)>>>,
    /// What the menu bar was last built with (its queues and ticks); none
    /// builds it again.
    pub(crate) menu_ticks: Option<crate::menus::Ticks>,
    /// The main window's frame as it last stood, kept as dm quits for the
    /// next start (`frame.rs`).
    frame: gpui::WindowBounds,
    /// The display it stood on, by its id.
    display: Option<u64>,
    /// What saved searches found, as each run ends, by the search's name.
    #[allow(clippy::type_complexity)]
    search_results:
        std::sync::Arc<std::sync::Mutex<Vec<(String, Result<dm_shell::lists::Crawled, String>)>>>,
    /// When the saved searches were last looked at.
    pub(crate) searches_checked: Option<std::time::Instant>,
    /// Each progress window open, by its download.
    progress_windows: std::collections::BTreeMap<dm_core::Id, gpui::AnyWindowHandle>,
    /// Progress windows a person closed: not opened again by themselves.
    progress_closed: std::collections::HashSet<dm_core::Id>,
    /// The downloads "Download complete" lists.
    pub(crate) complete: std::collections::BTreeSet<dm_core::Id>,
    /// "Download complete", while it is open.
    complete_window: Option<gpui::AnyWindowHandle>,
    /// Each download's File properties window, while open.
    properties_windows: std::collections::BTreeMap<dm_core::Id, gpui::AnyWindowHandle>,
    /// When a control client last acted: a file is not run, nor the update
    /// installed, by a script's click a moment ago (gaps row 159 f).
    pub(crate) scripted: dm_shell::app::control_api::ScriptGuard,
    /// What the update check has found.
    pub(crate) checked: crate::about::Checked,
    /// dm's update being downloaded, offered to install once verified.
    pub(crate) update_row: Option<dm_core::Id>,
    /// The weekly check asked: About opens if it finds a newer dm.
    pub(crate) weekly_pending: bool,
    /// About dm, while open.
    pub(crate) about_window: Option<gpui::AnyWindowHandle>,
    /// Tip of the day, while open.
    pub(crate) tip_window: Option<gpui::AnyWindowHandle>,
    /// Options, while open.
    pub(crate) options_window: Option<gpui::AnyWindowHandle>,
    /// The clipboard, watched where Options says (`clipwatch.rs`).
    pub(crate) clipboard: dm_shell::app::links::Clipboard,
    /// The status item in the menu bar (`tray.rs`); `None` where the system
    /// would not make one, and closing the window then ends dm.
    pub(crate) tray: Option<crate::tray::Tray>,
    /// The window is put away in the tray.
    pub(crate) hidden: bool,
    /// The floating drop target, while shown (`droptarget.rs`).
    pub(crate) drop_window: Option<gpui::AnyWindowHandle>,
    /// What the bars above the list remember (`bars.rs`).
    pub(crate) bars: crate::bars::Bars,
    /// The name each row is shown by: the one its file will get, a waiting
    /// second copy's "name (1)", worked out at each read.
    pub(crate) shown_names: std::collections::HashMap<dm_core::Id, String>,
    /// The settings a frame is drawn with, read once as it begins (and at
    /// each read): its parts read them a dozen times a frame, each a copy
    /// of the whole under its lock (gaps row 180).
    pub(crate) drawn: std::rc::Rc<dm_shell::Settings>,
    /// `drawn` was just read, by a read for the frame about to be drawn.
    drawn_fresh: bool,
    /// The queues, as the last read found them: Start Queue's and Stop
    /// Queue's arrows list them, and asking the engine as each frame was
    /// drawn held the window up behind the queue's lock.
    pub(crate) queue_list: Vec<String>,
    /// When the finished rows' files were last looked for, and whether a
    /// look is under way (`look_for_files`).
    files_looked: Option<Instant>,
    looking_for_files: bool,
    /// Work handed off the window's thread and not answered yet, and the
    /// control port's `command.run` answers held until it is
    /// (`list/later.rs`).
    errands: later::Pending,
    /// The clipboard is being read off the window's thread (`clipwatch`).
    pub(crate) clipboard_reading: bool,
    /// The rows on screen, as the list last laid them out.
    pub(crate) visible: std::ops::Range<usize>,
    /// Where each row shown is in the list, by id (`reshow`).
    positions: std::collections::HashMap<dm_core::Id, usize>,
    /// When the names and places were last worked out (`read`).
    named_at: Option<Instant>,
    /// How many earlier rows of the same name bound for the same folder
    /// take a free name before each row, as the last whole read counted
    /// them (`names::earlier_of_the_same_name`).
    earlier: std::collections::HashMap<dm_core::Id, usize>,
    /// The next read works everything out again (the settings changed).
    pub(crate) read_all: bool,
    /// When each row finished, in words, kept by the read (`keep_whens`).
    pub(crate) whens: dm_desktop_base::format::Whens,
    /// Set when a second launch, the browser or the Dock asks for the window.
    show_asked: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// What an opener said went wrong, from its thread.
    opener_said: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// What Scheduler › Hang up modem when done came to, from its thread.
    pub(crate) hung_up: std::sync::Arc<std::sync::Mutex<Option<dm_shell::dialup::HungUp>>>,
    /// View › Categories: the side panel shown.
    pub(crate) show_sidebar: bool,
    /// View › Toolbar: the toolbar shown.
    pub(crate) show_toolbar: bool,
    /// The progress on the taskbar button (Windows; nothing elsewhere).
    taskbar: dm_desktop_base::taskbar::Taskbar,
    /// Windows' High Contrast palette as last drawn (`tick.rs`): GPUI tells
    /// the window of light and dark only.
    pub(crate) contrast: Option<dm_desktop_base::contrast::Palette>,
}

impl Downloads {
    /// The view, reading the queue now and each time `woken` says.
    // What the launch hands the window, each needed from its first frame.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: Backend,
        woken: crate::wakes::Woken,
        requests: Option<std::sync::mpsc::Receiver<dm_desktop_base::control::Request>>,
        assistants_only: bool,
        show_asked: std::sync::Arc<std::sync::atomic::AtomicBool>,
        hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Downloads {
        let mut this = Downloads {
            backend,
            session: Session::default(),
            rows: Vec::new(),
            requests,
            assistants_only,
            scroll: UniformListScrollHandle::new(),
            screen: Screen::default(),
            focus: cx.focus_handle(),
            place: Place::All,
            places: Places::default(),
            shown: Vec::new(),
            sort: None,
            selected: Default::default(),
            anchor: None,
            cursor: None,
            menu_at: None,
            place_menu: None,
            typed: (String::new(), None),
            resizing: None,
            resize_kept: None,
            panel_resizing: None,
            menu: None,
            said: None,
            said_schedule_wait: false,
            heard: None,
            undo: Default::default(),
            asks: Default::default(),
            renames: Default::default(),
            asking: false,
            main: window.window_handle(),
            sheets: Default::default(),
            sheet_open: false,
            question_window: None,
            exit_asked: false,
            detected: Vec::new(),
            problems: Vec::new(),
            saved: Default::default(),
            handed_open: false,
            cancelled_hand_offs: Default::default(),
            countdown_shown: false,
            filter: cx.new(|cx| {
                gpui_component::input::InputState::new(window, cx)
                    .placeholder(dm_desktop_base::i18n::t("Name, address or note"))
            }),
            filter_text: String::new(),
            last_limit: 0,
            rechecked: Default::default(),
            menu_ticks: None,
            frame: window.window_bounds(),
            display: window.display(cx).map(|d| u64::from(d.id())),
            search_results: Default::default(),
            searches_checked: None,
            progress_windows: Default::default(),
            progress_closed: Default::default(),
            complete: Default::default(),
            complete_window: None,
            properties_windows: Default::default(),
            scripted: Default::default(),
            checked: std::sync::Arc::new(std::sync::Mutex::new(
                dm_shell::app::update::Update::NotAsked,
            )),
            update_row: None,
            weekly_pending: false,
            about_window: None,
            tip_window: None,
            options_window: None,
            clipboard: Default::default(),
            tray: None,
            hidden,
            drop_window: None,
            bars: Default::default(),
            shown_names: Default::default(),
            drawn: Default::default(),
            drawn_fresh: false,
            queue_list: Vec::new(),
            files_looked: None,
            looking_for_files: false,
            errands: Default::default(),
            clipboard_reading: false,
            visible: 0..0,
            positions: Default::default(),
            named_at: None,
            earlier: Default::default(),
            // The first read works it all out: an empty list's queues too.
            read_all: true,
            whens: Default::default(),
            show_asked,
            opener_said: Default::default(),
            hung_up: Default::default(),
            show_sidebar: true,
            show_toolbar: true,
            taskbar: Default::default(),
            contrast: dm_desktop_base::contrast::system(),
        };
        // The sort the person left the list in.
        let settings = this.backend.settings();
        this.sort = dm_desktop_base::columns::kept_sort(&settings);
        // The taskbar button's jump list (Windows): Add URL and the folder.
        dm_desktop_base::jumplist::publish(
            dm_desktop_base::i18n::t("Add URL"),
            dm_desktop_base::i18n::t("Open downloads folder"),
            &settings.download_dir,
        );
        // The clipboard watched, and the look asked for, as Options says.
        this.watch_clipboard(settings.watch_clipboard);
        crate::theme::follow(&settings.theme, cx);
        window.focus(&this.focus, cx);
        // The tray, and the close button that keeps dm in it.
        this.start_tray(window, cx);
        // A new install's first window, once.
        this.welcome_if_new(cx);
        // A link dragged from a browser is taken, as files are.
        crate::drops::take_links(window);
        let me = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            me.update(cx, |view, cx| view.close_asked(window, cx))
                .unwrap_or(true)
        });
        crate::screen::Screens::register(window.window_handle(), this.screen.clone(), false, cx);
        // A finished row's file is looked for off this thread from now on
        // (`look_for_files`), not asked of the disk as it is drawn.
        dm_desktop_base::rows::set_looked(Vec::new());
        this.read();
        // One task on the window's thread: each wake is one read, a burst of
        // queue changes one read a quarter of a second later.
        // Redrawn in the system's colours when the person switches between
        // light and dark.
        // The list narrowed as one types in the top bar's search box;
        // Return there chooses the first row it leaves.
        cx.subscribe_in(
            &this.filter,
            window,
            |this: &mut Downloads, field, event, window, cx| match event {
                gpui_component::input::InputEvent::Change => {
                    this.filter_text = field.read(cx).value().to_string();
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
                this.show_sheets(cx);
                // Back from a file manager, a file deleted there is said.
                this.files_looked = None;
                this.look_for_files(cx);
            }
        })
        .detach();
        // A login launch into the tray: GPUI's X11 windows are shown however
        // they were asked to open, so it is put away there once its first
        // frame is drawn (GPUI has mapped it on its own connection by then).
        if hidden && cfg!(all(unix, not(target_os = "macos"))) {
            window.on_next_frame(|window, _| crate::syswindow::hide(window));
        }
        // A login launch waits in the tray; where nothing is showing a tray
        // a while later (a panel that never came), its window opens rather
        // than dm running unseen, as the egui window's does.
        if hidden {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(HIDDEN_WAIT).await;
                let _ = this.update(cx, |this, cx| {
                    if this.tray.is_none() || !dm_desktop_base::tray::host_showing() {
                        this.show_main(cx);
                    }
                });
            })
            .detach();
        }
        // Its frame as it moves and is resized, kept as dm quits.
        cx.observe_window_bounds(window, |this, window, cx| {
            this.frame = window.window_bounds();
            this.display = window.display(cx).map(|d| u64::from(d.id()));
        })
        .detach();
        cx.observe_window_appearance(window, |_, window, cx| {
            // The text fields and menus (gpui-component's) follow too.
            gpui_component::Theme::sync_system_appearance(Some(window), cx);
            cx.notify();
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            // When the window last looked: its clock runs from there.
            let mut looked = Instant::now();
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
                // browser's hand-off, another dm's order) ends that wait, as
                // it does in the egui window: behind a running download's
                // changes, a hand-off waited the quarter out before its
                // download began.
                if wake == Wake::Soon {
                    // No later than the clock's own look, which reads it.
                    let until = (Instant::now() + Duration::from_millis(250)).min(looked + every);
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
                if in_own_window(&this, cx, |this, window, cx| {
                    let beside = this.shown_beside();
                    let changed = this.read();
                    this.taskbar_follow(window);
                    this.tick(cx);
                    this.watch_asks(cx);
                    let answered = this.answer_requests(window, cx);
                    // Drawn again only where something shown may have
                    // changed: a wake that changed nothing drew the whole
                    // window for nothing (gaps row 181).
                    if needs_drawing(
                        changed,
                        answered,
                        this.moves_with_time(),
                        beside != this.shown_beside(),
                    ) {
                        cx.notify();
                    }
                    this.look_for_files(cx);
                })
                .is_none()
                {
                    return;
                }
                looked = Instant::now();
            }
        })
        .detach();
        this
    }

    /// How far the downloads are, on the taskbar button, so a minimized
    /// window still says it, as the egui window's does: green while they
    /// run, yellow paused, red a minute after one failed (Windows); a bar
    /// and a count on the Dock (macOS) and the launcher (Linux).
    fn taskbar_follow(&mut self, window: &Window) {
        use dm_desktop_base::taskbar;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mode = taskbar::mode_of(&self.rows, now);
        #[cfg(windows)]
        if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) {
            self.taskbar.show(handle.as_raw(), mode);
        }
        #[cfg(not(windows))]
        let _ = window;
        self.taskbar
            .show_outside(taskbar::outside_of(mode, taskbar::running_of(&self.rows)));
    }

    /// How long the window sleeps when nothing wakes it: a quarter of a
    /// second while a download runs, whose figures nothing else tells it of
    /// (the engine reports them that often, and the egui window drew them so;
    /// read once a second, they moved in jumps); a second for When done's
    /// countdown and the clipboard; else 30 seconds.
    fn clock_every(&self) -> Duration {
        clock(
            self.rows.iter().any(|r| r.state == State::Running),
            self.session.countdown.is_some() || self.clipboard.on,
        )
    }

    /// Answers what the control port asked, in order: who may ask as the
    /// application says, the window's methods through its `WindowProbe`,
    /// the rest through the application (`control_api`).
    fn answer_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let asked: Vec<_> = match &self.requests {
            Some(requests) => requests.try_iter().collect(),
            None => return false,
        };
        if asked.is_empty() {
            return false;
        }
        let allow_mcp = self.backend.settings().allow_mcp;
        for request in asked {
            let answer = match dm_shell::app::control_api::refused(
                request.via_mcp,
                self.assistants_only,
                allow_mcp,
            ) {
                Some(why) => Err(why),
                None => crate::probe::answer(self, &request.method, &request.params, window, cx),
            };
            self.reply(&request.method, (request.reply, answer));
        }
        true
    }

    /// Carries out a command from the menu bar or a key. What the shell
    /// cannot do yet is greyed in the menus (`menus::carried_out`), and a
    /// key for it does nothing.
    pub fn run(&mut self, command: dm_shell::app::commands::Command, cx: &mut Context<Self>) {
        self.carry_out(command, cx);
        // The menus' ticks follow what it changed: When done, the speed
        // limiter, the View menu's.
        self.refresh_menus(cx);
    }

    fn carry_out(&mut self, command: dm_shell::app::commands::Command, cx: &mut Context<Self>) {
        use dm_shell::app::commands::Command;
        if !self.enabled(command) {
            return;
        }
        // Held by Options (B42): Quit and the dialogs' own still go. An open
        // question (Linux, `holder`) holds the window's own clicks and keys
        // only, not what the tray, `--when-done` or the control port asks.
        let options_open = self
            .options_window
            .is_some_and(|w| cx.windows().contains(&w));
        if command != Command::Quit
            && command != Command::Options
            && options_open
            && self.bring_holder(cx)
        {
            return;
        }
        if command == Command::Quit {
            self.ask_quit(cx);
            return;
        }
        let chosen: Vec<Entry> = self.chosen().into_iter().cloned().collect();
        match command {
            // Each address on a line of its own, without a sign-in that may
            // be in it: what is pasted elsewhere is the link.
            Command::CopyLink => {
                let links: Vec<String> = chosen
                    .iter()
                    .map(|r| dm_desktop_base::format::copyable_url(&r.url))
                    .collect();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(links.join("\n")));
                self.say(dm_desktop_base::i18n::tn(
                    "Copied {} link.||Copied {} links.",
                    links.len() as u64,
                    &[&links.len()],
                ));
                cx.notify();
                return;
            }
            Command::Remove | Command::DeleteFile => {
                self.remove(&chosen, command == Command::DeleteFile, cx);
                cx.notify();
                return;
            }
            Command::OpenFile
            | Command::OpenWith
            | Command::Preview
            | Command::OpenFolder
            | Command::ExtractHere
            | Command::ExtractTo => {
                self.act_on_file(command, &chosen, cx);
                return;
            }
            Command::SelectAll => {
                self.select_all(cx);
                return;
            }
            Command::Import => {
                self.import(cx);
                return;
            }
            Command::Export => {
                self.export(cx);
                return;
            }
            Command::Help => {
                self.help();
                cx.notify();
                return;
            }
            Command::ToggleCategories => {
                self.show_sidebar = !self.show_sidebar;
                cx.notify();
                return;
            }
            Command::ToggleToolbar => {
                self.show_toolbar = !self.show_toolbar;
                cx.notify();
                return;
            }
            Command::SortByName
            | Command::SortBySize
            | Command::SortByStatus
            | Command::SortByTimeLeft
            | Command::SortByRate
            | Command::SortByDate
            | Command::SortByDescription => {
                let column = match command {
                    Command::SortByName => Column::Name,
                    Command::SortBySize => Column::Size,
                    Command::SortByStatus => Column::Status,
                    Command::SortByTimeLeft => Column::TimeLeft,
                    Command::SortByRate => Column::Rate,
                    Command::SortByDate => Column::LastTry,
                    _ => Column::Description,
                };
                self.sort_by(column, cx);
                cx.notify();
                return;
            }
            Command::ShowProgress => {
                let ids: Vec<dm_core::Id> = chosen
                    .iter()
                    .filter(|r| r.state != State::Completed)
                    .map(|r| r.id)
                    .collect();
                for id in ids {
                    self.progress_closed.remove(&id);
                    self.show_progress(id, cx);
                }
                return;
            }
            Command::SiteFiles => {
                crate::grabber::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::UpdateLink => {
                crate::rowdialogs::update_link(cx.entity().downgrade(), chosen[0].id, cx);
                return;
            }
            Command::MoveToQueue => {
                let ids = chosen.iter().map(|r| r.id).collect();
                crate::rowdialogs::move_to_queue(cx.entity().downgrade(), ids, cx);
                return;
            }
            Command::MoveRename => {
                crate::rowdialogs::move_rename(cx.entity().downgrade(), chosen, cx);
                return;
            }
            Command::Properties => {
                for row in &chosen {
                    crate::properties::open(cx.entity().downgrade(), row.id, cx);
                }
                return;
            }
            Command::Scheduler => {
                crate::scheduler::open(cx.entity().downgrade(), self.current_queue(), cx);
                return;
            }
            Command::SpeedLimiterSettings => {
                crate::limiter::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::SpeedLimiterToggle
            | Command::SpeedLow
            | Command::SpeedMedium
            | Command::SpeedHigh
            | Command::SpeedUnlimited => {
                self.speed(command);
                cx.notify();
                return;
            }
            Command::StartQueue | Command::StopQueue => {
                let queue = self.current_queue();
                self.run_on_queue(command, &queue, cx);
                return;
            }
            // Downloads › When done: what dm does once the queue has run
            // dry, for this session.
            Command::WhenDoneNothing
            | Command::WhenDoneQuit
            | Command::WhenDoneSleep
            | Command::WhenDoneHibernate
            | Command::WhenDoneShutDown => {
                if let Some(chosen) = dm_shell::app::commands::when_done(command) {
                    self.session.when_done = chosen;
                }
                cx.notify();
                return;
            }
            Command::AddUrl => {
                crate::addurl::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::AddSeries => {
                crate::series::open(cx.entity().downgrade(), cx);
                return;
            }
            Command::AddFromClipboard => {
                self.add_from_clipboard(cx);
                return;
            }
            Command::Options => {
                self.open_options(cx);
                return;
            }
            // View › Drop target: kept in Options, and the card shown or put
            // away.
            Command::ToggleDropTarget => {
                let mut wanted = self.backend.settings();
                wanted.drop_target = !wanted.drop_target;
                if let Err(e) = self.apply_settings(wanted, cx) {
                    self.say(e);
                }
                self.follow_drop_target(cx);
                cx.notify();
                return;
            }
            Command::About | Command::CheckForUpdates => {
                self.open_about(command == Command::CheckForUpdates, cx);
                return;
            }
            Command::CopyDetails => {
                let settings = self.backend.settings();
                if let Some((text, count)) = dm_desktop_base::details::blocks_for(
                    &chosen,
                    settings.connections,
                    &settings.download_dir,
                ) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.say(dm_desktop_base::i18n::tn(
                        "Copied the details of {} download.||Copied the details of {} downloads.",
                        count as u64,
                        &[&count],
                    ));
                }
                cx.notify();
                return;
            }
            _ => {}
        }
        if dm_shell::app::commands::is_verb(command) {
            // Undo stands until the next command that changes the list (gaps row U14).
            self.undo.forget();
            // What Delete Completed takes off, kept for its Undo (B24).
            let kept = (command == Command::DeleteCompleted).then(|| self.backend.clearable_rows());
            // Off the window's thread: Stop on ten thousand chosen rows is
            // ten thousand changes to the queue (gaps row 183).
            let queue = self.current_queue();
            let ids: Vec<dm_core::Id> = chosen.iter().map(|r| r.id).collect();
            self.later(
                move |backend| dm_shell::app::commands::verb(command, backend, &chosen, &queue),
                move |this, ran, cx| this.verb_done(command, &ids, ran, kept, cx),
                cx,
            );
            return;
        }
        self.read();
        cx.notify();
    }

    /// What a command `commands::verb` carried out on the rows `chosen`
    /// did, said and shown, once it is done.
    fn verb_done(
        &mut self,
        command: dm_shell::app::commands::Command,
        chosen: &[dm_core::Id],
        ran: Option<dm_shell::app::commands::Ran>,
        kept: Option<Vec<(usize, Entry)>>,
        cx: &mut Context<Self>,
    ) {
        use dm_shell::app::commands::Command;
        self.read();
        if let Some(dm_shell::app::commands::Ran::Cleared(n)) = ran {
            // Undo puts them back, as it does what Remove took off (B24, gaps row U14).
            if n > 0 && self.undo.keep(kept.unwrap_or_default()) > 0 {
                self.undo_expires(cx);
            }
            self.say(dm_desktop_base::i18n::tn(
                "Deleted {} completed or cancelled download from the list.||Deleted {} completed or cancelled downloads from the list.",
                n as u64,
                &[&n],
            ));
        }
        // Resume opens each row's progress window where Options says, but
        // not one its person closed: fifteen Resumes opened it fifteen
        // times over the list. Show progress still opens it.
        if command == Command::DownloadNow && self.backend.settings().show_progress {
            for id in chosen {
                if !self.progress_closed.contains(id) {
                    self.show_progress(*id, cx);
                }
            }
        }
        // Resume on rows the schedule or the download limit holds: each of
        // the others went on, and these are asked about.
        if let Some(dm_shell::app::commands::Ran::WentOn { held }) = ran {
            self.ask_outside(held, cx);
        }
        cx.notify();
    }

    /// Adds `urls` with the options Download details chose, as the person's
    /// own; the rows are read again.
    pub fn add(
        &mut self,
        urls: Vec<String>,
        with: dm_core::AddOptions,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let later = with.later;
        let items = urls.into_iter().map(|u| (u, with.clone())).collect();
        let added = self.session.add(&self.backend, items);
        if let Some(id) = added.rows.first().map(|(id, _, _)| *id) {
            // The new row is the one chosen, as the person expects of a row
            // just added.
            self.selected.clear();
            self.selected.insert(id);
        }
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => dm_desktop_base::i18n::tf("One of the links was not added: {}", &[&e]),
                n => dm_desktop_base::i18n::tf("{} of the links were not added: {}", &[&n, &e]),
            });
        }
        self.read();
        // Its progress window opens as it starts, where that is asked for
        // (B63), for one download at a time.
        if let [(id, _, false)] = added.rows.as_slice() {
            if self.backend.settings().show_progress {
                self.show_progress(*id, cx);
            }
        }
        // Start now while the schedule or the download limit holds
        // downloads: asked, as Resume asks, whether to start it outside the
        // schedule (asked of the schedule itself: the list, just read, may
        // hold no row waiting on it yet).
        let settings = self.backend.settings();
        let shut = settings.schedule.enabled && !settings.schedule.open();
        if !later && (shut || self.backend.allowance_spent().is_some()) {
            let ids = added.rows.iter().map(|(id, _, _)| *id).collect();
            self.ask_outside(ids, cx);
        }
        cx.notify();
        added.outcome()
    }

    /// "Start outside the schedule?" for the rows `ids` the schedule or the
    /// download limit holds: Start now starts them this once; Keep waiting
    /// leaves them.
    pub fn ask_outside(&mut self, ids: Vec<dm_core::Id>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let settings = self.backend.settings();
        let mut categories = settings.categories();
        // The name the list shows: "big.zip" was asked about while the row
        // read "big (1).zip".
        let names: Vec<String> = ids
            .iter()
            .filter_map(|id| self.rows.iter().find(|r| r.id == *id))
            .map(|row| {
                dm_shell::app::names::name_it_will_get_after(
                    &self.rows,
                    row,
                    &settings.download_dir,
                    &mut categories,
                    None,
                )
            })
            .collect();
        let why = dm_desktop_base::held::held_because(&settings, self.backend.allowance_spent());
        let ask = dm_desktop_base::asks::outside(&names, why);
        let asked = crate::question::Asked {
            title: ask.title.into(),
            lines: std::iter::once(ask.sentence)
                .chain(ask.why)
                .chain(std::iter::once(ask.question.to_string()))
                .map(Into::into)
                .collect(),
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            // Keep waiting beside Start now, the default, rightmost.
            buttons: vec![ask.start.into(), ask.wait.into()],
            default: 0,
            escape: 1,
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, |view, cx| {
                for id in &ids {
                    view.backend.start_outside_schedule(*id);
                }
                view.read();
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Adds downloads with options of their own (Download links' each has
    /// its name, its description, its sign-in), as the person's own; how
    /// many went in.
    pub fn add_items(
        &mut self,
        items: Vec<(String, dm_core::AddOptions)>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let added = self.session.add(&self.backend, items);
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => dm_desktop_base::i18n::tf("One of the links was not added: {}", &[&e]),
                n => dm_desktop_base::i18n::tf("{} of the links were not added: {}", &[&n, &e]),
            });
        }
        self.read();
        cx.notify();
        added.outcome()
    }

    /// A handed-over download from `site` was cancelled: the second asks
    /// whether to leave that site's downloads to the browser (counted by the
    /// application, `handoff::Cancels`).
    pub fn hand_off_cancelled(&mut self, site: &str, cx: &mut Context<Self>) {
        let settings = self.backend.settings();
        if !self
            .cancelled_hand_offs
            .cancelled(site, &settings.catch.skip_hosts)
        {
            return;
        }
        let asked = crate::question::Asked {
            title: dm_desktop_base::i18n::t("Leave this site to the browser?").into(),
            lines: vec![dm_desktop_base::i18n::tf(
                "You cancelled two downloads from {}. Leave its downloads to the browser?",
                &[&site],
            )
            .into()],
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            buttons: vec![
                dm_desktop_base::i18n::t("Leave them to the browser").into(),
                dm_desktop_base::i18n::t("Keep handing them over").into(),
            ],
            default: 0,
            escape: 1,
        };
        let site = site.to_string();
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, |view, _| {
                let mut wanted = view.backend.settings();
                if !wanted.catch.skip_hosts.contains(&site) {
                    wanted.catch.skip_hosts.push(site);
                }
                if let Err(e) = view.backend.set_settings(wanted) {
                    view.problems.push(e);
                }
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// When done's countdown, shown once it starts: "dm will sleep in 30
    /// seconds.", Cancel (the default) and Now. It closes once the countdown
    /// is over or cancelled.
    pub(crate) fn show_countdown(&mut self, cx: &mut Context<Self>) {
        if self.session.countdown.is_none() {
            self.countdown_shown = false;
            return;
        }
        if self.countdown_shown {
            return;
        }
        self.countdown_shown = true;
        let Some(asked) = countdown_question(self) else {
            return;
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, |view, cx| {
                view.countdown_shown = false;
                match (button, view.session.countdown) {
                    // Now: the action at once.
                    (1, Some((action, _))) => {
                        view.session.countdown = Some((action, std::time::Instant::now()));
                        view.countdown_now(cx);
                    }
                    _ => view.session.countdown = None,
                }
                cx.notify();
            });
        });
        let live: crate::question::Live = Box::new(countdown_question);
        self.sheet(asked, None, Some(live), answer, cx);
    }

    /// Clears the search box: every row of the place shown again.
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| f.set_value("", window, cx));
        self.filter_text.clear();
        self.reshow();
        cx.notify();
    }

    /// Find (⌘F): the keyboard to the top bar's search box, its words
    /// chosen, as a code editor's ⌘P puts it in its command centre.
    pub fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| {
            f.focus(window, cx);
            f.select_all(window, cx);
        });
    }

    /// Find next (`forward`) or previous: the next row the search box shows (it narrows by name,
    /// address or note), going round the list, chosen and brought into view (gaps row U30, B48).
    pub fn find_step(&mut self, forward: bool) {
        use dm_desktop_base::i18n::tf;
        if self.filter_text.trim().is_empty() {
            return;
        }
        let from = self
            .shown
            .iter()
            .position(|&i| self.selected.contains(&self.rows[i].id));
        match dm_desktop_base::find::step(self.shown.len(), from, forward) {
            Some(at) => {
                let row = &self.rows[self.shown[at]];
                let name = dm_shell::app::names::name_of(row);
                self.selected = std::collections::BTreeSet::from([row.id]);
                self.scroll.scroll_to_item(at, gpui::ScrollStrategy::Center);
                self.say(tf("Found {}.", &[&name]));
            }
            None => self.say(tf("Nothing is called “{}”.", &[&self.filter_text])),
        }
    }

    /// Start Queue or Stop Queue for `queue`, and what came of it said:
    /// how many started (or that the schedule holds them, or that nothing
    /// was waiting) or stopped. Checking the Recheck queue is said when it
    /// is done.
    pub fn run_on_queue(
        &mut self,
        command: dm_shell::app::commands::Command,
        queue: &str,
        cx: &mut Context<Self>,
    ) {
        // Off the window's thread, as the commands on rows are (gaps row
        // 183), and said once done.
        let asked = queue.to_string();
        let queue = queue.to_string();
        self.later(
            move |backend| dm_shell::app::commands::verb(command, backend, &[], &asked),
            move |this, ran, cx| this.ran_on_queue(command, &queue, ran, cx),
            cx,
        );
    }

    /// What Start Queue or Stop Queue on `queue` did, said.
    fn ran_on_queue(
        &mut self,
        command: dm_shell::app::commands::Command,
        queue: &str,
        ran: Option<dm_shell::app::commands::Ran>,
        cx: &mut Context<Self>,
    ) {
        use dm_desktop_base::i18n::{tf, tn};
        use dm_shell::app::commands::{Command, Ran};
        self.read();
        let name = dm_desktop_base::i18n::name(queue);
        match (command, ran) {
            (Command::StartQueue, ran) => {
                let n = match ran {
                    Some(Ran::QueueStarted(n)) => n,
                    _ => 0,
                };
                let settings = self.backend.settings();
                // Its own sound, only where the person chose a file.
                if n > 0 {
                    if let Some(path) = settings
                        .sounds
                        .get("queue_started")
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                    {
                        self.backend.platform.play_file(&path);
                    }
                }
                let schedule = settings.schedule_for(queue);
                let lets = !schedule.enabled || schedule.open();
                self.say(if n == 0 {
                    tf("Nothing in {} was waiting to start.", &[&name])
                } else if !lets {
                    tf(
                        "{} is waiting for the schedule; it starts by itself when the schedule opens.",
                        &[&name],
                    )
                } else {
                    tn(
                        "Started {} download in {}.||Started {} downloads in {}.",
                        n as u64,
                        &[&n, &name],
                    )
                });
                self.said_schedule_wait = n > 0 && !lets;
                if queue == dm_core::RECHECK_QUEUE {
                    dm_shell::app::commands::recheck(&self.backend, self.rechecked.clone());
                }
            }
            (_, ran) => {
                let n = match ran {
                    Some(Ran::QueueStopped(n)) => n,
                    _ => 0,
                };
                self.say(tn(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &name],
                ));
            }
        }
        cx.notify();
    }

    /// The Speed limiter on or off, or a preset (Low, Medium, High,
    /// Unlimited), as `commands::limited` says, and said.
    fn speed(&mut self, command: dm_shell::app::commands::Command) {
        use dm_desktop_base::i18n::{t, tf};
        let now = self.backend.settings();
        let wanted =
            dm_shell::app::commands::limited(command, &now, &mut self.last_limit).unwrap_or(now);
        match self.backend.set_settings(wanted) {
            Ok(after) if after.speed_limit > 0 => {
                self.say(tf("Speed limiter on: {} KB/s.", &[&after.speed_limit]))
            }
            Ok(_) => self.say(t("Speed limiter off.").into()),
            Err(e) => self.say(e),
        }
        self.session.forget_limit();
    }

    /// The menu bar's queue submenus, built again when the queues change.
    pub(crate) fn refresh_menus(&mut self, cx: &mut Context<Self>) {
        let settings = self.backend.settings();
        let ticks = crate::menus::Ticks {
            queues: self.queue_list.clone(),
            when_done: self.session.when_done,
            speed_limit: settings.speed_limit,
            sidebar: self.show_sidebar,
            toolbar: self.show_toolbar,
            drop_target: settings.drop_target,
            column_order: settings.column_order.clone(),
            columns_hidden: settings.columns_hidden.clone(),
            toolbar_style: settings.toolbar_style.clone(),
            toolbar_order: settings.toolbar_order.clone(),
            toolbar_hidden: settings.toolbar_hidden.clone(),
            text_size: settings.text_size,
            language: settings.language.clone(),
            sorted_by: self.sort.and_then(|(column, _)| sort_command(column)),
            resume: dm_desktop_base::rows::resume_word(&self.chosen()),
            off: self.commands_off(),
        };
        if self.menu_ticks.as_ref() != Some(&ticks) {
            crate::menus::set(cx, &ticks);
            self.menu_ticks = Some(ticks);
        }
    }

    /// The Recheck queue's counts, once its check is done.
    pub(crate) fn say_rechecked(&mut self) {
        let done = dm_core::lock(&self.rechecked).take();
        if let Some((checked, changed)) = done {
            self.say(dm_desktop_base::i18n::tn(
                "Checked {} finished file with its server. Changed since, and downloaded again: {}.||Checked {} finished files with their servers. Changed since, and downloaded again: {}.",
                checked as u64,
                &[&checked, &changed],
            ));
        }
    }

    /// Adds what a search found. With `keep_layout` each file goes in a
    /// folder under the download folder that mirrors its address, one add
    /// for each folder; otherwise all of them in one.
    pub fn add_found(
        &mut self,
        urls: Vec<String>,
        with: dm_core::AddOptions,
        keep_layout: bool,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        if !keep_layout {
            let items = urls.into_iter().map(|u| (u, with.clone())).collect();
            return self.add_items(items, cx);
        }
        let root = self.backend.settings().download_dir;
        let mut items = Vec::new();
        for (folder, group) in dm_shell::app::search::by_site_folder(&urls, &root) {
            let mut here = with.clone();
            if folder.is_some() {
                here.save_dir = folder;
            }
            items.extend(group.into_iter().map(|u| (u, here.clone())));
        }
        self.add_items(items, cx)
    }

    /// Runs a saved search now, in the background; what it finds is taken
    /// in on a later wake (`take_searches`).
    pub fn run_search(&mut self, search: dm_shell::lists::Search) {
        let mut search = search;
        // The login Options › Sign-ins keeps for the site, where it keeps one.
        search.page = self.backend.settings().with_login(&search.page);
        let results = self.search_results.clone();
        let name = search.name.clone();
        dm_shell::app::search::crawl_later(&self.backend, search, move |found| {
            dm_core::lock(&results).push((name, found));
            dm_shell::app::changes::changed();
        });
    }

    /// Takes in what finished saved searches found (their new files added
    /// for later, said; what they have seen kept), and starts any saved
    /// search whose time has come, looked at once a minute.
    pub(crate) fn take_searches(&mut self, cx: &mut Context<Self>) {
        use dm_desktop_base::i18n::{tf, tn};
        let done: Vec<_> = std::mem::take(&mut *dm_core::lock(&self.search_results));
        for (name, found) in done {
            let settings = self.backend.settings();
            match found {
                Ok(found) => {
                    let saved = settings.site_searches.iter().find(|s| s.name == name);
                    let seen: Vec<String> = saved.map(|s| s.seen.clone()).unwrap_or_default();
                    let keep_layout = saved.is_some_and(|s| s.keep_layout);
                    // New: neither in the list nor found by this search
                    // before (`dm_shell::app::search`).
                    let finds = dm_shell::app::search::sort_finds(&found.files, &self.rows, &seen);
                    let new = finds.new;
                    let mut remember = finds.listed;
                    if new.is_empty() {
                        self.say(tf("The search {} found nothing new.", &[&name]));
                    } else {
                        let with = dm_core::AddOptions {
                            later: true,
                            queue: dm_core::MAIN_QUEUE.into(),
                            ..Default::default()
                        };
                        let added = self.add_found(new.clone(), with, keep_layout, cx);
                        let now = dm_shell::app::search::listed(&self.rows);
                        remember.extend(
                            new.iter()
                                .map(|f| dm_shell::lists::bare(f))
                                .filter(|f| now.contains(f)),
                        );
                        match added {
                            Ok(n) => self.say(tn(
                                "Added {} download to {}.||Added {} downloads to {}.",
                                n as u64,
                                &[&n, &dm_desktop_base::i18n::name(dm_core::MAIN_QUEUE)],
                            )),
                            Err(e) => self.problems.push(tf(
                                "the search {} found files that could not be added: {}",
                                &[&name, &e],
                            )),
                        }
                    }
                    let settings = self.backend.settings();
                    if let Some(wanted) =
                        dm_shell::app::search::seen_saved(&settings, &name, remember)
                    {
                        if let Err(e) = self.backend.set_settings(wanted) {
                            self.problems.push(tf(
                                "what the search {} has seen could not be saved: {}",
                                &[&name, &e],
                            ));
                        }
                    }
                }
                Err(e) => self
                    .problems
                    .push(tf("the search {} failed: {}", &[&name, &e])),
            }
        }
        let now = std::time::Instant::now();
        if self
            .searches_checked
            .is_some_and(|at| now.duration_since(at) < std::time::Duration::from_secs(60))
        {
            return;
        }
        self.searches_checked = Some(now);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let settings = self.backend.settings();
        let Some((due, wanted)) = dm_shell::app::search::due_now(&settings, epoch) else {
            return;
        };
        if let Err(e) = self.backend.set_settings(wanted) {
            // Still run, and not again for a minute; but said, since an
            // unsaved last run means the search runs again at every launch.
            self.problems.push(dm_desktop_base::i18n::tf(
                "when the saved searches last ran could not be saved: {}",
                &[&e],
            ));
        }
        for search in due {
            self.run_search(search);
        }
    }

    /// The progress window of `id`, opened (or brought forward).
    pub fn show_progress(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        if let Some(handle) = self.progress_windows.get(&id) {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        let nth = self.progress_windows.len();
        let me = cx.entity().downgrade();
        let main = self.main;
        cx.defer(move |cx| {
            if let Some(handle) = crate::progress::open(me.clone(), id, nth, main, cx) {
                let _ = me.update(cx, |view, _| {
                    view.progress_windows.insert(id, handle);
                });
            }
        });
    }

    /// Whether a person closed the progress window of `id`.
    pub(crate) fn progress_closed_has(&self, id: dm_core::Id) -> bool {
        self.progress_closed.contains(&id)
    }

    /// Closes the progress window of `id`, where one is open.
    fn close_progress(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        if let Some(handle) = self.progress_windows.remove(&id) {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Progress windows a person closed: forgotten, and not opened again by
    /// themselves.
    pub(crate) fn prune_progress(&mut self, cx: &mut Context<Self>) {
        let open = cx.windows();
        let gone: Vec<dm_core::Id> = self
            .progress_windows
            .iter()
            .filter(|(_, handle)| !open.contains(handle))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.progress_windows.remove(&id);
            self.progress_closed.insert(id);
        }
        if self.complete_window.is_some_and(|w| !open.contains(&w)) {
            self.complete_window = None;
            self.complete.clear();
        }
    }

    /// Downloads that finished: their progress windows close, and
    /// "Download complete" lists them, where that is asked for.
    pub(crate) fn finished(&mut self, ids: &[dm_core::Id], cx: &mut Context<Self>) {
        let show = self.backend.settings().show_complete;
        for id in ids {
            self.close_progress(*id, cx);
            if show {
                self.complete.insert(*id);
            }
        }
        if show && !self.complete.is_empty() && self.complete_window.is_none() {
            let me = cx.entity().downgrade();
            cx.defer(move |cx| {
                let handle = crate::complete::open(me.clone(), cx);
                let _ = me.update(cx, |view, _| view.complete_window = handle);
            });
        }
    }

    /// "Download complete" closed: a ticked "Don't show this window again"
    /// kept, and said.
    pub fn complete_closed(&mut self, never: bool, cx: &mut Context<Self>) {
        self.complete.clear();
        self.complete_window = None;
        if never {
            let mut wanted = self.backend.settings();
            wanted.show_complete = false;
            match self.backend.set_settings(wanted) {
                Ok(_) => self.say(
                    dm_desktop_base::i18n::t(
                        "This window won't open again. Turn it on in Options › General.",
                    )
                    .into(),
                ),
                Err(e) => self.say(e),
            }
        }
        cx.notify();
    }

    /// A file acted on in "Download complete": it leaves the list, and the
    /// window with the last.
    pub fn complete_done(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        self.complete.remove(&id);
        if self.complete.is_empty() {
            if let Some(handle) = self.complete_window.take() {
                // After the update that asked: its own button's click (Open,
                // Open folder) is still being handled inside that window,
                // which GPUI will not update again, and it stayed, empty.
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                });
            }
        }
        cx.notify();
    }

    /// Where an opener says what went wrong, from its thread.
    pub(crate) fn opener_said(&self) -> std::sync::Arc<std::sync::Mutex<Option<String>>> {
        self.opener_said.clone()
    }

    /// Opens a file with its usual app, off the window's thread.
    pub fn open_path(&self, path: &str) {
        let said = self.opener_said.clone();
        self.backend
            .platform
            .open_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Shows a file in the file manager, off the window's thread.
    pub fn reveal_path(&self, path: &str) {
        let said = self.opener_said.clone();
        self.backend
            .platform
            .reveal_later(path, Box::new(move |why| *dm_core::lock(&said) = Some(why)));
    }

    /// Open with…: the app chosen in the system's own panel, the window
    /// going on meanwhile; then `after`, chosen or not.
    pub fn open_path_with(
        &mut self,
        path: &str,
        after: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let chosen = crate::services::ask(crate::services::Asked::app());
        let path = path.to_string();
        cx.spawn(async move |this, cx| {
            let app = chosen.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(app) = app {
                    if let Err(e) = this.backend.platform.open_with(&path, &app) {
                        this.say(e);
                    }
                }
                after(this, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// What an opener said went wrong, from its thread.
    pub(crate) fn say_opener(&mut self) {
        let said = dm_core::lock(&self.opener_said).take();
        if let Some(why) = said {
            self.say(why);
        }
    }

    /// The name the list shows for `row`: one not yet placed reads as the
    /// name it will get ("big (1).zip", not "big.zip").
    pub(crate) fn name_shown(&self, row: &Entry) -> String {
        let settings = self.backend.settings();
        dm_shell::app::names::name_it_will_get_after(
            &self.rows,
            row,
            &settings.download_dir,
            &mut settings.categories(),
            None,
        )
    }

    /// Whether a dialog stands in front of the list: a question, or a
    /// dialog's window. Progress windows and "Download complete" stand
    /// beside it, not in front, as in the egui window.
    pub(crate) fn dialog_in_front(&self, cx: &gpui::App) -> bool {
        self.sheet_open
            || crate::screen::Screens::all(cx)
                .iter()
                .any(|(w, _, dialog)| {
                    *dialog
                        && !self.progress_windows.values().any(|p| p == w)
                        && self.complete_window.as_ref() != Some(w)
                })
    }

    /// Row `id`'s File properties window, where it is still open.
    pub(crate) fn properties_window(
        &self,
        id: dm_core::Id,
        cx: &gpui::App,
    ) -> Option<gpui::AnyWindowHandle> {
        let open = cx.windows();
        self.properties_windows
            .get(&id)
            .filter(|w| open.contains(w))
            .copied()
    }

    /// How many File properties windows are open.
    pub(crate) fn properties_open(&self, cx: &gpui::App) -> usize {
        let open = cx.windows();
        self.properties_windows
            .values()
            .filter(|w| open.contains(w))
            .count()
    }

    /// Row `id`'s File properties window opened; those since closed are
    /// forgotten.
    pub(crate) fn properties_opened(&mut self, id: dm_core::Id, window: gpui::AnyWindowHandle) {
        self.properties_windows.insert(id, window);
    }

    /// The dialog that keeps the clicks to itself and so holds this window
    /// (B42): Options, while open; on Linux an open question too, which GPUI
    /// holds a parent for only as a `Dialog`, which a question is not there
    /// (`question::KIND`). Elsewhere a question is a sheet or a `Dialog`, and
    /// the system holds the window for it.
    pub(crate) fn holder(&self, cx: &gpui::App) -> Option<gpui::AnyWindowHandle> {
        let open = cx.windows();
        let question = self
            .question_window
            .filter(|_| cfg!(not(any(target_os = "macos", windows))));
        [self.options_window, question]
            .into_iter()
            .flatten()
            .find(|w| open.contains(w))
    }

    /// Where a dialog holds the window, it comes forward; true if one does.
    pub(crate) fn bring_holder(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(holder) = self.holder(cx) else {
            return false;
        };
        cx.defer(move |cx| {
            let _ = holder.update(cx, |_, window, _| window.activate_window());
        });
        true
    }

    /// The main window.
    pub(crate) fn main_window(&self) -> gpui::AnyWindowHandle {
        self.main
    }

    /// The main window back in front, from the tray or wherever it is, after
    /// the update that asked.
    pub(crate) fn show_main(&mut self, cx: &mut Context<Self>) {
        self.hidden = false;
        let main = self.main;
        cx.defer(move |cx| {
            let _ = main.update(cx, |_, window, _| {
                crate::syswindow::show(window);
                window.activate_window();
            });
            // A dialog open over it stays in front, as a person left it:
            // brought forward after the main window, not buried under it.
            if let Some((dialog, _, _)) = crate::screen::Screens::all(cx)
                .into_iter()
                .find(|(_, _, dialog)| *dialog)
            {
                let _ = dialog.update(cx, |_, window, _| window.activate_window());
            }
            cx.activate(true);
        });
    }

    /// Whether a second launch, the browser or the Dock asked for the
    /// window since the last look.
    pub(crate) fn take_show_asked(&self) -> bool {
        self.show_asked
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    /// The rows chosen, in the list's order.
    pub fn chosen(&self) -> Vec<&Entry> {
        // From the choice, not from every row shown: the toolbar asks for
        // each of its buttons, a dozen times a frame (gaps row 180).
        let mut chosen: Vec<(usize, &Entry)> = self
            .selected
            .iter()
            .filter_map(|id| {
                let at = *self.positions.get(id)?;
                let row = self.shown.get(at).and_then(|&r| self.rows.get(r))?;
                (row.id == *id).then_some((at, &**row))
            })
            .collect();
        chosen.sort_unstable_by_key(|(at, _)| *at);
        chosen.into_iter().map(|(_, row)| row).collect()
    }

    /// The queue Start and Stop Queue act on: the one shown, else the Main
    /// queue.
    pub fn current_queue(&self) -> String {
        match &self.place {
            Place::Queue(name) => name.clone(),
            _ => dm_core::MAIN_QUEUE.to_string(),
        }
    }

    /// One read of the application: the rows and what follows from them.
    /// Says whether anything the window shows may have changed.
    ///
    /// A row the queue did not change is the queue's own `Arc` again, so
    /// what changed is found by comparing pointers. Where only figures
    /// moved (a running download's bytes, rate, time left), the names,
    /// places and choice worked out from the rows stand: worked out again
    /// at every read, they were most of a read of 10,000 rows (gaps row
    /// 181). They are worked out again where a row was added, removed or
    /// changed otherwise; and the names of the rows on screen and of those
    /// chosen every two seconds while anything changes, as a waiting row's
    /// name follows its folder (`name_it_will_get_after`).
    pub(crate) fn read(&mut self) -> bool {
        let queue_changed = dm_shell::app::changes::take();
        let settings = self.backend.settings();
        let read = self.session.read(&self.backend, &settings);
        // What it detected and the problems it met are the tick's.
        let mut changed = queue_changed || !read.problems.is_empty();
        self.detected.push(read.detected);
        self.problems.extend(read.problems);
        // What the read told beside the rows, which a row's words are said
        // from (ranges tried again, swarms, files taken by a later row, the
        // download limit), as the egui window sets it.
        dm_desktop_base::rows::set_retrying(read.snapshot.retrying);
        dm_desktop_base::rows::set_swarms(read.snapshot.swarms);
        dm_desktop_base::rows::set_limit_holds(read.snapshot.allowance_spent);
        dm_desktop_base::rows::set_hold_times(self.backend.allowance_spent(), &settings.schedule);
        let mut rows = read.snapshot.rows;
        // A finished row's size is its file's, read once from the disk.
        self.saved.apply(&mut rows);
        let same_rows = rows.len() == self.rows.len()
            && rows
                .iter()
                .zip(&self.rows)
                .all(|(new, old)| new.id == old.id);
        let mut moved = !same_rows || std::mem::take(&mut self.read_all);
        let mut finished_at = Vec::new();
        if same_rows {
            for (new, old) in rows.iter().zip(&self.rows) {
                if std::sync::Arc::ptr_eq(new, old) {
                    continue;
                }
                changed = true;
                finished_at.extend(new.finished_at);
                if !placed_alike(old, new) {
                    moved = true;
                    break;
                }
            }
        }
        changed |= moved;
        self.rows = rows;
        // Every row's finishing words where the rows are placed again, else
        // those of the rows that changed.
        self.keep_whens(&settings.language, moved, finished_at);
        if moved {
            self.named_at = Some(Instant::now());
            // The queues the rows name, for Start Queue's and Stop Queue's
            // arrows, the menu bar and the side panel: worked out from the
            // rows where they changed, not asked of the engine at each read
            // and each tick, a pass over every row under the queue's lock
            // each time (gaps row 181).
            self.queue_list = dm_core::queue_names(self.rows.iter().map(|r| &**r));
            // Each row by the name its file will get, as the egui window
            // shows it: a waiting second copy reads "name (1)".
            self.earlier = dm_shell::app::names::earlier_of_the_same_name(&self.rows);
            let every: Vec<usize> = (0..self.rows.len()).collect();
            self.shown_names.clear();
            self.name_rows(&every, &settings);
            dm_desktop_base::rows::set_superseded(&self.rows);
            // A row taken off the list is no longer chosen.
            let ids: std::collections::HashSet<dm_core::Id> =
                self.rows.iter().map(|r| r.id).collect();
            self.selected.retain(|id| ids.contains(id));
            self.places.place_rows(&self.rows, &settings.categories());
        } else if changed && self.named_at.is_none_or(|at| at.elapsed() >= NAMES_EVERY) {
            // A waiting row's name follows its folder (a file of its name
            // arriving there makes it "name (1)"), so while anything changes
            // the rows on screen and those chosen are named again every two
            // seconds; the rest are named again when the list itself changes.
            // Every row named again every two seconds was, at 10,000 waiting
            // rows, 5,000 looks at the disk a second (gaps row 181).
            self.named_at = Some(Instant::now());
            let seen = self.rows_seen();
            self.name_rows(&seen, &settings);
        }
        // The frame this read is for is drawn with these settings, not a
        // second copy of them.
        self.drawn = std::rc::Rc::new(settings);
        self.drawn_fresh = true;
        // Sorted again where a figure it sorts by may have moved.
        if moved || (changed && self.sort.is_some()) {
            self.reshow();
        }
        changed
    }

    /// Shows the rows of `place`.
    pub fn choose(&mut self, place: Place) {
        self.place = place;
        self.reshow();
    }

    /// A double-click on the row at `position`: what Options › General
    /// says a double-click does, Properties by default, for any download;
    /// Open or its folder for a finished one. A download that is running,
    /// waiting or stopped shows its progress window instead, which is what
    /// a person looking at one still going wants to see.
    pub fn double_click(&mut self, id: dm_core::Id, cx: &mut Context<Self>) {
        use dm_shell::app::commands::Command;
        let Some(row) = self
            .position_of(id)
            .and_then(|at| self.shown.get(at))
            .and_then(|&r| self.rows.get(r))
        else {
            return;
        };
        let finished = row.state == State::Completed;
        let going = matches!(row.state, State::Running | State::Queued | State::Paused);
        let id = row.id;
        let command = match self.backend.settings().double_click.as_str() {
            "open" if finished => Command::OpenFile,
            "folder" if finished => Command::OpenFolder,
            _ if going => Command::ShowProgress,
            _ => Command::Properties,
        };
        self.selected = [id].into();
        self.anchor = Some(id);
        self.run(command, cx);
    }

    /// Remove (or Delete file, `deleting_files`) on the rows chosen. Finished
    /// rows only leave the list, nothing lost, Undo on offer for a moment; any
    /// other is asked about first, as the egui window asks.
    fn remove(&mut self, chosen: &[Entry], deleting_files: bool, cx: &mut Context<Self>) {
        let ids: Vec<dm_core::Id> = chosen.iter().map(|r| r.id).collect();
        if !deleting_files && chosen.iter().all(|r| r.state == State::Completed) {
            let rows = self.backend.finished_rows(&ids);
            self.backend.remove_many(&ids);
            let n = self.undo.keep(rows);
            if n > 0 {
                self.undo_expires(cx);
                self.say(dm_desktop_base::i18n::tn(
                    "Removed {} download from the list.||Removed {} downloads from the list.",
                    n as u64,
                    &[&n],
                ));
            }
            self.read();
            return;
        }
        // The finished files still on disk that could go with their rows.
        let files: Vec<String> = chosen
            .iter()
            .filter(|r| r.state == State::Completed && std::path::Path::new(&r.path).is_file())
            .map(|r| r.path.clone())
            .collect();
        let named = match chosen {
            [only] => Some(dm_shell::app::names::name_of(only)),
            _ => None,
        };
        let rows: Vec<Entry> = chosen.to_vec();
        let count_files = files.len();
        let words = move |ticked: bool| {
            let refs: Vec<&Entry> = rows.iter().collect();
            let ask = dm_desktop_base::asks::remove(
                &refs,
                count_files,
                ticked,
                deleting_files,
                named.clone(),
            );
            crate::question::Asked {
                title: ask.title.into(),
                lines: std::iter::once(ask.sentence)
                    .chain(ask.files_sentence)
                    .map(Into::into)
                    .collect(),
                named: Some(ask.named.into()),
                detail: None,
                toggle: ask.toggle.map(Into::into),
                ticked,
                buttons: vec![ask.keep.into(), ask.go.into()],
                default: 0,
                escape: 0,
            }
        };
        // Delete file starts with its file ticked to go too, as the egui window's did; Remove
        // never does.
        let asked = words(deleting_files && count_files > 0);
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, delete, cx| {
            if button == 1 {
                let _ = view.update(cx, |view, cx| {
                    view.removed(&ids, &files, delete, cx);
                    cx.notify();
                });
            }
        });
        self.sheet(asked, Some(Box::new(words)), None, answer, cx);
    }

    /// The rows `ids` taken off the list, their finished `files` with them
    /// to the Trash where `delete`; the choice goes to the row after them.
    /// The Trash and the queue's change are made off the window's thread
    /// (each file is a round trip to the Finder on macOS: gaps row 183),
    /// and the choice moved and a failure said once they are done.
    fn removed(
        &mut self,
        ids: &[dm_core::Id],
        files: &[String],
        delete: bool,
        cx: &mut Context<Self>,
    ) {
        // The row after those removed, or the one before where they were
        // the last, is chosen next, so the keys have somewhere to go on from.
        let shown: Vec<dm_core::Id> = (0..self.shown.len())
            .filter_map(|p| self.id_at(p))
            .collect();
        let next = shown
            .iter()
            .rposition(|id| ids.contains(id))
            .and_then(|last| {
                shown[last + 1..]
                    .iter()
                    .find(|id| !ids.contains(id))
                    .or_else(|| shown[..last].iter().rev().find(|id| !ids.contains(id)))
                    .copied()
            });
        let (ids, files) = (ids.to_vec(), files.to_vec());
        self.later(
            move |backend| {
                let mut failed: Vec<String> = Vec::new();
                if delete {
                    for path in &files {
                        // A finished torrent still shared lets go of its files first.
                        dm_core::stop_sharing(std::path::Path::new(path));
                        if let Err(e) = backend.platform.trash(path) {
                            failed.push(format!("{path}: {e}"));
                        }
                    }
                }
                backend.remove_many(&ids);
                failed
            },
            move |this, failed, _| {
                this.read();
                this.selected = next.into_iter().collect();
                this.anchor = next;
                this.cursor = next;
                if !failed.is_empty() {
                    this.say(dm_desktop_base::i18n::tf(
                        "Could not delete {}",
                        &[&failed.join("; ")],
                    ));
                }
            },
            cx,
        );
    }

    /// Draws the window again once Undo's time is over, so a lapsed Undo is not left drawn
    /// while nothing else wakes the window (gaps row U14).
    fn undo_expires(&mut self, cx: &mut Context<Self>) {
        let over = dm_shell::app::backend::UNDO_FOR + std::time::Duration::from_millis(50);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(over).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Puts back the rows the last Remove took off, while Undo is on offer.
    pub fn undo_remove(&mut self) {
        if let Some(ids) = self.undo.put_back(&self.backend) {
            let n = ids.len();
            self.read();
            self.selected = ids.into_iter().collect();
            self.say(dm_desktop_base::i18n::tn(
                "Put back {} download.||Put back {} downloads.",
                n as u64,
                &[&n],
            ));
        }
    }

    /// The row of download `id`, found by where the list shows it, else
    /// by a look over every row (a row the place chosen does not show).
    pub(crate) fn row_of(&self, id: dm_core::Id) -> Option<&Arc<Entry>> {
        self.positions
            .get(&id)
            .and_then(|&at| self.shown.get(at))
            .and_then(|&r| self.rows.get(r))
            .filter(|r| r.id == id)
            .or_else(|| self.rows.iter().find(|r| r.id == id))
    }

    /// Says `sentence` in the status bar.
    pub fn say(&mut self, sentence: String) {
        self.said = Some((sentence, std::time::Instant::now()));
        self.said_schedule_wait = false;
    }

    /// Sorts by `column`: up, then the other way on a second click, then
    /// back in the order the downloads were added, as the egui window's
    /// header does; kept in the settings, so the list comes back sorted so
    /// after a restart (gaps row U29).
    pub fn sort_by(&mut self, column: Column, cx: &mut Context<Self>) {
        self.sort = dm_desktop_base::columns::next_sort(self.sort, column);
        self.reshow();
        let mut wanted = self.backend.settings();
        dm_desktop_base::columns::keep_sort(&mut wanted, self.sort);
        if let Err(e) = self.apply_settings(wanted, cx) {
            self.say(e);
        }
    }
}

/// The command that sorts by `column`, for View › Sort by's tick; none for
/// a column the menu does not sort by.
fn sort_command(column: Column) -> Option<dm_shell::app::commands::Command> {
    use dm_shell::app::commands::Command;
    Some(match column {
        Column::Name => Command::SortByName,
        Column::Size => Command::SortBySize,
        Column::Status => Command::SortByStatus,
        Column::TimeLeft => Command::SortByTimeLeft,
        Column::Rate => Command::SortByRate,
        Column::LastTry => Command::SortByDate,
        Column::Description => Command::SortByDescription,
        _ => return None,
    })
}

/// A row's picture of its file's kind, and the kind's colour (B45): told
/// apart by shape (a play mark, a note, lines, a window, a zip, a hill and a
/// sun, a plain sheet), in the egui window's colours.
pub(crate) fn kind_picture(kind: Option<dm_core::Category>) -> (&'static str, gpui::Rgba) {
    use dm_core::Category as K;
    match kind {
        Some(K::Videos) => ("icons/kind-videos.svg", gpui::rgb(0x8e44ad)),
        Some(K::Audio) => ("icons/kind-audio.svg", gpui::rgb(0xe67e22)),
        Some(K::Documents) => ("icons/kind-documents.svg", gpui::rgb(0x2b6cc4)),
        Some(K::Apps) => ("icons/kind-apps.svg", gpui::rgb(0x3d8b37)),
        Some(K::Archives) => ("icons/kind-archives.svg", gpui::rgb(0xb7931f)),
        Some(K::Pictures) => ("icons/kind-pictures.svg", gpui::rgb(0xc0395a)),
        _ => ("icons/kind-other.svg", gpui::rgb(0x7f8c8d)),
    }
}

/// What a row's line says, in the shell: its name and its state.
pub(crate) fn line(row: &Entry) -> (SharedString, SharedString) {
    let name = if row.filename.is_empty() {
        dm_shell::app::names::provisional(&row.url)
    } else {
        row.filename.clone()
    };
    let state = match &row.state {
        State::Running => match row.progress.total {
            Some(total) if total > 0 => {
                format!("{}%", row.progress.downloaded.saturating_mul(100) / total)
            }
            _ => dm_desktop_base::format::bytes(row.progress.downloaded),
        },
        State::Failed(why) => format!("Failed: {why}"),
        other => format!("{other:?}"),
    };
    (name.into(), state.into())
}

impl Render for Downloads {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::port::FRAMES.fetch_add(1, Ordering::Relaxed);
        // Its waits, copies and disk calls, counted in a debug build (row 180).
        let _drawing = dm_shell::app::costs::drawing();
        if !std::mem::take(&mut self.drawn_fresh) {
            self.drawn = std::rc::Rc::new(self.backend.settings());
        }
        // Resume's words in the menu bar follow the rows chosen (Retry,
        // Start now), however they were chosen: built again after this
        // frame where they changed.
        // And what is greyed follows them, and the rows' states (gaps row U5).
        let resume = dm_desktop_base::rows::resume_word(&self.chosen());
        let off = self.commands_off();
        if self
            .menu_ticks
            .as_ref()
            .is_some_and(|t| t.resume != resume || t.off != off)
        {
            let me = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = me.update(cx, |this, cx| this.refresh_menus(cx));
            });
        }
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let update_bar = self.update_bar(theme, &screen, cx);
        let holding = self.holder(cx).is_some();
        let browser_bar = self.browser_bar(theme, &screen, cx);
        let empty = self.shown.is_empty();
        div()
            .id("downloads")
            .role(Role::Application)
            .aria_label("dm")
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, run: &crate::menus::RunOnQueue, _, cx| {
                this.run_on_queue(run.command, &run.queue, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::menus::TipOfTheDay, _, cx| this.tip_of_the_day(cx)),
            )
            // View › Columns, Customize toolbar and Language.
            .on_action(
                cx.listener(|this, it: &crate::viewmenu::ToggleColumn, _, cx| {
                    this.toggle_column(&it.key, cx)
                }),
            )
            .on_action(
                cx.listener(|this, it: &crate::viewmenu::ToolbarStyle, _, cx| {
                    this.set_toolbar_style(&it.style, cx)
                }),
            )
            .on_action(
                cx.listener(|this, it: &crate::viewmenu::ToggleToolbarButton, _, cx| {
                    this.toggle_toolbar_button(&it.id, cx)
                }),
            )
            .on_action(
                cx.listener(|this, it: &crate::viewmenu::SetLanguage, _, cx| {
                    this.set_language(&it.code, cx)
                }),
            )
            // Files dropped on the window: the links they hold added.
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                this.dropped_files(&paths, cx);
            }))
            .on_action(
                cx.listener(|this, _: &crate::menus::CloseWindow, window, cx| {
                    if this.close_asked(window, cx) {
                        window.remove_window();
                    }
                }),
            )
            .on_action(cx.listener(|this, run: &crate::menus::Run, window, cx| {
                use dm_shell::app::commands::Command;
                // Held by Options: its window comes forward instead (B42).
                if this.bring_holder(cx) {
                    return;
                }
                match run.0 {
                    // The search box is the window's: these need it.
                    Command::Find => this.find(window, cx),
                    Command::FindNext => {
                        this.find_step(true);
                        cx.notify();
                    }
                    Command::FindPrevious => {
                        this.find_step(false);
                        cx.notify();
                    }
                    command => this.run(command, cx),
                }
            }))
            // Edit › Undo, Copy and Paste where no text field has the keys (a field's own come
            // first, in its context): Undo puts back what Remove took off, Copy copies the chosen
            // downloads' links, and Paste takes the links on the clipboard (gaps row U9).
            .on_action(cx.listener(|this, _: &gpui_base::input::Undo, _, cx| {
                this.undo_remove();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &gpui_base::input::Copy, _, cx| {
                use dm_shell::app::commands::Command;
                if !this.bring_holder(cx) {
                    this.run(Command::CopyLink, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &gpui_base::input::Paste, _, cx| {
                use dm_shell::app::commands::Command;
                if !this.bring_holder(cx) {
                    this.run(Command::AddFromClipboard, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &crate::menus::SelectPrevious, _, cx| {
                    if this.menu_open() {
                        this.step_menu(-1);
                    } else {
                        this.step(-1);
                    }
                    cx.notify();
                }),
            )
            .on_action(cx.listener(|this, _: &crate::menus::SelectNext, _, cx| {
                // In the open row menu, the arrows move along it (gaps row U6).
                if this.menu_open() {
                    this.step_menu(1);
                } else {
                    this.step(1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::menus::Confirm, window, cx| {
                if this.menu_open() {
                    this.confirm_menu(window, cx);
                } else {
                    this.run(dm_shell::app::commands::Command::ShowProgress, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::menus::Dismiss, _, cx| {
                if this.menu_open() || this.place_menu.is_some() {
                    this.close_menu();
                    this.close_place_menu();
                    cx.notify();
                }
            }))
            // A letter typed over the list finds the next row whose name starts with what was
            // typed (gaps row U6); not while a field or the search box has the keys.
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let held = &event.keystroke.modifiers;
                if !this.focus.is_focused(window)
                    || held.platform
                    || held.control
                    || held.alt
                    || held.function
                {
                    return;
                }
                if let Some(typed) = event.keystroke.key_char.as_deref() {
                    if typed.chars().count() == 1 && typed.chars().all(|c| !c.is_control()) {
                        this.type_to_find(typed);
                        cx.notify();
                    }
                }
            }))
            .on_action(cx.listener(|this, m: &crate::menus::MoveInList, _, cx| {
                use crate::menus::ListMove as M;
                use dm_desktop_base::selection::Step;
                let step = match m.to {
                    M::Up => Step::Up,
                    M::Down => Step::Down,
                    M::First => Step::First,
                    M::Last => Step::Last,
                    M::PageUp => Step::PageUp(this.page()),
                    M::PageDown => Step::PageDown(this.page()),
                };
                this.step_by(step, m.shift);
                this.refresh_menus(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::menus::RowMenuByKey, _, cx| {
                this.row_menu_by_key();
                cx.notify();
            }))
            // Clear under Mica (the toolbar on it too); the list's column
            // below stays solid.
            .bg(crate::theme::frame(theme.background))
            .text_color(theme.text)
            // The system's size for a list's rows and every control.
            .text_size(crate::zoom::px(crate::native::TEXT))
            .child(crate::titlebar::titlebar(self, theme, &screen, window, cx))
            .when(self.show_toolbar, |root| {
                root.child(crate::toolbar::toolbar(self, theme, &screen, window, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.show_sidebar, |row| {
                        row.child(crate::sidebar::sidebar(self, theme, &screen, window, cx))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .bg(theme.background)
                            // dm's update, downloaded and checked: Install
                            // now, or Later.
                            .children(update_bar)
                            // A download paused for want of room; those that
                            // did not finish; the way to add dm to a browser.
                            .children(self.stalled_bar(theme, &screen, cx))
                            .children(self.failed_bar(theme, &screen, cx))
                            .children(browser_bar)
                            // A link copied, offered.
                            .children(self.clipboard_bar(theme, &screen, cx))
                            // While a fresh link is awaited from the
                            // browser: what to do, and a way out.
                            .children(self.awaiting_bar(theme, &screen, cx))
                            // Problems from outside the list, each until
                            // dismissed, above it.
                            .children(self.problems.iter().enumerate().map(|(i, problem)| {
                                gpui::div()
                                    .flex()
                                    .items_center()
                                    .gap(crate::zoom::px(crate::native::LABEL_GAP))
                                    .px(crate::zoom::px(12.0))
                                    .py(crate::zoom::px(6.0))
                                    .border_b_1()
                                    .border_color(theme.separator)
                                    .child(gpui::div().flex_1().text_color(theme.warning).child(
                                        screen.label(dm_desktop_base::format::as_shown(problem)),
                                    ))
                                    .child(
                                        crate::native::button(
                                            ("dismiss", i),
                                            dm_desktop_base::i18n::t("Dismiss").into(),
                                            false,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if i < this.problems.len() {
                                                    this.problems.remove(i);
                                                }
                                                cx.notify();
                                            }),
                                        ),
                                    )
                            }))
                            // The headings and the rows scroll sideways
                            // together where the window is narrower than
                            // the columns, as a macOS list does: at 120% in
                            // a narrow window the columns after Status were
                            // out of reach. The sideways gesture alone moves
                            // them; the wheel still moves the rows. With no
                            // row, what the list says stands under them in
                            // the middle of what is in sight, not of the
                            // columns' whole width.
                            .child(
                                div()
                                    .id("table")
                                    .map(|table| crate::table::told_table(table, self))
                                    .flex()
                                    .when(empty, |table| table.flex_none())
                                    .when(!empty, |table| table.flex_1().min_h_0())
                                    .overflow_x_scroll()
                                    .restrict_scroll_to_axis()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .when(!empty, |list| list.h_full())
                                            .w_full()
                                            .min_w(crate::zoom::px(crate::table::least_width(self)))
                                            .child(crate::table::header(self, theme, &screen, cx))
                                            .when(!empty, |list| {
                                                list.child(crate::table::rows(self, theme, cx))
                                            }),
                                    ),
                            )
                            .when(empty, |list| {
                                list.child(crate::table::empty(self, theme, cx))
                            }),
                    ),
            )
            .child(crate::statusbar::statusbar(self, theme, &screen, cx))
            .when_some(self.menu, |root, at| {
                root.child(crate::rowmenu::menu(self, at, theme, &screen, cx))
            })
            .when_some(self.place_menu.clone(), |root, (place, at)| {
                root.child(crate::placemenu::menu(&place, at, theme, &screen, cx))
            })
            // Held by Options, the window takes no click: one brings Options
            // forward (B42).
            .when(holding, |root| {
                root.child(
                    div()
                        .id("held")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.bring_holder(cx);
                            }),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{clock, IDLE, RUNNING};
    use std::time::Duration;

    /// A running download's figures are drawn four times a second, as the
    /// engine reports them; a clock once a second; nothing moving, rarely.
    #[test]
    fn a_running_download_is_drawn_four_times_a_second() {
        assert_eq!(RUNNING, Duration::from_millis(250));
        assert_eq!(clock(true, false), RUNNING);
        assert_eq!(clock(true, true), RUNNING);
        assert_eq!(clock(false, true), Duration::from_secs(1));
        assert_eq!(clock(false, false), IDLE);
    }
}
