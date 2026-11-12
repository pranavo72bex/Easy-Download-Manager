// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' six pages (B43), each setting with the egui window's words and
//! the explanation under it, in the order the egui window has them: check
//! boxes, the system's pop-ups and fields, sections under bold headings. A
//! field's text goes into the draft as it is typed (`Options::put`); a
//! check box, a pop-up or a button changes the draft at once. On Windows,
//! as the egui window's: who opens magnet links and `.torrent` files
//! (Browser) and Dial Up / VPN (Advanced).

use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::options::{self, Tab};
use dm_shell::settings::Settings;
use gpui::{div, prelude::*, AnyElement, Context, SharedString, Window};

use crate::native;
use crate::options::Options;
use crate::popup::{self, Pick};
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// A field for a number.
const NUMBER: f32 = 90.0;
/// A field for a name or an address.
const WIDE: f32 = 300.0;
/// The scale slider's width, its stops' percentages under it, each in a
/// box this wide.
const SCALE_WIDTH: f32 = 200.0;
const STOP_WIDTH: f32 = 40.0;

/// Under a check box, its explanation lines up with its words.
const UNDER_CHECK: f32 = 20.0;

/// The looks this window draws: the computer's, light or dark (`classic`
/// is the egui window's own look, and here the computer's).
const THEMES: [&str; 3] = ["system", "light", "dark"];
/// What closing the window does, by the setting's word.
const ON_CLOSE: [&str; 3] = ["ask", "tray", "exit"];
/// What an address already listed does, by the setting's word.
const DUPLICATE: [&str; 4] = ["", "show", "again", "replace"];
/// The keys a click on a link can be held with ("" for none).
const TAKE_KEYS: [&str; 4] = ["", "alt", "shift", "ctrl"];
/// A proxy's kinds, in the pop-up's order.
const PROXY_KINDS: [dm_core::ProxyKind; 5] = [
    dm_core::ProxyKind::System,
    dm_core::ProxyKind::Http,
    dm_core::ProxyKind::Https,
    dm_core::ProxyKind::Socks4,
    dm_core::ProxyKind::Socks5,
];

/// A page being laid out: its rows, one under another.
struct Page {
    items: Vec<AnyElement>,
    /// The words of each item, lower case, for Options' search (gaps row U24): what the item
    /// says, given by whatever added it.
    words: Vec<String>,
    /// The words of the item about to be added.
    saying: String,
    theme: Theme,
    screen: Screen,
}

impl Page {
    fn add(&mut self, element: impl IntoElement) {
        self.items.push(element.into_any_element());
        self.words
            .push(std::mem::take(&mut self.saying).to_lowercase());
    }

    /// The next item added says `words`, for the search.
    fn says(&mut self, words: &str) {
        if !self.saying.is_empty() {
            self.saying.push(' ');
        }
        self.saying.push_str(words);
    }

    /// A section's heading, a little apart from what is above it.
    fn section(&mut self, words: &str) {
        self.says(words);
        let heading = div()
            .mt(px(10.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(self.screen.label(words.to_string()));
        self.add(heading);
    }

    /// What a check box does, under its words.
    fn hint(&mut self, words: &str) {
        self.says(words);
        let hint = div().pl(px(UNDER_CHECK)).child(native::note(
            words.to_string(),
            self.theme.muted,
            &self.screen,
        ));
        self.add(hint);
    }

    /// Small words of their own, at the page's edge.
    fn note(&mut self, words: &str) {
        self.says(words);
        let note = native::note(words.to_string(), self.theme.muted, &self.screen);
        self.add(note);
    }

    /// Plain words.
    fn words(&mut self, words: &str) {
        self.says(words);
        let label = div().child(self.screen.label(words.to_string()));
        self.add(label);
    }

    /// Words, then a control beside them; greyed while `on` is false.
    fn labelled(&mut self, words: &str, on: bool, control: impl IntoElement) {
        self.says(words);
        let row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(native::LABEL_GAP))
            .child(self.screen.label(words.to_string()))
            .child(control);
        self.add(native::enabled(on, row));
    }
}

/// A check box for one of the draft's switches: `flip` turns it.
fn check(
    p: &mut Page,
    cx: &mut Context<Options>,
    id: &'static str,
    words: &str,
    on: bool,
    enabled: bool,
    flip: fn(&mut Settings),
) {
    let check = native::checkbox(id, words.to_string().into(), on, p.theme, &p.screen).on_click(
        cx.listener(move |o, _, _, cx| {
            flip(&mut o.draft);
            cx.notify();
        }),
    );
    p.says(words);
    p.add(native::enabled(enabled, check));
}

/// The field `key`, `width` wide, holding what `value` gives as it is made.
#[allow(clippy::too_many_arguments)]
fn input(
    o: &mut Options,
    window: &mut Window,
    cx: &mut Context<Options>,
    key: &str,
    name: &'static str,
    width: f32,
    on: bool,
    masked: bool,
    value: impl FnOnce(&Options) -> String,
) -> AnyElement {
    let theme = Theme::of(window.appearance());
    let field = o.field(key, value, masked, window, cx);
    let not_a_number = o.not_numbers.contains(key);
    let screen = o.screen.clone();
    div()
        .w(px(width))
        .flex_none()
        .child(native::field_if(&field, name, on, theme))
        .when(not_a_number, |d| {
            d.child(native::note(
                t("Numbers only: the one before was kept"),
                theme.warning,
                &screen,
            ))
        })
        .into_any_element()
}

/// What `event` is called beside its sound, in English (`options::sound_words` says it in
/// the language in force).
fn sound_english(event: &str) -> &'static str {
    match event {
        "complete" => "Download complete",
        "failed" => "Download failed",
        "queue_started" => "Queue started",
        _ => "Queue finished",
    }
}

/// A plain button.
fn button(p: &Page, id: impl Into<gpui::ElementId>, words: &str) -> gpui::Stateful<gpui::Div> {
    native::button(id, words.to_string().into(), false, p.theme, &p.screen)
}

/// The page `o` shows, a row of settings each: laid out as a column by the view that scrolls
/// them, whose children they are, so it can scroll to one of them (`Options::to_proxy`).
pub(crate) fn page(
    o: &mut Options,
    theme: Theme,
    window: &mut Window,
    cx: &mut Context<Options>,
) -> Vec<AnyElement> {
    built(o, o.tab, theme, window, cx).items
}

/// The page of `tab`, with what each of its items says.
fn built(
    o: &mut Options,
    tab: Tab,
    theme: Theme,
    window: &mut Window,
    cx: &mut Context<Options>,
) -> Page {
    let mut p = Page {
        items: Vec::new(),
        words: Vec::new(),
        saying: String::new(),
        theme,
        screen: o.screen.clone(),
    };
    match tab {
        Tab::General => general(o, &mut p, window, cx),
        Tab::FileTypes => browser(o, &mut p, window, cx),
        Tab::SaveTo => folders(o, &mut p, window, cx),
        Tab::Downloads => downloads(o, &mut p, window, cx),
        Tab::Advanced => advanced(o, &mut p, window, cx),
        Tab::SiteLogins => sign_ins(o, &mut p, window, cx),
    }
    p
}

