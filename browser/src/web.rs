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
    /// Chrome's file name questions, held while dm decides.
    static HOLDS: RefCell<decide::Holds<Function>> = RefCell::new(decide::Holds::default());
}

mod actions;
mod downloads;
mod handoff;
mod host;
mod page;
mod plumbing;
mod settings;
mod streams;

use actions::*;
use handoff::*;
use host::*;
use page::*;
use plumbing::*;
use settings::*;
use streams::*;
