// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The harness's dialogs: the one in front, its words, a field revealed and filled, a menu item.

use super::*;

impl Harness {
    /// `act` on the view of the open dialog that is a `V` (Options, the Scheduler…), with its
    /// window: what a person sets by a drag, or reads off a control's look.
    pub(crate) fn dialog<V: 'static, R>(
        &mut self,
        act: impl FnOnce(&mut V, &mut gpui::Window, &mut gpui::Context<V>) -> R,
    ) -> R {
        let mut act = Some(act);
        let done = self.cx.update(|cx| {
            for handle in cx.windows() {
                let Some(root) = handle.downcast::<gpui_component::Root>() else {
                    continue;
                };
                let found = root.update(cx, |root, window, cx| {
                    let view = root.view().clone().downcast::<V>().ok()?;
                    let act = act.take()?;
                    Some(view.update(cx, |view, cx| act(view, window, cx)))
                });
                if let Ok(Some(done)) = found {
                    return Some(done);
                }
            }
            None
        });
        let done = done.unwrap_or_else(|| {
            panic!("no dialog of that kind is open; open: {:?}", self.dialogs())
        });
        self.step();
        done
    }

    /// The words of the dialog in front, where each was laid out, and its window.
    fn front_dialog(&mut self) -> (gpui::AnyWindowHandle, Vec<crate::window::screen::Drawn>) {
        self.cx.update(|cx| {
            let dialogs: Vec<_> = crate::window::screen::Screens::all(cx)
                .into_iter()
                .filter(|(_, _, dialog)| *dialog)
                .collect();
            let front = cx
                .active_window()
                .and_then(|active| dialogs.iter().find(|(w, _, _)| *w == active))
                .or(dialogs.first())
                .unwrap_or_else(|| panic!("no dialog is open"));
            (front.0, front.1.drawn())
        })
    }

    /// Where the words `words` are in the dialog in front, if they are on it.
    pub(crate) fn dialog_words(&mut self, words: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
        let (_, drawn) = self.front_dialog();
        drawn
            .iter()
            .find(|d| d.text.as_ref() == words)
            .map(|d| d.bounds)
    }

    /// Scrolls the page of the dialog in front, as a wheel over it does, until `words` are on
    /// it: down first, then back up.
    pub(crate) fn reveal(&mut self, words: &str) {
        self.reveal_after(None, words);
    }

    /// [`Harness::reveal`], for the words `words` laid out after the words `anchor` (the
    /// first, where there is no anchor): where the first such are, once on screen.
    fn reveal_after(&mut self, anchor: Option<&str>, words: &str) -> gpui::Bounds<gpui::Pixels> {
        let found = |drawn: &[crate::window::screen::Drawn]| {
            let from = match anchor {
                Some(anchor) => drawn.iter().position(|d| d.text.as_ref() == anchor)? + 1,
                None => 0,
            };
            drawn[from..]
                .iter()
                .find(|d| d.text.as_ref() == words)
                .map(|d| d.bounds)
        };
        for lines in [3.0, -3.0] {
            for _ in 0..60 {
                let (window, drawn) = self.front_dialog();
                if let Some(at) = found(&drawn) {
                    // Low in the page, the field under the words may be cut by its edge:
                    // brought up a little first, as a person would.
                    let height = self.cx.update(|cx| {
                        window
                            .update(cx, |_, window, _| window.viewport_size().height)
                            .unwrap()
                    });
                    if at.bottom() + gpui::px(120.0) < height {
                        return at;
                    }
                    self.wheel(window, at.center(), 2.0);
                    let (_, drawn) = self.front_dialog();
                    if let Some(at) = found(&drawn) {
                        return at;
                    }
                    continue;
                }
                // Over the middle of what the dialog shows: its page, not its buttons.
                let mut ys: Vec<_> = drawn.iter().map(|d| d.bounds.center()).collect();
                ys.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
                let Some(&at) = ys.get(ys.len() / 2) else {
                    break;
                };
                self.wheel(window, at, lines);
            }
        }
        panic!(
            "{words:?} (after {anchor:?}) is not in the dialog; on screen: {:?}",
            self.texts()
        );
    }

    /// Types `text` into the field the words `label` name (beside it or under it), in place of
    /// what it held, as a person clicks into it, selects all and types.
    pub(crate) fn fill(&mut self, label: &str, text: &str) {
        let at = self.reveal_after(None, label);
        self.type_at(at, label, text);
    }

    /// [`Harness::fill`], for the field of the first words `label` after the words
    /// `anchor`: a list's row, where each row's fields have the same names.
    pub(crate) fn fill_after(&mut self, anchor: &str, label: &str, text: &str) {
        let at = self.reveal_after(Some(anchor), label);
        self.type_at(at, label, text);
    }

    /// The menu item at `path` (`["Downloads", "Speed limiter", "Limit speed"]`) as the
    /// window's menus show it now: whether it is ticked, and whether greyed.
    pub(crate) fn menu_item(&mut self, path: &[&str]) -> (bool, bool) {
        let ticks = self.view(|view| view.menus.menu_ticks.clone().unwrap_or_default());
        let menus: Vec<gpui::OwnedMenu> = crate::window::menus::window_menus(&ticks)
            .into_iter()
            .map(|menu| menu.owned())
            .collect();
        let same =
            |name: &str, wanted: &str| name.trim_end_matches('…') == wanted.trim_end_matches('…');
        let (first, rest) = path.split_first().expect("a menu's name");
        let mut items = &menus
            .iter()
            .find(|menu| same(&menu.name, first))
            .unwrap_or_else(|| panic!("no menu {first:?}"))
            .items;
        for (i, name) in rest.iter().enumerate() {
            let last = i + 1 == rest.len();
            let found = items.iter().find_map(|item| match item {
                gpui::OwnedMenuItem::Submenu(sub) if !last && same(&sub.name, name) => {
                    Some(Err(&sub.items))
                }
                gpui::OwnedMenuItem::Action {
                    name: item,
                    checked,
                    disabled,
                    ..
                } if last && same(item, name) => Some(Ok((*checked, *disabled))),
                _ => None,
            });
            match found {
                Some(Ok(state)) => return state,
                Some(Err(sub)) => items = sub,
                None => panic!("no {name:?} in the menu at {path:?}"),
            }
        }
        panic!("{path:?} is a menu, not an item")
    }

    /// Types `text` into the field by the words `label` at `named`.
    fn type_at(&mut self, named: gpui::Bounds<gpui::Pixels>, label: &str, text: &str) {
        let (window, drawn) = self.front_dialog();
        // The field beside its words; else under them, or at the start of the line under them
        // where a row of fields wrapped.
        let page_left = drawn
            .iter()
            .map(|d| d.bounds.left())
            .fold(named.left(), |a, b| if b < a { b } else { a });
        let below = named.bottom() + gpui::px(14.0);
        let tries = [
            gpui::point(named.right() + gpui::px(16.0), named.center().y),
            gpui::point(named.left() + gpui::px(12.0), below),
            gpui::point(page_left + gpui::px(12.0), below),
        ];
        let in_a_field = |cx: &mut gpui::TestAppContext| {
            cx.update(|cx| {
                window
                    .update(cx, |_, window, _| {
                        window
                            .context_stack()
                            .iter()
                            .any(|context| context.contains("Input"))
                    })
                    .unwrap_or(false)
            })
        };
        let mut found = false;
        for at in tries {
            self.cx.update(|cx| {
                let _ = window.update(cx, |_, window, cx| window.blur(cx));
            });
            self.press_at(window, at, 1);
            if in_a_field(&mut self.cx) {
                found = true;
                break;
            }
        }
        assert!(found, "no field by {label:?} took the keyboard");
        let all = gpui::Keystroke {
            modifiers: gpui::Modifiers {
                platform: cfg!(target_os = "macos"),
                control: !cfg!(target_os = "macos"),
                ..Default::default()
            },
            key: "a".into(),
            key_char: None,
        };
        let backspace = gpui::Keystroke::parse("backspace").unwrap();
        let typed: Vec<gpui::Keystroke> = text
            .chars()
            .map(|c| gpui::Keystroke {
                modifiers: gpui::Modifiers::default(),
                key: c.to_lowercase().to_string(),
                key_char: Some(c.to_string()),
            })
            .collect();
        self.cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                crate::widgets::native::keys_to(window.window_handle(), || {
                    window.dispatch_keystroke(all, cx);
                    window.dispatch_keystroke(backspace, cx);
                    for key in typed {
                        window.dispatch_keystroke(key, cx);
                    }
                })
            });
        });
        self.step();
    }
}
