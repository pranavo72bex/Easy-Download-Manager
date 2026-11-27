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
mod network_allowance;
#[cfg(target_os = "android")]
mod phone_rules;
mod sign_ins;

/// A field for a number.
const NUMBER: f32 = native::NUMBER_FIELD;
/// A field for a name or an address.
const WIDE: f32 = 300.0;
/// The scale slider's width, its stops' percentages under it, each in a
/// box this wide.
const SCALE_WIDTH: f32 = 200.0;
const STOP_WIDTH: f32 = 40.0;
/// The slider's row, in rem: gpui-component's horizontal track (`h_6`).
const SLIDER_ROW: f32 = 1.5;

/// Under a check box, its explanation lines up with its words.
const UNDER_CHECK: f32 = if crate::platform::ON_A_PHONE {
    // A phone's larger check box (22) and the gap after it, so the words line up.
    28.0
} else {
    20.0
};

/// The looks this window draws: the computer's, light or dark (a `classic`
/// kept in the settings is drawn as the computer's).
const THEMES: [&str; 3] = ["system", "light", "dark"];
/// What closing the window does, by the setting's word.
const ON_CLOSE: [&str; 3] = ["ask", "tray", "exit"];
/// What an address already listed does, by the setting's word.
const DUPLICATE: [&str; 4] = ["", "show", "again", "replace"];
/// What the same file as one already here does (gaps row W18), by the setting's word.
const SAME_FILE: [&str; 3] = ["", "show", "again"];
/// The proxy kinds offered: on a phone not the computer's settings, which Android's
/// downloads do not follow there (gaps row N24); a proxy typed in still works.
fn proxy_kinds() -> Vec<dm_core::ProxyKind> {
    PROXY_KINDS
        .into_iter()
        .filter(|k| !(crate::platform::ON_A_PHONE && *k == dm_core::ProxyKind::System))
        .collect()
}

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
    /// Its sections' headings, in order: the page's summary in the list of pages (gaps row C21).
    headings: Vec<String>,
    /// What each item says first, as written: the summary of a page with no headings.
    labels: Vec<String>,
    /// What the first item says, where it stands before any heading: a page that opens with
    /// settings of no section (Downloads' connections) begins its summary with it.
    lead: Option<String>,
    theme: Theme,
    screen: Screen,
}

impl Page {
    fn add(&mut self, element: impl IntoElement) {
        if self.items.is_empty() && self.headings.is_empty() && !self.saying.is_empty() {
            self.lead = Some(self.saying.clone());
        }
        self.items.push(element.into_any_element());
        if !self.saying.is_empty() {
            self.labels.push(self.saying.clone());
        }
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
        self.headings.push(words.to_string());
        self.says(words);
        let heading = native::heading(words.to_string(), self.theme, &self.screen)
            // A page's first: under the page's own name, as a first setting is, not a section's
            // room further down (34 points under the name where a setting had 14).
            .when(self.items.is_empty(), |h| h.mt_0());
        self.add(heading);
    }

    /// What a setting does, close under its row (gaps row C20: a switch's words start the row).
    fn hint(&mut self, words: &str) {
        self.hint_when(true, words);
    }

    /// [`Page::hint`] greyed with its setting while `on` is false: under a field the words of a
    /// list are typed in (Browser's file types and sites, the proxy's script), as under a switch.
    /// Those stood a row's gap under their field, midway to the next setting, as a setting of
    /// their own.
    fn hint_when(&mut self, on: bool, words: &str) {
        self.says(words);
        let hint = native::hint(words.to_string(), self.theme, &self.screen);
        self.add(native::enabled(on, hint));
    }

    /// Small words of their own, at the page's edge, a row's gap from what is above them on a
    /// phone as on a computer: a phone's stood nearer, under a section's heading over it.
    fn note(&mut self, words: &str) {
        self.note_when(true, words);
    }

    /// [`Page::note`] greyed while `on` is false (what a greyed list below it is for).
    fn note_when(&mut self, on: bool, words: &str) {
        self.says(words);
        let note = native::measured(native::note(
            words.to_string(),
            self.theme.muted,
            &self.screen,
        ));
        self.add(native::enabled(on, note));
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
        // The words above their wide control, on every page and every system: beside it where
        // it fitted and under it where not read as two layouts on one page (gaps rows N7, E31).
        let row = above(&self.screen, words, control);
        self.add(native::enabled(on, row));
    }
}

