//! Finding the player under the pointer, and showing and hiding the panel over it.

use super::*;

/// What the page says about a media element.
pub(super) fn media_of(element: &Element) -> Option<Media> {
    let media: &HtmlMediaElement = element.dyn_ref()?;
    if element.has_attribute("data-dm-hidden") {
        return None;
    }
    let rect = element.get_bounding_client_rect();
    let src = Some(media.current_src())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| media.src());
    Some(Media {
        tag: element.tag_name(),
        src,
        width: rect.width(),
        height: rect.height(),
    })
}

/// Raises the panel over `element`, if it is a player worth offering.
pub(super) fn show(element: &Element) -> bool {
    if !RULES_ASKED.with(|c| c.replace(true)) {
        wasm_bindgen_futures::spawn_local(async {
            let _ = send(message(decide::RULES_MESSAGE, "", "").into()).await;
        });
    }
    if OFF_HERE.with(|c| c.get()) || OFF_ALL.with(|c| c.get()) {
        return false;
    }
    let Some(media) = media_of(element) else {
        return false;
    };
    let Some(offer) = decide::offer(&media) else {
        return false;
    };
    // A `mediaKeys` object alone is not DRM: refuse unless a playlist the page loaded was read and is clear.
    let key_object = Reflect::get(element, &"mediaKeys".into())
        .ok()
        .is_some_and(|keys| !keys.is_null() && !keys.is_undefined());
    let protected =
        key_object && decide::protection::stream_protection(&bodies::kept_texts()) != Some(false);
    let Some(window) = window() else {
        return false;
    };
    if !seen(&window, element) {
        return false;
    }
    // In fullscreen only the fullscreen element is drawn: the panel goes
    // inside it, or is not shown when that is the bare `<video>`.
    if !attach() {
        return false;
    }
    let view = (
        window
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        window
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
    );
    let rect = element.get_bounding_client_rect();
    let box_ = (rect.left(), rect.top(), rect.width(), rect.height());
    // On the part of the video a person sees, below a page's fixed bar over its top, and
    // compact on a narrow video (gaps row 76).
    let clipped = clipped_by_parents(&window, element, box_);
    let shown = decide::visible_box(clipped, view, bar_over_top(&window, element, clipped, view));
    let size = decide::panel_size(box_.2);
    let compact = size.0 < PANEL_SIZE.0;
    let spot = decide::panel_at(shown, size, view);
    SPOT.with(|s| s.set(spot));
    PLACED.with(|p| p.set(box_));
    // Where the person dragged it to, kept for the page.
    let (x, y) = decide::dragged(spot, NUDGE.with(|n| n.get()), size, view);
    with_panel(|p| {
        let same = p.current.as_ref() == Some(element);
        if !same {
            // A new player: a fresh offer, whatever the last one said.
            p.settled = false;
            follow_size_of(element);
            p.wrap.set_class_name("wrap");
            p.title.set_text_content(Some(if protected {
                decide::PROTECTED_TITLE
            } else {
                offer.title
            }));
            if protected {
                let _ = p.go.set_attribute("data-protected", "");
            } else {
                let _ = p.go.remove_attribute("data-protected");
            }
            if offer.page && !protected {
                let _ = p.go.set_attribute("aria-haspopup", "menu");
                let _ = p.go.set_attribute("aria-expanded", "false");
            } else {
                let _ = p.go.remove_attribute("aria-haspopup");
                let _ = p.go.remove_attribute("aria-expanded");
            }
            // Where it comes from, on hover; the bar says nothing more.
            let _ = p.go.set_attribute("title", &offer.sub);
            if offer.page && !protected {
                let _ = p.caret.remove_attribute("hidden");
            } else {
                let _ = p.caret.set_attribute("hidden", "");
            }
            close_list(p);
            say(p, "");
            p.current = Some(element.clone());
            if offer.page && !protected {
                read_ahead(&media.src);
            }
        }
        p.generation = p.generation.wrapping_add(1);
        // Compact: the mark and the close mark; the button keeps its words as its name.
        if let Ok(Some(body)) = p.root.query_selector(".panel") {
            let _ = body.set_attribute("class", if compact { "panel compact" } else { "panel" });
        }
        if compact {
            let words = p.title.text_content().unwrap_or_default();
            let _ = p.go.set_attribute("aria-label", &words);
        } else {
            let _ = p.go.remove_attribute("aria-label");
        }
        let style = p.host.style();
        let _ = style.set_property_with_priority("left", &format!("{x}px"), "important");
        let _ = style.set_property_with_priority("top", &format!("{y}px"), "important");
        let _ = style.set_property_with_priority("display", "block", "important");
    })
    .is_some()
}