/// What Options' search finds (gaps row U24): every setting whose words hold `wanted`, on
/// whatever page, under its page's name, which a click opens.
pub(crate) fn search(
    o: &mut Options,
    wanted: &str,
    theme: Theme,
    window: &mut Window,
    cx: &mut Context<Options>,
) -> Vec<AnyElement> {
    let wanted = wanted.trim().to_lowercase();
    let mut found = Vec::new();
    for (tab, name) in options::TABS {
        let page = built(o, tab, theme, window, cx);
        let mut hits = page
            .items
            .into_iter()
            .zip(page.words)
            .filter(|(_, words)| words.contains(&wanted))
            .map(|(item, _)| item)
            .peekable();
        if hits.peek().is_none() {
            continue;
        }
        found.push(
            div()
                .id(SharedString::from(format!("found-{name}")))
                .mt(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.selection)
                .cursor_pointer()
                .child(o.screen.label(t(name).to_string()))
                .on_click(cx.listener(move |o, _, window, cx| {
                    o.search.update(cx, |s, cx| s.set_value("", window, cx));
                    o.show(tab, cx);
                }))
                .into_any_element(),
        );
        found.extend(hits);
    }
    if found.is_empty() {
        found.push(
            native::note(
                tf("No setting says “{}”.", &[&wanted]),
                theme.muted,
                &o.screen,
            )
            .into_any_element(),
        );
    }
    found
}

/// A choice made in a page's pop-up.
pub(crate) fn pick(o: &mut Options, pick: &Pick) {
    let s = &mut o.draft;
    let i = pick.index;
    match pick.field {
        "language" => {
            if let Some((code, _)) = dm_desktop_base::i18n::LANGUAGES.get(i) {
                s.language = code.to_string();
            }
        }
        "theme" => {
            if let Some(word) = THEMES.get(i) {
                s.theme = word.to_string();
            }
        }
        "on-close" => {
            if let Some(word) = ON_CLOSE.get(i) {
                s.on_close = word.to_string();
            }
        }
        "duplicate" => {
            if let Some(word) = DUPLICATE.get(i) {
                s.duplicate_choice = word.to_string();
            }
        }
        "keep-key" => {
            if let Some(key) = dm_protocol::CLICK_KEYS.get(i) {
                s.catch.keep_key = key.to_string();
            }
        }
        "take-key" => {
            if let Some(key) = TAKE_KEYS.get(i) {
                s.catch.take_key = key.to_string();
            }
        }
        "quality" => {
            if let Some((value, _)) = dm_desktop_base::file_info::QUALITIES.get(i) {
                s.stream_quality = value.to_string();
            }
        }
        "queue-limit" => {
            let offered = limit_queues(&o.queues, &o.draft, pick.item);
            if let (Some(queue), Some(limit)) =
                (offered.get(i), o.draft.queue_limits.get_mut(pick.item))
            {
                limit.queue = queue.clone();
            }
        }
        "queue-start" => {
            if let (Some(queue), Some(start)) =
                (o.queues.get(i), o.draft.queue_starts.get_mut(pick.item))
            {
                start.queue = queue.clone();
            }
        }
        "network" => {
            if i == 0 {
                s.second_path.clear();
            } else if let Some(found) = o.networks.get(i - 1) {
                s.second_path = found.name.clone();
            }
        }
        "dialup-connection" => {
            if i == 0 {
                s.dialup.connection.clear();
            } else if let Some(found) = known_connections().get(i - 1) {
                s.dialup.connection = found.clone();
            }
        }
        "proxy-kind" => {
            if let Some(kind) = PROXY_KINDS.get(i) {
                let p = &mut s.proxy;
                p.port = options::port_for_kind(p.kind, *kind, p.port);
                p.kind = *kind;
                // Its field made again with the port the kind gave it.
                o.forget_field("proxy.port");
            }
        }
        _ => {}
    }
}

/// The queues a queue limit's row may pick: its own, and those no other
/// limit has (one limit a queue: two for Main queue were both kept and the
/// last won without a word).
fn limit_queues(queues: &[String], s: &Settings, row: usize) -> Vec<String> {
    let own = s.queue_limits.get(row).map(|l| l.queue.clone());
    let mut offered: Vec<String> = queues
        .iter()
        .filter(|q| {
            own.as_ref() == Some(*q)
                || !s
                    .queue_limits
                    .iter()
                    .enumerate()
                    .any(|(j, l)| j != row && &l.queue == *q)
        })
        .cloned()
        .collect();
    if let Some(own) = own {
        if !offered.contains(&own) {
            offered.push(own);
        }
    }
    offered
}

/* ---------------------------------------------------------------- General */

