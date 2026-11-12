// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options, a window of its own with the egui window's six pages (B43) and
//! their words, laid out as macOS's own settings are: the pages as a tab
//! view's segments at the top, each setting a check box, a pop-up or a
//! field, with what it does under it, in sections under bold headings. OK
//! keeps only what Options changed and refuses what cannot be kept, on the
//! page that holds it (`dm_desktop_base::options`); Cancel keeps nothing.
//! The pages are drawn in `options_pages.rs`.

use std::collections::HashMap;

use dm_desktop_base::i18n::t;
use dm_desktop_base::options::{self, Tab, Typed};
use dm_shell::settings::Settings;
use dm_shell::settings::TEXT_SIZE;
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, Role, ScrollHandle, SharedString,
    WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::slider::{SliderEvent, SliderState};

use crate::list::Downloads;
use crate::native;
use crate::popup::Pick;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

/// The window's size: one for every page, so the tabs and the buttons stay
/// where they are when another page is picked; a page longer than that
/// scrolls. Smaller on Windows, where 600 points stood nearly the height of
/// a laptop's screen.
const WIDTH: f32 = crate::native::pick(680.0, 620.0, 680.0);
const HEIGHT: f32 = crate::native::pick(600.0, 520.0, 600.0);

/// Options' view.
pub struct Options {
    pub(crate) view: WeakEntity<Downloads>,
    /// The page shown.
    pub(crate) tab: Tab,
    /// The settings as they are being edited.
    pub(crate) draft: Settings,
    /// The settings as they were when Options opened: OK writes only what
    /// differs from these.
    pub(crate) opened: Settings,
    /// The lists typed as words.
    pub(crate) typed: Typed,
    /// What went wrong, said above the buttons.
    pub(crate) says: Option<SharedString>,
    /// Advanced is to show its proxy section, where a proxy that cannot be
    /// one was said.
    pub(crate) to_proxy: bool,
    /// The queues there are, for Downloads' limits and start times.
    pub(crate) queues: Vec<String>,
    /// The second networks offered, as last listed.
    pub(crate) networks: Vec<dm_core::Interface>,
    /// "How to use your phone" is open.
    pub(crate) phone: bool,
    /// Who opens magnet links and `.torrent` files (Windows), once read
    /// beside the window; read again after a change.
    #[cfg(windows)]
    pub(crate) handlers: Option<(dm_shell::register::Owner, dm_shell::register::Owner)>,
    /// They are being read.
    #[cfg(windows)]
    pub(crate) handlers_asked: bool,
    /// Each field, by its key, made the first time it is drawn.
    fields: HashMap<String, Entity<InputState>>,
    /// Appearance's scale slider, once drawn.
    scale: Option<Entity<SliderState>>,
    pub(crate) scroll: ScrollHandle,
    focus: gpui::FocusHandle,
    done: bool,
    pub(crate) screen: Screen,
}

impl Options {
    /// The field `key`, holding `value` when it is first made; what is
    /// typed in it goes into the draft (`put`).
    pub(crate) fn field(
        &mut self,
        key: &str,
        value: impl FnOnce(&Options) -> String,
        masked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(field) = self.fields.get(key) {
            return field.clone();
        }
        let text = value(self);
        let field = cx.new(|cx| {
            let mut input = InputState::new(window, cx).masked(masked);
            input.set_value(text, window, cx);
            input
        });
        let owned = key.to_string();
        cx.subscribe_in(
            &field,
            window,
            move |this: &mut Options, input, event, window, cx| match event {
                InputEvent::Change => {
                    let text = input.read(cx).value().to_string();
                    this.put(&owned, &text);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.ok(window, cx),
                _ => {}
            },
        )
        .detach();
        self.fields.insert(key.to_string(), field.clone());
        field
    }

    /// The field `key` forgotten, to be made again from the draft: a button
    /// changed what it holds (Browse…, Choose…, a preset).
    pub(crate) fn forget_field(&mut self, key: &str) {
        self.fields.remove(key);
    }

    /// Every field made again from the draft, and the lists' words read
    /// again from it: settings loaded from a file.
    pub(crate) fn retype(&mut self) {
        self.typed = Typed::of(&self.draft);
        self.fields.clear();
    }

    /// The system's folder panel; `then` with the folder chosen, once it
    /// is closed (`services::ask`: the window is not held meanwhile).
    pub(crate) fn choose_folder(
        &self,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
    ) {
        crate::services::ask_then(crate::services::Asked::folder(), cx, |o, picked, cx| {
            then(o, picked.display().to_string(), cx)
        });
    }

    /// The system's open or save panel, `name` suggested; `then` with the
    /// file chosen, once it is closed.
    pub(crate) fn choose_file(
        &self,
        save: bool,
        name: &str,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, std::path::PathBuf, &mut Context<Self>) + 'static,
    ) {
        crate::services::ask_then(crate::services::Asked::file(save, name), cx, then);
    }

