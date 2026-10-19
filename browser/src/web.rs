//! The browser side: extension APIs, native messaging, and the handlers the
//! loader forwards events to.
//!
//! The extension APIs are reached by name through `js_sys::Reflect` rather
//! than through generated bindings: they are not part of the web platform,
//! so web-sys has none, and the handful used here do not justify a binding
//! crate. `browser` is preferred where it exists (Firefox, promise-based);
//! Chrome's `chrome` returns promises too under Manifest V3.

use std::cell::RefCell;

use dm_protocol::{CatchRules, Request, Start, NATIVE_HOST};
use futures_channel::mpsc::{unbounded, UnboundedReceiver};
use futures_util::future::{select, Either};
use futures_util::StreamExt;
use js_sys::{Array, Function, Promise, Reflect, JSON};
use serde_json::Value;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::decide::{
    self, absent_text, first_look, hand_id, App, Catchable, DownloadItem, Known, MenuClick, Probe,
    Released, Target,
};

thread_local! {
    /// What is known of the app, and since when. Lost when the worker stops,
    /// which costs one more handshake and nothing else.
    static KNOWN: RefCell<Option<Known>> = const { RefCell::new(None) };
    static RELEASED: RefCell<Released> = RefCell::new(Released::default());
    /// Links clicked with the take key held: their downloads go to the app
    /// whatever its types and size floor say.
    static TAKEN: RefCell<Released> = RefCell::new(Released::default());
    /// Media each tab has loaded, for the panel's list.
    static STREAMS: RefCell<decide::Streams> = RefCell::new(decide::Streams::default());
    /// Which page each tab is on, so an `Add` that outlives its page is dropped.
    static PAGES: RefCell<decide::Pages> = RefCell::new(decide::Pages::default());
    /// Whether "update the dm app" has been said in this worker's life: once
    /// is a notice, every caught download would be nagging.
    static TOLD_OLD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Chrome's file name questions, held while dm decides (gap 122).
    static HOLDS: RefCell<decide::Holds<Function>> = RefCell::new(decide::Holds::default());
}

/* ------------------------------------------------------------- plumbing */

fn ext() -> JsValue {
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
fn field(from: &JsValue, key: &str) -> JsValue {
    if from.is_object() {
        Reflect::get(from, &key.into()).unwrap_or(JsValue::UNDEFINED)
    } else {
        JsValue::UNDEFINED
    }
}

fn get(from: &JsValue, path: &str) -> JsValue {
    path.split('.').fold(from.clone(), |at, key| {
        if at.is_object() || at.is_function() {
            Reflect::get(&at, &key.into()).unwrap_or(JsValue::UNDEFINED)
        } else {
            JsValue::UNDEFINED
        }
    })
}

fn api(path: &str) -> JsValue {
    get(&ext(), path)
}

fn say(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| get(error, "message").as_string())
        .or_else(|| JSON::stringify(error).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "an error with nothing to say".into())
}

fn call_on(target: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let function: Function = get(target, method)
        .dyn_into()
        .map_err(|_| format!("{method} is not available in this browser"))?;
    let args: Array = args.iter().collect();
    function.apply(target, &args).map_err(|e| say(&e))
}

/// Calls `namespace.method(...)` and waits for it if it returned a promise.
async fn call(namespace: &str, method: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let returned = call_on(&api(namespace), method, args)?;
    match returned.dyn_into::<Promise>() {
        Ok(promise) => JsFuture::from(promise).await.map_err(|e| say(&e)),
        Err(value) => Ok(value),
    }
}

fn to_js(value: &Value) -> JsValue {
    JSON::parse(&value.to_string()).unwrap_or(JsValue::UNDEFINED)
}

