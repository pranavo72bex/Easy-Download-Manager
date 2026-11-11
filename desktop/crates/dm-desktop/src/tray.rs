// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// The tray's words in the language in force (`dm_desktop_base::tray`,
/// both windows).
pub(super) fn tray_words() -> [&'static str; 5] {
    dm_desktop_base::tray::words()
}

/// The tray icon: Show, Start queue and Stop all, the speed limiter and
/// Scheduler, and Quit.
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

    use dm_desktop::commands::{Command, Shown};
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
        // The limiter's own dialog, as the Downloads menu's Settings…
        Command::SpeedLimiterSettings,
    ]
    .into_iter()
    .map(|c| (c, MenuItem::new(c.title(), true, None)))
    .collect();
    // Start and stop the downloads without opening the window.
    let queue: Vec<(Command, MenuItem)> = [Command::StartQueue, Command::StopAll]
        .into_iter()
        .map(|c| (c, MenuItem::new(c.title(), true, None)))
        .collect();
    let speeds = Submenu::new(dm_desktop::i18n::t("Speed"), true);
    for (_, item) in &presets {
        let _ = speeds.append(item);
    }
    let mut items = vec![show.clone(), scheduler.clone(), quit.clone()];
    items.extend(presets.iter().map(|(_, item)| item.clone()));
    items.extend(queue.iter().map(|(_, item)| item.clone()));
    let menu = Menu::with_items(&[
        &show,
        &PredefinedMenuItem::separator(),
        &queue[0].1,
        &queue[1].1,
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
        .chain(queue.iter())
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
            // The dialog needs the window shown; the presets do not.
            Tray::Run(*preset, *preset == Command::SpeedLimiterSettings)
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
            dm_desktop_base::tray::keep_registered();
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

/// Whether something is showing tray icons now (`dm_desktop_base::tray::host_showing`).
pub(super) fn tray_host() -> bool {
    dm_desktop_base::tray::host_showing()
}

pub(super) fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "the tray service could not start".into())
}
