// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' General page, its sections in the page's order, each adding its
//! rows under the one before.

use super::*;

pub(super) fn general(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let s = o.draft.clone();
    language(p, &s);
    appearance(o, p, window, cx, &s);
    starting(p, cx, &s);
    sounds(o, p, window, cx, &s);
    backdrop(p, cx, &s);
    pop_ups(p, cx, &s);
    keeping(p, cx, &s);
    the_list(p, cx, &s);
    settings_file(p, cx);
}

/// The language the windows speak.
fn language(p: &mut Page, s: &Settings) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let languages: Vec<SharedString> = dm_desktop_base::i18n::LANGUAGES
        .iter()
        .map(|(_, name)| SharedString::from(*name))
        .collect();
    let now = dm_desktop_base::i18n::chosen(&s.general.language);
    let at = dm_desktop_base::i18n::LANGUAGES
        .iter()
        .position(|(code, _)| *code == now);
    p.labelled(
        t("Language"),
        true,
        popup::button(
            "language",
            dm_desktop_base::i18n::name_of(now).into(),
            languages,
            at,
            theme,
            &screen,
        ),
    );
}

/// Appearance: the theme and the text size's slider.
fn appearance(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    s: &Settings,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    p.section(t("Appearance"));
    let theme_words = |word: &str| {
        options::THEMES
            .iter()
            .find(|(w, _)| *w == word)
            .map(|(_, words)| t(words))
            .unwrap_or_else(|| t("As the computer is"))
    };
    p.labelled(
        t("Theme"),
        true,
        popup::button(
            "theme",
            theme_words(&s.general.theme).into(),
            THEMES.iter().map(|w| theme_words(w).into()).collect(),
            THEMES.iter().position(|w| *w == s.general.theme),
            theme,
            &screen,
        ),
    );
    // The window's scale, a slider of four stops (60, 80, 100 and 120
    // percent; the maintainer's word), the one View › Larger
    // and Smaller and Ctrl (⌘) with + and − also step: it shows the scale in
    // force, a choice takes effect at once in every window, and it is kept
    // whatever OK or Cancel. Each stop's
    // percentage under the track picks it too, for the keyboard's and a
    // script's click.
    let slider = o.scale_slider(window, cx);
    let now = crate::window::zoom::factor();
    let sizes = dm_shell::settings::TEXT_SIZE;
    let stops = sizes.stops.iter().enumerate().map(|(i, stop)| {
        let stop = *stop;
        let on = sizes.index(now) == i;
        div()
            .id(("scale-stop", i))
            .role(gpui::Role::Button)
            // Centred under where the slider's thumb stands at this stop.
            .absolute()
            .top_0()
            .left(px(
                SCALE_WIDTH * i as f32 / (sizes.stops.len() - 1) as f32 - STOP_WIDTH / 2.0
            ))
            .w(px(STOP_WIDTH))
            .flex()
            .justify_center()
            .text_size(px(native::SMALL_TEXT))
            .text_color(if on { theme.text } else { theme.muted })
            .child(screen.label(dm_desktop_base::i18n::tf("{}%", &[&sizes.percent(stop)])))
            .on_click(cx.listener(move |o, _, _, cx| o.pick_scale(stop, cx)))
    });
    p.labelled(
        t("Text size"),
        true,
        div()
            .flex()
            .items_center()
            // Room for the end stops' percentages, which stand half outside
            // the track.
            .gap(px(STOP_WIDTH / 2.0))
            .child(
                div()
                    .ml(px(STOP_WIDTH / 2.0 - native::LABEL_GAP))
                    .w(px(SCALE_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(space::XXS))
                    .child(
                        gpui_component::slider::Slider::new(&slider)
                            .accessibility_label(t("Text size")),
                    )
                    .child(
                        div()
                            .relative()
                            .h(px(native::SMALL_TEXT * 1.5))
                            .children(stops),
                    ),
            )
            .child(
                div()
                    .min_w(px(40.0))
                    .child(screen.label(dm_desktop_base::i18n::tf("{}%", &[&sizes.percent(now)]))),
            ),
    );
}

/// Starting and updating: signing in, closing the window, an address
/// already listed, who may control dm, and updates.
fn starting(p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    let (theme, screen) = (p.theme, p.screen.clone());
    p.section(t("Starting and updating"));
    check(
        p,
        cx,
        "start-on-login",
        t("Start dm when I sign in"),
        s.general.start_on_login,
        true,
        |s| s.general.start_on_login = !s.general.start_on_login,
    );
    let close_words = [t("Ask me"), t("Keep running in the tray"), t("Exit dm")];
    let close_at = ON_CLOSE
        .iter()
        .position(|w| *w == s.general.on_close)
        .unwrap_or(0);
    p.labelled(
        t("When I close the window"),
        true,
        popup::button(
            "on-close",
            close_words[close_at].into(),
            close_words.iter().map(|w| SharedString::from(*w)).collect(),
            Some(close_at),
            theme,
            &screen,
        ),
    );
    // What a caught or typed address already in the list does, asked once
    // and remembered from the question's tick: undone here.
    let duplicate_words = [
        t("Ask me"),
        t("Show it"),
        t("Download again"),
        t("Replace the old file"),
    ];
    let duplicate_at = DUPLICATE
        .iter()
        .position(|w| *w == s.duplicate_choice)
        .unwrap_or(0);
    p.labelled(
        t("When an address is already in the list"),
        true,
        popup::button(
            "duplicate",
            duplicate_words[duplicate_at].into(),
            duplicate_words
                .iter()
                .map(|w| SharedString::from(*w))
                .collect(),
            Some(duplicate_at),
            theme,
            &screen,
        ),
    );
    // The same file as one already here, from another address or in the folder (gaps row
    // W18): asked, and remembered from the question's tick; undone here.
    let same_words = [t("Ask me"), t("Show it"), t("Download again")];
    let same_at = SAME_FILE
        .iter()
        .position(|w| *w == s.same_file_choice)
        .unwrap_or(0);
    p.labelled(
        t("When the same file is already here"),
        true,
        popup::button(
            "same-file",
            same_words[same_at].into(),
            same_words.iter().map(|w| SharedString::from(*w)).collect(),
            Some(same_at),
            theme,
            &screen,
        ),
    );
    check(
        p,
        cx,
        "allow-command-line",
        t("Allow the command line to control dm"),
        s.general.allow_command_line,
        true,
        |s| s.general.allow_command_line = !s.general.allow_command_line,
    );
    p.hint(t(
        "Lets other programs on this computer add and stop downloads.",
    ));
    check(
        p,
        cx,
        "allow-mcp",
        t("Allow AI assistants to control dm (MCP)"),
        s.general.allow_mcp,
        true,
        |s| s.general.allow_mcp = !s.general.allow_mcp,
    );
    p.hint(t(
        "Lets an AI assistant you run on this computer add and manage downloads.",
    ));
    check(
        p,
        cx,
        "check-updates",
        t("Check for updates once a week"),
        s.general.check_updates,
        true,
        |s| s.general.check_updates = !s.general.check_updates,
    );
}

/// Sounds: when one plays, and a file of one's own for each event.
fn sounds(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    s: &Settings,
) {
    p.section(t("Sounds"));
    check(
        p,
        cx,
        "sound-complete",
        t("Play a sound when a download finishes"),
        s.general.sound_on_complete,
        true,
        |s| s.general.sound_on_complete = !s.general.sound_on_complete,
    );
    check(
        p,
        cx,
        "sound-failure",
        t("Play a sound when a download fails"),
        s.general.sound_on_failure,
        true,
        |s| s.general.sound_on_failure = !s.general.sound_on_failure,
    );
    // A sound of one's own for each event, as reference A offers: empty
    // keeps the system's.
    p.note(t(
        "A sound file of your own for each, or empty for the usual sound (.wav).",
    ));
    for (n, event) in options::SOUND_EVENTS.iter().enumerate() {
        let key = format!("sounds.{event}");
        let path = s.sounds.get(*event).cloned().unwrap_or_default();
        let field = input(
            o,
            window,
            cx,
            &key,
            // The field's name in English: the field translates its own name, and the
            // translated words asked for again were reported as missing from every catalog.
            sound_english(event),
            260.0,
            true,
            false,
            {
                let path = path.clone();
                move |_| path
            },
        );
        let chosen_key = key.clone();
        let choose = button(p, ("sound-choose", n), t("Choose…")).on_click(cx.listener(
            move |_, _, _, cx| {
                let chosen_key = chosen_key.clone();
                crate::platform::services::ask_then(
                    crate::platform::services::Asked {
                        filter: Some(("WAV", &["wav"])),
                        ..Default::default()
                    },
                    cx,
                    move |o: &mut Options, file, cx| {
                        o.put(&chosen_key, &file.display().to_string());
                        o.forget_field(&chosen_key);
                        cx.notify();
                    },
                );
            },
        ));
        let has = !path.trim().is_empty();
        let play = native::enabled(
            has,
            button(p, ("sound-play", n), t("Play")).on_click(cx.listener(move |o, _, _, cx| {
                o.play(path.trim(), cx);
            })),
        );
        p.labelled(
            options::sound_words(event),
            true,
            div()
                .flex()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(field)
                .child(choose)
                .child(play),
        );
    }
}

/// The Mica backdrop's switch, under the sounds.
fn backdrop(p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    // Offered only where it can be drawn: Windows 11 22H2 and later. From
    // dm's next start, as the window is made see-through when it opens
    // (gaps row G11).
    if dm_desktop_base::mica::possible() {
        check(
            p,
            cx,
            "mica",
            t("Use the Mica backdrop"),
            s.general.mica,
            true,
            |s| s.general.mica = !s.general.mica,
        );
        p.hint(t(
            "Lets the desktop show faintly through the window, as Windows' own apps do.",
        ));
    }
}

/// Pop-ups: progress, a finished download, copied links, and the questions
/// before a download starts.
fn pop_ups(p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    p.section(t("Pop-ups"));
    check(
        p,
        cx,
        "show-progress",
        t("Show progress when a download starts"),
        s.general.show_progress,
        true,
        |s| s.general.show_progress = !s.general.show_progress,
    );
    p.hint(t(
        "Off: press Details on a download's row to open its progress.",
    ));
    check(
        p,
        cx,
        "show-complete",
        t("Show a message when a download finishes"),
        s.general.show_complete,
        true,
        |s| s.general.show_complete = !s.general.show_complete,
    );
    check(
        p,
        cx,
        "watch-clipboard",
        t("Offer to download links I copy"),
        s.general.watch_clipboard,
        true,
        |s| s.general.watch_clipboard = !s.general.watch_clipboard,
    );
    p.hint(t(
        "dm looks at what you copy and offers to download any link.",
    ));
    check(
        p,
        cx,
        "start-copied",
        t("Take a copied link at once, without offering it first"),
        s.start_copied,
        s.general.watch_clipboard,
        |s| s.start_copied = !s.start_copied,
    );
    check(
        p,
        cx,
        "ask-panel",
        t("Ask where to save videos"),
        s.general.ask_panel_downloads,
        true,
        |s| s.general.ask_panel_downloads = !s.general.ask_panel_downloads,
    );
    p.hint(t(
        "Shows Download details before a video from a page starts.",
    ));
    check(
        p,
        cx,
        "ask-caught",
        t("Ask where to save browser downloads"),
        s.general.ask_caught_downloads,
        true,
        |s| s.general.ask_caught_downloads = !s.general.ask_caught_downloads,
    );
    p.hint(t(
        "Shows Download details before a download from your browser starts.",
    ));
    // Reference A's "start downloading while the dialog is open".
    check(
        p,
        cx,
        "start-while-asking",
        t("Start downloading while Download details is open"),
        s.start_while_asking,
        true,
        |s| s.start_while_asking = !s.start_while_asking,
    );
    p.hint(t(
        "The file is fetched while you choose where it goes; Start now puts it there, Cancel removes it.",
    ));
    check(
        p,
        cx,
        "offer-new-link",
        t("Offer a new link when a download's link has expired"),
        s.general.offer_new_link,
        true,
        |s| s.general.offer_new_link = !s.general.offer_new_link,
    );
    p.hint(t(
        "Asks you to click the file again on its page, to get a link that works.",
    ));
}

/// Adding and keeping files: the server's dates.
fn keeping(p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    p.section(t("Adding and keeping files"));
    check(
        p,
        cx,
        "server-dates",
        t("Keep the file's date from the server"),
        s.general.server_dates,
        true,
        |s| s.general.server_dates = !s.general.server_dates,
    );
    p.hint(t(
        "A saved file gets the date the website gives it, not today's.",
    ));
}

/// The list: what double-clicking a download does.
fn the_list(p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    let (theme, screen) = (p.theme, p.screen.clone());
    p.section(t("The list"));
    p.words(t("Double-clicking a download"));
    for (word, words) in [
        ("properties", t("shows its properties")),
        ("open", t("opens the file")),
        ("folder", t("opens its folder")),
    ] {
        let radio = native::radio(
            SharedString::from(format!("double-{word}")),
            words.into(),
            s.general.double_click == word,
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            o.draft.general.double_click = word.to_string();
            cx.notify();
        }));
        p.add(radio);
    }
}

