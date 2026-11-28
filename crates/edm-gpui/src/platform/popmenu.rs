// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The row menu on macOS: an `NSMenu` EDM builds itself from the row menu's items, so that each
//! item shows the key bound to its action at its right, as the menu bar's items show theirs
//! (gaps row U27). gpui-component's `NativeMenu`, which the title menus and the row menu on
//! Windows and Linux go through, cannot give an item a key on macOS; otherwise this does what it
//! does: the same words, greyed items, separators, submenu and tick, popped up at the same place
//! in the window's content view, and the item chosen dispatched to the window once the menu has
//! closed, as its `MenuTarget` has it dispatched.
//!
//! The keys are only shown. A popped-up menu is not in the menu bar, so once it has closed AppKit
//! looks for no key in it and a key goes to the window's own binding as before; while it is open
//! its tracking takes the keys, so a key pressed then chooses at most its one item, dispatched
//! once.

use std::cell::Cell;

use gpui::{
    Action, App, FocusHandle, KeyContext, Menu, MenuItem, Pixels, Point, SharedString, Window,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker, Message};
use objc2_app_kit::{
    NSControlStateValueOn, NSDownArrowFunctionKey, NSEndFunctionKey, NSEventModifierFlags,
    NSF1FunctionKey, NSHelpFunctionKey, NSHomeFunctionKey, NSLeftArrowFunctionKey, NSMenu,
    NSMenuItem, NSPageDownFunctionKey, NSPageUpFunctionKey, NSRightArrowFunctionKey,
    NSUpArrowFunctionKey, NSView,
};
use objc2_foundation::{NSPoint, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// One entry of the menu, read from GPUI's `Menu` with its key looked up while the window can
/// still be asked.
enum Entry {
    Separator,
    Item {
        words: SharedString,
        enabled: bool,
        ticked: bool,
        key: Option<Equivalent>,
        action: Box<dyn Action>,
    },
    Submenu {
        words: SharedString,
        enabled: bool,
        entries: Vec<Entry>,
    },
}

/// A menu item's key equivalent: the key's character as AppKit takes it, and its modifiers.
struct Equivalent {
    character: String,
    mask: NSEventModifierFlags,
}

/// What [`Target`] keeps: the tag of the item chosen, `-1` while none is.
struct Chosen {
    tag: Cell<isize>,
}

define_class!(
    // The menu's items' target for as long as the menu is open: it keeps the tag of the item
    // chosen.
    #[unsafe(super(NSObject))]
    #[name = "EdmGpuiRowMenuTarget"]
    #[ivars = Chosen]
    struct Target;

    impl Target {
        #[unsafe(method(rowMenuItemChosen:))]
        fn chosen(&self, sender: &NSMenuItem) {
            self.ivars().tag.set(sender.tag());
        }
    }
);

impl Target {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(Chosen { tag: Cell::new(-1) });
        // SAFETY: `this` is a newly allocated `Target` with its ivars set; NSObject's `init`
        // initialises it and returns it.
        unsafe { msg_send![super(this), init] }
    }
}

/// Pops `menu` up at `at` (window points, from the top left) over `window`, and dispatches the
/// action of the item chosen to the window once it has closed. AppKit's tracking runs in a task
/// of the window's thread of its own, so GPUI is not borrowed while the menu is open.
pub(crate) fn show(menu: Menu, at: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let Some(view) = content_view(window) else {
        return;
    };
    let focused = window.focused(cx);
    let entries = read(menu.items, focused.as_ref(), window);
    if entries.is_empty() {
        return;
    }
    // GPUI's own handle of the window, not the system's of the same name.
    let handle = Window::window_handle(window);
    cx.spawn(async move |cx| {
        let action = run(&view, &entries, at);
        cx.update(move |app| {
            let _ = handle.update(app, move |_, window, app| {
                if let Some(action) = action {
                    window.dispatch_action(action, app);
                }
                // Drawn again once the tracking has returned, chosen or not, as `NativeMenu`
                // has it: else the window stays idle and a second right-click goes unanswered.
                window.refresh();
            });
        });
    })
    .detach();
}

