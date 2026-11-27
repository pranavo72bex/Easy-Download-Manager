// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window laid out for a phone's width (gaps row N7, the maintainer, 2026-11-20: "like we
//! minimize the desktop it becomes mobile view. so that we do not need separate UI"): below
//! [`NARROW_BELOW`] the menu bar and the toolbar become one top bar (the places' button, the
//! title, Add, and an overflow menu), the table a list of cards (`cards.rs`), the side panel a
//! drawer, and the menus a sheet drawn in the window (`sheet.rs`). The views and the actions
//! are the desktop's own: the same rows, the same `Command`s run the same way, the same menus
//! (`menus::window_menus`, `rowmenu::row_menu`) walked as a sheet. At the desktop's widths
//! nothing here is drawn.

use dm_core::Id;
use dm_shell::app::places::Place;
use gpui::{div, prelude::*, Context, Div, Stateful, Window};

use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod bottomnav;
pub(crate) mod cards;
pub(crate) mod sheet;

pub(crate) use cards::CARD_HEIGHT;
pub(crate) use sheet::Sheet;

/// The width, in logical points, below which the window is laid out as a phone's. Under the
/// least width the desktop window had before this layout (640 points: `main.rs`), so every
/// width a desktop window could have had is laid out as it was; and above the widest phones
/// held upright (about 430 points) and the small tablets' 600, where Android's own layouts
/// start to call a screen wide.
pub const NARROW_BELOW: f32 = 600.0;

/// The least width the main window may be made on the desktop: the narrowest phones held
/// upright (360 points), so narrowing the window reaches the phone's layout.
pub const LEAST_WIDTH: f32 = 360.0;

/// The floating Add URL button's size: Android's floating action button.
pub(super) const FLOATING: f32 = 56.0;

/// How long a press is held on a card before it is the card's menu, as a phone's long press.
const LONG_PRESS: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether a window `width` points wide is laid out as a phone's.
pub fn narrow_at(width: f32) -> bool {
    width < NARROW_BELOW
}

/// Whether `window` is laid out as a phone's: its width in points of dm's design, at the text
/// size in force. At 200% a window 600 points wide holds what 300 would at 100%, so it takes
/// the phone's layout rather than a list squeezed beside the side panel (gaps row V15).
pub fn is_narrow(window: &Window) -> bool {
    narrow_at(f32::from(window.viewport_size().width) / crate::window::zoom::factor())
}

/// What the phone's layout keeps between frames.
#[derive(Default)]
pub(crate) struct Narrow {
    /// The sheet open: the overflow menu or a card's menu, walked into its submenus.
    pub(crate) sheet: Option<Sheet>,
    /// The drawer of places open, with the place chosen when it opened: a place chosen in it
    /// closes it.
    pub(crate) drawer: Option<Place>,
    /// The card pressed and when, for a long press.
    pub(crate) pressed: Option<(Id, std::time::Instant)>,
    /// Where the press began: a finger that moves from it is scrolling, not holding.
    pub(crate) pressed_from: Option<gpui::Point<gpui::Pixels>>,
    /// The card whose menu a long press opened while the finger was still down: its lift is
    /// not a tap (gaps row C22).
    pub(crate) held_open: Option<Id>,
    /// Several cards being chosen (a card menu's Select, gaps row N23): a tap ticks or unticks
    /// a card, and the top bar acts on those ticked.
    pub(crate) choosing: bool,
    /// The search box opened across the top bar by its search button (or Find).
    pub(crate) searching: bool,
    /// Whether the last frame was laid out as a phone's: the list's kept place then counts
    /// cards, not the table's rows (`refresh::top_row`).
    pub(crate) laid_out: bool,
}

/// How far below the window's top the drawer starts: under the top bar where the system's
/// traffic lights stand in it (a narrow window on macOS), whose red, yellow and green buttons
/// were drawn over the drawer's first heading; from the top elsewhere, where nothing of the
/// system's stands at the bar's start (a phone keeps clear of its own bars by `safe::edges`).
fn drawer_clear_of_the_bar() -> gpui::Pixels {
    if cfg!(target_os = "macos") && !crate::platform::ON_A_PHONE {
        gpui::px(crate::window::titlebar::narrow_height())
    } else {
        gpui::px(0.0)
    }
}

