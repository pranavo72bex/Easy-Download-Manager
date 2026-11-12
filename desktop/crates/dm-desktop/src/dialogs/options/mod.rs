// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

mod tabs;
use tabs::*;

/// Only what Options changed, over what is in force (`dm_desktop_base::options`,
/// both windows).
pub use dm_desktop_base::options::changed_only;

pub(super) fn options(app: &mut DmApp, ctx: &egui::Context) {
    if app.dialogs.options.is_none() {
        return;
    }
    let queues = app.queues();
    let platform = std::sync::Arc::clone(&app.backend.platform);
    // One size for every tab, so the dialog and its buttons stay where they
    // are when another tab is picked; a tab longer than that scrolls.
    // Sized from the first frame: a window that grew into its size over a
    // few frames moved under the pointer, centred as it is.
    // No taller than the window either: at 150% in the smallest window its
    // OK sat below the bottom edge.
    // Below the menu bar, as every dialog keeps.
    let menu_bottom = app.menu_bottom.max(0.0);
    let room_below = super::room(ctx).height() - menu_bottom - 16.0;
    let tall = room_below.clamp(240.0, 680.0);
    let Some(d) = app.dialogs.options.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    // No wider than the window: at 200% text in a 960-point window the
    // tabs, the title and OK were off its edges.
    let wide = (super::room(ctx).width() - sides(ctx)).clamp(320.0, 660.0);
    // The settings of the tab picked, drawn in whichever layout fits.
    let settings = |ui: &mut egui::Ui, d: &mut OptionsDialog| {
        let fresh = std::mem::take(&mut d.fresh);
        // The newest row's first field, when one was just added.
        let first = |field: egui::Response, last: bool| {
            if fresh && last {
                field.request_focus();
                field.scroll_to_me(None);
            }
        };
        let env = TabEnv {
            queues: &queues,
            platform: &*platform,
            first: &first,
        };
        match d.tab {
            Tab::General => tab_general(ui, d, &env),
            Tab::FileTypes => tab_file_types(ui, d, &env),
            Tab::SaveTo => tab_save_to(ui, d, &env),
            Tab::Downloads => tab_downloads(ui, d, &env),
            Tab::Advanced => tab_advanced(ui, d, &env),
            Tab::SiteLogins => tab_site_logins(ui, d, &env),
        }
        // Each tab's own settings back to a new install's, in the draft: OK
        // keeps them, Cancel leaves everything as it was.
        if !tab_settings(d.tab).is_empty() {
            ui.add_space(16.0);
            if ui.button(t("Restore defaults")).clicked() {
                d.restore_defaults();
            }
        }
    };
    // Too short for the tabs, a few rows of settings and the buttons (200%
    // text in a 460-point window, 300% in 800): the dialog scrolls as one
    // page, its buttons at the end, rather than drawing them over the
    // settings.
    let short = room_below < 270.0;
    let mut frame = window(ctx, "Options").open(&mut open);
    frame = if short {
        frame
            .vscroll(true)
            .min_width(wide)
            .max_width(wide)
            .max_height((room_below - 56.0).max(120.0))
    } else {
        // Its body scrolls itself, between the tabs and the buttons.
        frame
            .vscroll(false)
            .fixed_size(egui::vec2(wide, tall - 60.0))
    };
    frame.show(ctx, |ui| {
        // At its top as it opens, in a window where the whole dialog
        // scrolls: asked of the tab, the request went to the tabs' own
        // sideways strip and the page stayed where it was left.
        // Its very top, title and all: the cursor is under the title now.
        if d.fresh_tab > 0 {
            let top = egui::Rect::from_min_size(ui.min_rect().min, egui::vec2(1.0, 1.0));
            ui.scroll_to_rect(top, Some(egui::Align::TOP));
        }
        // The tabs scroll sideways where they do not all fit, with a bar
        // that shows it: one shown only under the pointer left Sign-ins cut
        // off at 150% text with no sign of more. Wrapped on
        // to more rows, they left the page no room at 200%.
        let strip = egui::ScrollArea::horizontal().id_salt("options-tabs");
        let labels: Vec<&str> = TABS.iter().map(|(_, label)| t(label)).collect();
        // A few points kept spare: fitted exactly, the strip still drew
        // its scroll bar at 150% under tabs that did not move.
        let padding = tabs_padding(ui, &labels, ui.available_width() - 4.0);
        ui.scope(|ui| {
            ui.spacing_mut().scroll = crate::theme::steady_scroll();
            strip.show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in TABS {
                        let picked = selector(ui, d.tab == tab, t(label), padding);
                        if picked.clicked() {
                            // At its top, not where it was left: Folders
                            // opened with its first field scrolled out of
                            // sight.
                            if d.tab != tab {
                                d.fresh_tab = 2;
                            }
                            d.tab = tab;
                        }
                        // The tab in force in sight as the dialog opens: the
                        // strip kept the place it was scrolled to, General
                        // out of view.
                        // To the top: in a short window the dialog itself
                        // scrolls, and reopened where it was left, its tabs
                        // out of view.
                        if d.tab == tab && d.fresh_tab > 0 {
                            picked.scroll_to_me(Some(egui::Align::TOP));
                        }
                    }
                })
            });
        });
        let fresh = d.fresh_tab > 0;
        d.fresh_tab = d.fresh_tab.saturating_sub(1);
        ui.separator();
        if short {
            settings(ui, d);
            if !crate::theme::Tokens::get(ui.ctx()).modern {
                ui.separator();
            }
            if !d.says.is_empty() {
                ui.add(egui::Label::new(RichText::new(&d.says).color(warn(ui))).wrap());
            }
            footer(ui, |ui| {
                ok = primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
            return;
        }
        // The buttons first, from the window's foot up, with a warning
        // over them only when there is one; the tab's body takes exactly
        // the room left. A fixed 80 kept for them left two settings showing
        // at 720×460; a guess left a band under the body, and
        // a measured one lagged a frame behind a warning.
        let top = ui.cursor().top();
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            footer(ui, |ui| {
                ok = primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
            // The modern footer draws its own line.
            if !crate::theme::Tokens::get(ui.ctx()).modern {
                ui.separator();
            }
            if !d.says.is_empty() {
                ui.add(egui::Label::new(RichText::new(&d.says).color(warn(ui))).wrap());
            }
            // What is left between the tabs and the buttons.
            let body = (ui.cursor().bottom() - ui.spacing().item_spacing.y - top).max(24.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                // A bar that is always drawn where a tab is longer than the
                // room, rather than one that shows only under the pointer:
                // at 960 px General's last line was cut off with no sign of
                // more.
                ui.spacing_mut().scroll = crate::theme::steady_scroll();
                let mut area = egui::ScrollArea::vertical()
                    // Each tab its own place: one shared position opened
                    // Browser or Downloads part-way down.
                    .id_salt(("options-tab", d.tab))
                    .max_height(body)
                    .min_scrolled_height(body)
                    .auto_shrink([false, false]);
                // At its top as the dialog opens, wherever it was left.
                if fresh {
                    area = area.vertical_scroll_offset(0.0);
                }
                area.show(ui, |ui| settings(ui, d));
            });
        });
    });
    if cancel || !open {
        app.dialogs.options = None;
        return;
    }
    // The lists typed read in, held to the rules, and only what Options
    // changed kept over what is in force (`dm_desktop_base::options::ok`,
    // both windows).
    if ok {
        let typed = dm_desktop_base::options::Typed {
            types: d.types.clone(),
            skip: d.skip.clone(),
            once: d.once.clone(),
            bypass: d.bypass.clone(),
            custom: d.custom.clone(),
        };
        match dm_desktop_base::options::ok(&d.opened, &d.draft, &typed, &app.applied) {
            Err(refused) => {
                d.says = refused.says;
                if let Some(tab) = refused.tab {
                    d.tab = tab;
                }
                d.to_proxy |= refused.to_proxy;
            }
            // Left open with what was typed, and why it was not taken:
            // reopening it on what is in force threw every edit away to say
            // what was wrong with one. OK compares against what it opened
            // on, so another try changes only what was changed.
            Ok(wanted) => match app.apply_settings(wanted) {
                Ok(()) => app.dialogs.options = None,
                Err(e) => {
                    if let Some(d) = app.dialogs.options.as_mut() {
                        d.says = e;
                    }
                }
            },
        }
    }
}
