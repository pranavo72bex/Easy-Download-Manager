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
    /// Only when the setting and the system disagree. Registering writes the
    /// path of the running binary, so doing it on every launch would point
    /// the login item at whatever ran last — a debug build, the app on a
    /// mounted disk image. Not being able to read the state is fatal only for
    /// registering: a login item that cannot be read must still be one a
    /// person can get rid of.
    fn set_login_item(&self, wanted: bool) -> Result<(), String> {
        // Linux's entry is dm's own: see `autostart`.
        if cfg!(target_os = "linux") {
            return dm_desktop::autostart::set(wanted);
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let launcher = auto_launch::AutoLaunchBuilder::new()
            .set_app_name("dm")
            .set_app_path(&exe.to_string_lossy())
            // The login item launches into the tray, not onto the desktop.
            .set_args(&["--hidden"])
            .build()
            .map_err(|e| e.to_string())?;
        match launcher.is_enabled() {
            Ok(now) if now == wanted => return Ok(()),
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

    fn power(&self, action: dm_desktop::power::WhenDone) -> Result<(), String> {
        dm_desktop::power::run(action)
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

    fn choose_file(&self, save: bool, suggested: &str) -> Option<std::path::PathBuf> {
        let dialog = rfd::FileDialog::new().add_filter("Download lists", &["dmlist", "txt"]);
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
    _icon: Option<tray_icon::TrayIcon>,
}

impl Window {
    fn show(ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

impl eframe::App for Window {
    /// Runs while the window is hidden too, which is what lets the tray and
    /// a second launch bring it back.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(asked) = self.tray.try_recv() {
            match asked {
                Tray::Show => Self::show(ctx),
                Tray::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Tray::Run(command, show) => {
                    if show {
                        Self::show(ctx);
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
        if self.show_asked.swap(false, Ordering::Relaxed) {
            Self::show(ctx);
        }
        if let Some(until) = self.hidden_until {
            let now = std::time::Instant::now();
            if now < until {
                ctx.request_repaint_after(until - now);
            } else {
                self.hidden_until = None;
                if !(self.has_tray && tray_host()) {
                    Self::show(ctx);
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
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        self.app.logic(ctx);
        // Program › Exit, as the tray's Quit does.
        if std::mem::take(&mut self.app.quit_asked) {
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

/// The tray icon: Show, the speed limiter and Scheduler, and Quit.
///
/// Optional: a desktop may not expose a StatusNotifier host. In that case dm
/// says so in the window and closing it quits instead of hiding without a tray.
fn tray(wake: egui::Context, send: mpsc::Sender<Tray>) -> Result<tray_icon::TrayIcon, String> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let image = image::load_from_memory(ICON)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (w, h) = image.dimensions();
    let icon = tray_icon::Icon::from_rgba(image.into_raw(), w, h).map_err(|e| e.to_string())?;

    use dm_desktop::commands::Command;
    use tray_icon::menu::PredefinedMenuItem;
    let show = MenuItem::new("Show dm", true, None);
    let limiter = MenuItem::new("Speed limiter on/off", true, None);
    let scheduler = MenuItem::new("Scheduler…", true, None);
    let quit = MenuItem::new("Quit dm", true, None);
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

    TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("dm — downloads keep running here")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())
}

/// Starts the tray where its platform wants it, and says whether there is one.
///
/// On Linux, KSNI runs its D-Bus service on its own thread. Elsewhere the
/// tray is made on the window's thread, after the event loop has started,
/// which macOS requires.
fn start_tray(
    wake: egui::Context,
    send: mpsc::Sender<Tray>,
) -> Result<Option<tray_icon::TrayIcon>, String> {
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
        Some(all)
    }
}

fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "the tray service could not start".into())
}

/// Starts the control protocol and says where in dm's home.
fn start_control(app: &mut DmApp, port: u16, ctx: &egui::Context) {
    let home = settings::home();
    let started = dm_desktop::control::token(&home).and_then(|token| {
        let (to_window, requests) = mpsc::channel();
        let wake = ctx.clone();
        let port =
            dm_desktop::control::listen(port, token, to_window, move || wake.request_repaint())?;
        std::fs::write(home.join("control.port"), port.to_string())?;
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
    if let Some(code) = dm_shell::register::from_args(&args) {
        std::process::exit(code);
    }

    let handed = shell::orders(&args);

    // One dm at a time. The lock is held for the life of this process by
    // living to the end of `main`.
    let Some(_running) = instance::claim(&settings::home()) else {
        shell::hand_to_running(&settings::home(), &handed);
        return;
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("dm could not start: {e}");
            std::process::exit(1);
        }
    };
    let shell::Started {
        queue,
        settings: saved,
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
    // state the person last chose. Said in the window, not fatal.
    if let Err(why) = platform.set_login_item(saved.start_on_login) {
        notices.push(format!(
            "could not change start-on-login: {why}; start on login is not in force"
        ));
    }

    // Anything a browser handed over while the app was closed is waiting in
    // the inbox; anything that was mid-flight came back queued.
    let show_asked = Arc::new(AtomicBool::new(false));
    let repaint: Arc<std::sync::OnceLock<egui::Context>> = Arc::default();
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
    if args.iter().any(|a| a == "--headless") {
        let ctx = egui::Context::default();
        let mut app = DmApp::new(backend);
        app.headless = true;
        if let Some(port) = control {
            start_control(&mut app, port, &ctx);
        }
        while !app.quit_asked {
            app.logic(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        return;
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("dm")
        .with_app_id("dm")
        .with_inner_size([960.0, 660.0])
        .with_min_inner_size([720.0, 460.0])
        // The login item's launch opens nothing; it waits in the tray.
        .with_visible(!hidden);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Remembers where the window was and how big, across launches.
        persist_window: true,
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
            if !cfg!(debug_assertions) && std::env::var_os("DM_HOME").is_none() {
                if let Err(why) = dm_shell::register::ensure_this_copy() {
                    backend.note(format!(
                        "your browsers could not be told where dm is: {why}. Until they are, \
                         the extension leaves downloads in the browser."
                    ));
                }
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
                _icon: icon,
            }))
        }),
    );
    if let Err(e) = result {
        eprintln!("dm: the window could not be opened: {e}");
        // Said in the exit status too: a launcher, a login entry or a
        // script asking whether dm started must not be told it did.
        std::process::exit(1);
    }
}