fn general(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let s = o.draft.clone();
    let languages: Vec<SharedString> = dm_desktop_base::i18n::LANGUAGES
        .iter()
        .map(|(_, name)| SharedString::from(*name))
        .collect();
    let now = dm_desktop_base::i18n::chosen(&s.language);
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
            theme_words(&s.theme).into(),
            THEMES.iter().map(|w| theme_words(w).into()).collect(),
            THEMES.iter().position(|w| *w == s.theme),
            theme,
            &screen,
        ),
    );
    // The window's scale, a slider of four stops (60, 80, 100 and 120
    // percent; the maintainer's word of 2026-11-11), the one View › Larger
    // and Smaller and Ctrl (⌘) with + and − also step: it shows the scale in
    // force, a choice takes effect at once in every window, and it is kept
    // whatever OK or Cancel, as the egui window's Text size is. Each stop's
    // percentage under the track picks it too, for the keyboard's and a
    // script's click.
    let slider = o.scale_slider(window, cx);
    let now = crate::zoom::factor();
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
                    .gap(px(2.0))
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
    p.section(t("Starting and updating"));
    check(
        p,
        cx,
        "start-on-login",
        t("Start dm when I sign in"),
        s.start_on_login,
        true,
        |s| s.start_on_login = !s.start_on_login,
    );
    let close_words = [t("Ask me"), t("Keep running in the tray"), t("Exit dm")];
    let close_at = ON_CLOSE.iter().position(|w| *w == s.on_close).unwrap_or(0);
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
    check(
        p,
        cx,
        "allow-command-line",
        t("Allow the command line to control dm"),
        s.allow_command_line,
        true,
        |s| s.allow_command_line = !s.allow_command_line,
    );
    p.hint(t(
        "Lets other programs on this computer add and stop downloads.",
    ));
    check(
        p,
        cx,
        "allow-mcp",
        t("Allow AI assistants to control dm (MCP)"),
        s.allow_mcp,
        true,
        |s| s.allow_mcp = !s.allow_mcp,
    );
    p.hint(t(
        "Lets an AI assistant you run on this computer add and manage downloads.",
    ));
    check(
        p,
        cx,
        "check-updates",
        t("Check for updates once a week"),
        s.check_updates,
        true,
        |s| s.check_updates = !s.check_updates,
    );
    p.section(t("Sounds"));
    check(
        p,
        cx,
        "sound-complete",
        t("Play a sound when a download finishes"),
        s.sound_on_complete,
        true,
        |s| s.sound_on_complete = !s.sound_on_complete,
    );
    check(
        p,
        cx,
        "sound-failure",
        t("Play a sound when a download fails"),
        s.sound_on_failure,
        true,
        |s| s.sound_on_failure = !s.sound_on_failure,
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
                crate::services::ask_then(
                    crate::services::Asked {
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
    // Offered only where it can be drawn: Windows 11 22H2 and later. From
    // dm's next start, as the window is made see-through when it opens
    // (gaps row G11).
    if dm_desktop_base::mica::possible() {
        check(
            p,
            cx,
            "mica",
            t("Use the Mica backdrop"),
            s.mica,
            true,
            |s| s.mica = !s.mica,
        );
        p.hint(t(
            "Lets the desktop show faintly through the window, as Windows' own apps do.",
        ));
    }
    p.section(t("Pop-ups"));
    check(
        p,
        cx,
        "show-progress",
        t("Show progress when a download starts"),
        s.show_progress,
        true,
        |s| s.show_progress = !s.show_progress,
    );
    p.hint(t(
        "Off: press Details on a download's row to open its progress.",
    ));
    check(
        p,
        cx,
        "show-complete",
        t("Show a message when a download finishes"),
        s.show_complete,
        true,
        |s| s.show_complete = !s.show_complete,
    );
    check(
        p,
        cx,
        "watch-clipboard",
        t("Offer to download links I copy"),
        s.watch_clipboard,
        true,
        |s| s.watch_clipboard = !s.watch_clipboard,
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
        s.watch_clipboard,
        |s| s.start_copied = !s.start_copied,
    );
    check(
        p,
        cx,
        "ask-panel",
        t("Ask where to save videos"),
        s.ask_panel_downloads,
        true,
        |s| s.ask_panel_downloads = !s.ask_panel_downloads,
    );
    p.hint(t(
        "Shows Download details before a video from a page starts.",
    ));
    check(
        p,
        cx,
        "ask-caught",
        t("Ask where to save browser downloads"),
        s.ask_caught_downloads,
        true,
        |s| s.ask_caught_downloads = !s.ask_caught_downloads,
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
        s.offer_new_link,
        true,
        |s| s.offer_new_link = !s.offer_new_link,
    );
    p.hint(t(
        "Asks you to click the file again on its page, to get a link that works.",
    ));
    p.section(t("Adding and keeping files"));
    check(
        p,
        cx,
        "server-dates",
        t("Keep the file's date from the server"),
        s.server_dates,
        true,
        |s| s.server_dates = !s.server_dates,
    );
    p.hint(t(
        "A saved file gets the date the website gives it, not today's.",
    ));
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
            s.double_click == word,
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            o.draft.double_click = word.to_string();
            cx.notify();
        }));
        p.add(radio);
    }
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

/* ---------------------------------------------------------------- Browser */

fn browser(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // The extension, for someone who said not now to the bar over the list,
    // or has a second browser: each browser with whether its extension has
    // talked to dm, and Add where it has not.
    p.words(t("Add dm to your browser:"));
    // Read again on asking, as an extension added or removed meanwhile changes it.
    p.add(
        button(p, "check-browsers", t("Check now")).on_click(cx.listener(|o, _, _, cx| {
            o.heard = dm_shell::host::browsers_heard(&dm_shell::settings::home());
            cx.notify();
        })),
    );
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
        p.add(
            div()
                .flex()
                .items_center()
                .gap(px(24.0))
                .child(div().w(px(160.0)).child(screen.label(name)))
                .child(state),
        );
    }
    // Who opens magnet links and .torrent files: Windows keeps that, not
    // the browser.
    #[cfg(windows)]
    handlers(o, p, cx);
    let c = o.draft.catch.clone();
    // The items dm adds to the browser's right-click menu, each chosen.
    p.words(t("Items dm adds to the browser's right-click menu"));
    for (id, _) in dm_protocol::MENU_ITEMS {
        let words = match id {
            "dm-link" => t("Download link with dm"),
            "dm-media" => t("Download media with dm"),
            "dm-all-links" => t("Download all links with dm"),
            _ => t("Download selected links with dm"),
        };
        let item = native::checkbox(
            SharedString::from(format!("menu-{id}")),
            words.into(),
            c.shows_menu(id),
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            let c = &mut o.draft.catch;
            let on = c.shows_menu(id);
            c.hidden_menus.retain(|h| h != id);
            if on {
                c.hidden_menus.push(id.to_string());
            }
            cx.notify();
        }));
        p.add(item);
    }
    check(
        p,
        cx,
        "catch-enabled",
        t("Take these file types from the browser"),
        c.enabled,
        true,
        |s| s.catch.enabled = !s.catch.enabled,
    );
    p.hint(t(
        "Off: the browser keeps every download it starts. Its right-click menu still sends links to dm.",
    ));
    let on = c.enabled;
    let types = input(
        o,
        window,
        cx,
        "types",
        "File types",
        480.0,
        on,
        false,
        |o| o.typed.types.clone(),
    );
    p.labelled(t("File types"), on, types);
    p.add(native::enabled(
        on,
        native::note(
            t("Separate types with spaces, no dots. Leave empty for all."),
            theme.muted,
            &screen,
        ),
    ));
    let min = input(
        o,
        window,
        cx,
        "min_size_kb",
        "Ignore files smaller than (KB)",
        NUMBER,
        on,
        false,
        |o| o.draft.catch.min_size_kb.to_string(),
    );
    p.labelled(t("Ignore files smaller than (KB)"), on, min);
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
        let item = native::checkbox(
            SharedString::from(format!("browser-{browser}")),
            words.into(),
            c.takes_from(browser),
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            let c = &mut o.draft.catch;
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
        480.0,
        on,
        false,
        |o| o.typed.skip.clone(),
    );
    p.labelled(t("Never take downloads from these sites"), on, skip);
    p.add(native::enabled(
        on,
        native::note(
            t("Separate sites with spaces. example.com covers its subdomains."),
            theme.muted,
            &screen,
        ),
    ));
    // A link that works once was spent by dm asking about it before
    // taking it.
    let once = input(
        o,
        window,
        cx,
        "once",
        "Sites whose links work only once",
        480.0,
        on,
        false,
        |o| o.typed.once.clone(),
    );
    p.labelled(t("Sites whose links work only once"), on, once);
    p.add(native::enabled(
        on,
        native::note(
            t("dm takes their downloads without first asking the server whether it can fetch them, which would use the link up. A link that needs the browser's sign-in then fails in dm rather than finishing in the browser."),
            theme.muted,
            &screen,
        ),
    ));
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
    check(
        p,
        cx,
        "video-panel",
        t("Show Download video on web videos"),
        c.video_panel,
        true,
        |s| s.catch.video_panel = !s.catch.video_panel,
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
        |s| s.catch.sign_in_everywhere = !s.catch.sign_in_everywhere,
    );
    p.hint(t("Files that need you signed in come to dm too. Your sign-in goes with that one download and is forgotten after it. Off: only the sites you turn on in the extension's menu send it."));
    p.note(t("Your browser picks this up the next time it sends dm a download. Its right-click menu sends any link either way."));
}

