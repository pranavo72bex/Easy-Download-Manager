use super::*;

mod tabs;
use tabs::*;

/// The settings in force now, with only the settings Options changed taken
/// from its draft: each top-level setting the draft holds differently from
/// when the dialog opened. Whatever else changed meanwhile stands.
pub fn changed_only(
    opened: &Settings,
    draft: &Settings,
    now: &Settings,
) -> Result<Settings, String> {
    let object = |s: &Settings| match serde_json::to_value(s) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        _ => Err("the settings could not be read to save them".to_string()),
    };
    let (opened, draft, mut merged) = (object(opened)?, object(draft)?, object(now)?);
    for (key, value) in draft {
        if opened.get(&key) != Some(&value) {
            merged.insert(key, value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(merged))
        .map_err(|e| format!("the settings could not be saved: {e}"))
}

pub(super) fn options(app: &mut DmApp, ctx: &egui::Context) {
    if app.dialogs.options.is_none() {
        return;
    }
    let queues = app.backend.queues();
    let platform = std::sync::Arc::clone(&app.backend.platform);
    // One size for every tab, so the dialog and its buttons stay where they
    // are when another tab is picked; a tab longer than that scrolls.
    // Sized from the first frame: a window that grew into its size over a
    // few frames moved under the pointer, centred as it is.
    // No taller than the window either: at 150% in the smallest window its
    // OK sat below the bottom edge.
    // Below the menu bar, as every dialog keeps.
    let menu_bottom = app.menu_bottom.max(0.0);
    let room_below = ctx.content_rect().height() - menu_bottom - 16.0;
    let tall = room_below.clamp(240.0, 680.0);
    let Some(d) = app.dialogs.options.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    // No wider than the window: at 200% text in a 960-point window the
    // tabs, the title and OK were off its edges.
    let wide = (ctx.content_rect().width() - sides(ctx)).clamp(320.0, 660.0);
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
            Tab::Connection => tab_connection(ui, d, &env),
            Tab::Proxy => tab_proxy(ui, d, &env),
            Tab::SiteLogins => tab_site_logins(ui, d, &env),
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
    if ok {
        // Split on ';' too: "xlsx;" was kept as a type.
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                .map(|w| w.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|w| !w.is_empty())
                .collect()
        };
        let mut wanted = d.draft.clone();
        wanted.catch.extensions = words(&d.types);
        wanted.catch.skip_hosts = sites(&words(&d.skip));
        wanted.catch.once_hosts = sites(&words(&d.once));
        wanted.proxy.bypass = words(&d.bypass);
        // A sign-in's host as the rules read it: an address pasted whole
        // matched nothing.
        for login in &mut wanted.site_logins {
            if let Some(host) = sites(&[login.host.trim().to_string()]).pop() {
                login.host = host;
            }
        }
        wanted.custom_categories = d
            .custom
            .iter()
            .filter(|[name, ..]| !name.trim().is_empty())
            .map(|[name, types, only]| dm_core::Custom {
                name: name.trim().to_string(),
                extensions: words(types),
                sites: sites(&words(only)),
            })
            .collect();
        // A category's name is its folder: one that is not one folder
        // would be saved and then sort nothing, without a word.
        // Only those this edit made or renamed: one an older dm saved is
        // left out as the settings load, and must not fail every OK after.
        // Held to every computer's rules, since settings travel; one saved
        // before that works here keeps working. The built-in kinds' folders
        // (Options › Folders) likewise, where this edit renamed one; empty
        // leaves that kind unsorted.
        // A proxy turned on with nowhere to connect to was taken, and every
        // download then went out directly without a word.
        {
            use dm_core::proxy::Kind;
            let p = &wanted.proxy;
            if p.enabled && p.kind != Kind::System {
                let host = p.host.trim();
                let why = if host.is_empty() {
                    Some(
                        t("Enter the proxy's host, or untick \"Connect through a proxy\".")
                            .to_string(),
                    )
                } else if host.contains("://")
                    || host
                        .chars()
                        .any(|c| c.is_whitespace() || "/@?#".contains(c))
                {
                    Some(tf(
                        "\"{}\" isn't a valid proxy host. Enter a name like proxy.example.com; the port goes in its own field.",
                        &[&host],
                    ))
                } else if p.port == 0 {
                    Some(t("A proxy needs a port from 1 to 65535.").to_string())
                } else {
                    None
                };
                if let Some(why) = why {
                    d.says = why;
                    d.tab = Tab::Proxy;
                    return;
                }
            }
        }
        let new_custom = wanted
            .custom_categories
            .iter()
            .map(|c| c.name.clone())
            .filter(|name| !d.opened.custom_categories.iter().any(|o| o.name == *name));
        let renamed_kinds = wanted
            .category_folders
            .iter()
            .enumerate()
            .filter(|(i, name)| {
                !name.trim().is_empty()
                    && d.opened.category_folders.get(*i).map(|o| o.trim()) != Some(name.trim())
            })
            .map(|(_, name)| name.trim().to_string());
        if let Some(bad) = new_custom
            .chain(renamed_kinds)
            .find(|name| !portable_folder(name))
        {
            d.says = tf(
                "\"{}\" cannot be a category: its name is also its folder, and that is not a folder name every computer can use (no / \\ : * ? \" < > |, no name like CON, nothing ending in a dot or space)",
                &[&bad],
            );
            d.tab = Tab::SaveTo;
            return;
        }
        let applied = changed_only(&d.opened, &wanted, &app.applied)
            .and_then(|wanted| app.apply_settings(wanted));
        match applied {
            Ok(()) => app.dialogs.options = None,
            Err(e) => {
                // Left open with what was typed, and why it was not taken:
                // reopening it on what is in force threw every edit away to
                // say what was wrong with one. OK compares against what it
                // opened on, so another try changes only what was changed.
                if let Some(d) = app.dialogs.options.as_mut() {
                    d.says = e;
                }
            }
        }
    }
}

#[cfg(test)]
mod changed_only_tests {
    use super::*;

    #[test]
    fn options_writes_only_what_it_changed() {
        let opened = Settings::default();
        // Options changed the connections.
        let mut draft = opened.clone();
        draft.connections = 4;
        // Meanwhile View › Language and the Scheduler changed others.
        let mut now = opened.clone();
        now.language = "de".into();
        now.schedule.enabled = true;
        let wanted = changed_only(&opened, &draft, &now).unwrap();
        assert_eq!(wanted.connections, 4, "Options' own change");
        assert_eq!(wanted.language, "de", "the language picked meanwhile");
        assert!(wanted.schedule.enabled, "the schedule set meanwhile");
    }
}
