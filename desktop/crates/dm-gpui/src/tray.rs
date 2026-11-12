// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm in the menu bar's status area, as the egui window's tray: Show dm,
//! Add URL… and Add links from clipboard… (the window brought forward),
//! Start Queue and Stop All, the speed limiter (ticked while a limit is in
//! force) with its presets (the one in force ticked) and Settings… (B52),
//! the Scheduler, Exit dm; a
//! click on the icon shows the window. Its picture is a template image, as
//! the system's own status items are, so it takes the menu bar's colour.
//! The close button keeps dm here where Options says so (asked once: "Keep
//! running in the tray?"), and the window comes back from here, from the
//! Dock, or from a second launch. The words are dm's (`dm_desktop_base::tray`).

use dm_shell::app::commands::Command;
use gpui::{Context, Window};
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::list::Downloads;

/// What the tray asked of the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Asked {
    /// Show dm, or a click on the icon.
    Show,
    /// Exit dm.
    Quit,
    /// A command, and whether it needs the window shown (a dialog's).
    Run(Command, bool),
}

/// The tray, and the items whose words and tick follow the window.
pub struct Tray {
    icon: TrayIcon,
    /// Show dm, Scheduler…, Exit dm.
    items: [MenuItem; 3],
    limiter: CheckMenuItem,
    /// The Speed submenu, its title following the language.
    speeds: Submenu,
    /// The speed presets, the one in force ticked.
    presets: Vec<(Command, CheckMenuItem)>,
    /// The items that run a command (Add URL…, Start Queue, Settings…),
    /// named by it.
    commands: Vec<(Command, MenuItem)>,
    /// The tooltip said now.
    tip: String,
    /// The language its words are in.
    language: String,
}

impl Tray {
    /// The tray, each choice of its menu and a click on its icon sent to
    /// `asked`.
    pub fn new(asked: async_channel::Sender<Asked>) -> Result<Tray, String> {
        use dm_desktop_base::tray::{menu_words, PRESETS};
        let [show, limiter, scheduler, quit, tooltip] = dm_desktop_base::tray::words();
        let show = MenuItem::new(show, true, None);
        let limiter = CheckMenuItem::new(limiter, true, false, None);
        let scheduler = MenuItem::new(scheduler, true, None);
        let quit = MenuItem::new(quit, true, None);
        let command_item = |c: Command| (c, MenuItem::new(menu_words(c), true, None));
        // Add URL… and the clipboard's links, as the toolbar adds them
        // (gaps row U28).
        let adds: Vec<(Command, MenuItem)> = dm_desktop_base::tray::ADDS
            .into_iter()
            .map(command_item)
            .collect();
        // The speed presets, one click from the tray, the one in force
        // ticked (`follow`), and the limiter's own dialog, as Downloads ›
        // Speed limiter › Settings… (B52).
        let presets: Vec<(Command, CheckMenuItem)> = PRESETS
            .into_iter()
            .map(|c| (c, CheckMenuItem::new(menu_words(c), true, false, None)))
            .collect();
        let settings = command_item(Command::SpeedLimiterSettings);
        // Start and stop the downloads without opening the window.
        let queue: Vec<(Command, MenuItem)> = [Command::StartQueue, Command::StopAll]
            .into_iter()
            .map(command_item)
            .collect();
        let speeds = Submenu::new(dm_desktop_base::i18n::t("Speed"), true);
        for (_, item) in &presets {
            speeds.append(item).map_err(|e| e.to_string())?;
        }
        speeds.append(&settings.1).map_err(|e| e.to_string())?;
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
        let ids: Vec<(MenuId, Command)> = commands
            .iter()
            .map(|(c, item)| (item.id().clone(), *c))
            .chain(presets.iter().map(|(c, item)| (item.id().clone(), *c)))
            .collect();
        let to_menu = asked.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let asked = if event.id == show_id {
                Asked::Show
            } else if event.id == quit_id {
                Asked::Quit
            } else if event.id == limiter_id {
                Asked::Run(Command::SpeedLimiterToggle, false)
            } else if event.id == scheduler_id {
                Asked::Run(Command::Scheduler, true)
            } else if let Some((_, command)) = ids.iter().find(|(id, _)| *id == event.id) {
                // A dialog (Add URL…, Settings…) needs the window shown and
                // brought forward; the presets do not.
                Asked::Run(*command, dm_desktop_base::tray::shows_window(*command))
            } else {
                return;
            };
            let _ = to_menu.try_send(asked);
        }));
        // A left click on the icon is the one gesture everyone tries first;
        // the menu is on the right button.
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = asked.try_send(Asked::Show);
            }
        }));
        // One id across launches: the system remembers an item's place by it.
        let icon = TrayIconBuilder::new().with_id("dm");
        // macOS draws a template picture in the menu bar's own colour;
        // Windows and Linux show dm's own icon, as the egui window's tray.
        #[cfg(target_os = "macos")]
        let icon = icon.with_icon_templated(template_icon()?);
        #[cfg(not(target_os = "macos"))]
        let icon = icon.with_icon(app_icon()?);
        // A name for panels that list items by it: its D-Bus title was
        // empty (Linux); on a Mac a title is words beside the icon.
        #[cfg(target_os = "linux")]
        let icon = icon.with_title("dm");
        let icon = icon
            .with_tooltip(tooltip)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Tray {
            icon,
            items: [show, scheduler, quit],
            limiter,
            speeds,
            presets,
            commands,
            tip: tooltip.to_string(),
            language: dm_desktop_base::i18n::code().to_string(),
        })
    }

    /// Its words in the language in force, its tooltip `tip` (how many
    /// downloads and how fast, else what dm is doing there), its limiter's
    /// tick (while a limit is in force) and the preset in force at a limit
    /// of `speed_limit` KB/s, where they changed. A checked item the
    /// system ticks or unticks on a click is put back as the settings say.
    pub fn follow(&mut self, tip: String, speed_limit: u64) {
        use dm_desktop_base::tray::{menu_words, ticked};
        let language = dm_desktop_base::i18n::code();
        if language != self.language {
            self.language = language.to_string();
            let [show, limiter, scheduler, quit, _] = dm_desktop_base::tray::words();
            for (item, words) in self.items.iter().zip([show, scheduler, quit]) {
                item.set_text(words);
            }
            self.limiter.set_text(limiter);
            self.speeds.set_text(dm_desktop_base::i18n::t("Speed"));
            for (command, item) in &self.commands {
                item.set_text(menu_words(*command));
            }
            for (command, item) in &self.presets {
                item.set_text(menu_words(*command));
            }
            self.tip.clear();
        }
        if tip != self.tip {
            let _ = self.icon.set_tooltip(Some(&tip));
            self.tip = tip;
        }
        let limited = speed_limit > 0;
        if self.limiter.is_checked() != limited {
            self.limiter.set_checked(limited);
        }
        for (preset, item) in &self.presets {
            let on = ticked(*preset, speed_limit);
            if item.is_checked() != on {
                item.set_checked(on);
            }
        }
    }
}

