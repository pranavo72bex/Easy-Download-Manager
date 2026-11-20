// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the main window's view keeps, a struct per concern (gaps row A7):
//! the list as it is seen, what the last read worked out, the questions
//! asked, the windows open, the menus, the layout, the status bar's words,
//! the system's parts, the control port and the work off the window's
//! thread. `Downloads` holds one of each.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dm_desktop_base::columns::Column;
use dm_shell::app::places::{Place, Places};
use gpui::{
    AnyWindowHandle, AppContext as _, Context, Pixels, Point, UniformListScrollHandle, Window,
};

use super::Downloads;

/// The list as a person sees it: where it is scrolled, the place chosen,
/// the rows shown and their order, the rows chosen and the search box.
pub(crate) struct ListView {
    /// Where the list is scrolled, and where it is on screen.
    pub(crate) scroll: UniformListScrollHandle,
    /// Where the list stands sideways, for its sideways bar (gaps row U43).
    pub(crate) sideways: gpui::ScrollHandle,
    /// The rows on screen, as the list last laid them out.
    pub(crate) visible: std::ops::Range<usize>,
    /// The place chosen in the side panel.
    pub(crate) place: Place,
    /// Each row's place and the places' counts (`dm_shell::app::places`).
    pub(crate) places: Places,
    /// The rows the list shows, in the place chosen: their indices in `rows`.
    pub(crate) shown: Vec<usize>,
    /// Where each row shown is in the list, by id (`reshow`).
    pub(super) positions: HashMap<dm_core::Id, usize>,
    /// The column the list is sorted by, and whether up; none: as added.
    pub(crate) sort: Option<(Column, bool)>,
    /// The rows chosen, by id: they stay chosen as the list is read again
    /// and sorted, as a person expects of a selection.
    pub(crate) selected: BTreeSet<dm_core::Id>,
    /// Where a ⇧-click's range starts: the row last clicked alone.
    pub(crate) anchor: Option<dm_core::Id>,
    /// The keyboard's place: the row last clicked or moved to, which the
    /// arrows, Home, End and the page keys move from (`keys.rs`).
    pub(crate) cursor: Option<dm_core::Id>,
    /// What was typed over the list to find a row by its name, and when last (`keys.rs`).
    pub(crate) typed: (String, Option<Instant>),
    /// The top bar's search box: the words that narrow the list.
    pub(crate) filter: gpui::Entity<gpui_component::input::InputState>,
    /// Its words, as last typed.
    pub(crate) filter_text: String,
}

impl ListView {
    /// The list as a new window shows it: everything, as added, nothing
    /// chosen, the search box empty.
    pub(super) fn new(window: &mut Window, cx: &mut Context<Downloads>) -> ListView {
        ListView {
            scroll: UniformListScrollHandle::new(),
            sideways: gpui::ScrollHandle::new(),
            visible: 0..0,
            place: Place::All,
            places: Places::default(),
            shown: Vec::new(),
            positions: Default::default(),
            sort: None,
            selected: Default::default(),
            anchor: None,
            cursor: None,
            typed: (String::new(), None),
            filter: cx.new(|cx| {
                gpui_component::input::InputState::new(window, cx)
                    .placeholder(dm_desktop_base::i18n::t("Name, address or note"))
            }),
            filter_text: String::new(),
        }
    }
}

