// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' Browser page, and on Windows who opens magnet links and
//! `.torrent` files.

use super::*;

pub(super) fn browser(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    extensions(o, p, cx);
    // Who opens magnet links and .torrent files: Windows keeps that, not
    // the browser.
    #[cfg(windows)]
    handlers(o, p, cx);
    let c = o.draft.browser.catch.clone();
    menu_items(p, cx, &c);
    catching(o, p, window, cx, &c);
    click_keys(p, &c);
    panel_and_sign_in(p, cx, &c);
}

/// Each browser with whether its extension has talked to dm, and Add where it has not.
fn extensions(o: &mut Options, p: &mut Page, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // The extension, for someone who said not now to the bar over the list,
    // or has a second browser: each browser with whether its extension has
    // talked to dm, and Add where it has not.
    p.section(t("Add dm to your browser"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for (n, (name, page, family)) in dm_desktop_base::project::extension_browsers()
        .into_iter()
        .enumerate()
    {
        // When its extension last spoke to dm, so one removed since is not taken for one that
        // is there (gaps row U24): "Connected" alone meant it ever had.
        let heard = o
            .heard
            .iter()
            .filter(|(b, _)| family.contains(&b.as_str()))
            .map(|(_, at)| *at)
            .max();
        let state = if let Some(at) = heard {
            let words = match at {
                Some(at) => tf(
                    "Connected · last heard {} ago",
                    &[&dm_desktop_base::format::span(
                        std::time::Duration::from_secs(now.saturating_sub(at).max(1)),
                    )],
                ),
                None => t("Connected").to_string(),
            };
            div()
                .text_color(theme.success)
                .child(screen.label(words))
                .into_any_element()
        } else {
            button(p, ("add-extension", n), t("Add"))
                .on_click(cx.listener(move |o, _, _, cx| o.open_page(page, cx)))
                .into_any_element()
        };
        // A setting's row: the browser at the leading edge, what it says at the trailing one.
        p.setting(name, true, state);
    }
    // Read again on asking, as an extension added or removed meanwhile changes it: under the
    // rows it reads again (it stood over them, before anything it acts on). Its own size, not
    // a bar the page's width (gaps row E31).
    p.add(div().flex().child(
        button(p, "check-browsers", t("Check now")).on_click(cx.listener(|o, _, _, cx| {
            o.heard = dm_shell::host::browsers_heard(&dm_shell::settings::home());
            cx.notify();
        })),
    ));
}

/// The items dm adds to the browser's right-click menu, each chosen.
fn menu_items(p: &mut Page, cx: &mut Context<Options>, c: &dm_protocol::CatchRules) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // The items dm adds to the browser's right-click menu, each chosen.
    // A heading over its switches, as "Add dm to your browser" is over its rows (seen in
    // ui-shot.py's picture: it stood as plain words).
    p.section(t("Items dm adds to the browser's right-click menu"));
    for (id, _) in dm_protocol::MENU_ITEMS {
        let words = match id {
            "dm-link" => t("Download link with dm"),
            "dm-media" => t("Download media with dm"),
            "dm-all-links" => t("Download all links with dm"),
            _ => t("Download selected links with dm"),
        };
        let item = native::switch(
            SharedString::from(format!("menu-{id}")),
            words.into(),
            c.shows_menu(id),
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            let c = &mut o.draft.browser.catch;
            let on = c.shows_menu(id);
            c.hidden_menus.retain(|h| h != id);
            if on {
                c.hidden_menus.push(id.to_string());
            }
            cx.notify();
        }));
        p.add(item);
    }
}

