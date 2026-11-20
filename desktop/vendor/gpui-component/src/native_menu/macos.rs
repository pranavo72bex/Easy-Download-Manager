//! macOS native menu implementation (AppKit `NSMenu` via objc2).

use std::{cell::Cell, sync::Arc};

use gpui::{Action, App, AssetSource, FocusHandle, KeyContext, Keystroke, Pixels, Point, Window};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSDownArrowFunctionKey, NSEndFunctionKey, NSEventModifierFlags, NSF1FunctionKey,
    NSHelpFunctionKey, NSHomeFunctionKey, NSImage, NSLeftArrowFunctionKey, NSMenu, NSMenuItem,
    NSPageDownFunctionKey, NSPageUpFunctionKey, NSRightArrowFunctionKey, NSUpArrowFunctionKey,
    NSView,
};
use objc2_foundation::{NSData, NSPoint, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{NativeMenuItem, resolve_icon_image};

/// Side length (in points) menu item images are scaled to. AppKit does not resize the image to fit
/// the row, so a large file would otherwise overflow it.
const MENU_IMAGE_SIZE: f64 = 16.0;

/// Ivars for [`MenuTarget`]: the tag of the selected item, or `-1` if none.
struct MenuTargetIvars {
    selected: Cell<isize>,
}

define_class!(
    // A throwaway Objective-C object that receives the menu item action and
    // records which item (by tag) was clicked.
    #[unsafe(super(NSObject))]
    #[name = "GPUIComponentNativeMenuTarget"]
    #[ivars = MenuTargetIvars]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(menuItemClicked:))]
        fn menu_item_clicked(&self, sender: &NSMenuItem) {
            self.ivars().selected.set(sender.tag());
        }
    }
);

