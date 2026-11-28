// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! EDM's menus where the system keeps them: on macOS the menu bar, with the
//! app menu macOS programs have (About, Options, Services, Hide, Quit) and
//! then EDM's own menus (Tasks, File, Downloads, View, Help),
//! built from the application's command registry (`Command`: each one's
//! label, in the language in force, and its keys), in the same order. A
//! command the shell cannot carry out yet is shown greyed, not run.

use edm_desktop_base::i18n::t;
use edm_shell::app::commands::{Chord, Command, Key};
use edm_shell::app::power::WhenDone;
use gpui::{Action, App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};

mod by_path;
pub(crate) use by_path::action_at;
mod bar_menus;

use bar_menus::{downloads_menu, file_menu, help_menu, view_menu};

/// What the menus show ticked, and the queues Start Queue and Stop Queue
/// list: the menu bar is built again
/// when any of it changes (`Downloads::refresh_menus`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ticks {
    /// The queues, for Start queue's and Stop queue's submenus.
    pub queues: Vec<String>,
    /// Downloads › When done's choice.
    pub when_done: WhenDone,
    /// The speed limit in force, in KB/s (0: none): Limit speed is ticked
    /// while there is one, and so is the preset it is.
    pub speed_limit: u64,
    /// View › Categories.
    pub sidebar: bool,
    /// View › Toolbar.
    pub toolbar: bool,
    /// View › Details (gaps row E8).
    pub details: bool,
    /// View › Drop target.
    pub drop_target: bool,
    /// The columns' order and those hidden, by key (View › Columns).
    pub column_order: Vec<String>,
    /// The columns hidden, by key.
    pub columns_hidden: Vec<String>,
    /// The toolbar's style, its buttons' order and those hidden (View ›
    /// Customize toolbar).
    pub toolbar_style: String,
    /// The toolbar's buttons in the person's order, by command id.
    pub toolbar_order: Vec<String>,
    /// The toolbar's buttons hidden, by command id.
    pub toolbar_hidden: Vec<String>,
    /// The text size in force (View › Text size's percentage).
    pub text_size: f32,
    /// The language setting (View › Language).
    pub language: String,
    /// The sort in force, as the command that sorts so (View › Sort by).
    pub sorted_by: Option<Command>,
    /// Resume's words for the rows chosen, in English (Retry, Start now:
    /// `rows::resume_word`); none for Resume's own.
    pub resume: &'static str,
    /// The commands that do not apply to the rows chosen now (`Downloads::enabled`): greyed in
    /// the menus exactly when their toolbar buttons are (gaps row U5).
    pub off: Vec<Command>,
}

thread_local! {
    /// `Ticks::off` for the menus being built: `item` greys what it holds.
    static OFF: std::cell::RefCell<Vec<Command>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A command of EDM's, as a menu item or a key runs it.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = edm, no_json)]
pub struct Run(pub Command);

/// Start Queue or Stop Queue for the queue named, whatever the list shows
/// (B64): Downloads › Start queue's and Stop queue's submenus, and the
/// toolbar's arrows beside them.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = edm, no_json)]
pub struct RunOnQueue {
    /// Start Queue or Stop Queue.
    pub command: Command,
    /// The queue, by its name in EDM's list.
    pub queue: String,
}

/// Ends EDM (Exit, ⌘Q): the application's, so it is heard from any window
/// in front, as Quit is in every macOS program; the list's commands
/// (`Run`) are the main window's.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct Quit;

/// Chooses the row above, as ↑ does in a macOS list.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct SelectPrevious;

/// Chooses the row below, as ↓ does.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct SelectNext;

/// Home, End, Page Up and Page Down in the list, and with Shift the arrows too: the choice moved
/// or, with `shift`, extended (`list/keys.rs`).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = edm, no_json)]
pub struct MoveInList {
    /// Where to.
    pub to: ListMove,
    /// Whether the run from where it started is chosen.
    pub shift: bool,
}

/// A move of the keyboard's place in the list.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ListMove {
    /// One row up.
    Up,
    /// One row down.
    Down,
    /// The first row.
    First,
    /// The last row.
    Last,
    /// A screenful up.
    PageUp,
    /// A screenful down.
    PageDown,
}