/* ---------------------------------------------------------------- Folders */

/// A folder's field and Browse… beside it; the folder picked goes into the
/// draft by `set`, and the field is made again from it.
#[allow(clippy::too_many_arguments)]
fn folder(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    key: &'static str,
    label: &'static str,
    on: bool,
    value: impl FnOnce(&Options) -> String,
    set: fn(&mut Options, String),
) {
    let field = input(o, window, cx, key, label, 330.0, on, false, value);
    let browse = button(p, SharedString::from(format!("browse-{key}")), t("Browse…")).on_click(
        cx.listener(move |o, _, _, cx| {
            o.choose_folder(cx, move |o, folder, cx| {
                set(o, folder);
                o.forget_field(key);
                cx.notify();
            });
        }),
    );
    p.add(native::enabled(
        on,
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(p.screen.label(t(label)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(field)
                    .child(browse),
            ),
    ));
}

fn folders(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    folder(
        o,
        p,
        window,
        cx,
        "download_dir",
        "Download folder",
        true,
        |o| o.draft.download_dir.display().to_string(),
        |o, folder| o.draft.download_dir = folder.into(),
    );
    folder(
        o,
        p,
        window,
        cx,
        "parts_dir",
        "Folder for unfinished downloads",
        true,
        |o| o.draft.parts_dir.clone(),
        |o, folder| o.draft.parts_dir = folder,
    );
    p.note(t("A download already begun keeps the folder it began in."));
    let on = o.draft.categorize;
    check(
        p,
        cx,
        "categorize",
        t("Sort finished files into folders by kind"),
        on,
        true,
        |s| s.categorize = !s.categorize,
    );
    // Why what follows is greyed.
    if !on {
        p.note(t(
            "Turn on sorting to set the kinds' folders and your own categories.",
        ));
    }
    // Each kind's folder: the kind in the window's language, the folder a
    // name on disk, as it is.
    let kinds = dm_core::Category::ALL;
    for pair in kinds.chunks(2) {
        let mut row = div().flex().gap(px(16.0));
        for kind in pair {
            let i = kinds.iter().position(|k| k == kind).unwrap_or(0);
            let name = dm_desktop_base::i18n::name(&dm_desktop_base::format::titled(
                &dm_shell::app::names::category_word(*kind),
            ));
            let field = input(
                o,
                window,
                cx,
                &format!("category_folders.{i}"),
                "Category",
                170.0,
                on,
                false,
                move |o| o.draft.category_folders.get(i).cloned().unwrap_or_default(),
            );
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(
                        div()
                            .w(px(90.0))
                            .flex()
                            .justify_end()
                            .child(screen.label(name)),
                    )
                    .child(field),
            );
        }
        p.add(native::enabled(on, row));
    }
    p.add(native::enabled(
        on,
        div().child(screen.label(t("Your own categories come first. A file of these types, from these sites (any site when empty), goes into a folder named after the category."))),
    ));
    for i in 0..o.typed.custom.len() {
        let name = input(
            o,
            window,
            cx,
            &format!("custom.{i}.0"),
            "Category",
            130.0,
            on,
            false,
            move |o| o.typed.custom[i][0].clone(),
        );
        let types = input(
            o,
            window,
            cx,
            &format!("custom.{i}.1"),
            "Types",
            150.0,
            on,
            false,
            move |o| o.typed.custom[i][1].clone(),
        );
        let sites = input(
            o,
            window,
            cx,
            &format!("custom.{i}.2"),
            "Sites",
            150.0,
            on,
            false,
            move |o| o.typed.custom[i][2].clone(),
        );
        let delete = button(p, ("delete-category", i), t("Delete category")).on_click(cx.listener(
            move |o, _, _, cx| {
                if i < o.typed.custom.len() {
                    o.typed.custom.remove(i);
                    o.forget("custom");
                }
                cx.notify();
            },
        ));
        p.add(native::enabled(
            on,
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Category")))
                .child(name)
                .child(screen.label(t("Types")))
                .child(types)
                .child(screen.label(t("Sites")))
                .child(sites)
                .child(delete),
        ));
    }
    let add =
        button(p, "add-category", t("Add a category")).on_click(cx.listener(|o, _, _, cx| {
            o.typed.custom.push(Default::default());
            cx.notify();
        }));
    p.add(native::enabled(on, div().flex().child(add)));
    let _ = theme;
}

/* -------------------------------------------------------------- Downloads */