impl Downloads {
    /// The window's parts at a phone's width, on `root`: the top bar, the bars and the cards,
    /// the status bar, and the drawer or the sheet over them where open.
    pub(crate) fn narrow_window(
        &mut self,
        root: Stateful<Div>,
        theme: Theme,
        screen: &Screen,
        shown: crate::window::list::Shown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        // A place chosen in the drawer closes it.
        if self
            .layout
            .narrow
            .drawer
            .as_ref()
            .is_some_and(|at| *at != self.list.place)
        {
            self.layout.narrow.drawer = None;
        }
        let drawer = self.layout.narrow.drawer.is_some().then(|| {
            crate::window::sidebar::sidebar(self, theme, screen, window, cx).into_any_element()
        });
        // The layers below lie over the whole window, the bars a phone draws over it included
        // (gaps row N16): the drawer keeps clear of them itself.
        let safe = crate::window::safe::edges(window);
        if let Some(sheet) = self.layout.narrow.sheet.as_mut() {
            sheet.focus_first(window, cx);
        }
        let sheet = self
            .layout
            .narrow
            .sheet
            .as_ref()
            .map(|sheet| sheet::drawn(sheet, theme, screen, window, cx).into_any_element());
        root.child(crate::window::titlebar::narrow_titlebar(
            self, theme, screen, window, cx,
        ))
        .child(self.narrow_list(theme, screen, shown, cx))
        // A phone says how the downloads go under the top bar's title: no second bar over the
        // bottom navigation.
        .when(!crate::platform::ON_A_PHONE, |root| {
            root.child(crate::window::statusbar::statusbar(self, theme, screen, cx))
        })
        .child(bottomnav::bottom_nav(self, theme, screen, cx))
        .when_some(drawer, |root, drawer| {
            root.child(
                over(cx, |view| view.layout.narrow.drawer = None).child(entering(
                    "drawer-enter",
                    Enter::FromLeft,
                    div()
                        .id("drawer")
                        .absolute()
                        .top(safe.top + drawer_clear_of_the_bar())
                        .bottom(safe.bottom)
                        .left(safe.left)
                        .flex()
                        .occlude()
                        .child(drawer),
                )),
            )
        })
        .when_some(sheet, |root, sheet| {
            root.child(
                over(cx, |view| view.layout.narrow.sheet = None).child(entering(
                    "sheet-enter",
                    Enter::FromBottom,
                    sheet,
                )),
            )
        })
    }