/// Return over the list: the chosen downloads' progress.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct Confirm;

/// Escape over the list: the open place menu closed (the row menu is the system's, which
/// closes itself).
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct Dismiss;

/// Tab: the keyboard to the next control of the window in front (gaps row U2).
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct FocusNext;

/// Shift+Tab: the keyboard to the control before.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct FocusPrevious;

/// Shift+F10: the row menu at the keyboard's place.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct RowMenuByKey;

/// Ctrl+Tab: Options' next page (gaps row U2).
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct NextPage;

/// Ctrl+Shift+Tab: Options' page before.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct PreviousPage;

/// F10: the window's first menu (Tasks) opened under its title, as a desktop's menu bar opens
/// from the keyboard (gaps row U6).
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct MenuBarByKey;

/// A page of the project's, or the email that tells a friend: Help's
/// topics, home page, support and Tell a friend, opened by the system.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = edm, no_json)]
pub struct OpenPage {
    /// The page's address (or the email's `mailto:`).
    pub address: String,
}

/// One of Help › How to…'s step-by-step guides, by its page's name (gaps row C16d): written
/// offline under EDM's home and opened by the system, as a dialog's ? button opens its page.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = edm, no_json)]
pub struct OpenGuide {
    /// The page's name, as `edm_desktop_base::help::write_page` knows it.
    pub page: String,
}

/// The Dock menu's "Open download folder": the folder Options › Save to
/// names, in Finder, whether or not EDM's window is showing (gaps row U16).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = edm, no_json)]
pub struct OpenDownloadFolder;

/// File › Close (⌘W), as every macOS program has it: the main window's
/// close button, whatever Options says it does.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct CloseWindow;

/// Window › Minimize (⌘M), as every macOS program's Window menu has it.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct Minimize;

/// Window › Zoom.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct Zoom;

/// Window › Bring All to Front.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct BringAllToFront;

/// Help › Tip of the day.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct TipOfTheDay;

/// Help › Copy a report (gaps row W8): what an issue needs, on the clipboard, sent nowhere.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct CopyReport;

/// Hides EDM, as every macOS program's app menu does.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct Hide;

/// Hides the other programs.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct HideOthers;

/// Shows the other programs again.
#[derive(Clone, PartialEq, Default, Action)]
#[action(namespace = edm, no_json)]
pub struct ShowAll;

/// What the shell can carry out today: Exit, and the commands the
/// application carries out by itself (`commands::verb`); the rest open a
/// dialog, which comes with its slice.
pub fn carried_out(command: Command) -> bool {
    use Command as C;
    matches!(
        command,
        C::Quit
            | C::AddUrl
            | C::DownloadNow
            | C::StopDownload
            | C::Redownload
            | C::RemoveFromQueue
            | C::MoveUp
            | C::MoveDown
            | C::ResumeAll
            | C::StopAll
            | C::DeleteCompleted
            | C::CopyLink
            | C::Remove
            | C::DeleteFile
            | C::CopyDetails
            | C::Find
            | C::FindNext
            | C::FindPrevious
            | C::Scheduler
            | C::SiteFiles
            | C::ShowProgress
            | C::AddSeries
            | C::AddFromClipboard
            | C::About
            | C::CheckForUpdates
            | C::Options
            | C::ToggleDropTarget
            | C::UpdateLink
            | C::MoveToQueue
            | C::MoveRename
            | C::Properties
            | C::OpenFile
            | C::OpenWith
            | C::Preview
            | C::OpenFolder
            | C::ExtractHere
            | C::ExtractTo
            | C::SelectAll
            | C::Import
            | C::Export
            | C::BackUp
            | C::Restore
            | C::Help
            | C::ToggleCategories
            | C::ToggleToolbar
            | C::ToggleDetails
            | C::SortByName
            | C::SortBySize
            | C::SortByStatus
            | C::SortByTimeLeft
            | C::SortByRate
            | C::SortByDate
            | C::SortByDescription
            | C::SpeedLimiterToggle
            | C::SpeedLimiterSettings
            | C::SpeedLow
            | C::SpeedMedium
            | C::SpeedHigh
            | C::SpeedUnlimited
            | C::StartQueue
            | C::StopQueue
            | C::WhenDoneNothing
            | C::WhenDoneQuit
            | C::WhenDoneSleep
            | C::WhenDoneHibernate
            | C::WhenDoneShutDown
    )
}

