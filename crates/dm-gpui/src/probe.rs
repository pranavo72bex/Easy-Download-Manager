// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The shell's answer to the control port: the window's methods through
//! `dm_desktop_base::probe::WindowProbe`, the rest through the application
//! (`dm_shell::app::control_api`). What the shell does not draw yet it says
//! so, by name, rather than answering as if it did.

use dm_desktop_base::probe::{self, WindowProbe};
use gpui::{
    point, Bounds, Context, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, ScrollDelta, ScrollEvent, Window,
};
use serde_json::{json, Value};

use crate::window::list::{line, Downloads, ROW_HEIGHT};
use crate::window::screen::Drawn;
use crate::window::zoom::px;

mod window_probe;

/// Answers one request on the window's thread.
pub fn answer(
    view: &mut Downloads,
    method: &str,
    params: &Value,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> Result<Value, String> {
    // The main window as it stands on screen, pictured by the system.
    if method == "ui.screenshot" {
        let path = dm_shell::app::control_api::screenshot_path(&dm_shell::settings::home(), params);
        crate::platform::syswindow::picture(window, &path)?;
        return Ok(serde_json::json!({ "path": path.display().to_string() }));
    }
    // A menu item run by its path, as a person chooses it in the menu bar: on macOS the menus'
    // titles are the system's bar's, not the window's, and a script's click cannot reach them.
    if method == "ui.menu" {
        let path: Vec<String> = params
            .get("path")
            .and_then(Value::as_array)
            .map(|p| {
                p.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .filter(|p: &Vec<String>| p.len() >= 2)
            .ok_or("give `path`: a menu's name, then its items' words")?;
        let ticks = view.menus.menu_ticks.clone().unwrap_or_default();
        let action = crate::window::menus::action_at(&ticks, &path)
            .ok_or_else(|| format!("no item to choose at {path:?}"))?;
        window.dispatch_action(action, cx);
        return Ok(serde_json::json!({ "path": path }));
    }
    if let Some(answer) = probe::input(&mut Probe { view, window, cx }, method, params) {
        return answer;
    }
    answer_inside(view, method, params, window, cx)
}

/// What a `batch` reaches as well: the view and the application, never the
/// window's input.
fn answer_inside(
    view: &mut Downloads,
    method: &str,
    params: &Value,
    window: &mut Window,
    cx: &mut Context<Downloads>,
) -> Result<Value, String> {
    if let Some(answer) = probe::view(&mut Probe { view, window, cx }, method, params) {
        return answer;
    }
    if let Some(answer) =
        dm_shell::app::control_api::answer(&view.backend, &view.rows, method, params)
    {
        return answer;
    }
    match method {
        "downloads.add" => {
            let settings = view.backend.settings();
            let asked = dm_shell::app::control_api::add_asked(&view.rows, &settings, params)?;
            let added = if asked.fresh.is_empty() {
                0
            } else {
                let items = asked
                    .fresh
                    .iter()
                    .map(|url| (url.clone(), asked.with.clone()))
                    .collect();
                let added = view.session.add(&view.backend, items).outcome();
                // Read at once after an add: a batch's next step
                // (`ui.inspect`, a second add of the same address) sees the rows added.
                view.read();
                added?
            };
            Ok(json!({ "added": added, "already_listed": asked.listed }))
        }
        // An exact ceiling in KB/s; 0 is no limit. A script's change keeps
        // what makes dm run something (`Backend::set_settings_from`).
        "downloads.speed_limit" => {
            let (wanted, _) =
                dm_shell::app::control_api::speed_limit(&view.backend.settings(), params)?;
            let (now, _) = view.backend.set_settings_from(wanted, true);
            Ok(json!({ "speed_limit_kb": now?.advanced.speed_limit }))
        }
        "batch" => dm_shell::app::control_api::batch(params, |method, params| {
            answer_inside(view, method, params, window, cx)
        }),
        other => Err(format!("no method is called {other}")),
    }
}

/// A rectangle as the control port says one: left, top, right, bottom.
fn rect_of(b: Bounds<Pixels>) -> probe::Rect {
    [
        f32::from(b.origin.x),
        f32::from(b.origin.y),
        f32::from(b.origin.x + b.size.width),
        f32::from(b.origin.y + b.size.height),
    ]
}

/// A key's name as the control port gives it, as GPUI names it.
fn key_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    Some(
        match lower.as_str() {
            "escape" | "esc" => "escape",
            "enter" | "return" => "enter",
            "tab" => "tab",
            "space" => "space",
            "backspace" => "backspace",
            "delete" => "delete",
            "arrowup" | "up" => "up",
            "arrowdown" | "down" => "down",
            "arrowleft" | "left" => "left",
            "arrowright" | "right" => "right",
            "home" => "home",
            "end" => "end",
            "pageup" => "pageup",
            "pagedown" => "pagedown",
            // The keyboard's Menu (application) key.
            "menu" | "contextmenu" | "apps" => "menu",
            "comma" => ",",
            "period" => ".",
            other if other.chars().count() == 1 => return Some(other.to_string()),
            other if other.starts_with('f') && other[1..].parse::<u8>().is_ok() => {
                return Some(other.to_string())
            }
            _ => return None,
        }
        .to_string(),
    )
}

/// The shell as a `WindowProbe`.
struct Probe<'a, 'b> {
    view: &'a mut Downloads,
    window: &'a mut Window,
    cx: &'a mut Context<'b, Downloads>,
}

impl Probe<'_, '_> {
    /// The `nth` piece of text on screen that says `wanted`, nearest the
    /// front first: exactly, else ignoring case, else containing it.
    /// Runs `send` after this update in the window in front: a dialog's,
    /// where one is open (it stands in front of dm's window), else dm's.
    fn in_front(&mut self, send: impl FnOnce(&mut Window, &mut gpui::App) + 'static) {
        // The dialog the system has in front, else the newest.
        let dialogs: Vec<gpui::AnyWindowHandle> = crate::window::screen::Screens::all(self.cx)
            .into_iter()
            .filter(|(_, _, dialog)| *dialog)
            .map(|(handle, _, _)| handle)
            .collect();
        let front = self
            .cx
            .active_window()
            .filter(|active| dialogs.contains(active))
            .or_else(|| dialogs.first().copied());
        match front {
            Some(handle) => self.cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    crate::widgets::native::keys_to(handle, || send(window, cx))
                });
            }),
            None => self.window.defer(self.cx, send),
        }
        self.cx.notify();
    }

    /// Every word on screen in every window, nearest the front first (a
    /// dialog's before the main window's), with its window and whether it
    /// is a dialog's.
    fn everything(&self) -> Vec<(Drawn, gpui::AnyWindowHandle, bool)> {
        let mut all = Vec::new();
        for (handle, screen, dialog) in crate::window::screen::Screens::all(self.cx) {
            all.extend(
                screen
                    .drawn()
                    .into_iter()
                    .rev()
                    .map(|d| (d, handle, dialog)),
            );
        }
        all
    }

    /// Each open dialog that has not drawn a word yet, drawn now. A dialog's words are known
    /// once its window has painted, and a window the system has just made on a busy computer
    /// may wait seconds for its first frame: a script's click on "Download again" or "Keep it"
    /// then found nothing for the whole of its tries though the dialog was open (human-qa B73
    /// and B38, 2026-11-26). After a click that found nothing, so the next try finds them.
    fn draw_unpainted_dialogs(&mut self) {
        let unpainted: Vec<gpui::AnyWindowHandle> = crate::window::screen::Screens::all(self.cx)
            .into_iter()
            .filter(|(_, screen, dialog)| *dialog && screen.drawn().is_empty())
            .map(|(handle, _, _)| handle)
            .collect();
        if unpainted.is_empty() {
            return;
        }
        self.cx.defer(move |cx| {
            for handle in unpainted {
                let _ = handle.update(cx, |_, window, cx| {
                    window.draw(cx).clear(cx);
                    window.refresh();
                });
            }
        });
    }

    /// As [`Self::find`], with the window the words are in.
    fn find_in(&self, wanted: &str, nth: usize) -> Result<(Drawn, gpui::AnyWindowHandle), String> {
        let everything = self.everything();
        let drawn: Vec<Drawn> = everything.iter().map(|(d, _, _)| d.clone()).collect();
        let lower = wanted.to_lowercase();
        let tries: [&dyn Fn(&Drawn) -> bool; 3] = [
            &|d| d.text.as_ref() == wanted,
            &|d| d.text.to_lowercase() == lower,
            &|d| d.text.to_lowercase().contains(&lower),
        ];
        for matches in tries {
            let found: Vec<&Drawn> = drawn.iter().filter(|d| matches(d)).collect();
            if let Some(d) = found.get(nth) {
                let at = drawn
                    .iter()
                    .position(|x| std::ptr::eq(x, *d))
                    .and_then(|i| everything.get(i))
                    .map(|(_, w, _)| *w);
                if let Some(window) = at {
                    return Ok(((*d).clone(), window));
                }
            }
        }
        let near: Vec<&str> = drawn.iter().map(|d| d.text.as_ref()).take(40).collect();
        Err(format!(
            "nothing on screen says {wanted:?}; some of what does: {near:?}"
        ))
    }

    /// The rows on screen, with where each line is: (index, top, left, width).
    fn on_screen(&self) -> Vec<(usize, f32, f32, f32)> {
        let state = self.view.list.scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let offset = state.base_handle.offset();
        let (top, left) = (f32::from(bounds.origin.y), f32::from(bounds.origin.x));
        let (height, width) = (f32::from(bounds.size.height), f32::from(bounds.size.width));
        let scrolled = -f32::from(offset.y);
        // A row's height on screen, at the text size in force.
        let row = crate::window::zoom::of(ROW_HEIGHT);
        let first = (scrolled / row).floor().max(0.0) as usize;
        let count = (height / row).ceil() as usize + 1;
        (first..(first + count).min(self.view.list.shown.len()))
            .map(|i| (i, top + i as f32 * row - scrolled, left, width))
            .filter(|(_, y, _, _)| *y + row > top && *y < top + height)
            .collect()
    }
}

