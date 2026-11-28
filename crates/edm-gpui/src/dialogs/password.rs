// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The archive's password (gaps row C2b): Extract here or Extract to on an archive that has a
//! password none of the saved ones opens asks for it in a small dialog. Its Extract tries the
//! password typed; one that does not open the archive is said in the dialog, which stays open
//! for another try. "Keep this password" adds it to Options › Folders › Archive passwords once
//! it has opened the archive, so the next archive that has it opens without asking. The
//! password goes with the extracting thread and is dropped with it, kept nowhere else.

use std::path::PathBuf;

use edm_desktop_base::i18n::{t, tf, tn};
use gpui::{
    prelude::*, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent, SharedString,
    WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

/// The dialog's title.
const TITLE: &str = "Archive password";
/// The password's field.
const PASSWORD: &str = "Password";
/// The switch that keeps the password.
const KEEP: &str = "Keep this password";

/// An archive to extract that wants its password: where it is, where it came from, and where
/// it goes (`None`: beside itself).
#[derive(Clone, Debug)]
pub(crate) struct Asked {
    pub path: PathBuf,
    pub url: String,
    pub into: Option<PathBuf>,
}

/// The dialog's view.
pub struct PasswordDialog {
    view: WeakEntity<Downloads>,
    asked: Asked,
    password: Entity<InputState>,
    keep: bool,
    says: Option<SharedString>,
    /// The archive being extracted: Extract meanwhile is not taken.
    busy: bool,
    focus: FocusHandle,
    screen: Screen,
}

impl PasswordDialog {
    /// Extract: the archive tried with the password typed, off this thread.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let password = self.password.read(cx).value().to_string();
        if password.is_empty() {
            return self.refused(t("Type the archive’s password.").into(), window, cx);
        }
        self.busy = true;
        let (tell, told) = async_channel::bounded(1);
        let Asked { path, url, into } = self.asked.clone();
        let tried = password.clone();
        let started = edm_shell::app::files::extract_later_with(
            path,
            url,
            into,
            Some(password),
            move |answer| {
                let _ = tell.send_blocking(answer);
            },
        );
        if let Err(e) = started {
            self.busy = false;
            return self.refused(tf("Not extracted: {}", &[&e]).into(), window, cx);
        }
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = told.recv().await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.answered(answer, tried, window, cx)
            });
        })
        .detach();
        cx.notify();
    }

    /// What the extraction came to: done (the password kept where asked, the window told, the
    /// dialog closed), the wrong password (said here, for another try), or another failure
    /// (said in the window, the dialog closed).
    fn answered(
        &mut self,
        answer: Result<edm_core::extract::Extracted, edm_core::extract::Error>,
        password: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use edm_core::extract::Error;
        self.busy = false;
        let said = match answer {
            Err(Error::WrongPassword(_) | Error::Encrypted(_)) => {
                return self.refused(t("That password does not open it.").into(), window, cx);
            }
            Ok(done) => {
                if self.keep {
                    keep(password);
                }
                tn(
                    "Extracted {} file to {}||Extracted {} files to {}",
                    done.files as u64,
                    &[&done.files, &done.folder.display()],
                )
            }
            Err(e) => tf("Not extracted: {}", &[&e]),
        };
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |view, cx| {
                view.say(said);
                cx.notify();
            });
        }
        window.remove_window();
    }

    /// `why` said above the buttons, the password chosen to be typed again.
    fn refused(&mut self, why: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.says = Some(why);
        self.password.update(cx, |p, cx| {
            p.focus(window, cx);
            p.select_all(window, cx);
        });
        cx.notify();
    }

    /// Cancel, Escape or the close button: nothing extracted.
    fn cancel(&mut self, window: &mut Window) {
        self.busy = true;
        window.remove_window();
    }
}

/// `password` first among the saved archive passwords (Options › Folders).
fn keep(password: String) {
    let home = edm_shell::settings::home();
    let mut all = vec![password];
    all.extend(edm_core::extract::passwords::load(&home));
    if let Err(e) = edm_core::extract::passwords::save(&home, &all) {
        edm_core::note!(warn, "EDM: an archive password could not be kept: {e}");
    }
}

impl Render for PasswordDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let screen = self.screen.clone();
        let width = native::label_width(&[t(PASSWORD)]);
        let name = self
            .asked
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let said = tf(
            "{} has a password, and none of the saved ones opens it.",
            &[&name],
        );
        let root = native::dialog("password", t(TITLE), theme, cx)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter"
                    && native::return_is_default(window, Some(&this.focus), cx)
                {
                    this.ok(window, cx);
                }
            }))
            .child(crate::widgets::fit::fitted(
                native::body(theme)
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::note(said, theme.text, &screen),
                    ))
                    .child(native::row(
                        Some(t(PASSWORD)),
                        width,
                        &screen,
                        native::secret_field(&self.password, PASSWORD, theme),
                    ))
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::switch_inline("keep", t(KEEP).into(), self.keep, theme, &screen)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.keep = !this.keep;
                                cx.notify();
                            })),
                    ))
                    .child(native::row(
                        None,
                        width,
                        &screen,
                        native::says("password-says", self.says.clone(), theme, &screen),
                    ))
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("Extract").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ));
        crate::window::safe::kept_clear(root, window)
    }
}

/// Opens the dialog over the middle of the screen for the archive `asked`.
pub(crate) fn open(view: WeakEntity<Downloads>, asked: Asked, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, asked, cx));
}

fn open_now(view: WeakEntity<Downloads>, asked: Asked, cx: &mut App) {
    // Asked for again while open: the one open, as it stands.
    if crate::window::screen::bring_named(TITLE, cx) {
        return;
    }
    // A sentence, a field and a switch, as wide as Add URL.
    let (width, _) = native::WindowSize::AddUrl.here();
    let bounds = Bounds::centered(None, size(px(width), px(260.0)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t(TITLE).into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let password = cx.new(|cx| InputState::new(window, cx).masked(true));
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let dialog = cx.new(|cx| {
                // Return in the field is Extract; what was said goes once it is changed.
                cx.subscribe_in(
                    &password,
                    window,
                    |this: &mut PasswordDialog, _, event, window, cx| match event {
                        InputEvent::PressEnter { .. } => this.ok(window, cx),
                        InputEvent::Change if this.says.take().is_some() => cx.notify(),
                        _ => {}
                    },
                )
                .detach();
                PasswordDialog {
                    view,
                    asked,
                    password: password.clone(),
                    keep: false,
                    says: None,
                    busy: false,
                    focus: cx.focus_handle(),
                    screen,
                }
            });
            password.update(cx, |p, cx| p.focus(window, cx));
            native::close_is_cancel(&dialog, window, cx);
            cx.new(|cx| gpui_component::Root::new(dialog, window, cx))
        },
    );
    if let Err(e) = opened {
        edm_core::note!(error, "edm-gpui: {TITLE} could not be opened: {e}");
    }
}

impl native::CloseButton for PasswordDialog {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
