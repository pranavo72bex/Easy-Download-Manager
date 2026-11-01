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
pub(super) async fn fetch_text(url: &str, cap: usize, ms: f64) -> Result<String, String> {
    let global: JsValue = js_sys::global().into();
    let controller = Reflect::get(&global, &"AbortController".into())
        .ok()
        .and_then(|c| c.dyn_into::<Function>().ok())
        .and_then(|ctor| Reflect::construct(&ctor, &Array::new()).ok());
    let init = to_js(&serde_json::json!({ "credentials": "include", "cache": "no-store" }));
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
        if stated.is_some_and(|len| len > cap) {
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
                return Err("longer than a playlist".into());
            }
            bytes.extend_from_slice(&part);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
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
