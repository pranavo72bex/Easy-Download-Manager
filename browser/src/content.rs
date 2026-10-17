//! The page script: the floating "Download video" panel.
//!
//! It raises a panel at the top right of a video or audio player when the
//! pointer is over it or it starts playing, and a click hands the media to
//! the dm app through the background (`dm.media`). It reads nothing else on
//! the page and sends nothing until the panel is clicked.
//!
//! The panel lives in a shadow root on a fixed host, so the page's styles
//! cannot restyle it and its own cannot leak out. The player under the
//! pointer is found by hit-testing (`elementsFromPoint`) rather than by
//! listening on each element, because players stack transparent control
//! layers over the `<video>` that would swallow every hover event.
//!
//! What it offers, where it sits and what a click hands over are decided in
//! [`crate::decide`], which is tested on the host.

use std::cell::RefCell;

use js_sys::{Array, Function, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Document, Element, Event, HtmlElement, HtmlMediaElement, MouseEvent, ShadowRoot,
    ShadowRootInit, ShadowRootMode, Window,
};

use crate::decide::{self, Media, PANEL_MESSAGE};

/// How long the panel waits after the pointer leaves before it goes, so the
/// pointer can travel from the player to the panel.
const HIDE_AFTER_MS: i32 = 300;
/// How long a panel raised by playback stays without the pointer near it.
const PLAY_SHOWS_MS: i32 = 3500;
/// How often the pointer is hit-tested, at most.
const SCAN_EVERY_MS: f64 = 90.0;

