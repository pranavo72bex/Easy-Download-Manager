//! Building the panel in its shadow root.

use super::*;

/// Builds the panel once, hidden.
pub(super) fn build(document: &Document) -> Option<Panel> {
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
pub(super) fn listen(target: &web_sys::EventTarget, name: &str, handler: impl Fn(Event) + 'static) {
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
pub(super) fn as_drawn() -> bool {
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
pub(super) const MAX_DRAWN: (f64, f64) = (480.0, 640.0);

/// The panel, if built. `None` too when it is already in use further up
/// the stack: moving the focus fires focus events at once, and their
/// listeners come back here.
pub(super) fn with_panel<T>(f: impl FnOnce(&mut Panel) -> T) -> Option<T> {
    PANEL.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}
