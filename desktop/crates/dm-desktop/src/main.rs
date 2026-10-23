// dm, with its window drawn in Rust.
//
// The same program as the Tauri one — the same startup, browser host, lock,
// inbox and queue, all from dm-shell — with egui in place of a web view.
// What lives here is only what a real desktop has and a test must not
// touch: the tray, the login item, notifications, the clipboard.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use dm_desktop::app::DmApp;
use dm_desktop::backend::{Backend, Platform};
use dm_shell::{host, instance, settings, shell};

#[cfg(target_os = "macos")]
mod dock;

const ICON: &[u8] = include_bytes!("../icons/128x128.png");

/// The desktop's own services.
struct Desktop;

impl Platform for Desktop {
    /// Only when the setting and the system disagree — or when the login
    /// item starts another copy of dm that is gone (the app moved, or was
    /// removed and installed elsewhere) and this one may claim it
    /// (`autostart::takes_over`, `autostart::may_claim`: a release build,
    /// not a test's dm). A copy still there keeps it, so two installed
    /// copies do not take it from each other at every launch. Registering
    /// writes the path of the running binary, so a copy that will not be
    /// there at the next login, or that macOS keeps from starting there, is
    /// refused (`autostart::refused_at_login`). Not being able to read the
    /// state is fatal only for registering: a login item that cannot be
    /// read must still be one a person can get rid of.
    fn set_login_item(&self, wanted: bool) -> Result<(), String> {
        // Linux's entry is dm's own: see `autostart`.
        if cfg!(target_os = "linux") {
            return dm_desktop::autostart::set(wanted);
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if wanted {
            let home = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            if let Some(why) = dm_desktop::autostart::refused_at_login(
                &exe,
                &home,
                cfg!(target_os = "macos"),
                dm_shell::register::mounted_read_only,
            ) {
                return Err(why);
            }
        }
        let launcher = login_launcher(&exe)?;
        match launcher.is_enabled() {
            Ok(now) if now == wanted => {
                let gone = wanted
                    && login_item_program().is_some_and(|found| {
                        dm_desktop::autostart::takes_over(
                            &found,
                            &exe,
                            dm_desktop::autostart::may_claim(),
                            |p| p.exists(),
                        )
                    });
                if !gone {
                    return Ok(());
                }
                // Enabled for a copy that is gone: written again for this one.
            }
            Ok(_) => {}
            Err(e) if wanted => return Err(e.to_string()),
            Err(_) => {}
        }
        if wanted {
            launcher.enable()
        } else {
            launcher.disable()
        }
        .map_err(|e| e.to_string())
    }

    /// Posted from a thread of its own: on macOS, `show` waits on the
    /// system for about two seconds, and called from the window's thread
    /// every finished download froze the window that long (several at once,
    /// 25 seconds). Found by `scripts/ui-shot.py` and a `sample` of the
    /// stuck process.
    fn notify(&self, title: &str, body: &str) {
        let (title, body) = (title.to_string(), body.to_string());
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .appname("dm")
                .summary(&title)
                .body(&body)
                .show();
        });
    }

    /// As `notify`, and a click on it opens the file, where the system
    /// says which was clicked: Linux's notification servers and Windows'
    /// toasts do. macOS says so only to a sender that waits on it in step,
    /// which can hold a thread for as long as the notification is kept, so
    /// there a click brings dm forward, as the system does by itself.
    fn notify_file(&self, title: &str, body: &str, path: &str) {
        /// Notifications waited on at once: each holds a thread until it is
        /// clicked or closed, and some servers keep them a long time.
        static WAITING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        const MOST_WAITING: usize = 8;
        let (title, body, path) = (title.to_string(), body.to_string(), path.to_string());
        std::thread::spawn(move || {
            let mut notification = notify_rust::Notification::new();
            notification.appname("dm").summary(&title).body(&body);
            #[cfg(all(unix, not(target_os = "macos")))]
            if !path.is_empty() {
                // The body's click, as the servers name it.
                notification.action("default", "Open");
            }
            let Ok(handle) = notification.show() else {
                return;
            };
            #[cfg(target_os = "macos")]
            let _ = (handle, &path, &WAITING, MOST_WAITING);
            #[cfg(not(target_os = "macos"))]
            {
                use std::sync::atomic::Ordering;
                if path.is_empty() || WAITING.fetch_add(1, Ordering::AcqRel) >= MOST_WAITING {
                    WAITING.fetch_sub(1, Ordering::AcqRel);
                    return;
                }
                let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                    if response.is_default_action() {
                        let _ = shell::open_file(&path);
                    }
                });
                WAITING.fetch_sub(1, Ordering::AcqRel);
            }
        });
    }

    fn clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn reveal(&self, path: &str) -> Result<(), String> {
        shell::reveal_in_folder(path)
    }

    fn open(&self, path: &str) -> Result<(), String> {
        shell::open_file(path)
    }

    fn open_later(&self, path: &str, report: dm_desktop::power::Report) {
        let path = path.to_string();
        std::thread::spawn(move || {
            if let Err(e) = shell::open_file(&path) {
                report(e);
            }
        });
    }

    fn reveal_later(&self, path: &str, report: dm_desktop::power::Report) {
        let path = path.to_string();
        std::thread::spawn(move || {
            // The file manager's own "show this file", selected in its
            // folder, where one answers on the session bus; otherwise the
            // folder alone (UI round 10).
            #[cfg(target_os = "linux")]
            if std::path::Path::new(&path).exists() && show_item(&path) {
                return;
            }
            if let Err(e) = shell::reveal_in_folder(&path) {
                report(e);
            }
        });
    }

    fn chime(&self) {
        dm_desktop::power::chime();
    }

    fn alert(&self) {
        dm_desktop::power::alert();
    }

    fn power(
        &self,
        action: dm_desktop::power::WhenDone,
        report: dm_desktop::power::Report,
    ) -> Result<(), String> {
        dm_desktop::power::run(action, report)
    }

    fn choose_app(&self) -> Option<std::path::PathBuf> {
        let dialog = rfd::FileDialog::new().set_title("Open with");
        #[cfg(target_os = "macos")]
        let dialog = dialog
            .set_directory("/Applications")
            .add_filter("Applications", &["app"]);
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Apps", &["exe"]);
        dialog.pick_file()
    }

    fn open_with(&self, path: &str, app: &std::path::Path) -> Result<(), String> {
        shell::open_with(path, app)
    }

    fn choose_folder(&self) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    /// On Linux the chooser is the desktop portal's, else zenity's; with
    /// neither, rfd answers as if cancelled. Elsewhere the system has one.
    fn can_choose_folder(&self) -> bool {
        #[cfg(target_os = "linux")]
        return dm_desktop::backend::chooser_here(
            file_chooser_portal(),
            std::env::var_os("PATH").as_deref(),
        );
        #[cfg(not(target_os = "linux"))]
        true
    }

    fn choose_file(&self, save: bool, suggested: &str) -> Option<std::path::PathBuf> {
        // What is asked for decides the filter: a settings file is JSON,
        // and was hidden behind the download lists' filter (round 17).
        let dialog = if suggested.ends_with(".json") {
            rfd::FileDialog::new().add_filter("dm settings", &["json"])
        } else {
            rfd::FileDialog::new().add_filter("Download lists", &["dmlist", "txt"])
        };
        if save {
            dialog.set_file_name(suggested).save_file()
        } else {
            dialog.pick_file()
        }
    }
}