impl Page {
    /// Words, then a small control (a number's box, a drop-down): in one row, the words at the
    /// leading edge and the control at the trailing one, as the switches are, so a page's
    /// controls stand in one column whatever their words (gaps row E10; on a phone since N7).
    /// A wide control (a folder, a list of sites) stays `labelled`, on a line of its own under
    /// its words.
    fn setting(&mut self, words: &str, on: bool, control: impl IntoElement) {
        self.says(words);
        let row = div()
            .flex()
            // Its words at the right and the control at the left, right to left (gaps E36).
            .map(crate::window::mirror::row)
            .items_center()
            .gap(px(space::L))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.screen.label(words.to_string())),
            )
            .child(div().flex_none().child(control));
        self.add(native::enabled(on, row));
    }
}

/// A switch for one of the draft's on/off settings: `flip` turns it (gaps row C20).
fn check(
    p: &mut Page,
    cx: &mut Context<Options>,
    id: &'static str,
    words: &str,
    on: bool,
    enabled: bool,
    flip: fn(&mut Settings),
) {
    let check = native::switch(id, words.to_string().into(), on, p.theme, &p.screen).on_click(
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
        // A text field (a host, a name, a list of sites) to the line's trailing edge, under
        // its words; a number's box stays small, beside its words (gaps row E31).
        .when(width >= WIDE, |d| {
            d.flex_1().w_full().flex_shrink(1.0).min_w_0()
        })
        .child(if masked {
            native::secret_field_if(&field, name, on, theme)
        } else {
            native::field_if(&field, name, on, theme)
        })
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

/// `words`, then under them a wide control (a list's field, a folder and its Browse…): one room
/// between them on every page, where the folders' stood nearer theirs than the lists' did.
fn above(screen: &Screen, words: &str, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .items_stretch()
        .gap(px(space::S))
        .child(screen.label(words.to_string()))
        .child(control)
}

/// A plain button.
fn button(p: &Page, id: impl Into<gpui::ElementId>, words: &str) -> gpui::Stateful<gpui::Div> {
    native::button(id, words.to_string().into(), false, p.theme, &p.screen)
}

/// A button that destroys (Delete, Clear), its words in the error colour (rule 3).
fn destructive(p: &Page, id: impl Into<gpui::ElementId>, words: &str) -> gpui::Stateful<gpui::Div> {
    native::destructive(id, words.to_string().into(), p.theme, &p.screen)
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
        headings: Vec::new(),
        labels: Vec::new(),
        lead: None,
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
/// What page `tab` holds, in a line for the list of pages: its first sections' headings, in the
/// words the page itself uses (gaps row C21).
pub(crate) fn summary(
    o: &mut Options,
    tab: Tab,
    theme: Theme,
    window: &mut Window,
    cx: &mut Context<Options>,
) -> String {
    let page = built(o, tab, theme, window, cx);
    // A page with no sections is summed up by its first settings' names; one with sections by
    // their headings, after the setting it opens with where that stands before them: Downloads
    // said "Queues with their own limit, …" and not its connections, its first setting.
    let named = if page.headings.is_empty() {
        page.labels
    } else {
        page.lead.into_iter().chain(page.headings).collect()
    };
    named
        .into_iter()
        // A setting's name, without the explanation said with it.
        .map(|words| {
            words
                .split(". ")
                .next()
                .unwrap_or(&words)
                .trim()
                .trim_end_matches(':')
                .to_string()
        })
        .filter(|words| !words.is_empty() && words.len() <= 40)
        .take(3)
        .collect::<Vec<_>>()
        .join(", ")
}

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
                .role(gpui::Role::Link)
                .aria_label(SharedString::from(t(name)))
                .map(|c| native::ringed(c, theme))
                .mt(px(space::M_L))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                // A link to its page, in the accent's text shade: the selection's fill read 2.4
                // to 1 on a dark dialog.
                .text_color(theme.accent_text)
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
        // A third network beside the second (gaps row K3).
        "third-network" => {
            if i == 0 {
                s.third_path.clear();
            } else if let Some(found) = o.networks.get(i - 1) {
                s.third_path = found.name.clone();
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
            if let Some(kind) = proxy_kinds().get(i) {
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
    p.says(t(label));
    p.add(native::enabled(
        on,
        above(
            &p.screen,
            t(label),
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