/// What the last read of the application worked out for drawing, kept so
/// that a frame does not work it out again (`refresh.rs`).
#[derive(Default)]
pub(crate) struct Known {
    /// The settings a frame is drawn with, read once as it begins (and at
    /// each read): its parts read them a dozen times a frame, each a copy
    /// of the whole under its lock (gaps row 180).
    pub(crate) drawn: std::rc::Rc<dm_shell::Settings>,
    /// `drawn` was just read, by a read for the frame about to be drawn.
    pub(super) drawn_fresh: bool,
    /// The name each row is shown by: the one its file will get, a waiting
    /// second copy's "name (1)", worked out at each read.
    pub(crate) shown_names: HashMap<dm_core::Id, String>,
    /// The queues, as the last read found them: Start Queue's and Stop
    /// Queue's arrows list them, and asking the engine as each frame was
    /// drawn held the window up behind the queue's lock.
    pub(crate) queue_list: Vec<String>,
    /// When each row finished, in words, kept by the read (`keep_whens`).
    pub(crate) whens: dm_desktop_base::format::Whens,
    /// Finished rows' sizes, read once from their files.
    pub(crate) saved: dm_shell::app::snapshot::SavedSizes,
    /// The size a dialog learned of a row it added (Download links asks each link's server),
    /// shown until the row knows its own.
    pub(crate) sizes_told: HashMap<dm_core::Id, u64>,
    /// How many earlier rows of the same name bound for the same folder
    /// take a free name before each row, as the last whole read counted
    /// them (`names::earlier_of_the_same_name`).
    pub(super) earlier: HashMap<dm_core::Id, usize>,
    /// When the names and places were last worked out (`read`).
    pub(super) named_at: Option<Instant>,
    /// Some rows are not named yet: before the first frame only the rows it shows are, the
    /// rest once it is drawn (`name_the_rest`, gaps row S16).
    pub(super) names_owed: bool,
    /// The next read works everything out again (the settings changed).
    pub(crate) read_all: bool,
    /// How many rows move with the clock (running, or a file's days
    /// before the Trash), kept by the read (`moves_with_time`).
    pub(super) timed_rows: usize,
    /// How many rows have a time to move their file to the Trash ("Delete
    /// file after"), kept by the read: the tick looks for those due only
    /// where one has (gaps row 181).
    pub(crate) deleting: usize,
    /// The rows running, paused or failed, by id, kept by the read from the
    /// rows that changed: the tick's own looks (the taskbar's, the clock's,
    /// the hints', the speeds') go over them, not over every row (gaps row
    /// 181).
    pub(crate) busy: std::collections::BTreeMap<dm_core::Id, std::sync::Arc<dm_core::Entry>>,
    /// How many times the window was built: a wake that changed nothing
    /// builds it no more (gaps row 181).
    pub(crate) renders: u64,
    /// When the finished rows' files were last looked for, and whether a
    /// look is under way (`look_for_files`).
    pub(super) files_looked: Option<Instant>,
    pub(super) looking_for_files: bool,
}

/// The questions the window asks, as sheets or windows of their own, one
/// at a time, and the asks waiting their turn.
pub(crate) struct Questions {
    /// The questions the rows raise as they run (B39 to B41), asked one at
    /// a time in the order they came.
    pub(super) asks: dm_shell::app::questions::Questions,
    /// Names a person chose for rows still downloading, given as each
    /// finishes.
    pub(super) renames: dm_shell::app::names::PendingRenames,
    /// A question is being asked: the next waits for its answer.
    pub(super) asking: bool,
    /// The main window: a sheet is opened on it alone.
    pub(super) main: AnyWindowHandle,
    /// Questions waiting to be asked as sheets, one at a time.
    pub(super) sheets: VecDeque<(
        crate::dialogs::question::Asked,
        Option<crate::dialogs::question::Reword>,
        Option<crate::dialogs::question::Live>,
        crate::dialogs::question::Answer,
    )>,
    /// A sheet is open on the main window.
    pub(super) sheet_open: bool,
    /// The open question's window: on Linux it holds the main window as
    /// Options does (`holder`), since GPUI's X11 and Wayland windows hold
    /// their parent only for a `Dialog`, which a question is not there.
    pub(super) question_window: Option<AnyWindowHandle>,
    /// "Exit dm?" is being asked: Exit again does not ask twice.
    pub(crate) exit_asked: bool,
    /// Exit was carried out (`quit`): what the window's tests read, as GPUI's test platform
    /// ends nothing.
    pub(crate) quit_asked: bool,
    /// A browser's hand-off is being asked about (Download details or
    /// Download links): the next waits for it.
    pub(crate) handed_open: bool,
    /// Hand-offs cancelled, by site (`handoff::Cancels`): the second asks
    /// whether to leave that site's downloads to the browser.
    pub(super) cancelled_hand_offs: dm_shell::app::handoff::Cancels,
    /// When done's countdown is being shown.
    pub(super) countdown_shown: bool,
}

impl Questions {
    /// Nothing asked yet, sheets to open on `main`.
    pub(super) fn new(main: AnyWindowHandle) -> Questions {
        Questions {
            asks: Default::default(),
            renames: Default::default(),
            asking: false,
            main,
            sheets: Default::default(),
            sheet_open: false,
            question_window: None,
            exit_asked: false,
            quit_asked: false,
            handed_open: false,
            cancelled_hand_offs: Default::default(),
            countdown_shown: false,
        }
    }
}

