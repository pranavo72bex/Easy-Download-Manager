# Platform gap analysis: egui/eframe to GPUI

Every OS integration and platform-specific piece of the desktop app today,
and what GPUI offers for each. Read from the code on branch `gpui-migration`
(head `aea95ae`, 2026-11-09). GPUI facts come from Zed's source at commit
`4ed72e0` (2026-11-09). Which GPUI distribution to depend on is a separate
decision, made in [GPUI_DEPENDENCY_DECISION.md](GPUI_DEPENDENCY_DECISION.md).
The current window's structure is described in
[CURRENT_UI_ARCHITECTURE.md](CURRENT_UI_ARCHITECTURE.md).

## How to read this

**Coupling** says how much of each piece is tied to the current toolkit:

* **Keep.** The piece does not use egui, eframe or winit types. It can carry
  over as it is. At most its caller moves, for example from an egui frame to
  a GPUI entity or task.
* **Adapt.** The logic does not depend on the toolkit, but it is attached to
  something winit or eframe owns: winit's event loop, winit's ObjC delegate,
  winit's `IDropTarget`, eframe's viewport commands, or an `egui::Context`
  used as a waker. The OS calls stay; the attachment point must be rewritten
  against GPUI.
* **Redo.** Written in egui, eframe or winit, or in a vendored patch of one.
  It must be rewritten for GPUI.

**GPUI** says what GPUI offers for the piece:

* "Verified" means read in Zed's source at `4ed72e0` (paths are under
  `crates/`) or in Zed's docs.
* "To verify in Phase 1" means it could not be confirmed by reading and
  needs a build or a run.
* GPUI here means upstream `main`. The `gpui` 0.2.2 crate on crates.io (2025-10-22)
  lacks AccessKit, system notifications, wgpu on Linux and the separate
  platform crates. Most "verified" rows below do not hold for 0.2.2. See
  GPUI_DEPENDENCY_DECISION.md §2.

**Tests** names what covers the piece today:

* Rust tests are under `desktop/crates/dm-desktop/tests/` (abbreviated
  `tests/`).
* Scripts are under `scripts/`.
* `human-qa.py` is the behaviour contract's full run.

**Paths:**

* `src/` means `desktop/crates/dm-desktop/src/`.
* `shell/` means `desktop/crates/dm-shell/src/`.
* `vendor/` means `desktop/vendor/` (patched crates, see
  `desktop/vendor/README.md`).

## 0. Baseline: what eframe and winit carry today

| Fact | Where |
|---|---|
| `eframe 0.37` features: `default_fonts`, `glow` (OpenGL), `persistence`, `wayland`, `x11`, `accesskit`, `clipboard` | `dm-desktop/Cargo.toml` |
| `egui-winit 0.37` and `winit 0.30` are named directly. They are used for the hidden login start (`hidden.rs`) and for input to the dialogs' windows | `dm-desktop/Cargo.toml`, `src/hidden.rs`, `vendor/egui-winit` |
| Three patched crates make up the window layer. **winit**: links dropped on X11 and Wayland, Wayland drag-out. **egui-winit**: `OTHER_WINDOWS_INPUT`, so control input reaches dialog windows. **eframe**: an AccessKit adapter for each window | `desktop/Cargo.toml` `[patch.crates-io]`, `vendor/README.md` |
| `egui_kittest 0.37` (with `wgpu` and `snapshot`) is the harness for the window tests: 485 `#[test]` functions in `tests/`, 188 `Harness` references | `dm-desktop/Cargo.toml` dev-deps, `tests/window/support.rs` |
| The control protocol's `ui.*` methods let scripts and the MCP client drive the real window: `ui.click`, `ui.key`, `ui.type`, `ui.scroll`, `ui.texts`, `ui.window(s)`, `ui.move_window`, `ui.accessibility`, `ui.inspect`, `ui.place`, `ui.select`, `ui.screenshot`. These are egui-specific: `src/app/human.rs` has 45 egui references | `src/control.rs`, `src/app/human.rs` |

None of this survives a move to GPUI; section 9 covers it.

## 1. Window creation and lifecycle

