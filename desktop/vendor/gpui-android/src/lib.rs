//! GPUI's Android platform, from gpui-mobile
//! (<https://github.com/itsbalamurali/gpui-mobile>, commit
//! `5fd54f4554263ae06753e90472c7f2e2ff5d7495`), taken by dm under GPL-3.0-or-later.
//!
//! dm(android-only): gpui-mobile's Android platform alone. Its iOS platform, components, packages
//! (Flutter-style plugins), platform views (Java views over the GPUI surface), host-driven
//! `SurfaceView` entry (`android/host.rs`) and the iOS frame demand are removed; what
//! remains is the `android` module, the soft-keyboard and system-chrome calls below, and
//! the touch helpers (`fling_guard`, `frame_pacer`, `momentum`). dm's changes are marked
//! `dm(<tag>):`, each tag a row of the table in `desktop/vendor/README.md` (gaps row N16).
//!
//! The Android implementation uses the NDK for windowing and `gpui_wgpu` for
//! Vulkan rendering.  Key modules:
//!
//! - `platform`   — `AndroidPlatform` implementing the GPUI `Platform` trait
//! - `window`     — `AndroidWindow` backed by `ANativeWindow` + `gpui_wgpu`
//! - `display`    — `AndroidDisplay` wrapping NDK display info
//! - `dispatcher` — `AndroidDispatcher` using `ALooper` + thread pool
//! - `keyboard`   — Android NDK key code → GPUI `Keystroke` mapping
//! - `jni`        — `android_main` support, JNI entry points + event loop

// ── Re-export the gpui crate so consumers can access types through us ────────

pub use gpui;

// ── shared modules ───────────────────────────────────────────────────────────

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) mod fling_guard;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) mod frame_pacer;
pub mod momentum;

// ── System chrome (status bar / navigation bar) styling ──────────────────────

/// Controls the appearance of the device status bar text and icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusBarContentStyle {
    /// White text/icons — use on dark backgrounds.
    Light,
    /// Dark text/icons — use on light backgrounds.
    #[default]
    Dark,
}

/// Configures the system chrome (status bar and navigation bar) appearance.
///
/// Use [`set_system_chrome`] to apply a style. Colors are specified as
/// 0xRRGGBB values (no alpha). Pass `None` for a color field to leave it
/// unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemChromeStyle {
    /// Background color for the top safe area (behind the status bar).
    pub status_bar_color: Option<u32>,
    /// Whether the status bar content (text/icons) should be light or dark.
    pub status_bar_style: StatusBarContentStyle,
    /// Background color for the bottom safe area (behind the home indicator / nav bar).
    pub navigation_bar_color: Option<u32>,
}

impl Default for SystemChromeStyle {
    fn default() -> Self {
        Self {
            status_bar_color: None,
            status_bar_style: StatusBarContentStyle::Dark,
            navigation_bar_color: None,
        }
    }
}

/// Apply system chrome styling (status bar style, navigation bar color).
///
/// On iOS this updates `preferredStatusBarStyle` on the root view controller.
/// On Android this calls `Window.setStatusBarColor()`,
/// `Window.setNavigationBarColor()`, and configures light/dark status bar icons.
///
/// On unsupported platforms this is a no-op.
pub fn set_system_chrome(style: &SystemChromeStyle) {
    #[cfg(target_os = "android")]
    {
        android::jni::set_system_chrome(style);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = style;
    }
}

// ── Text input callback ──────────────────────────────────────────────────────

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

type TextInputCallbackFn = Box<dyn FnMut(&str)>;

/// Global flag indicating that text input was received and a re-render is needed.
///
/// GPUI only redraws when its invalidator is dirty. Since `dispatch_text_input`
/// stores text in a thread-local (`PENDING_TEXT`) outside of GPUI's entity
/// system, nothing marks the window dirty. The platform frame callbacks check
/// this flag and pass `force_render: true` to ensure the render cycle runs,
/// which in turn calls `drain_pending_text()` and updates the UI.
pub static TEXT_INPUT_DIRTY: AtomicBool = AtomicBool::new(false);

/// Marks text input as waiting and asks the platform for the frame that will
/// process it — a host that paused its frame source on an idle screen has
/// to be woken, since typing does not go through GPUI's invalidator.
pub(crate) fn mark_text_input_dirty() {
    TEXT_INPUT_DIRTY.store(true, Ordering::Release);
    #[cfg(target_os = "android")]
    android::frame_source::wake();
}

thread_local! {
    /// Global text input callback — set by the active text input component.
    /// When the software keyboard sends text, this callback is invoked.
    static TEXT_INPUT_CALLBACK: RefCell<Option<TextInputCallbackFn>> = RefCell::new(None);
}

/// Register a callback that receives text from the software keyboard.
///
/// Only one callback can be active at a time. Call with `None` to clear it.
/// This is typically called by the text input component when it gains focus.
pub fn set_text_input_callback(callback: Option<TextInputCallbackFn>) {
    TEXT_INPUT_CALLBACK.with(|cb| {
        *cb.borrow_mut() = callback;
    });
}