/// A key as GPUI names it in a binding.
fn key_name(key: Key) -> &'static str {
    match key {
        Key::A => "a",
        Key::F => "f",
        Key::I => "i",
        Key::N => "n",
        Key::Q => "q",
        Key::R => "r",
        Key::Comma => ",",
        Key::Period => ".",
        Key::F1 => "f1",
        Key::F3 => "f3",
        Key::Backspace => "backspace",
        Key::Delete => "delete",
    }
}

/// A chord as GPUI writes a keystroke: ⌘ on macOS and Ctrl elsewhere, as
/// EDM's chords mean.
pub fn keystroke(chord: &Chord) -> String {
    let mut words = Vec::new();
    if chord.command {
        words.push(if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        });
    }
    if chord.shift {
        words.push("shift");
    }
    words.push(key_name(chord.key));
    words.join("-")
}

/// Every command's keys, bound to running it.
pub fn bind_keys(cx: &mut App) {
    let mut bindings = Vec::new();
    for command in Command::ALL {
        for chord in command.chords() {
            // Bound once, at start: GPUI keeps a binding's keystroke for the
            // life of the program.
            let keys: &'static str = keystroke(&chord).leak();
            // Checked first: GPUI's binding panics on a keystroke it cannot
            // read, and a key EDM cannot bind is said, not a crash.
            if let Err(e) = gpui::Keystroke::parse(keys) {
                edm_core::note!(
                    warn,
                    "edm-gpui: {} cannot be bound to {keys}: {e:?}",
                    command.id()
                );
                continue;
            }
            if *command == Command::Quit {
                bindings.push(KeyBinding::new(keys, Quit, None));
            } else {
                bindings.push(KeyBinding::new(keys, Run(*command), None));
            }
        }
    }
    // Showing and hiding the categories: macOS's own keys for a sidebar.
    bindings.push(KeyBinding::new(
        "ctrl-cmd-s",
        Run(Command::ToggleCategories),
        None,
    ));
    // ⌘W on macOS, Ctrl+W elsewhere: Close, as each system says it.
    bindings.push(KeyBinding::new(
        if cfg!(target_os = "macos") {
            "cmd-w"
        } else {
            "ctrl-w"
        },
        CloseWindow,
        None,
    ));
    // View › Text size, from any window, as a browser's zoom: ⌘ on macOS
    // and Ctrl elsewhere with + (or =, the same key unshifted), − and 0.
    let (larger, smaller, normal): (&[&'static str], &'static str, &'static str) =
        if cfg!(target_os = "macos") {
            (&["cmd-=", "cmd-+", "cmd-shift-="], "cmd--", "cmd-0")
        } else {
            (&["ctrl-=", "ctrl-+", "ctrl-shift-="], "ctrl--", "ctrl-0")
        };
    for keys in larger {
        bindings.push(KeyBinding::new(
            keys,
            crate::window::viewmenu::TextLarger,
            None,
        ));
    }
    bindings.push(KeyBinding::new(
        smaller,
        crate::window::viewmenu::TextSmaller,
        None,
    ));
    bindings.push(KeyBinding::new(
        normal,
        crate::window::viewmenu::TextNormal,
        None,
    ));
    // Minimize is macOS's ⌘M; Windows and Linux have no key for it.
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-m", Minimize, None));
    }
    // Undo, Copy and Paste for the list (`list.rs`): a text field's own bindings, in its
    // context, come before these, so its text is copied and pasted as ever.
    let (undo, copy, paste) = if cfg!(target_os = "macos") {
        ("cmd-z", "cmd-c", "cmd-v")
    } else {
        ("ctrl-z", "ctrl-c", "ctrl-v")
    };
    bindings.push(KeyBinding::new(undo, gpui_base::input::Undo, None));
    bindings.push(KeyBinding::new(copy, gpui_base::input::Copy, None));
    bindings.push(KeyBinding::new(paste, gpui_base::input::Paste, None));
    bindings.push(KeyBinding::new("up", SelectPrevious, None));
    bindings.push(KeyBinding::new("down", SelectNext, None));
    // The rest of the list's keys (gaps row U6). A text field
    // binds these in its own context, which GPUI puts first.
    for (keys, to, shift) in [
        ("home", ListMove::First, false),
        ("end", ListMove::Last, false),
        ("pageup", ListMove::PageUp, false),
        ("pagedown", ListMove::PageDown, false),
        ("shift-up", ListMove::Up, true),
        ("shift-down", ListMove::Down, true),
        ("shift-home", ListMove::First, true),
        ("shift-end", ListMove::Last, true),
        ("shift-pageup", ListMove::PageUp, true),
        ("shift-pagedown", ListMove::PageDown, true),
    ] {
        bindings.push(KeyBinding::new(keys, MoveInList { to, shift }, None));
    }
    // Enter shows the chosen downloads' progress.
    bindings.push(KeyBinding::new("enter", Confirm, None));
    bindings.push(KeyBinding::new("escape", Dismiss, None));
    bindings.push(KeyBinding::new("shift-f10", RowMenuByKey, None));
    // The keyboard's Menu key, on Windows and Linux keyboards, as a right-click (gaps row U6).
    bindings.push(KeyBinding::new("menu", RowMenuByKey, None));
    bindings.push(KeyBinding::new("f10", MenuBarByKey, None));
    // A tab view's next and previous page, on every system (Options).
    bindings.push(KeyBinding::new("ctrl-tab", NextPage, None));
    bindings.push(KeyBinding::new("ctrl-shift-tab", PreviousPage, None));
    // Tab and Shift+Tab walk a window's controls (gaps row U2), from a text field too: its own
    // indenting, bound in its context, is for a code editor, not a form, and this binding of the
    // same context, made after it, comes first.
    for context in [None, Some("Input")] {
        bindings.push(KeyBinding::new("tab", FocusNext, context));
        bindings.push(KeyBinding::new("shift-tab", FocusPrevious, context));
    }
    cx.bind_keys(bindings);
    hear_app_actions(cx);
}

/// The application's own actions, heard from any window: registered once (`bind_keys`), as the
/// menus are built again whenever the queues change.
fn hear_app_actions(cx: &mut App) {
    // On the window the keys went to (the control port's, else the system's key window), after
    // the dispatch that asked: that window is busy with the key until then, and an update of it
    // now does nothing (B72's second passphrase field was never reached).
    cx.on_action(|_: &FocusNext, cx| {
        if let Some(front) = crate::widgets::native::keys_window(cx) {
            cx.defer(move |cx| {
                let _ = front.update(cx, |_, window, cx| window.focus_next(cx));
            });
        }
    });
    cx.on_action(|_: &FocusPrevious, cx| {
        if let Some(front) = crate::widgets::native::keys_window(cx) {
            cx.defer(move |cx| {
                let _ = front.update(cx, |_, window, cx| window.focus_prev(cx));
            });
        }
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|page: &OpenPage, cx| cx.open_url(&page.address));
    cx.on_action(|guide: &OpenGuide, cx| {
        let home = edm_shell::settings::home();
        match edm_desktop_base::help::write_page(&home, &guide.page) {
            Ok(path) => cx.open_url(&format!("file://{}", path.to_string_lossy())),
            Err(why) => edm_core::note!(warn, "{why}"),
        }
    });
    // ⌘W in a dialog: its own close (the main window hears it first).
    cx.on_action(|_: &CloseWindow, cx| crate::widgets::native::close_front(cx));
    // The Window menu, on the window in front.
    cx.on_action(|_: &Minimize, cx| {
        if let Some(front) = cx.active_window() {
            cx.defer(move |cx| {
                let _ = front.update(cx, |_, window, _| window.minimize_window());
            });
        }
    });
    cx.on_action(|_: &Zoom, cx| {
        if let Some(front) = cx.active_window() {
            cx.defer(move |cx| {
                let _ = front.update(cx, |_, window, _| window.zoom_window());
            });
        }
    });
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    // Read from the settings file as saved, not the store of secrets: the
    // folder is all it needs.
    cx.on_action(|_: &OpenDownloadFolder, _| {
        let settings =
            edm_shell::settings::Settings::read_only(&edm_shell::settings::Settings::path());
        let folder = settings.folders.download_dir.display().to_string();
        if let Err(e) = edm_shell::shell::open_file(&folder) {
            edm_core::note!(warn, "EDM: the download folder could not be opened: {e}");
        }
    });
}

fn item(command: Command) -> MenuItem {
    if command == Command::Quit {
        return MenuItem::action(t(command.label()), Quit);
    }
    let off = OFF.with(|off| off.borrow().contains(&command));
    MenuItem::action(shown(command), Run(command)).disabled(!carried_out(command) || off)
}

/// Options in the app menu: "Settings…" on macOS, as every Mac program names it there (with
/// ⌘, beside it), the window itself still called Options; "Options…" elsewhere.
fn settings_item() -> MenuItem {
    if cfg!(target_os = "macos") {
        MenuItem::action(format!("{}…", t("Settings")), Run(Command::Options))
    } else {
        item(Command::Options)
    }
}

/// A command's words on its menu item: with "…" where it opens a dialog
/// that asks for more first, as macOS's own menus say it.
fn shown(command: Command) -> String {
    let title = t(command.label());
    if command.asks_more() && !title.ends_with('…') {
        format!("{title}…")
    } else {
        title.to_string()
    }
}

/// Downloads › Speed limiter's items, top to bottom (`None` a separator),
/// each with whether it is ticked at a limit of `speed_limit` KB/s: Limit
/// speed while a limit is in force, and the preset that is, as the tray
/// ticks them. The status bar's Speed limiter pops up the same (gaps row
/// U29).
pub(crate) fn speed_limiter(speed_limit: u64) -> Vec<Option<(Command, bool)>> {
    use edm_desktop_base::tray::{ticked, PRESETS};
    let mut choices = vec![Some((Command::SpeedLimiterToggle, speed_limit > 0)), None];
    choices.extend(
        PRESETS
            .into_iter()
            .map(|preset| Some((preset, ticked(preset, speed_limit)))),
    );
    choices.extend([None, Some((Command::SpeedLimiterSettings, false))]);
    choices
}

/// Downloads › Speed limiter's items as a pop-up of the system's own
/// (`NativeMenu`), for the status bar's Speed limiter: the same words and
/// ticks, each running its command as the menu bar's does.
pub(crate) fn speed_limiter_popup(speed_limit: u64) -> gpui_component::native_menu::NativeMenu {
    speed_limiter(speed_limit).into_iter().fold(
        gpui_component::native_menu::NativeMenu::new(),
        |menu, choice| match choice {
            Some((command, ticked)) => {
                menu.menu_with_check(shown(command), ticked, Box::new(Run(command)))
            }
            None => menu.separator(),
        },
    )
}

fn items(commands: &[Command]) -> Vec<MenuItem> {
    commands.iter().copied().map(item).collect()
}

fn menu(title: &'static str, groups: &[&[Command]]) -> Menu {
    let mut all = Vec::new();
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            all.push(MenuItem::separator());
        }
        all.extend(items(group));
    }
    Menu::new(t(title)).items(all)
}