/// What the tray asks for.
enum Tray {
    Show,
    Quit,
    /// The tray carries the speed limiter; the Scheduler opens over the
    /// window.
    Run(dm_desktop::commands::Command, bool),
}

/// The window, with the tray and the close-into-the-tray behaviour around it.
struct Window {
    app: DmApp,
    tray: mpsc::Receiver<Tray>,
    /// Whether there is a tray to hide into. Without one, closing the window
    /// has to quit: hiding it would leave no way back.
    has_tray: bool,
    /// A login launch, hidden from the start: if no tray is showing by
    /// this moment, the window opens rather than leave dm unreachable.
    hidden_until: Option<std::time::Instant>,
    quitting: bool,
    /// The window is in the tray: nothing is drawn, and dm wakes only as
    /// often as its timers need.
    hidden: bool,
    /// The window is on a Wayland display, where a window cannot be hidden
    /// or unminimized by its own program (winit's `set_visible` does
    /// nothing there). Learnt from the first frame's display handle.
    wayland: bool,
    /// Set by a second launch through the inbox, or a click on the Dock
    /// icon, asking for the window back.
    show_asked: Arc<AtomicBool>,
    /// The look last told to the window's title bar (Windows).
    title_bar_in_force: Option<(bool, egui::Color32, egui::Color32)>,
    /// Kept alive for as long as the window: dropping the icon removes it.
    /// With its menu's items, relabelled when the language changes.
    icon: Option<TrayMade>,
    /// The language the tray's words are in.
    tray_language: String,
    /// What the tray's tooltip says now, so it is set only when it changes.
    tray_tip: String,
    /// Where the window was when it went to the tray: it comes back there.
    /// A window manager places a window shown again as it likes, which was
    /// somewhere else each time.
    hidden_at: Option<egui::Pos2>,
    /// Whether it was maximized then: shown again it came back at its
    /// restored size in a corner (round 12).
    hidden_maximized: bool,
    /// The last registration failure said in the window, so opening dm
    /// again does not say it again.
    browsers_said: Option<String>,
}

/// The window's words for a registration that failed.
fn registration_failed(why: &str) -> String {
    format!(
        "your browsers could not be told where dm is: {why}. Until they are, the extension \
         leaves downloads in the browser."
    )
}

