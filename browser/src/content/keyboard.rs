//! The panel's spoken lines, its menus and its keyboard.

use super::*;

/// The line under the bar: what happened, or what ? says. Empty hides it.
pub(super) fn say(p: &Panel, text: &str) {
    // A line of its own starts with a capital: "the dm app has it" read
    // as half a sentence. The words are the background's,
    // shared with its notifications, where they follow a title.
    // But dm's own name stays as it is written everywhere, lowercase
    // ("Dm downloads this video…").
    let mut chars = text.chars();
    let text = match chars.next() {
        _ if text.starts_with("dm ") => text.to_string(),
        Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
        None => String::new(),
    };
    let text = text.as_str();
    p.sub.set_text_content(Some(text));
    if text.is_empty() {
        let _ = p.sub.set_attribute("hidden", "");
    } else {
        let _ = p.sub.remove_attribute("hidden");
        announce(p);
    }
}

/// What the panel now says, read out by a screen reader: the bar's words
/// and the line under it.
pub(super) fn announce(p: &Panel) {
    let title = p.title.text_content().unwrap_or_default();
    let sub = p.sub.text_content().unwrap_or_default();
    let said = if sub.is_empty() {
        title
    } else {
        format!("{title}. {sub}")
    };
    p.live.set_text_content(Some(&said));
}

/// A result on the bar: its words, said aloud.
pub(super) fn set_title(p: &Panel, text: &str) {
    p.title.set_text_content(Some(text));
    announce(p);
}

/// The × menu: hide the panel for this player, or never show it on this site.
pub(super) fn close_menu() {
    let Some(document) = document() else {
        return;
    };
    let site = decide::site_of(&top_page()).unwrap_or_else(|| "this site".into());
    let first = with_panel(|p| {
        p.settled = true;
        close_list(p);
        say(p, "");
        for (line, never) in [
            ("Hide for now".to_string(), false),
            (format!("Never on {site}"), true),
        ] {
            let Ok(item) = document.create_element("button") else {
                continue;
            };
            item.set_class_name("item");
            let _ = item.set_attribute("role", "menuitem");
            item.set_text_content(Some(&line));
            listen(&item, "click", move |event| {
                event.prevent_default();
                event.stop_propagation();
                if never {
                    never_here();
                } else {
                    // Not offered again for this player until the page reloads.
                    with_panel(|p| {
                        if let Some(el) = &p.current {
                            let _ = el.set_attribute("data-dm-hidden", "");
                        }
                    });
                    hide();
                }
            });
            let _ = p.list.append_child(&item);
        }
        open_list(p)
    })
    .flatten();
    focus(first);
}

pub(super) fn close_list(p: &Panel) {
    p.list.set_inner_html("");
    let _ = p.list.set_attribute("hidden", "");
    if p.go.has_attribute("aria-expanded") {
        let _ = p.go.set_attribute("aria-expanded", "false");
    }
}

/// Opens the list just filled. Returns its first line to put the focus on
/// when the focus was in the panel (a person at the keyboard); the caller
/// focuses it once it has let go of the panel.
pub(super) fn open_list(p: &Panel) -> Option<HtmlElement> {
    let _ = p.list.remove_attribute("hidden");
    if p.go.has_attribute("aria-expanded") {
        let _ = p.go.set_attribute("aria-expanded", "true");
    }
    p.root.active_element()?;
    focus_target(p, Focus::Item(0))
}

pub(super) fn focus(element: Option<HtmlElement>) {
    if let Some(element) = element {
        let _ = element.focus();
    }
}

pub(super) fn items(p: &Panel) -> Vec<HtmlElement> {
    let Ok(found) = p.list.query_selector_all(".item") else {
        return Vec::new();
    };
    (0..found.length())
        .filter_map(|i| found.item(i))
        .filter_map(|n| n.dyn_into::<HtmlElement>().ok())
        .collect()
}

pub(super) fn focus_target(p: &Panel, at: Focus) -> Option<HtmlElement> {
    match at {
        Focus::Go => p.go.clone().dyn_into().ok(),
        Focus::Help => p
            .root
            .query_selector(".help")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into().ok()),
        Focus::Close => p
            .root
            .query_selector(".close")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into().ok()),
        Focus::Item(i) => items(p).into_iter().nth(i),
    }
}

/// Where the focus is in the panel, if it is.
pub(super) fn focus_in(p: &Panel) -> Option<Focus> {
    let active = p.root.active_element()?;
    if active == p.go {
        return Some(Focus::Go);
    }
    let class = active.class_name();
    if class == "help" {
        return Some(Focus::Help);
    }
    if class == "close" {
        return Some(Focus::Close);
    }
    let at = items(p)
        .iter()
        .position(|i| JsValue::from(i.clone()) == JsValue::from(active.clone()))?;
    Some(Focus::Item(at))
}

/// The player the panel is up for, to put the focus back on.
pub(super) fn player_target(p: &Panel) -> Option<HtmlElement> {
    p.player_focus
        .clone()
        .or_else(|| p.current.clone())
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
}

/// A key in the panel: Escape backs out, the arrows walk the list, and Tab
/// moves on through the page as though the panel sat after its player.
/// Enter and Space are the buttons' own.
pub(super) fn on_key(event: Event) {
    let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
        return;
    };
    let key = Key::of(&key.key(), key.shift_key());
    let decided = with_panel(|p| {
        let at = focus_in(p)?;
        let lines = if p.list.has_attribute("hidden") {
            0
        } else {
            items(p).len()
        };
        let act = panel::act(key, at, lines);
        let to = match act {
            Act::Focus(to) => focus_target(p, to),
            Act::CloseList => {
                close_list(p);
                focus_target(p, Focus::Go)
            }
            _ => None,
        };
        Some((act, to, player_target(p)))
    })
    .flatten();
    let Some((act, to, player)) = decided else {
        return;
    };
    // The focus is moved here, with the panel let go of.
    match act {
        Act::Nothing => return,
        Act::Focus(_) | Act::CloseList => focus(to),
        Act::ToPlayer { .. } => focus(player),
        // Back on the player first (which would raise the panel), then gone.
        Act::Hide => {
            focus(player);
            hide();
        }
    }
    // Tab off the last button is let go on, from the player.
    if act != (Act::ToPlayer { stop: false }) {
        event.prevent_default();
        event.stop_propagation();
    }
}