/// The menu bar.
pub fn set(cx: &mut App, ticks: &Ticks) {
    cx.set_menus(build(ticks));
    // The Dock icon's menu on macOS, as Mail's and Music's have one: what is wanted without
    // the window in front (PLATFORM_GAP_ANALYSIS item 28). Windows' Jump List is EDM's own
    // (`jumplist.rs`), so not there.
    if cfg!(target_os = "macos") {
        cx.set_dock_menu(dock_menu());
    }
}

/// The Dock icon's menu: add a download, resume or stop them all, open the
/// download folder.
fn dock_menu() -> Vec<MenuItem> {
    let mut items = items(&crate::window::reach::DOCK);
    items.extend([
        MenuItem::separator(),
        MenuItem::action(t("Open download folder"), OpenDownloadFolder),
    ]);
    items
}

/// The window's menus' titles, in English, in their order: what the top bar
/// shows, without building the menus themselves (`window_menus`, built on a
/// click).
pub const WINDOW_MENUS: [&str; 5] = ["Tasks", "File", "Downloads", "View", "Help"];

/// Every menu of the menu bar, by name.
struct Bar {
    app: Menu,
    tasks: Menu,
    file: Menu,
    edit: Menu,
    downloads: Menu,
    view: Menu,
    window: Menu,
    help: Menu,
}