impl Window {
    fn show(&mut self, ctx: &egui::Context) {
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
    fn hide(&mut self, ctx: &egui::Context) {
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
    #[cfg(target_os = "linux")]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let ended = dm_desktop::xdnd::ENDED.swap(false, std::sync::atomic::Ordering::Relaxed)
            | winit::platform::wayland::dm_take_drag_ended();
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
        // a tenth of a core idle (life-cycle QA, gap 67).
        if self.hidden {
            // The floating drop target stays, and takes what is dropped.
            self.app.while_hidden(ui.ctx());
            return;
        }
        self.app.ui(ui);
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
    fn tick(&mut self, ctx: &egui::Context) {
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
        // so "open dm once" works while dm runs (gap 131).
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
            // first while downloads run, as Exit is (it went at once, life-
            // cycle QA, gap 66). The answer Exit comes back through
            // `quit_asked`.
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

/// A login launch: dm waits in the tray with no window until one is asked
/// for — the tray's Show, a second launch, the Dock icon, a hand-off that
/// opens a dialog, or no tray to wait in. The window and its GL context
/// are about 40 MB (`qa/after.md`) that a dm waiting in the tray all day has
/// no use for; eframe's own start made them at once and hid them, after
/// showing them for a frame.
///
/// Until then dm's logic runs on a stand-in context with no fonts and no
/// drawing (`run_logic`), woken by what wakes the window: its own timers,
/// the queue, the tray, a second launch. Once shown, eframe's context takes
/// over, and every wake the stand-in is asked for is passed on to it.
fn run_hidden(
    options: eframe::NativeOptions,
    make: impl FnOnce(&egui::Context) -> Window + 'static,
) -> Result<(), String> {
    use winit::event_loop::EventLoop;
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let real: Arc<std::sync::OnceLock<egui::Context>> = Arc::default();
    let real_ctx = Arc::clone(&real);
    let made: std::rc::Rc<std::cell::RefCell<Option<Window>>> = std::rc::Rc::default();
    let eframe_app = eframe::create_native(
        "dm",
        options,
        Box::new({
            let (real, made) = (Arc::clone(&real), std::rc::Rc::clone(&made));
            move |cc| {
                let _ = real.set(cc.egui_ctx.clone());
                match made.borrow_mut().take() {
                    Some(window) => Ok(Box::new(window)),
                    None => Err("the window was asked for before dm had started".into()),
                }
            }
        }),
        &event_loop,
    );
    let ctx = egui::Context::default();
    let due: Arc<Mutex<Option<std::time::Instant>>> = Arc::default();
    let ticking = Arc::new(AtomicBool::new(false));
    ctx.set_repaint_observer({
        let (due, ticking, proxy) = (
            Arc::clone(&due),
            Arc::clone(&ticking),
            event_loop.create_proxy(),
        );
        move |info| {
            if let Some(real) = real.get() {
                real.request_repaint_after(info.delay);
                return;
            }
            let at = std::time::Instant::now() + info.delay;
            {
                let mut due = dm_core::lock(&due);
                if due.is_none_or(|d| at < d) {
                    *due = Some(at);
                }
            }
            // From another thread, or a timer's: the loop is woken to look
            // at the new time. A tick's own asks are read when it ends.
            if !ticking.load(Ordering::Relaxed) {
                let _ = proxy.send_event(eframe::UserEvent::RequestRepaint {
                    viewport_id: egui::ViewportId::ROOT,
                    when: at,
                    cumulative_pass_nr: info.current_cumulative_pass_nr,
                });
            }
        }
    });
    let mut lazy = Lazy {
        eframe: eframe_app,
        make: Some(Box::new(make)),
        made,
        ctx,
        real: real_ctx,
        asked: Vec::new(),
        due,
        ticking,
        shown: false,
    };
    event_loop.run_app(&mut lazy).map_err(|e| e.to_string())
}

/// What makes the window's state around a context.
type MakeWindow<'a> = Box<dyn FnOnce(&egui::Context) -> Window + 'a>;

/// The event loop of a login launch: dm's own until the window is shown,
/// eframe's from then on.
struct Lazy<'a> {
    eframe: eframe::EframeWinitApplication<'a>,
    make: Option<MakeWindow<'a>>,
    /// The window's state, made on the loop's start and handed to eframe's
    /// creator when shown.
    made: std::rc::Rc<std::cell::RefCell<Option<Window>>>,
    ctx: egui::Context,
    /// eframe's context, once the window is made.
    real: Arc<std::sync::OnceLock<egui::Context>>,
    /// What dm asked of a window that was not there yet — a control
    /// request's screenshot, the focus — given to the window once it is.
    asked: Vec<egui::ViewportCommand>,
    /// When dm's logic next asked to run.
    due: Arc<Mutex<Option<std::time::Instant>>>,
    ticking: Arc<AtomicBool>,
    shown: bool,
}

impl Lazy<'_> {
    fn tick(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let now = std::time::Instant::now();
        *dm_core::lock(&self.due) = None;
        let (quitting, show) = {
            let mut made = self.made.borrow_mut();
            let Some(window) = made.as_mut() else {
                return;
            };
            self.ticking.store(true, Ordering::Relaxed);
            let out = self
                .ctx
                .run_logic(&egui::RawInput::default(), |ctx| window.tick(ctx));
            self.ticking.store(false, Ordering::Relaxed);
            for (_, commands) in out.viewport_commands {
                self.asked.extend(commands);
            }
            (window.quitting, !window.hidden)
        };
        if quitting {
            event_loop.exit();
        } else if show {
            self.shown = true;
            winit::application::ApplicationHandler::resumed(&mut self.eframe, event_loop);
            if let Some(real) = self.real.get() {
                for command in self.asked.drain(..) {
                    real.send_viewport_cmd(command);
                }
                real.request_repaint();
            }
        } else {
            // Not straight round again on an ask for "now": dm's headless
            // loop looks twenty times a second at most, and so does this.
            let mut due = dm_core::lock(&self.due);
            let floor = now + std::time::Duration::from_millis(50);
            *due = Some(due.map_or(now + std::time::Duration::from_secs(30), |d| d.max(floor)));
        }
    }
}

impl winit::application::ApplicationHandler<eframe::UserEvent> for Lazy<'_> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.resumed(event_loop);
            return;
        }
        // On the loop's start, as eframe's creator would be: the tray and
        // the Dock's hook need the application winit has made by now.
        if let Some(make) = self.make.take() {
            *self.made.borrow_mut() = Some(make(&self.ctx));
            *dm_core::lock(&self.due) = Some(std::time::Instant::now());
        }
    }

    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if self.shown {
            self.eframe.new_events(event_loop, cause);
        }
    }

    fn user_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: eframe::UserEvent,
    ) {
        if self.shown {
            self.eframe.user_event(event_loop, event);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if self.shown {
            self.eframe.window_event(event_loop, window_id, event);
        }
    }

    fn device_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if self.shown {
            self.eframe.device_event(event_loop, device_id, event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !self.shown {
            let due = *dm_core::lock(&self.due);
            if due.is_some_and(|d| d <= std::time::Instant::now()) {
                self.tick(event_loop);
            }
        }
        if self.shown {
            self.eframe.about_to_wait(event_loop);
        } else if let Some(due) = *dm_core::lock(&self.due) {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(due));
        }
    }

    fn suspended(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.suspended(event_loop);
        }
    }

    fn exiting(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.exiting(event_loop);
        }
    }

    fn memory_warning(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.memory_warning(event_loop);
        }
    }
}

/// Starts the system's drag of finished files out of the window, into
/// Finder or Explorer, a mail or a chat. Called while the pointer is down,
/// from the frame that saw the drag begin.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn drag_out(frame: &eframe::Frame, files: Vec<std::path::PathBuf>) -> Result<(), String> {
    drag::start_drag(
        frame,
        drag::DragItem::Files(files),
        drag::Image::Raw(DRAG_ICON.to_vec()),
        |_, _| {},
        drag::Options::default(),
    )
    .map_err(|e| format!("the file could not be dragged out: {e}"))
}

/// On Linux the drag library needs a GTK window, and dm's is not one: on
/// X11 dm speaks the drag protocol itself (`xdnd`); on Wayland the drag has
/// to begin on winit's own connection, with the press's serial, so dm's
/// patched winit starts it (`dm_request_drag_out`).
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn drag_out(frame: &eframe::Frame, files: Vec<std::path::PathBuf>) -> Result<(), String> {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    #[cfg(target_os = "linux")]
    if let Ok(handle) = frame.display_handle() {
        match handle.as_raw() {
            RawDisplayHandle::Xlib(xlib) => {
                let display = xlib.display.map_or(std::ptr::null_mut(), |d| d.as_ptr());
                return dm_desktop::xdnd::start(display, files);
            }
            RawDisplayHandle::Wayland(_) => {
                winit::platform::wayland::dm_request_drag_out(dm_desktop::xdnd::uri_list(&files));
                return Ok(());
            }
            _ => {}
        }
    }
    let _ = (frame, files);
    Err(dm_desktop::i18n::t(
        "Dragging files out of dm is not available here: right-click › Open folder",
    )
    .into())
}

/// What a dragged file looks like under the pointer: dm's icon, small.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const DRAG_ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// Where the window's size and place are kept: under a `DM_HOME` when one
/// is set, beside that run's own settings and queue, so a test's or a
/// screenshot's dm does not write the person's real window state; the
/// system's place for dm otherwise.
fn window_state_file(home: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    home.filter(|h| !h.is_empty())
        .map(|h| std::path::PathBuf::from(h).join("window.ron"))
}

