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
    Document, Element, Event, HtmlElement, HtmlMediaElement, KeyboardEvent, MouseEvent, ShadowRoot,
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

use crate::panel::{self, Act, Focus, Key, BODY, STYLE};

/// The panel's size as `panel_at` places it: the bar alone.
const PANEL_SIZE: (f64, f64) = (240.0, 30.0);

/// What ? says.
const HELP: &str = "dm downloads this video with the dm app, where you choose its name and folder. A link clicked with the keep key held (Alt unless changed in dm's Options) stays in the browser.";

struct Panel {
    host: HtmlElement,
    wrap: Element,
    go: Element,
    title: Element,
    caret: Element,
    list: Element,
    sub: Element,
    /// Says each result to a screen reader.
    live: Element,
    root: ShadowRoot,
    /// The player it is shown for.
    current: Option<Element>,
    /// The page's element that had the focus when the panel was raised for
    /// it (the `<video>` or its player), where Tab and Escape go back to.
    player_focus: Option<Element>,
    /// The keyboard's focus is in the panel, or on its player: it does not
    /// hide then. A pointer's click focuses a button too, and that must not
    /// keep the panel up once the pointer has gone.
    focused: bool,
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
    // Off everywhere: the app's Options › Browser, through its catch rules.
    static OFF_ALL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // Whether this page has asked for the rules to be made current.
    static RULES_ASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // How far the person dragged the panel from its corner, for this page.
    static NUDGE: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((0.0, 0.0)) };
    // Where the panel's corner is for the player it is shown over.
    static SPOT: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((0.0, 0.0)) };
    // A drag under way: where the pointer went down, and the nudge then.
    static DRAG: std::cell::Cell<Option<(f64, f64, f64, f64)>> =
        const { std::cell::Cell::new(None) };
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

/// The app's catch rules as the background keeps them (`dm.catch`): the
/// panel is off everywhere where they say so.
const RULES_KEY: &str = "dm.catch";