    /// The bars above the list, and the cards or what an empty list says.
    fn narrow_list(
        &mut self,
        theme: Theme,
        screen: &Screen,
        shown: crate::window::list::Shown,
        cx: &mut Context<Self>,
    ) -> Div {
        let empty = shown.empty;
        let add = dm_desktop_base::i18n::t(dm_shell::app::commands::Command::AddUrl.label());
        // Undo after Remove, where a phone's apps offer it: a bar over the list's foot for as
        // long as the status bar's Undo would show (gaps row N23).
        let undo = self.status.undo.live().then(|| self.status.undo.count());
        let lifted = if undo.is_some() {
            space::XL + space::TOUCH + space::S
        } else {
            space::XL
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .bg(theme.background)
            .children(shown.bars)
            .when(!empty, |list| list.child(cards::cards(self, theme, cx)))
            .when(empty, |list| {
                list.child(crate::window::table::empty(self, theme, cx))
            })
            // Add URL where the thumb is, at the list's bottom right, as a phone's own
            // download lists have it; the top bar's + does the same at the top.
            .when(!empty, |list| {
                list.child(
                    div()
                        .id("add-floating")
                        .role(gpui::Role::Button)
                        .aria_label(add)
                        .absolute()
                        .bottom(px(lifted))
                        // Where the thumb is in a language read right to left, too (E19).
                        .map(|button| {
                            if crate::window::mirror::rtl() {
                                button.left(px(space::XL))
                            } else {
                                button.right(px(space::XL))
                            }
                        })
                        .size(px(FLOATING))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.accent)
                        .shadow_md()
                        .active(move |b| {
                            b.bg(theme.answering(theme.accent, crate::window::theme::PRESSED))
                        })
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.run(dm_shell::app::commands::Command::AddUrl, cx)
                        }))
                        .child(
                            screen.mark(
                                add,
                                gpui::svg()
                                    .path("icons/plus.svg")
                                    .size(px(26.0))
                                    .text_color(theme.on_accent),
                            ),
                        ),
                )
            })
            .when_some(undo, |list, n| list.child(snackbar(n, theme, screen, cx)))
    }

    /// Whether the top bar shows the search box: opened by its button or Find, or words in it
    /// (typed at a desktop's width, say) still narrowing the list.
    pub(crate) fn narrow_searching(&self) -> bool {
        self.layout.narrow.searching || !self.list.filter_text.trim().is_empty()
    }

    /// The top bar's search: the desktop's search box across it, the keyboard in it, its words
    /// chosen, as Find puts them.
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.narrow.drawer = None;
        self.layout.narrow.sheet = None;
        self.find(window, cx);
        cx.notify();
    }

    /// The search box's close: its words cleared, as the desktop's ✕ and Escape clear them,
    /// the top bar back, and the keyboard back to the list.
    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.narrow.searching = false;
        self.clear_filter(window, cx);
        window.focus(&self.focus, cx);
    }

    /// Escape in the phone's layout (a phone's Back is Escape given to the window on top, gaps
    /// row N16): a submenu walked into in the sheet goes back to the menu it opened from, else
    /// the sheet closes, else the drawer. True where one of them did.
    pub(crate) fn narrow_back(&mut self) -> bool {
        let narrow = &mut self.layout.narrow;
        if let Some(sheet) = narrow.sheet.as_mut() {
            if !sheet.back() {
                narrow.sheet = None;
            }
            return true;
        }
        if narrow.drawer.take().is_some() {
            return true;
        }
        // Choosing several ends, the cards as they were ticked left chosen.
        std::mem::take(&mut narrow.choosing)
    }

    /// The top bar's Places: the drawer opened, or closed where open.
    pub(crate) fn toggle_drawer(&mut self, cx: &mut Context<Self>) {
        self.layout.narrow.sheet = None;
        self.layout.narrow.drawer = match self.layout.narrow.drawer {
            Some(_) => None,
            None => Some(self.list.place.clone()),
        };
        cx.notify();
    }

    /// The top bar's overflow: the toolbar's commands, then the menus, as a sheet.
    pub(crate) fn open_overflow(&mut self, cx: &mut Context<Self>) {
        self.layout.narrow.drawer = None;
        let menu = sheet::overflow(self);
        self.layout.narrow.sheet = Some(Sheet::new(menu));
        cx.notify();
    }

    /// A card's menu (its ⋯, a long press, a right-click): the row chosen as a right-click
    /// chooses it, and the row menu shown as a sheet.
    pub(crate) fn open_card_menu(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.position_of(id).is_none() {
            return;
        }
        if !self.list.selected.contains(&id) {
            self.list.selected = [id].into();
            self.list.anchor = Some(id);
        }
        self.list.cursor = Some(id);
        self.layout.narrow.drawer = None;
        let menu = crate::window::rowmenu::card_menu(self).owned();
        // Its Open, Share, Delete, Pause or Resume and Select as big buttons at its top (gaps
        // rows C22, N23).
        self.layout.narrow.sheet = Some(Sheet::with_quick(menu, |item| {
            use dm_shell::app::commands::Command as C;
            let gpui::OwnedMenuItem::Action { action, .. } = item else {
                return false;
            };
            let any = action.as_any();
            any.is::<crate::window::rowmenu::ShareFile>()
                || any.is::<crate::window::rowmenu::ChooseSeveral>()
                || any
                    .downcast_ref::<crate::window::menus::Run>()
                    .is_some_and(|run| {
                        matches!(
                            run.0,
                            C::OpenFile | C::Remove | C::DownloadNow | C::StopDownload
                        )
                    })
        }));
        cx.notify();
    }
}