/// Whether dm takes the browser's downloads, which and from where.
fn catching(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    c: &dm_protocol::CatchRules,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // Its own heading: under the right-click menu's, which downloads dm takes read as one of
    // that menu's items. The help's guide to it has these words (translated already).
    p.section(t("Downloads from your browser"));
    check(
        p,
        cx,
        "catch-enabled",
        t("Take these file types from the browser"),
        c.enabled,
        true,
        |s| s.browser.catch.enabled = !s.browser.catch.enabled,
    );
    p.hint(t(
        "Off: the browser keeps every download it starts. Its right-click menu still sends links to dm.",
    ));
    let on = c.enabled;
    let types = input(o, window, cx, "types", "File types", WIDE, on, false, |o| {
        o.typed.types.clone()
    });
    p.labelled(t("File types"), on, types);
    p.hint_when(
        on,
        t("Separate types with spaces, no dots. Leave empty for all."),
    );
    let min = input(
        o,
        window,
        cx,
        "min_size_kb",
        "Ignore files smaller than (KB)",
        NUMBER,
        on,
        false,
        |o| o.draft.browser.catch.min_size_kb.to_string(),
    );
    p.setting(t("Ignore files smaller than (KB)"), on, min);
    // The browsers it takes downloads from, each ticked or not; an
    // unticked one keeps its downloads.
    p.add(native::enabled(
        on,
        div().child(screen.label(t("Take downloads from these browsers"))),
    ));
    for browser in dm_protocol::BROWSERS {
        let words = if browser == "Chromium" {
            t("Other Chromium browsers")
        } else {
            browser
        };
        let item = native::switch(
            SharedString::from(format!("browser-{browser}")),
            words.into(),
            c.takes_from(browser),
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            let c = &mut o.draft.browser.catch;
            let taking = c.takes_from(browser);
            c.off_browsers.retain(|b| !b.eq_ignore_ascii_case(browser));
            if taking {
                c.off_browsers.push(browser.to_string());
            }
            cx.notify();
        }));
        p.add(native::enabled(on, item));
    }
    let skip = input(
        o,
        window,
        cx,
        "skip",
        "Never take downloads from these sites",
        WIDE,
        on,
        false,
        |o| o.typed.skip.clone(),
    );
    p.labelled(t("Never take downloads from these sites"), on, skip);
    p.hint_when(
        on,
        t("Separate sites with spaces. example.com covers its subdomains."),
    );
    // A link that works once was spent by dm asking about it before
    // taking it.
    let once = input(
        o,
        window,
        cx,
        "once",
        "Sites whose links work only once",
        WIDE,
        on,
        false,
        |o| o.typed.once.clone(),
    );
    p.labelled(t("Sites whose links work only once"), on, once);
    p.hint_when(on, t("dm takes their downloads without first asking the server whether it can fetch them, which would use the link up. A link that needs the browser's sign-in then fails in dm rather than finishing in the browser."));
}

/// The keys a click on a link can be held with, to leave it with the browser or send it to dm.
fn click_keys(p: &mut Page, c: &dm_protocol::CatchRules) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // The keys a click on a link can be held with.
    let key_words = |key: &str| match key {
        "alt" => t("Alt").to_string(),
        "shift" => t("key:Shift").trim_start_matches("key:").to_string(),
        "ctrl" => t("Ctrl or Cmd").to_string(),
        _ => t("None").to_string(),
    };
    let keep_at = dm_protocol::CLICK_KEYS
        .iter()
        .position(|k| *k == c.keep_key);
    p.labelled(
        t("Hold while clicking to leave it with the browser"),
        true,
        popup::button(
            "keep-key",
            key_words(&c.keep_key).into(),
            dm_protocol::CLICK_KEYS
                .iter()
                .map(|k| key_words(k).into())
                .collect(),
            keep_at,
            theme,
            &screen,
        ),
    );
    let take_at = TAKE_KEYS.iter().position(|k| *k == c.take_key);
    p.labelled(
        t("Hold while clicking to send it to dm whatever its type or size"),
        true,
        popup::button(
            "take-key",
            key_words(&c.take_key).into(),
            TAKE_KEYS.iter().map(|k| key_words(k).into()).collect(),
            take_at,
            theme,
            &screen,
        ),
    );
    if !c.take_key.is_empty() && c.take_key == c.keep_key {
        p.add(native::note(
            t("The two keys are the same; holding it leaves the download with the browser."),
            theme.warning,
            &screen,
        ));
    }
}