    /// A sound file played, as it would be heard.
    pub(crate) fn play(&self, path: &str, cx: &App) {
        if let Some(view) = self.view.upgrade() {
            view.read(cx).backend.platform.play_file(path);
        }
    }

    /// A page opened in the browser (an extension's store).
    pub(crate) fn open_page(&self, page: &str, cx: &App) {
        if let Some(view) = self.view.upgrade() {
            view.read(cx).open_path(page);
        }
    }

    /// Who opens magnet links or `.torrent` files was changed (`done`): what
    /// went wrong said, and who opens them read again, so the page says
    /// what is so now.
    #[cfg(windows)]
    pub(crate) fn handlers_changed(&mut self, done: Result<(), String>, cx: &mut Context<Self>) {
        if let Err(why) = done {
            self.says = Some(why.into());
        }
        self.handlers = None;
        self.handlers_asked = false;
        cx.notify();
    }

    /// The fields of a list's rows forgotten, to be made again from the
    /// draft: a row added or deleted moves the rows after it.
    pub(crate) fn forget(&mut self, prefix: &str) {
        let start = format!("{prefix}.");
        self.fields.retain(|key, _| !key.starts_with(&start));
    }

    /// What was typed in field `key` put into the draft. A number out of
    /// its range is held to it; one that is not a number is left as it was.
    pub(crate) fn put(&mut self, key: &str, text: &str) {
        let n = |lo: u64, hi: u64| text.trim().parse::<u64>().ok().map(|n| n.clamp(lo, hi));
        let s = &mut self.draft;
        let parts: Vec<&str> = key.split('.').collect();
        let at = |i: &str| i.parse::<usize>().ok();
        match parts.as_slice() {
            ["types"] => self.typed.types = text.into(),
            ["skip"] => self.typed.skip = text.into(),
            ["once"] => self.typed.once = text.into(),
            ["bypass"] => self.typed.bypass = text.into(),
            ["custom", i, j] => {
                if let (Some(i), Some(j)) = (at(i), at(j)) {
                    if let Some(cell) = self.typed.custom.get_mut(i).and_then(|r| r.get_mut(j)) {
                        *cell = text.into();
                    }
                }
            }
            ["download_dir"] => s.download_dir = text.into(),
            ["parts_dir"] => s.parts_dir = text.into(),
            ["category_folders", i] => {
                if let Some(folder) = at(i).and_then(|i| s.category_folders.get_mut(i)) {
                    *folder = text.into();
                }
            }
            ["min_size_kb"] => {
                if let Some(n) = n(0, 1024 * 1024) {
                    s.catch.min_size_kb = n;
                }
            }
            ["connections"] => {
                if let Some(n) = n(1, 32) {
                    s.connections = n as usize;
                }
            }
            ["concurrency"] => {
                if let Some(n) = n(1, 10) {
                    s.concurrency = n as usize;
                }
            }
            ["queue_limits", i, "files"] => {
                if let (Some(limit), Some(n)) =
                    (at(i).and_then(|i| s.queue_limits.get_mut(i)), n(1, 10))
                {
                    limit.files = n as usize;
                }
            }
            ["queue_starts", i, "at"] => {
                if let Some(start) = at(i).and_then(|i| s.queue_starts.get_mut(i)) {
                    start.at = text.into();
                }
            }
            ["watch_folder"] => s.watch_folder = text.into(),
            ["record_minutes"] => {
                if let Some(n) = n(0, 1440) {
                    s.record_minutes = n;
                }
            }
            ["recheck_hours"] => {
                if let Some(n) = n(0, 24 * 31) {
                    s.recheck_hours = n;
                }
            }
            ["on_complete"] => s.on_complete = text.into(),
            ["speed_limit"] => {
                if let Some(n) = n(0, 1024 * 1024) {
                    s.speed_limit = n;
                }
            }
            ["attempts"] => {
                if let Some(n) = n(0, 100) {
                    s.attempts = n as u32;
                }
            }
            ["retry_wait"] => {
                if let Some(n) = n(0, 3600) {
                    s.retry_wait = n;
                }
            }
            ["read_timeout"] => {
                if let Some(n) = n(5, 600) {
                    s.read_timeout = n;
                }
            }
            ["host_limits", i, "host"] => {
                if let Some(limit) = at(i).and_then(|i| s.host_limits.get_mut(i)) {
                    limit.host = text.into();
                }
            }
            ["host_limits", i, "connections"] => {
                if let (Some(limit), Some(n)) =
                    (at(i).and_then(|i| s.host_limits.get_mut(i)), n(1, 32))
                {
                    limit.connections = n as usize;
                }
            }
            ["site_rules", i, field] => {
                if let Some(rule) = at(i).and_then(|i| s.site_rules.get_mut(i)) {
                    match *field {
                        "host" => rule.host = text.into(),
                        "folder" => rule.folder = text.into(),
                        _ => {
                            if let Some(n) = n(0, 1024 * 1024) {
                                rule.speed = n;
                            }
                        }
                    }
                }
            }
            ["cookies_file"] => {
                // What was said about the file goes with it.
                let before = std::mem::replace(&mut s.cookies_file, text.into());
                if !before.is_empty() && self.says.as_ref().is_some_and(|w| w.contains(&before)) {
                    self.says = None;
                }
            }
            ["proxy", field] => {
                let p = &mut s.proxy;
                match *field {
                    "host" => p.host = text.into(),
                    "username" => p.username = text.into(),
                    "password" => p.password = text.into(),
                    "script" => p.script = text.into(),
                    _ => {
                        if let Some(n) = n(1, 65535) {
                            p.port = n as u16;
                        }
                    }
                }
            }
            ["per_scheme", scheme, field] => {
                if let Some(entry) = s.proxy.per_scheme.get_mut(*scheme) {
                    if *field == "host" {
                        entry.host = text.into();
                    } else if let Some(n) = n(1, 65535) {
                        entry.port = n as u16;
                    }
                }
            }
            ["dialup", field] => {
                let d = &mut s.dialup;
                match *field {
                    "username" => d.username = text.into(),
                    "password" => d.password = text.into(),
                    "attempts" => {
                        if let Some(n) = n(0, u64::from(dm_shell::dialup::MOST_ATTEMPTS)) {
                            d.attempts = n as u32;
                        }
                    }
                    _ => {
                        if let Some(n) = n(0, dm_shell::dialup::MOST_WAIT_SECONDS) {
                            d.wait_seconds = n;
                        }
                    }
                }
            }
            ["peer_blocklist"] => s.peer_blocklist = text.into(),
            ["torrent_share_ratio"] => {
                if let Ok(ratio) = text.trim().parse::<f64>() {
                    s.torrent_share_ratio = ratio.clamp(0.1, 100.0);
                }
            }
            ["torrent_share_minutes"] => {
                if let Some(n) = n(1, 7 * 24 * 60) {
                    s.torrent_share_minutes = n;
                }
            }
            ["torrent_upload_limit"] => {
                if let Some(n) = n(0, 1_000_000) {
                    s.torrent_upload_limit = n;
                }
            }
            ["site_logins", i, field] => {
                if let Some(login) = at(i).and_then(|i| s.site_logins.get_mut(i)) {
                    match *field {
                        "host" => login.host = text.into(),
                        "username" => login.username = text.into(),
                        _ => login.password = text.into(),
                    }
                }
            }
            ["sounds", event] => {
                if text.trim().is_empty() {
                    s.sounds.remove(*event);
                } else {
                    s.sounds.insert(event.to_string(), text.into());
                }
            }
            _ => {}
        }
    }

