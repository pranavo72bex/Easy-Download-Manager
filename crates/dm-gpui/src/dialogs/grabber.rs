// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Grabber (Tasks › Grabber, B60, B61), a window of its own: a start
//! page, the kinds of file to find (a template fills them in), how deep to
//! follow its links and whether to other sites, whether to keep the site's
//! folders, how long to read pages, the files' sizes and paths; Find files,
//! the files found ticked to add for later; and a search saved to run by
//! itself every so many hours. What it searches and saves is dm's
//! (`dm_desktop_base::grabber`).

use std::collections::HashSet;

use dm_desktop_base::format;
use dm_desktop_base::grabber::{templates, Grab, SiteFiles};
use dm_desktop_base::i18n::{t, tf};
use gpui::{
    div, prelude::*, size, uniform_list, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent,
    Role, SharedString, UniformListScrollHandle, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::widgets::popup::{self, Pick};
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod sections;

/// How often the Grabber is drawn again while it reads, so its count moves.
const LOOKING_DRAWN_EVERY: std::time::Duration = std::time::Duration::from_millis(300);

/// A found file's row, in points.
const ROW: f32 = 22.0;
/// The rows shown before the list scrolls.
const SHOWN: usize = 10;

/// The Grabber window's view.
pub struct Grabber {
    view: WeakEntity<Downloads>,
    d: SiteFiles,
    page: Entity<InputState>,
    types: Entity<InputState>,
    depth: Entity<InputState>,
    minutes: Entity<InputState>,
    min_kb: Entity<InputState>,
    max_kb: Entity<InputState>,
    include: Entity<InputState>,
    exclude: Entity<InputState>,
    name: Entity<InputState>,
    every: Entity<InputState>,
    /// Addresses already in the list (bare), offered unticked.
    listed: HashSet<String>,
    focus: FocusHandle,
    screen: Screen,
    scroll: UniformListScrollHandle,
    /// The crawl under way, its pages and files so far, and its Stop (gaps row U25).
    watch: std::sync::Arc<dm_shell::lists::CrawlWatch>,
}

/// A whole number as typed, within its bounds; what is not one is 0.
fn number(input: &Entity<InputState>, most: u64, cx: &App) -> u64 {
    input
        .read(cx)
        .value()
        .trim()
        .parse::<u64>()
        .map(|n| n.min(most))
        .unwrap_or(0)
}

impl Grabber {
    /// What the fields say, into the search.
    fn read_fields(&mut self, cx: &App) {
        self.d.page = self.page.read(cx).value().to_string();
        self.d.types = self.types.read(cx).value().to_string();
        self.d.depth = number(&self.depth, u64::from(dm_shell::lists::MAX_DEPTH), cx) as u8;
        self.d.stop_after_minutes = number(&self.minutes, dm_shell::lists::MOST_MINUTES, cx);
        self.d.min_kb = number(&self.min_kb, u64::from(u32::MAX), cx);
        self.d.max_kb = number(&self.max_kb, u64::from(u32::MAX), cx);
        self.d.include = self.include.read(cx).value().to_string();
        self.d.exclude = self.exclude.read(cx).value().to_string();
        self.d.name = self.name.read(cx).value().to_string();
        self.d.every_hours = number(&self.every, 24 * 31, cx);
    }

    /// The settings in force, from the main window's view.
    fn settings(&self, cx: &App) -> dm_shell::Settings {
        self.view
            .upgrade()
            .map(|v| v.read(cx).backend.settings())
            .unwrap_or_default()
    }

    /// Find files: the site read in the background, what it found shown
    /// here, those already in the list unticked.
    fn look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_fields(cx);
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let backend = view.read(cx).backend.clone();
        let mut search = self.d.search();
        // The login Options › Sign-ins keeps for the site, where it keeps one.
        search.page = backend.settings().with_login(&search.page);
        *dm_core::lock(&self.d.state) = Grab::Looking;
        let state = self.d.state.clone();
        let listed = self.listed.clone();
        let (tell, told) = async_channel::bounded::<()>(1);
        self.watch = Default::default();
        dm_shell::app::search::crawl_watched_later(
            &backend,
            search,
            self.watch.clone(),
            move |found| {
                *dm_core::lock(&state) = match found {
                    Ok(found) => Grab::Found(
                        found
                            .files
                            .into_iter()
                            .map(|l| {
                                let new = !listed.contains(&dm_shell::lists::bare(&l));
                                (l, new)
                            })
                            .collect(),
                        found.pages,
                        found.stopped,
                    ),
                    Err(e) => Grab::Failed(e),
                };
                let _ = tell.try_send(());
            },
        );
        cx.spawn_in(window, async move |this, cx| {
            if told.recv().await.is_ok() {
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
        // Drawn again while it reads, so its count moves (gaps row U25).
        let state = self.d.state.clone();
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(LOOKING_DRAWN_EVERY).await;
            if !matches!(*dm_core::lock(&state), Grab::Looking) {
                return;
            }
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                return;
            }
        })
        .detach();
        cx.notify();
    }

    /// Stop: the crawl ends after the page it is reading, keeping what it found.
    fn stop(&mut self, cx: &mut Context<Self>) {
        self.watch
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        cx.notify();
    }

    /// Save search: saved under its name, to run by itself every so many
    /// hours (0: by hand).
    fn save(&mut self, cx: &mut Context<Self>) {
        self.read_fields(cx);
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let backend = view.read(cx).backend.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        match dm_desktop_base::grabber::saved(&backend.settings(), self.d.search(), now) {
            Ok(settings) => {
                self.d.says.clear();
                if let Err(e) = backend.set_settings(settings) {
                    self.d.says = e;
                }
            }
            Err(words) => self.d.says = words.to_string(),
        }
        cx.notify();
    }

    /// A saved search run now, or deleted.
    fn saved_search(&mut self, name: String, run: bool, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let backend = view.read(cx).backend.clone();
        let mut settings = backend.settings();
        if run {
            if let Some(search) = settings
                .site_searches
                .iter()
                .find(|s| s.name == name)
                .cloned()
            {
                let _ = self.view.update(cx, |view, _| view.run_search(search));
            }
            return;
        }
        settings.site_searches.retain(|s| s.name != name);
        if let Err(e) = backend.set_settings(settings) {
            self.d.says = e;
        }
        cx.notify();
    }

    /// Add for later, or Start now (`later` false, gaps row U25): the files ticked, in the Main
    /// queue (in folders that mirror the site's, where that is asked for).
    fn add(&mut self, later: bool, window: &mut Window, cx: &mut Context<Self>) {
        let chosen: Vec<String> = match &*dm_core::lock(&self.d.state) {
            Grab::Found(links, _, _) => links.iter().filter(|l| l.1).map(|l| l.0.clone()).collect(),
            _ => return,
        };
        let keep_layout = self.d.keep_layout;
        let with = dm_core::AddOptions {
            later,
            queue: dm_core::MAIN_QUEUE.into(),
            ..dm_core::AddOptions::default()
        };
        let _ = self.view.update(cx, |view, cx| {
            if let Ok(n) = view.add_found(chosen, with, keep_layout, cx) {
                view.say(dm_desktop_base::i18n::tn(
                    "Added {} download to {}.||Added {} downloads to {}.",
                    n as u64,
                    &[&n, &dm_desktop_base::i18n::name(dm_core::MAIN_QUEUE)],
                ));
            }
        });
        window.remove_window();
    }

    /// A template chosen: its types filled in.
    fn pick(&mut self, pick: &Pick, window: &mut Window, cx: &mut Context<Self>) {
        if pick.field == "template" {
            if let Some((_, types)) = templates().get(pick.index) {
                let types = types.to_string();
                self.types
                    .update(cx, |f, cx| f.set_value(types, window, cx));
            }
        }
        cx.notify();
    }
}