/// Whether the rules, as stored, turn the panel off everywhere; applied at
/// once, and hidden where it shows.
fn apply_rules(rules: &JsValue) {
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
async fn stored_rules() -> JsValue {
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

/// The viewport's width and height.
fn window_size() -> (f64, f64) {
    let Some(window) = window() else {
        return (0.0, 0.0);
    };
    let size = |v: Result<JsValue, JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    (size(window.inner_width()), size(window.inner_height()))
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
        // A right-to-left page does not mirror it.
        ("direction", "ltr"),
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
    let live = root.query_selector(".live").ok()??;
    let body = root.query_selector(".panel").ok()??;
    let help = root.query_selector(".help").ok()??;
    let grip = root.query_selector(".grip").ok()??;
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
    // The keyboard's focus in the panel keeps it up; leaving it lets it go.
    listen(&host, "focusin", |_| {
        with_panel(|p| {
            p.focused = p.root.active_element().is_some_and(|e| by_keyboard(&e));
        });
    });
    listen(&host, "focusout", |_| {
        with_panel(|p| p.focused = false);
        hide_soon(HIDE_AFTER_MS);
    });
    listen(&body, "keydown", on_key);
    // Dragged by its grip, anywhere in the window.
    listen(&grip, "mousedown", |event| {
        let Some(mouse) = event.dyn_ref::<web_sys::MouseEvent>() else {
            return;
        };
        event.prevent_default();
        let (dx, dy) = NUDGE.with(|n| n.get());
        DRAG.with(|d| {
            d.set(Some((
                f64::from(mouse.client_x()),
                f64::from(mouse.client_y()),
                dx,
                dy,
            )))
        });
    });
    if let Some(window) = window() {
        listen(&window, "mousemove", |event| {
            let Some((sx, sy, dx, dy)) = DRAG.with(|d| d.get()) else {
                return;
            };
            let Some(mouse) = event.dyn_ref::<web_sys::MouseEvent>() else {
                return;
            };
            let by = (
                dx + f64::from(mouse.client_x()) - sx,
                dy + f64::from(mouse.client_y()) - sy,
            );
            NUDGE.with(|n| n.set(by));
            let view = window_size();
            let (x, y) = decide::dragged(SPOT.with(|s| s.get()), by, PANEL_SIZE, view);
            with_panel(|p| {
                let style = p.host.style();
                let _ = style.set_property_with_priority("left", &format!("{x}px"), "important");
                let _ = style.set_property_with_priority("top", &format!("{y}px"), "important");
            });
        });
        listen(&window, "mouseup", |_| DRAG.with(|d| d.set(None)));
    }
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
        live,
        root,
        current: None,
        player_focus: None,
        focused: false,
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
///
/// A click also needs the panel to be as dm drew it. A page cannot press
/// it, but it can restyle the element that holds it — invisible and over
/// the whole window — so that a person's next click anywhere lands on it
/// and hands the page's choice of file to dm.
fn listen(target: &web_sys::EventTarget, name: &str, handler: impl Fn(Event) + 'static) {
    let click = name == "click";
    let closure = Closure::<dyn Fn(Event)>::new(move |event: Event| {
        if event.is_trusted() && (!click || as_drawn()) {
            handler(event);
        }
    });
    let _ = target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
    // Listeners live as long as the page.
    closure.forget();
}

/// Whether the panel is shown as dm drew it: opaque, visible, and no
/// larger than its open list, wherever a page's styles would have it.
fn as_drawn() -> bool {
    let Some(host) = with_panel(|panel| panel.host.clone()) else {
        return false;
    };
    let Some(window) = window() else {
        return false;
    };
    let Ok(Some(style)) = window.get_computed_style(&host) else {
        return false;
    };
    let value = |name: &str| style.get_property_value(name).unwrap_or_default();
    let rect = host.get_bounding_client_rect();
    let opaque = value("opacity")
        .trim()
        .parse::<f64>()
        .is_ok_and(|o| o >= 0.99);
    opaque
        && value("visibility").trim() == "visible"
        && value("filter").trim() == "none"
        && value("clip-path").trim() == "none"
        && rect.width() <= MAX_DRAWN.0
        && rect.height() <= MAX_DRAWN.1
}

/// The panel's largest size, with its quality list open.
const MAX_DRAWN: (f64, f64) = (480.0, 640.0);

/// The panel, if built. `None` too when it is already in use further up
/// the stack: moving the focus fires focus events at once, and their
/// listeners come back here.
fn with_panel<T>(f: impl FnOnce(&mut Panel) -> T) -> Option<T> {
    PANEL.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
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
    if !RULES_ASKED.with(|c| c.replace(true)) {
        wasm_bindgen_futures::spawn_local(async {
            let _ = send(message(decide::RULES_MESSAGE, "", "").into()).await;
        });
    }
    if OFF_HERE.with(|c| c.get()) || OFF_ALL.with(|c| c.get()) {
        return false;
    }
    let Some(media) = media_of(element) else {
        return false;
    };
    let Some(offer) = decide::offer(&media) else {
        return false;
    };
    // Played through the browser's DRM: a `mediaKeys` is set on it.
    let protected = Reflect::get(element, &"mediaKeys".into())
        .ok()
        .is_some_and(|keys| !keys.is_null() && !keys.is_undefined());
    let Some(window) = window() else {
        return false;
    };
    if !seen(&window, element) {
        return false;
    }
    // In fullscreen only the fullscreen element is drawn: the panel goes
    // inside it, or is not shown when that is the bare `<video>`.
    if !attach() {
        return false;
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
    let spot = decide::panel_at(
        (rect.left(), rect.top(), rect.width(), rect.height()),
        PANEL_SIZE,
        view,
    );
    SPOT.with(|s| s.set(spot));
    // Where the person dragged it to, kept for the page.
    let (x, y) = decide::dragged(spot, NUDGE.with(|n| n.get()), PANEL_SIZE, view);
    with_panel(|p| {
        let same = p.current.as_ref() == Some(element);
        if !same {
            // A new player: a fresh offer, whatever the last one said.
            p.settled = false;
            p.wrap.set_class_name("wrap");
            p.title.set_text_content(Some(if protected {
                decide::PROTECTED_TITLE
            } else {
                offer.title
            }));
            if protected {
                let _ = p.go.set_attribute("data-protected", "");
            } else {
                let _ = p.go.remove_attribute("data-protected");
            }
            if offer.page && !protected {
                let _ = p.go.set_attribute("aria-haspopup", "menu");
                let _ = p.go.set_attribute("aria-expanded", "false");
            } else {
                let _ = p.go.remove_attribute("aria-haspopup");
                let _ = p.go.remove_attribute("aria-expanded");
            }
            // Where it comes from, on hover; the bar says nothing more.
            let _ = p.go.set_attribute("title", &offer.sub);
            if offer.page && !protected {
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
        p.focused = false;
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
            let stale =
                with_panel(|p| p.generation != generation || p.hovered || p.on_player || p.focused)
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
    pointer_at(f64::from(event.client_x()), f64::from(event.client_y()));
}

/// The page script's word, once this has loaded, of where the pointer
/// rests: it loaded this because the pointer came over a player, and a
/// pointer that then rests sends no move for the panel to see.
#[wasm_bindgen]
pub fn content_pointer(x: f64, y: f64) {
    with_panel(|p| p.last_scan = 0.0);
    pointer_at(x, y);
}

/// The page script's word, once this has loaded, of media that played
/// before it had: the play that loaded it was over by then.
#[wasm_bindgen]
pub fn content_played(element: Element) {
    if show(&element) {
        hide_soon(PLAY_SHOWS_MS);
    }
}

fn pointer_at(x: f64, y: f64) {
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
    match media_at(&document, x, y) {
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

/// The line under the bar: what happened, or what ? says. Empty hides it.
fn say(p: &Panel, text: &str) {
    // A line of its own starts with a capital: "the dm app has it" read
    // as half a sentence (UI round 16). The words are the background's,
    // shared with its notifications, where they follow a title.
    // But dm's own name stays as it is written everywhere, lowercase
    // ("Dm downloads this video…", UI round 17).
    let mut chars = text.chars();
    let text = match chars.next() {
        _ if text.starts_with("dm ") => text.to_string(),
        Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
        None => String::new(),
    };
    let text = text.as_str();
    p.sub.set_text_content(Some(text));
    if text.is_empty() {
        let _ = p.sub.set_attribute("hidden", "");
    } else {
        let _ = p.sub.remove_attribute("hidden");
        announce(p);
    }
}

/// What the panel now says, read out by a screen reader: the bar's words
/// and the line under it.
fn announce(p: &Panel) {
    let title = p.title.text_content().unwrap_or_default();
    let sub = p.sub.text_content().unwrap_or_default();
    let said = if sub.is_empty() {
        title
    } else {
        format!("{title}. {sub}")
    };
    p.live.set_text_content(Some(&said));
}

/// A result on the bar: its words, said aloud.
fn set_title(p: &Panel, text: &str) {
    p.title.set_text_content(Some(text));
    announce(p);
}

/// The × menu: hide the panel for this player, or never show it on this site.
fn close_menu() {
    let Some(document) = document() else {
        return;
    };
    let site = decide::site_of(&top_page()).unwrap_or_else(|| "this site".into());
    let first = with_panel(|p| {
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
        open_list(p)
    })
    .flatten();
    focus(first);
}

fn close_list(p: &Panel) {
    p.list.set_inner_html("");
    let _ = p.list.set_attribute("hidden", "");
    if p.go.has_attribute("aria-expanded") {
        let _ = p.go.set_attribute("aria-expanded", "false");
    }
}

/// Opens the list just filled. Returns its first line to put the focus on
/// when the focus was in the panel (a person at the keyboard); the caller
/// focuses it once it has let go of the panel.
fn open_list(p: &Panel) -> Option<HtmlElement> {
    let _ = p.list.remove_attribute("hidden");
    if p.go.has_attribute("aria-expanded") {
        let _ = p.go.set_attribute("aria-expanded", "true");
    }
    p.root.active_element()?;
    focus_target(p, Focus::Item(0))
}

fn focus(element: Option<HtmlElement>) {
    if let Some(element) = element {
        let _ = element.focus();
    }
}

fn items(p: &Panel) -> Vec<HtmlElement> {
    let Ok(found) = p.list.query_selector_all(".item") else {
        return Vec::new();
    };
    (0..found.length())
        .filter_map(|i| found.item(i))
        .filter_map(|n| n.dyn_into::<HtmlElement>().ok())
        .collect()
}

fn focus_target(p: &Panel, at: Focus) -> Option<HtmlElement> {
    match at {
        Focus::Go => p.go.clone().dyn_into().ok(),
        Focus::Help => p
            .root
            .query_selector(".help")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into().ok()),
        Focus::Close => p
            .root
            .query_selector(".close")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into().ok()),
        Focus::Item(i) => items(p).into_iter().nth(i),
    }
}

/// Where the focus is in the panel, if it is.
fn focus_in(p: &Panel) -> Option<Focus> {
    let active = p.root.active_element()?;
    if active == p.go {
        return Some(Focus::Go);
    }
    let class = active.class_name();
    if class == "help" {
        return Some(Focus::Help);
    }
    if class == "close" {
        return Some(Focus::Close);
    }
    let at = items(p)
        .iter()
        .position(|i| JsValue::from(i.clone()) == JsValue::from(active.clone()))?;
    Some(Focus::Item(at))
}

/// The player the panel is up for, to put the focus back on.
fn player_target(p: &Panel) -> Option<HtmlElement> {
    p.player_focus
        .clone()
        .or_else(|| p.current.clone())
        .and_then(|e| e.dyn_into::<HtmlElement>().ok())
}

/// A key in the panel: Escape backs out, the arrows walk the list, and Tab
/// moves on through the page as though the panel sat after its player.
/// Enter and Space are the buttons' own.
fn on_key(event: Event) {
    let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
        return;
    };
    let key = Key::of(&key.key(), key.shift_key());
    let decided = with_panel(|p| {
        let at = focus_in(p)?;
        let lines = if p.list.has_attribute("hidden") {
            0
        } else {
            items(p).len()
        };
        let act = panel::act(key, at, lines);
        let to = match act {
            Act::Focus(to) => focus_target(p, to),
            Act::CloseList => {
                close_list(p);
                focus_target(p, Focus::Go)
            }
            _ => None,
        };
        Some((act, to, player_target(p)))
    })
    .flatten();
    let Some((act, to, player)) = decided else {
        return;
    };
    // The focus is moved here, with the panel let go of.
    match act {
        Act::Nothing => return,
        Act::Focus(_) | Act::CloseList => focus(to),
        Act::ToPlayer { .. } => focus(player),
        // Back on the player first (which would raise the panel), then gone.
        Act::Hide => {
            focus(player);
            hide();
        }
    }
    // Tab off the last button is let go on, from the player.
    if act != (Act::ToPlayer { stop: false }) {
        event.prevent_default();
        event.stop_propagation();
    }
}

/// Whether a player is what a person sees there: not transparent or
/// hidden, and the thing at its middle is the player, its own overlay, or
/// a page layer it lies under that lets the pointer through.
fn seen(window: &Window, element: &Element) -> bool {
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
fn composed_parent(element: &Element) -> Option<Element> {
    if let Some(parent) = element.parent_element() {
        return Some(parent);
    }
    let root: JsValue = element.get_root_node().into();
    Reflect::get(&root, &"host".into())
        .ok()
        .and_then(|h| h.dyn_into::<Element>().ok())
}

/// Whether `inner` is `outer` or inside it, shadow roots crossed.
fn within(outer: &Element, inner: &Element) -> bool {
    panel::composed_within(inner.clone(), outer, composed_parent, 64)
}

/// The element at a point, followed into each open shadow root on top.
fn deepest_at(root: &JsValue, x: f64, y: f64) -> Option<Element> {
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

/// Puts the panel where it can be seen: inside the fullscreen element when
/// there is one (only it is drawn then), else in the page's body. `false`
/// when the fullscreen element is a media element itself, which draws
/// nothing of the page's over it.
fn attach() -> bool {
    let Some(document) = document() else {
        return false;
    };
    let mut full = Reflect::get(&document, &"fullscreenElement".into())
        .ok()
        .and_then(|f| f.dyn_into::<Element>().ok());
    // The document names a shadow host; the element in fullscreen may be
    // inside its shadow root.
    for _ in 0..8 {
        let inner = full
            .as_ref()
            .and_then(shadow_of)
            .and_then(|r| Reflect::get(&r, &"fullscreenElement".into()).ok())
            .and_then(|f| f.dyn_into::<Element>().ok());
        match inner {
            Some(inner) if Some(&inner) != full.as_ref() => full = Some(inner),
            _ => break,
        }
    }
    if full.as_ref().is_some_and(is_media) {
        return false;
    }
    // A light-DOM child of a shadow host is drawn only through a slot: in
    // the host's open shadow root it is drawn whatever the slots. A closed
    // one cannot be reached, and there is no panel rather than one unseen.
    let parent: Option<web_sys::Node> = match &full {
        Some(full) => match shadow_of(full) {
            Some(root) => root.dyn_into().ok(),
            None if full.tag_name().contains('-') => return false,
            None => Some(full.clone().into()),
        },
        None => document
            .body()
            .map(Into::into)
            .or_else(|| document.document_element().map(Into::into)),
    };
    let Some(parent) = parent else {
        return false;
    };
    with_panel(|p| {
        if p.host.parent_node().as_ref() != Some(&parent) {
            let _ = parent.append_child(&p.host);
        }
    });
    true
}

/// Fullscreen came or went: the panel moves with it, and is shown again
/// over its player if it was up.
fn on_fullscreen(_: Event) {
    let current = with_panel(|p| p.current.clone()).flatten();
    match current {
        Some(element) => {
            if show(&element) {
                hide_soon(PLAY_SHOWS_MS);
            } else {
                hide();
                attach();
            }
        }
        None => {
            attach();
        }
    }
}

/// The focus on a player (its `<video>`, or a player element holding one)
/// raises the panel for it, as the pointer does, and keeps it up.
fn on_focus(event: Event) {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    if with_panel(|p| JsValue::from(p.host.clone()) == JsValue::from(target.clone())) == Some(true)
    {
        return;
    }
    let media = if is_media(&target) {
        Some(target.clone())
    } else {
        target.query_selector("video, audio").ok().flatten()
    };
    let Some(media) = media else {
        return;
    };
    // A click on a player focuses it too; the pointer raises the panel then.
    if !by_keyboard(&target) {
        return;
    }
    if show(&media) {
        with_panel(|p| {
            p.player_focus = Some(target);
            p.focused = true;
        });
    }
}

/// Whether an element's focus came from the keyboard (`:focus-visible`).
fn by_keyboard(element: &Element) -> bool {
    element.matches(":focus-visible").unwrap_or(false)
}

fn on_blur(event: Event) {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    let ours = with_panel(|p| p.player_focus.as_ref() == Some(&target)).unwrap_or(false);
    if ours {
        with_panel(|p| p.focused = false);
        hide_soon(HIDE_AFTER_MS);
    }
}

/// Tab on a player whose panel is up goes into the panel first.
fn on_page_key(event: Event) {
    let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
        return;
    };
    if Key::of(&key.key(), key.shift_key()) != Key::Tab {
        return;
    }
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return;
    };
    let into = with_panel(|p| {
        let up = p.current.is_some() && p.player_focus.as_ref() == Some(&target);
        up.then(|| focus_target(p, Focus::Go)).flatten()
    })
    .flatten();
    if into.is_some() {
        event.prevent_default();
        focus(into);
    }
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
    // A copy-protected player: said, and nothing handed over.
    if with_panel(|p| {
        let protected = p.go.has_attribute("data-protected");
        if protected {
            say(p, decide::PROTECTED_SAID);
        }
        protected
    })
    .unwrap_or(false)
    {
        return;
    }
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
        // Short enough for the bar's title, which cut the longer one to
        // "Reading the vi…" (UI round 7).
        set_title(p, "Reading qualities…");
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
    let first = with_panel(|p| {
        set_title(p, "Download video");
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
        open_list(p)
    })
    .flatten();
    focus(first);
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
        set_title(p, "dm could not take it");
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
        set_title(p, "Sending to dm…");
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
            set_title(p, title);
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
        apply_rules(&stored_rules().await);
    });
    let changed = Closure::<dyn Fn(JsValue, JsValue)>::new(|changes: JsValue, area: JsValue| {
        if area.as_string().as_deref() != Some("local") {
            return;
        }
        // The app's Options › Browser, in force without a reload.
        if let Ok(rules) = Reflect::get(&changes, &RULES_KEY.into()) {
            if rules.is_object() {
                apply_rules(
                    &Reflect::get(&rules, &"newValue".into()).unwrap_or(JsValue::UNDEFINED),
                );
            }
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
    // Alt-click and "Download all/selected links" are answered by
    // `extension/content.js` itself, before this loads: a page that forbids
    // WebAssembly still has them.
    // The keyboard's way in: the focus on a player raises its panel, and
    // Tab from it goes into the panel.
    listen(&document, "focusin", on_focus);
    listen(&document, "focusout", on_blur);
    let tab = Closure::<dyn Fn(Event)>::new(on_page_key);
    let _ = document.add_event_listener_with_callback_and_bool(
        "keydown",
        tab.as_ref().unchecked_ref(),
        true,
    );
    tab.forget();
    // Fullscreen: only the fullscreen element is drawn, so the panel goes
    // inside it, and back to the body after.
    listen(&document, "fullscreenchange", on_fullscreen);
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