/// The tray's words in the language in force: its four menu items, then
/// its tooltip.
fn tray_words() -> [&'static str; 5] {
    use dm_desktop::i18n::t;
    [
        t("Show dm"),
        t("Speed limiter"),
        t("Scheduler…"),
        t("Exit dm"),
        t("dm — downloads keep running here"),
    ]
}

/// The tray icon: Show, the speed limiter and Scheduler, and Quit.
///
/// Optional: a desktop may not expose a StatusNotifier host. In that case dm
/// says so in the window and closing it quits instead of hiding without a tray.
type TrayMade = (
    tray_icon::TrayIcon,
    Vec<tray_icon::menu::MenuItem>,
    tray_icon::menu::CheckMenuItem,
);

fn tray(wake: egui::Context, send: mpsc::Sender<Tray>) -> Result<TrayMade, String> {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, Submenu};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let image = image::load_from_memory(ICON)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (w, h) = image.dimensions();
    let icon = tray_icon::Icon::from_rgba(image.into_raw(), w, h).map_err(|e| e.to_string())?;

    use dm_desktop::commands::Command;
    use tray_icon::menu::PredefinedMenuItem;
    let [show, limiter, scheduler, quit, tooltip] = tray_words();
    let show = MenuItem::new(show, true, None);
    let limiter = CheckMenuItem::new(limiter, true, false, None);
    let scheduler = MenuItem::new(scheduler, true, None);
    let quit = MenuItem::new(quit, true, None);
    // The speed presets, one click from the tray.
    let presets: Vec<(Command, MenuItem)> = [
        Command::SpeedLow,
        Command::SpeedMedium,
        Command::SpeedHigh,
        Command::SpeedUnlimited,
    ]
    .into_iter()
    .map(|c| (c, MenuItem::new(c.title(), true, None)))
    .collect();
    let speeds = Submenu::new(dm_desktop::i18n::t("Speed"), true);
    for (_, item) in &presets {
        let _ = speeds.append(item);
    }
    let mut items = vec![show.clone(), scheduler.clone(), quit.clone()];
    items.extend(presets.iter().map(|(_, item)| item.clone()));
    let menu = Menu::with_items(&[
        &show,
        &PredefinedMenuItem::separator(),
        &limiter,
        &speeds,
        &scheduler,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .map_err(|e| e.to_string())?;
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
    let (limiter_id, scheduler_id) = (limiter.id().clone(), scheduler.id().clone());
    let preset_ids: Vec<(tray_icon::menu::MenuId, Command)> = presets
        .iter()
        .map(|(c, item)| (item.id().clone(), *c))
        .collect();

    let (to_menu, to_click) = (send.clone(), send);
    let (wake_menu, wake_click) = (wake.clone(), wake);
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let asked = if event.id == show_id {
            Tray::Show
        } else if event.id == quit_id {
            Tray::Quit
        } else if event.id == limiter_id {
            Tray::Run(Command::SpeedLimiterToggle, false)
        } else if event.id == scheduler_id {
            Tray::Run(Command::Scheduler, true)
        } else if let Some((_, preset)) = preset_ids.iter().find(|(id, _)| *id == event.id) {
            Tray::Run(*preset, false)
        } else {
            return;
        };
        let _ = to_menu.send(asked);
        wake_menu.request_repaint();
    }));
    // A left click on the icon is the one gesture everyone tries first; the
    // menu is on the right button.
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = to_click.send(Tray::Show);
            wake_click.request_repaint();
        }
    }));

    // One id across launches: panels remember an item's place, and whether
    // it is hidden, by it, and it was the process's number (UI round 4).
    let builder = TrayIconBuilder::new()
        .with_id("dm")
        .with_icon(icon)
        .with_tooltip(tooltip);
    // A name for panels that list items by it (its D-Bus title was empty,
    // UI round 2); on a Mac a title is text beside the icon, so not there.
    #[cfg(target_os = "linux")]
    let builder = builder.with_title("dm");
    builder
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map(|icon| (icon, items, limiter.clone()))
        .map_err(|e| e.to_string())
}

/// Starts the tray where its platform wants it, and says whether there is one.
///
/// On Linux, KSNI runs its D-Bus service on its own thread. Elsewhere the
/// tray is made on the window's thread, after the event loop has started,
/// which macOS requires.
fn start_tray(wake: egui::Context, send: mpsc::Sender<Tray>) -> Result<Option<TrayMade>, String> {
    #[cfg(target_os = "linux")]
    {
        // Made whether or not a tray host is up yet: at login dm can start
        // before the panel does, and the tray registers when one appears.
        // Whether it is showing is asked when it matters, on closing.
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tray(wake, send)))
            .unwrap_or_else(|panic| Err(panic_text(&*panic)))
            .map(Some);
        if made.is_ok() {
            sni::keep_registered();
        }
        made
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tray(wake, send)))
            .unwrap_or_else(|panic| Err(panic_text(&*panic)))
            .map(Some)
    }
}