/// dm's own icon, for a taskbar's tray (Windows, Linux).
#[cfg(not(target_os = "macos"))]
fn app_icon() -> Result<tray_icon::Icon, String> {
    const PNG: &[u8] = include_bytes!("../../dm-desktop/icons/32x32.png");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(PNG));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut rgba = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let frame = reader.next_frame(&mut rgba).map_err(|e| e.to_string())?;
    rgba.truncate(frame.buffer_size());
    tray_icon::Icon::from_rgba(rgba, frame.width, frame.height).map_err(|e| e.to_string())
}

/// The icon: an arrow down into a tray, black on clear, for the system to
/// draw in the menu bar's colour (a template image), 18 points at twice the
/// pixels.
#[cfg(target_os = "macos")]
fn template_icon() -> Result<tray_icon::Icon, String> {
    const SIDE: usize = 36;
    // Four samples a pixel's side, for smooth edges.
    const SAMPLES: usize = 4;
    let inside = |x: f32, y: f32| -> bool {
        // The arrow's stem and head.
        let stem = (16.4..=19.6).contains(&x) && (4.0..=19.0).contains(&y);
        let head = {
            // The triangle (10, 14), (26, 14), (18, 23).
            let (ax, ay, bx, by, cx, cy) = (10.0, 14.0, 26.0, 14.0, 18.0, 23.0);
            let side =
                |px: f32, py: f32, qx: f32, qy: f32| (qx - px) * (y - py) - (qy - py) * (x - px);
            let (d1, d2, d3) = (
                side(ax, ay, bx, by),
                side(bx, by, cx, cy),
                side(cx, cy, ax, ay),
            );
            (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
        };
        // The tray: its floor and its two sides.
        let floor = (5.0..=31.0).contains(&x) && (28.0..=31.0).contains(&y);
        let left = (5.0..=8.0).contains(&x) && (20.0..=31.0).contains(&y);
        let right = (28.0..=31.0).contains(&x) && (20.0..=31.0).contains(&y);
        stem || head || floor || left || right
    };
    let mut rgba = Vec::with_capacity(SIDE * SIDE * 4);
    for py in 0..SIDE {
        for px in 0..SIDE {
            let mut covered = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = px as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if inside(x, y) {
                        covered += 1;
                    }
                }
            }
            let alpha = (covered * 255 / (SAMPLES * SAMPLES)) as u8;
            rgba.extend_from_slice(&[0, 0, 0, alpha]);
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIDE as u32, SIDE as u32).map_err(|e| e.to_string())
}

