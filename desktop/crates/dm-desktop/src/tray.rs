use super::*;

/// The tray's words in the language in force: its four menu items, then
/// its tooltip.
pub(super) fn tray_words() -> [&'static str; 5] {
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
pub(super) type TrayMade = (
    tray_icon::TrayIcon,
    Vec<tray_icon::menu::MenuItem>,
    tray_icon::menu::CheckMenuItem,
);

pub(super) fn tray(wake: egui::Context, send: mpsc::Sender<Tray>) -> Result<TrayMade, String> {
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
    // it is hidden, by it, and it was the process's number.
    let builder = TrayIconBuilder::new()
        .with_id("dm")
        .with_icon(icon)
        .with_tooltip(tooltip);
    // A name for panels that list items by it (its D-Bus title was empty); on a Mac a title is text beside the icon, so not there.
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
pub(super) fn start_tray(
    wake: egui::Context,
    send: mpsc::Sender<Tray>,
) -> Result<Option<TrayMade>, String> {
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
pub(super) fn tray_host() -> bool {
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
        // Listed is not shown: a watcher with no panel drawing its items
        // left a hidden window nothing could bring back. A
        // watcher that will not say is believed.
        let host = watcher
            .get_property::<bool>("IsStatusNotifierHostRegistered")
            .unwrap_or(true);
        Some(all && host)
    }
}

pub(super) fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "the tray service could not start".into())
}
