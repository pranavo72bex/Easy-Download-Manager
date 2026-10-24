use super::*;

/// The window's words for a registration that failed.
pub(super) fn registration_failed(why: &str) -> String {
    format!(
        "your browsers could not be told where dm is: {why}. Until they are, the extension \
         leaves downloads in the browser."
    )
}

impl Window {
    pub(super) fn show(&mut self, ctx: &egui::Context) {
        let maximized = std::mem::take(&mut self.hidden_maximized);
        // A maximized window has no place of its own to go back to.
        let at = self.hidden_at.take().filter(|_| !maximized);
        if let Some(at) = at {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(at));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        if maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        if let Some(at) = at {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(at));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.hidden = false;
    }

    /// Into the tray, remembering where it was.
    pub(super) fn hide(&mut self, ctx: &egui::Context) {
        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
            self.hidden_at = Some(rect.min);
        }
        self.hidden_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if self.wayland {
            // `Visible(false)` does nothing on Wayland, and marking the
            // window hidden stopped it drawing while the compositor still
            // showed it: a frozen ghost that could only be killed (KDE
            // Plasma 6, AppImage). The compositor hides a minimized window
            // itself and the task manager brings it back; dm keeps drawing.
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.hidden = true;
    }
}

impl eframe::App for Window {
    /// Clear under Mica, so the backdrop shows through the frame.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if dm_desktop::chrome::mica() {
            [0.0; 4]
        } else {
            // eframe's own, as before.
            egui::Color32::from_rgba_unmultiplied(12, 12, 12, 180).to_normalized_gamma_f32()
        }
    }

    /// Runs while the window is hidden too, which is what lets the tray and
    /// a second launch bring it back.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        #[cfg(target_os = "linux")]
        {
            use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
            self.wayland = frame
                .display_handle()
                .is_ok_and(|h| matches!(h.as_raw(), RawDisplayHandle::Wayland(_)));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = frame;
        self.tick(ctx);
    }

    /// A drag out of the window on X11 ends with the button let go under
    /// the drag's own grab, which winit never hears: egui is told, or it
    /// would go on thinking the button held (`xdnd`).
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.extend(self.app.synthetic_events(ctx));
        #[cfg(target_os = "linux")]
        let ended = dm_desktop::xdnd::ENDED.swap(false, std::sync::atomic::Ordering::Relaxed)
            | winit::platform::wayland::dm_take_drag_ended();
        #[cfg(target_os = "linux")]
        if ended {
            let pos = ctx.input(|i| i.pointer.latest_pos()).unwrap_or_default();
            raw_input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: ctx.input(|i| i.modifiers),
            });
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Hidden in the tray, nothing is drawn: it was drawn on every wake,
        // a tenth of a core idle.
        if self.hidden {
            // The floating drop target stays, and takes what is dropped.
            self.app.while_hidden(ui.ctx());
            return;
        }
        self.app.ui(ui);
        self.app.scan_screen(ui.ctx());
        // Links dragged in from a browser, where winit hands over files only.
        {
            use raw_window_handle::HasWindowHandle;
            if let Ok(handle) = frame.window_handle() {
                dm_desktop::drops::attach(handle.as_raw());
                // The title bar in the look's colours, when the look changes.
                let t = dm_desktop::theme::Tokens::get(ui.ctx());
                let look = (t.dark, t.frame, t.text);
                if self.title_bar_in_force != Some(look) {
                    self.title_bar_in_force = Some(look);
                    dm_desktop::chrome::title_bar(handle.as_raw(), t.dark, t.frame, t.text);
                }
            }
        }
        let files = std::mem::take(&mut self.app.drag_out);
        if !files.is_empty() {
            if let Err(e) = drag_out(frame, files) {
                self.app.said = e.into();
            }
        }
    }
}

impl Window {
    /// Everything but the drawing: the tray, a second launch, the queue.
    /// Also run before there is a window at all, for a login launch.
    pub(super) fn tick(&mut self, ctx: &egui::Context) {
        while let Ok(asked) = self.tray.try_recv() {
            match asked {
                Tray::Show => self.show(ctx),
                // Asked first while downloads run, in the window.
                Tray::Quit => {
                    self.app.ask_quit();
                    if self.app.dialogs.confirm_exit {
                        self.show(ctx);
                    }
                }
                Tray::Run(command, show) => {
                    if show {
                        self.show(ctx);
                    }
                    self.app.run(command);
                }
            }
        }
        // Asked to show itself — dm opened again, or its Dock icon
        // clicked: a browser installed since dm started is registered now,
        // so "open dm once" works while dm runs.
        if std::mem::take(&mut self.app.show_asked) {
            self.show(ctx);
        }
        if self.show_asked.swap(false, Ordering::Relaxed) {
            if let Some(why) = dm_shell::register::worth_saying(
                &mut self.browsers_said,
                dm_shell::register::ensure_if_claimed(),
            ) {
                self.app.backend.note(registration_failed(&why));
            }
            self.show(ctx);
        }
        if let Some(until) = self.hidden_until {
            let now = std::time::Instant::now();
            if now < until {
                ctx.request_repaint_after(until - now);
            } else {
                self.hidden_until = None;
                if !(self.has_tray && tray_host()) {
                    self.show(ctx);
                }
            }
        }

        // The close button hides the window instead of ending the app: a
        // download manager that stops downloading when you close its window
        // is not one. Quit is in the tray menu, and Cmd+Q still quits.
        if ctx.input(|i| i.viewport().close_requested())
            && self.has_tray
            && !self.quitting
            && tray_host()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide(ctx);
        } else if ctx.input(|i| i.viewport().close_requested())
            && !self.quitting
            && !self.app.quit_asked
            && self.app.downloading() > 0
        {
            // With no tray to go to, closing the window ends dm: asked
            // first while downloads run, as Exit is (it went at once). The
            // answer Exit comes back through `quit_asked`.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.app.ask_quit();
        }
        self.app.hidden = self.hidden;
        self.app.logic(ctx);
        if self.app.applied.language != self.tray_language {
            self.tray_language = self.app.applied.language.clone();
            if let Some((icon, items, limiter)) = &self.icon {
                let [show, limit, scheduler, quit, tooltip] = tray_words();
                for (item, word) in items.iter().zip([show, scheduler, quit]) {
                    item.set_text(word);
                }
                limiter.set_text(limit);
                let _ = icon.set_tooltip(Some(tooltip));
            }
        }
        // The tray's tooltip: how many downloads and how fast, else what dm
        // is doing there.
        if let Some((icon, _, _)) = &self.icon {
            let tip = self
                .app
                .tray_activity()
                .unwrap_or_else(|| tray_words()[4].to_string());
            if tip != self.tray_tip {
                let _ = icon.set_tooltip(Some(&tip));
                self.tray_tip = tip;
            }
        }
        // The tray's limiter item is ticked while a limit is in force.
        if let Some((_, _, limiter)) = &self.icon {
            let on = self.app.applied.speed_limit > 0;
            if limiter.is_checked() != on {
                limiter.set_checked(on);
            }
        }
        // Program › Exit, as the tray's Quit does; and a logout, `kill` or
        // a service manager stopping dm, which ask the same.
        if std::mem::take(&mut self.app.quit_asked) || SIGNALLED.load(Ordering::SeqCst) {
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