impl MenuTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(MenuTargetIvars {
            selected: Cell::new(-1),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Show a native popup menu and dispatch the selected item's action.
///
/// The AppKit tracking loop is run from a foreground task so that GPUI is not
/// borrowed while the menu is open.
pub(super) fn show(
    items: Vec<NativeMenuItem>,
    asset_source: Arc<dyn AssetSource>,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(view_ptr) = ns_view_ptr(window) else {
        return;
    };
    // Inherent `Window::window_handle` (GPUI's `AnyWindowHandle`), not the
    // `raw_window_handle::HasWindowHandle` trait method in scope below.
    let handle = Window::window_handle(window);
    // dm(mac-keys): each item's key, looked up while the window can still be asked.
    let keys = item_keys(&items, window, cx);

    cx.spawn(async move |cx| {
        let action = run_menu(view_ptr, &items, &keys, asset_source.as_ref(), position);
        let _ = cx.update(move |app| {
            let _ = handle.update(app, move |_, window, app| {
                if let Some(action) = action {
                    window.dispatch_action(action, app);
                }
                // Wake GPUI after the AppKit tracking loop returns so the window
                // resumes painting and re-registers its mouse handlers. Without
                // this, a dismissed menu (especially when nothing is selected)
                // leaves the window idle and unresponsive to a second
                // right-click.
                window.refresh();
            });
        });
    })
    .detach();
}

/// Build the menu (recursively, including submenus), show it, and return the
/// selected item's action.
fn run_menu(
    view_ptr: usize,
    items: &[NativeMenuItem],
    keys: &[Option<Keystroke>],
    asset_source: &dyn AssetSource,
    position: Point<Pixels>,
) -> Option<Box<dyn Action>> {
    let mtm = MainThreadMarker::new()?;
    // SAFETY: `view_ptr` came from the window's AppKit handle, and the window
    // outlives this synchronous call.
    let view: &NSView = unsafe { &*(view_ptr as *const NSView) };

    let target = MenuTarget::new();
    let mut actions: Vec<&Box<dyn Action>> = Vec::new();
    let ns_menu = build_menu(
        items,
        &mut keys.iter(),
        asset_source,
        &target,
        mtm,
        &mut actions,
    );

    // `position` is window-relative, logical pixels, origin top-left (GPUI).
    // AppKit view coordinates have their origin at the bottom-left, so flip y.
    let height = view.bounds().size.height;
    let location = NSPoint::new(
        f32::from(position.x) as f64,
        height - f32::from(position.y) as f64,
    );
    ns_menu.popUpMenuPositioningItem_atLocation_inView(None, location, Some(view));

    let tag = target.ivars().selected.get();
    if tag >= 0 {
        actions.get(tag as usize).map(|action| action.boxed_clone())
    } else {
        None
    }
}

/// Recursively build an `NSMenu`. Each actionable leaf item is given a tag equal
/// to its index in `actions`, so the selected tag maps back to its action.
/// dm(mac-keys): `keys` holds each item's key, in the order the items are walked here.
fn build_menu<'a>(
    items: &'a [NativeMenuItem],
    keys: &mut std::slice::Iter<'_, Option<Keystroke>>,
    asset_source: &dyn AssetSource,
    target: &MenuTarget,
    mtm: MainThreadMarker,
    actions: &mut Vec<&'a Box<dyn Action>>,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    // Items are configured explicitly, so disable AppKit's automatic enabling.
    menu.setAutoenablesItems(false);

    for item in items {
        match item {
            NativeMenuItem::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            NativeMenuItem::Item {
                label,
                disabled,
                checked,
                icon,
                action,
            } => {
                let ns_item = NSMenuItem::new(mtm);
                unsafe {
                    ns_item.setTitle(&NSString::from_str(label));
                    ns_item.setEnabled(!*disabled);
                    if let Some(icon) = icon {
                        if let Some(ns_image) = ns_image_for_icon(icon, asset_source) {
                            ns_image.setSize(NSSize::new(MENU_IMAGE_SIZE, MENU_IMAGE_SIZE));
                            ns_image.setTemplate(true);
                            ns_item.setImage(Some(&ns_image));
                        }
                    }

                    if *checked {
                        // `NSControlStateValueOn`
                        ns_item.setState(1);
                    }
                    // dm(mac-keys): the key bound to the item's action, shown at its right.
                    if let Some(Some(key)) = keys.next() {
                        set_key_equivalent(&ns_item, key);
                    }
                    if let Some(action) = action {
                        if !*disabled {
                            ns_item.setTag(actions.len() as isize);
                            actions.push(action);
                            ns_item.setTarget(Some(target as &AnyObject));
                            ns_item.setAction(Some(sel!(menuItemClicked:)));
                        }
                    }
                }
                menu.addItem(&ns_item);
            }
            NativeMenuItem::Submenu {
                label,
                disabled,
                items,
            } => {
                let ns_item = NSMenuItem::new(mtm);
                let submenu = build_menu(items, keys, asset_source, target, mtm, actions);
                ns_item.setTitle(&NSString::from_str(label));
                ns_item.setEnabled(!*disabled);
                ns_item.setSubmenu(Some(&submenu));
                menu.addItem(&ns_item);
            }
        }
    }

    menu
}

/// dm(mac-keys): the key bound to each item's action, in the order [`build_menu`] walks the
/// items (a submenu's in its place); `None` where an item has none. Looked up as the drawn
/// fallback menu looks its keys up: for the focused element, else wherever focus is.
fn item_keys(items: &[NativeMenuItem], window: &Window, cx: &App) -> Vec<Option<Keystroke>> {
    let focused = window.focused(cx);
    let mut keys = Vec::new();
    collect_keys(items, focused.as_ref(), window, &mut keys);
    keys
}

/// dm(mac-keys): [`item_keys`]' walk, adding each item's key to `keys`.
fn collect_keys(
    items: &[NativeMenuItem],
    focused: Option<&FocusHandle>,
    window: &Window,
    keys: &mut Vec<Option<Keystroke>>,
) {
    for item in items {
        match item {
            NativeMenuItem::Separator => {}
            NativeMenuItem::Item { action, .. } => keys.push(
                action
                    .as_ref()
                    .and_then(|action| bound_key(&**action, focused, window)),
            ),
            NativeMenuItem::Submenu { items, .. } => collect_keys(items, focused, window, keys),
        }
    }
}

/// dm(mac-keys): the one keystroke `action` is bound to; `None` where it has no binding or a
/// sequence of keys, which a menu item cannot show.
fn bound_key(
    action: &dyn Action,
    focused: Option<&FocusHandle>,
    window: &Window,
) -> Option<Keystroke> {
    let binding = focused
        .and_then(|handle| window.highest_precedence_binding_for_action_in(action, handle))
        .or_else(|| {
            window.highest_precedence_binding_for_action_in_context(action, KeyContext::default())
        })?;
    match binding.keystrokes() {
        [only] => Some(only.inner().clone()),
        _ => None,
    }
}

/// dm(mac-keys): `key` as the item's key equivalent, set as GPUI's own menu bar sets its
/// items' (`gpui-pre-macos` `create_menu_item`): the key's character and its modifiers' mask,
/// not moved to another key on other keyboard layouts (as macOS 12 and later would).
///
/// The equivalent is only shown: a popped-up menu is not in the menu bar, so once it is closed
/// AppKit looks for no key equivalent in it, and the key goes to the window's own binding as
/// before. While the menu is open its tracking takes every key, so a key pressed then chooses
/// at most this one item, dispatched once through [`MenuTarget`].
fn set_key_equivalent(item: &NSMenuItem, key: &Keystroke) {
    let Some(character) = menu_equivalent(&key.key) else {
        return;
    };
    let mut mask = NSEventModifierFlags::empty();
    for (held, flag) in [
        (key.modifiers.platform, NSEventModifierFlags::Command),
        (key.modifiers.control, NSEventModifierFlags::Control),
        (key.modifiers.alt, NSEventModifierFlags::Option),
        (key.modifiers.shift, NSEventModifierFlags::Shift),
    ] {
        if held {
            mask |= flag;
        }
    }
    // macOS 11 has no such property.
    if item.respondsToSelector(sel!(setAllowsAutomaticKeyEquivalentLocalization:)) {
        item.setAllowsAutomaticKeyEquivalentLocalization(false);
    }
    item.setKeyEquivalent(&NSString::from_str(&character));
    item.setKeyEquivalentModifierMask(mask);
}

/// dm(mac-keys): the character AppKit takes as a menu item's key equivalent for GPUI's name of
/// a key, as `gpui-pre-macos`' `key_to_menu_equivalent` (private to that crate) gives it: a
/// named key's own character, a single character as it is; `None` for a name it does not know.
fn menu_equivalent(key: &str) -> Option<String> {
    let code = match key {
        "space" => u32::from(b' '),
        // A menu's key equivalent takes other characters than NSEvent reports for these two:
        // https://developer.apple.com/documentation/appkit/nsmenuitem/keyequivalent#Discussion
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

fn ns_image_for_icon(
    icon: &crate::Icon,
    asset_source: &dyn AssetSource,
) -> Option<Retained<NSImage>> {
    let image = resolve_icon_image(icon, asset_source)?;
    if image.bytes.is_empty() {
        return None;
    }

    let data = unsafe {
        NSData::dataWithBytes_length(image.bytes.as_ptr().cast(), image.bytes.len() as _)
    };
    NSImage::initWithData(NSImage::alloc(), &data)
}

/// Extract the AppKit `NSView` pointer from the window's raw handle.
fn ns_view_ptr(window: &Window) -> Option<usize> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.ns_view.as_ptr() as usize)
}