/// The player's box as its parents let it be seen: each of the few around it that hides what
/// overflows it (a carousel, a cropped card) cuts the box to its own, as reference A clips its
/// bar to such parents (gaps row 125). The player's own box where nothing would be left.
fn clipped_by_parents(
    window: &Window,
    element: &Element,
    rect: (f64, f64, f64, f64),
) -> (f64, f64, f64, f64) {
    let (mut left, mut top) = (rect.0, rect.1);
    let (mut right, mut bottom) = (rect.0 + rect.2, rect.1 + rect.3);
    for parent in std::iter::successors(visible::composed_parent(element), visible::composed_parent)
        .take(12)
        .take_while(|e| !matches!(e.tag_name().to_ascii_lowercase().as_str(), "body" | "html"))
    {
        let hides = window
            .get_computed_style(&parent)
            .ok()
            .flatten()
            .map(|st| {
                let value = |name: &str| st.get_property_value(name).unwrap_or_default();
                ["overflow-x", "overflow-y"]
                    .iter()
                    .any(|name| matches!(value(name).trim(), "hidden" | "clip" | "scroll" | "auto"))
            })
            .unwrap_or(false);
        if hides {
            let r = parent.get_bounding_client_rect();
            left = left.max(r.left());
            top = top.max(r.top());
            right = right.min(r.right());
            bottom = bottom.min(r.bottom());
        }
    }
    if right > left && bottom > top {
        (left, top, right - left, bottom - top)
    } else {
        rect
    }
}

/// The bottom of a page's fixed or sticky bar lying over the top of the player (a site's
/// header), where one does: the element at the top of the player's visible part, when it is
/// neither the player nor anything inside or around it, and it or one of a few elements around
/// it stays put on the screen.
fn bar_over_top(
    window: &Window,
    element: &Element,
    rect: (f64, f64, f64, f64),
    view: (f64, f64),
) -> Option<f64> {
    let (left, top, width, height) = rect;
    let x = (left + width / 2.0).clamp(1.0, (view.0 - 1.0).max(1.0));
    let y = top.max(0.0) + 2.0;
    if y >= top + height || y >= view.1 {
        return None;
    }
    let hit = document().and_then(|d| visible::deepest_at(&d.into(), x, y))?;
    if &hit == element || visible::within(&hit, element) || visible::within(element, &hit) {
        return None;
    }
    if with_panel(|p| visible::within(&p.host, &hit)).unwrap_or(false) {
        return None;
    }
    std::iter::successors(Some(hit), visible::composed_parent)
        .take(8)
        .take_while(|e| !matches!(e.tag_name().to_ascii_lowercase().as_str(), "body" | "html"))
        .find(|e| {
            window
                .get_computed_style(e)
                .ok()
                .flatten()
                .and_then(|st| st.get_property_value("position").ok())
                .is_some_and(|p| matches!(p.trim(), "fixed" | "sticky"))
        })
        .map(|bar| bar.get_bounding_client_rect().bottom())
        .filter(|bottom| *bottom > y && *bottom < top + height)
}

pub(super) fn hide() {
    stop_following_size();
    with_panel(|p| {
        let _ = p
            .host
            .style()
            .set_property_with_priority("display", "none", "important");
        p.current = None;
        p.hovered = false;
        p.on_player = false;
        p.focused = false;
        close_list(p);
    });
}

/// Hides after `ms`, unless the panel has been shown again or is under the
/// pointer by then.
pub(super) fn hide_soon(ms: i32) {
    let Some(generation) = with_panel(|p| p.generation) else {
        return;
    };
    set_timeout(
        move || {
            let stale =
                with_panel(|p| p.generation != generation || p.hovered || p.on_player || p.focused)
                    .unwrap_or(true);
            if !stale {
                hide();
            }
        },
        ms,
    );
}