/// Whether something is showing tray icons now: on Linux, a
/// StatusNotifier host may come later than dm, or never (GNOME without its
/// AppIndicator extension). Elsewhere the system always has one.
fn tray_host() -> bool {
    #[cfg(target_os = "linux")]
    {
        sni::shown(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Shows `path` selected in its folder through the freedesktop file
/// manager interface, which the usual file managers answer (and are
/// started for). False where none is there to ask.
#[cfg(target_os = "linux")]
fn show_item(path: &str) -> bool {
    const NAME: &str = "org.freedesktop.FileManager1";
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let there = zbus::blocking::fdo::DBusProxy::new(&connection).is_ok_and(|bus| {
        let Ok(name) = zbus::names::BusName::try_from(NAME) else {
            return false;
        };
        bus.name_has_owner(name).unwrap_or(false)
            || bus
                .list_activatable_names()
                .is_ok_and(|names| names.iter().any(|n| n.as_str() == NAME))
    });
    if !there {
        return false;
    }
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination(NAME)
        .and_then(|b| b.path("/org/freedesktop/FileManager1"))
        .and_then(|b| b.interface(NAME))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    let Ok(proxy) = proxy else {
        return false;
    };
    proxy
        .call::<_, _, ()>("ShowItems", &(vec![dm_desktop::format::file_uri(path)], ""))
        .is_ok()
}

/// Whether the desktop portal offers its file chooser on the session bus.
#[cfg(target_os = "linux")]
fn file_chooser_portal() -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination("org.freedesktop.portal.Desktop")
        .and_then(|b| b.path("/org/freedesktop/portal/desktop"))
        .and_then(|b| b.interface("org.freedesktop.portal.FileChooser"))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    proxy.is_ok_and(|p| p.get_property::<u32>("version").is_ok())
}

/// dm's tray item and the StatusNotifierWatcher that panels (GNOME's
/// extension, KDE, waybar) show items from.
#[cfg(target_os = "linux")]
mod sni {
    use zbus::blocking::{fdo::DBusProxy, Connection, Proxy};

    const WATCHER: &str = "org.kde.StatusNotifierWatcher";

    /// Whether the watcher lists this process's tray item. With `register`,
    /// one it does not list is registered again: a watcher that has just
    /// (re)started can take its bus name before it answers calls, and a
    /// registration that lands in that gap is dropped without a reply —
    /// seen in the Linux check, and what a panel restart (waybar's, often)
    /// would do. A watcher that will not say what it lists is believed.
    pub fn shown(register: bool) -> bool {
        let Ok(connection) = Connection::session() else {
            return false;
        };
        check(&connection, register).unwrap_or(false)
    }

    /// Every few seconds, for as long as dm runs: the tray item stays
    /// registered with whatever watcher there is.
    pub fn keep_registered() {
        let _ = std::thread::Builder::new()
            .name("dm-tray-watch".into())
            .spawn(|| {
                let mut connection = None;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    if connection.is_none() {
                        connection = Connection::session().ok();
                    }
                    if let Some(connection) = &connection {
                        let _ = check(connection, true);
                    }
                }
            });
    }

    fn check(connection: &Connection, register: bool) -> Option<bool> {
        let bus = DBusProxy::new(connection).ok()?;
        let watcher_up = bus
            .name_has_owner(zbus::names::BusName::try_from(WATCHER).ok()?)
            .ok()?;
        if !watcher_up {
            return Some(false);
        }
        let prefix = format!("org.kde.StatusNotifierItem-{}-", std::process::id());
        let ours: Vec<String> = bus
            .list_names()
            .ok()?
            .into_iter()
            .map(|n| n.to_string())
            .filter(|n| n.starts_with(&prefix))
            .collect();
        if ours.is_empty() {
            return Some(false);
        }
        let watcher = zbus::blocking::proxy::Builder::<Proxy>::new(connection)
            .destination(WATCHER)
            .ok()?
            .path("/StatusNotifierWatcher")
            .ok()?
            .interface(WATCHER)
            .ok()?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .ok()?;
        let Ok(listed) = watcher.get_property::<Vec<String>>("RegisteredStatusNotifierItems")
        else {
            return Some(true);
        };
        let mut all = true;
        for name in &ours {
            let owner = zbus::names::BusName::try_from(name.as_str())
                .ok()
                .and_then(|n| bus.get_name_owner(n).ok())
                .map(|o| o.to_string());
            // Listed as the name, the name and a path, or the connection
            // that owns it: watchers differ.
            let is_ours = |entry: &String| {
                [Some(name.as_str()), owner.as_deref()]
                    .into_iter()
                    .flatten()
                    .any(|id| entry == id || entry.starts_with(&format!("{id}/")))
            };
            if !listed.iter().any(is_ours) {
                all = false;
                if register {
                    let _ = watcher.call_method("RegisterStatusNotifierItem", &(name.as_str(),));
                }
            }
        }
        // Listed is not shown: a watcher with no panel drawing its items
        // left a hidden window nothing could bring back (UI round 1). A
        // watcher that will not say is believed.
        let host = watcher
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .unwrap_or(true);
        Some(all && host)
    }
}

fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "the tray service could not start".into())
}

/// Set when the system asks dm to end: SIGTERM, SIGINT or SIGHUP (a
/// logout, `kill`, a service manager stopping it, Ctrl+C in a terminal),
/// or Windows' console close, logoff and shutdown events.
static SIGNALLED: AtomicBool = AtomicBool::new(false);

/// Ends dm as Program › Exit does when the system asks, rather than being
/// cut off where it stood: its downloads were stopped by nothing, a
/// yt-dlp run under it went on alone, and `control.port` was left naming a
/// port nobody listened on. A second signal while closing ends it at once.
fn on_signal(runtime: &tokio::runtime::Runtime, wake: impl Fn() + Send + Sync + 'static) {
    let wake = Arc::new(wake);
    // The handlers are installed here, now: a stream made inside the task
    // would leave the default action in place until the task first ran.
    let _inside = runtime.enter();
    let mut asked: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>> =
        Vec::new();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        for kind in [
            SignalKind::terminate(),
            SignalKind::interrupt(),
            SignalKind::hangup(),
        ] {
            if let Ok(mut stream) = signal(kind) {
                asked.push(Box::pin(async move {
                    while stream.recv().await.is_some() {
                        if quit_on_signal() {
                            return;
                        }
                    }
                }));
            }
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        macro_rules! listen {
            ($make:expr) => {
                if let Ok(mut stream) = $make {
                    asked.push(Box::pin(async move {
                        while stream.recv().await.is_some() {
                            if quit_on_signal() {
                                return;
                            }
                        }
                    }));
                }
            };
        }
        listen!(windows::ctrl_c());
        listen!(windows::ctrl_close());
        listen!(windows::ctrl_logoff());
        listen!(windows::ctrl_shutdown());
    }
    for each in asked {
        let wake = Arc::clone(&wake);
        runtime.spawn(async move {
            each.await;
            wake();
        });
    }
}

/// The first signal asks dm to quit; a second, while it is still closing,
/// ends it there. Says whether this one asked.
fn quit_on_signal() -> bool {
    if SIGNALLED.swap(true, Ordering::SeqCst) {
        dm_desktop::control::forget_port(&settings::home());
        // Leaving without unwinding: what yt-dlp runs is ended here, or it
        // would go on downloading with nobody to stop it.
        dm_core::ytdlp::end_all();
        std::process::exit(130);
    }
    true
}

/// Starts the control protocol and says where in dm's home.
fn start_control(app: &mut DmApp, port: u16, ctx: &egui::Context) {
    let home = settings::home();
    let started = dm_desktop::control::token(&home).and_then(|token| {
        let (to_window, requests) = mpsc::channel();
        let wake = ctx.clone();
        let port =
            dm_desktop::control::listen(port, token, to_window, move || wake.request_repaint())?;
        dm_desktop::control::write_port(&home, port)?;
        Ok((port, requests))
    });
    match started {
        Ok((_, requests)) => app.attach_control(requests),
        Err(e) => app
            .backend
            .note(format!("the control protocol could not start: {e}")),
    }
}

