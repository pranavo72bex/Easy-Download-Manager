//! Whether a player is really in view and not covered.

use super::*;

/// Whether a player is what a person sees there: not transparent or
/// hidden, and the thing at its middle is the player, its own overlay, or
/// a page layer it lies under that lets the pointer through.
pub(super) fn seen(window: &Window, element: &Element) -> bool {
    // Its own and its ancestors' opacity and visibility, where the browser
    // can say (Chrome 121, Firefox 122).
    if let Ok(check) =
        Reflect::get(element, &"checkVisibility".into()).and_then(|f| f.dyn_into::<Function>())
    {
        let options = js_sys::Object::new();
        let _ = Reflect::set(&options, &"opacityProperty".into(), &JsValue::TRUE);
        let _ = Reflect::set(&options, &"visibilityProperty".into(), &JsValue::TRUE);
        if check
            .call1(element, &options)
            .ok()
            .and_then(|v| v.as_bool())
            == Some(false)
        {
            return false;
        }
    }
    if let Ok(Some(style)) = window.get_computed_style(element) {
        let value = |name: &str| style.get_property_value(name).unwrap_or_default();
        if !panel::looks_visible(&value("opacity"), &value("visibility")) {
            return false;
        }
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
    let Some((x, y)) =
        panel::centre_seen((rect.left(), rect.top(), rect.width(), rect.height()), view)
    else {
        // Off screen: nothing to hit-test, and nothing to hide it either.
        return true;
    };
    // What is on top there, followed down through open shadow roots: a
    // component's control layer (a gesture layer over `<mux-video>`) can
    // sit in a tree outside the one the `<video>` is in.
    let Some(top) = document().and_then(|d| deepest_at(&d.into(), x, y)) else {
        return true;
    };
    if &top == element || within(&top, element) {
        return true;
    }
    // dm's own panel (its list can reach the middle of a small player).
    let host = with_panel(|p| p.host.clone());
    if host.is_some_and(|h| within(&h, &top)) {
        return true;
    }
    // The player's own layers: inside the few elements around the video
    // that make up a player (not the page itself), and about the player's
    // size — a dialog over the whole page is not one of them. The player's
    // box, not the video's: a vertical clip is letterboxed in a wide one.
    let around: Vec<Element> = std::iter::successors(composed_parent(element), composed_parent)
        .take(4)
        .take_while(|e| {
            let tag = e.tag_name().to_ascii_lowercase();
            tag != "body" && tag != "html"
        })
        .collect();
    let positioned = around.iter().find(|e| {
        window
            .get_computed_style(e)
            .ok()
            .flatten()
            .and_then(|st| st.get_property_value("position").ok())
            .is_some_and(|p| p.trim() != "static")
    });
    let area = |e: &Element| {
        let r = e.get_bounding_client_rect();
        r.width() * r.height()
    };
    if !panel::layer_fits(
        rect.width() * rect.height(),
        positioned.map(area),
        area(&top),
    ) {
        return false;
    }
    around.iter().any(|player| within(player, &top))
}

/// The element through the shadow root holding it, or its parent.
pub(super) fn composed_parent(element: &Element) -> Option<Element> {
    if let Some(parent) = element.parent_element() {
        return Some(parent);
    }
    let root: JsValue = element.get_root_node().into();
    Reflect::get(&root, &"host".into())
        .ok()
        .and_then(|h| h.dyn_into::<Element>().ok())
}

/// Whether `inner` is `outer` or inside it, shadow roots crossed.
pub(super) fn within(outer: &Element, inner: &Element) -> bool {
    panel::composed_within(inner.clone(), outer, composed_parent, 64)
}

/// The element at a point, followed into each open shadow root on top.
pub(super) fn deepest_at(root: &JsValue, x: f64, y: f64) -> Option<Element> {
    let hit = |root: &JsValue| {
        Reflect::get(root, &"elementFromPoint".into())
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
            .and_then(|f| {
                f.call2(root, &JsValue::from_f64(x), &JsValue::from_f64(y))
                    .ok()
            })
            .and_then(|t| t.dyn_into::<Element>().ok())
    };
    let mut top = hit(root)?;
    for _ in 0..8 {
        let Some(inner) = shadow_of(&top) else {
            break;
        };
        match hit(&inner) {
            Some(deeper) if deeper != top => top = deeper,
            _ => break,
        }
    }
    Some(top)
}