fn downloads(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let connections = input(
        o,
        window,
        cx,
        "connections",
        "Connections per file",
        NUMBER,
        true,
        false,
        |o| o.draft.connections.to_string(),
    );
    p.labelled(t("Connections per file"), true, connections);
    p.note(t("How many parts of a file dm fetches at once. More is faster on most sites, but some limit it."));
    // The presets (B56): one sets the box; the one matching it is chosen.
    let now = o.draft.connections;
    let presets = options::CONNECTION_PRESETS.iter().map(|(words, count)| {
        let count = *count;
        let on = now == count as usize;
        div()
            .id(SharedString::from(format!("preset-{count}")))
            .role(gpui::Role::RadioButton)
            .aria_label(SharedString::from(t(words)))
            .aria_toggled(if on {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .px(px(8.0))
            .h(px(20.0))
            .flex()
            .items_center()
            .rounded(px(native::RADIUS))
            .text_size(px(native::SMALL_TEXT))
            .when(on, |d| d.bg(theme.accent).text_color(theme.on_accent))
            .when(!on, |d| d.bg(theme.control))
            .child(screen.label(t(words)))
            .on_click(cx.listener(move |o, _, _, cx| {
                o.draft.connections = count as usize;
                o.forget_field("connections");
                cx.notify();
            }))
    });
    p.add(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .child(native::note(t("Presets:"), theme.muted, &screen))
            .children(presets),
    );
    let files = input(
        o,
        window,
        cx,
        "concurrency",
        "Files at once",
        NUMBER,
        true,
        false,
        |o| o.draft.concurrency.to_string(),
    );
    p.labelled(t("Files at once"), true, files);
    p.note(t("How many files download at the same time."));
    let qualities = dm_desktop_base::file_info::QUALITIES;
    let quality_at = qualities
        .iter()
        .position(|(v, _)| *v == o.draft.stream_quality);
    let quality_words = quality_at
        .map(|i| t(qualities[i].1))
        .unwrap_or_else(|| t("Best available"));
    p.labelled(
        t("Video quality"),
        true,
        popup::button(
            "quality",
            quality_words.into(),
            qualities
                .iter()
                .map(|(_, l)| SharedString::from(t(l)))
                .collect(),
            quality_at,
            theme,
            &screen,
        ),
    );
    let s = o.draft.clone();
    check(
        p,
        cx,
        "start-queue-on-launch",
        t("Start the Main queue when dm opens"),
        s.start_queue_on_launch,
        true,
        |s| s.start_queue_on_launch = !s.start_queue_on_launch,
    );
    p.hint(t(
        "Downloads waiting in the Main queue start without a click.",
    ));
    check(
        p,
        cx,
        "subtitles",
        t("Embed subtitles in videos from player pages"),
        s.subtitles,
        true,
        |s| s.subtitles = !s.subtitles,
    );
    p.hint(t(
        "A video's subtitles are saved inside its file, to turn on in your player.",
    ));
    check(
        p,
        cx,
        "remux-ts",
        t("Save streams that arrive as .ts as MP4"),
        s.remux_ts,
        true,
        |s| s.remux_ts = !s.remux_ts,
    );
    p.hint(t(
        "Streamed videos are saved as MP4 files, which more players open.",
    ));
    // Queues with their own limit, one limit a queue.
    p.words(t("Queues with their own limit:"));
    for i in 0..s.queue_limits.len() {
        let offered = limit_queues(&o.queues, &s, i);
        let queue = s.queue_limits[i].queue.clone();
        let at = offered.iter().position(|q| *q == queue);
        let pick = popup::button_of(
            "queue-limit",
            i,
            dm_desktop_base::i18n::name(&queue).into(),
            offered
                .iter()
                .map(|q| dm_desktop_base::i18n::name(q).into())
                .collect(),
            at,
            theme,
            &screen,
        );
        let files = input(
            o,
            window,
            cx,
            &format!("queue_limits.{i}.files"),
            "Files at once",
            NUMBER,
            true,
            false,
            move |o| o.draft.queue_limits[i].files.to_string(),
        );
        let delete = button(p, ("delete-queue-limit", i), t("Delete queue limit")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.queue_limits.len() {
                    o.draft.queue_limits.remove(i);
                    o.forget("queue_limits");
                }
                cx.notify();
            }),
        );
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Queue")))
                .child(pick)
                .child(screen.label(t("Files at once")))
                .child(files)
                .child(delete),
        );
    }
    let unlimited = o
        .queues
        .iter()
        .find(|q| !s.queue_limits.iter().any(|l| &l.queue == *q))
        .cloned();
    let add_limit = button(p, "add-queue-limit", t("Add a queue limit")).on_click(cx.listener(
        move |o, _, _, cx| {
            if let Some(queue) = unlimited.clone() {
                o.draft
                    .queue_limits
                    .push(dm_shell::settings::QueueLimit { queue, files: 1 });
            }
            cx.notify();
        },
    ));
    p.add(native::enabled(
        o.queues
            .iter()
            .any(|q| !s.queue_limits.iter().any(|l| &l.queue == q)),
        div().flex().child(add_limit),
    ));
    // Queues that start on their own, each at its own time.
    p.words(t("Queues that start at a set time:"));
    for i in 0..s.queue_starts.len() {
        let queue = s.queue_starts[i].queue.clone();
        let pick = popup::button_of(
            "queue-start",
            i,
            dm_desktop_base::i18n::name(&queue).into(),
            o.queues
                .iter()
                .map(|q| dm_desktop_base::i18n::name(q).into())
                .collect(),
            o.queues.iter().position(|q| *q == queue),
            theme,
            &screen,
        );
        let at = input(
            o,
            window,
            cx,
            &format!("queue_starts.{i}.at"),
            "Starts at (HH:MM)",
            70.0,
            true,
            false,
            move |o| o.draft.queue_starts[i].at.clone(),
        );
        let bad = !dm_core::is_time(&o.draft.queue_starts[i].at);
        let delete = button(p, ("delete-start", i), t("Delete start time")).on_click(cx.listener(
            move |o, _, _, cx| {
                if i < o.draft.queue_starts.len() {
                    o.draft.queue_starts.remove(i);
                    o.forget("queue_starts");
                }
                cx.notify();
            },
        ));
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Queue")))
                .child(pick)
                .child(screen.label(t("Starts at (HH:MM)")))
                .child(at)
                .when(bad, |d| {
                    d.child(native::note(t("Not a time"), theme.warning, &screen))
                })
                .child(delete),
        );
    }
    let add_start =
        button(p, "add-start", t("Add a start time")).on_click(cx.listener(|o, _, _, cx| {
            let queue = o
                .queues
                .first()
                .cloned()
                .unwrap_or_else(|| dm_core::MAIN_QUEUE.to_string());
            o.draft.queue_starts.push(dm_shell::settings::QueueStart {
                queue,
                at: "02:00".into(),
            });
            cx.notify();
        }));
    p.add(div().flex().child(add_start));
    folder(
        o,
        p,
        window,
        cx,
        "watch_folder",
        "Watch a folder: links (.url, .webloc or a text file of links) and .torrent files saved there are added, then moved to its \"done\" folder. Leave it empty to turn this off.",
        true,
        |o| o.draft.watch_folder.clone(),
        |o, folder| o.draft.watch_folder = folder,
    );
    let record = input(
        o,
        window,
        cx,
        "record_minutes",
        "Stop recording after (minutes, 0 for no limit)",
        NUMBER,
        true,
        false,
        |o| o.draft.record_minutes.to_string(),
    );
    p.labelled(
        t("Stop recording after (minutes, 0 for no limit)"),
        true,
        record,
    );
    let recheck = input(
        o,
        window,
        cx,
        "recheck_hours",
        "Recheck every (hours, 0 for manual)",
        NUMBER,
        true,
        false,
        |o| o.draft.recheck_hours.to_string(),
    );
    p.labelled(t("Recheck every (hours, 0 for manual)"), true, recheck);
    p.words(t("When a download finishes, run this command (a virus checker or a script). The file's path is added at the end:"));
    let command = input(
        o,
        window,
        cx,
        "on_complete",
        "Command",
        400.0,
        true,
        false,
        |o| o.draft.on_complete.clone(),
    );
    p.labelled(t("Command"), true, command);
}

/* --------------------------------------------------------------- Advanced */

