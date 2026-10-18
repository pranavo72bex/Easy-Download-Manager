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

    fn clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn reveal(&self, path: &str) -> Result<(), String> {
        shell::reveal_in_folder(path)
    }

    fn open(&self, path: &str) -> Result<(), String> {
        shell::open_file(path)
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
    /// Frames left in which to hide the window again, for the login item's
    /// launch. eframe shows the window after its first frame whatever the
    /// builder asked for — a `--hidden` launch opened a window over the
    /// desktop, which the Linux check counted — so it is put back.
    hide_frames: u8,
    /// Set by a second launch through the inbox, or a click on the Dock
    /// icon, asking for the window back.
    show_asked: Arc<AtomicBool>,
    /// Kept alive for as long as the window: dropping the icon removes it.
    /// With its menu's items, relabelled when the language changes.
    icon: Option<TrayMade>,
    /// The language the tray's words are in.
    tray_language: String,
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
    }

    /// Into the tray, remembering where it was.
    fn hide(&mut self, ctx: &egui::Context) {
        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
            self.hidden_at = Some(rect.min);
        }
        self.hidden_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }
}

impl eframe::App for Window {
    /// Runs while the window is hidden too, which is what lets the tray and
    /// a second launch bring it back.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
        if self.hide_frames > 0 {
            self.hide_frames -= 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint();
        }
        // Asked to show itself — dm opened again, or its Dock icon
        // clicked: a browser installed since dm started is registered now,
        // so "open dm once" works while dm runs (gap 131).
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
        }
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

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui);
        let files = std::mem::take(&mut self.app.drag_out);
        if !files.is_empty() {
            if let Err(e) = drag_out(frame, files) {
                self.app.said = e.into();
            }
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

/// The drag library starts a drag from a GTK window on Linux, and dm's
/// window is not one; said rather than silently doing nothing.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn drag_out(_frame: &eframe::Frame, _files: Vec<std::path::PathBuf>) -> Result<(), String> {
    Err("Dragging files out of dm is not available on Linux yet: right-click › Open folder".into())
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
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
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
    let items = vec![show.clone(), scheduler.clone(), quit.clone()];
    let menu = Menu::with_items(&[
        &show,
        &PredefinedMenuItem::separator(),
        &limiter,
        &scheduler,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .map_err(|e| e.to_string())?;
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
    let (limiter_id, scheduler_id) = (limiter.id().clone(), scheduler.id().clone());

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

fn main() {
    let args = shell::args();

    // Launched by a browser rather than a person: framed JSON on stdin and
    // stdout, no window, nothing else printed.
    if shell::browser_launch(&args) {
        host::run(&settings::home());
        return;
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
    match dm_desktop::autostart::at_launch(saved.start_on_login, login_item_turned_off_outside()) {
        dm_desktop::autostart::AtLaunch::TurnedOffOutside => {
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
        dm_desktop::autostart::AtLaunch::Follow(wanted) => {
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
    let handed: Arc<Mutex<std::collections::VecDeque<dm_shell::host::Request>>> = Arc::default();
    {
        let (watched, flag, repaint) =
            (queue.clone(), Arc::clone(&show_asked), Arc::clone(&repaint));
        let (asked, ask_flag, ask_repaint) = (
            Arc::clone(&handed),
            Arc::clone(&show_asked),
            Arc::clone(&repaint),
        );
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
        ));
        let queue = queue.clone();
        runtime.spawn(async move { queue.run().await });
    }

    let mut backend = Backend::new(runtime.handle().clone(), queue, saved, notices, platform);
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
        ])
        // The login item's launch opens nothing; it waits in the tray.
        .with_visible(!hidden);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Remembers where the window was and how big, across launches.
        persist_window: true,
        persistence_path: window_state_file(std::env::var_os("DM_HOME")),
        ..Default::default()
    };

    let result = eframe::run_native(
        "dm",
        options,
        Box::new(move |cc| {
            let _ = repaint.set(cc.egui_ctx.clone());
            // A click on the Dock icon of an app whose window is hidden asks
            // for it back; without this the click did nothing.
            #[cfg(target_os = "macos")]
            {
                let (flag, ctx) = (Arc::clone(&show_asked), cc.egui_ctx.clone());
                dock::on_reopen(move || {
                    flag.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                });
            }
            let (send, receive) = mpsc::channel();
            // The tray's words in the window's language.
            let language = backend.settings().language;
            dm_desktop::i18n::set(dm_desktop::i18n::chosen(&language));
            let built = start_tray(cc.egui_ctx.clone(), send);
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
                start_control(&mut app, port, &cc.egui_ctx);
            }
            Ok(Box::new(Window {
                app,
                tray: receive,
                has_tray,
                hide_frames: if hidden { 3 } else { 0 },
                hidden_until: hidden
                    .then(|| std::time::Instant::now() + std::time::Duration::from_secs(10)),
                quitting: false,
                show_asked,
                icon,
                tray_language: language,
                hidden_at: None,
                hidden_maximized: false,
                browsers_said,
            }))
        }),
    );
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