fn window_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(ICON).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// The login item for `exe`, launched into the tray.
fn login_launcher(exe: &std::path::Path) -> Result<auto_launch::AutoLaunch, String> {
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name("dm")
        .set_app_path(&exe.to_string_lossy())
        // The login item launches into the tray, not onto the desktop.
        .set_args(&["--hidden"])
        // The current user's Run key, never the machine's: run elevated,
        // the default wrote HKLM, which dm unelevated can neither see nor
        // remove (gap 97).
        .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser)
        .build()
        .map_err(|e| e.to_string())
}

/// The program the login item `auto-launch` wrote starts: on macOS its
/// launch agent's plist, on Windows the Run key's value. `None` when there
/// is none or it cannot be read, which is not taken as a reason to rewrite
/// it.
fn login_item_program() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        let plist = std::path::PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/LaunchAgents/dm.plist");
        let text = std::fs::read_to_string(plist).ok()?;
        return dm_desktop::autostart::plist_program(&text);
    }
    #[cfg(windows)]
    {
        use dm_desktop::autostart::{registry, run_value_program, RUN_KEY};
        registry::string(RUN_KEY, "dm").and_then(|value| run_value_program(&value))
    }
    #[cfg(not(windows))]
    None
}

/// Whether the person turned dm's login item off outside dm: on Windows,
/// in Task Manager's Startup tab. Elsewhere there is no such switch.
fn login_item_turned_off_outside() -> bool {
    #[cfg(windows)]
    {
        use dm_desktop::autostart::{registry, turned_off_in_task_manager, STARTUP_APPROVED_KEY};
        registry::binary(STARTUP_APPROVED_KEY, "dm")
            .is_some_and(|approved| turned_off_in_task_manager(&approved))
    }
    #[cfg(not(windows))]
    false
}

/// `--unregister`'s other half: dm's login item, removed, and what was
/// done said. Only this copy's, or one whose program is gone: uninstalling
/// a second copy must not take the login item from the installed one.
/// The browsers' manifests are `register::from_args`'s.
fn remove_login_item() {
    use dm_desktop::autostart::{self, Removal};
    let removed: Result<Removal, String> = if cfg!(target_os = "linux") {
        match (autostart::entry_path(), autostart::program()) {
            (Some(path), Ok(this)) => autostart::remove_at(&path, &this, |p| p.exists()),
            (None, _) => Ok(Removal::None),
            (_, Err(e)) => Err(e.to_string()),
        }
    } else {
        std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| {
                let found = login_item_program();
                if let Some(found) = found
                    .as_deref()
                    .filter(|f| !autostart::removes(Some(f), &exe, |p| p.exists()))
                {
                    return Ok(Removal::Other(found.to_path_buf()));
                }
                let launcher = login_launcher(&exe)?;
                // On Windows, unconditionally: a login item turned off in
                // Task Manager reads as not enabled while its Run value is
                // still there, and a missing value is no error.
                let there = found.is_some() || launcher.is_enabled().unwrap_or(true);
                if !there && !cfg!(target_os = "windows") {
                    return Ok(Removal::None);
                }
                launcher.disable().map_err(|e| e.to_string())?;
                Ok(if there {
                    Removal::Removed(if cfg!(target_os = "macos") {
                        "the login item (~/Library/LaunchAgents/dm.plist)".to_string()
                    } else {
                        r"the login item (HKCU\Software\Microsoft\Windows\CurrentVersion\Run\dm)"
                            .to_string()
                    })
                } else {
                    Removal::None
                })
            })
    };
    match removed {
        Ok(Removal::Removed(what)) => println!("removed {what}"),
        Ok(Removal::None) => println!("dm had no login item"),
        Ok(Removal::Other(program)) => println!(
            "the login item starts another copy of dm, at {}; left alone",
            program.display()
        ),
        Err(why) => eprintln!("dm: the login item could not be removed: {why}"),
    }
}

/// `--unregister`'s last part: the login setting turned off, so the next
/// launch does not put the login item back, and a dm still running asked
/// to end, which otherwise wrote the browser manifest and the login entry
/// straight back (life-cycle QA, gap 69).
fn unregister_the_rest() {
    let (mut saved, _) = settings::Settings::load();
    if saved.start_on_login {
        saved.start_on_login = false;
        match saved.save() {
            Ok(()) => println!("turned off starting dm at login"),
            Err(e) => eprintln!("dm: the login setting could not be turned off: {e}"),
        }
    }
    match shell::ask_running_to_quit(&settings::home()) {
        Ok(true) => println!("asked the running dm to end"),
        Ok(false) => {}
        Err(e) => eprintln!("dm: the running dm could not be asked to end: {e}"),
    }
}

/// On Windows a release build is a GUI program with no console, so what
/// `--register` and `--unregister` print went nowhere when run from cmd or
/// PowerShell (gap 90). Attached to the console of whatever started it,
/// `println!` reaches the terminal; with none (started from Explorer)
/// nothing changes.
fn attach_console() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        // SAFETY: no pointers; it fails harmlessly when there is no parent
        // console or one is already attached.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// What `dm-desktop --help` prints.
const USAGE: &str = "\
dm — a download manager

Usage:
  dm-desktop [options] [LINK ...]

Each link (http, https, ftp, magnet, or a .torrent file) is downloaded by the
dm already running, or by one started for it. Options apply to the link that
follows them:
  --save-to FOLDER   save the next link into FOLDER
  --name NAME        save the next link as NAME
  --later            add the next link to the queue without starting it
  --start-queue      start the main queue
  --when-done WHAT   once the downloads are done: nothing, exit, sleep,
                     hibernate or shutdown (for this session)

Other commands:
  --hidden           start in the tray, with no window
  --headless         run with no window at all
  --register [ID]    connect dm to the installed browsers (ID: an extension id)
  --unregister       disconnect dm from the browsers and remove its login item
  mcp                serve dm's commands to an AI client (Model Context Protocol)
  list [--json]      what dm is downloading (dm must run with --control 0)
  status ID          one download
  pause|resume|cancel ID…   act on downloads by their numbers from `list`
  --version          print dm's version
  --help             print this

Examples:
  dm-desktop https://example.com/file.zip
  dm-desktop --save-to ~/Videos --name talk.mp4 https://example.com/v.mp4
  dm-desktop --later https://example.com/a.iso --later https://example.com/b.iso
  dm-desktop --start-queue --when-done shutdown

Portable: an empty file named \"portable\" beside the program keeps dm's
settings and list in a \"dm-data\" folder beside it, and leaves the
browsers and the login items to an installed dm.
";

