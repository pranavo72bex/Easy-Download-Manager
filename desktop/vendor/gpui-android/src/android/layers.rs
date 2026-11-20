//! dm(android-stack): GPUI's windows as layers on the phone's one surface.
//!
//! NativeActivity hands the app one `ANativeWindow` ([`AndroidWindow`]), and upstream gave
//! every window GPUI opened that same surface, each registering its callbacks on it in place
//! of the last: the newest window took the frames and the input, and once it closed nothing
//! drew the one under it again. Here each GPUI window ([`super::AndroidPlatformWindow`]) keeps
//! its callbacks in a [`Layer`] of a stack (`crate::stack`, the plain logic), and the surface's
//! callbacks are forwarders: frames and input go to the window on top, size, appearance and
//! visibility to every window, active status true to the top alone. A window closing that was
//! on top has the one below repainted in full (its last frame was drawn over). Back is
//! Escape given to the window on top, then what `crate::stack::after_back` decides.
//!
//! All of it runs on the loop's thread (GPUI's main thread), so the stack is a thread-local
//! and a layer's callbacks need not be `Send`. A callback is taken out of its slot while it
//! runs and put back after, so one that opens or closes a window (which reaches the stack) or
//! re-enters does not find its own slot borrowed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gpui::DispatchEventResult;

use super::keyboard::{AKEYCODE_BACK, AKEY_EVENT_ACTION_DOWN, AKEY_EVENT_ACTION_UP};
use super::window::{AndroidWindow, WindowAppearance};
use super::{AndroidKeyEvent, DevicePixels, Size, TouchPoint};
use crate::stack::{after_back, Back, Stack};

type Slot<F> = RefCell<Option<Box<F>>>;

/// What one GPUI window registered with the platform: the callbacks the surface's forwarders
/// call while it is the window they go to.
#[derive(Default)]
pub(crate) struct Layer {
    pub(crate) frame: Slot<dyn FnMut()>,
    pub(crate) touch: Slot<dyn FnMut(TouchPoint)>,
    pub(crate) key: Slot<dyn FnMut(AndroidKeyEvent) -> DispatchEventResult>,
    pub(crate) resize: Slot<dyn FnMut(Size<DevicePixels>, f32)>,
    pub(crate) appearance: Slot<dyn FnMut(WindowAppearance)>,
    pub(crate) active: Slot<dyn FnMut(bool)>,
    pub(crate) visible: Slot<dyn FnMut(bool)>,
    pub(crate) should_close: Slot<dyn FnMut() -> bool>,
    pub(crate) close: RefCell<Option<Box<dyn FnOnce()>>>,
    /// The active status last told to the window, so each change is told once.
    told_active: Cell<Option<bool>>,
}

/// A key's result where no window used it: Escape then goes on to Back's next step.
pub(crate) const UNUSED: DispatchEventResult = DispatchEventResult {
    propagate: true,
    default_prevented: false,
};

thread_local! {
    static STACK: RefCell<Stack<Rc<Layer>>> = RefCell::new(Stack::default());
}

/// Runs the callback in `slot`, if one is there, taken out while it runs.
fn call<F: ?Sized, R>(slot: &Slot<F>, run: impl FnOnce(&mut F) -> R) -> Option<R> {
    let mut callback = slot.borrow_mut().take()?;
    let result = run(&mut *callback);
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(callback);
    }
    Some(result)
}

/// Sets the callback in `slot` (a window's registration, in place of the surface's).
pub(crate) fn set<F: ?Sized>(slot: &Slot<F>, callback: Box<F>) {
    *slot.borrow_mut() = Some(callback);
}

/// The window on top and its layer.
fn top() -> Option<(u64, Rc<Layer>)> {
    STACK.with(|stack| {
        stack
            .borrow()
            .top()
            .map(|(id, layer)| (id, Rc::clone(layer)))
    })
}

/// Every window's layer, oldest first.
fn all() -> Vec<(u64, Rc<Layer>)> {
    STACK.with(|stack| {
        stack
            .borrow()
            .iter()
            .map(|(id, layer)| (id, Rc::clone(layer)))
            .collect()
    })
}

/// Whether window `id` is the one on top.
pub(crate) fn is_top(id: u64) -> bool {
    STACK.with(|stack| stack.borrow().top_id() == Some(id))
}

/// Window `id` opened on `window`'s surface, over the others where `shown`: its layer, for
/// its callbacks.
pub(crate) fn open(window: &Arc<AndroidWindow>, id: u64, shown: bool) -> Rc<Layer> {
    let layer = Rc::new(Layer::default());
    STACK.with(|stack| stack.borrow_mut().push(id, shown, Rc::clone(&layer)));
    forward(window);
    if shown {
        uncovered(window);
    }
    layer
}