/// The window's content view, kept for the menu's task.
fn content_view(window: &Window) -> Option<Retained<NSView>> {
    // The system's handle, not GPUI's own of the same name.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: `ns_view` is the live content view of `window`, which this borrow of the window
    // keeps open; it is retained here, on the main thread, where this runs.
    let view: &NSView = unsafe { appkit.ns_view.cast().as_ref() };
    Some(view.retain())
}

/// GPUI's items as the menu's entries, each with the key bound to its action; a menu of the
/// system's (Services) has no place in a popped-up menu and is left out, as `NativeMenu` leaves
/// it out.
fn read(items: Vec<MenuItem>, focused: Option<&FocusHandle>, window: &Window) -> Vec<Entry> {
    let mut entries = Vec::new();
    for item in items {
        match item {
            MenuItem::Separator => entries.push(Entry::Separator),
            MenuItem::Action {
                name,
                action,
                checked,
                disabled,
                ..
            } => entries.push(Entry::Item {
                words: name,
                enabled: !disabled,
                ticked: checked,
                key: bound_key(action.as_ref(), focused, window),
                action,
            }),
            MenuItem::Submenu(submenu) => entries.push(Entry::Submenu {
                words: submenu.name,
                enabled: !submenu.disabled,
                entries: read(submenu.items, focused, window),
            }),
            MenuItem::SystemMenu(_) => {}
        }
    }
    entries
}

/// The key `action` is bound to, as the drawn menu on Linux finds it: for the element focused,
/// else wherever focus is. `None` where it has no binding or a sequence of keys, which a menu
/// item cannot show.
fn bound_key(
    action: &dyn Action,
    focused: Option<&FocusHandle>,
    window: &Window,
) -> Option<Equivalent> {
    let binding = focused
        .and_then(|handle| window.highest_precedence_binding_for_action_in(action, handle))
        .or_else(|| {
            window.highest_precedence_binding_for_action_in_context(action, KeyContext::default())
        })?;
    let [only] = binding.keystrokes() else {
        return None;
    };
    let modifiers = only.modifiers();
    let mut mask = NSEventModifierFlags::empty();
    for (held, flag) in [
        (modifiers.platform, NSEventModifierFlags::Command),
        (modifiers.control, NSEventModifierFlags::Control),
        (modifiers.alt, NSEventModifierFlags::Option),
        (modifiers.shift, NSEventModifierFlags::Shift),
    ] {
        if held {
            mask |= flag;
        }
    }
    Some(Equivalent {
        character: character(only.key())?,
        mask,
    })
}

/// The character AppKit takes as a menu item's key equivalent for GPUI's name of a key, as
/// GPUI's own menu bar gives it (`key_to_menu_equivalent`, private to GPUI's macOS crate): a
/// named key's own character, a single character as it is; `None` for a name it does not know.
fn character(key: &str) -> Option<String> {
    let code = match key {
        "space" => u32::from(b' '),
        // A menu's key equivalent takes other characters for these two than a key's event
        // reports: https://developer.apple.com/documentation/appkit/nsmenuitem/keyequivalent
        "backspace" => 0x08,
        "delete" => 0x7f,
        "escape" => 0x1b,
        "enter" => 0x0d,
        "tab" => 0x09,
        "up" => NSUpArrowFunctionKey,
        "down" => NSDownArrowFunctionKey,
        "left" => NSLeftArrowFunctionKey,
        "right" => NSRightArrowFunctionKey,
        "pageup" => NSPageUpFunctionKey,
        "pagedown" => NSPageDownFunctionKey,
        "home" => NSHomeFunctionKey,
        "end" => NSEndFunctionKey,
        "insert" => NSHelpFunctionKey,
        _ if key.chars().count() == 1 => return Some(key.to_string()),
        // F1 to F35, whose characters follow one another.
        _ => key
            .strip_prefix('f')
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| (1..=35).contains(n))
            .map(|n| NSF1FunctionKey + n - 1)?,
    };
    char::from_u32(code).map(String::from)
}