/// The media element at a point, under whatever overlays a player.
///
/// The hit test comes first, since it knows what is on top. It skips
/// anything with `pointer-events: none`, which YouTube puts on its player's
/// layers as the controls fade, so a pointer resting on a playing video is
/// then over "nothing". Failing the hit test, any visible media element
/// whose box holds the point is the one.
pub(super) fn media_at(document: &Document, x: f64, y: f64) -> Option<Element> {
    let root: JsValue = document.clone().into();
    media_in(&root, x, y, 0).filter(big_enough)
}

/// Whether a player is a player: one smaller than 15 by 10 pixels is a
/// decoration or a beacon, which reference A passes by as well
/// (`SMALLEST_PLAYER`).
fn big_enough(element: &Element) -> bool {
    let (wide, high) = crate::decide::excluded::SMALLEST_PLAYER;
    let rect = element.get_bounding_client_rect();
    rect.width() >= wide && rect.height() >= high
}

pub(super) fn is_media(element: &Element) -> bool {
    let tag = element.tag_name().to_ascii_lowercase();
    tag == "video" || tag == "audio"
}

/// The open shadow root an element hosts, if any.
pub(super) fn shadow_of(element: &Element) -> Option<JsValue> {
    Reflect::get(element, &"shadowRoot".into())
        .ok()
        .filter(|r| r.is_object())
}

/// The media under `(x, y)` in `root` (the document, or a component's open
/// shadow root), looking inside components on the way. Players built as
/// web components keep their `<video>` in a shadow root, which the
/// document's own hit-test stops at: the Internet Archive's `play-av` is
/// one, and its videos got no panel.
pub(super) fn media_in(root: &JsValue, x: f64, y: f64, depth: u8) -> Option<Element> {
    let hit_test: Function = Reflect::get(root, &"elementsFromPoint".into())
        .ok()?
        .dyn_into()
        .ok()?;
    let stack: Array = hit_test
        .call2(root, &JsValue::from_f64(x), &JsValue::from_f64(y))
        .ok()?
        .dyn_into()
        .ok()?;
    let elements: Vec<Element> = stack.iter().filter_map(|n| n.dyn_into().ok()).collect();
    if let Some(found) = elements.iter().find(|e| is_media(e)) {
        return Some(found.clone());
    }
    if depth < 4 {
        for host in &elements {
            if let Some(inner) = shadow_of(host) {
                if let Some(found) = media_in(&inner, x, y, depth + 1) {
                    return Some(found);
                }
            }
        }
    }
    // A player whose controls let no hit-test through: any media element of
    // this root whose box holds the point.
    let query: Function = Reflect::get(root, &"querySelectorAll".into())
        .ok()?
        .dyn_into()
        .ok()?;
    let all: Array = Array::from(&query.call1(root, &"video, audio".into()).ok()?);
    all.iter().rev().find_map(|node| {
        let element: Element = node.dyn_into().ok()?;
        let r = element.get_bounding_client_rect();
        let inside = r.width() > 0.0
            && r.height() > 0.0
            && (r.left()..=r.right()).contains(&x)
            && (r.top()..=r.bottom()).contains(&y);
        inside.then_some(element)
    })
}

pub(super) fn on_pointer(event: Event) {
    let Some(event) = event.dyn_ref::<MouseEvent>() else {
        return;
    };
    pointer_at(f64::from(event.client_x()), f64::from(event.client_y()));
}

/// The page script's word, once this has loaded, of where the pointer
/// rests: it loaded this because the pointer came over a player, and a
/// pointer that then rests sends no move for the panel to see.
#[wasm_bindgen]
pub fn content_pointer(x: f64, y: f64) {
    with_panel(|p| p.last_scan = 0.0);
    pointer_at(x, y);
}

/// The page script's word, once this has loaded, of media that played
/// before it had: the play that loaded it was over by then.
#[wasm_bindgen]
pub fn content_played(element: Element) {
    if show(&element) {
        hide_soon(PLAY_SHOWS_MS);
    }
}

