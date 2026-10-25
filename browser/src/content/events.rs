//! Listening to the page: pointer, focus, fullscreen and keys.

use super::*;

/// Puts the panel where it can be seen: inside the fullscreen element when
/// there is one (only it is drawn then), else in the page's body. `false`
/// when the fullscreen element is a media element itself, which draws
/// nothing of the page's over it.
pub(super) fn attach() -> bool {
    let Some(document) = document() else {
        return false;
    };
    let mut full = Reflect::get(&document, &"fullscreenElement".into())
        .ok()
        .and_then(|f| f.dyn_into::<Element>().ok());
    // The document names a shadow host; the element in fullscreen may be
    // inside its shadow root.
    for _ in 0..8 {
        let inner = full
            .as_ref()
            .and_then(shadow_of)
            .and_then(|r| Reflect::get(&r, &"fullscreenElement".into()).ok())
            .and_then(|f| f.dyn_into::<Element>().ok());
        match inner {
            Some(inner) if Some(&inner) != full.as_ref() => full = Some(inner),
            _ => break,
        }
    }
    if full.as_ref().is_some_and(is_media) {
        return false;
    }
    // A light-DOM child of a shadow host is drawn only through a slot: in
    // the host's open shadow root it is drawn whatever the slots. A closed
    // one cannot be reached, and there is no panel rather than one unseen.
    let parent: Option<web_sys::Node> = match &full {
        Some(full) => match shadow_of(full) {
            Some(root) => root.dyn_into().ok(),
            None if full.tag_name().contains('-') => return false,
            None => Some(full.clone().into()),
        },
        None => document
            .body()
            .map(Into::into)
            .or_else(|| document.document_element().map(Into::into)),
    };
    let Some(parent) = parent else {
        return false;
    };
    with_panel(|p| {
        if p.host.parent_node().as_ref() != Some(&parent) {
            let _ = parent.append_child(&p.host);
        }
    });
    true
}

/// Fullscreen came or went: the panel moves with it, and is shown again
/// over its player if it was up.
pub(super) fn on_fullscreen(_: Event) {
    let current = with_panel(|p| p.current.clone()).flatten();
    match current {
        Some(element) => {
            if show(&element) {
                hide_soon(PLAY_SHOWS_MS);
            } else {
                hide();
                attach();
            }
        }
        None => {
            attach();
        }
    }
}

/// The focus on a player (its `<video>`, or a player element holding one)
/// raises the panel for it, as the pointer does, and keeps it up.
pub(super) fn on_focus(event: Event) {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    if with_panel(|p| JsValue::from(p.host.clone()) == JsValue::from(target.clone())) == Some(true)
    {
        return;
    }
    let media = if is_media(&target) {
        Some(target.clone())
    } else {
        target.query_selector("video, audio").ok().flatten()
    };
    let Some(media) = media else {
        return;
    };
    // A click on a player focuses it too; the pointer raises the panel then.
    if !by_keyboard(&target) {
        return;
    }
    if show(&media) {
        with_panel(|p| {
            p.player_focus = Some(target);
            p.focused = true;
        });
    }
}

/// Whether an element's focus came from the keyboard (`:focus-visible`).
pub(super) fn by_keyboard(element: &Element) -> bool {
    element.matches(":focus-visible").unwrap_or(false)
}

pub(super) fn on_blur(event: Event) {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    let ours = with_panel(|p| p.player_focus.as_ref() == Some(&target)).unwrap_or(false);
    if ours {
        with_panel(|p| p.focused = false);
        hide_soon(HIDE_AFTER_MS);
    }
}

/// Tab on a player whose panel is up goes into the panel first.
pub(super) fn on_page_key(event: Event) {
    let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
        return;
    };
    if Key::of(&key.key(), key.shift_key()) != Key::Tab {
        return;
    }
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    let into = with_panel(|p| {
        let up = p.current.is_some() && p.player_focus.as_ref() == Some(&target);
        up.then(|| focus_target(p, Focus::Go)).flatten()
    })
    .flatten();
    if into.is_some() {
        event.prevent_default();
        focus(into);
    }
}
