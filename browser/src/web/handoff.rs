//! Handing a download to the app, and telling the person what became of it.

use super::*;

/// Forgets what is known of the app, so the next download asks again.
#[wasm_bindgen]
pub fn forget_app() {
    KNOWN.with(|k| *k.borrow_mut() = None);
}

/// What is known of the app, for a person or a harness looking at the worker.
#[wasm_bindgen]
pub fn app_status() -> String {
    KNOWN.with(|k| match &*k.borrow() {
        None => "not asked yet".into(),
        Some(Known {
            app: App::Absent { reason },
            ..
        }) => format!("absent: {reason}"),
        Some(Known {
            app: App::Present { hello, rules },
            ..
        }) => format!(
            "present: dm {} (yt-dlp: {}), catching {}",
            hello.engine_version,
            hello.ytdlp.map_or("unknown".into(), |y| y.to_string()),
            if rules.enabled {
                format!("{} file types", rules.extensions.len())
            } else {
                "nothing".into()
            }
        ),
    })
}

/// Hands one download to the app, and waits for it to say it has it.
pub(super) async fn hand(port: Option<Port>, start: Start) -> Result<(), String> {
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    let id = start.id.clone();
    // The app is told when this stops waiting, so it does not take what has
    // been given back to the browser by then.
    let deadline = now() + decide::TAKEN_WITHIN_MS;
    let start = Start {
        deadline: Some(deadline.max(0.0) as u64),
        ..start
    };
    port.send(&Request::Start(start))?;
    let outcome = loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Some(outcome) = decide::taken(&frame, &id) {
                    break outcome;
                }
            }
            Heard::Gone(why) => break Err(why),
        }
    };
    if outcome.is_err() {
        // Whatever went wrong may be the app going away; ask again next time.
        forget_app();
    }
    outcome
}

pub(super) fn notify(title: &str, message: &str) {
    log("info", &format!("{title}: {message}"));
    let icon = call_on(&api("runtime"), "getURL", &["icons/icon128.png".into()])
        .unwrap_or(JsValue::UNDEFINED);
    let options = to_js(&serde_json::json!({
        "type": "basic",
        "title": title,
        "message": message,
    }));
    let _ = Reflect::set(&options, &"iconUrl".into(), &icon);
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(why) = call("notifications", "create", &[options]).await {
            log("warn", &format!("could not show a notification: {why}"));
        }
    });
}

/// The cookie store (Firefox's container) of the tab at `path` in `from`
/// ("" for `from` itself), if the browser names one. Chrome does not.
pub(super) fn tab_store(from: &JsValue, path: &str) -> Option<String> {
    let key = if path.is_empty() {
        "cookieStoreId".to_string()
    } else {
        format!("{path}.cookieStoreId")
    };
    get(from, &key).as_string().filter(|s| !s.is_empty())
}

pub(super) fn own_id() -> String {
    api("runtime.id").as_string().unwrap_or_default()
}

pub(super) fn is_firefox() -> bool {
    call_on(&api("runtime"), "getURL", &["".into()])
        .ok()
        .and_then(|url| url.as_string())
        .is_some_and(|url| url.starts_with("moz-extension:"))
}

/* -------------------------------------------------------------- handlers */