impl Downloads {
    /// The tray made, its choices heard on the window's thread. Where the
    /// system will not make one, closing the window ends dm instead.
    pub(crate) fn start_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The window's tests (`harness.rs`) run off the main thread, where the system's tray
        // cannot be made, and have no system to put one in.
        if cfg!(test) {
            return;
        }
        let (asked, heard) = async_channel::unbounded::<Asked>();
        match Tray::new(asked) {
            Ok(tray) => {
                self.tray = Some(tray);
                // Registered again with a panel that starts after dm, or
                // restarts (Linux), as the egui window's tray is.
                dm_desktop_base::tray::keep_registered();
            }
            Err(e) => {
                self.problems.push(dm_desktop_base::tray::unavailable(&e));
                return;
            }
        }
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(asked) = heard.recv().await {
                let carried = cx.update(|window, cx| {
                    this.update(cx, |this, cx| this.tray_asked(asked, window, cx))
                        .is_ok()
                });
                if !matches!(carried, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// What the tray asked, carried out.
    fn tray_asked(&mut self, asked: Asked, window: &mut Window, cx: &mut Context<Self>) {
        match asked {
            Asked::Show => self.show_window(window, cx),
            Asked::Quit => {
                // Asked in the window, which is shown for it.
                self.show_window(window, cx);
                self.ask_quit(cx);
            }
            Asked::Run(command, shows) => {
                if shows {
                    self.show_window(window, cx);
                }
                self.run(command, cx);
            }
        }
    }

    /// The tray's tooltip and ticks, as the window has them now.
    pub(crate) fn tray_follow(&mut self) {
        let speed_limit = self.backend.settings().speed_limit;
        let tip = dm_desktop_base::tray::activity(&self.rows)
            .unwrap_or_else(|| dm_desktop_base::tray::words()[4].to_string());
        if let Some(tray) = self.tray.as_mut() {
            tray.follow(tip, speed_limit);
        }
    }

    /// The close button: kept in the tray, ended, or asked once which, as
    /// Options › General says ("When I close the window"). True where the
    /// window may close, as it does where there is no tray to keep dm in.
    pub(crate) fn close_asked(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.tray.is_none()
            || crate::syswindow::nowhere_to_hide(window)
            || !dm_desktop_base::tray::host_showing()
        {
            // Nowhere to keep dm, nowhere to put its window (a tiling
            // Wayland desktop, which ignores minimize), or nothing showing
            // the tray it would be kept in (a Linux desktop with no panel for
            // it): closing ends it, asked first while downloads run, as Exit
            // is, and as the egui window does.
            self.ask_quit(cx);
            return false;
        }
        match self.backend.settings().on_close.as_str() {
            "exit" => self.ask_quit(cx),
            "tray" => self.hide_window(window),
            _ => self.ask_close(cx),
        }
        false
    }

    /// "Keep running in the tray?", asked once: the answer is kept in
    /// Options, and carried out. Asked by the window's tests too, which
    /// have no tray.
    pub(crate) fn ask_close(&mut self, cx: &mut Context<Self>) {
        use dm_desktop_base::i18n::t;
        let asked = crate::question::Asked {
            title: t("Keep running in the tray?").into(),
            lines: vec![t("Keep dm in the tray and your downloads carry on, and your browser can still send it links. Or exit dm now. You can change this later in Options.").into()],
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            // Exit at the leading side, Keep running the default, as the
            // egui window's buttons read.
            buttons: vec![t("Exit").into(), t("Keep running").into()],
            default: 1,
            escape: 1,
        };
        let view = cx.entity().downgrade();
        let answer: crate::question::Answer = Box::new(move |button, _, cx| {
            let keep = button == 1;
            let _ = view.update(cx, |view, cx| {
                let mut wanted = view.backend.settings();
                wanted.on_close = if keep { "tray" } else { "exit" }.into();
                if let Err(e) = view.backend.set_settings(wanted) {
                    view.say(e);
                }
                if keep {
                    view.hide_main(cx);
                } else {
                    view.ask_quit(cx);
                }
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// The main window hidden in the tray, wherever this is asked from,
    /// after the update that asked.
    fn hide_main(&mut self, cx: &mut Context<Self>) {
        let main = self.main_window();
        self.hidden = true;
        cx.defer(move |cx| {
            let _ = main.update(cx, |_, window, _| hide(window));
        });
    }

    /// The window into the tray: put away, not closed, so it comes back as
    /// it was.
    pub(crate) fn hide_window(&mut self, window: &mut Window) {
        hide(window);
        self.hidden = true;
    }

    /// The window back from the tray, in front.
    pub(crate) fn show_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::syswindow::show(window);
        self.hidden = false;
        window.activate_window();
        cx.activate(true);
    }
}

use crate::syswindow::hide;
