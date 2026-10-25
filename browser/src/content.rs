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
    // The qualities of the page being watched, asked for when the panel was
    // raised so that a click has them already: the page's address, when it
    // was asked (ms) and the answer on its way.
    static READING: RefCell<Option<(String, f64, Promise)>> = const { RefCell::new(None) };
    // Where the panel's corner is for the player it is shown over.
    static SPOT: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((0.0, 0.0)) };
    // A drag under way: where the pointer went down, and the nudge then.
    static DRAG: std::cell::Cell<Option<(f64, f64, f64, f64)>> =
        const { std::cell::Cell::new(None) };
}

mod build;
mod dom;
mod events;
mod handoff;
mod keyboard;
mod rules;
mod show;
mod visible;

use build::*;
use dom::*;
use events::*;
use handoff::*;
use keyboard::*;
use rules::*;
use show::*;
use visible::*;