/// The menus in the window's own top bar, on every system, as a code
/// editor's title bar has them: Tasks (Exit
/// last), File, Downloads (Options last), View and Help (About last), with
/// the menu bar's items and ticks. On macOS the system's menu bar holds
/// them too, beside EDM's own app menu, Edit and Window.
pub fn window_menus(ticks: &Ticks) -> Vec<Menu> {
    window_menus_on(ticks, crate::platform::ON_A_PHONE)
}

/// [`window_menus`] as a phone (`phone`) or a computer draws them: a phone's leave out what
/// `reach::on_phone` says it has not.
pub(crate) fn window_menus_on(ticks: &Ticks, phone: bool) -> Vec<Menu> {
    use Command as C;
    let Bar {
        mut tasks,
        file,
        mut downloads,
        view,
        mut help,
        ..
    } = bar(ticks, phone);
    tasks.items.extend([MenuItem::separator(), item(C::Quit)]);
    downloads
        .items
        .extend([MenuItem::separator(), item(C::Options)]);
    help.items.push(item(C::About));
    vec![tasks, file, downloads, view, help]
}

/// The menu bar's menus as a computer has them, then the Dock's menu, on any system: what the
/// list of actions is checked against (`window_tests/reach.rs`).
#[cfg(test)]
pub(crate) fn bar_and_dock(ticks: &Ticks) -> Vec<Menu> {
    let mut menus = window_menus_on(ticks, false);
    let Bar {
        mut app,
        edit,
        window,
        ..
    } = bar(ticks, false);
    app.items.extend(dock_menu());
    menus.extend([app, edit, window]);
    menus
}