    /// The window's scale chosen (Appearance's slider or one of its stops):
    /// not a draft, in force at once in every window and kept whatever OK
    /// or Cancel, as View › Larger and Smaller are.
    pub(crate) fn pick_scale(&mut self, size: f32, cx: &mut Context<Self>) {
        if let Some(view) = self.view.upgrade() {
            // After this update: the list redraws and resizes every window,
            // this one included.
            cx.defer(move |cx| view.update(cx, |view, cx| view.set_text_size(size, cx)));
        }
        cx.notify();
    }

    /// Appearance's slider, made the first time it is drawn, at the scale
    /// in force then and after (a key in another window moves it too). A
    /// choice is put in force when the slider is let go: resized under a
    /// drag, the window would move the slider from under the pointer.
    pub(crate) fn scale_slider(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SliderState> {
        let percent = TEXT_SIZE.percent(crate::zoom::factor()) as f32;
        if let Some(slider) = self.scale.clone() {
            if (slider.read(cx).value().start() - percent).abs() > 0.5 {
                slider.update(cx, |s, cx| s.set_value(percent, window, cx));
            }
            return slider;
        }
        let stops = TEXT_SIZE.stops;
        let slider = cx.new(|_| {
            SliderState::new()
                .min(stops[0] * 100.0)
                .max(stops[stops.len() - 1] * 100.0)
                .step(20.0)
                .default_value(percent)
        });
        cx.subscribe(&slider, |this: &mut Options, _, event: &SliderEvent, cx| {
            if let SliderEvent::Release(value) = event {
                this.pick_scale(TEXT_SIZE.held(value.start() / 100.0), cx);
            }
        })
        .detach();
        self.scale = Some(slider.clone());
        slider
    }

    /// A choice made in one of the pages' pop-ups.
    fn pick(&mut self, pick: &Pick, cx: &mut Context<Self>) {
        crate::options_pages::pick(self, pick);
        cx.notify();
    }

    /// Restore defaults: the page's settings back to a new install's, in the
    /// draft, and its fields made again from them.
    fn restore_defaults(&mut self, cx: &mut Context<Self>) {
        self.draft = options::restore_defaults(self.tab, &self.draft);
        self.typed = Typed::of(&self.draft);
        self.fields.clear();
        cx.notify();
    }

    /// OK: what Options changed kept, or the reason it is not said on the
    /// page that holds it, with what was typed left as it was.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
        };
        let now = view.read(cx).backend.settings();
        match options::ok(&self.opened, &self.draft, &self.typed, &now) {
            Err(refused) => {
                self.says = Some(refused.says.into());
                if let Some(tab) = refused.tab {
                    self.tab = tab;
                }
                self.to_proxy = refused.to_proxy;
                cx.notify();
            }
            Ok(wanted) => match view.update(cx, |view, cx| view.apply_settings(wanted, cx)) {
                Ok(()) => {
                    self.done = true;
                    window.remove_window();
                }
                Err(e) => {
                    self.says = Some(e.into());
                    cx.notify();
                }
            },
        }
    }

    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// Another page shown, from its top.
    fn show(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.scroll.set_offset(gpui::point(px(0.0), px(0.0)));
        }
        cx.notify();
    }
}