/// Window `id` brought to the top (GPUI's `activate`).
pub(crate) fn raise(window: &AndroidWindow, id: u64) {
    let was_top = is_top(id);
    if STACK.with(|stack| stack.borrow_mut().raise(id)) && !was_top {
        uncovered(window);
    }
}

/// Window `id` closed (GPUI dropped it). Its layer is dropped after the stack is let go, as
/// its callbacks hold GPUI's own.
pub(crate) fn close(window: &AndroidWindow, id: u64) {
    // `try_with`: a window dropped as the thread ends finds the stack gone, and nothing to do.
    let Ok((was_top, removed)) = STACK.try_with(|stack| {
        let mut stack = stack.borrow_mut();
        let was_top = stack.top_id() == Some(id);
        (was_top, stack.remove(id))
    }) else {
        return;
    };
    drop(removed);
    if was_top {
        uncovered(window);
    }
}

/// The window now on top is new to the surface: drawn in full at the next frame (what is on
/// the surface is another window's), and told it is active then.
fn uncovered(window: &AndroidWindow) {
    window.repaint_in_full();
    super::frame_source::schedule_frame();
}

/// Tells each window whether it is active: the window on top while the app is in front, no
/// other. Run outside GPUI's own updates (a frame, a change of focus), as GPUI's callback
/// updates the window; those losing it are told first.
fn settle(app_active: bool) {
    let top = STACK.with(|stack| stack.borrow().top_id());
    let layers = all();
    for wanted in [false, true] {
        for (id, layer) in &layers {
            let active = app_active && Some(*id) == top;
            if active != wanted || layer.told_active.get() == Some(active) {
                continue;
            }
            if call(&layer.active, |told| told(active)).is_some() {
                layer.told_active.set(Some(active));
            }
        }
    }
}

/// The surface's callbacks set to forward to the layers (the same each time, so setting them
/// again for another window changes nothing).
fn forward(window: &Arc<AndroidWindow>) {
    let surface = Arc::downgrade(window);
    window.on_request_frame(move || {
        settle(surface.upgrade().is_none_or(|surface| surface.is_active()));
        if let Some((_, layer)) = top() {
            call(&layer.frame, |frame| frame());
        }
    });
    window.on_touch(|touch| {
        if let Some((_, layer)) = top() {
            call(&layer.touch, |touched| touched(touch));
        }
    });
    window.on_key_event(key);
    window.on_resize(|size, scale| {
        for (_, layer) in all() {
            call(&layer.resize, |resized| resized(size, scale));
        }
    });
    window.on_appearance_changed(|appearance| {
        for (_, layer) in all() {
            call(&layer.appearance, |changed| changed(appearance));
        }
    });
    window.on_active_status_change(settle);
    window.on_visibility_change(|visible| {
        for (_, layer) in all() {
            call(&layer.visible, |seen| seen(visible));
        }
    });
}

/// A key to the window on top; Back on its release, as Android's own Back acts.
fn key(event: AndroidKeyEvent) {
    if event.key_code != AKEYCODE_BACK {
        if let Some((_, layer)) = top() {
            call(&layer.key, |key| key(event));
        }
        return;
    }
    if event.action == AKEY_EVENT_ACTION_UP {
        back(event);
    }
}

/// Back: Escape pressed and released in the window on top (the keyboard's mapping gives
/// Back as Escape), then, where the window did nothing with it, the window closed or the task
/// sent to the background.
fn back(release: AndroidKeyEvent) {
    let before = top();
    let mut handled = false;
    if let Some((_, layer)) = &before {
        let press = AndroidKeyEvent {
            action: AKEY_EVENT_ACTION_DOWN,
            ..release.clone()
        };
        let pressed = call(&layer.key, |key| key(press)).unwrap_or(UNUSED);
        call(&layer.key, |key| key(release));
        handled = !pressed.propagate || pressed.default_prevented;
    }
    let (top_after, shown_after) = STACK.with(|stack| {
        let stack = stack.borrow();
        (stack.top_id(), stack.shown())
    });
    let top_before = before.as_ref().map(|(id, _)| *id);
    match after_back(handled, top_before, top_after, shown_after) {
        Back::Done => {}
        Back::Close(_) => {
            let Some((_, layer)) = before else { return };
            // As the window's close button: asked first (a dialog's Cancel may keep it).
            if call(&layer.should_close, |may| may()).unwrap_or(true) {
                let close = layer.close.borrow_mut().take();
                if let Some(close) = close {
                    close();
                }
            }
        }
        Back::ToBackground => super::jni::move_task_to_back(),
    }
}
