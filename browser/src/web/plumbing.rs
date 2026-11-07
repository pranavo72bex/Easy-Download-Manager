// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The browser's extension APIs reached by name, and the small helpers around them.

use super::*;

pub(super) fn ext() -> JsValue {
    let global: JsValue = js_sys::global().into();
    let browser = Reflect::get(&global, &"browser".into()).unwrap_or(JsValue::UNDEFINED);
    if browser.is_object() {
        browser
    } else {
        Reflect::get(&global, &"chrome".into()).unwrap_or(JsValue::UNDEFINED)
    }
}

/// `a.b.c` of an object, or undefined anywhere along the way.
/// One property of `from`, named whole. Storage keys have dots in them
/// (`dm.catch`), which [`get`] would read as a path: the catch rules kept
/// between the worker's naps were never read back that way.
pub(super) fn field(from: &JsValue, key: &str) -> JsValue {
    if from.is_object() {
        Reflect::get(from, &key.into()).unwrap_or(JsValue::UNDEFINED)
    } else {
        JsValue::UNDEFINED
    }
}

pub(super) fn get(from: &JsValue, path: &str) -> JsValue {
    path.split('.').fold(from.clone(), |at, key| {
        if at.is_object() || at.is_function() {
            Reflect::get(&at, &key.into()).unwrap_or(JsValue::UNDEFINED)
        } else {
            JsValue::UNDEFINED
        }
    })
}

pub(super) fn api(path: &str) -> JsValue {
    get(&ext(), path)
}

pub(super) fn say(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| get(error, "message").as_string())
        .or_else(|| JSON::stringify(error).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "an error with nothing to say".into())
}

pub(super) fn call_on(target: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let function: Function = get(target, method)
        .dyn_into()
        .map_err(|_| format!("{method} is not available in this browser"))?;
    let args: Array = args.iter().collect();
    function.apply(target, &args).map_err(|e| say(&e))
}

/// Calls `namespace.method(...)` and waits for it if it returned a promise.
pub(super) async fn call(
    namespace: &str,
    method: &str,
    args: &[JsValue],
) -> Result<JsValue, String> {
    let returned = call_on(&api(namespace), method, args)?;
    match returned.dyn_into::<Promise>() {
        Ok(promise) => JsFuture::from(promise).await.map_err(|e| say(&e)),
        Err(value) => Ok(value),
    }
}

pub(super) fn to_js(value: &Value) -> JsValue {
    JSON::parse(&value.to_string()).unwrap_or(JsValue::UNDEFINED)
}

