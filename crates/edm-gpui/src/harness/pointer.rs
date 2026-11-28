// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The harness's pointer: clicks as a person makes them, scrolls, drags and the window's size.

use super::*;

impl Harness {
    /// Clicks the words `text` as a right-click.
    pub(crate) fn right_click(&mut self, text: &str) {
        let asked = serde_json::json!({ "text": text, "button": "right" });
        if let Err(why) = self.call("ui.click", asked) {
            panic!("{text} could not be right-clicked: {why}");
        }
    }

    /// Clicks (`clicks` 2: double-clicks) the words `text` as a person's pointer does, not a
    /// script's: what runs a downloaded file takes a person's click alone.
    pub(crate) fn person_click(&mut self, text: &str, clicks: usize) {
        let (window, at) = self.drawn(text, 0);
        self.press_at(window, at.center(), clicks);
    }

    /// Picks `words` in the menu of the system's the port opened (the row menu), as a person's
    /// pick does: its action sent to the main window, not held against a script's moment, so
    /// Open and Open with… run as a person's choice of them does.
    pub(crate) fn pick_as_person(&mut self, words: &str) {
        let Some(action) = crate::probe::held_action(words) else {
            panic!("no menu the port opened says {words:?}: {:?}", self.texts());
        };
        self.view(|view| view.port.scripted = Default::default());
        let main = self.window;
        self.cx.update(|cx| {
            let _ = main.update(cx, |_, window, cx| window.dispatch_action(action, cx));
        });
        self.step();
    }

    /// Waits until a script's last act is long enough ago for a person's click to run a file.
    pub(crate) fn until_a_persons_moment(&mut self) {
        self.until_within(
            "the script's moment to pass",
            std::time::Duration::from_secs(10),
            &mut |h| !h.view(|view| view.port.scripted.by_script()),
        );
    }

    /// Where the `nth` piece of text saying `text` was drawn, nearest the front first, and in
    /// which window.
    pub(super) fn drawn(
        &mut self,
        text: &str,
        nth: usize,
    ) -> (gpui::AnyWindowHandle, gpui::Bounds<gpui::Pixels>) {
        self.step();
        let found = self.cx.update(|cx| {
            crate::window::screen::Screens::all(cx)
                .into_iter()
                .flat_map(|(handle, screen, _)| {
                    screen
                        .drawn()
                        .into_iter()
                        .rev()
                        .filter(|d| d.text.as_ref() == text)
                        .map(move |d| (handle, d.bounds))
                        .collect::<Vec<_>>()
                })
                .nth(nth)
        });
        found.unwrap_or_else(|| panic!("nothing on screen says {text:?}: {:?}", self.texts()))
    }

    /// A finger put down on the words `text`, as a phone's screen reports a touch's start
    /// (GPUI's `TouchEvent`, `Started`), and left there: the window and the place.
    pub(crate) fn finger_down(
        &mut self,
        text: &str,
    ) -> (gpui::AnyWindowHandle, gpui::Point<gpui::Pixels>) {
        let (window, bounds) = self.drawn(text, 0);
        let at = bounds.center();
        self.touch(window, at, gpui::TouchPhase::Started);
        (window, at)
    }

    /// The finger put down at `at` in `window` lifted there: GPUI makes it a tap.
    pub(crate) fn finger_up(
        &mut self,
        window: gpui::AnyWindowHandle,
        at: gpui::Point<gpui::Pixels>,
    ) {
        self.touch(window, at, gpui::TouchPhase::Ended);
    }

    /// What a finger is pressing in `window` as its pressed face shows it (`widgets::press`).
    pub(crate) fn pressed(
        &mut self,
        window: gpui::AnyWindowHandle,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.cx
            .update(|cx| {
                window.update(cx, |_, window, _| crate::widgets::press::pressed_in(window))
            })
            .ok()
            .flatten()
    }