/// Whether a software keyboard text callback is registered. A callback that
/// is currently being invoked counts as registered.
#[allow(dead_code)]
pub(crate) fn has_text_input_callback() -> bool {
    TEXT_INPUT_CALLBACK.with(|cb| cb.try_borrow().map_or(true, |cb| cb.is_some()))
}

/// Dispatch text input to the registered callback.
///
/// Called internally by the platform layer when keyboard text is received.
/// Returns true if a callback handled the text. Also sets `TEXT_INPUT_DIRTY`
/// so the platform frame callback forces a re-render.
pub fn dispatch_text_input(text: &str) -> bool {
    TEXT_INPUT_CALLBACK.with(|cb| {
        if let Some(callback) = cb.borrow_mut().as_mut() {
            callback(text);
            mark_text_input_dirty();
            true
        } else {
            false
        }
    })
}

// ── Software keyboard control ────────────────────────────────────────────────

/// The type of software keyboard to present.
///
/// Maps to `UIKeyboardType` on iOS and `InputType` on Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardType {
    /// Standard text keyboard.
    #[default]
    Default,
    /// Keyboard optimized for email addresses (includes @ and .).
    EmailAddress,
    /// Phone number pad.
    Phone,
    /// Numeric keypad (digits only).
    NumberPad,
    /// Keyboard optimized for URL entry.
    URL,
    /// Decimal number pad (digits and decimal point).
    Decimal,
}

/// Show the software keyboard with the default keyboard type.
///
/// On iOS this makes the hidden text input view the first responder.
/// On Android this opens the input method editor.
/// On unsupported platforms this is a no-op.
pub fn show_keyboard() {
    show_keyboard_with_type(KeyboardType::Default);
}

/// Show the software keyboard with a specific keyboard type.
///
/// On iOS this sets the keyboard type on the text input view before
/// making it first responder. On Android this opens the IME with the
/// appropriate input type.
/// On unsupported platforms this is a no-op.
pub fn show_keyboard_with_type(keyboard_type: KeyboardType) {
    #[cfg(target_os = "android")]
    {
        android::jni::show_keyboard_android(keyboard_type);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = keyboard_type;
    }
}

/// Hide the software keyboard.
///
/// On iOS this resigns first responder from the text input view.
/// On Android this closes the input method editor.
/// On unsupported platforms this is a no-op.
pub fn hide_keyboard() {
    #[cfg(target_os = "android")]
    {
        android::jni::hide_keyboard_android();
    }
    #[cfg(not(target_os = "android"))]
    {}
}

// ── Keyboard height ─────────────────────────────────────────────────────────

use std::sync::atomic::AtomicU32;

/// Current keyboard height in logical points, stored as `f32` bits.
///
/// Updated by the platform layer when the software keyboard shows/hides.
/// Read with `keyboard_height()`.
pub static KEYBOARD_HEIGHT_BITS: AtomicU32 = AtomicU32::new(0);

/// Get the current software keyboard height in logical points.
///
/// Returns 0.0 when the keyboard is hidden.
pub fn keyboard_height() -> f32 {
    f32::from_bits(KEYBOARD_HEIGHT_BITS.load(Ordering::Relaxed))
}

/// Set the keyboard height (called from platform layer).
pub fn set_keyboard_height(height: f32) {
    let prev = f32::from_bits(KEYBOARD_HEIGHT_BITS.load(Ordering::Relaxed));
    if (prev - height).abs() > 0.5 {
        KEYBOARD_HEIGHT_BITS.store(height.to_bits(), Ordering::Release);
        mark_text_input_dirty();
    }
}

// ── Safe area insets ─────────────────────────────────────────────────────────

/// Query the safe area insets from the platform.
///
/// Returns `(top, bottom, left, right)` in logical points.
/// On iOS this queries `safeAreaInsets` from the UIView.
/// On Android use the `AndroidWindow::safe_area_insets_logical()` method instead.
/// On unsupported platforms returns zeros.
pub fn safe_area_insets() -> (f32, f32, f32, f32) {
    (0.0, 0.0, 0.0, 0.0)
}

// ── platform modules ─────────────────────────────────────────────────────────

#[cfg(target_os = "android")]
pub mod android;

// ── public re-exports ────────────────────────────────────────────────────────

#[cfg(target_os = "android")]
pub use android::{current_platform, AndroidPlatform};

// ── fallback for non-mobile host builds (e.g. documentation / CI) ────────────

/// Returns the platform implementation for the current mobile OS.
///
/// On host builds (documentation, CI) this always panics — the caller must
/// compile for `aarch64-apple-ios` or an Android target.
///
/// When compiled for iOS, returns an `Rc<dyn gpui::Platform>` backed by `IosPlatform`.
/// When compiled for Android, returns an `Rc<dyn gpui::Platform>` backed by `AndroidPlatform`.
#[cfg(not(target_os = "android"))]
pub fn current_platform(_headless: bool) -> ! {
    panic!(
        "gpui-mobile: `current_platform` is only available when compiled for \
         `target_os = \"ios\"` or `target_os = \"android\"`."
    );
}