| # | Integration | What it does | Files | API today | OS | Coupling | Tests | GPUI |
|---|---|---|---|---|---|---|---|---|
| 1 | Main window | 820x500, title and app id "dm", minimum size, icon. Position and size are remembered across launches (`persist_window`, `window_state_file`). With Mica on, the window is made transparent | `src/main.rs` (`ViewportBuilder`, `NativeOptions`, `run_native`) | eframe/winit | all | Redo | `src/main.rs` test `a_run_with_its_own_home_keeps_its_window_state_there`; `human-qa.py` B25 | `cx.open_window(WindowOptions{window_bounds, titlebar, app_id, window_min_size, icon (X11 only), window_background, …})`, verified (`gpui/src/platform.rs` `WindowOptions`). Bounds are not persisted: dm must save and restore `window_bounds` itself |
| 2 | Close to tray | The close button hides the window when a tray exists, and quits when there is none. On a Wayland compositor with no minimize, close quits (`minimize_is_ignored`). Close requests use `close_requested` and `CancelClose` | `src/window.rs` `tick`, `hide`, `show` | egui `ViewportCommand::{Visible, Minimized, CancelClose, Close, Focus, OuterPosition, Maximized}` | all; Wayland has its own path | Redo | `scripts/linux-gui-check.sh` (close into the tray, then Show), `linux-wayland-close-check.sh` (sway), `linux-kde-close-check.sh` (KWin), `tests/quit.rs`, `human-qa.py` | `on_window_should_close` and `on_window_closed` exist, verified. With `QuitMode::Explicit` the app runs with no window, verified (`gpui/src/app.rs`). There is **no per-window hide or show after creation**; only `show` at creation. So "hide" becomes closing the window and reopening it, with bounds restored. `cx.hide()` hides the whole app, macOS-style |
| 3 | Hidden start (login launch) | dm waits in the tray with no window and no GL context, saving about 40 MB. It uses a stand-in context until the window is first asked for. The window is created lazily on a hand-built winit event loop | `src/hidden.rs` (`run_hidden`, `Lazy`, 19 winit references) | winit `ApplicationHandler`, eframe | all | Redo, simpler in GPUI | `scripts/launch-latency.py`, `memory-check.py`, `human-qa.py` | `Application::run` with `QuitMode::Explicit` and no `open_window` until needed should give this directly. Whether a GPU context is created before the first window is to verify in Phase 1 |
| 4 | Minimize and restore, maximized state | Restores a minimized or maximized window when shown again (`hidden_maximized`) | `src/window.rs` | egui viewport commands | all | Redo | `linux-wayland-close-check.sh`, `human-qa.py` | `minimize_window`, `activate_window`, `zoom_window` (`gpui/src/window.rs`) and `is_maximized` (`PlatformWindow`) exist. Restore-from-minimized is verified on Windows; on Linux it is to verify in Phase 1 |
| 5 | Single instance | An advisory file lock under dm's home. A second launch hands its arguments to the running dm (inbox) | `shell/instance.rs`, `shell/shell.rs` `hand_to_running` | std fs lock, `libc` | all | Keep | `dm-shell` unit tests, `scripts/linux-check.sh`, `arch-check.sh` | Not needed from GPUI |
| 6 | Dialogs as separate OS windows (contract **B42**) | Each dialog is its own window with the system title bar. It can be moved anywhere. Modal dialogs hold the main window | `src/dialogs/apart.rs`, `dialogs/frame.rs`, `vendor/egui-winit` (`OTHER_WINDOWS_INPUT`) | egui immediate viewports | all | Redo | `tests/dialog_windows.rs`; `windows-a11y-check.py`; `human-qa.py` B42 (`win32_move`, `win32_drag`, `screen_region`) | Multiple windows are verified. `WindowKind::Dialog` behaves differently per OS (`gpui_macos/src/window.rs`, `gpui_windows/src/window.rs`, `gpui_linux/.../x11/window.rs`, `wayland/window.rs`). **macOS:** a sheet (`beginSheet`) attached to the main window, which is not movable anywhere and so breaks B42. **Windows:** an owned window with `EnableWindow(parent,false)`; the parent is `GetActiveWindow()`. **X11 and Wayland:** a transient window, made modal through `xdg_dialog_v1` where the compositor offers it. On macOS dm must use `Normal` or `Floating` and do modality itself |
| 7 | Floating drop target | A small window with no decorations, transparent, always on top. It takes drops and can be dragged | `src/app/drops.rs` | egui viewport `with_decorations(false)`, `with_transparent`, `with_always_on_top` | all | Redo | `tests/app_more_cover.rs` (drop target while hidden) | Transparency: `WindowBackgroundAppearance::Transparent` is verified. For "always on top" there are three candidates. **`PopUp`:** `WS_EX_TOPMOST` on Windows and `NSPopUpWindowLevel` on macOS, both verified, but it is a non-activating panel. **`Floating`:** "above its parent" only. **Linux:** global always-on-top is to verify in Phase 1 (`LayerShell` on Wayland only) |
| 8 | Window frame colour, Mica (Windows) | Dark title bar and caption colour set to the background. Mica is opt-in | `src/chrome.rs` | `windows-sys` DWM (`DWMWA_USE_IMMERSIVE_DARK_MODE`, `DWMWA_CAPTION_COLOR`, `DWMWA_TEXT_COLOR`, `DWMWA_SYSTEMBACKDROP_TYPE`) via `raw-window-handle` | Windows | Adapt (it needs the HWND) | `windows-modules-check.sh` (compiles only), `human-qa.py` on Windows | GPUI sets immersive dark mode itself and offers `WindowBackgroundAppearance::{MicaBackdrop, MicaAltBackdrop}`, verified (`gpui_windows/src/window.rs`). `Window` implements `HasWindowHandle`, verified (`gpui/src/window.rs`), so dm's caption-colour call can stay |
| 9 | Quit on system signal | SIGTERM, SIGINT and SIGHUP, and Windows console, logoff and shutdown events, quit as Program › Exit does | `src/signals.rs` | tokio `signal`, `windows` | all | Keep (the flag is polled by the UI; move it to a GPUI task) | `tests/quit.rs` | Not needed from GPUI |
| 10 | Window app id / WM class | `with_app_id("dm")` matches the `.desktop` file's `StartupWMClass=dm` on Wayland | `src/main.rs`, `desktop/packaging/deb.desktop`, `dm.desktop` | winit | Linux | Redo (one line) | `linux-wayland-close-check.sh`, `tests/packaging.rs` | `WindowOptions.app_id` is verified. Whether X11 `WM_CLASS` is set from it is to verify in Phase 1 |

## 2. Tray, notifications, sounds

