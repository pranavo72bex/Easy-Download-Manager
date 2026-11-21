// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' six pages (B43), each setting with its words and the
//! explanation under it: check boxes, the system's pop-ups and fields,
//! sections under bold headings. A field's text goes into the draft as it
//! is typed (`Options::put`); a check box, a pop-up or a button changes the
//! draft at once. On Windows: who opens magnet links and `.torrent` files
//! (Browser) and Dial Up / VPN (Advanced).

use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::options::{self, Tab};
use dm_shell::settings::Settings;
use gpui::{div, prelude::*, AnyElement, Context, SharedString, Window};

use crate::options::Options;
use crate::widgets::native;
use crate::widgets::popup::{self, Pick};
use crate::widgets::space;
use crate::window::screen::Screen;
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod advanced;
mod browser;
mod dialup;
mod downloads;
mod folders;
mod general;
#[cfg(target_os = "android")]
mod phone_rules;
mod sign_ins;

/// A field for a number.
const NUMBER: f32 = 90.0;
/// A field for a name or an address.
const WIDE: f32 = 300.0;
/// The scale slider's width, its stops' percentages under it, each in a
/// box this wide.
const SCALE_WIDTH: f32 = 200.0;
const STOP_WIDTH: f32 = 40.0;

/// Under a check box, its explanation lines up with its words.
const UNDER_CHECK: f32 = if crate::platform::ON_A_PHONE {
    // A phone's larger check box (22) and the gap after it, so the words line up.
    28.0
} else {
    20.0
};

/// How much nearer its setting a hint or note stands on a phone than one setting to the next:
/// close under what it explains, the settings apart from each other.
const HINT_PULL: f32 = native::ROW_GAP - space::XS;