pub(super) fn pointer_at(x: f64, y: f64) {
    let due = with_panel(|p| {
        let due = now() - p.last_scan >= SCAN_EVERY_MS;
        if due {
            p.last_scan = now();
        }
        due
    })
    .unwrap_or(false);
    if !due {
        return;
    }
    let Some(document) = document() else {
        return;
    };
    match media_at(&document, x, y) {
        Some(element) if show(&element) => {
            with_panel(|p| p.on_player = true);
        }
        _ => {
            with_panel(|p| p.on_player = false);
            let over = with_panel(|p| p.hovered || p.current.is_none()).unwrap_or(true);
            if !over {
                hide_soon(HIDE_AFTER_MS);
            }
        }
    }
}

pub(super) fn on_play(event: Event) {
    let Some(element) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    if show(&element) {
        hide_soon(PLAY_SHOWS_MS);
    }
}

thread_local! {
    /// Watches the player the panel sits over for a change of size.
    static RESIZED: std::cell::RefCell<Option<web_sys::ResizeObserver>> =
        const { std::cell::RefCell::new(None) };
    /// The player's box the panel was last placed for.
    static PLACED: std::cell::Cell<(f64, f64, f64, f64)> =
        const { std::cell::Cell::new((0.0, 0.0, 0.0, 0.0)) };
    /// The timer that looks, while the panel is shown, for a player that
    /// moved without changing size.
    static MOVING: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

/// How often a shown panel looks whether its player moved under it.
const FOLLOW_EVERY_MS: i32 = 200;

/// Puts the panel back on its player if the player's box is not the one
/// it was placed for.
fn follow_if_moved() {
    let Some(Some(element)) = with_panel(|p| p.current.clone()) else {
        return;
    };
    let rect = element.get_bounding_client_rect();
    let now = (rect.left(), rect.top(), rect.width(), rect.height());
    if now != PLACED.with(|p| p.get()) && !show(&element) {
        hide();
    }
}

/// Has the panel follow `element` when it changes size with the window
/// unchanged: a page laying its player out late (YouTube's settles after
/// the panel is up, a few hundred pixels narrower), a theatre mode; and
/// when it moves without changing size, as it does when the page above
/// it folds (YouTube's player goes 8 px left and 12 px up a second after
/// its first layout, BUG-007), which no observer reports: the panel looks
/// every [`FOLLOW_EVERY_MS`] while it is shown. A scroll and a window
/// resize already move it (`on_move`).
fn follow_size_of(element: &Element) {
    MOVING.with(|timer| {
        if timer.get().is_none() {
            let look = Closure::<dyn Fn()>::new(follow_if_moved);
            if let Some(window) = window() {
                timer.set(
                    window
                        .set_interval_with_callback_and_timeout_and_arguments_0(
                            look.as_ref().unchecked_ref(),
                            FOLLOW_EVERY_MS,
                        )
                        .ok(),
                );
            }
            look.forget();
        }
    });
    RESIZED.with(|watched| {
        let mut slot = watched.borrow_mut();
        if slot.is_none() {
            let moved = Closure::<dyn Fn(JsValue, JsValue)>::new(|_, _| {
                if let Some(Some(element)) = with_panel(|p| p.current.clone()) {
                    if !show(&element) {
                        hide();
                    }
                }
            });
            *slot = web_sys::ResizeObserver::new(moved.as_ref().unchecked_ref()).ok();
            moved.forget();
        }
        if let Some(observer) = slot.as_ref() {
            observer.disconnect();
            observer.observe(element);
        }
    });
}

/// Stops following the last player's size and place (`follow_size_of`).
fn stop_following_size() {
    RESIZED.with(|watched| {
        if let Some(observer) = watched.borrow().as_ref() {
            observer.disconnect();
        }
    });
    MOVING.with(|timer| {
        if let (Some(id), Some(window)) = (timer.take(), window()) {
            window.clear_interval_with_handle(id);
        }
    });
}

/// Keeps the panel on its player as the page scrolls or resizes.
pub(super) fn on_move(_: Event) {
    if let Some(Some(element)) = with_panel(|p| p.current.clone()) {
        if !show(&element) {
            hide();
        }
    }
}
