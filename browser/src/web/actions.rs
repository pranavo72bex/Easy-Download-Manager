// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The context menus, the toolbar button and the extension's install and start.

use super::*;

/// "Download … with dm" in a context menu: what the item does is the app's to say, where it
/// served the menu, by one of the mechanisms this extension knows (`dm_protocol::menu_does`).
#[wasm_bindgen]
pub async fn on_menu_clicked(info: JsValue, tab: JsValue) {
    use dm_protocol::menu_does;
    let click: MenuClick = serde_json::from_value(from_js(&info)).unwrap_or_default();
    let private = get(&tab, "incognito").as_bool() == Some(true);
    let item = click.menu_item_id.as_str().unwrap_or_default().to_string();
    let served = remembered_menus().await;
    let Some(does) = decide::menu_does_for(served.as_deref(), &item) else {
        return notify(
            "dm cannot take that",
            &format!("{item} is not one of dm's menu items"),
        );
    };
    // "Download all links" and "Download selected links".
    if does == menu_does::ALL_LINKS || does == menu_does::SELECTED_LINKS {
        if let Err(why) = decide::not_private(private) {
            return notify("dm cannot take those links", &why);
        }
        let page = get(&info, "frameUrl")
            .as_string()
            .or_else(|| get(&info, "pageUrl").as_string())
            .unwrap_or_default();
        match page_links(&tab, &info, does == menu_does::SELECTED_LINKS, &page).await {
            Ok(n) => log("info", &format!("handed {n} links to the dm app")),
            Err(why) => notify("dm could not take those links", &why),
        }
        return;
    }
    if does == menu_does::APP {
        if let Err(why) = decide::not_private(private) {
            return notify("dm cannot take that", &why);
        }
        return menu_to_app(&click, &tab).await;
    }
    match decide::menu_target_doing(&click, does, private) {
        Ok(target) => hand_asked(target, tab_store(&tab, ""), tab_of(&tab, "")).await,
        Err(why) => notify("dm cannot take that", &why),
    }
}

/// A click on an item the app added to its menu: sent to the app as the browser said it, with
/// the sign-in the person hands over for its site, and the app chooses what it takes.
async fn menu_to_app(click: &MenuClick, tab: &JsValue) {
    let title = get(tab, "title").as_string().unwrap_or_default();
    let mut asked = decide::menu_clicked(click, &hand_id("c", 0, now()), &title);
    let address = decide::menu_clicked_address(&asked).to_string();
    if !address.is_empty() {
        let (cookies, once) = sign_in_for(&address, tab_store(tab, "").as_deref(), false).await;
        asked.cookies = cookies;
        asked.cookies_once = once;
    }
    let name = decide::taken_name("", &address);
    let (app, port) = app().await;
    if let App::Absent { reason } = app {
        return notify(
            "dm could not take that",
            &absent_text(&reason, &own_id(), is_firefox()),
        );
    }
    let id = asked.id.clone();
    match ask_app(port, &Request::MenuClicked(asked), &id).await {
        Ok(true) => {}
        Ok(false) => toast_sent(name),
        Err(why) => notify("dm could not take that", &why),
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
    let hidden = remembered_rules()
        .await
        .map(|rules| rules.hidden_menus)
        .unwrap_or_default();
    make_menus_without(&hidden).await;
}

/// The menus: the ones the app served, or, from an app that serves none, this extension's own
/// without the items the app's Options leave out (gaps row 86).
pub(super) async fn make_menus_without(hidden: &[String]) {
    let served = remembered_menus().await;
    let _ = call("contextMenus", "removeAll", &[]).await;
    for menu in decide::menus_to_make(served.as_deref(), hidden) {
        // In the browser's language where this extension knows the item: the menus are made
        // again at each start. The app's words for one it added.
        let title = menu.known.map_or(menu.title.clone(), |word| word.say());
        let id = menu.id.clone();
        let item =
            to_js(&serde_json::json!({ "id": menu.id, "title": title, "contexts": menu.contexts }));
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
                &format!("the {} menu item could not be made: {why}", menu.id),
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