    /// Touch 1 at `at` in `window`, in `phase`.
    fn touch(
        &mut self,
        window: gpui::AnyWindowHandle,
        at: gpui::Point<gpui::Pixels>,
        phase: gpui::TouchPhase,
    ) {
        // The touch lands now: the test clock brought to the present first, so what it starts
        // (a press's time limit, `widgets::press`) runs from this moment, not from the last
        // step's, however long a slow machine took since (Windows' CI, main's Desktop run).
        let now = std::time::Instant::now();
        self.cx.executor().advance_clock(now - self.clock);
        self.clock = now;
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::Touch(gpui::TouchEvent {
                        id: gpui::TouchId(1),
                        phase,
                        position: at,
                        predicted_position: None,
                        force: None,
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    /// A press and release of the pointer at `at` in `window`, `clicks` times.
    /// A finger held still on the words `text`, as the phone's gesture recogniser reports one
    /// (GPUI's `LongPressEvent`), in the main window.
    pub(crate) fn long_press(&mut self, text: &str) {
        let (window, bounds) = self.drawn(text, 0);
        let at = bounds.center();
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::LongPress(gpui::LongPressEvent {
                        phase: gpui::TouchPhase::Started,
                        start_position: at,
                        position: at,
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    pub(super) fn press_at(
        &mut self,
        window: gpui::AnyWindowHandle,
        at: gpui::Point<gpui::Pixels>,
        clicks: usize,
    ) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                let none = Modifiers::default();
                window.dispatch_event(
                    gpui::PlatformInput::MouseMove(MouseMoveEvent {
                        position: at,
                        pressed_button: None,
                        modifiers: none,
                    }),
                    cx,
                );
                for count in 1..=clicks {
                    window.dispatch_event(
                        gpui::PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position: at,
                            modifiers: none,
                            click_count: count,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        gpui::PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position: at,
                            modifiers: none,
                            click_count: count,
                        }),
                        cx,
                    );
                }
            });
        });
        self.step();
    }

    /// The settings changed by `alter`, through the window's own way in, as Options' OK
    /// puts them in force.
    pub(crate) fn apply(&mut self, alter: impl FnOnce(&mut edm_shell::Settings)) {
        let mut wanted = self.view(|view| view.backend.settings());
        alter(&mut wanted);
        if let Err(why) = self.update(|view, cx| view.apply_settings(wanted, cx)) {
            panic!("the settings were refused: {why}");
        }
    }

    /// Clicks the `nth` words `text` as a person's pointer does, where the port will not
    /// (Open, Open with…: what runs a downloaded file): pressed and let go over them, in the
    /// window they are in, nearest the front first.
    pub(crate) fn click_as_person(&mut self, text: &str, nth: usize) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
        let found = self.cx.update(|cx| {
            crate::window::screen::Screens::all(cx)
                .into_iter()
                .flat_map(|(handle, screen, _)| {
                    screen.drawn().into_iter().rev().map(move |d| (handle, d))
                })
                .filter(|(_, d)| d.text.as_ref() == text)
                .nth(nth)
        });
        let Some((window, drawn)) = found else {
            panic!("{text} ({nth}) is not on screen: {:?}", self.texts());
        };
        let at = drawn.bounds.center();
        // A person's, not the port's: what the port clicked a moment ago is not held against it.
        self.view(|view| view.port.scripted = Default::default());
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                let none = Modifiers::default();
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    /// Clicks the field beside the words `label` (its name, at its left), as a person clicks
    /// into it: the keyboard goes there.
    pub(crate) fn click_beside(&mut self, label: &str) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
        let found = self.cx.update(|cx| {
            crate::window::screen::Screens::all(cx)
                .into_iter()
                .flat_map(|(handle, screen, _)| {
                    screen.drawn().into_iter().rev().map(move |d| (handle, d))
                })
                .find(|(_, d)| d.text.as_ref() == label)
        });
        let Some((window, drawn)) = found else {
            panic!("{label} is not on screen: {:?}", self.texts());
        };
        let bounds = drawn.bounds;
        let at = gpui::point(
            bounds.origin.x + bounds.size.width + gpui::px(30.0),
            bounds.origin.y + bounds.size.height / 2.0,
        );
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                let none = Modifiers::default();
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: none,
                        click_count: 1,
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    /// Double-clicks the words `text`, as a person does.
    pub(crate) fn double_click(&mut self, text: &str) {
        let asked = serde_json::json!({ "text": text, "double": true });
        if let Err(why) = self.call("ui.click", asked) {
            panic!("{text} could not be double-clicked: {why}");
        }
    }

    /// Turns the wheel `lines` down (up where negative) with the pointer over the words `text`.
    pub(crate) fn scroll(&mut self, text: &str, lines: f32) {
        let asked = serde_json::json!({ "text": text, "lines": lines });
        if let Err(why) = self.call("ui.scroll", asked) {
            panic!("{text} could not be scrolled: {why}");
        }
    }

    /// Scrolls the dialog in front down, a few lines at a time with the pointer over its
    /// words, until `words` are in sight.
    pub(crate) fn scroll_to(&mut self, words: &str) {
        for _ in 0..60 {
            if self.shows(words) {
                return;
            }
            let texts = self.call("ui.texts", Value::Null).unwrap();
            let in_dialog: Vec<String> = texts["texts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| t["window"] == "dialog")
                .filter_map(|t| t["text"].as_str().map(str::to_string))
                .collect();
            // One from the middle: the page's, not its buttons' at the foot.
            let Some(over) = in_dialog.get(in_dialog.len() / 2).cloned() else {
                break;
            };
            self.scroll(&over, 3.0);
        }
        assert!(
            self.shows(words),
            "{words} never came in sight: {:?}",
            self.texts()
        );
    }

    /// The wheel turned `lines` down (up where negative) over `at` in `window`.
    pub(super) fn wheel(
        &mut self,
        window: gpui::AnyWindowHandle,
        at: gpui::Point<gpui::Pixels>,
        lines: f32,
    ) {
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::Scroll(gpui::ScrollEvent {
                        position: at,
                        delta: gpui::ScrollDelta::Pixels(gpui::point(
                            gpui::px(0.0),
                            gpui::px(-lines * 40.0),
                        )),
                        ..Default::default()
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    /// The wheel turned `points` sideways (to the right where positive) over the words `text`
    /// in the main window, as a trackpad's sideways swipe does.
    pub(crate) fn wheel_sideways(&mut self, text: &str, points: f32) {
        let at = self.bounds_of(text).center();
        let window = self.window;
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::Scroll(gpui::ScrollEvent {
                        position: at,
                        delta: gpui::ScrollDelta::Pixels(gpui::point(
                            gpui::px(-points),
                            gpui::px(0.0),
                        )),
                        ..Default::default()
                    }),
                    cx,
                );
            });
        });
        self.step();
    }

    /// Where the words `text` were drawn, nearest the front first, in whichever window.
    pub(crate) fn bounds_of(&mut self, text: &str) -> gpui::Bounds<gpui::Pixels> {
        self.drawn(text, 0).1
    }

    /// The main window made `width` by `height`, as a person's drag of its corner makes it.
    pub(crate) fn resize(&mut self, width: f32, height: f32) {
        let main = self.window;
        self.cx
            .update(|cx| {
                main.update(cx, |_, window, _| {
                    window.resize(gpui::size(gpui::px(width), gpui::px(height)))
                })
            })
            .unwrap();
        self.step();
    }

    /// The pointer pressed at `from` in the main window and moved to `to` in eight moves, held
    /// down, as a person drags; let go there where `release`. Left held, what is being
    /// dragged can be read (`cx.has_active_drag`) before [`Harness::release`].
    pub(crate) fn drag(
        &mut self,
        from: gpui::Point<gpui::Pixels>,
        to: gpui::Point<gpui::Pixels>,
        release: bool,
    ) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, PlatformInput};
        let main = self.window;
        let none = Modifiers::default();
        let send = |cx: &mut TestAppContext, event: PlatformInput| {
            cx.update(|cx| {
                let _ = main.update(cx, |_, window, cx| window.dispatch_event(event, cx));
            });
        };
        send(
            &mut self.cx,
            PlatformInput::MouseMove(MouseMoveEvent {
                position: from,
                pressed_button: None,
                modifiers: none,
            }),
        );
        send(
            &mut self.cx,
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers: none,
                click_count: 1,
                first_mouse: false,
            }),
        );
        self.step();
        for step in 1..=8 {
            let at = from + (to - from) * (step as f32 / 8.0);
            send(
                &mut self.cx,
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: at,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: none,
                }),
            );
            self.step();
        }
        if release {
            self.release(to);
        }
    }

    /// The pointer held down since [`Harness::drag`] let go at `at` in the main window.
    pub(crate) fn release(&mut self, at: gpui::Point<gpui::Pixels>) {
        let main = self.window;
        self.cx.update(|cx| {
            let _ = main.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                        button: gpui::MouseButton::Left,
                        position: at,
                        modifiers: gpui::Modifiers::default(),
                        click_count: 1,
                    }),
                    cx,
                )
            });
        });
        self.step();
    }

    /// The words `from` pressed, dragged onto the words `onto` in the same window, and let go
    /// there, as a person drags a row or a heading.
    pub(crate) fn drag_words(&mut self, from: &str, onto: &str) {
        use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
        let (window, start) = self.drawn(from, 0);
        let (_, end) = self.drawn(onto, 0);
        let (start, end) = (start.center(), end.center());
        let none = Modifiers::default();
        let send = |h: &mut Harness, event: gpui::PlatformInput| {
            h.cx.update(|cx| {
                let _ = window.update(cx, |_, window, cx| {
                    window.dispatch_event(event, cx);
                });
            });
            h.step();
        };
        send(
            self,
            gpui::PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: start,
                modifiers: none,
                click_count: 1,
                first_mouse: false,
            }),
        );
        // In steps, as a hand moves: the drag starts once the pointer has gone a few points.
        for k in 1..=4 {
            let at = start + (end - start) * (k as f32 / 4.0);
            send(
                self,
                gpui::PlatformInput::MouseMove(MouseMoveEvent {
                    position: at,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: none,
                }),
            );
        }
        send(
            self,
            gpui::PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: end,
                modifiers: none,
                click_count: 1,
            }),
        );
    }
}