/// The panel over web videos, and the browser's sign-in with every download.
fn panel_and_sign_in(p: &mut Page, cx: &mut Context<Options>, c: &dm_protocol::CatchRules) {
    check(
        p,
        cx,
        "video-panel",
        t("Show Download video on web videos"),
        c.video_panel,
        true,
        |s| s.browser.catch.video_panel = !s.browser.catch.video_panel,
    );
    p.hint(t(
        "A button over a video on a web page, to download it with dm.",
    ));
    check(
        p,
        cx,
        "sign-in-everywhere",
        t("Send the browser's sign-in with downloads"),
        c.sign_in_everywhere,
        true,
        |s| s.browser.catch.sign_in_everywhere = !s.browser.catch.sign_in_everywhere,
    );
    p.hint(t("Files that need you signed in come to dm too. Your sign-in goes with that one download and is forgotten after it. Off: only the sites you turn on in the extension's menu send it."));
    p.note(t("Your browser picks this up the next time it sends dm a download. Its right-click menu sends any link either way."));
}

/// Who opens magnet links and `.torrent` files on this computer (Windows),
/// as the Browser page says it: each with a button that makes it dm where
/// it is not, and one that gives it back to the program that had it. Read
/// beside the window as the page opens, and again after a change.
#[cfg(windows)]
fn handlers(o: &mut Options, p: &mut Page, cx: &mut Context<Options>) {
    use dm_shell::register::{self, Owner};
    let screen = p.screen.clone();
    if o.handlers.is_none() && !o.handlers_asked {
        o.handlers_asked = true;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async {
                    (
                        register::magnet_owner(register::run_reg),
                        register::torrent_owner(register::run_reg),
                    )
                })
                .await;
            let _ = this.update(cx, |o, cx| {
                o.handlers = Some(found);
                cx.notify();
            });
        })
        .detach();
    }
    let home = dm_shell::settings::home();
    for magnet in [true, false] {
        let owner = o.handlers.map(|(m, t)| if magnet { m } else { t });
        let backup = if magnet {
            register::magnet_backup(&home)
        } else {
            register::torrent_backup(&home)
        };
        let said = match owner {
            None => t("Looking…"),
            Some(Owner::Dm) => "dm",
            Some(Owner::Other) => t("another program"),
            Some(Owner::Nobody) => t("nothing yet"),
            Some(Owner::WindowsChoice) => t("the app chosen in Windows' Default apps"),
        };
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(native::LABEL_GAP))
            .child(screen.label(if magnet {
                t("Magnet links open with:")
            } else {
                t(".torrent files open with:")
            }))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(screen.label(said)),
            );
        if matches!(owner, Some(Owner::Other | Owner::Nobody)) {
            let take = if magnet {
                t("Use dm for magnet links")
            } else {
                t("Use dm for .torrent files")
            };
            let backup = backup.clone();
            row = row.child(
                button(p, ("take-handler", usize::from(magnet)), take).on_click(cx.listener(
                    move |o, _, _, cx| {
                        let done =
                            std::env::current_exe()
                                .map_err(|e| e.to_string())
                                .and_then(|exe| {
                                    if magnet {
                                        register::take_magnet_windows(
                                            &exe,
                                            &backup,
                                            register::run_reg,
                                        )
                                    } else {
                                        register::take_torrent_windows(
                                            &exe,
                                            &backup,
                                            register::run_reg,
                                        )
                                    }
                                });
                        o.handlers_changed(done, cx);
                    },
                )),
            );
        }
        if owner == Some(Owner::Dm) && backup.is_file() {
            let give = if magnet {
                t("Give magnet links back")
            } else {
                t("Give .torrent files back")
            };
            row = row.child(
                button(p, ("give-handler", usize::from(magnet)), give).on_click(cx.listener(
                    move |o, _, _, cx| {
                        let done = if magnet {
                            register::give_back_magnet_windows(&backup, register::run_reg)
                        } else {
                            register::give_back_torrent_windows(&backup, register::run_reg)
                        };
                        o.handlers_changed(done, cx);
                    },
                )),
            );
        }
        p.add(row);
    }
    p.note(t("dm keeps what another program had, and can give it back. Where Windows' Default apps has a choice, that choice decides: change it in Settings, Apps, Default apps."));
    // Windows' own chooser at a first magnet click is its question, not dm's.
    p.note(t("The first magnet link you click may make Windows ask which app opens it: choose dm there. dm can't answer for you."));
}
