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

mod hidden;
mod login;
mod platform;
mod signals;
mod tray;
mod window;

use hidden::*;
use login::*;
use platform::*;
use signals::*;
use tray::*;
use window::*;

const ICON: &[u8] = include_bytes!("../icons/128x128.png");

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
    taskbar: dm_desktop::taskbar::Taskbar,
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
    /// restored size in a corner.
    hidden_maximized: bool,
    /// The last registration failure said in the window, so opening dm
    /// again does not say it again.
    browsers_said: Option<String>,
}

/// Where the window's size and place are kept: under a `DM_HOME` when one
/// is set, beside that run's own settings and queue, so a test's or a
/// screenshot's dm does not write the person's real window state; the
/// system's place for dm otherwise.
fn window_state_file(home: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    home.filter(|h| !h.is_empty())
        .map(|h| std::path::PathBuf::from(h).join("window.ron"))
}

/// Starts the control protocol and says where in dm's home.
fn start_control(app: &mut DmApp, port: u16, ctx: &egui::Context, assistants_only: bool) {
    let home = settings::home();
    let started = dm_desktop::control::fresh_token(&home).and_then(|token| {
        let (to_window, requests) = mpsc::channel();
        let wake = ctx.clone();
        let port =
            dm_desktop::control::listen(port, token, to_window, move || wake.request_repaint())?;
        dm_desktop::control::write_port(&home, port)?;
        Ok((port, requests))
    });
    match started {
        Ok((_, requests)) => {
            app.attach_control(requests);
            app.restrict_control_to_assistants(assistants_only);
        }
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

/// On Windows a release build is a GUI program with no console, so what
/// `--register` and `--unregister` print went nowhere when run from cmd or
/// PowerShell. Attached to the console of whatever started it,
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
    // opened the window.
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

    // A word that is no link, file or option is a typo, not a request to
    // open the window: say so and stop (`dm lst` used to open dm silently).
    let strays = shell::unrecognised(&args);
    if !strays.is_empty() {
        attach_console();
        for word in &strays {
            eprintln!("dm: {word:?} is not a link, a file or a command (see --help)");
        }
        std::process::exit(2);
    }

    let handed = shell::orders(&args);

    // One dm at a time. The lock is held for the life of this process by
    // living to the end of `main`.
    let headless = args.iter().any(|a| a == "--headless");
    let _running = match instance::claim(&settings::home()) {
        Ok(Some(lock)) => lock,
        // Registered first: this launch ends at the hand-off, and a browser
        // installed while dm ran was never told where dm is. The
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
        .and_then(|p| p.parse().ok())
        // Or the person's own setting (Options › General): the command line.
        .or_else(|| (saved.allow_command_line || saved.allow_mcp).then_some(0));
    // Open only because AI assistants are allowed (not asked for by
    // `--control`, `DM_CONTROL` or the command-line setting): then the port
    // answers assistants and nothing else, so the default does not hand the
    // command line to people who never turned it on.
    let assistants_only = !args.iter().any(|a| a == "--control")
        && std::env::var_os("DM_CONTROL").is_none()
        && !saved.allow_command_line;

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

    // The computer kept awake for as long as a download is running.
    {
        let (queue, handle) = (queue.clone(), runtime.handle().clone());
        dm_desktop::awake::watch(move || {
            handle
                .block_on(queue.list())
                .iter()
                .any(|row| row.state == dm_core::queue::State::Running)
        });
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
            start_control(&mut app, port, &ctx, assistants_only);
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
        // the size it was left at. The window sets the real
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
            start_control(&mut app, port, ctx, assistants_only);
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
            taskbar: dm_desktop::taskbar::Taskbar::default(),
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