fn main() {
    // First, before any thread starts: a copy with a "portable" file beside
    // it keeps its things beside it, for the host and the window alike.
    settings::enter_portable();
    let args = shell::args();

    // Launched by a browser rather than a person: framed JSON on stdin and
    // stdout, no window, nothing else printed.
    if shell::browser_launch(&args) {
        host::run(&settings::home());
        return;
    }

    // `--version`: the version, and nothing else. Without it, `dm --version`
    // opened the window (found checking an installed AppImage, 0.2.0).
    if matches!(args.first().map(String::as_str), Some("--version" | "-V")) {
        println!("dm {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // `--help`: how to use dm from a command line, and nothing else. It
    // opened the window (QA pass, 2026-10-22).
    if matches!(
        args.first().map(String::as_str),
        Some("--help" | "-h" | "/?")
    ) {
        attach_console();
        print!("{USAGE}");
        return;
    }

    // `list`, `status`, `pause`, `resume`, `cancel`: a client of the running
    // dm's control protocol, like `mcp`. Takes no lock.
    if args.first().is_some_and(|a| dm_desktop::cli::is_command(a)) {
        attach_console();
        std::process::exit(dm_desktop::cli::main(&args, &settings::home()));
    }

    // `mcp`: a Model Context Protocol server on stdin and stdout, relaying
    // to the running dm's control protocol. Takes no lock: it is a client.
    if args.first().map(String::as_str) == Some("mcp") {
        std::process::exit(dm_desktop::mcp::main(&args[1..], &settings::home()));
    }

    // `--register [id…]` / `--unregister`: the browsers' host manifests,
    // written or removed, and nothing else. No window, no lock.
    if matches!(
        args.first().map(String::as_str),
        Some("--register" | "--unregister")
    ) {
        attach_console();
    }
    if args.first().map(String::as_str) == Some("--unregister") {
        remove_login_item();
        unregister_the_rest();
    }
    if let Some(code) = dm_shell::register::from_args(&args) {
        std::process::exit(code);
    }

    let handed = shell::orders(&args);

    // One dm at a time. The lock is held for the life of this process by
    // living to the end of `main`.
    let headless = args.iter().any(|a| a == "--headless");
    let _running = match instance::claim(&settings::home()) {
        Ok(Some(lock)) => lock,
        // Registered first: this launch ends at the hand-off, and a browser
        // installed while dm ran was never told where dm is (gap 131). The
        // running dm, asked to show itself, checks again and says what
        // fails in its window.
        Ok(None) => {
            if let Some(why) = shell::hand_off(
                &settings::home(),
                &handed,
                dm_shell::register::ensure_if_claimed,
            ) {
                eprintln!("dm: your browsers could not be told where dm is: {why}");
            }
            return;
        }
        // Not taken for a running dm, which it used to be: dm exited without
        // a word, every time. Nor started without it: a second dm started
        // the same way would write the same queue. Said, where a person
        // launching from the Dock will see it too.
        Err(e) => {
            let why = format!(
                "dm cannot tell whether another dm is running, so it has not started: {e}. \
                 Remove that file, or make it yours, and start dm again."
            );
            eprintln!("dm: {why}");
            if !headless {
                let _ = rfd::MessageDialog::new()
                    .set_title("dm could not start")
                    .set_description(&why)
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }
            std::process::exit(1);
        }
    };
    // `--when-done`, for the window this launch starts to take up.
    shell::leave_when_done(&settings::home(), &handed);

    // Made before the signals are listened for, so one arriving during
    // startup still reaches the window once there is one.
    let repaint_early: Arc<std::sync::OnceLock<egui::Context>> = Arc::default();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("dm could not start: {e}");
            std::process::exit(1);
        }
    };
    on_signal(&runtime, {
        let repaint = Arc::clone(&repaint_early);
        move || {
            if let Some(ctx) = repaint.get() {
                ctx.request_repaint();
            }
        }
    });
    let shell::Started {
        queue,
        settings: mut saved,
        mut notices,
    } = runtime.block_on(shell::start(&handed));
    let hidden = shell::launched_at_login(&args);
    // Read once: Mica is the window's from its making, so a change waits
    // for the next start.
    let mica = dm_desktop::chrome::use_mica(saved.mica);
    // `--control <port>`, or DM_CONTROL: the control protocol, for tests and
    // agents to drive the running window. Off unless asked for.
    let control: Option<u16> = args
        .iter()
        .position(|a| a == "--control")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .or_else(|| std::env::var("DM_CONTROL").ok())
        .and_then(|p| p.parse().ok());

    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application("com.dm.desktop");

    let platform: Arc<dyn Platform> = Arc::new(Desktop);
    // The login item follows the saved setting, so a reinstall lands in the
    // state the person last chose. Said in the window, not fatal. One the
    // person turned off in Task Manager stays off, and so does the setting.
    // Not for a dm run on a home of its own (`DM_HOME`: a test, a script, a
    // portable copy): starting at login is on for a new install, and every
    // test's fresh home would otherwise put a login item on the computer
    // running it. Turning the setting on in Options still registers one.
    let own_home = std::env::var_os("DM_HOME").is_some_and(|h| !h.is_empty());
    let at_launch = (!own_home).then(|| {
        dm_desktop::autostart::at_launch(saved.start_on_login, login_item_turned_off_outside())
    });
    match at_launch {
        None => {}
        Some(dm_desktop::autostart::AtLaunch::TurnedOffOutside) => {
            saved.start_on_login = false;
            notices.push(match saved.save() {
                Ok(()) => "start on login was turned off in Task Manager, so dm has turned it \
                           off too; Options › General turns it back on"
                    .to_string(),
                Err(e) => format!(
                    "start on login was turned off in Task Manager and dm leaves it off, \
                     but the setting could not be saved: {e}"
                ),
            });
        }
        Some(dm_desktop::autostart::AtLaunch::Follow(wanted)) => {
            if let Err(why) = platform.set_login_item(wanted) {
                notices.push(format!(
                    "could not change start-on-login: {why}; start on login is not in force"
                ));
            }
        }
    }

    // Anything a browser handed over while the app was closed is waiting in
    // the inbox; anything that was mid-flight came back queued.
    let show_asked = Arc::new(AtomicBool::new(false));
    let repaint = repaint_early;
    // A change to the queue wakes the window, which then has no need to look
    // every second for one while nothing moves.
    {
        let repaint = Arc::clone(&repaint);
        // Within a quarter of a second, not at once: while downloads run the
        // queue changes many times a second, and a repaint for each drew the
        // window far more often than it is drawn anyway (10 downloads: 9.7 %
        // of a core became 14.6 %). Requests within the quarter are one.
        queue.on_change(move || {
            if let Some(ctx) = repaint.get() {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        });
    }
    let handed: Arc<Mutex<std::collections::VecDeque<dm_shell::host::Request>>> = Arc::default();
    {
        let (watched, flag, repaint) =
            (queue.clone(), Arc::clone(&show_asked), Arc::clone(&repaint));
        let (asked, ask_flag, ask_repaint) = (
            Arc::clone(&handed),
            Arc::clone(&show_asked),
            Arc::clone(&repaint),
        );
        let quit_repaint = Arc::clone(&repaint);
        runtime.spawn(shell::watch_inbox(
            watched,
            move || {
                flag.store(true, Ordering::Relaxed);
                if let Some(ctx) = repaint.get() {
                    ctx.request_repaint();
                }
            },
            // A video from the panel: shown in the window's own dialog.
            move |request| {
                dm_core::lock(&asked).push_back(request);
                ask_flag.store(true, Ordering::Relaxed);
                if let Some(ctx) = ask_repaint.get() {
                    ctx.request_repaint();
                }
            },
            // `--unregister` of an uninstall: ended as a stop signal ends dm.
            move || {
                SIGNALLED.store(true, Ordering::SeqCst);
                if let Some(ctx) = quit_repaint.get() {
                    ctx.request_repaint();
                }
            },
        ));
        let queue = queue.clone();
        runtime.spawn(async move { queue.run().await });
    }

    let mut backend = Backend::new(runtime.handle().clone(), queue, saved, notices, platform);
    // A sleeping computer woken for the schedule, where the Scheduler says.
    {
        let settings = backend.settings_handle();
        dm_desktop::wake::watch(move || {
            let now = dm_core::lock(&settings).clone();
            if !now.schedule.wake {
                return None;
            }
            let starts: Vec<u32> = now
                .queue_start_list()
                .into_iter()
                .map(|(_, at)| at)
                .collect();
            let (weekday, minute) = dm_core::schedule::weekday_now();
            dm_core::schedule::minutes_until_next_start(&now.schedule, &starts, weekday, minute)
        });
    }
    backend.handed = handed;

    // `--headless`: the queue, the browser hand-off and the control
    // protocol, with no window — for a server, a script or an agent.
    if headless {
        let ctx = egui::Context::default();
        let mut app = DmApp::new(backend);
        app.headless = true;
        if let Some(port) = control {
            start_control(&mut app, port, &ctx);
        }
        while !app.quit_asked && !SIGNALLED.load(Ordering::SeqCst) {
            app.logic(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        dm_desktop::control::closing();
        dm_desktop::control::forget_port(&settings::home());
        return;
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("dm")
        .with_app_id("dm")
        .with_inner_size([960.0, 660.0])
        // Half the smallest at first: eframe applies this before the saved
        // text size, so at 200% the full one forced a 1440×920 window over
        // the size it was left at (UI round 4). The window sets the real
        // one, divided by the text size, on its first frame.
        .with_min_inner_size([
            dm_desktop::app::MIN_WINDOW.0 / 2.0,
            dm_desktop::app::MIN_WINDOW.1 / 2.0,
        ]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    if mica {
        viewport = viewport.with_transparent(true);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Remembers where the window was and how big, across launches.
        persist_window: true,
        persistence_path: window_state_file(std::env::var_os("DM_HOME")),
        ..Default::default()
    };

    // What the window's creator makes, around the context it is given:
    // eframe's at once, or for a login launch a stand-in's until the window
    // is first asked for.
    let make = move |ctx: &egui::Context| {
        let _ = repaint.set(ctx.clone());
        // A click on the Dock icon of an app whose window is hidden asks
        // for it back; without this the click did nothing.
        #[cfg(target_os = "macos")]
        {
            let (flag, ctx) = (Arc::clone(&show_asked), ctx.clone());
            dock::on_reopen(move || {
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            });
        }
        let (send, receive) = mpsc::channel();
        // The tray's words in the window's language.
        let language = backend.settings().language;
        dm_desktop::i18n::set(dm_desktop::i18n::chosen(&language));
        let built = start_tray(ctx.clone(), send);
        if let Err(why) = &built {
            backend.note(format!(
                "the tray icon could not be shown ({}); until it can, closing this window \
                 quits dm rather than keeping it downloading in the background.",
                why.lines().next().unwrap_or("").trim()
            ));
        }
        // An installed dm tells the browsers where it is, so the
        // extension works without a terminal. Not a development build
        // or a test's dm (DM_HOME), which must not take the browsers'
        // registration from the installed one.
        let mut browsers_said = None;
        if let Some(why) = dm_shell::register::worth_saying(
            &mut browsers_said,
            dm_shell::register::ensure_if_claimed(),
        ) {
            backend.note(registration_failed(&why));
        }
        let has_tray = built.is_ok();
        let icon = built.ok().flatten();
        let mut app = DmApp::new(backend);
        if let Some(port) = control {
            start_control(&mut app, port, ctx);
        }
        Window {
            app,
            tray: receive,
            has_tray,
            hidden,
            wayland: false,
            hidden_until: hidden
                .then(|| std::time::Instant::now() + std::time::Duration::from_secs(10)),
            quitting: false,
            show_asked,
            title_bar_in_force: None,
            icon,
            tray_language: language,
            tray_tip: String::new(),
            hidden_at: None,
            hidden_maximized: false,
            browsers_said,
        }
    };
    let result = if hidden {
        run_hidden(options, make)
    } else {
        eframe::run_native(
            "dm",
            options,
            Box::new(move |cc| Ok(Box::new(make(&cc.egui_ctx)))),
        )
        .map_err(|e| e.to_string())
    };
    dm_desktop::control::closing();
    dm_desktop::control::forget_port(&settings::home());
    if let Err(e) = result {
        eprintln!("dm: the window could not be opened: {e}");
        // Said in the exit status too: a launcher, a login entry or a
        // script asking whether dm started must not be told it did.
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_run_with_its_own_home_keeps_its_window_state_there() {
        assert_eq!(
            super::window_state_file(Some("/tmp/dm-test".into())),
            Some(std::path::PathBuf::from("/tmp/dm-test/window.ron"))
        );
        assert_eq!(super::window_state_file(None), None, "the system's place");
        assert_eq!(super::window_state_file(Some("".into())), None);
    }
}
