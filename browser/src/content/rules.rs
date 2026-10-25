//! Where the page is, and whether the person or the app switched the panel off here.

use super::*;

/// The page a person sees: this one, or for a player in a frame, the page
/// the frame is in — so "Never on this site" on an embedded video is about
/// the site it is embedded in.
pub(super) fn top_page() -> String {
    let Some(window) = window() else {
        return String::new();
    };
    let here = window.location().href().unwrap_or_default();
    let top_is_here = window
        .top()
        .ok()
        .flatten()
        .is_some_and(|top| JsValue::from(top) == JsValue::from(window.clone()));
    if top_is_here {
        return here;
    }
    // Chrome names the frames' ancestors; Firefox leaves the referrer.
    let ancestors = Reflect::get(&window.location(), &"ancestorOrigins".into())
        .ok()
        .filter(|a| a.is_object());
    if let Some(ancestors) = ancestors {
        let count = Reflect::get(&ancestors, &"length".into())
            .ok()
            .and_then(|n| n.as_f64())
            .unwrap_or(0.0) as u32;
        if count > 0 {
            if let Some(top) = Reflect::get_u32(&ancestors, count - 1)
                .ok()
                .and_then(|t| t.as_string())
            {
                return top;
            }
        }
    }
    document()
        .map(|d| d.referrer())
        .filter(|r| !r.is_empty())
        .unwrap_or(here)
}

/// The extension's local storage area.
pub(super) fn local_storage() -> Option<JsValue> {
    let storage = Reflect::get(&ext(), &"storage".into()).ok()?;
    Reflect::get(&storage, &"local".into())
        .ok()
        .filter(|l| l.is_object())
}

/// The app's catch rules as the background keeps them (`dm.catch`): the
/// panel is off everywhere where they say so.
pub(super) const RULES_KEY: &str = "dm.catch";

/// Whether the rules, as stored, turn the panel off everywhere; applied at
/// once, and hidden where it shows.
pub(super) fn apply_rules(rules: &JsValue) {
    let off = Reflect::get(rules, &"videoPanel".into())
        .ok()
        .and_then(|v| v.as_bool())
        .is_some_and(|on| !on);
    OFF_ALL.with(|c| c.set(off));
    if off {
        hide();
    }
}

/// The app's catch rules as stored, for `apply_rules`.
pub(super) async fn stored_rules() -> JsValue {
    let Some(local) = local_storage() else {
        return JsValue::UNDEFINED;
    };
    let Ok(get) = Reflect::get(&local, &"get".into()).and_then(|g| g.dyn_into::<Function>()) else {
        return JsValue::UNDEFINED;
    };
    let Ok(promise) = get
        .call1(&local, &RULES_KEY.into())
        .and_then(|p| p.dyn_into::<Promise>())
    else {
        return JsValue::UNDEFINED;
    };
    let Ok(got) = JsFuture::from(promise).await else {
        return JsValue::UNDEFINED;
    };
    Reflect::get(&got, &RULES_KEY.into()).unwrap_or(JsValue::UNDEFINED)
}

/// The sites the panel is kept off, as stored.
pub(super) async fn sites_off() -> Vec<String> {
    let Some(local) = local_storage() else {
        return Vec::new();
    };
    let Ok(get) = Reflect::get(&local, &"get".into()).and_then(|g| g.dyn_into::<Function>()) else {
        return Vec::new();
    };
    let Ok(promise) = get
        .call1(&local, &decide::PANEL_OFF_KEY.into())
        .and_then(|p| p.dyn_into::<Promise>())
    else {
        return Vec::new();
    };
    let Ok(got) = JsFuture::from(promise).await else {
        return Vec::new();
    };
    list_of(&Reflect::get(&got, &decide::PANEL_OFF_KEY.into()).unwrap_or(JsValue::UNDEFINED))
}

pub(super) fn list_of(value: &JsValue) -> Vec<String> {
    value
        .dyn_ref::<Array>()
        .map(|a| a.iter().filter_map(|v| v.as_string()).collect())
        .unwrap_or_default()
}

/// Whether the stored list keeps the panel off this page, applied at once.
pub(super) fn apply_off(off: &[String]) {
    let off_here = decide::panel_off(off, &top_page());
    OFF_HERE.with(|c| c.set(off_here));
    if off_here {
        hide();
    }
}

/// Keeps the panel off this site from now on, here and on every page of it.
pub(super) fn never_here() {
    let Some(site) = decide::site_of(&top_page()) else {
        hide();
        return;
    };
    OFF_HERE.with(|c| c.set(true));
    hide();
    wasm_bindgen_futures::spawn_local(async move {
        let off = decide::with_site(&sites_off().await, &site, true);
        let Some(local) = local_storage() else {
            return;
        };
        let item = js_sys::Object::new();
        let list: Array = off.iter().map(|s| JsValue::from_str(s)).collect();
        let _ = Reflect::set(&item, &decide::PANEL_OFF_KEY.into(), &list);
        if let Ok(set) = Reflect::get(&local, &"set".into()).and_then(|s| s.dyn_into::<Function>())
        {
            if let Ok(promise) = set
                .call1(&local, &item)
                .and_then(|p| p.dyn_into::<Promise>())
            {
                let _ = JsFuture::from(promise).await;
            }
        }
    });
}
