// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The shell's answer to the control port: the window's methods through
//! `dm_desktop_base::probe::WindowProbe`, the rest through the application
//! (`dm_shell::app::control_api`), as the egui window answers them. What the
//! shell does not draw yet it says so, by name, rather than answering as if
//! it did.

use dm_desktop_base::probe::{self, WindowProbe};
use gpui::{
    point, Bounds, Context, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, ScrollDelta, ScrollEvent, Window,
};
use serde_json::{json, Value};

use crate::list::{line, Downloads, ROW_HEIGHT};
use crate::screen::Drawn;
use crate::zoom::px;

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
        crate::syswindow::picture(window, &path)?;
        return Ok(serde_json::json!({ "path": path.display().to_string() }));
    }
    if let Some(answer) = probe::input(&mut Probe { view, window, cx }, method, params) {
        return answer;
    }
    answer_inside(view, method, params, window, cx)
}

/// What a `batch` reaches as well: the view and the application, never the
/// window's input, as with the egui window.
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
                // Read at once, as the egui window reads after an add: a batch's next step
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
            Ok(json!({ "speed_limit_kb": now?.speed_limit }))
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

/// A key's name as the control port gives it (egui's), as GPUI names it.
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
        let dialogs: Vec<gpui::AnyWindowHandle> = crate::screen::Screens::all(self.cx)
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
                    crate::native::keys_to(handle, || send(window, cx))
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
        for (handle, screen, dialog) in crate::screen::Screens::all(self.cx) {
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
        let state = self.view.scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let offset = state.base_handle.offset();
        let (top, left) = (f32::from(bounds.origin.y), f32::from(bounds.origin.x));
        let (height, width) = (f32::from(bounds.size.height), f32::from(bounds.size.width));
        let scrolled = -f32::from(offset.y);
        // A row's height on screen, at the text size in force.
        let row = crate::zoom::of(ROW_HEIGHT);
        let first = (scrolled / row).floor().max(0.0) as usize;
        let count = (height / row).ceil() as usize + 1;
        (first..(first + count).min(self.view.shown.len()))
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
            for item in &menu.items {
                match item {
                    gpui::OwnedMenuItem::Submenu(sub) if same(&sub.name) => {
                        let sub = sub.clone();
                        OPEN_MENU.with(|m| *m.borrow_mut() = Some(sub));
                        return Some(clicked());
                    }
                    gpui::OwnedMenuItem::Action { name, action, .. } if same(name) => {
                        self.view.scripted.acted();
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
        let title = crate::menus::WINDOW_MENUS
            .iter()
            .position(|name| same(dm_desktop_base::i18n::t(name)));
        let in_a_dialog = || {
            shown
                .iter()
                .any(|(d, _, dialog)| *dialog && d.text.to_lowercase() == lower)
        };
        if let Some(index) = title.filter(|_| !in_a_dialog()) {
            let ticks = self.view.menu_ticks.clone().unwrap_or_default();
            if let Some(menu) = crate::menus::window_menus(&ticks).into_iter().nth(index) {
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
        crate::screen::Screens::all(self.cx)
            .into_iter()
            .rev()
            .filter(|(_, _, dialog)| *dialog)
            .filter_map(|(handle, _, _)| {
                crate::screen::Screens::name_of(handle, self.cx)
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
                crate::screen::Screens::title_of(handle, self.cx).map(|title| title.to_string())
            })
            .or_else(|| {
                crate::screen::Screens::name_of(handle, self.cx).map(|name| name.to_string())
            })
    }
}

impl WindowProbe for Probe<'_, '_> {
    fn may_step(&mut self) -> Result<(), String> {
        // A click or a key on a window parked in the tray would act on what no one sees, as
        // the egui window refused it.
        if self.view.hidden {
            return Err(
                "dm's window is hidden in the tray: launch dm again, or choose Show in the tray, first"
                    .into(),
            );
        }
        Ok(())
    }

    fn has_window(&self) -> bool {
        true
    }

    fn texts(&mut self) -> Result<probe::Texts, String> {
        // A menu the port opened first, as its list is in front of
        // everything while it is open: each item's words (the menu bar
        // is the system's, so where they are drawn is not dm's to say).
        let menu: Vec<probe::Text> = OPEN_MENU.with(|m| {
            m.borrow()
                .iter()
                .flat_map(|menu| &menu.items)
                .filter_map(|item| match item {
                    gpui::OwnedMenuItem::Action { name, .. } => Some(name.to_string()),
                    gpui::OwnedMenuItem::Submenu(sub) => Some(sub.name.to_string()),
                    _ => None,
                })
                .map(|text| probe::Text {
                    text,
                    rect: [0.0; 4],
                    window: probe::Window::Main,
                })
                .collect()
        });
        // Then the dialogs, then the main window, each the reverse of the
        // order laid out: nearest the front first.
        Ok(probe::Texts::new(
            menu.into_iter()
                .chain(
                    self.everything()
                        .into_iter()
                        .map(|(d, _, dialog)| probe::Text {
                            text: d.text.to_string(),
                            rect: rect_of(d.bounds),
                            window: if dialog {
                                probe::Window::Dialog
                            } else {
                                probe::Window::Main
                            },
                        }),
                )
                .collect(),
        ))
    }

    fn click(&mut self, click: probe::Click) -> Result<probe::Clicked, String> {
        // One of the choices of a pop-up the port clicked a moment ago.
        if let Some(held) = crate::popup::held() {
            let lower = click.text.to_lowercase();
            match held
                .choices
                .iter()
                .position(|c| c.as_ref() == click.text || c.to_lowercase() == lower)
            {
                Some(index) => {
                    self.view.scripted.acted();
                    let pick = crate::popup::Pick {
                        field: held.field,
                        item: held.item,
                        index,
                    };
                    self.cx.defer(move |cx| {
                        let _ = held.window.update(cx, |_, window, cx| {
                            window.dispatch_action(Box::new(pick), cx)
                        });
                    });
                    self.cx.notify();
                    return Ok(probe::Clicked {
                        clicked: click.text,
                        at: [0.0, 0.0],
                    });
                }
                None => crate::popup::hold(held),
            }
        }
        // An item of a menu the port opened, or a menu's title where nothing
        // on screen says these words exactly: the system's menu bar, walked
        // as a person walks it.
        if let Some(walked) = self.walk_menus(&click.text) {
            return walked;
        }
        let (target, in_window) = self.find_in(&click.text, click.nth)?;
        if probe::is_run_a_file(&target.text) {
            return Err(format!("{} is done by a person", target.text));
        }
        // The moment a click is given to the window is a script's.
        self.view.scripted.acted();
        let at = target.bounds.center();
        let button = match click.button {
            probe::Button::Primary => MouseButton::Left,
            probe::Button::Secondary => MouseButton::Right,
        };
        let clicks = if click.double { 2 } else { 1 };
        // After this update, through the input path of the window the words
        // are in, as a pointer's: moved there, pressed, released (twice for a
        // double).
        self.cx.defer(move |cx| {
            let _ = in_window.update(cx, |_, window, cx| {
                crate::popup::scripted(|| {
                    let none = Modifiers::default();
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: at,
                            pressed_button: None,
                            modifiers: none,
                        }),
                        cx,
                    );
                    for count in 1..=clicks {
                        window.dispatch_event(
                            PlatformInput::MouseDown(MouseDownEvent {
                                button,
                                position: at,
                                modifiers: none,
                                click_count: count,
                                first_mouse: false,
                            }),
                            cx,
                        );
                        window.dispatch_event(
                            PlatformInput::MouseUp(MouseUpEvent {
                                button,
                                position: at,
                                modifiers: none,
                                click_count: count,
                            }),
                            cx,
                        );
                    }
                })
            });
        });
        self.cx.notify();
        Ok(probe::Clicked {
            clicked: target.text.to_string(),
            at: [f32::from(at.x), f32::from(at.y)],
        })
    }

    fn scroll(&mut self, scroll: probe::Scroll) -> Result<probe::Scrolled, String> {
        let (over, in_window) = self
            .find_in(&scroll.text, 0)
            .map_err(|_| format!("nothing on screen says {:?}", scroll.text))?;
        let at = over.bounds.center();
        self.view.scripted.acted();
        // A line is 40 points, as the egui window turns a wheel's; down is
        // a negative delta, as the system gives it. In the window the words
        // are in: a dialog's page scrolls in its own.
        let delta =
            ScrollDelta::Pixels(point(px(-scroll.sideways * 40.0), px(-scroll.lines * 40.0)));
        self.cx.defer(move |cx| {
            let _ = in_window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position: at,
                        delta,
                        ..Default::default()
                    }),
                    cx,
                );
            });
        });
        self.cx.notify();
        Ok(probe::Scrolled {
            scrolled: scroll.text,
            lines: scroll.lines,
        })
    }

    fn type_text(&mut self, text: &str) -> Result<usize, String> {
        // Each character as a key that types it, into the field with the
        // keyboard in the window in front: through the window's own text
        // input, as the system sends a person's typing.
        let keystrokes: Vec<Keystroke> = text
            .chars()
            .map(|c| Keystroke {
                modifiers: Modifiers::default(),
                key: c.to_lowercase().to_string(),
                key_char: Some(c.to_string()),
            })
            .collect();
        let typed = keystrokes.len();
        self.view.scripted.acted();
        self.in_front(move |window, cx| {
            for keystroke in &keystrokes {
                window.dispatch_keystroke(keystroke.clone(), cx);
            }
        });
        Ok(typed)
    }

    fn key(&mut self, press: probe::KeyPress) -> Result<(), String> {
        let key = key_name(&press.key).ok_or_else(|| format!("no key is called {}", press.key))?;
        // Escape closes a menu the port opened, as it closes an open menu,
        // and goes no further.
        if key == "escape" && OPEN_MENU.with(|m| m.borrow_mut().take()).is_some() {
            return Ok(());
        }
        let m = press.modifiers;
        let keystroke = Keystroke {
            modifiers: Modifiers {
                control: m.ctrl,
                alt: m.alt,
                shift: m.shift,
                platform: m.cmd,
                ..Default::default()
            },
            key,
            key_char: None,
        };
        self.view.scripted.acted();
        self.in_front(move |window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
        Ok(())
    }

    fn accessibility(&mut self) -> Result<Value, String> {
        dm_desktop_base::a11y_tree::windows()
    }

    fn windows(&mut self) -> Result<probe::Windows, String> {
        // Each window's frame on the screen (points, from the top left), and
        // its inside: the frame less its title bar.
        let frames = |window: &Window| {
            // Windows says its frame itself (`syswindow::frames`).
            if let Some((outer, inner)) = crate::syswindow::frames(window) {
                return (rect_of(outer), rect_of(inner));
            }
            let outer = window.bounds();
            let inside = window.viewport_size();
            let top = outer.size.height - inside.height;
            let inner = Bounds::new(point(outer.origin.x, outer.origin.y + top), inside);
            (rect_of(outer), rect_of(inner))
        };
        let main = Some(frames(self.window).0);
        let pixels_per_point = Some(self.window.scale_factor());
        let mut dialogs = Vec::new();
        // The oldest first, as they were opened.
        for (handle, _, dialog) in crate::screen::Screens::all(self.cx).into_iter().rev() {
            if !dialog {
                continue;
            }
            let title = self.title_of(handle).unwrap_or_default();
            let _ = handle.update(self.cx, |_, window, _| {
                let (outer, inner) = frames(window);
                dialogs.push(probe::DialogWindow {
                    title,
                    outer: Some(outer),
                    inner: Some(inner),
                });
            });
        }
        Ok(probe::Windows {
            pixels_per_point,
            main,
            dialogs,
        })
    }

    fn window_to(&mut self, to: probe::WindowTo) -> Result<(), String> {
        match to {
            // Brought in front, dm with it, as a click on its Dock icon does.
            probe::WindowTo::Shown => {
                self.view.show_main(self.cx);
                Ok(())
            }
            probe::WindowTo::Minimized => {
                self.window
                    .defer(self.cx, |window, _| window.minimize_window());
                Ok(())
            }
            // As the close button does where Options says to keep dm in the
            // tray; refused where there is no tray to keep it in.
            probe::WindowTo::Tray => {
                if self.view.tray.is_none() {
                    return Err("there is no tray to keep dm in here".into());
                }
                self.view.hide_window(self.window);
                Ok(())
            }
        }
    }

    fn move_window(&mut self, to: probe::MoveWindow) -> Result<probe::Moved, String> {
        // The dialog's window by its title, put where a drag by its title
        // bar would put it.
        let wanted = to.title.to_lowercase();
        let found = crate::screen::Screens::all(self.cx)
            .into_iter()
            .filter(|(_, _, dialog)| *dialog)
            .find_map(|(handle, _, _)| {
                self.title_of(handle)
                    .filter(|title| title.to_lowercase().contains(&wanted))
                    .map(|title| (handle, title))
            });
        let (handle, title) = found.ok_or_else(|| format!("no dialog is titled {:?}", to.title))?;
        let [x, y] = to.to;
        handle
            .update(self.cx, |_, window, _| {
                crate::syswindow::move_to(window, x, y)
            })
            .map_err(|e| e.to_string())??;
        Ok(probe::Moved {
            moved: title,
            to: to.to,
        })
    }

    fn select(&mut self, ids: Vec<u64>) -> Result<Vec<u64>, String> {
        let listed: std::collections::HashSet<u64> = self.view.rows.iter().map(|r| r.id).collect();
        self.view.selected = ids.into_iter().filter(|id| listed.contains(id)).collect();
        self.cx.notify();
        Ok(self.view.selected.iter().copied().collect())
    }

    fn place(&mut self, place: &str) -> Result<(), String> {
        let settings = self.view.backend.settings();
        let customs: Vec<String> = settings
            .custom_categories
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let place =
            dm_shell::app::places::Place::from_word(place, &self.view.backend.queues(), &customs)?;
        self.view.choose(place);
        self.cx.notify();
        Ok(())
    }

    fn inspect(&mut self) -> Result<probe::Inspect, String> {
        let shown: std::collections::HashMap<usize, (f32, f32, f32)> = self
            .on_screen()
            .into_iter()
            .map(|(i, y, left, width)| (i, (y, left, width)))
            .collect();
        let rows = self
            .view
            .shown
            .iter()
            .filter_map(|&r| self.view.rows.get(r))
            .enumerate()
            .map(|(i, row)| {
                // As the list shows the row, its status in English, as the
                // egui window reports it.
                let name = match self.view.shown_names.get(&row.id) {
                    Some(name) => name.clone(),
                    None => line(row).0.to_string(),
                };
                probe::InspectRow {
                    id: row.id,
                    name,
                    status: dm_desktop_base::rows::status_in_english(row),
                    queue: row.queue.clone(),
                    description: row.description.clone(),
                    url: dm_desktop_base::format::shown_url(&row.url),
                    rect: shown.get(&i).map(|(y, left, width)| {
                        [*left, *y, left + width, y + crate::zoom::of(ROW_HEIGHT)]
                    }),
                }
            })
            .collect();
        Ok(probe::Inspect {
            place: format!("{:?}", self.view.place),
            selected: self.view.selected.iter().copied().collect(),
            sort: self.view.sort.map(|(c, up)| probe::Sort {
                column: c.english().to_string(),
                ascending: up,
            }),
            categories: self.view.show_sidebar,
            toolbar: self.view.show_toolbar,
            dark: matches!(
                self.window.appearance(),
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
            ),
            hidden: self.view.hidden,
            said: self
                .view
                .said
                .as_ref()
                .map(|(said, _)| said.clone())
                .unwrap_or_default(),
            problems: self.view.problems.clone(),
            // The dialogs' windows open, by title, the oldest first.
            dialogs: self.dialog_titles(),
            rows,
        })
    }

    fn commands(&mut self) -> Vec<probe::CommandState> {
        dm_shell::app::commands::Command::ALL
            .iter()
            .map(|c| probe::CommandState {
                id: c.id(),
                label: c.label(),
                enabled: self.view.enabled(*c),
            })
            .collect()
    }

    fn run_command(&mut self, id: &str) -> Result<probe::Ran, String> {
        let command = dm_shell::app::commands::Command::from_id(id)
            .ok_or_else(|| format!("no command is called {id}"))?;
        // Opening a downloaded file is a person's click in the window.
        if let Some(why) = dm_shell::app::control_api::person_only(command.id()) {
            return Err(why);
        }
        if !dm_shell::app::commands::Command::ALL.contains(&command) || !self.view.enabled(command)
        {
            return Err(if crate::menus::carried_out(command) {
                format!("{id} has nothing to act on")
            } else {
                format!("{id} is not in the GPUI shell yet")
            });
        }
        use dm_shell::app::commands::Command;
        match command {
            // The search box is the window's: these need it, as its action handler has it
            // (`list.rs`, `menus::Run`), where `run` alone left them undone and the port said
            // they ran.
            Command::Find => self.view.find(self.window, self.cx),
            Command::FindNext | Command::FindPrevious => {
                self.view.find_step(command == Command::FindNext);
                self.cx.notify();
            }
            // Exit asks a person, but nothing over the port can answer: a script's exits at
            // once (docs/decisions.md, "Exit asks; scripts do not"), as the egui window's did.
            Command::Quit => self.view.quit(self.cx),
            _ => self.view.run(command, self.cx),
        }
        Ok(probe::Ran {
            ran: id.to_string(),
            said: self
                .view
                .said
                .as_ref()
                .map(|(said, _)| said.clone())
                .unwrap_or_default(),
        })
    }
}
