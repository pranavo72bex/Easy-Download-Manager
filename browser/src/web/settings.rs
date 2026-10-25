//! What is kept in the extension's storage between the worker's naps: the app's catch rules and the sites switched on or off.

use super::*;

/// Where the app's catch rules are kept between the worker's naps.
pub(super) const RULES_KEY: &str = "dm.catch";

/// Keeps the app's catch rules where a worker woken by a download can read
/// them at once. Asking the app means starting its host, which takes long
/// enough for a small download to finish in the browser first.
pub(super) async fn remember_rules(rules: &CatchRules) {
    let Ok(value) = serde_json::to_value(rules) else {
        return;
    };
    let item = to_js(&serde_json::json!({ RULES_KEY: value }));
    if let Err(why) = call("storage.local", "set", &[item]).await {
        log("debug", &format!("the catch rules were not kept: {why}"));
    }
}

/// The catch rules the app last sent, if any were kept.
pub(super) async fn remembered_rules() -> Option<CatchRules> {
    let got = call("storage.local", "get", &[JsValue::from_str(RULES_KEY)])
        .await
        .ok()?;
    serde_json::from_value(from_js(&field(&got, RULES_KEY))).ok()
}

/// Whether the toolbar popup's switch lets downloads be caught.
pub(super) async fn catching() -> bool {
    let Ok(got) = call(
        "storage.local",
        "get",
        &[JsValue::from_str(decide::CATCHING_KEY)],
    )
    .await
    else {
        return true;
    };
    field(&got, decide::CATCHING_KEY).as_bool() != Some(false)
}

/// Whether the toolbar popup has every download from the download's site
/// taken, whatever the app's rules say about its type or size: the site it
/// was asked for on, served from, or started from.
pub(super) async fn forced_for(found: &Catchable) -> bool {
    let Ok(got) = call(
        "storage.local",
        "get",
        &[JsValue::from_str(decide::CATCH_ALL_KEY)],
    )
    .await
    else {
        return false;
    };
    let list: Vec<String> =
        serde_json::from_value(from_js(&field(&got, decide::CATCH_ALL_KEY))).unwrap_or_default();
    decide::forced_download(&list, found)
}

/// The browser's cookies for `url`, as the header it would send, where the
/// toolbar popup has that switched on for the site; empty otherwise, and
/// empty on any failure. `store` is the cookie store (Firefox's container)
/// the download or tab belongs to, whose cookies are its sign-in.
pub(super) async fn cookies_for(url: &str, store: Option<&str>) -> String {
    listed_sign_in(url, store, false, false).await.0
}

/// [`listed_sign_in`] for one download: the app's "every site" switch
/// counts.
pub(super) async fn sign_in_for(url: &str, store: Option<&str>, video: bool) -> (String, bool) {
    let everywhere = remembered_rules()
        .await
        .is_some_and(|rules| rules.sign_in_everywhere);
    listed_sign_in(url, store, video, everywhere).await
}

/// The cookies a hand-off of `url` carries (`decide::cookies_sent`), and
/// whether they are for that download alone. A video page (`video`) sends
/// its session whether or not the site is listed, and so does any site
/// with the app's "every site" switch on (`everywhere`).
pub(super) async fn listed_sign_in(
    url: &str,
    store: Option<&str>,
    video: bool,
    everywhere: bool,
) -> (String, bool) {
    let Ok(got) = call(
        "storage.local",
        "get",
        &[JsValue::from_str(decide::COOKIES_SITES_KEY)],
    )
    .await
    else {
        return (String::new(), false);
    };
    let sites: Vec<String> =
        serde_json::from_value(from_js(&field(&got, decide::COOKIES_SITES_KEY)))
            .unwrap_or_default();
    let Some(once) = decide::cookies_sent(&sites, url, video, everywhere) else {
        return (String::new(), false);
    };
    let query = to_js(&decide::cookie_query(url, store));
    let Ok(listed) = call("cookies", "getAll", &[query]).await else {
        return (String::new(), false);
    };
    let cookies: Vec<decide::BrowserCookie> =
        serde_json::from_value(from_js(&listed)).unwrap_or_default();
    (decide::cookie_header(&cookies), once)
}