/// A layer over the whole window, dimmed, whose tap anywhere outside what it holds runs
/// `close`.
/// Where a layer comes in from: the drawer from the window's left, where it stands, a sheet
/// from its foot.
#[derive(Clone, Copy)]
enum Enter {
    FromLeft,
    FromBottom,
}

/// How far a layer travels as it comes in, and over how long (`better-motion`: a panel enters
/// in 350 to 450 ms, easing out, travelling only far enough to say where it came from).
const ENTER_TRAVEL: f32 = 48.0;
const ENTER_FOR: std::time::Duration = std::time::Duration::from_millis(350);

/// `layer` coming in as it opens: from `from` by [`ENTER_TRAVEL`], fading in, over
/// [`ENTER_FOR`]; GPUI draws its last frame at once where the system asks for less motion. A
/// wrapper the window's size is what moves, so what the layer places itself against (a sheet's
/// foot and its drag) is as it was. It goes at once on closing: an exit says nothing more.
fn entering(id: &'static str, from: Enter, layer: impl IntoElement) -> impl IntoElement {
    use gpui::{Animation, AnimationExt};
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(layer)
        .with_animation(
            id,
            Animation::new(ENTER_FOR).with_easing(gpui::ease_out_quint()),
            move |wrapper, delta| {
                let off = (1.0 - delta) * ENTER_TRAVEL;
                let wrapper = wrapper.opacity(delta);
                // The drawer stands at the window's left whatever the language: in from there.
                match from {
                    Enter::FromBottom => wrapper.top(px(off)),
                    Enter::FromLeft => wrapper.left(px(-off)),
                }
            },
        )
}

fn over(cx: &mut Context<Downloads>, close: impl Fn(&mut Downloads) + 'static) -> Stateful<Div> {
    div()
        .id("narrow-over")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(gpui::rgba(crate::window::theme::SCRIM))
        .occlude()
        .on_click(cx.listener(move |view, _, _, cx| {
            close(view);
            cx.notify();
        }))
}

/// The bar that offers Undo after `n` downloads were taken off the list.
fn snackbar(n: usize, theme: Theme, screen: &Screen, cx: &mut Context<Downloads>) -> Stateful<Div> {
    let said = dm_desktop_base::i18n::tn(
        "Removed {} download from the list.||Removed {} downloads from the list.",
        n as u64,
        &[&n],
    );
    let undo = dm_desktop_base::i18n::t("Undo");
    div()
        .id("snackbar")
        .absolute()
        .bottom(px(space::XL))
        .left(px(space::L))
        .right(px(space::L))
        .min_h(px(space::TOUCH))
        .map(|bar| {
            if crate::window::mirror::rtl() {
                bar.pr(px(space::L))
            } else {
                bar.pl(px(space::L))
            }
        })
        .flex()
        // Its sentence, then Undo, from the right where the window is mirrored (E19).
        .map(crate::window::mirror::row)
        .items_center()
        .gap(px(space::S))
        .rounded(px(crate::widgets::native::RADIUS))
        .bg(theme.text)
        .text_color(theme.background)
        .shadow_md()
        .occlude()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(screen.label(gpui::SharedString::from(said))),
        )
        .child(
            div()
                .id("snackbar-undo")
                .role(gpui::Role::Button)
                .aria_label(undo)
                .flex_none()
                .min_w(px(space::TOUCH))
                .h(px(space::TOUCH))
                .px(px(space::L))
                .flex()
                .items_center()
                .justify_center()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.selection)
                .on_click(cx.listener(|view, _, _, cx| {
                    view.undo_remove(cx);
                    cx.notify();
                }))
                .child(screen.label(undo)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every width a desktop window could have had is laid out as before; a phone's is not.
    #[test]
    fn the_desktops_widths_stay_the_desktops() {
        assert!(!narrow_at(640.0));
        assert!(!narrow_at(NARROW_BELOW));
        assert!(narrow_at(412.0));
        assert!(narrow_at(LEAST_WIDTH));
        const { assert!(LEAST_WIDTH < NARROW_BELOW && NARROW_BELOW < 640.0) };
    }
}