fn advanced(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    p.section(t("Network"));
    for (key, words, hint) in [
        (
            "speed_limit",
            "Speed limit, KB/s (0 for none)",
            "Keeps dm from using all of your connection.",
        ),
        (
            "attempts",
            "Attempts per connection (0 to keep trying)",
            "How many times dm tries again after a connection fails.",
        ),
        (
            "retry_wait",
            "Wait between attempts (seconds, 0 for automatic)",
            "How long dm waits before it tries again.",
        ),
        (
            "read_timeout",
            "Time out a silent connection after (seconds)",
            "How long a connection may say nothing before dm gives up on it.",
        ),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            NUMBER,
            true,
            false,
            move |o| match key {
                "speed_limit" => o.draft.speed_limit.to_string(),
                "attempts" => o.draft.attempts.to_string(),
                "retry_wait" => o.draft.retry_wait.to_string(),
                _ => o.draft.read_timeout.to_string(),
            },
        );
        p.labelled(t(words), true, field);
        p.note(t(hint));
    }
    // A second network beside the one the system routes by.
    o.networks = dm_core::interfaces();
    let network_words = |o: &Options| -> Vec<SharedString> {
        std::iter::once(SharedString::from(t("None")))
            .chain(o.networks.iter().map(|found| {
                match dm_core::system_name(&found.name) {
                    Some(named) => format!("{named} ({})", found.name),
                    None => format!("{} ({})", found.name, found.address),
                }
                .into()
            }))
            .collect()
    };
    let second = o.draft.second_path.clone();
    let at = if second.is_empty() {
        Some(0)
    } else {
        o.networks
            .iter()
            .position(|n| n.name == second)
            .map(|i| i + 1)
    };
    p.labelled(
        t("Second network"),
        true,
        popup::button(
            "network",
            if second.is_empty() {
                t("None").to_string()
            } else {
                second.clone()
            }
            .into(),
            network_words(o),
            at,
            theme,
            &screen,
        ),
    );
    p.note(t("Use a second connection too, such as your phone over USB or its hotspot. Half of each download's connections go over it. Not used while a proxy is on."));
    // How to have a phone there, step by step, shown on a click.
    let phone = o.phone;
    p.add(
        div()
            .id("phone-steps")
            .role(gpui::Role::Button)
            .aria_label(SharedString::from(t("How to use your phone")))
            .aria_expanded(phone)
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_color(theme.accent)
            .child(screen.label(if phone { "▾" } else { "▸" }))
            .child(screen.label(t("How to use your phone")))
            .on_click(cx.listener(|o, _, _, cx| {
                o.phone = !o.phone;
                cx.notify();
            })),
    );
    if phone {
        for step in [
            t("1. Connect your phone to this computer with its USB cable. If it asks whether to trust this computer, say yes."),
            t("2. Turn on its hotspot: on an iPhone, Settings › Personal Hotspot; on Android, USB tethering."),
            t("3. Choose it above. Keep Wi-Fi as this computer's own connection, so only downloads use the phone's data."),
        ] {
            p.hint(step);
        }
    }
    // Sites with their own connection limit (B57).
    p.words(t("Sites with their own connection limit:"));
    for i in 0..o.draft.host_limits.len() {
        let host = input(
            o,
            window,
            cx,
            &format!("host_limits.{i}.host"),
            "Site",
            200.0,
            true,
            false,
            move |o| o.draft.host_limits[i].host.clone(),
        );
        let count = input(
            o,
            window,
            cx,
            &format!("host_limits.{i}.connections"),
            "Connections",
            NUMBER,
            true,
            false,
            move |o| o.draft.host_limits[i].connections.to_string(),
        );
        let delete = button(p, ("delete-site-limit", i), t("Delete site limit")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.host_limits.len() {
                    o.draft.host_limits.remove(i);
                    o.forget("host_limits");
                }
                cx.notify();
            }),
        );
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Site")))
                .child(host)
                .child(screen.label(t("Connections")))
                .child(count)
                .child(delete),
        );
    }
    let add_limit =
        button(p, "add-site-limit", t("Add a site limit")).on_click(cx.listener(|o, _, _, cx| {
            o.draft.host_limits.push(dm_core::HostLimit {
                host: String::new(),
                connections: 1,
            });
            cx.notify();
        }));
    p.add(div().flex().child(add_limit));
    // Sites with their own folder and speed.
    p.words(t("Sites with their own folder and speed:"));
    for i in 0..o.draft.site_rules.len() {
        let host = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.host"),
            "Site",
            160.0,
            true,
            false,
            move |o| o.draft.site_rules[i].host.clone(),
        );
        let rule_folder = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.folder"),
            "Folder",
            200.0,
            true,
            false,
            move |o| o.draft.site_rules[i].folder.clone(),
        );
        let browse = button(p, ("site-rule-browse", i), t("Browse…")).on_click(cx.listener(
            move |o, _, _, cx| {
                o.choose_folder(cx, move |o, folder, cx| {
                    if let Some(rule) = o.draft.site_rules.get_mut(i) {
                        rule.folder = folder;
                    }
                    o.forget_field(&format!("site_rules.{i}.folder"));
                    cx.notify();
                });
            },
        ));
        let speed = input(
            o,
            window,
            cx,
            &format!("site_rules.{i}.speed"),
            "Speed limit, KB/s (0 for none)",
            NUMBER,
            true,
            false,
            move |o| o.draft.site_rules[i].speed.to_string(),
        );
        let delete = button(p, ("delete-site-rule", i), t("Delete site rule")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.site_rules.len() {
                    o.draft.site_rules.remove(i);
                    o.forget("site_rules");
                }
                cx.notify();
            }),
        );
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Site")))
                .child(host)
                .child(screen.label(t("Folder")))
                .child(rule_folder)
                .child(browse)
                .child(screen.label(t("Speed limit, KB/s (0 for none)")))
                .child(speed)
                .child(delete),
        );
    }
    let add_rule =
        button(p, "add-site-rule", t("Add a site rule")).on_click(cx.listener(|o, _, _, cx| {
            o.draft
                .site_rules
                .push(dm_shell::settings::SiteRule::default());
            cx.notify();
        }));
    p.add(div().flex().child(add_rule));
    p.words(t(
        "For downloads that need a sign-in: a cookies.txt from your browser.",
    ));
    let cookies = input(
        o,
        window,
        cx,
        "cookies_file",
        "Cookies file",
        260.0,
        true,
        false,
        |o| o.draft.cookies_file.clone(),
    );
    let choose = button(p, "cookies-choose", t("Choose…")).on_click(cx.listener(|_, _, _, cx| {
        crate::services::ask_then(
            crate::services::Asked {
                filter: Some(("cookies.txt", &["txt"])),
                ..Default::default()
            },
            cx,
            |o: &mut Options, file, cx| {
                o.put("cookies_file", &file.display().to_string());
                o.forget_field("cookies_file");
                cx.notify();
            },
        );
    }));
    let has_cookies = !o.draft.cookies_file.is_empty();
    let clear = button(p, "cookies-clear", t("Clear")).on_click(cx.listener(|o, _, _, cx| {
        o.put("cookies_file", "");
        o.forget_field("cookies_file");
        cx.notify();
    }));
    p.labelled(
        t("Cookies file"),
        true,
        div()
            .flex()
            .items_center()
            .gap(px(native::LABEL_GAP))
            .child(cookies)
            .child(choose)
            .when(has_cookies, |d| d.child(clear)),
    );
    // The name dm gives servers first, and the switch for dm's own (B51).
    p.words(t("dm identifies itself to servers as:"));
    let agent = if o.draft.identify_as_dm {
        dm_core::USER_AGENT
    } else {
        dm_core::IE_AGENT
    };
    p.add(
        div()
            .font_family(crate::native::mono(cx))
            .text_size(px(native::SMALL_TEXT))
            .text_color(theme.muted)
            .child(screen.label(agent)),
    );
    check(
        p,
        cx,
        "identify-as-dm",
        t("Identify as dm only"),
        o.draft.identify_as_dm,
        true,
        |s| s.identify_as_dm = !s.identify_as_dm,
    );
    p.hint(t(
        "Sends dm's own name first. A site that refuses it is asked again with another name.",
    ));
    // Where a proxy that cannot be one was said, the page shows it.
    let proxy_at = p.items.len();
    p.section(t("Proxy"));
    proxy(o, p, window, cx);
    if std::mem::take(&mut o.to_proxy) {
        // Its heading at the top of the page, its settings under it.
        o.scroll.scroll_to_top_of_item(proxy_at);
    }
    // A dial-up or VPN connection every download waits for, as the egui
    // window's, in reference A's words and order (contract B43). Windows
    // only: the connections are the ones Windows knows.
    if cfg!(windows) {
        dialup(o, p, window, cx);
    }
    p.section(t("BitTorrent"));
    let blocklist = input(
        o,
        window,
        cx,
        "peer_blocklist",
        "Block the peers listed at this address (optional)",
        360.0,
        true,
        false,
        |o| o.draft.peer_blocklist.clone(),
    );
    p.labelled(
        t("Block the peers listed at this address (optional)"),
        true,
        blocklist,
    );
    p.note(t("A list in the P2P format, plain or gzipped, on the web or on this computer. One on the web is fetched through the proxy and read again once a day; a torrent stops, saying why, if the list cannot be read."));
    // Off unless turned on: sharing sends from the person's connection.
    let share = o.draft.torrent_share;
    check(
        p,
        cx,
        "torrent-share",
        t("Keep sharing a finished torrent"),
        share,
        true,
        |s| s.torrent_share = !s.torrent_share,
    );
    p.hint(t("Its files stay where they are, and other peers are sent them until it has sent the times its size below, or for the minutes below, whichever comes first. Off, a torrent stops once its files are whole."));
    for (key, words) in [
        ("torrent_share_ratio", "Until it has sent (times its size)"),
        ("torrent_share_minutes", "For at most (minutes)"),
        (
            "torrent_upload_limit",
            "Send at most (KB/s, 0 for no limit)",
        ),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            NUMBER,
            share,
            false,
            move |o| match key {
                "torrent_share_ratio" => o.draft.torrent_share_ratio.to_string(),
                "torrent_share_minutes" => o.draft.torrent_share_minutes.to_string(),
                _ => o.draft.torrent_upload_limit.to_string(),
            },
        );
        p.labelled(t(words), share, field);
    }
    p.note(t("The first torrent makes Windows ask whether dm may use public and private networks. Allowed, other peers can reach dm and torrents go faster; refused, dm still downloads, reaching peers itself. Change it later in Windows Security, Firewall, Allow an app through firewall."));
}