/// Settings file: saving the settings to a file and loading them back.
fn settings_file(p: &mut Page, cx: &mut Context<Options>) {
    // Settings to carry to another computer, never with a password; read
    // back in, they wait here for OK.
    p.section(t("Settings file"));
    let save = button(p, "settings-save", t("Save settings to a file…")).on_click(cx.listener(
        |o, _, _, cx| {
            o.choose_file(true, "dm-settings.json", cx, |o, path, cx| {
                o.says = Some(
                    match std::fs::write(&path, o.draft.exported()) {
                        Ok(()) => tf("Saved to {}, without any password.", &[&path.display()]),
                        Err(e) => tf("{} could not be written: {}", &[&path.display(), &e]),
                    }
                    .into(),
                );
                cx.notify();
            });
        },
    ));
    let load = button(p, "settings-load", t("Load settings from a file…")).on_click(cx.listener(
        |o, _, _, cx| {
            o.choose_file(false, "dm-settings.json", cx, |o, path, cx| {
                let read = std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| o.draft.imported(&text));
                o.says = Some(
                    match read {
                        Ok(read) => {
                            o.draft = read;
                            o.retype();
                            t("Loaded. Look them over, then OK to use them.").to_string()
                        }
                        Err(e) => tf("{} could not be read: {}", &[&path.display(), &e]),
                    }
                    .into(),
                );
                cx.notify();
            });
        },
    ));
    p.add(
        div()
            .flex()
            .flex_wrap()
            .gap(px(native::LABEL_GAP))
            .child(save)
            .child(load),
    );
}