/// The windows the main window opens and keeps track of, and its own frame.
pub(crate) struct OpenWindows {
    /// Each progress window open, by its download.
    pub(crate) progress_windows: BTreeMap<dm_core::Id, AnyWindowHandle>,
    /// Progress windows a person closed: not opened again by themselves.
    pub(super) progress_closed: HashSet<dm_core::Id>,
    /// The downloads "Download complete" lists.
    pub(crate) complete: BTreeSet<dm_core::Id>,
    /// "Download complete", while it is open.
    pub(crate) complete_window: Option<AnyWindowHandle>,
    /// Each download's File properties window, while open.
    pub(crate) properties_windows: BTreeMap<dm_core::Id, AnyWindowHandle>,
    /// About dm, while open.
    pub(crate) about_window: Option<AnyWindowHandle>,
    /// Tip of the day, while open.
    pub(crate) tip_window: Option<AnyWindowHandle>,
    /// Options, while open.
    pub(crate) options_window: Option<AnyWindowHandle>,
    /// The floating drop target, while shown (`droptarget.rs`).
    pub(crate) drop_window: Option<AnyWindowHandle>,
    /// View › Statistics, while open (gaps row W15).
    pub(crate) statistics_window: Option<AnyWindowHandle>,
    /// The main window's frame as it last stood, kept as dm quits for the
    /// next start (`frame.rs`).
    pub(super) frame: gpui::WindowBounds,
    /// The display it stood on, by its id.
    pub(super) display: Option<u64>,
}

impl OpenWindows {
    /// No window open beside the main one, whose `frame` stands on `display`.
    pub(super) fn new(frame: gpui::WindowBounds, display: Option<u64>) -> OpenWindows {
        OpenWindows {
            progress_windows: Default::default(),
            progress_closed: Default::default(),
            complete: Default::default(),
            complete_window: None,
            properties_windows: Default::default(),
            about_window: None,
            tip_window: None,
            options_window: None,
            drop_window: None,
            statistics_window: None,
            frame,
            display,
        }
    }
}

/// The menus: the row menu, a place's menu and the menu bar.
#[derive(Default)]
pub(crate) struct Menus {
    /// Where the row menu last opened: what a test reads, as the system's menu itself is not
    /// the window's to see (`Downloads::show_row_menu`).
    pub(crate) row_menu_opened: Option<Point<Pixels>>,
    /// A place's right-click menu, open where it was asked for (`placemenu`).
    pub(crate) place_menu: Option<(Place, Point<Pixels>)>,
    /// What the menu bar was last built with (its queues and ticks); none
    /// builds it again.
    pub(crate) menu_ticks: Option<crate::window::menus::Ticks>,
    /// The window menu last opened, by its place in the bar: what a test reads, as the
    /// system's menu itself is not the window's to see.
    pub(crate) bar_menu_opened: Option<usize>,
}

/// What of the window is shown and how wide: the side panel, the toolbar,
/// the bars above the list, the columns being dragged, the palette.
#[derive(Default)]
pub(crate) struct Layout {
    /// View › Categories: the side panel shown.
    pub(crate) show_sidebar: bool,
    /// View › Toolbar: the toolbar shown.
    pub(crate) show_toolbar: bool,
    /// What the bars above the list remember (`bars.rs`).
    pub(crate) bars: crate::window::bars::Bars,
    /// A column's width as its heading's edge is dragged, and when the drag's width was last
    /// kept (`table.rs`).
    pub(crate) resizing: Option<(Column, f32)>,
    pub(crate) resize_kept: Option<Instant>,
    /// The side panel's width as its edge is dragged (`sidebar.rs`), kept as `resize_kept` says.
    pub(crate) panel_resizing: Option<f32>,
    /// Windows' High Contrast palette as last drawn (`tick.rs`): GPUI tells
    /// the window of light and dark only.
    pub(crate) contrast: Option<dm_desktop_base::contrast::Palette>,
    /// The phone's layout's drawer and sheet (`narrow.rs`).
    pub(crate) narrow: crate::window::narrow::Narrow,
}