/// Builds the menu, pops it up and waits for it to close: the action of the item chosen, `None`
/// where none was.
fn run(view: &NSView, entries: &[Entry], at: Point<Pixels>) -> Option<Box<dyn Action>> {
    let mtm = MainThreadMarker::new()?;
    let target = Target::new();
    let mut actions = Vec::new();
    let menu = build(entries, &target, mtm, &mut actions);
    // GPUI's points have their origin at the top left; the view's at the bottom left.
    let height = view.bounds().size.height;
    let location = NSPoint::new(
        f64::from(f32::from(at.x)),
        height - f64::from(f32::from(at.y)),
    );
    menu.popUpMenuPositioningItem_atLocation_inView(None, location, Some(view));
    let tag = target.ivars().tag.get();
    let chosen = usize::try_from(tag).ok().and_then(|tag| actions.get(tag))?;
    Some(chosen.boxed_clone())
}

/// The `NSMenu` of `entries`, a submenu's built in its place. Each item that can be chosen is
/// tagged with its place in `actions`, so the tag chosen finds its action.
fn build<'a>(
    entries: &'a [Entry],
    target: &Target,
    mtm: MainThreadMarker,
    actions: &mut Vec<&'a dyn Action>,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    // Each item's state is set here, not worked out by AppKit.
    menu.setAutoenablesItems(false);
    for entry in entries {
        match entry {
            Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            Entry::Item {
                words,
                enabled,
                ticked,
                key,
                action,
            } => {
                let item = NSMenuItem::new(mtm);
                item.setTitle(&NSString::from_str(words));
                item.setEnabled(*enabled);
                if *ticked {
                    item.setState(NSControlStateValueOn);
                }
                if let Some(key) = key {
                    set_key(&item, key);
                }
                if *enabled {
                    item.setTag(actions.len() as isize);
                    actions.push(action.as_ref());
                    let target: &AnyObject = target;
                    // SAFETY: `target` is a live `Target`, kept until the menu has closed
                    // (`run`), and it answers the selector set here.
                    unsafe {
                        item.setTarget(Some(target));
                        item.setAction(Some(sel!(rowMenuItemChosen:)));
                    }
                }
                menu.addItem(&item);
            }
            Entry::Submenu {
                words,
                enabled,
                entries,
            } => {
                let item = NSMenuItem::new(mtm);
                item.setTitle(&NSString::from_str(words));
                item.setEnabled(*enabled);
                item.setSubmenu(Some(&build(entries, target, mtm, actions)));
                menu.addItem(&item);
            }
        }
    }
    menu
}

/// `key` as the item's key equivalent, set as GPUI's own menu bar sets its items': not moved to
/// another key on another keyboard's layout, as macOS 12 and later would.
fn set_key(item: &NSMenuItem, key: &Equivalent) {
    // macOS 11 has no such property.
    if item.respondsToSelector(sel!(setAllowsAutomaticKeyEquivalentLocalization:)) {
        item.setAllowsAutomaticKeyEquivalentLocalization(false);
    }
    item.setKeyEquivalent(&NSString::from_str(&key.character));
    item.setKeyEquivalentModifierMask(key.mask);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key's character as GPUI's menu bar gives it to AppKit: named keys their own, a letter
    /// or a sign itself, and the function keys in their run.
    #[test]
    fn each_key_is_the_character_the_menu_bar_gives() {
        assert_eq!(character("r").as_deref(), Some("r"));
        assert_eq!(character(",").as_deref(), Some(","));
        assert_eq!(character(".").as_deref(), Some("."));
        assert_eq!(character("backspace").as_deref(), Some("\u{8}"));
        assert_eq!(character("delete").as_deref(), Some("\u{7f}"));
        assert_eq!(character("space").as_deref(), Some(" "));
        assert_eq!(character("up").as_deref(), Some("\u{f700}"));
        assert_eq!(character("f1").as_deref(), Some("\u{f704}"));
        assert_eq!(character("f3").as_deref(), Some("\u{f706}"));
        assert_eq!(character("f35").as_deref(), Some("\u{f726}"));
        assert_eq!(character("f36"), None);
        assert_eq!(character("menu"), None);
    }
}
