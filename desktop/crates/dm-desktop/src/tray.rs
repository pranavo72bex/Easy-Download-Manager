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

/// The tray icon: Show, Add URL… and the clipboard's links, Start queue and
/// Stop all, the speed limiter with its presets, Scheduler, and Quit; with
/// its menu's items, relabelled when the language changes and ticked as the
/// speed limit stands.
///
/// Optional: a desktop may not expose a StatusNotifier host. In that case dm
/// says so in the window and closing it quits instead of hiding without a tray.
pub(super) struct TrayMade {
    /// Kept alive for as long as the window: dropping the icon removes it.
    pub(super) icon: tray_icon::TrayIcon,
    /// Show dm, Scheduler…, Exit dm.
    items: [tray_icon::menu::MenuItem; 3],
    /// Ticked while a limit is in force.
    limiter: tray_icon::menu::CheckMenuItem,
    /// The Speed submenu, its title following the language.
    speeds: tray_icon::menu::Submenu,
    /// The speed presets, the one in force ticked.
    presets: Vec<(
        dm_desktop::commands::Command,
        tray_icon::menu::CheckMenuItem,
    )>,
    /// The items that run a command (Add URL…, Start Queue, Settings…).
    commands: Vec<(dm_desktop::commands::Command, tray_icon::menu::MenuItem)>,
}

impl TrayMade {
    /// Every item's words in the language in force, and the tooltip said
    /// when nothing downloads.
    pub(super) fn relabel(&self) {
        use dm_desktop_base::tray::menu_words;
        let [show, limit, scheduler, quit, tooltip] = tray_words();
        for (item, word) in self.items.iter().zip([show, scheduler, quit]) {
            item.set_text(word);
        }
        self.limiter.set_text(limit);
        self.speeds.set_text(dm_desktop::i18n::t("Speed"));
        for (command, item) in &self.commands {
            item.set_text(menu_words(*command));
        }
        for (command, item) in &self.presets {
            item.set_text(menu_words(*command));
        }
        let _ = self.icon.set_tooltip(Some(tooltip));
    }

    /// The limiter ticked while a limit is in force, and the preset in force
    /// at a limit of `speed_limit` KB/s; an item the system ticked or
    /// unticked on a click is put back as the settings say.
    pub(super) fn tick(&self, speed_limit: u64) {
        let on = speed_limit > 0;
        if self.limiter.is_checked() != on {
            self.limiter.set_checked(on);
        }
        for (preset, item) in &self.presets {
            let on = dm_desktop_base::tray::ticked(*preset, speed_limit);
            if item.is_checked() != on {
                item.set_checked(on);
            }
        }
    }
}

pub(super) fn tray(wake: egui::Context, send: mpsc::Sender<Tray>) -> Result<TrayMade, String> {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, Submenu};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let image = image::load_from_memory(ICON)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (w, h) = image.dimensions();
    let icon = tray_icon::Icon::from_rgba(image.into_raw(), w, h).map_err(|e| e.to_string())?;

    use dm_desktop::commands::Command;
    use dm_desktop_base::tray::{menu_words, ADDS, PRESETS};
    use tray_icon::menu::PredefinedMenuItem;
    let [show, limiter, scheduler, quit, tooltip] = tray_words();
    let show = MenuItem::new(show, true, None);
    let limiter = CheckMenuItem::new(limiter, true, false, None);
    let scheduler = MenuItem::new(scheduler, true, None);
    let quit = MenuItem::new(quit, true, None);
    let command_item = |c: Command| (c, MenuItem::new(menu_words(c), true, None));
    // Add URL… and the clipboard's links, as the toolbar adds them (gaps
    // row U28).
    let adds: Vec<(Command, MenuItem)> = ADDS.into_iter().map(command_item).collect();
    // The speed presets, one click from the tray, the one in force ticked
    // (`TrayMade::tick`).
    let presets: Vec<(Command, CheckMenuItem)> = PRESETS
        .into_iter()
        .map(|c| (c, CheckMenuItem::new(menu_words(c), true, false, None)))
        .collect();
    // The limiter's own dialog, as the Downloads menu's Settings…
    let settings = command_item(Command::SpeedLimiterSettings);
    // Start and stop the downloads without opening the window.
    let queue: Vec<(Command, MenuItem)> = [Command::StartQueue, Command::StopAll]
        .into_iter()
        .map(command_item)
        .collect();
    let speeds = Submenu::new(dm_desktop::i18n::t("Speed"), true);
    for (_, item) in &presets {
        let _ = speeds.append(item);
    }
    let _ = speeds.append(&settings.1);
    let menu = Menu::with_items(&[
        &show,
        &PredefinedMenuItem::separator(),
        &adds[0].1,
        &adds[1].1,
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
    let commands: Vec<(Command, MenuItem)> =
        adds.into_iter().chain(queue).chain([settings]).collect();
    let preset_ids: Vec<(tray_icon::menu::MenuId, Command)> = commands
        .iter()
        .map(|(c, item)| (item.id().clone(), *c))
        .chain(presets.iter().map(|(c, item)| (item.id().clone(), *c)))
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
        } else if let Some((_, command)) = preset_ids.iter().find(|(id, _)| *id == event.id) {
            // A dialog (Add URL…, Settings…) needs the window shown and
            // brought forward; the presets do not.
            Tray::Run(*command, dm_desktop_base::tray::shows_window(*command))
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
        .map(|icon| TrayMade {
            icon,
            items: [show, scheduler, quit],
            limiter,
            speeds,
            presets,
            commands,
        })
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