/// What the window tells a person: the status bar's sentence, what a
/// screen reader hears, the problems above the list, and Undo.
#[derive(Default)]
pub(crate) struct Status {
    /// A sentence for the status bar, and when it was said: it stands for
    /// a while.
    pub(crate) said: Option<(String, Instant)>,
    /// Whether `said` says a queue waits for the schedule, so the status bar does not say it
    /// twice (`statusbar::parts`).
    pub(crate) said_schedule_wait: bool,
    /// What finished or failed last, said aloud to a screen reader from the status bar and not
    /// drawn (`a11y::heard`, gaps row U23), and when.
    pub(crate) heard: Option<(String, Instant)>,
    /// Problems to show above the list, each until dismissed.
    pub(crate) problems: Vec<String>,
    /// Finished rows Remove took off, kept a moment for Undo.
    pub(crate) undo: dm_shell::app::backend::Undo,
    /// The downloads' speeds together since dm opened, for View › Statistics's average and
    /// best (gaps row W15): in memory only.
    pub(crate) speeds: dm_desktop_base::statistics::SpeedsSeen,
}

/// The system's parts the window works with: the tray, the taskbar button
/// and the clipboard.
#[derive(Default)]
pub(crate) struct System {
    /// The status item in the menu bar (`tray.rs`); `None` where the system
    /// would not make one, and closing the window then ends dm.
    pub(crate) tray: Option<crate::platform::tray::Tray>,
    /// The window is put away in the tray.
    pub(crate) hidden: bool,
    /// The progress on the taskbar button (Windows; nothing elsewhere).
    pub(super) taskbar: dm_desktop_base::taskbar::Taskbar,
    /// The clipboard, watched where Options says (`clipwatch.rs`).
    pub(crate) clipboard: dm_shell::app::links::Clipboard,
    /// The clipboard is being read off the window's thread (`clipwatch`).
    pub(crate) clipboard_reading: bool,
}

/// What asks the window from outside it: the control port, and a second
/// launch, the browser or the Dock asking for the window.
#[derive(Default)]
pub(crate) struct Port {
    /// The control port's requests, answered on this thread.
    pub(super) requests: Option<std::sync::mpsc::Receiver<dm_desktop_base::control::Request>>,
    /// The port was opened only for AI assistants.
    pub(crate) assistants_only: bool,
    /// When a control client last acted: a file is not run, nor the update
    /// installed, by a script's click a moment ago (gaps row 159 f).
    pub(crate) scripted: dm_shell::app::control_api::ScriptGuard,
    /// Set when a second launch, the browser or the Dock asks for the window.
    pub(super) show_asked: Arc<AtomicBool>,
}

/// Work handed off the window's thread, and what it brings back: checks,
/// saved searches, openers, the modem and the update.
pub(crate) struct Threads {
    /// Work handed off the window's thread and not answered yet, and the
    /// control port's `command.run` answers held until it is
    /// (`list/later.rs`).
    pub(super) errands: super::later::Pending,
    /// The Recheck queue's counts, once a check of it is done.
    pub(super) rechecked: Arc<Mutex<Option<(usize, usize)>>>,
    /// What saved searches found, as each run ends, by the search's name.
    #[allow(clippy::type_complexity)]
    pub(super) search_results: Arc<Mutex<Vec<(String, Result<dm_shell::lists::Crawled, String>)>>>,
    /// When the saved searches were last looked at.
    pub(crate) searches_checked: Option<Instant>,
    /// What an opener said went wrong, from its thread.
    pub(super) opener_said: Arc<Mutex<Option<String>>>,
    /// What Scheduler › Hang up modem when done came to, from its thread.
    pub(crate) hung_up: Arc<Mutex<Option<dm_shell::dialup::HungUp>>>,
    /// What the update check has found.
    pub(crate) checked: crate::dialogs::about::Checked,
    /// dm's update being downloaded, offered to install once verified.
    pub(crate) update_row: Option<dm_core::Id>,
    /// The weekly check asked: About opens if it finds a newer dm.
    pub(crate) weekly_pending: bool,
}

impl Default for Threads {
    /// Nothing handed off yet, the update not asked about.
    fn default() -> Threads {
        Threads {
            errands: Default::default(),
            rechecked: Default::default(),
            search_results: Default::default(),
            searches_checked: None,
            opener_said: Default::default(),
            hung_up: Default::default(),
            checked: Arc::new(Mutex::new(dm_shell::app::update::Update::NotAsked)),
            update_row: None,
            weekly_pending: false,
        }
    }
}