impl Render for Grabber {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let settings = self.settings(cx);
        // Whether Options › Sign-ins keeps a login for the start page's
        // site: the search uses it.
        let page = self.page.read(cx).value().trim().to_string();
        let signed_in = !page.is_empty() && settings.with_login(&page) != page;
        let saved = settings.site_searches.clone();
        let labels = [
            t("Start page"),
            t("Template"),
            t("File types"),
            t("Page depth (0 for the start page only)"),
            t("Stop reading pages after (minutes)"),
            t("Files at least (KB, 0 for any)"),
            t("and at most (KB, 0 for any)"),
            t("Only paths containing"),
            t("Skip paths containing"),
        ];
        let width = native::label_width(&labels);
        // On a phone a field under its words at the whole width, a number or a choice at the
        // right of them (gaps row N31); a computer's form as it was.
        let row = |i: usize, control: gpui::AnyElement| {
            native::row_wide(Some(labels[i]), width, &screen, control)
        };
        let short = |i: usize, control: gpui::AnyElement| {
            native::row_short(Some(labels[i]), width, &screen, control)
        };
        let field = |input: &Entity<InputState>, name: &'static str| {
            div()
                .flex_1()
                .child(native::field(input, name, theme))
                .into_any_element()
        };
        let small = |input: &Entity<InputState>, name: &'static str| {
            div()
                .w(px(90.0))
                .child(native::field(input, name, theme))
                .into_any_element()
        };
        let templates: Vec<SharedString> = templates().iter().map(|(w, _)| (*w).into()).collect();
        let other = self.d.other_sites;
        let keep = self.d.keep_layout;

        let state = self.state(theme, &screen, cx);
        let found_any =
            matches!(&*dm_core::lock(&self.d.state), Grab::Found(links, _, _) if !links.is_empty());

        let body = native::body(theme)
            .child(row(0, field(&self.page, "Start page")))
            .when(signed_in, |d| {
                d.child(native::row(
                    None,
                    width,
                    &screen,
                    native::note(
                        t("Signed in with the login Options › Sign-ins keeps for this site."),
                        theme.muted,
                        &screen,
                    ),
                ))
            })
            .child(short(
                1,
                popup::button(
                    "template",
                    t("Choose…").into(),
                    templates,
                    None,
                    theme,
                    &screen,
                )
                .into_any_element(),
            ))
            .child(row(2, field(&self.types, "File types")))
            .child(short(
                3,
                small(&self.depth, "Page depth (0 for the start page only)"),
            ))
            .child(Self::follow(other, keep, theme, &screen, width, cx))
            .child(short(
                4,
                small(&self.minutes, "Stop reading pages after (minutes)"),
            ))
            .child(short(
                5,
                small(&self.min_kb, "Files at least (KB, 0 for any)"),
            ))
            .child(short(6, small(&self.max_kb, "and at most (KB, 0 for any)")))
            .child(row(7, field(&self.include, "Only paths containing")))
            .child(row(8, field(&self.exclude, "Skip paths containing")))
            // The dialog's action straight after what it acts on.
            .child(native::row(
                None,
                width,
                &screen,
                native::button("find", t("Find files").into(), true, theme, &screen)
                    .on_click(cx.listener(|this, _, window, cx| this.look(window, cx))),
            ))
            .child(
                div()
                    .h(px(space::LINE))
                    .my(px(space::XS))
                    .bg(theme.separator),
            );
        let body = self
            .saving(theme, &screen, &saved, body, cx)
            .child(
                div()
                    .h(px(space::LINE))
                    .my(px(space::XS))
                    .bg(theme.separator),
            )
            .children(state)
            .child(Self::buttons(theme, &screen, found_any, cx));

        let root = div()
            .id("grabber")
            .role(Role::Dialog)
            .aria_label(t("Grabber"))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, pick: &Pick, window, cx| this.pick(pick, window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                } else if event.keystroke.key == "enter" {
                    this.look(window, cx);
                }
            }))
            .size_full()
            .bg(theme.window);
        let root = native::phone_framed(root, t("Grabber"), theme)
            .child(crate::widgets::fit::fitted(body));
        crate::window::safe::kept_clear(root, window)
    }
}