fn from_js(value: &JsValue) -> Value {
    JSON::stringify(value)
        .ok()
        .and_then(|s| s.as_string())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn log(level: &str, message: &str) {
    let console = get(&js_sys::global().into(), "console");
    let _ = call_on(&console, level, &[format!("dm: {message}").into()]);
}

async fn sleep(ms: f64) {
    let promise = Promise::new(&mut |resolve, _| {
        let global: JsValue = js_sys::global().into();
        let _ = call_on(&global, "setTimeout", &[resolve.into(), ms.into()]);
    });
    let _ = JsFuture::from(promise).await;
}

fn add_listener(event: &JsValue, listener: &JsValue) -> Result<(), String> {
    call_on(event, "addListener", std::slice::from_ref(listener)).map(|_| ())
}

/* ----------------------------------------------------- native messaging */

enum Heard {
    Frame(Value),
    Gone(String),
}

/// One connection to the host: a process the browser starts for it, and
/// stops when it is dropped.
struct Port {
    port: JsValue,
    heard: UnboundedReceiver<Heard>,
    on_message: Closure<dyn FnMut(JsValue)>,
    on_disconnect: Closure<dyn FnMut(JsValue)>,
}

impl Port {
    fn open() -> Result<Port, String> {
        let port = call_on(&api("runtime"), "connectNative", &[NATIVE_HOST.into()])?;
        let (tell, heard) = unbounded();
        let told = tell.clone();
        let on_message = Closure::<dyn FnMut(JsValue)>::new(move |message: JsValue| {
            let _ = tell.unbounded_send(Heard::Frame(from_js(&message)));
        });
        let on_disconnect = Closure::<dyn FnMut(JsValue)>::new(move |port: JsValue| {
            // Firefox says why on the port; Chrome in lastError.
            let why = get(&port, "error.message")
                .as_string()
                .or_else(|| api("runtime.lastError.message").as_string())
                .unwrap_or_else(|| "the dm app closed the connection".into());
            let _ = told.unbounded_send(Heard::Gone(why));
        });
        add_listener(&get(&port, "onMessage"), on_message.as_ref())?;
        add_listener(&get(&port, "onDisconnect"), on_disconnect.as_ref())?;
        Ok(Port {
            port,
            heard,
            on_message,
            on_disconnect,
        })
    }

    fn send(&self, request: &Request) -> Result<(), String> {
        call_on(&self.port, "postMessage", &[to_js(&request.to_frame())]).map(|_| ())
    }

    /// The next thing the host says, or why there will be nothing.
    async fn next(&mut self, deadline: f64) -> Heard {
        let left = deadline - now();
        if left <= 0.0 {
            return Heard::Gone("the dm app did not answer in time".into());
        }
        let heard = Box::pin(self.heard.next());
        let timer = Box::pin(sleep(left));
        match select(heard, timer).await {
            Either::Left((Some(heard), _)) => heard,
            Either::Left((None, _)) => Heard::Gone("the connection to dm was lost".into()),
            Either::Right(_) => Heard::Gone("the dm app did not answer in time".into()),
        }
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        // Listeners first, so nothing calls into closures about to be freed.
        let _ = call_on(
            &get(&self.port, "onMessage"),
            "removeListener",
            &[self.on_message.as_ref().clone()],
        );
        let _ = call_on(
            &get(&self.port, "onDisconnect"),
            "removeListener",
            &[self.on_disconnect.as_ref().clone()],
        );
        let _ = call_on(&self.port, "disconnect", &[]);
    }
}

/// Asks the host whether it is there and what it wants caught. The port is
/// returned open when it answered, so a hand-off can follow on it rather
/// than start the host a second time.
async fn probe() -> (App, Option<Port>) {
    let mut port = match Port::open() {
        Ok(port) => port,
        Err(why) => return (App::Absent { reason: why }, None),
    };
    if let Err(why) = port
        .send(&Request::Hello)
        .and_then(|()| port.send(&Request::Settings))
    {
        return (App::Absent { reason: why }, None);
    }

    let mut probe = Probe::default();
    let mut deadline = now() + decide::HELLO_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                let had_hello = probe.has_hello();
                probe.absorb(&frame);
                if let Some(app) = probe.finished() {
                    let open = matches!(app, App::Present { .. }).then_some(port);
                    return (app, open);
                }
                if probe.has_hello() && !had_hello {
                    deadline = deadline.min(now() + decide::SETTINGS_WITHIN_MS);
                }
            }
            // Gone or silent: a hand-off that follows opens a port of its own.
            Heard::Gone(why) => return (probe.give_up(&why), None),
        }
    }
}

/// What is known of the app, asked again when the answer has gone stale.
async fn app() -> (App, Option<Port>) {
    let at = now();
    if let Some(known) = KNOWN.with(|k| k.borrow().clone()) {
        if known.fresh(at) {
            return (known.app, None);
        }
    }
    let (app, port) = probe().await;
    if let App::Absent { reason } = &app {
        log("info", &format!("no app to hand downloads to: {reason}"));
    }
    if let App::Present { rules, .. } = &app {
        remember_rules(rules).await;
    }
    KNOWN.with(|k| {
        *k.borrow_mut() = Some(Known {
            app: app.clone(),
            at_ms: at,
        })
    });
    (app, port)
}

/// Where the app's catch rules are kept between the worker's naps.
const RULES_KEY: &str = "dm.catch";

/// Keeps the app's catch rules where a worker woken by a download can read
/// them at once. Asking the app means starting its host, which takes long
/// enough for a small download to finish in the browser first.
async fn remember_rules(rules: &CatchRules) {
    let Ok(value) = serde_json::to_value(rules) else {
        return;
    };
    let item = to_js(&serde_json::json!({ RULES_KEY: value }));
    if let Err(why) = call("storage.local", "set", &[item]).await {
        log("debug", &format!("the catch rules were not kept: {why}"));
    }
}

/// The catch rules the app last sent, if any were kept.
async fn remembered_rules() -> Option<CatchRules> {
    let got = call("storage.local", "get", &[JsValue::from_str(RULES_KEY)])
        .await
        .ok()?;
    serde_json::from_value(from_js(&field(&got, RULES_KEY))).ok()
}