pub(super) fn from_js(value: &JsValue) -> Value {
    JSON::stringify(value)
        .ok()
        .and_then(|s| s.as_string())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

pub(super) fn now() -> f64 {
    js_sys::Date::now()
}

pub(super) fn log(level: &str, message: &str) {
    let console = get(&js_sys::global().into(), "console");
    let _ = call_on(&console, level, &[format!("dm: {message}").into()]);
}

pub(super) async fn sleep(ms: f64) {
    let promise = Promise::new(&mut |resolve, _| {
        let global: JsValue = js_sys::global().into();
        let _ = call_on(&global, "setTimeout", &[resolve.into(), ms.into()]);
    });
    let _ = JsFuture::from(promise).await;
}

/// A text the worker fetches with the browser's own session (the host's
/// cookies, by the host permission): at most `cap` bytes and `ms`
/// milliseconds. The body is read as it arrives and cut at the cap, never
/// whole; `Err` says why there is no text.
pub(super) async fn fetch_text(
    url: &str,
    from: &str,
    cap: usize,
    ms: f64,
) -> Result<String, String> {
    fetch_bytes(url, from, cap, ms, false)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// The first `cap` bytes of a file, asked for by range with the browser's own session, as
/// [`fetch_text`] fetches; a server that answers the whole file is read as far as `cap`.
pub(super) async fn fetch_start(
    url: &str,
    from: &str,
    cap: usize,
    ms: f64,
) -> Result<Vec<u8>, String> {
    fetch_bytes(url, from, cap, ms, true).await
}

/// What [`fetch_text`] and [`fetch_start`] share: `start` asks for the first `cap` bytes by
/// range and keeps what came as far as `cap`, where a playlist longer than `cap` is refused.
///
/// `from` is the page or playlist that named `url`: an address on this computer or a private
/// network is fetched only for one on that very host (`decide::may_reread`). A redirect is an
/// error, never followed, as the address it leads to was never checked: a public server would
/// otherwise send the worker on to a router or a printer, with the browser's session (security
/// review, 2026-11-08). A playlist behind a redirect is left undescribed.
async fn fetch_bytes(
    url: &str,
    from: &str,
    cap: usize,
    ms: f64,
    start: bool,
) -> Result<Vec<u8>, String> {
    if !decide::may_reread(url, from) {
        return Err(
            "an address on this computer or a private network, for another site's page".into(),
        );
    }
    let global: JsValue = js_sys::global().into();
    let controller = Reflect::get(&global, &"AbortController".into())
        .ok()
        .and_then(|c| c.dyn_into::<Function>().ok())
        .and_then(|ctor| Reflect::construct(&ctor, &Array::new()).ok());
    let mut options =
        serde_json::json!({ "credentials": "include", "cache": "no-store", "redirect": "error" });
    if start {
        options["headers"] =
            serde_json::json!({ "Range": format!("bytes=0-{}", cap.saturating_sub(1)) });
    }
    let init = to_js(&options);
    if let Some(signal) = controller.as_ref().map(|c| field(c, "signal")) {
        if signal.is_object() {
            let _ = Reflect::set(&init, &"signal".into(), &signal);
        }
    }
    let fetcher = global.clone();
    let work = async move {
        let promise = call_on(&fetcher, "fetch", &[url.into(), init])?;
        let promise = promise
            .dyn_into::<Promise>()
            .map_err(|_| "the browser's fetch gave no promise".to_string())?;
        let response = JsFuture::from(promise).await.map_err(|e| say(&e))?;
        if field(&response, "ok").as_bool() != Some(true) {
            return Err(format!(
                "the server answered {}",
                field(&response, "status").as_f64().unwrap_or(0.0)
            ));
        }
        let headers = field(&response, "headers");
        let stated = call_on(&headers, "get", &["content-length".into()])
            .ok()
            .and_then(|v| v.as_string())
            .and_then(|v| v.parse::<usize>().ok());
        if !start && stated.is_some_and(|len| len > cap) {
            return Err("longer than a playlist".into());
        }
        let reader = call_on(&field(&response, "body"), "getReader", &[])?;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let read = call_on(&reader, "read", &[])?
                .dyn_into::<Promise>()
                .map_err(|_| "the body gave no promise".to_string())?;
            let chunk = JsFuture::from(read).await.map_err(|e| say(&e))?;
            if field(&chunk, "done").as_bool() == Some(true) {
                break;
            }
            let part = js_sys::Uint8Array::new(&field(&chunk, "value")).to_vec();
            if bytes.len() + part.len() > cap {
                let _ = call_on(&reader, "cancel", &[]);
                if start {
                    bytes.extend_from_slice(&part[..cap - bytes.len()]);
                    break;
                }
                return Err("longer than a playlist".into());
            }
            bytes.extend_from_slice(&part);
        }
        Ok(bytes)
    };
    match select(Box::pin(work), Box::pin(sleep(ms))).await {
        Either::Left((done, _)) => done,
        Either::Right(_) => {
            if let Some(controller) = &controller {
                let _ = call_on(controller, "abort", &[]);
            }
            Err("timed out".into())
        }
    }
}

pub(super) fn add_listener(event: &JsValue, listener: &JsValue) -> Result<(), String> {
    call_on(event, "addListener", std::slice::from_ref(listener)).map(|_| ())
}

/* ----------------------------------------------------- native messaging */
