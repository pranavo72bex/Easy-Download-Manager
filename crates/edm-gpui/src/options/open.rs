// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options opened from the main window (brought forward where it is open, at a page), its
//! window made, and what OK keeps put in force in the main window.

use super::*;

impl Downloads {
    /// Options, over the settings in force; brought forward where it is
    /// open, with what was typed in it.
    pub(crate) fn open_options(&mut self, cx: &mut Context<Self>) {
        self.open_options_at(Tab::General, cx);
    }

    /// Options open at `tab`; brought forward where it is open, at its tab as it stands.
    pub(crate) fn open_options_at(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let open = cx.windows();
        if let Some(options) = self.windows.options_window.filter(|w| open.contains(w)) {
            let _ = options.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let settings = self.backend.settings();
        let queues = edm_core::queue_names(self.rows.iter().map(|r| &**r));
        let view = cx.entity().downgrade();
        let me = view.clone();
        cx.defer(move |cx| {
            let handle = open_window(view, settings, queues, tab, cx);
            let _ = me.update(cx, |me, _| me.windows.options_window = handle);
        });
    }

    /// Puts settings into force: what makes EDM run something is a person's
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
            .set_settings_from(wanted, self.port.scripted.by_script());
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
        edm_desktop_base::i18n::set(edm_desktop_base::i18n::chosen(&now.general.language));
        if before.site_searches != now.site_searches {
            self.threads.searches_checked = None;
        }
        if before.general.watch_clipboard != now.general.watch_clipboard {
            self.watch_clipboard(now.general.watch_clipboard);
        }
        if before.general.theme != now.general.theme {
            crate::window::theme::follow(&now.general.theme, cx);
        }
        // Every window at the text size chosen, at once; the View menu's
        // percentage with it.
        if crate::window::zoom::set(now.text_size, cx) {
            self.menus.menu_ticks = None;
        }
        if before.general.language != now.general.language {
            // The menus and every window in the language picked.
            self.menus.menu_ticks = None;
            cx.refresh_windows();
        }
        if before.general.language != now.general.language
            || before.folders.download_dir != now.folders.download_dir
        {
            // The taskbar button's jump list (Windows) in those words, its
            // folder the one downloads now go to.
            edm_desktop_base::jumplist::publish(
                edm_desktop_base::i18n::t("Add URL"),
                edm_desktop_base::i18n::t("Open downloads folder"),
                &now.folders.download_dir,
            );
        }
        // The names and places follow the settings (the download folder,
        // the categories): worked out again, not only where a row changed.
        self.known.read_all = true;
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
    let opened = crate::window::screen::open_window(
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
            let search = cx.new(|cx| InputState::new(window, cx).placeholder(t("Find a setting")));
            let archive_passwords =
                edm_core::extract::passwords::load(&edm_shell::settings::home());
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
                instant: crate::platform::ON_A_PHONE,
                applied: None,
                #[cfg(windows)]
                handlers: None,
                #[cfg(windows)]
                handlers_asked: false,
                fields: HashMap::new(),
                numbers: Default::default(),
                not_numbers: Default::default(),
                scale: None,
                scroll: ScrollHandle::new(),
                focus,
                done: false,
                screen,
                heard: edm_shell::host::browsers_heard(&edm_shell::settings::home()),
                search: search.clone(),
                pages_shown: true,
                list_only: false,
                archive_passwords: archive_passwords.clone(),
                archive_passwords_opened: archive_passwords,
            });
            // Each letter typed into the search draws the settings it finds.
            cx.subscribe(&search, {
                let options = options.downgrade();
                move |_, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let _ = options.update(cx, |_, cx| cx.notify());
                    }
                }
            })
            .detach();
            crate::widgets::native::close_is_cancel(&options, window, cx);
            cx.new(|cx| gpui_component::Root::new(options, window, cx))
        },
    );
    match opened {
        Ok(handle) => Some(handle.into()),
        Err(e) => {
            edm_core::note!(error, "edm-gpui: Options could not be opened: {e}");
            None
        }
    }
}