/// Whether the toolbar popup's switch lets downloads be caught.
async fn catching() -> bool {
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
async fn forced_for(found: &Catchable) -> bool {
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
/// toolbar popup has that switched on for the site and the browser granted
/// the `cookies` permission; empty otherwise, and empty on any failure.
/// `store` is the cookie store (Firefox's container) the download or tab
/// belongs to, whose cookies are its sign-in.
async fn cookies_for(url: &str, store: Option<&str>) -> String {
    let Ok(got) = call(
        "storage.local",
        "get",
        &[JsValue::from_str(decide::COOKIES_SITES_KEY)],
    )
    .await
    else {
        return String::new();
    };
    let sites: Vec<String> =
        serde_json::from_value(from_js(&field(&got, decide::COOKIES_SITES_KEY)))
            .unwrap_or_default();
    if !decide::cookies_wanted(&sites, url) {
        return String::new();
    }
    let wanted = to_js(&serde_json::json!({ "permissions": ["cookies"] }));
    let granted = call("permissions", "contains", &[wanted])
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !granted {
        return String::new();
    }
    let query = to_js(&decide::cookie_query(url, store));
    let Ok(listed) = call("cookies", "getAll", &[query]).await else {
        return String::new();
    };
    let cookies: Vec<decide::BrowserCookie> =
        serde_json::from_value(from_js(&listed)).unwrap_or_default();
    decide::cookie_header(&cookies)
}

/// Where a tab's streams are kept between the worker's naps: the session's
/// storage, which the browser clears when it closes.
fn streams_key(tab: i64) -> String {
    format!("dm.streams.{tab}")
}

/// A response the browser received, read for media a page's player loads,
/// to find a stream on sites no page reader knows.
#[wasm_bindgen]
pub async fn on_response_started(details: JsValue) {
    let seen: decide::Seen = match serde_json::from_value(from_js(&details)) {
        Ok(seen) => seen,
        Err(_) => return,
    };
    let tab = seen.tab_id;
    if tab < 0 {
        return;
    }
    let kept = match decide::keep(&seen) {
        decide::Keep::Nothing => return,
        // A new page: the old one's streams go from memory and from the
        // kept copy, which a worker woken by this navigation never read.
        decide::Keep::Forget => {
            PAGES.with(|p| p.borrow_mut().next(tab));
            STREAMS.with(|s| s.borrow_mut().forget(tab));
            forget_kept(tab).await;
            return;
        }
        decide::Keep::Add => {
            // What was kept before the worker slept, merged in first, so the
            // copy written back adds to it rather than replacing it.
            let page = PAGES.with(|p| p.borrow().now(tab));
            if !STREAMS.with(|s| s.borrow().knows(tab)) {
                let kept = kept_streams(tab).await;
                // The tab moved on while the copy was read: this response
                // and the copy are the old page's.
                if !PAGES.with(|p| p.borrow().still(tab, page)) {
                    return;
                }
                STREAMS.with(|s| s.borrow_mut().restore(tab, kept));
            }
            let before = STREAMS.with(|s| s.borrow().raw(tab));
            STREAMS.with(|s| s.borrow_mut().absorb(&seen));
            let after = STREAMS.with(|s| s.borrow().raw(tab));
            if after == before {
                return;
            }
            let item = to_js(&serde_json::json!({ streams_key(tab): after }));
            call("storage.session", "set", &[item]).await
        }
    };
    if let Err(why) = kept {
        log("debug", &format!("a tab's streams were not kept: {why}"));
    }
}

/// A tab's streams as kept in the session's storage.
async fn kept_streams(tab: i64) -> Vec<decide::Stream> {
    let key = streams_key(tab);
    match call("storage.session", "get", &[JsValue::from_str(&key)]).await {
        Ok(got) => serde_json::from_value(from_js(&field(&got, &key))).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Removes the kept copy of a tab's streams.
async fn forget_kept(tab: i64) {
    let key = streams_key(tab);
    if let Err(why) = call("storage.session", "remove", &[JsValue::from_str(&key)]).await {
        log(
            "debug",
            &format!("a tab's streams were not forgotten: {why}"),
        );
    }
}

/// A tab closed: what it loaded is forgotten, here and in the kept copy.
#[wasm_bindgen]
pub async fn on_tab_removed(tab: JsValue, _info: JsValue) {
    let Some(tab) = tab.as_f64().map(|t| t as i64).filter(|t| *t >= 0) else {
        return;
    };
    PAGES.with(|p| p.borrow_mut().next(tab));
    STREAMS.with(|s| s.borrow_mut().forget(tab));
    forget_kept(tab).await;
}

/// What a tab has loaded, from this worker or, after it stopped, from the
/// session's storage.
async fn streams_of(tab: i64) -> Vec<decide::Stream> {
    if tab >= 0 && !STREAMS.with(|s| s.borrow().knows(tab)) {
        let kept = kept_streams(tab).await;
        STREAMS.with(|s| s.borrow_mut().restore(tab, kept));
    }
    STREAMS.with(|s| s.borrow().of(tab))
}

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
async fn hand(port: Option<Port>, start: Start) -> Result<(), String> {
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

fn notify(title: &str, message: &str) {
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
fn tab_store(from: &JsValue, path: &str) -> Option<String> {
    let key = if path.is_empty() {
        "cookieStoreId".to_string()
    } else {
        format!("{path}.cookieStoreId")
    };
    get(from, &key).as_string().filter(|s| !s.is_empty())
}

fn own_id() -> String {
    api("runtime.id").as_string().unwrap_or_default()
}

fn is_firefox() -> bool {
    call_on(&api("runtime"), "getURL", &["".into()])
        .ok()
        .and_then(|url| url.as_string())
        .is_some_and(|url| url.starts_with("moz-extension:"))
}

/* -------------------------------------------------------------- handlers */

#[wasm_bindgen(start)]
pub fn started() {
    std::panic::set_hook(Box::new(|panic| log("error", &panic.to_string())));
}

/// A download the browser has begun. Taken when the app wants it and takes
/// it; otherwise left exactly as it was.
#[wasm_bindgen]
pub async fn on_download_created(item: JsValue) {
    let item: DownloadItem = serde_json::from_value(from_js(&item)).unwrap_or_default();
    // However this ends, Chrome's question about the file's name is
    // answered, or dropped with the download dm took (gap 122).
    let mut decided = Decided {
        id: item.id.map(|n| n as i64),
        taken: false,
    };
    // The toolbar popup's switch: off, the browser keeps every download.
    if !catching().await {
        return log("debug", "left with the browser: catching is switched off");
    }
    let found = match first_look(&item, &own_id()) {
        Ok(found) => found,
        Err(why) => return log("debug", &format!("left with the browser: {why}")),
    };
    let asked_for = item.url.clone().unwrap_or_default();
    if RELEASED.with(|r| {
        let mut released = r.borrow_mut();
        released.take(&found.url, now()) || released.take(&asked_for, now())
    }) {
        return log(
            "debug",
            "left with the browser: given back to it a moment ago",
        );
    }

    // Held while the app is asked, so the browser does not finish it in the
    // meantime and leave the file in two places. When the rules the app last
    // sent would catch it, it is held before the app is asked at all:
    // starting the host takes long enough for a small download to finish.
    let id = JsValue::from_f64(found.id as f64);
    let pause = || async {
        call("downloads", "pause", std::slice::from_ref(&id))
            .await
            .is_ok()
    };
    // With nothing remembered yet (before the app has ever answered), every
    // candidate is held; one the app turns out not to want is given back.
    // A site the popup takes everything from is held whatever the rules say
    // about type and size; the app's own switch and skip list still hold.
    let clicked = TAKEN.with(|t| {
        let mut taken = t.borrow_mut();
        taken.take(&found.url, now()) || taken.take(&asked_for, now())
    });
    let forced = clicked || forced_for(&found).await;
    let early = remembered_rules()
        .await
        .is_none_or(|rules| rules.takes(&found.candidate(), forced).is_ok());
    let mut paused = early && pause().await;

    let (app, port) = app().await;
    let verdict = match &app {
        App::Present { rules, hello } => rules
            .takes(&found.candidate(), forced)
            .map_err(|left| left.reason().to_string())
            .and_then(|()| {
                // An app too old to tell a sign-in page from the file would
                // take the page, and the browser's copy would be erased.
                decide::takes_caught(hello).map_err(|old| {
                    if !TOLD_OLD.with(|t| t.replace(true)) {
                        notify("Update the dm app", old);
                    }
                    old.to_string()
                })
            }),
        App::Absent { .. } => Err("no app to hand it to".to_string()),
    };
    if let Err(why) = verdict {
        if paused {
            give_back(&id, &found.url).await;
        }
        return log("debug", &format!("left with the browser: {why}"));
    }

    if !paused {
        paused = pause().await;
    }
    // Not held: it may have finished already (a small file served fast, a
    // PDF in Chromium above all); that still goes, by its address (gap 103).
    if !paused {
        if let Err(why) = decide::unheld(state_of(found.id).await.as_deref()) {
            return log("debug", &format!("left with the browser: {why}"));
        }
    }

    let start = Start {
        id: hand_id("b", found.id, now()),
        url: found.url.clone(),
        referrer: found.referrer.clone(),
        // The browser holds it: dm checks it can fetch it before taking it,
        // and only an app that judges what it is served may take it.
        caught: true,
        held: true,
        // The site's sign-in, where the popup has that on for the site.
        cookies: cookies_for(&found.url, found.store.as_deref()).await,
        // What the browser is being sent, so the app can tell a sign-in
        // page served to it in the file's place, and refuse.
        mime: found.mime.clone(),
        ..Start::default()
    };
    match hand(port, start).await {
        Ok(()) => {
            log("info", &format!("handed {} to the dm app", found.url));
            // Said in the browser, where the download just vanished; dm's
            // own window may be behind it (UI round 13).
            notify(
                "dm is downloading it",
                &decide::taken_name(&found.filename, &found.url),
            );
            // Held or not, it can have finished in the browser meanwhile:
            // a pause does not stop what Chromium has already received.
            let (verb, what) = match decide::after_taken(state_of(found.id).await.as_deref()) {
                decide::BrowserCopy::Cancel => ("cancelled", "cancel"),
                decide::BrowserCopy::Remove => ("removed", "removeFile"),
            };
            let mut done = call("downloads", what, std::slice::from_ref(&id)).await;
            // Finished between the look and the cancel: a fast file (a
            // 300 KB PDF) stayed whole in the browser's folder beside dm's,
            // its list entry erased below (Linux QA, 2026-10-19). Chromium's
            // cancel of a finished download succeeds and does nothing, so
            // the state is looked at after every cancel, and a finished copy
            // removed.
            if what == "cancel" {
                for _ in 0..20 {
                    match state_of(found.id).await.as_deref() {
                        Some("complete") => {
                            done = call("downloads", "removeFile", std::slice::from_ref(&id)).await;
                            break;
                        }
                        Some("in_progress") => sleep(100.0).await,
                        // Interrupted, as a cancel that took leaves it, or gone.
                        _ => break,
                    }
                }
            }
            match done {
                // Gone from the browser before it named the file: its Save
                // dialog, where it would have asked, never opens.
                Ok(_) => decided.taken = true,
                Err(why) => log(
                    "warn",
                    &format!("the browser's copy could not be {verb}: {why}"),
                ),
            }
            let query = to_js(&serde_json::json!({ "id": found.id }));
            let _ = call("downloads", "erase", &[query]).await;
        }
        Err(why) => {
            log(
                "warn",
                &format!(
                    "the dm app did not take {}, so the browser keeps it: {why}",
                    found.url
                ),
            );
            if paused {
                give_back(&id, &found.url).await;
            }
        }
    }
}

/// Chrome asking what to name a download (Chrome only; Firefox has no such
/// event). Held until `on_download_created` has decided, so a download dm
/// takes is gone before Chrome would open its Save dialog for it: it asks
/// when the name is too long for the disk, or when "Ask where to save each
/// file" is on, and a dialog opened before the cancel stayed open (gap 122).
/// The loader returns `true` for it, which tells Chrome the answer comes later.
#[wasm_bindgen]
pub fn on_determining_filename(item: JsValue, suggest: JsValue) {
    let Ok(suggest) = suggest.dyn_into::<Function>() else {
        return;
    };
    let Some(id) = get(&item, "id").as_f64().map(|n| n as i64) else {
        return name_as_the_browser_would(&suggest);
    };
    if let Some(now) = HOLDS.with(|h| h.borrow_mut().ask(id, suggest)) {
        return name_as_the_browser_would(&now);
    }
    // Whatever happens to the decision, the browser is not held for ever.
    wasm_bindgen_futures::spawn_local(async move {
        sleep(decide::DETERMINE_WITHIN_MS).await;
        if let Some(late) = HOLDS.with(|h| h.borrow_mut().expire(id)) {
            log(
                "warn",
                "a download's name was held too long; the browser names it",
            );
            name_as_the_browser_would(&late);
        }
    });
}

/// Answers Chrome's question with no suggestion: the name it chose stands.
fn name_as_the_browser_would(suggest: &Function) {
    if let Err(why) = suggest.call0(&JsValue::UNDEFINED) {
        log(
            "debug",
            &format!("the browser's name question: {}", say(&why)),
        );
    }
}

/// Settles Chrome's file name question for a download once its `onCreated`
/// has run its course, on every way out of it.
struct Decided {
    id: Option<i64>,
    /// Taken by dm and cancelled or removed in the browser.
    taken: bool,
}

impl Drop for Decided {
    fn drop(&mut self) {
        let Some(id) = self.id else { return };
        if self.taken {
            HOLDS.with(|h| h.borrow_mut().drop_taken(id));
        } else if let Some(suggest) = HOLDS.with(|h| h.borrow_mut().decide(id)) {
            name_as_the_browser_would(&suggest);
        }
    }
}

/// The state the browser reports for a download now: `in_progress`,
/// `interrupted` or `complete`; `None` once it is gone from the list.
async fn state_of(id: i64) -> Option<String> {
    let query = to_js(&serde_json::json!({ "id": id }));
    let found = call("downloads", "search", &[query])
        .await
        .unwrap_or_default();
    get(&found, "0.state").as_string()
}

/// Lets the browser carry on with a download it was holding. Where it cannot
/// be resumed — Firefox cannot resume what the server will not range — it is
/// started again, marked so it is not caught a second time.
async fn give_back(id: &JsValue, url: &str) {
    if call("downloads", "resume", std::slice::from_ref(id))
        .await
        .is_ok()
    {
        return;
    }
    // Finished while it was held (Chromium completes a fast PDF despite the
    // pause): the browser has the file, and starting it again would make two.
    if let Some(n) = id.as_f64() {
        if state_of(n as i64).await.as_deref() == Some("complete") {
            return;
        }
    }
    RELEASED.with(|r| r.borrow_mut().remember(url, now()));
    let options = to_js(&serde_json::json!({ "url": url }));
    if let Err(why) = call("downloads", "download", &[options]).await {
        log(
            "error",
            &format!("{url} could not be given back to the browser: {why}"),
        );
        notify(
            "A download could not be kept",
            &format!("Neither dm nor the browser could carry on with {url}: {why}"),
        );
    }
}

/// A file the person asked dm for that dm could not take, downloaded by the
/// browser instead: the
/// person asked for the file, and gets it either way. A page (a player for
/// yt-dlp) has nothing the browser could save, so its reason stands.
async fn in_browser_instead(target: &Target, why: &str) -> Result<String, String> {
    // Only what the browser downloads: a magnet given to its downloader was
    // accepted and failed, said as taken, and nothing was opened (torrent
    // check, 2026-10-19). The caller gives a magnet back to the page.
    let web = ["http://", "https://", "ftp://"]
        .iter()
        .any(|s| target.url.to_ascii_lowercase().starts_with(s));
    if target.page || !web {
        return Err(why.to_string());
    }
    RELEASED.with(|r| r.borrow_mut().remember(&target.url, now()));
    let options = to_js(&serde_json::json!({ "url": target.url }));
    match call("downloads", "download", &[options]).await {
        Ok(_) => {
            log(
                "info",
                &format!("{} left to the browser: {why}", target.url),
            );
            Ok(format!("the browser is downloading it (dm: {why})"))
        }
        Err(browser) => Err(format!(
            "{why}; and the browser could not either: {browser}"
        )),
    }
}

/// Hands something a person asked for. `Ok` carries a word for whoever
/// asked; `Err` why it could not be done.
async fn hand_target(target: Target, store: Option<String>) -> Result<String, String> {
    hand_chosen(target, store, None, None, false).await
}

/// Hands one download over, with the quality picked from the panel's list
/// and, as for a video from the panel, asking the app to show
/// "Download details" before it starts.
async fn hand_chosen(
    target: Target,
    store: Option<String>,
    quality: Option<String>,
    label: Option<String>,
    ask: bool,
) -> Result<String, String> {
    let (app, port) = app().await;
    let hello = match app {
        App::Absent { reason } => {
            let why = absent_text(&reason, &own_id(), is_firefox());
            return in_browser_instead(&target, &why).await;
        }
        App::Present { hello, .. } => hello,
    };
    let cookies = cookies_for(&target.url, store.as_deref()).await;
    let start = Start {
        id: hand_id("m", 0, now()),
        url: target.url.clone(),
        referrer: target.referrer.clone(),
        cookies,
        // A file is checked before dm takes it, as a caught download is: a
        // site that refuses dm (W3Schools did, with 403, on one network)
        // would otherwise leave a failed row and nothing downloaded.
        caught: !target.page,
        // A page is handed over for its media, picked or not: with a
        // quality the app never saves the page itself in the video's place
        // when yt-dlp cannot read it (UI round 9).
        quality: quality.or_else(|| target.page.then(|| "best".to_string())),
        label,
        ask,
        ..Start::default()
    };
    if let Err(why) = hand(port, start).await {
        return in_browser_instead(&target, &why).await;
    }
    log("info", &format!("handed {} to the dm app", target.url));
    if target.page && hello.ytdlp == Some(false) {
        return Ok(decide::NO_YTDLP.to_string());
    }
    Ok(if ask {
        "the dm app has it".into()
    } else if target.page {
        "the dm app is fetching it with yt-dlp".into()
    } else {
        "the dm app is downloading it".into()
    })
}

/// The qualities a player page's video comes in, from the app's yt-dlp. A
/// file is one quality, so it has no list.
async fn formats_of(target: &Target) -> Result<Vec<dm_protocol::Choice>, String> {
    if !target.page {
        return Ok(Vec::new());
    }
    let (app, port) = app().await;
    let hello = match app {
        App::Absent { reason } => return Err(absent_text(&reason, &own_id(), is_firefox())),
        App::Present { hello, .. } => hello,
    };
    if hello.ytdlp == Some(false) {
        return Err(decide::NO_YTDLP.to_string());
    }
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    port.send(&Request::Formats(dm_protocol::FormatsAsked {
        url: target.url.clone(),
        referrer: target.referrer.clone(),
    }))?;
    // yt-dlp reads a page in a few seconds; the app gives it thirty.
    let deadline = now() + 40_000.0;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Ok(dm_protocol::Reply::Formats { choices, reason }) =
                    dm_protocol::Reply::from_frame(&frame)
                {
                    return match reason {
                        Some(why) if choices.is_empty() => Err(why),
                        _ => Ok(choices),
                    };
                }
            }
            Heard::Gone(why) => return Err(why),
        }
    }
}

/// Hands something a person asked for, and says so when it cannot.
async fn hand_asked(target: Target, store: Option<String>) {
    let name = if target.url.to_ascii_lowercase().starts_with("magnet:") {
        decide::magnet_name(&target.url)
    } else {
        decide::taken_name("", &target.url)
    };
    match hand_target(target, store).await {
        Ok(said) if said == decide::NO_YTDLP => notify("dm", &said),
        // Left to the browser, which is downloading it: said so, not as
        // dm's (torrent check, 2026-10-19).
        Ok(said) if said.starts_with("the browser is downloading it") => notify("dm", &said),
        // Said, as a caught download is: nothing in the browser showed it
        // had been taken (UI round 13).
        Ok(_) => notify("dm is downloading it", &name),
        Err(why) => notify("dm could not take that", &why),
    }
}

/// A click on the floating panel the page script draws over a player.
/// Answered, so the panel can say what happened; only this extension's own
/// page script, in a tab, is listened to.
#[wasm_bindgen]
pub async fn on_page_message(message: JsValue, sender: JsValue) -> JsValue {
    let answer = |ok: bool, said: &str| to_js(&serde_json::json!({ "ok": ok, "said": said }));
    if get(&sender, "id").as_string().unwrap_or_default() != own_id() {
        return answer(false, "that message did not come from dm");
    }
    // The toolbar popup: how the app is, and bringing it up.
    match get(&message, "type").as_string().as_deref() {
        Some(decide::STATUS_MESSAGE) => {
            let (app, _) = app().await;
            return to_js(&match app {
                App::Present { hello, .. } => {
                    let (line, bad) = decide::status_line(&hello);
                    serde_json::json!({
                        "ok": true,
                        "version": hello.engine_version,
                        "ytdlp": hello.ytdlp,
                        "line": line,
                        "bad": bad,
                    })
                }
                // With no host, the popup's Open dm has nothing to ask.
                App::Absent { reason } => serde_json::json!({
                    "ok": false,
                    "said": decide::absent_popup_text(&reason, &own_id(), is_firefox()),
                    "host": !decide::not_connected(&reason),
                }),
            });
        }
        Some(decide::OPEN_MESSAGE) => {
            return match show_app().await {
                Ok(()) => answer(true, "dm is open"),
                Err(why) => answer(false, &why),
            };
        }
        _ => {}
    }
    let tab_url = get(&sender, "tab.url").as_string().unwrap_or_default();
    let private = get(&sender, "tab.incognito").as_bool() == Some(true);
    let click: decide::PanelClick = serde_json::from_value(from_js(&message)).unwrap_or_default();
    // A magnet link clicked on a page: the app's, as a clicked .torrent is;
    // and where the app cannot take it, the browser does what it would
    // have done, opening it with whatever the system has for magnets.
    if click.kind == decide::MAGNET_MESSAGE {
        let url = match decide::magnet_url(&click) {
            Ok(url) => url,
            Err(why) => return answer(false, &why),
        };
        let target = Target {
            url: url.clone(),
            referrer: tab_url.clone(),
            page: false,
        };
        // The popup's "Catch downloads" switch off: the browser's, as a
        // clicked file would be (torrent check, 2026-10-19).
        let refused = if private {
            Some("dm takes nothing from a private window".to_string())
        } else if !catching().await {
            Some("catching is switched off".to_string())
        } else {
            match hand_target(target, tab_store(&sender, "tab")).await {
                Ok(_) => {
                    notify("dm is downloading it", &decide::magnet_name(&url));
                    return answer(true, "the app takes it");
                }
                Err(why) => Some(why),
            }
        };
        let why = refused.unwrap_or_default();
        log(
            "info",
            &format!("a clicked magnet is left to the browser: {why}"),
        );
        // The page opens it on this answer, as it would have without dm,
        // so the browser's prompt names the site, not the extension.
        return answer(false, &why);
    }
    // A click with the take key held: the download about to start is the
    // app's, whatever the types and size floor say.
    // Handed over as "Download link with dm" is: a link that is not itself
    // a download (Shift opened it in a new window, round 17) reaches the
    // app all the same. The page script kept the browser from acting on it.
    if click.kind == decide::TAKE_MESSAGE {
        return match decide::take_url(&click) {
            Ok(url) => {
                TAKEN.with(|t| t.borrow_mut().remember(&url, now()));
                let menu = decide::MenuClick {
                    menu_item_id: serde_json::Value::String(decide::MENU_LINK.to_string()),
                    link_url: Some(url),
                    page_url: Some(tab_url.clone()),
                    ..decide::MenuClick::default()
                };
                match decide::menu_target(&menu, private) {
                    Ok(target) => {
                        hand_asked(target, tab_store(&sender, "tab")).await;
                        answer(true, "the app takes it")
                    }
                    Err(why) => answer(false, &why),
                }
            }
            Err(why) => answer(false, &why),
        };
    }
    // Alt-click: the download about to start is the browser's.
    if click.kind == decide::SKIP_MESSAGE {
        return match decide::skip_url(&click) {
            Ok(url) => {
                RELEASED.with(|r| r.borrow_mut().remember(&url, now()));
                // Firefox does not download on Alt-click as Chromium does;
                // asked to, it downloads it itself, and dm leaves it be
                // (it did nothing at all, round 17).
                if is_firefox() {
                    let wanted = to_js(&serde_json::json!({ "url": url }));
                    if let Err(why) = call("downloads", "download", &[wanted]).await {
                        return answer(false, &why);
                    }
                }
                answer(true, "the browser keeps it")
            }
            Err(why) => answer(false, &why),
        };
    }
    let target = match decide::panel_target(&click, &tab_url, private) {
        Ok(target) => target,
        Err(why) => return answer(false, &why),
    };
    // What the video comes in, for the panel's list: the qualities the
    // app's yt-dlp reads from the page, then the streams the page loaded.
    // A site yt-dlp does not know still offers what its player fetched.
    if click.kind == decide::FORMATS_MESSAGE {
        let tab = get(&sender, "tab.id").as_f64().map_or(-1, |id| id as i64);
        let streams = streams_of(tab).await;
        let read = formats_of(&target).await;
        let mut lines: Vec<serde_json::Value> = match &read {
            Ok(choices) => choices
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "quality": c.quality,
                        "label": c.label,
                        "line": decide::choice_line(&c.label, c.size),
                    })
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        lines.extend(streams.iter().map(|s| {
            serde_json::json!({
                "url": s.url,
                "label": s.label,
                "line": decide::choice_line(&s.label, s.size),
            })
        }));
        return match read {
            Err(why) if lines.is_empty() => answer(false, &why),
            _ => to_js(&serde_json::json!({ "ok": true, "choices": lines })),
        };
    }
    // A click on the bar or on a line of its list: handed over to be asked
    // about in the app, which opens "Download details".
    match hand_chosen(
        target,
        tab_store(&sender, "tab"),
        click.quality,
        click.label,
        true,
    )
    .await
    {
        Ok(said) => answer(true, &said),
        Err(why) => answer(false, &why),
    }
}

/// Asks the page (the frame that was right-clicked) for its links, all or
/// the selected ones, and hands them to the app together, for its "Download
/// all links" dialog. Returns how many.
async fn page_links(
    tab: &JsValue,
    info: &JsValue,
    selected: bool,
    page: &str,
) -> Result<usize, String> {
    let tab_id = get(tab, "id");
    if tab_id.as_f64().is_none() {
        return Err("that is not a page dm can read links from".into());
    }
    let frame_id = get(info, "frameId").as_f64().unwrap_or(0.0);
    let asked = to_js(&serde_json::json!({ "type": decide::LINKS_MESSAGE, "selected": selected }));
    let frame = to_js(&serde_json::json!({ "frameId": frame_id }));
    let args = [tab_id.clone(), asked, frame];
    let ask = || call("tabs", "sendMessage", &args);
    let answer = match ask().await {
        Ok(answer) => answer,
        // No page script answered: a tab open before dm was installed or
        // updated (Chromium puts content scripts only into pages loaded
        // since). It is given one now, where the browser allows, and asked
        // again (gap 122).
        Err(first) => {
            if !decide::injectable(page) {
                return Err(decide::NO_LINKS_HERE.into());
            }
            let target = to_js(&serde_json::json!({
                "target": { "tabId": tab_id.as_f64(), "frameIds": [frame_id] },
                "files": ["content.js"],
                // Now, not once the page has finished arriving.
                "injectImmediately": true,
            }));
            if let Err(why) = call("scripting", "executeScript", &[target]).await {
                log("warn", &format!("the links reader could not be put into {page}: {why} (asked first: {first})"));
                return Err(decide::NO_LINKS_HERE.into());
            }
            // Asked a few times: a reader just put in can take a moment to
            // be listening on a page still arriving (Linux QA, 2026-10-19:
            // 0 of 3 links on one machine, not seen elsewhere).
            let mut answer = ask().await;
            for _ in 0..3 {
                if answer.is_ok() {
                    break;
                }
                sleep(150.0).await;
                answer = ask().await;
            }
            answer.map_err(|why| {
                log(
                    "warn",
                    &format!("{page} did not answer for its links: {why} (asked first: {first})"),
                );
                decide::NO_LINKS_HERE.to_string()
            })?
        }
    };
    let hrefs: Vec<String> =
        serde_json::from_value(from_js(&get(&answer, "links"))).unwrap_or_default();
    let urls = decide::links_from(&hrefs, page);
    if urls.is_empty() {
        return Err(if selected {
            "the selection has no links".into()
        } else {
            "this page has no links".into()
        });
    }
    let (app, port) = app().await;
    if let App::Absent { reason } = app {
        return Err(absent_text(&reason, &own_id(), is_firefox()));
    }
    let mut port = match port {
        Some(port) => port,
        None => Port::open()?,
    };
    let id = hand_id("l", 0, now());
    let count = urls.len();
    // The site's sign-in where the popup hands it over, as for one link:
    // "Download selected links" fetched a signed-in file without it and got
    // a 403 (round 13).
    let cookies = cookies_for(page, tab_store(tab, "").as_deref()).await;
    port.send(&Request::Batch(dm_protocol::Batch {
        id: id.clone(),
        page: page.to_string(),
        urls,
        cookies,
    }))?;
    let deadline = now() + decide::TAKEN_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                if let Some(outcome) = decide::taken(&frame, &id) {
                    return outcome.map(|()| count);
                }
            }
            Heard::Gone(why) => return Err(why),
        }
    }
}

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
async fn show_app() -> Result<(), String> {
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
async fn make_menus() {
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