/// Opens the Grabber.
pub fn open(view: WeakEntity<Downloads>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, SiteFiles::default(), cx));
}

/// Opens the Grabber filled in with a saved `search`: a Grabber project clicked in the side
/// panel (gaps rows U40, U41).
pub fn open_with(view: WeakEntity<Downloads>, search: &dm_shell::lists::Search, cx: &mut App) {
    let filled = SiteFiles::from_search(search);
    cx.defer(move |cx| open_now(view, filled, cx));
}

fn open_now(view: WeakEntity<Downloads>, filled: SiteFiles, cx: &mut App) {
    // Asked for again while open: the one open, as it stands.
    if crate::window::screen::bring_named(t("Grabber"), cx) {
        return;
    }
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let listed = dm_shell::app::control_api::listed_addresses(&downloads.read(cx).rows);
    // Smaller on Windows, as Options is.
    let (width, height) = crate::widgets::native::WindowSize::Grabber.here();
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Grabber").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let d = filled;
            let field = |value: String, window: &mut Window, cx: &mut App| {
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx);
                    s.set_value(value, window, cx);
                    s
                })
            };
            let page = field(d.page.clone(), window, cx);
            let types = field(d.types.clone(), window, cx);
            let depth = field(d.depth.to_string(), window, cx);
            let minutes = field(d.stop_after_minutes.to_string(), window, cx);
            let min_kb = field(d.min_kb.to_string(), window, cx);
            let max_kb = field(d.max_kb.to_string(), window, cx);
            let include = field(d.include.clone(), window, cx);
            let exclude = field(d.exclude.clone(), window, cx);
            let name = field(d.name.clone(), window, cx);
            let every = field(d.every_hours.to_string(), window, cx);
            page.update(cx, |p, cx| p.focus(window, cx));
            let grabber = cx.new(|cx| {
                // Return in the start page finds files; in the name, saves.
                cx.subscribe_in(&page, window, |this: &mut Grabber, _, event, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.look(window, cx);
                    }
                })
                .detach();
                Grabber {
                    view,
                    d,
                    page,
                    types,
                    depth,
                    minutes,
                    min_kb,
                    max_kb,
                    include,
                    exclude,
                    name,
                    every,
                    listed,
                    focus: cx.focus_handle(),
                    screen,
                    scroll: UniformListScrollHandle::new(),
                    watch: Default::default(),
                }
            });
            cx.new(|cx| gpui_component::Root::new(grabber, window, cx))
        },
    );
    if let Err(e) = opened {
        dm_core::note!(error, "dm-gpui: the Grabber could not be opened: {e}");
    }
}

#[cfg(test)]
impl Grabber {
    /// The start page, the types, the smallest size and the name as typed (the window's tests).
    pub(crate) fn typed(&self, cx: &App) -> (String, String, String, String) {
        let value = |field: &Entity<InputState>| field.read(cx).value().to_string();
        (
            value(&self.page),
            value(&self.types),
            value(&self.min_kb),
            value(&self.name),
        )
    }
}
