//! The context menus, the toolbar button and the extension's install and start.

use super::*;

/// "Download … with dm" in a context menu.
#[wasm_bindgen]
pub async fn on_menu_clicked(info: JsValue, tab: JsValue) {
    let click: MenuClick = serde_json::from_value(from_js(&info)).unwrap_or_default();
    let private = get(&tab, "incognito").as_bool() == Some(true);
    // "Download all links" and "Download selected links".
    let item = click.menu_item_id.as_str().unwrap_or_default();
    if item == decide::MENU_ALL_LINKS || item == decide::MENU_SELECTED_LINKS {
        if let Err(why) = decide::not_private(private) {
            return notify("dm cannot take those links", &why);
        }
        let page = get(&info, "frameUrl")
            .as_string()
            .or_else(|| get(&info, "pageUrl").as_string())
            .unwrap_or_default();
        match page_links(&tab, &info, item == decide::MENU_SELECTED_LINKS, &page).await {
            Ok(n) => log("info", &format!("handed {n} links to the dm app")),
            Err(why) => notify("dm could not take those links", &why),
        }
        return;
    }
    match decide::menu_target(&click, private) {
        Ok(target) => hand_asked(target, tab_store(&tab, "")).await,
        Err(why) => notify("dm cannot take that", &why),
    }
}

/// The toolbar button: the page in the active tab.
#[wasm_bindgen]
pub async fn on_action_clicked(_tab: JsValue) {
    // The toolbar button opens its popup (`popup.html`), so this fires only
    // where a browser shows no popup. Then it brings dm's window up, as the
    // popup's "Open dm" does; it downloads nothing.
    if let Err(why) = show_app().await {
        notify("dm could not be opened", &why);
    }
}

/// Asks the app to show its window, starting it when it is not running.
pub(super) async fn show_app() -> Result<(), String> {
    let (app, port) = app().await;
    if let App::Absent { reason } = app {
        return Err(absent_text(&reason, &own_id(), is_firefox()));
    }
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    port.send(&Request::Show)?;
    let deadline = now() + 15_000.0;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Ok(dm_protocol::Reply::Shown { reason }) =
                    dm_protocol::Reply::from_frame(&frame)
                {
                    return reason.map_or(Ok(()), Err);
                }
            }
            Heard::Gone(why) => return Err(why),
        }
    }
}

/// Context menus are made on install and on every browser start: Chrome keeps
/// them, Firefox's event pages may not, and removing them first makes it safe
/// either way.
pub(super) async fn make_menus() {
    let _ = call("contextMenus", "removeAll", &[]).await;
    for (id, title, contexts) in decide::MENUS {
        let item = to_js(&serde_json::json!({ "id": id, "title": title, "contexts": contexts }));
        // With a callback, so a refusal is read here rather than reported
        // by the browser as an unchecked error.
        let checked = Closure::once_into_js(move || {
            if let Some(why) = api("runtime.lastError.message").as_string() {
                log(
                    "warn",
                    &format!("the {id} menu item could not be made: {why}"),
                );
            }
        });
        if let Err(why) = call_on(&api("contextMenus"), "create", &[item, checked]) {
            log(
                "warn",
                &format!("the {id} menu item could not be made: {why}"),
            );
        }
    }
}

#[wasm_bindgen]
pub async fn on_installed(_details: JsValue) {
    make_menus().await;
    // Asked now, so the catch rules are remembered before the first download.
    app().await;
}

#[wasm_bindgen]
pub async fn on_startup() {
    make_menus().await;
    app().await;
}