/// The proxy's settings, under Advanced's "Proxy".
fn proxy(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    use dm_core::ProxyKind as Kind;
    let (theme, screen) = (p.theme, p.screen.clone());
    let proxy = o.draft.proxy.clone();
    check(
        p,
        cx,
        "proxy-enabled",
        t("Connect through a proxy"),
        proxy.enabled,
        true,
        |s| s.proxy.enabled = !s.proxy.enabled,
    );
    p.hint(t("For a network that reaches the internet through a proxy server. Whoever runs the network can give you its address."));
    let on = proxy.enabled;
    let kind_words = |kind: Kind| match kind {
        Kind::System => t("The computer's proxy settings").to_string(),
        other => format!("{other:?}").to_uppercase(),
    };
    p.labelled(
        t("Proxy type"),
        on,
        popup::button(
            "proxy-kind",
            kind_words(proxy.kind).into(),
            PROXY_KINDS.iter().map(|k| kind_words(*k).into()).collect(),
            PROXY_KINDS.iter().position(|k| *k == proxy.kind),
            theme,
            &screen,
        ),
    );
    let own = on && proxy.kind != Kind::System;
    for (key, words, width, masked) in [
        ("proxy.host", "Host", 260.0, false),
        ("proxy.port", "Port", NUMBER, false),
        ("proxy.username", "Username, if it needs one", 200.0, false),
        ("proxy.password", "Password", 200.0, true),
    ] {
        let field = input(o, window, cx, key, words, width, own, masked, move |o| {
            let p = &o.draft.proxy;
            match key {
                "proxy.host" => p.host.clone(),
                "proxy.port" => p.port.to_string(),
                "proxy.username" => p.username.clone(),
                _ => p.password.clone(),
            }
        });
        p.labelled(t(words), own, field);
    }
    let bypass = input(
        o,
        window,
        cx,
        "bypass",
        "Not for these sites (fetched directly)",
        260.0,
        own,
        false,
        |o| o.typed.bypass.clone(),
    );
    p.labelled(t("Not for these sites (fetched directly)"), own, bypass);
    // A proxy of its own for one kind of download; the rest go through the
    // one above.
    for (scheme, words) in [
        ("http", t("A proxy of its own for HTTP downloads")),
        ("https", t("A proxy of its own for HTTPS downloads")),
    ] {
        let has = proxy.per_scheme.contains_key(scheme);
        let toggle = native::checkbox(
            SharedString::from(format!("per-scheme-{scheme}")),
            words.into(),
            has,
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            let p = &mut o.draft.proxy;
            if p.per_scheme.remove(scheme).is_none() {
                p.per_scheme.insert(
                    scheme.to_string(),
                    dm_core::SchemeProxy {
                        kind: Kind::Http,
                        host: String::new(),
                        port: 8080,
                        username: String::new(),
                        password: String::new(),
                    },
                );
            }
            o.forget("per_scheme");
            cx.notify();
        }));
        p.add(native::enabled(own, toggle));
        if has {
            let host = input(
                o,
                window,
                cx,
                &format!("per_scheme.{scheme}.host"),
                "Its host",
                220.0,
                own,
                false,
                move |o| {
                    o.draft
                        .proxy
                        .per_scheme
                        .get(scheme)
                        .map(|e| e.host.clone())
                        .unwrap_or_default()
                },
            );
            let port = input(
                o,
                window,
                cx,
                &format!("per_scheme.{scheme}.port"),
                "Its port",
                NUMBER,
                own,
                false,
                move |o| {
                    o.draft
                        .proxy
                        .per_scheme
                        .get(scheme)
                        .map(|e| e.port.to_string())
                        .unwrap_or_default()
                },
            );
            p.add(native::enabled(
                own,
                div()
                    .pl(px(UNDER_CHECK))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(screen.label(t("Its host")))
                    .child(host)
                    .child(screen.label(t("Its port")))
                    .child(port),
            ));
        }
    }
    let script = input(
        o,
        window,
        cx,
        "proxy.script",
        "Automatic configuration script address (it chooses each site's proxy, in place of the above)",
        WIDE,
        on,
        false,
        |o| o.draft.proxy.script.clone(),
    );
    p.add(native::enabled(
        on,
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(screen.label(t("Automatic configuration script address (it chooses each site's proxy, in place of the above)")))
            .child(script),
    ));
    p.add(native::enabled(
        on,
        native::note(
            t("Every download, the video tools and a torrent's SOCKS5 connections go through it; FTP through SOCKS or HTTP CONNECT."),
            theme.muted,
            &screen,
        ),
    ));
}