| # | Integration | What it does | Files | API today | OS | Coupling | Tests | GPUI |
|---|---|---|---|---|---|---|---|---|
| 11 | System tray and its menu | Tray menu items: Show, Start queue, Stop all, Speed limiter (tick and presets, plus Settings…; contract **B52**), Scheduler, Quit. A left click shows the window. If there is no StatusNotifier host, dm says so and close quits | `src/tray.rs` | `tray-icon 0.26` (`ksni` feature on Linux, StatusNotifierItem over D-Bus, no GTK) with `muda` menus. `zbus` checks `org.kde.StatusNotifierWatcher`. An `egui::Context` is used as the waker | Win, macOS, Linux X11 and Wayland | Adapt: only the waker is egui | `linux-gui-check.sh` (`scripts/linux/tray_host.py` and `tray_watcher.py`, dbusmenu clicks), `linux-wayland-close-check.sh`, `linux-kde-close-check.sh`, `human-qa.py` B52 | **None built in**, verified: no tray, `NSStatusBar`, `Shell_NotifyIcon` or SNI code in any gpui crate, and Zed does not depend on `tray-icon`. Keep `tray-icon`. It needs a Win32 message loop on the thread that created it (GPUI's main thread has one) and a running `NSApplication` on macOS (GPUI provides that). That it works under GPUI's loop is to verify in Phase 1. ksni runs its own thread and is independent of the loop |
| 12 | OS notifications (macOS, Linux) | "Download complete" with an Open action. Posted from a thread of its own, because `show` blocks for about 2 s on macOS | `src/platform.rs` (`notify`, `notify_file`, `say`) | `notify-rust 4` (not on Windows) | macOS, Linux | Keep | `tests/window.rs` (fake platform); `human-qa.py` | Built in: `cx.show_system_notification` (UNUserNotificationCenter, Windows toasts, notify-rust on Linux), verified (`gpui/src/app.rs`, `examples/system_notifications.rs`). Not needed: keep dm's own |
| 13 | Windows toasts | Toasts posted by dm itself. Handlers are removed after a click or timeout, which fixed a leak of 3 handles per toast (gaps row 156) | `src/toast.rs` | `windows` WinRT `UI_Notifications`, `Data_Xml_Dom` | Windows | Keep | `tests/handles.rs` | GPUI's Windows toast path is not needed. Keep dm's, which carries the handle-leak fix |
| 14 | Quiet hours | Holds notifications while an app is fullscreen, a game or presentation is running, or Focus Assist is on | `src/quiet.rs` | `SHQueryUserNotificationState` | Windows | Keep | unit tests in `quiet.rs` | Not applicable |
| 15 | Sounds | A chime and an alert, or the user's own sound file | `src/power.rs` | `MessageBeep` and `PlaySoundW` (Windows); `afplay` (macOS); `canberra-gtk-play` and `paplay` (Linux) | all | Keep | `power.rs` unit tests | Not applicable |

## 3. Dialogs, clipboard, drag and drop

| # | Integration | What it does | Files | API today | OS | Coupling | Tests | GPUI |
|---|---|---|---|---|---|---|---|---|
| 16 | File and folder dialogs | Choose a folder, Import and Export (`.json`, `.dmlist`, `.txt`), "Open with…" (an app chooser on macOS and Windows). On Linux it uses the portal, falls back to zenity, and with neither returns "cancelled". `can_choose_folder` and `file_chooser_portal()` check whether a chooser exists | `src/platform.rs` (`choose_folder`, `choose_file`, `choose_app`, `file_chooser_portal`) | `rfd 0.17` (default features: xdg-portal and zenity on Linux) | all | Keep | `tests/window.rs` (fake platform), `human-qa.py` | `prompt_for_paths` and `prompt_for_new_path` are verified. **macOS:** NSOpenPanel and NSSavePanel. **Windows:** IFileOpenDialog and IFileSaveDialog. **Linux:** the ashpd portal **only**; with no portal it returns an error, and there is no zenity fallback (`gpui_linux/src/linux/platform.rs`). **Keep rfd** for its zenity fallback on bare X11. Calling rfd's synchronous dialogs from a GPUI main-thread callback is to verify in Phase 1 (rfd requires the main thread on macOS) |
| 17 | Native alert | dm draws its own questions (Confirm and similar) as dialog windows | `src/dialogs/confirm.rs`, `asks.rs` | egui | all | Redo (part of the dialogs) | `tests/dialogs_more_cover.rs` | `window.prompt` uses NSAlert (macOS, by name unverified) and TaskDialogIndirect (Windows), verified. On Linux it returns `None` and GPUI draws the prompt in the window. dm should keep its own windows to satisfy B42 |
| 18 | Clipboard write | "Copy address", "Copy details" (contract **B55**), and others | `src/app/ui.rs` (`ctx.copy_text`) | egui-winit (arboard and smithay-clipboard) | all | Redo (one call site) | `tests/window/*` (the harness reads egui output), `human-qa.py` B55 | `cx.write_to_clipboard(ClipboardItem)` is verified for String and Image. The Linux primary selection is available too |
| 19 | Clipboard read and the clipboard watch | The watch polls the clipboard and offers links that look downloadable | `src/platform.rs` `clipboard()`; `shell/app/links.rs` `Clipboard::look`; `src/app/clipboard.rs` | `arboard 3` (polled) | all (on Wayland, arboard needs data-control or XWayland) | Keep | `tests/app_cover.rs`, `tests/window/*` (fake clipboard) | `read_from_clipboard` is verified. There is **no clipboard-change notification** in GPUI, so polling stays. Keep arboard, or poll `cx.read_from_clipboard` on a timer. That one polling from a background thread is safe alongside GPUI's own clipboard owner on X11 is to verify in Phase 1 |
| 20 | Drop in: files (`.torrent`, a text file of links, `.url`, `.webloc`) | A dropped file is read as links (the first 64 KB of a text file), or as a `.torrent` by its path | `src/app/drops.rs` (`dropped_links`), `src/drops.rs` | winit `DroppedFile`; dm's own `IDropTarget` on Windows; swizzled delegate methods on macOS | all | Redo (the event source); parsing is Keep | `linux-drag-check.sh` (X11, xdotool), `linux-gui-check.sh` (a file of links), `linux-wayland-drag-check.sh`, `tests/app_cover.rs`, `human-qa.py` | `FileDropEvent` / `ExternalPaths` exists on all four backends, verified (`gpui/src/interactive.rs`) |
| 21 | Drop in: a **link or text** from a browser | A link dragged from a page is queued as `Dropped::Text`. **macOS:** two winit window-delegate drag methods are swapped at run time to read the URL or text from the pasteboard. **Windows:** winit's `IDropTarget` is replaced on the HWND by dm's, which takes `UniformResourceLocatorW` or text. **X11 and Wayland:** a patched winit takes `text/uri-list`, `_NETSCAPE_URL`, `text/x-moz-url` or UTF-8 text | `src/drops.rs` (868 lines), `vendor/winit` (`x11/dnd.rs`, `wayland/dnd.rs`) | `objc2` swizzle, `windows-sys` OLE, a winit fork | all | Redo | `linux-drag-check.sh` (Firefox `_NETSCAPE_URL`, Chrome `https://` in uri-list), `linux-wayland-drag-check.sh` (GTK source, `vpointer.c`), `human-qa.py` | **Not supported.** Drops are files only: `NSFilenamesPboardType` on macOS, `CF_HDROP` on Windows, `text/uri-list` on Wayland, Xdnd files on X11. Verified. Whether `https://` entries in a Wayland uri-list get through is to verify. dm would need either (a) a patched GPUI fork (as with winit today), or (b) per-OS hooks: on macOS, register extra pasteboard types on GPUI's NSView; on Windows, replace GPUI's `IDropTarget` on the HWND (dm already does this to winit's); on Linux, Xdnd and wl_data_device are owned inside GPUI, so a patch is required |
| 22 | Drag out: a finished file to Finder, Explorer, a mail or a chat | **macOS and Windows:** the `drag` crate starts the OS drag from the native window handle. **X11:** dm's own XDND source (`xdnd.rs`) on its own x11rb connection; it takes the pointer grab from winit through x11-dl. **Wayland:** a patched winit (`dm_request_drag_out`, `dm_take_drag_ended`) on winit's own connection with the press serial | `src/platform.rs` `drag_out`, `src/xdnd.rs`, `vendor/winit`, `src/window.rs` `raw_input_hook` | `drag 2.1`, `x11rb`, `x11-dl`, the winit fork | all | Redo on Linux; Adapt on macOS and Windows | `drag-out-check.py` (macOS, CGEvent), `linux-wayland-drag-check.sh` (both directions), `linux-drag-check.sh` | `ExternalDragPayload::Files` and `start_external_drag` are implemented on **macOS and Wayland only**. Windows and X11 return `false` (trait default), verified. **macOS:** use GPUI's, or keep `drag`. **Windows:** keep `drag`, which takes a raw HWND; that this works is to verify in Phase 1. **Wayland:** GPUI's. **X11:** keep `xdnd.rs`, but it must ungrab GPUI's pointer instead of winit's; how is to verify, since GPUI's X11 connection is private |

## 4. System integration

| # | Integration | What it does | Files | API today | OS | Coupling | Tests | GPUI |
|---|---|---|---|---|---|---|---|---|
| 23 | Start at login | **Linux:** dm's own XDG autostart entry (quoted path, AppImage-aware). **macOS and Windows:** a LaunchAgent plist and the `HKCU\…\Run` key via `auto-launch`. dm refuses paths that will be gone at the next login and takes over a stale entry | `src/autostart.rs`, `src/login.rs`, `src/platform.rs` `set_login_item` | `auto-launch 0.6`, `windows-sys` Registry | all | Keep | `tests/login_cover.rs`, `autostart.rs` unit tests | Not needed from GPUI |
| 24 | Keep awake while downloading | `SetThreadExecutionState(ES_SYSTEM_REQUIRED)` while any row is running | `src/awake.rs` | `windows-sys` Power | Windows (no-op elsewhere) | Keep | unit tests | Not applicable |
| 25 | When done: sleep, hibernate, shut down (contract **B49**) | A cancellable countdown, then the system's own command. **Windows:** `SetSuspendState`, `shutdown`. **macOS:** `pmset`, `osascript`. **Linux:** `systemctl` | `src/power.rs` | `windows-sys`, `std::process` | all | Keep (the countdown window is Redo) | `power.rs` unit tests, `human-qa.py` B49 | Not applicable |
| 26 | Wake timer for the schedule | `CreateWaitableTimerW` and `SetWaitableTimer` | `src/wake.rs` | `windows-sys` Threading | Windows | Keep | unit tests | Not applicable |
| 27 | Dock: reopen on click | `applicationShouldHandleReopen:` is added at run time to **winit's** application delegate class | `src/dock.rs` | `objc2` (adding a method to a class at run time) | macOS | Redo | `human-qa.py` (macOS) | `cx.on_reopen` is verified, so dock.rs can be deleted |
| 28 | Dock badge or menu | None today: dock.rs handles reopen only | – | – | macOS | – | – | `set_dock_menu` exists, verified (an optional gain). No badge API was found |
| 29 | Taskbar progress | The taskbar button shows green, yellow or red progress | `src/taskbar.rs` | `ITaskbarList3` via `CoCreateInstance`, HWND from `raw-window-handle` | Windows | Adapt (the HWND source) | `windows-modules-check.sh` (compiles only), `human-qa.py` on Windows | No GPUI API found. Keep, with the HWND from GPUI's `HasWindowHandle` |
| 30 | Jump list | "Add URL" and "Open downloads folder" | `src/jumplist.rs` | `windows` COM `ICustomDestinationList`, `IShellLinkW` | Windows | Keep | `windows-modules-check.sh` | GPUI has `update_jump_list` (`gpui_windows/src/destination_list.rs`), verified. Not needed: keep dm's |
| 31 | Magnet links and `.torrent` opened by the system, macOS | Apple events `GURL` and `odoc` handled from the Apple Event Manager. They are installed at `applicationWillFinishLaunching`, after winit sets its delegate (gaps row 105) | `src/opened.rs`, `dm-desktop/macos/Info.plist` (`CFBundleURLSchemes` magnet, `.torrent` document type) | `objc2`, `objc2-foundation` | macOS | Adapt | `human-qa.py` (macOS) | `cx.on_open_urls` is verified, but in practice it is called **on macOS only** (Windows and Linux store it and never call it). Whether it receives `odoc` file opens as well as URLs is to verify in Phase 1. Otherwise keep `opened.rs`, timed against GPUI's `finishLaunching` |
| 32 | Magnet and `.torrent` handler, Windows | `HKCU\Software\Classes\magnet`, a `dm.torrent` ProgId, Default Apps listing, backup and give-back of the previous owner | `shell/register.rs` (`register_magnet_windows`, `torrent_keys`, `default_apps_keys`) | `reg.exe` | Windows | Keep | `tests/magnet_registry.rs` (manual, `DM_REGISTRY_TEST=1`), `register/tests.rs` | `register_url_scheme` is unimplemented on Windows and Linux, verified. Not needed |
| 33 | Magnet and `.torrent` handler, Linux | `.desktop` with `MimeType=x-scheme-handler/magnet;application/x-bittorrent;` and `Exec=… %u`. The link arrives on argv and goes to the running dm | `desktop/packaging/deb.desktop`, `dm.desktop` | packaging | Linux | Keep | `tests/packaging.rs`, `linux-package.sh` | Not needed |
| 34 | Browser native-messaging host | Writes host manifests for Chromium-family browsers and Firefox, and `HKCU` keys on Windows. dm runs as the host when a browser starts it (framed JSON on stdin and stdout, no window) | `shell/register.rs`, `shell/host.rs`, `src/main.rs` (`browser_launch`) | std, `reg.exe` | all | Keep | `dm-shell` tests, `arch-check.sh`, `linux-check.sh`, `human-qa.py` B26–B37 | Not needed; no window is involved |
| 35 | Open file, reveal in folder, open with, trash | **Open:** `open` / `explorer` / `xdg-open` with fallbacks. **Reveal:** select in Finder or Explorer; on Linux, the FileManager1 D-Bus `ShowItems` (`show_item`). **Open with:** `open -a` or the program itself. **Trash:** the Recycle Bin or Trash | `shell/shell.rs` (`open_file`, `reveal_in_folder`, `open_with`), `src/platform.rs` (`show_item` via zbus, `trash`) | `std::process`, `zbus`, `trash 5` | all | Keep | `shell/tests.rs`, `tests/packaging.rs` (xdg-utils), `human-qa.py` | `cx.open_url` and `cx.reveal_path` exist, verified. Not needed |
| 36 | Dial-up / VPN before downloads; firewall hint | RAS dialling; whether the firewall blocks dm | `shell/dialup.rs`, `shell/firewall.rs` | `windows-sys` Rras, WinSock | Windows | Keep | `dm-shell` tests | Not applicable |
| 37 | Global shortcuts | **None.** dm registers no system-wide hotkey (no `RegisterHotKey` or global-hotkey crate) | – | – | – | – | – | GPUI has none either, verified. No gap |
| 38 | App shortcuts and menus | Menu accelerators (F3, Shift-F3, Ctrl+N and others, contract B48), and keyboard navigation of the list and dialogs, all drawn and handled in egui | `src/app/menus.rs`, `src/app/commands.rs`, `src/table/*` | egui input | all | Redo | `tests/window/keyboard.rs`, `human-qa.py` B48 | Actions, `KeyBinding` and keymaps are verified. The menu bar is drawn by dm today. GPUI `set_menus` is native on macOS only (on Windows it is stored, not shown), so dm keeps an in-window menu bar |

## 5. Look: theme, fonts, text, DPI

| # | Integration | What it does | Files | API today | OS | Coupling | Tests | GPUI |
|---|---|---|---|---|---|---|---|---|
| 39 | System dark/light | **macOS and Windows:** from eframe/winit. **Linux:** the portal `color-scheme`, then the GNOME setting, the GTK theme name, `GTK_THEME`, and `kdeglobals` | `src/system_theme.rs`, `src/theme/apply.rs` | eframe (macOS, Windows); `gdbus` (portal) and `gsettings` subprocesses (Linux) | all | Probe: Keep. Apply: Redo | `system_theme.rs` unit tests | `window_appearance` and `observe_window_appearance` are verified. **macOS:** NSAppearance. **Windows:** settings plus `WM_SETTINGCHANGE`. **Linux:** the portal `color-scheme` only. Keep dm's Linux fallbacks for desktops with no portal |
| 40 | High Contrast | Draws in the system colours when a Windows High Contrast theme is on | `src/contrast.rs` | `windows-sys` Accessibility, `GetSysColor` | Windows | Probe: Keep. Apply: Redo | unit tests; contract B45 (the high-contrast look) | No GPUI API found. Keep the probe |
| 41 | Accent colour | Windows' 7-shade palette; NSColor `controlAccentColor` on macOS; the portal `accent-color` on Linux | `src/accent.rs` | `windows-sys`, `objc2-app-kit`, zbus/portal | all | Probe: Keep. Apply: Redo | unit tests | No GPUI API found. Keep |
| 42 | Reduce motion | Windows `SPI_GETCLIENTAREAANIMATION`, macOS `accessibilityDisplayShouldReduceMotion`, GNOME `enable-animations` | `src/motion.rs` | `windows-sys`, `objc2-app-kit`, `gsettings` | all | Probe: Keep. Apply: Redo | unit tests | `cx.reduce_motion()` and `set_reduce_motion` exist, but the **app sets them**; GPUI does not read the OS setting (`gpui/src/app.rs`), verified. Keep the probe and feed `set_reduce_motion` |
| 43 | Regional formats | Decimal mark, date order, 12/24-hour clock. **macOS:** `defaults` `AppleLocale`. **Linux:** `LC_NUMERIC` / `LC_TIME`. **Windows:** the locale APIs | `src/region.rs`; `sys-locale` for the UI language | `sys-locale`, subprocess | all | Keep | unit tests, `tests/i18n.rs` | Not applicable |
| 44 | Bundled fonts and icon font | Inter (regular and semibold), Selawik, `FluentSystemIcons-dm.ttf`, and egui's monochrome emoji font | `src/theme/fonts.rs`, `dm-desktop/fonts/`, `src/icons.rs` | egui `FontDefinitions` | all | Redo | `tests/i18n.rs` (every translation drawable), B25 | `add_fonts` (`PlatformTextSystem`) takes embedded font bytes, verified. Emoji fallback per OS is to verify in Phase 1 |
| 45 | CJK, Arabic, Hebrew and Thai via system fonts | Each script is loaded lazily from system fonts: fixed paths on macOS and Windows, `fc-match` with the script's language on Linux. Files are memory-mapped with `memmap2` (a CJK collection is 20 MB). Linux packages depend on or recommend Noto CJK | `src/theme/scripts.rs`, `desktop/packaging/nfpm.yaml`, `PKGBUILD` | `memmap2`, `fc-match` | all | Redo (GPUI loads system fonts itself) | `tests/i18n.rs`; CI installs `fonts-noto-cjk` (`desktop.yml`) | Text is shaped by CoreText (macOS), DirectWrite (Windows) and cosmic-text 0.19 (Linux), all verified, and these use system fallback. `FallbackFontClass` and `font_fallbacks` exist. CJK and emoji fallback quality per OS is to verify in Phase 1, so `scripts.rs` may become unnecessary |
| 46 | Bidi / RTL | `bidi.rs` reorders Arabic and Hebrew names into visual order with `unicode-bidi`, because egui does not. The screen reader still gets the stored order | `src/bidi.rs` | `unicode-bidi 0.3` | all | Keep the algorithm; how it is applied depends on GPUI | `bidi.rs` tests `right_to_left_words_are_drawn_in_reading_order` (parity row "Arabic and Hebrew names…") | **Incomplete in GPUI.** The source says "Workaround for the lack of complete UAX #9 support" (`gpui/src/text_system/line_layout.rs`), DirectWrite is called with `bidiLevel: 0`, and issue #31102 (RTL input and rendering) is open. All verified. Keep `bidi.rs` and verify in Phase 1 whether pre-reordered text stays correct after GPUI's shaper |
| 47 | Text size / UI scale | The user's text size multiplies egui's `pixels_per_point`. The minimum window size is divided by it | `src/theme/apply.rs`, `src/app/mod.rs`, `src/main.rs` | egui zoom | all | Redo | `tests/window/layout.rs`, B25 | GPUI has `window.rem_size()` and `set_rem_size`, verified. The mapping is to work out in Phase 1 |
| 48 | DPI | **Windows:** the manifest sets PerMonitorV2. **Elsewhere:** winit's scale factor | `dm-desktop/dm.manifest`, `build.rs` (`embed-resource`) | winit | all | Keep the manifest; scale handling is Redo | `human-qa.py` | **Windows:** PerMonitorV2 plus `WM_DPICHANGED`. **Wayland:** `wp_fractional_scale_v1`. **X11:** `GPUI_X11_SCALE_FACTOR`, then `Xft.dpi`, then RandR. All verified. GPUI ships its own `gpui.manifest.xml`; which application manifest wins, or how to merge dm's (icon and version resource, `requestedExecutionLevel`) with it, is to verify in Phase 1 |
| 49 | IME / text input | egui's `TextEdit` with winit IME (Add URL, Find, Options fields) | egui widgets | egui/winit | all | Redo | `tests/window/*` (typing), `ui.type` | `EntityInputHandler` exists, but there is **no ready-made text field** in GPUI's elements. IME is verified on each backend: NSTextInputClient, IMM, `zwp_text_input_v3`, XIM. Text fields must come from a widget library (gpui-component, now `longbridge/gpui-kit`, Apache-2.0, pinned to `gpui-pre`) or be built from `examples/input.rs` |

## 6. Accessibility

| # | Integration | What it does | Files | API today | OS | Coupling | Tests | GPUI |
|---|---|---|---|---|---|---|---|---|
| 50 | Screen-reader tree, main window | egui widgets give AccessKit nodes with a role and label automatically | all of `src/` (egui widgets), eframe `accesskit` | AccessKit through eframe | all | Redo | `tests/window/commands.rs` `every_control_has_a_name_a_screen_reader_can_say`; the whole kittest suite queries by AccessKit role and label; parity row "Every control has a name a screen reader says" | AccessKit support was merged on 2026-05-27 (PR #56065), verified: `accesskit_macos` 0.26, `accesskit_windows` 0.34, `accesskit_unix` 0.22, on X11 and Wayland. It is **opt-in per element**: an element needs `.id()` and `.role()`, "nodes with no role are not reported" (`gpui/src/_accessibility.rs`). Every dm control must be given a role and label by hand, or by a widget library that does it (to verify for gpui-kit). Known open issues: #66690 (cached views drop nodes), #66729 (wrong X11 AT-SPI coordinates), #66691 (macOS reports the window as focused), #41138 (Zed on Windows lacks screen-reader support) |
| 51 | Screen readers reach each dialog window | eframe was patched to give each window its own AccessKit adapter, so Orca, NVDA/Narrator and VoiceOver reach dialogs (gaps row 160) | `vendor/eframe` (`ADAPTER_PER_WINDOW`, `glow_integration.rs`) | patched eframe | all | Redo (the patch is no longer needed) | `linux-gui-check.sh` (AT-SPI finds Add URL's OK and Cancel; `DM_ORCA=1` runs real Orca), `windows-a11y-check.py` (UIA, 5 of 5 dialogs), `src/a11y_tree.rs` with `ui.accessibility` (VoiceOver's view on macOS) | `a11y_init` is called for every window inside `Window::new` (`gpui/src/window.rs`), so each window has its own adapter, verified. Whether NVDA, Orca and VoiceOver actually read a GPUI dialog end to end is to verify in Phase 1 (see #41138, #66729) |
| 52 | macOS a11y self-read | Walks each NSWindow's accessibility children from inside dm, for `ui.accessibility` | `src/a11y_tree.rs` | `objc2`, `objc2-app-kit` | macOS | Keep (works on any NSWindow) | `tests/control_cover.rs` `the_screen_reader_tree_is_read_only_where_it_can_be` | Not needed from GPUI |

## 7. Rendering and GPU

| # | Integration | What it does | Files | API today | OS | Coupling | Tests | GPUI |
|---|---|---|---|---|---|---|---|---|
| 53 | Renderer | OpenGL through `glow` on every OS: EGL or GLX on Linux, WGL on Windows, CGL on macOS. The window and its GL context cost about 40 MB (`hidden.rs`) | eframe `glow` | OpenGL 2+ / GLES | all | Redo | every GUI script; `memory-check.py` (Windows working set 64–76 MB, macOS 112 MB resident) | **macOS:** Metal. **Windows:** Direct3D 11 at feature level 10_1 or higher, through DirectComposition. **Linux:** wgpu with `VULKAN | GL` (the `gles` feature), so an OpenGL/GLES fallback exists. All verified (`gpui_wgpu/src/wgpu_context.rs` `instance()`; Linux features `["gles","vulkan",…]`). Adapters are ranked discrete, integrated, other, virtual, then CPU. **CPU adapters (lavapipe, llvmpipe) are allowed at first open** and refused only on device-loss recovery. Whether Windows falls back to WARP with no GPU is to verify in Phase 1. GPUI's wgpu is a **git rev** of wgpu, not a release |
| 54 | Virtual-display tests | **X11:** Xvfb with openbox and xdotool, using Mesa `libgl1-mesa-dri` (llvmpipe) for GL. **Wayland:** sway with `WLR_BACKENDS=headless WLR_RENDERER=pixman`, and KWin `--virtual` with `LIBGL_ALWAYS_SOFTWARE=1`. All in Docker with no GPU | `scripts/linux/Dockerfile*`, `wayland_session.sh`, `wayland_close_session.sh`, `kde_session.sh` | Mesa software GL | Linux | Adapt (images and env) | `linux-gui-check.sh`, `linux-human-qa.sh` (64 of 64 rows), `linux-drag-check.sh`, `linux-wayland-*.sh`, `linux-kde-close-check.sh`, `linux-newcomer-check.sh` | Today's GL path runs on llvmpipe. Under GPUI the Docker images need `mesa-vulkan-drivers` (lavapipe) and `libvulkan1`, or must rely on wgpu's GL backend through EGL on llvmpipe. Whether each works under Xvfb (an xlib/xcb surface) and under pixman-rendered sway or KWin virtual (a Wayland surface from a client with no GPU) is to verify in Phase 1. Evidence of the risk: `tests/window/layout.rs` already notes that **wgpu panics finding no adapter on CI's Linux runner**, and catches it. GPUI's Linux `HeadlessConnection` (`gpui_linux/src/linux/headless.rs`) draws nothing, so it cannot replace B25 pixels |
| 55 | Offscreen render for tests | `egui_kittest` with `wgpu` renders snapshots where a GPU exists; `ui.screenshot` writes the live window to PNG | `tests/window/layout.rs`, `src/control.rs` | egui_kittest, eframe screenshot | all | Redo | `tests/window/layout.rs`, `human-qa.py` B25 | `TestAppContext` and `VisualTestContext` run with no renderer, verified. `VisualTestPlatform` renders for real **on macOS only**. Headless Linux rendering arrived in PR #64718; its readiness for this use is to verify in Phase 1. Screenshots of a live window: no GPUI API found, to verify |

## 8. Packaging and runtime libraries

| # | Item | Today | Files | Coupling | Tests | Effect of GPUI |
|---|---|---|---|---|---|---|
| 56 | macOS `.app` and `.dmg` | `cargo packager`, ad-hoc signed (`signingIdentity = "-"`), minimum macOS 11.0, merged `Info.plist` (magnet and `.torrent`) | `dm-desktop/Cargo.toml` `[package.metadata.packager.macos]`, `macos/Info.plist` | Keep | `.github/workflows/desktop.yml` (app, dmg), `human-qa.py` | Metal is present on macOS 11. GPUI's own minimum macOS and `register_url_scheme` (needs macOS 12) are to verify. Zed pins Rust 1.98.1, so check the MSRV against dm's toolchain |
| 57 | Linux `.deb` and AppImage | `cargo packager` on ubuntu-22.04 (x86_64 and arm64). Depends: `libx11-6, libxcursor1, libx11-xcb1, libxi6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-egl1, libegl1, libgl1, xdg-utils, libsecret-tools`. `desktopTemplate = deb.desktop` | `dm-desktop/Cargo.toml` `[package.metadata.packager.deb]`, `desktop.yml` | Adapt | `linux-package.sh`, `linux-human-qa.sh` (DM_DEB), `tests/packaging.rs` | Add `libvulkan1`, recommend `mesa-vulkan-drivers`, and add `libfontconfig1` and `libxcb1` if linked (to verify with `ldd` and `objdump -p` on the Phase 1 binary). Keep `libegl1` for the GLES fallback. `libgl1` and `libwayland-egl1` may become unnecessary (to verify). `libxcursor1` and `libxi6` were for winit; GPUI uses x11rb's `xinput` and `cursor` (to verify). The AppImage must not bundle a Vulkan ICD |
| 58 | `.rpm` | nfpm. Requires sonames `libX11`, `libXcursor`, `libX11-xcb`, `libXi`, `libxkbcommon(-x11)`, `libwayland-client`, `libwayland-egl`, `libEGL`, `libGL`, `xdg-utils`, `secret-tool`; Noto CJK suggested | `desktop/packaging/nfpm.yaml`, `scripts/linux/nfpm.sh` | Adapt | `rpm-check.sh` | The same list change as the `.deb`, plus `libvulkan.so.1` |
| 59 | Arch PKGBUILD | Builds from source. `depends=(hicolor-icon-theme libx11 libxcursor libxi libxkbcommon-x11 wayland libglvnd xdg-utils libsecret)`, optdepends `noto-fonts-cjk` | `desktop/packaging/PKGBUILD` | Adapt | `arch-check.sh` | Add `vulkan-icd-loader`, and optionally `vulkan-driver`. GPUI's git dependencies (wgpu rev, `zed-font-kit`, `xim-rs`) need network at `prepare()` time; `cargo fetch --locked` handles that |
| 60 | Windows NSIS installer and portable zip | `cargo packager --formats nsis`; the uninstaller runs `--unregister` first; the portable zip has a `portable` marker file. `build.rs` embeds the icon, version info and `dm.manifest` | `dm-desktop/Cargo.toml` `[package.metadata.packager.nsis]`, `desktop.yml` "The portable zip", `build.rs` | Keep | `portable-check.py`, `windows-check.sh`, `windows-build.sh`, `windows-modules-check.sh`, `.github/workflows/windows-check.yml` | D3D11 and DirectComposition ship with Windows 10 and later. The manifest merge is item 48. DirectX on Windows VMs and RDP with no GPU is to verify |
| 61 | CI | `desktop.yml`: test matrix on ubuntu-latest, windows-latest and macos-latest (clippy, nextest), with `fonts-noto-cjk` installed; packaging matrix; `release.yml`; `qa.yml` | `.github/workflows/` | Adapt | – | The Linux runner has no GPU. Install lavapipe there, or keep window tests on `TestAppContext` (no renderer). Add GPUI's build dependencies (`libxkbcommon-dev`, `libwayland-dev`, `libx11-xcb-dev`, `libfontconfig-dev`, `libvulkan-dev`; to verify against `script/linux`) |

## 9. Test infrastructure that the migration replaces

This is not an OS integration, but it decides how every row above is proven,
and today all of it is bound to egui:

* **`tests/` (485 tests).** They drive the window through `egui_kittest::Harness`
  and query egui's AccessKit tree by role and label (`tests/window/support.rs`).
  The equivalent in GPUI is `TestAppContext` or `VisualTestContext`, with
  simulated input. Querying by a11y label has no ready helper in GPUI; that
  is to verify in Phase 1, and dm may have to build one over its own a11y
  nodes.
* **Control protocol `ui.*` (`src/app/human.rs`, `src/control.rs`).** It is
  used by `human-qa.py` (contract rows), `linux/gui.py`, `linux-human-qa.sh`,
  `windows-a11y-check.py`, `ui-shot.py` and the MCP client. It reads "the
  words on screen" from egui's output and injects egui input into each
  window. It must be re-implemented on GPUI, through dispatched actions,
  simulated `PlatformInput`, and the a11y tree.
* **Contract B25.** "Each looks as in `docs/behaviour-baseline/` … no more
  than 1% of pixels differ." GPUI's text rasteriser and anti-aliasing differ
  from egui's, so B25 will fail on a faithful port. The contract says a
  re-take of the baseline needs the maintainer's approval, in words, in the
  same commit.

## 10. Ranked blockers and risks

Highest first. Each comes with a fallback that keeps dm shippable.

1. **Screen readers reaching every dialog (items 50 and 51).** The parity row
   and gaps row 160 hold dm to Orca, UIA/Narrator and VoiceOver reaching each
   dialog's controls. GPUI's AccessKit is new (2026-05), opt-in per element,
   and has open bugs on X11 coordinates, focus on macOS and cached views.
   Zed itself still lists Windows screen-reader support as missing (#41138).
   It is absent from crates.io `gpui` 0.2.2.
   *Fallback:* a distribution that has AccessKit (upstream git or
   `gpui-pre`). Give every control a role and label through one dm wrapper
   component. Run `windows-a11y-check.py` and `DM_ORCA=1
   linux-gui-check.sh` against the first GPUI dialog in Phase 1, before
   porting the rest. If it fails, fix or patch GPUI's adapters, as eframe
   was patched; there is no toolkit-independent fallback for this.
2. **The test and QA harness is all egui (section 9).** 485 kittest tests,
   the `ui.*` control protocol behind `human-qa.py` and every Linux GUI
   script, and the B25 pixel baseline all have to be rebuilt before any
   contract row can be shown to pass. Rebuilding the baseline needs
   maintainer approval.
   *Fallback:* port `ui.*` first, so `human-qa.py` keeps working unchanged.
   Run egui and GPUI side by side behind a feature flag until the full gate
   passes. Ask the maintainer early to approve re-taking B25.
3. **Rendering without a GPU in Docker and CI (items 53–55).** Today it is
   OpenGL on llvmpipe, which every Xvfb, sway-pixman and KWin-virtual check
   relies on. GPUI needs Vulkan or GLES through wgpu on Linux, and wgpu
   already finds no adapter on CI's Linux runner (`tests/window/layout.rs`).
   On Windows it is D3D11, and WARP is unverified. Users on old or remote
   machines with no GPU are affected too.
   *Fallback:* add `mesa-vulkan-drivers` and `libvulkan1` to every
   `scripts/linux/Dockerfile*` and CI, or force wgpu's GL backend on
   llvmpipe. Prove both under Xvfb, sway headless and KWin virtual in
   Phase 1. Keep `TestAppContext` (no renderer) for Rust tests.
4. **Dropping links and text in (item 21), and dragging out on Windows and
   X11 (item 22).** GPUI accepts dropped files only, and starts external
   drags on macOS and Wayland only. dm reaches the same ground today only
   through a winit fork, a winit-delegate method swap and a replaced
   `IDropTarget`.
   *Fallback:* keep the `drag` crate (macOS, Windows), which takes any raw
   window handle. Keep `xdnd.rs` for X11. Port the Windows `IDropTarget`
   replacement to GPUI's HWND. On macOS add pasteboard types to GPUI's view.
   For Linux link drops, carry a small patch of `gpui_linux`, as `vendor/`
   does for winit today, and record it in `vendor/README.md`.
5. **Dialogs as movable, modal OS windows on macOS, and hiding the main
   window (items 2, 6, 7).** On macOS GPUI's `WindowKind::Dialog` is a sheet
   (`beginSheet`), which breaks B42's "movable anywhere". There is no
   per-window hide or show, which close-to-tray uses. Always-on-top for the
   drop target is not portable.
   *Fallback:* on macOS, open dialogs as `Normal` or `Floating` windows and
   do modality in dm (block input to the main window, bring the dialog
   forward on click). Implement hide as close-and-reopen with saved bounds.
   Use `PopUp` for the drop target on macOS and Windows, and test it on
   Linux.
6. **System tray (item 11).** GPUI has none (verified).
   *Fallback:* **keep `tray-icon`** (ksni on Linux) as it is. Only its waker
   changes from `egui::Context` to a GPUI foreground task or channel.
   Verify in Phase 1 that its Win32 and NSStatusItem event handling runs
   under GPUI's main loop.
7. **Linux file chooser with no portal (item 16).** GPUI's Linux file
   dialogs are portal-only and error out without one.
   *Fallback:* **keep `rfd`**, with its portal then zenity order and dm's
   `can_choose_folder` check, for every OS.
8. **RTL and bidi text (item 46).** GPUI's UAX #9 support is incomplete
   (in-source comment, issue #31102).
   *Fallback:* **keep `bidi.rs`** reordering, and check
   `right_to_left_words_are_drawn_in_reading_order` against GPUI's shaper.
9. **Text input widgets (item 49).** GPUI ships no text field.
   *Fallback:* `longbridge/gpui-kit` (Apache-2.0). It is tied to the
   `gpui-pre` version it pins, which constrains the distribution choice in
   GPUI_DEPENDENCY_DECISION.md. Otherwise, a dm text field built on
   `EntityInputHandler` following `examples/input.rs`.
10. **Supply chain and packaging (items 56–61).** Upstream GPUI is
    unpublished, at `publish = false`. It pulls wgpu, font-kit and xim
    from git revs, is edition 2024, and pins a recent Rust (1.98.1). Linux
    package dependencies change (add Vulkan, possibly drop GL). GPUI's
    Windows manifest has to be merged with `dm.manifest`.
    *Fallback:* a published snapshot (`gpui-pre`) or a pinned git rev with
    `cargo vendor`. Regenerate the package dependency lists from `ldd` of
    the Phase 1 binary, and extend `tests/packaging.rs` to hold them.
11. **macOS open events (item 31).** GPUI's `on_open_urls` is macOS-only
    (fine), but whether it receives `.torrent` opens (`odoc`) is
    unverified.
    *Fallback:* keep `opened.rs`'s Apple Event handlers.
    **Dock reopen (item 27)** is a gain: `cx.on_reopen` replaces `dock.rs`.
12. **Clipboard (items 18 and 19).** There is no change notification in
    GPUI, which is fine because the watch polls today.
    *Fallback:* **keep `arboard`** for the watch's reads. Writes can use
    `cx.write_to_clipboard`.

**Can be kept unchanged (toolkit-independent):** single instance, the
native-messaging host and its registration, magnet and `.torrent` handler
registration (Windows registry, Linux `.desktop`), start at login
(`autostart.rs`, `login.rs`, `auto-launch`), `awake.rs`, `power.rs`,
`wake.rs`, `quiet.rs`, `toast.rs`, `notify-rust`, `jumplist.rs`, `trash`,
open, reveal and open-with (`dm-shell` `shell.rs`), the theme, accent,
contrast, motion and region probes, `signals.rs`, `a11y_tree.rs`, dial-up
and firewall.

**Need only a new HWND or NSView source** (GPUI's `Window` implements
`HasWindowHandle`): `taskbar.rs`, `chrome.rs` (its caption colour), and the
`drag` crate.

## 11. Phase 1 verification list

Each "to verify in Phase 1" above, as a check to run:

1. **Screen readers.** A GPUI dialog window with buttons, read by
   `windows-a11y-check.py` (UIA), by AT-SPI and Orca on X11 and Wayland, and
   by VoiceOver through `a11y_tree.rs`.
2. **Rendering with no GPU.** A GPUI window under Xvfb with llvmpipe and
   lavapipe, under sway `WLR_RENDERER=pixman`, and under KWin `--virtual`.
   On Windows, one with no GPU (WARP).
3. **Tray under GPUI's loop.** `tray-icon` menu and click events on
   Windows, macOS and Linux (ksni) while GPUI runs the loop.
4. **rfd from inside GPUI.** Synchronous `rfd` calls from a GPUI action on
   macOS (main thread) and on Windows.
5. **Drag out on Windows.** The `drag` crate from GPUI's HWND.
6. **Drag out on X11.** `xdnd.rs` coexisting with GPUI's pointer grab.
7. **Wayland drops.** Whether GPUI keeps `https://` entries in a dropped
   uri-list.
8. **Always-on-top on Linux.** For the drop target window.
9. **X11 `WM_CLASS`** from `app_id`.
10. **`on_open_urls` and `.torrent`.** Whether it receives `odoc` file
    opens.
11. **Fonts.** CJK, emoji, Arabic and Thai fallback with system fonts
    only, on each OS.
12. **Bidi.** `bidi.rs` output through GPUI's shaper.
13. **Windows manifest.** Merging GPUI's `gpui.manifest.xml` with
    `dm.manifest`.
14. **Runtime libraries.** The Linux runtime `.so` list from `ldd` of a
    GPUI build, to feed the `.deb`, `.rpm` and PKGBUILD lists.
15. **Memory at rest.** Against `memory-check.py`'s ceilings (a Metal,
    D3D11 or Vulkan context versus GL).
16. **Hidden start.** Whether `Application::run` with no window allocates a
    GPU context.
17. **Screenshots.** A way to capture a live GPUI window for `ui.screenshot`
    and B25.