const STYLE: &str = r#"
:host { all: initial; }
* { box-sizing: border-box; margin: 0; font-family: system-ui, -apple-system, Roboto, sans-serif; }
.panel { width: 240px; color: #f2f3f5; }
.wrap {
  display: flex; align-items: stretch; height: 30px; border-radius: 6px; overflow: hidden;
  background: rgba(28, 30, 35, .94); color: #f2f3f5; user-select: none;
  border: 1px solid rgba(255,255,255,.18); box-shadow: 0 4px 16px rgba(0,0,0,.45);
  font-size: 12.5px; line-height: 1;
}
.wrap.done { border-color: #35c07d; }
.wrap.dead { border-color: #f0616d; }
button { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; }
.go { flex: 1; min-width: 0; display: flex; align-items: center; gap: 7px; padding: 0 8px; text-align: left; }
.go:hover, .go:focus-visible { background: rgba(255,255,255,.09); outline: none; }
.mark { width: 18px; height: 18px; flex: none; border-radius: 50%; background: #f08a24; color: #fff;
  display: flex; align-items: center; justify-content: center; font-weight: 700; font-size: 12px; }
.wrap.done .mark { background: #35c07d; }
.wrap.dead .mark { background: #f0616d; }
.title { flex: 1; min-width: 0; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.caret { flex: none; font-size: 10px; color: #c4c8d0; }
.help, .close { width: 24px; flex: none; color: #9aa1ad; font-size: 13px; border-left: 1px solid rgba(255,255,255,.1); }
.help:hover, .close:hover { background: rgba(255,255,255,.1); color: #fff; }
.list { margin-top: 3px; border-radius: 6px; overflow: hidden; background: rgba(28, 30, 35, .97);
  border: 1px solid rgba(255,255,255,.18); box-shadow: 0 6px 20px rgba(0,0,0,.5); max-height: 260px; overflow-y: auto; }
.item { display: block; width: 100%; padding: 8px 10px; text-align: left; font-size: 12.5px; white-space: nowrap; color: #f2f3f5;
  overflow: hidden; text-overflow: ellipsis; border-top: 1px solid rgba(255,255,255,.06); }
.item:first-child { border-top: 0; }
.item:hover, .item:focus-visible { background: #f08a24; color: #fff; outline: none; }
.sub { margin-top: 3px; padding: 6px 9px; border-radius: 6px; background: rgba(28, 30, 35, .94);
  color: #c4c8d0; font-size: 11.5px; line-height: 1.35; }
[hidden] { display: none !important; }
"#;

const BODY: &str = r#"<div class="panel">
<div class="wrap">
<button class="go" aria-label="Download video"><span class="mark">&#8595;</span><span class="title"></span><span class="caret" hidden>&#9662;</span></button>
<button class="help" aria-label="What is this" title="What is this">?</button>
<button class="close" aria-label="Hide" title="Hide">&#215;</button>
</div>
<div class="list" role="menu" hidden></div>
<div class="sub" hidden></div>
</div>"#;

/// The panel's size as `panel_at` places it: the bar alone.
const PANEL_SIZE: (f64, f64) = (240.0, 30.0);

/// What ? says.
const HELP: &str = "dm downloads this video with the dm app, where you choose its name and folder. Alt-click a link to keep a download in the browser.";

struct Panel {
    host: HtmlElement,
    wrap: Element,
    go: Element,
    title: Element,
    caret: Element,
    list: Element,
    sub: Element,
    /// The player it is shown for.
    current: Option<Element>,
    /// The pointer is over the panel itself.
    hovered: bool,
    /// The pointer is on the player itself: the panel stays,
    /// however often the page restarts playback beneath it.
    on_player: bool,
    /// A hand-off is under way, or has finished and is being shown.
    settled: bool,
    last_scan: f64,
    /// Bumped on every show; a hide timer for an older show does nothing.
    generation: u32,
}

thread_local! {
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    /// The person asked for no panel on this site (its × menu, or the
    /// toolbar popup).
    static OFF_HERE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The page a person sees: this one, or for a player in a frame, the page
/// the frame is in — so "Never on this site" on an embedded video is about
/// the site it is embedded in.
fn top_page() -> String {
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
fn local_storage() -> Option<JsValue> {
    let storage = Reflect::get(&ext(), &"storage".into()).ok()?;
    Reflect::get(&storage, &"local".into())
        .ok()
        .filter(|l| l.is_object())
}

/// The sites the panel is kept off, as stored.
async fn sites_off() -> Vec<String> {
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

fn list_of(value: &JsValue) -> Vec<String> {
    value
        .dyn_ref::<Array>()
        .map(|a| a.iter().filter_map(|v| v.as_string()).collect())
        .unwrap_or_default()
}

/// Whether the stored list keeps the panel off this page, applied at once.
fn apply_off(off: &[String]) {
    let off_here = decide::panel_off(off, &top_page());
    OFF_HERE.with(|c| c.set(off_here));
    if off_here {
        hide();
    }
}

/// Keeps the panel off this site from now on, here and on every page of it.
fn never_here() {
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

fn window() -> Option<Window> {
    web_sys::window()
}

fn document() -> Option<Document> {
    window()?.document()
}

fn now() -> f64 {
    js_sys::Date::now()
}

thread_local! {
    /// The extension API, as the loader handed it over. In Firefox a module
    /// a page script imports does not see the page script's `browser`.
    static EXT: RefCell<JsValue> = const { RefCell::new(JsValue::UNDEFINED) };
}

/// The extension API object: the one the loader handed over, else
/// `browser` where it exists, else `chrome`.
fn ext() -> JsValue {
    let given = EXT.with(|cell| cell.borrow().clone());
    if given.is_object() {
        return given;
    }
    let global: JsValue = js_sys::global().into();
    let browser = Reflect::get(&global, &"browser".into()).unwrap_or(JsValue::UNDEFINED);
    if browser.is_object() {
        browser
    } else {
        Reflect::get(&global, &"chrome".into()).unwrap_or(JsValue::UNDEFINED)
    }
}

fn set_timeout(callback: impl FnOnce() + 'static, ms: i32) {
    let closure = Closure::once_into_js(callback);
    if let (Some(window), Ok(function)) = (window(), closure.dyn_into::<Function>()) {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&function, ms);
    }
}

/// Builds the panel once, hidden.
fn build(document: &Document) -> Option<Panel> {
    let host: HtmlElement = document.create_element("dm-panel").ok()?.dyn_into().ok()?;
    let style = host.style();
    // A page's rules on unknown elements must not move or hide it.
    for (name, value) in [
        ("position", "fixed"),
        ("top", "0"),
        ("left", "0"),
        ("z-index", "2147483647"),
        ("display", "none"),
        ("margin", "0"),
        ("padding", "0"),
        ("border", "none"),
        ("background", "none"),
        ("transform", "none"),
        ("pointer-events", "auto"),
    ] {
        let _ = style.set_property_with_priority(name, value, "important");
    }
    let root: ShadowRoot = host
        .attach_shadow(&ShadowRootInit::new(ShadowRootMode::Open))
        .ok()?;
    root.set_inner_html(&format!("<style>{STYLE}</style>{BODY}"));
    let wrap = root.query_selector(".wrap").ok()??;
    let go = root.query_selector(".go").ok()??;
    let title = root.query_selector(".title").ok()??;
    let caret = root.query_selector(".caret").ok()??;
    let list = root.query_selector(".list").ok()??;
    let sub = root.query_selector(".sub").ok()??;
    let help = root.query_selector(".help").ok()??;
    let close = root.query_selector(".close").ok()??;
    let parent: Element = document
        .body()
        .map(Into::into)
        .or_else(|| document.document_element())?;
    parent.append_child(&host).ok()?;

    listen(&host, "mouseenter", |_| {
        with_panel(|p| p.hovered = true);
    });
    listen(&host, "mouseleave", |_| {
        with_panel(|p| p.hovered = false);
        hide_soon(HIDE_AFTER_MS);
    });
    // A button: Enter and Space click it already.
    listen(&go, "click", |event| {
        event.prevent_default();
        event.stop_propagation();
        clicked();
    });
    listen(&help, "click", |event| {
        event.prevent_default();
        event.stop_propagation();
        with_panel(|p| {
            let showing =
                !p.sub.has_attribute("hidden") && p.sub.text_content().as_deref() == Some(HELP);
            say(p, if showing { "" } else { HELP });
        });
    });
    // × offers two ways to go: for now, or for this whole site.
    listen(&close, "click", |event| {
        event.prevent_default();
        event.stop_propagation();
        close_menu();
    });

    Some(Panel {
        host,
        wrap,
        go,
        title,
        caret,
        list,
        sub,
        current: None,
        hovered: false,
        on_player: false,
        settled: false,
        last_scan: 0.0,
        generation: 0,
    })
}

/// Listens for what a person does, and only that: an event a page's own
/// script made up (`isTrusted` false) is ignored. The panel's root is open,
/// so a page could otherwise press its buttons itself, and a press can hand
/// a file to dm or, without dm, start the browser's download of it.
fn listen(target: &web_sys::EventTarget, name: &str, handler: impl Fn(Event) + 'static) {
    let closure = Closure::<dyn Fn(Event)>::new(move |event: Event| {
        if event.is_trusted() {
            handler(event);
        }
    });
    let _ = target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
    // Listeners live as long as the page.
    closure.forget();
}

fn with_panel<T>(f: impl FnOnce(&mut Panel) -> T) -> Option<T> {
    PANEL.with(|cell| cell.borrow_mut().as_mut().map(f))
}

/// What the page says about a media element.
fn media_of(element: &Element) -> Option<Media> {
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
fn show(element: &Element) -> bool {
    if OFF_HERE.with(|c| c.get()) {
        return false;
    }
    let Some(media) = media_of(element) else {
        return false;
    };
    let Some(offer) = decide::offer(&media) else {
        return false;
    };
    let Some(window) = window() else {
        return false;
    };
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
    let (x, y) = decide::panel_at(
        (rect.left(), rect.top(), rect.width(), rect.height()),
        PANEL_SIZE,
        view,
    );
    with_panel(|p| {
        let same = p.current.as_ref() == Some(element);
        if !same {
            // A new player: a fresh offer, whatever the last one said.
            p.settled = false;
            p.wrap.set_class_name("wrap");
            p.title.set_text_content(Some(offer.title));
            // Where it comes from, on hover; the bar says nothing more.
            let _ = p.go.set_attribute("title", &offer.sub);
            if offer.page {
                let _ = p.caret.remove_attribute("hidden");
            } else {
                let _ = p.caret.set_attribute("hidden", "");
            }
            close_list(p);
            say(p, "");
            p.current = Some(element.clone());
        }
        p.generation = p.generation.wrapping_add(1);
        let style = p.host.style();
        let _ = style.set_property_with_priority("left", &format!("{x}px"), "important");
        let _ = style.set_property_with_priority("top", &format!("{y}px"), "important");
        let _ = style.set_property_with_priority("display", "block", "important");
    })
    .is_some()
}

fn hide() {
    with_panel(|p| {
        let _ = p
            .host
            .style()
            .set_property_with_priority("display", "none", "important");
        p.current = None;
        p.hovered = false;
        p.on_player = false;
        close_list(p);
    });
}

/// Hides after `ms`, unless the panel has been shown again or is under the
/// pointer by then.
fn hide_soon(ms: i32) {
    let Some(generation) = with_panel(|p| p.generation) else {
        return;
    };
    set_timeout(
        move || {
            let stale = with_panel(|p| p.generation != generation || p.hovered || p.on_player)
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
fn media_at(document: &Document, x: f64, y: f64) -> Option<Element> {
    let root: JsValue = document.clone().into();
    media_in(&root, x, y, 0)
}

fn is_media(element: &Element) -> bool {
    let tag = element.tag_name().to_ascii_lowercase();
    tag == "video" || tag == "audio"
}

/// The open shadow root an element hosts, if any.
fn shadow_of(element: &Element) -> Option<JsValue> {
    Reflect::get(element, &"shadowRoot".into())
        .ok()
        .filter(|r| r.is_object())
}

/// The media under `(x, y)` in `root` (the document, or a component's open
/// shadow root), looking inside components on the way. Players built as
/// web components keep their `<video>` in a shadow root, which the
/// document's own hit-test stops at: the Internet Archive's `play-av` is
/// one, and its videos got no panel.
fn media_in(root: &JsValue, x: f64, y: f64, depth: u8) -> Option<Element> {
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

fn on_pointer(event: Event) {
    let Some(event) = event.dyn_ref::<MouseEvent>() else {
        return;
    };
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
    match media_at(
        &document,
        f64::from(event.client_x()),
        f64::from(event.client_y()),
    ) {
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

fn on_play(event: Event) {
    let Some(element) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    if show(&element) {
        hide_soon(PLAY_SHOWS_MS);
    }
}

/// Keeps the panel on its player as the page scrolls or resizes.
fn on_move(_: Event) {
    if let Some(Some(element)) = with_panel(|p| p.current.clone()) {
        if !show(&element) {
            hide();
        }
    }
}

/// Answers the background's `dm.links`: the address of every link on the
/// page, or of each one the selection touches. Nothing else is read.
fn on_message(message: JsValue, _sender: JsValue, reply: JsValue) -> JsValue {
    let kind = Reflect::get(&message, &"type".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    if kind != decide::LINKS_MESSAGE {
        return JsValue::FALSE;
    }
    let selected = Reflect::get(&message, &"selected".into())
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let links = Array::new();
    if let (Some(document), Some(window)) = (document(), window()) {
        let selection = if selected {
            window.get_selection().ok().flatten()
        } else {
            None
        };
        let selection_text: String = selection
            .as_ref()
            .and_then(|s| s.to_string().as_string())
            .unwrap_or_default();
        if let Ok(all) = document.query_selector_all("a[href]") {
            for i in 0..all.length() {
                let Some(node) = all.item(i) else {
                    continue;
                };
                if selected {
                    // Inside the selection, or partly inside with its words
                    // in the selected text: a link that only touches the
                    // selection's edge was not selected.
                    let Some(selection) = selection.as_ref() else {
                        continue;
                    };
                    let whole = selection
                        .contains_node_with_allow_partial_containment(&node, false)
                        .unwrap_or(false);
                    let words = node.text_content().unwrap_or_default();
                    let partly = !words.trim().is_empty()
                        && selection
                            .contains_node_with_allow_partial_containment(&node, true)
                            .unwrap_or(false)
                        && selection_text.contains(words.trim());
                    if !(whole || partly) {
                        continue;
                    }
                }
                if let Some(href) = Reflect::get(&node, &"href".into())
                    .ok()
                    .and_then(|v| v.as_string())
                {
                    links.push(&href.into());
                }
            }
        }
    }
    let answer = js_sys::Object::new();
    let _ = Reflect::set(&answer, &"links".into(), &links);
    if let Ok(reply) = reply.dyn_into::<Function>() {
        let _ = reply.call1(&JsValue::NULL, &answer);
    }
    JsValue::FALSE
}

/// Alt-click: a link clicked with Alt held is the browser's to
/// download, and the background is told before the download starts.
fn on_click(event: Event) {
    // Not held to `isTrusted`, unlike the panel: all a made-up Alt+click
    // can do is leave a download with the browser, as it would be with no dm.
    let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
        return;
    };
    if !mouse.alt_key() {
        return;
    }
    let link = event
        .target()
        .and_then(|t| t.dyn_into::<Element>().ok())
        .and_then(|e| e.closest("a[href]").ok().flatten());
    let Some(link) = link else {
        return;
    };
    // The property, not the attribute: resolved against the page.
    let href = Reflect::get(&link, &"href".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    let message = js_sys::Object::new();
    let _ = Reflect::set(&message, &"type".into(), &decide::SKIP_MESSAGE.into());
    let _ = Reflect::set(&message, &"src".into(), &href.into());
    let _ = Reflect::set(&message, &"frame".into(), &JsValue::from_str(""));
    wasm_bindgen_futures::spawn_local(async move {
        let _ = send(message.into()).await;
    });
}

/// The line under the bar: what happened, or what ? says. Empty hides it.
fn say(p: &Panel, text: &str) {
    p.sub.set_text_content(Some(text));
    if text.is_empty() {
        let _ = p.sub.set_attribute("hidden", "");
    } else {
        let _ = p.sub.remove_attribute("hidden");
    }
}

/// The × menu: hide the panel for this player, or never show it on this site.
fn close_menu() {
    let Some(document) = document() else {
        return;
    };
    let site = decide::site_of(&top_page()).unwrap_or_else(|| "this site".into());
    with_panel(|p| {
        p.settled = true;
        close_list(p);
        say(p, "");
        for (line, never) in [
            ("Hide for now".to_string(), false),
            (format!("Never on {site}"), true),
        ] {
            let Ok(item) = document.create_element("button") else {
                continue;
            };
            item.set_class_name("item");
            let _ = item.set_attribute("role", "menuitem");
            item.set_text_content(Some(&line));
            listen(&item, "click", move |event| {
                event.prevent_default();
                event.stop_propagation();
                if never {
                    never_here();
                } else {
                    // Not offered again for this player until the page reloads.
                    with_panel(|p| {
                        if let Some(el) = &p.current {
                            let _ = el.set_attribute("data-dm-hidden", "");
                        }
                    });
                    hide();
                }
            });
            let _ = p.list.append_child(&item);
        }
        let _ = p.list.remove_attribute("hidden");
    });
}

fn close_list(p: &Panel) {
    p.list.set_inner_html("");
    let _ = p.list.set_attribute("hidden", "");
}

/// The player's media and the frame it is in, for a message to the
/// background, or `None` when the panel is already busy.
fn asking() -> Option<(String, String)> {
    let (src, settled) = with_panel(|p| {
        let src = p
            .current
            .as_ref()
            .and_then(media_of)
            .map(|m| m.src)
            .unwrap_or_default();
        (src, p.settled)
    })?;
    if settled {
        return None;
    }
    let frame = window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();
    Some((src, frame))
}

fn message(kind: &str, src: &str, frame: &str) -> js_sys::Object {
    let message = js_sys::Object::new();
    let _ = Reflect::set(&message, &"type".into(), &kind.into());
    let _ = Reflect::set(&message, &"src".into(), &src.into());
    let _ = Reflect::set(&message, &"frame".into(), &frame.into());
    message
}

fn text_of(reply: &JsValue, key: &str) -> String {
    Reflect::get(reply, &key.into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

/// A click on the bar. A file is one quality and goes at once; a player
/// page's qualities are asked of the app first and, when there is more than
/// one, listed with their sizes.
fn clicked() {
    let Some((src, frame)) = asking() else {
        return;
    };
    let page = with_panel(|p| !p.caret.has_attribute("hidden")).unwrap_or(false);
    if !page {
        hand(&src, &frame, None, None);
        return;
    }
    with_panel(|p| {
        p.settled = true;
        p.title
            .set_text_content(Some("Reading the video's qualities…"));
        close_list(p);
        say(p, "");
    });
    let asked = message(decide::FORMATS_MESSAGE, &src, &frame);
    wasm_bindgen_futures::spawn_local(async move {
        let reply = match send(asked.into()).await {
            Ok(reply) => reply,
            Err(e) => {
                web_sys::console::debug_2(&"dm: the panel's message was not delivered:".into(), &e);
                failed("dm was updated: reload this page to use it");
                return;
            }
        };
        let ok = Reflect::get(&reply, &"ok".into())
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !ok {
            failed(&text_of(&reply, "said"));
            return;
        }
        let choices: Array = Reflect::get(&reply, &"choices".into())
            .ok()
            .and_then(|v| v.dyn_into().ok())
            .unwrap_or_default();
        with_panel(|p| p.settled = false);
        match choices.length() {
            0 => hand(&src, &frame, None, None),
            1 => hand_choice(&src, &frame, &choices.get(0)),
            _ => list(&src, &frame, &choices),
        }
    });
}

/// The list: one line per quality, with its size.
fn list(src: &str, frame: &str, choices: &Array) {
    let Some(document) = document() else {
        return;
    };
    with_panel(|p| {
        p.title.set_text_content(Some("Download video"));
        close_list(p);
        for choice in choices.iter() {
            let Ok(item) = document.create_element("button") else {
                continue;
            };
            item.set_class_name("item");
            let _ = item.set_attribute("role", "menuitem");
            item.set_text_content(Some(&text_of(&choice, "line")));
            let (src, frame) = (src.to_string(), frame.to_string());
            listen(&item, "click", move |event| {
                event.prevent_default();
                event.stop_propagation();
                hand_choice(&src, &frame, &choice);
            });
            let _ = p.list.append_child(&item);
        }
        let _ = p.list.remove_attribute("hidden");
    });
}

/// One line of the list, handed over: a quality of the page the app's
/// yt-dlp reads, or a stream the page loaded, which goes by its own address.
fn hand_choice(src: &str, frame: &str, choice: &JsValue) {
    let label = Some(text_of(choice, "label")).filter(|l| !l.is_empty());
    let url = text_of(choice, "url");
    if url.is_empty() {
        hand(src, frame, Some(text_of(choice, "quality")), label);
    } else {
        hand(&url, frame, None, label);
    }
}

fn failed(said: &str) {
    with_panel(|p| {
        p.settled = true;
        p.wrap.set_class_name("wrap dead");
        p.title.set_text_content(Some("dm could not take it"));
        say(p, said);
    });
    hide_soon(6000);
}

/// Hands the player's media to the app through the background, at the
/// quality picked; the app asks where to save it.
fn hand(src: &str, frame: &str, quality: Option<String>, label: Option<String>) {
    let message = message(PANEL_MESSAGE, src, frame);
    if let Some(quality) = &quality {
        let _ = Reflect::set(&message, &"quality".into(), &quality.into());
    }
    if let Some(label) = &label {
        let _ = Reflect::set(&message, &"label".into(), &label.into());
    }
    with_panel(|p| {
        p.settled = true;
        close_list(p);
        p.title.set_text_content(Some("Sending to dm…"));
    });
    wasm_bindgen_futures::spawn_local(async move {
        let answer = send(message.into()).await;
        let (ok, said) = match answer {
            Ok(reply) => (
                Reflect::get(&reply, &"ok".into())
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                text_of(&reply, "said"),
            ),
            // The extension was updated or removed under this page.
            Err(e) => {
                web_sys::console::debug_2(&"dm: the panel's message was not delivered:".into(), &e);
                (false, "dm was updated: reload this page to use it".into())
            }
        };
        if !ok {
            failed(&said);
            return;
        }
        // dm could not take it and the browser is downloading it instead:
        // said as that, not as sent.
        let title = if said.starts_with("the browser is downloading it") {
            "Downloading in the browser"
        } else {
            "Sent to dm"
        };
        with_panel(|p| {
            p.wrap.set_class_name("wrap done");
            p.title.set_text_content(Some(title));
            say(p, &said);
        });
        hide_soon(2500);
    });
}

async fn send(message: JsValue) -> Result<JsValue, JsValue> {
    let runtime = Reflect::get(&ext(), &"runtime".into())?;
    let send: Function = Reflect::get(&runtime, &"sendMessage".into())?.dyn_into()?;
    let returned = send.call1(&runtime, &message)?;
    let promise: Promise = returned.dyn_into()?;
    JsFuture::from(promise).await
}

/// Starts the page script; called by `extension/content.js` once the
/// WebAssembly has loaded. Runs once per page and frame.
#[wasm_bindgen]
pub fn content_start(ext: JsValue) {
    EXT.with(|cell| *cell.borrow_mut() = ext);
    let Some(document) = document() else {
        return;
    };
    let global: JsValue = js_sys::global().into();
    if Reflect::has(&global, &"__dmPanel".into()).unwrap_or(false) {
        return;
    }
    let _ = Reflect::set(&global, &"__dmPanel".into(), &JsValue::TRUE);

    let Some(panel) = build(&document) else {
        return;
    };
    PANEL.with(|cell| *cell.borrow_mut() = Some(panel));
    // Kept off this site? Read now, and followed as the list changes, so
    // the toolbar popup turning it back on is in force without a reload.
    wasm_bindgen_futures::spawn_local(async {
        apply_off(&sites_off().await);
    });
    let changed = Closure::<dyn Fn(JsValue, JsValue)>::new(|changes: JsValue, area: JsValue| {
        if area.as_string().as_deref() != Some("local") {
            return;
        }
        let Ok(change) = Reflect::get(&changes, &decide::PANEL_OFF_KEY.into()) else {
            return;
        };
        if change.is_object() {
            apply_off(&list_of(
                &Reflect::get(&change, &"newValue".into()).unwrap_or(JsValue::UNDEFINED),
            ));
        }
    });
    let on_changed = Reflect::get(&self::ext(), &"storage".into())
        .and_then(|storage| Reflect::get(&storage, &"onChanged".into()))
        .and_then(|event| Reflect::get(&event, &"addListener".into()).map(|add| (event, add)));
    if let Ok((event, add)) = on_changed {
        if let Ok(add) = add.dyn_into::<Function>() {
            let _ = add.call1(&event, changed.as_ref());
        }
    }
    changed.forget();

    listen(&document, "mousemove", on_pointer);
    // "Download all links" and "Download selected links": the
    // background asks this page for them.
    let asked = Closure::<dyn Fn(JsValue, JsValue, JsValue) -> JsValue>::new(on_message);
    let on_message = Reflect::get(&self::ext(), &"runtime".into())
        .and_then(|runtime| Reflect::get(&runtime, &"onMessage".into()))
        .and_then(|event| Reflect::get(&event, &"addListener".into()).map(|add| (event, add)));
    if let Ok((event, add)) = on_message {
        if let Ok(add) = add.dyn_into::<Function>() {
            let _ = add.call1(&event, asked.as_ref());
        }
    }
    asked.forget();
    // Capturing, so a page that stops the click on its way up cannot hide
    // the Alt from dm.
    let alt = Closure::<dyn Fn(Event)>::new(on_click);
    let _ = document.add_event_listener_with_callback_and_bool(
        "click",
        alt.as_ref().unchecked_ref(),
        true,
    );
    alt.forget();
    // Capturing, because `play` does not bubble.
    let play = Closure::<dyn Fn(Event)>::new(on_play);
    let _ = document.add_event_listener_with_callback_and_bool(
        "play",
        play.as_ref().unchecked_ref(),
        true,
    );
    play.forget();
    // Capturing, so a scroll inside any container moves the panel too.
    let moved = Closure::<dyn Fn(Event)>::new(on_move);
    let _ = document.add_event_listener_with_callback_and_bool(
        "scroll",
        moved.as_ref().unchecked_ref(),
        true,
    );
    moved.forget();
    if let Some(window) = window() {
        listen(&window, "resize", on_move);
    }
}