impl Render for Options {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let page = crate::options_pages::page(self, theme, window, cx);
        // The pages as a tab view's segments, as macOS draws them.
        let tabs = options::TABS.iter().map(|(tab, words)| {
            let tab = *tab;
            let on = self.tab == tab;
            div()
                .id(SharedString::from(*words))
                .role(Role::Tab)
                // Reached by Tab, chosen by Space or Return (gaps row U2).
                .tab_index(0)
                .focus(move |s| s.bg(theme.stripe))
                .aria_label(SharedString::from(t(words)))
                .aria_selected(on)
                .px(px(12.0))
                .h(px(native::CONTROL - 2.0))
                .flex()
                .items_center()
                .rounded(px(native::RADIUS - 1.0))
                .when(on, |d| d.bg(theme.accent).text_color(theme.on_accent))
                .child(screen.label(t(words)))
                .on_click(cx.listener(move |this, _, _, cx| this.show(tab, cx)))
        });
        let restore = !options::tab_settings(self.tab).is_empty();
        crate::native::dialog("options", t("Options"), theme, cx)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, pick: &Pick, _, cx| this.pick(pick, cx)))
            .text_size(px(native::TEXT))
            .text_color(theme.text)
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(native::MARGIN - 6.0))
                    .pb(px(native::ROW_GAP))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("pages")
                            .role(Role::TabList)
                            .p(px(1.0))
                            .flex()
                            .gap(px(1.0))
                            .rounded(px(native::RADIUS))
                            .border_1()
                            .border_color(theme.control_border)
                            .bg(theme.control)
                            .children(tabs),
                    ),
            )
            .child(
                div()
                    .id("page")
                    .flex_1()
                    .min_h_0()
                    .mx(px(native::MARGIN))
                    .rounded(px(native::RADIUS))
                    .border_1()
                    .border_color(theme.separator)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    // The settings its own children, so the page scrolls to one by its
                    // place (Advanced to its proxy section, where a proxy was refused): a
                    // column inside it was its one child, and that scroll went nowhere.
                    .p(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(native::ROW_GAP))
                    .children(page),
            )
            .child(
                div()
                    .px(px(native::MARGIN))
                    .pb(px(native::MARGIN))
                    .flex()
                    .flex_col()
                    .gap(px(native::ROW_GAP))
                    .when_some(self.says.clone(), |d, says| {
                        d.mt(px(native::ROW_GAP))
                            .child(native::note(says, theme.warning, &screen))
                    })
                    .child(native::buttons(
                        restore
                            .then(|| {
                                native::button(
                                    "restore",
                                    t("Restore defaults").into(),
                                    false,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.restore_defaults(cx)))
                                .into_any_element()
                            })
                            .into_iter()
                            .collect(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("OK").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            )
    }
}