/// The menu bar's menus, in their order.
fn build(ticks: &Ticks) -> Vec<Menu> {
    let Bar {
        app,
        tasks,
        file,
        edit,
        downloads,
        view,
        window,
        help,
    } = bar(ticks, crate::platform::ON_A_PHONE);
    vec![app, tasks, file, edit, downloads, view, window, help]
}

/// Each of the menu bar's menus.
fn bar(ticks: &Ticks, phone: bool) -> Bar {
    OFF.with(|off| off.borrow_mut().clone_from(&ticks.off));
    use Command as C;
    let app = Menu::new("EDM").items([
        item(C::About),
        MenuItem::separator(),
        settings_item(),
        MenuItem::separator(),
        MenuItem::os_submenu(t("Services"), SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action(t("Hide EDM"), Hide),
        MenuItem::action(t("Hide Others"), HideOthers),
        MenuItem::action(t("Show All"), ShowAll),
        MenuItem::separator(),
        item(C::Quit),
    ]);
    let tasks = menu("Tasks", crate::window::reach::TASKS);
    let file = file_menu(ticks);
    let downloads = downloads_menu(ticks);
    let view = view_menu(ticks, phone);
    let help = help_menu(phone);
    // Edit, as every macOS program has it: the text fields' own Undo, Redo,
    // Cut, Copy and Paste (the system's actions, so its services and
    // dictation work too). The list's Select all stays in File.
    let edit = Menu::new(t("Edit")).items([
        MenuItem::os_action(t("Undo"), gpui_base::input::Undo, OsAction::Undo),
        MenuItem::os_action(t("Redo"), gpui_base::input::Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action(t("Cut"), gpui_base::input::Cut, OsAction::Cut),
        MenuItem::os_action(t("Copy"), gpui_base::input::Copy, OsAction::Copy),
        MenuItem::os_action(t("Paste"), gpui_base::input::Paste, OsAction::Paste),
    ]);
    // Window, as every macOS program has it.
    let window = Menu::new(t("Window")).items([
        MenuItem::action(t("Minimize"), Minimize),
        MenuItem::action(t("Zoom"), Zoom),
        MenuItem::separator(),
        MenuItem::action(t("Bring All to Front"), BringAllToFront),
    ]);
    Bar {
        app,
        tasks,
        file,
        edit,
        downloads,
        view,
        window,
        help,
    }
}

#[cfg(test)]
mod tests;