thread_local! {
    /// The menu the port opened, until an item of it is clicked.
    static OPEN_MENU: std::cell::RefCell<Option<gpui::OwnedMenu>> =
        const { std::cell::RefCell::new(None) };
}

/// Holds `menu` as the menu the port opened: a menu of the system's that the port's click
/// opened (the row menu), which waits for its next click, as a person's would.
pub(crate) fn hold_menu(menu: gpui::OwnedMenu) {
    OPEN_MENU.with(|m| *m.borrow_mut() = Some(menu));
}

/// The action of the item saying `words` in the menu the port opened, the menu taken: a
/// person's pick of it, which may be what runs a downloaded file, as the port's may not.
#[cfg(test)]
pub(crate) fn held_action(words: &str) -> Option<Box<dyn gpui::Action>> {
    let menu = OPEN_MENU.with(|m| m.borrow_mut().take())?;
    menu.items.iter().find_map(|item| match item {
        gpui::OwnedMenuItem::Action { name, action, .. }
            if name.trim_end_matches('…') == words.trim_end_matches('…') =>
        {
            Some(action.boxed_clone())
        }
        _ => None,
    })
}

impl Probe<'_, '_> {
    /// A click the menu bar takes: an item of the menu open (a submenu
    /// opens in its place; an action runs in the main window, as the
    /// system's menu sends it), or, where no words on screen are exactly
    /// these, a menu's title, which opens it. `None` where neither.
    fn walk_menus(&mut self, words: &str) -> Option<Result<probe::Clicked, String>> {
        let same = |name: &str| {
            name.trim_end_matches('…')
                .eq_ignore_ascii_case(words.trim_end_matches('…'))
        };
        let clicked = || {
            Ok(probe::Clicked {
                clicked: words.to_string(),
                at: [0.0, 0.0],
            })
        };
        if let Some(menu) = OPEN_MENU.with(|m| m.borrow_mut().take()) {
            // What runs a downloaded file is a person's to pick (the row menu's Open, Open
            // with…, Preview), asked for by a part of its words too; the menu stays open, as a
            // person's would.
            let part = words.to_lowercase();
            let runs_a_file = menu.items.iter().find_map(|item| match item {
                gpui::OwnedMenuItem::Action { name, .. }
                    if (same(name) || name.to_lowercase().contains(&part))
                        && probe::is_run_a_file(name) =>
                {
                    Some(name.clone())
                }
                _ => None,
            });
            if let Some(name) = runs_a_file {
                OPEN_MENU.with(|m| *m.borrow_mut() = Some(menu));
                return Some(Err(format!("{name} is done by a person")));
            }
            for item in &menu.items {
                match item {
                    gpui::OwnedMenuItem::Submenu(sub) if same(&sub.name) => {
                        let sub = sub.clone();
                        OPEN_MENU.with(|m| *m.borrow_mut() = Some(sub));
                        return Some(clicked());
                    }
                    gpui::OwnedMenuItem::Action { name, action, .. } if same(name) => {
                        self.view.port.scripted.acted();
                        let action = action.boxed_clone();
                        let main = self.view.main_window();
                        self.cx.defer(move |cx| {
                            let _ =
                                main.update(cx, |_, window, cx| window.dispatch_action(action, cx));
                        });
                        self.cx.notify();
                        return Some(clicked());
                    }
                    _ => {}
                }
            }
            // Not in it: the menu closes, as a click elsewhere closes it.
        }
        // A menu's title in the window's top bar: its menu opened here, as
        // the port reads it, rather than the system's, which it cannot. Not
        // where a dialog shows the same words (Options' Downloads tab).
        let lower = words.to_lowercase();
        let shown = self.everything();
        let title = crate::window::menus::WINDOW_MENUS
            .iter()
            .position(|name| same(dm_desktop_base::i18n::t(name)));
        let in_a_dialog = || {
            shown
                .iter()
                .any(|(d, _, dialog)| *dialog && d.text.to_lowercase() == lower)
        };
        if let Some(index) = title.filter(|_| !in_a_dialog()) {
            let ticks = self.view.menus.menu_ticks.clone().unwrap_or_default();
            if let Some(menu) = crate::window::menus::window_menus(&ticks)
                .into_iter()
                .nth(index)
            {
                OPEN_MENU.with(|m| *m.borrow_mut() = Some(menu.owned()));
                return Some(clicked());
            }
        }
        let on_screen = shown.iter().any(|(d, _, _)| d.text.to_lowercase() == lower);
        if on_screen {
            return None;
        }
        let menu = self
            .cx
            .get_menus()?
            .into_iter()
            .find(|menu| same(&menu.name))?;
        OPEN_MENU.with(|m| *m.borrow_mut() = Some(menu));
        Some(clicked())
    }

    /// The titles of the dialogs' windows open, the oldest first.
    fn dialog_titles(&mut self) -> Vec<String> {
        crate::window::screen::Screens::all(self.cx)
            .into_iter()
            .rev()
            .filter(|(_, _, dialog)| *dialog)
            .filter_map(|(handle, _, _)| {
                crate::window::screen::Screens::name_of(handle, self.cx)
                    .map(|name| name.to_string())
                    .or_else(|| self.title_of(handle))
            })
            .collect()
    }

    /// A window's title as it stands now (a progress window's starts with
    /// how far it is): what the system says, else what dm last set
    /// (`screen::set_title`; GPUI's Linux and Windows windows do not say
    /// theirs).
    fn title_of(&mut self, handle: gpui::AnyWindowHandle) -> Option<String> {
        handle
            .update(self.cx, |_, window, _| window.window_title())
            .ok()
            .filter(|title| !title.is_empty())
            .or_else(|| {
                crate::window::screen::Screens::title_of(handle, self.cx)
                    .map(|title| title.to_string())
            })
            .or_else(|| {
                crate::window::screen::Screens::name_of(handle, self.cx)
                    .map(|name| name.to_string())
            })
    }
}