/// The connections Windows knows (none elsewhere), read again at most every
/// two seconds: the page is drawn every frame it is open.
fn known_connections() -> Vec<String> {
    static READ: std::sync::Mutex<Option<(std::time::Instant, Vec<String>)>> =
        std::sync::Mutex::new(None);
    let mut read = dm_core::lock(&READ);
    match &*read {
        Some((at, names)) if at.elapsed() < std::time::Duration::from_secs(2) => names.clone(),
        _ => {
            let names = dm_shell::dialup::connections();
            *read = Some((std::time::Instant::now(), names.clone()));
            names
        }
    }
}

/// Advanced's Dial Up / VPN section (Windows), as the egui window's:
/// reference A's switch, then the connection with its sign-in, then the
/// redials, which wait for a connection chosen.
fn dialup(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let dial = o.draft.dialup.clone();
    p.section(t("Dial Up / VPN"));
    check(
        p,
        cx,
        "dialup-enabled",
        t("Use Windows Dial Up / VPN Networking"),
        dial.enabled,
        true,
        |s| s.dialup.enabled = !s.dialup.enabled,
    );
    let on = dial.enabled;
    let known = known_connections();
    let choices: Vec<SharedString> = std::iter::once(t("None").into())
        .chain(known.iter().map(|name| SharedString::from(name.clone())))
        .collect();
    let (now, chosen) = if dial.connection.is_empty() {
        (t("None").to_string(), Some(0))
    } else {
        (
            dial.connection.clone(),
            known
                .iter()
                .position(|name| *name == dial.connection)
                .map(|i| i + 1),
        )
    };
    p.labelled(
        t("Connection"),
        on,
        popup::button(
            "dialup-connection",
            now.into(),
            choices,
            chosen,
            theme,
            &screen,
        ),
    );
    let chosen_one = on && !dial.connection.is_empty();
    for (key, words, masked) in [
        ("dialup.username", "User name", false),
        ("dialup.password", "Password", true),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            200.0,
            chosen_one,
            masked,
            move |o| match key {
                "dialup.username" => o.draft.dialup.username.clone(),
                _ => o.draft.dialup.password.clone(),
            },
        );
        p.labelled(t(words), chosen_one, field);
    }
    check(
        p,
        cx,
        "dialup-save-password",
        t("Save password"),
        dial.save_password,
        chosen_one,
        |s| s.dialup.save_password = !s.dialup.save_password,
    );
    for (key, words) in [
        ("dialup.attempts", "Redial attempts (zero if endlessly)"),
        (
            "dialup.wait_seconds",
            "Time between redial attempts (seconds)",
        ),
    ] {
        let field = input(
            o,
            window,
            cx,
            key,
            words,
            NUMBER,
            chosen_one,
            false,
            move |o| match key {
                "dialup.attempts" => o.draft.dialup.attempts.to_string(),
                _ => o.draft.dialup.wait_seconds.to_string(),
            },
        );
        p.labelled(t(words), chosen_one, field);
    }
    p.note(t("A connection Windows knows (Settings › Network & internet › Dial-up or VPN). A download waits while dm connects it, and stops if it does not come up. With no user name or password here, dm uses the ones Windows keeps; without Save password, the one typed is forgotten when dm closes."));
}

/// Who opens magnet links and `.torrent` files on this computer (Windows),
/// as the egui window's Browser page says it: each with a button that makes
/// it dm where it is not, and one that gives it back to the program that
/// had it. Read beside the window as the page opens, and again after a
/// change.
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
    p.note(t("The first magnet link you click may make Windows ask which app opens it: choose dm there. dm cannot answer for you."));
}

/* --------------------------------------------------------------- Sign-ins */

fn sign_ins(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let screen = p.screen.clone();
    p.words(t(
        "A user and password used for every download from a host.",
    ));
    p.note(t(
        "Sent as a Basic login, or as a Digest where the server asks for one.",
    ));
    for i in 0..o.draft.site_logins.len() {
        let host = input(
            o,
            window,
            cx,
            &format!("site_logins.{i}.host"),
            "Host",
            150.0,
            true,
            false,
            move |o| o.draft.site_logins[i].host.clone(),
        );
        let user = input(
            o,
            window,
            cx,
            &format!("site_logins.{i}.username"),
            "User",
            110.0,
            true,
            false,
            move |o| o.draft.site_logins[i].username.clone(),
        );
        let password = input(
            o,
            window,
            cx,
            &format!("site_logins.{i}.password"),
            "Password",
            110.0,
            true,
            true,
            move |o| o.draft.site_logins[i].password.clone(),
        );
        let delete = button(p, ("delete-login", i), t("Delete")).on_click(cx.listener(
            move |o, _, _, cx| {
                if i < o.draft.site_logins.len() {
                    o.draft.site_logins.remove(i);
                    o.forget("site_logins");
                }
                cx.notify();
            },
        ));
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Host")))
                .child(host)
                .child(screen.label(t("User")))
                .child(user)
                .child(screen.label(t("Password")))
                .child(password)
                .child(delete),
        );
    }
    let add = button(p, "add-login", t("Add a login")).on_click(cx.listener(|o, _, _, cx| {
        o.draft
            .site_logins
            .push(dm_shell::settings::SiteLogin::default());
        cx.notify();
    }));
    p.add(div().flex().child(add));
    p.note(t(
        "Kept in dm's settings file on this computer, readable by your account.",
    ));
}

#[cfg(test)]
mod tests {
    use super::known_connections;

    #[test]
    fn the_connections_known_are_read_again_only_after_a_while() {
        // None here (a system without Windows' dial-up), read once and kept.
        assert_eq!(known_connections(), dm_shell::dialup::connections());
        assert_eq!(known_connections(), dm_shell::dialup::connections());
    }
}