impl Downloads {
    /// Options, over the settings in force; brought forward where it is
    /// open, with what was typed in it.
    pub(crate) fn open_options(&mut self, cx: &mut Context<Self>) {
        self.open_options_at(Tab::General, cx);
    }

    /// Options open at `tab`; brought forward where it is open, at its tab as it stands.
    pub(crate) fn open_options_at(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let open = cx.windows();
        if let Some(options) = self.options_window.filter(|w| open.contains(w)) {
            let _ = options.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let settings = self.backend.settings();
        let queues = dm_core::queue_names(self.rows.iter().map(|r| &**r));
        let view = cx.entity().downgrade();
        let me = view.clone();
        cx.defer(move |cx| {
            let handle = open_window(view, settings, queues, tab, cx);
            let _ = me.update(cx, |me, _| me.options_window = handle);
        });
    }

    /// Puts settings into force: what makes dm run something is a person's
    /// to set, so a script's change keeps the ones in force and says so
    /// (`Backend::set_settings_from`); then the language, the appearance,
    /// the clipboard's watch and the menus follow.
    pub(crate) fn apply_settings(
        &mut self,
        wanted: Settings,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let before = self.backend.settings();
        let (outcome, kept) = self
            .backend
            .set_settings_from(wanted, self.scripted.by_script());
        let said = if kept.command {
            Some(t("The command to run when a download finishes was not changed: a script was driving the window a moment ago; set it again"))
        } else if kept.schedule {
            Some(t("The program to run when the scheduled downloads are done was not changed: a script was driving the window a moment ago; set it again"))
        } else if kept.proxy_script {
            Some(t("The proxy script was not changed: a script was driving the window a moment ago; set it again"))
        } else if kept.folders {
            Some(t("The folders downloads go to were not changed: a script was driving the window a moment ago; set them again"))
        } else {
            None
        };
        if let Some(said) = said {
            self.say(said.to_string());
        }
        let now = outcome?;
        dm_desktop_base::i18n::set(dm_desktop_base::i18n::chosen(&now.language));
        if before.site_searches != now.site_searches {
            self.searches_checked = None;
        }
        if before.watch_clipboard != now.watch_clipboard {
            self.watch_clipboard(now.watch_clipboard);
        }
        if before.theme != now.theme {
            crate::theme::follow(&now.theme, cx);
        }
        // Every window at the text size chosen, at once; the View menu's
        // percentage with it.
        if crate::zoom::set(now.text_size, cx) {
            self.menu_ticks = None;
        }
        if before.language != now.language {
            // The menus and every window in the language picked.
            self.menu_ticks = None;
            cx.refresh_windows();
        }
        if before.language != now.language || before.download_dir != now.download_dir {
            // The taskbar button's jump list (Windows) in those words, its
            // folder the one downloads now go to.
            dm_desktop_base::jumplist::publish(
                dm_desktop_base::i18n::t("Add URL"),
                dm_desktop_base::i18n::t("Open downloads folder"),
                &now.download_dir,
            );
        }
        // The names and places follow the settings (the download folder,
        // the categories): worked out again, not only where a row changed.
        self.read_all = true;
        self.read();
        // The menus' ticks too (the speed limit, the drop target).
        self.refresh_menus(cx);
        cx.notify();
        Ok(())
    }
}

/// Opens Options over `settings`.
fn open_window(
    view: WeakEntity<Downloads>,
    settings: Settings,
    queues: Vec<String>,
    tab: Tab,
    cx: &mut App,
) -> Option<gpui::AnyWindowHandle> {
    let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Options").into()),
                ..Default::default()
            }),
            is_resizable: false,
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let options = cx.new(|_| Options {
                view,
                tab,
                typed: Typed::of(&settings),
                opened: settings.clone(),
                draft: settings,
                says: None,
                to_proxy: false,
                queues,
                networks: Vec::new(),
                phone: false,
                #[cfg(windows)]
                handlers: None,
                #[cfg(windows)]
                handlers_asked: false,
                fields: HashMap::new(),
                scale: None,
                scroll: ScrollHandle::new(),
                focus,
                done: false,
                screen,
            });
            crate::native::close_is_cancel(&options, window, cx);
            cx.new(|cx| gpui_component::Root::new(options, window, cx))
        },
    );
    match opened {
        Ok(handle) => Some(handle.into()),
        Err(e) => {
            eprintln!("dm-gpui: Options could not be opened: {e}");
            None
        }
    }
}

impl crate::native::CloseButton for Options {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
