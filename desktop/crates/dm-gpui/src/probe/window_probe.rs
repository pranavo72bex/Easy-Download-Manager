// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's side of the control port: each of `WindowProbe`'s methods, answered from what is
//! drawn.

use super::*;

impl WindowProbe for Probe<'_, '_> {
    fn may_step(&mut self) -> Result<(), String> {
        // A click or a key on a window parked in the tray would act on what no one sees, so
        // it is refused.
        if self.view.system.hidden {
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
        if let Some(held) = crate::widgets::popup::held() {
            let lower = click.text.to_lowercase();
            match held
                .choices
                .iter()
                .position(|c| c.as_ref() == click.text || c.to_lowercase() == lower)
            {
                Some(index) => {
                    self.view.port.scripted.acted();
                    let pick = crate::widgets::popup::Pick {
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
                None => crate::widgets::popup::hold(held),
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
        self.view.port.scripted.acted();
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
                crate::widgets::popup::scripted(|| {
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
        self.view.port.scripted.acted();
        // A line is 40 points; down is a negative delta, as the system
        // gives it. In the window the words are in: a dialog's page scrolls
        // in its own.
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
        self.view.port.scripted.acted();
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
        self.view.port.scripted.acted();
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
            if let Some((outer, inner)) = crate::platform::syswindow::frames(window) {
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
        for (handle, _, dialog) in crate::window::screen::Screens::all(self.cx)
            .into_iter()
            .rev()
        {
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
                if self.view.system.tray.is_none() {
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
        let found = crate::window::screen::Screens::all(self.cx)
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
                crate::platform::syswindow::move_to(window, x, y).map_err(String::from)
            })
            .map_err(|e| e.to_string())??;
        Ok(probe::Moved {
            moved: title,
            to: to.to,
        })
    }

    fn select(&mut self, ids: Vec<u64>) -> Result<Vec<u64>, String> {
        let listed: std::collections::HashSet<u64> = self.view.rows.iter().map(|r| r.id).collect();
        self.view.list.selected = ids.into_iter().filter(|id| listed.contains(id)).collect();
        self.cx.notify();
        Ok(self.view.list.selected.iter().copied().collect())
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
            .list
            .shown
            .iter()
            .filter_map(|&r| self.view.rows.get(r))
            .enumerate()
            .map(|(i, row)| {
                // As the list shows the row, its status in English.
                let name = match self.view.known.shown_names.get(&row.id) {
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
                        [
                            *left,
                            *y,
                            left + width,
                            y + crate::window::zoom::of(ROW_HEIGHT),
                        ]
                    }),
                }
            })
            .collect();
        Ok(probe::Inspect {
            place: format!("{:?}", self.view.list.place),
            selected: self.view.list.selected.iter().copied().collect(),
            sort: self.view.list.sort.map(|(c, up)| probe::Sort {
                column: c.english().to_string(),
                ascending: up,
            }),
            categories: self.view.layout.show_sidebar,
            toolbar: self.view.layout.show_toolbar,
            dark: matches!(
                self.window.appearance(),
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
            ),
            hidden: self.view.system.hidden,
            said: self
                .view
                .status
                .said
                .as_ref()
                .map(|(said, _)| said.clone())
                .unwrap_or_default(),
            problems: self.view.status.problems.clone(),
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
            return Err(if crate::window::menus::carried_out(command) {
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
            // once (docs/decisions.md, "Exit asks; scripts do not").
            Command::Quit => self.view.quit(self.cx),
            _ => self.view.run(command, self.cx),
        }
        Ok(probe::Ran {
            ran: id.to_string(),
            said: self
                .view
                .status
                .said
                .as_ref()
                .map(|(said, _)| said.clone())
                .unwrap_or_default(),
        })
    }
}