/// The looks this window draws: the computer's, light or dark (a `classic`
/// kept in the settings is drawn as the computer's).
const THEMES: [&str; 3] = ["system", "light", "dark"];
/// What closing the window does, by the setting's word.
const ON_CLOSE: [&str; 3] = ["ask", "tray", "exit"];
/// What an address already listed does, by the setting's word.
const DUPLICATE: [&str; 4] = ["", "show", "again", "replace"];
/// What the same file as one already here does (gaps row W18), by the setting's word.
const SAME_FILE: [&str; 3] = ["", "show", "again"];
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
            .mt(px(space::M_L))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            // A phone's settings head each group in the accent colour, set apart above.
            .when(crate::platform::ON_A_PHONE, |heading| {
                heading
                    .mt(px(space::XL))
                    .text_size(px(native::SMALL_TEXT + 1.0))
                    .text_color(self.theme.accent)
            })
            .child(self.screen.label(words.to_string()));
        self.add(heading);
    }

    /// What a check box does, under its words.
    fn hint(&mut self, words: &str) {
        self.says(words);
        let hint = div()
            .pl(px(UNDER_CHECK))
            .when(crate::platform::ON_A_PHONE, |hint| hint.mt(px(-HINT_PULL)))
            .child(native::note(
                words.to_string(),
                self.theme.muted,
                &self.screen,
            ));
        self.add(hint);
    }

    /// Small words of their own, at the page's edge.
    fn note(&mut self, words: &str) {
        self.says(words);
        let note = native::note(words.to_string(), self.theme.muted, &self.screen)
            .when(crate::platform::ON_A_PHONE, |note| note.mt(px(-HINT_PULL)));
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
            // A phone's: the words above their control, every setting alike, as a phone's
            // own settings are; beside it where it fitted and under it where not read as
            // two layouts on one page.
            .when(crate::platform::ON_A_PHONE, |row| {
                row.flex_col().items_start().gap(px(space::S))
            })
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
        // A phone's: as wide as the room it has, never past the screen's edge with what
        // stands beside it (a folder's Browse…).
        .when(crate::platform::ON_A_PHONE, |d| {
            d.flex_shrink(1.0).min_w_0().max_w_full()
        })
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
        Tab::General => general::general(o, &mut p, window, cx),
        Tab::FileTypes => browser::browser(o, &mut p, window, cx),
        Tab::SaveTo => folders::folders(o, &mut p, window, cx),
        Tab::Downloads => downloads::downloads(o, &mut p, window, cx),
        Tab::Advanced => advanced::advanced(o, &mut p, window, cx),
        Tab::SiteLogins => sign_ins::sign_ins(o, &mut p, window, cx),
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
                .mt(px(space::M_L))
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
        // A queue's start time, hour or minute (gaps row U32).
        "start-hour" | "start-minute" => {
            if let Some(start) = s.downloads.queue_starts.get_mut(pick.item) {
                native::time_picked(&mut start.at, pick.field == "start-hour", i);
            }
        }
        "language" => {
            if let Some((code, _)) = dm_desktop_base::i18n::LANGUAGES.get(i) {
                s.general.language = code.to_string();
            }
        }
        "theme" => {
            if let Some(word) = THEMES.get(i) {
                s.general.theme = word.to_string();
            }
        }
        "on-close" => {
            if let Some(word) = ON_CLOSE.get(i) {
                s.general.on_close = word.to_string();
            }
        }
        "duplicate" => {
            if let Some(word) = DUPLICATE.get(i) {
                s.duplicate_choice = word.to_string();
            }
        }
        "same-file" => {
            if let Some(word) = SAME_FILE.get(i) {
                s.same_file_choice = word.to_string();
            }
        }
        "keep-key" => {
            if let Some(key) = dm_protocol::CLICK_KEYS.get(i) {
                s.browser.catch.keep_key = key.to_string();
            }
        }
        "take-key" => {
            if let Some(key) = TAKE_KEYS.get(i) {
                s.browser.catch.take_key = key.to_string();
            }
        }
        "quality" => {
            if let Some((value, _)) = dm_desktop_base::file_info::QUALITIES.get(i) {
                s.downloads.stream_quality = value.to_string();
            }
        }
        // The video's own first, then each language (gaps row Y10).
        "sound-language" => {
            if let Some(tag) = options::sound_language_of(i) {
                s.downloads.sound_language = tag.to_string();
            }
        }
        // Go on, ask or wait on a metered connection (gaps row W11).
        "on-metered" => {
            if let Some(word) = options::ON_METERED.get(i) {
                s.downloads.on_metered = word.to_string();
            }
        }
        "queue-limit" => {
            let offered = limit_queues(&o.queues, &o.draft, pick.item);
            if let (Some(queue), Some(limit)) = (
                offered.get(i),
                o.draft.downloads.queue_limits.get_mut(pick.item),
            ) {
                limit.queue = queue.clone();
            }
        }
        "queue-start" => {
            if let (Some(queue), Some(start)) = (
                o.queues.get(i),
                o.draft.downloads.queue_starts.get_mut(pick.item),
            ) {
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
        // The one network every download is held to (gaps row W12).
        "only-through" => {
            if i == 0 {
                s.advanced.only_through.clear();
            } else if let Some(found) = o.networks.get(i - 1) {
                s.advanced.only_through = found.name.clone();
            }
        }
        "dialup-connection" => {
            if i == 0 {
                s.advanced.dialup.connection.clear();
            } else if let Some(found) = dialup::known_connections().get(i - 1) {
                s.advanced.dialup.connection = found.clone();
            }
        }
        "proxy-kind" => {
            if let Some(kind) = PROXY_KINDS.get(i) {
                let p = &mut s.advanced.proxy;
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
    let own = s.downloads.queue_limits.get(row).map(|l| l.queue.clone());
    let mut offered: Vec<String> = queues
        .iter()
        .filter(|q| {
            own.as_ref() == Some(*q)
                || !s
                    .downloads
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
            .gap(px(space::XS))
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

#[cfg(test)]
mod tests {
    use super::dialup::known_connections;

    #[test]
    fn the_connections_known_are_read_again_only_after_a_while() {
        // None here (a system without Windows' dial-up), read once and kept.
        assert_eq!(known_connections(), dm_shell::dialup::connections());
        assert_eq!(known_connections(), dm_shell::dialup::connections());
    }
}
