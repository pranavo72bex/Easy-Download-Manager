# The GPUI window's architecture (proposed)

How the GPUI window is built, so that each step (`docs/gaps.md` G2 to G9) has one design to
follow. Status: **proposed, 2026-11-09**. The crate layout and the ownership rules below follow
from the audit and hold whatever GPUI's API turns out to be; everything that names a GPUI type
or call is marked *(verify in G6)* and is confirmed or corrected by the shell against the
pinned version ([GPUI_DEPENDENCY_DECISION.md](GPUI_DEPENDENCY_DECISION.md)).

## 1. Crates

```text
dm-protocol      the extension's words                         (unchanged)
dm-core          domain and engine                             (unchanged)
dm-shell         the application: dm_shell::app                (G3 adds the rules moved from DmApp)
dm-desktop-base  the desktop with no window toolkit (G2):      control server and its auth, mcp, cli,
                 the Desktop Platform, power, autostart, awake, wake, quiet, region, toast,
                 format, details, i18n; the WindowProbe trait (G4)
dm-desktop       the egui window (today's), on dm-desktop-base (kept until G9)
dm-gpui          the GPUI window (G6), on dm-desktop-base      (new)
```

`cargo xtask layers` (desktop/xtask) holds the edges: dm-desktop-base may use dm-core,
dm-protocol and dm-shell and no GUI crate; dm-gpui may use those and dm-desktop-base and GPUI,
never dm-desktop, never egui. Neither window crate may be used by anything.

**Binaries.** Until G9 the GPUI window is a second binary, `dm-gpui`, built and checked beside
`dm-desktop`; `human-qa.py` runs against either by `DM_BINARY`. At the switch (D5) the GPUI crate
takes the binary name `dm-desktop`, because the packages, the browser's native messaging
manifests and the registered URL handlers name that file.

**Modes.** `main` in each window crate calls dm-desktop-base's command-line dispatch first
(native host, warm hand-off, JavaScript host, CLI verbs, MCP, register, the instance lock);
only the window mode differs. Those paths stay byte for byte (CURRENT_UI_ARCHITECTURE.md 1.1).

## 2. Threads and the tick

- **One tokio runtime for the engine**, as today; `Backend` is called from GPUI's foreground
  thread only, never inside a tokio task (CURRENT_UI_ARCHITECTURE.md 8.2).
- **One waker.** `dm_shell::app::changes::subscribe` (and the inbox, tray, signal, control and
  lookup callbacks) send `()` on one channel; a foreground task *(verify in G6: `cx.spawn` on
  the app's entity)* drains it, waits out the 250 ms coalescing that `EVENT_FLOW.md` measured,
  runs **the tick once**, and calls `notify` on the entities whose view changed.
- **The tick** is the order `docs/architecture/ARCHITECTURE.md` gives (after G1 it says what the
  code does), with every rule in `dm_shell::app` after G3: `changes::take`, inbox orders,
  `Session::read`, `schedule_tick`, `ran_dry` and `when_done_fires`, `Hands::next`,
  `countdown_due`, `delete_due`, `look_at_watched`, control requests. The window only reacts to
  the read's `detected` (toasts, sounds, which dialog opens) and draws the snapshot.
- **While no window is shown** (tray only, `--hidden`, before the first show) a timer drives the
  same tick; the main window is made lazily on the first show *(verify in G6 that GPUI's
  application runs with no window open)*.

## 3. State

| State | Owner | Notes |
|---|---|---|
| Rows, queues, settings, schedules, what was detected | `dm_shell::app` (`Session`, `Backend`) | Read as `Snapshot` (rows are `Arc<Entry>`); never copied into a mutable model |
| The rules (questions B39 to B41, renames, hints, saved searches, the update check, the script guard, …) | `dm_shell::app` after G3 | The window calls them; it decides only the words and the dialog |
| View state: selection (by `Id`), sort, place in the side panel, Find's text, which dialogs are open and their fields, focus, scroll | the GPUI app entity `DmGpui` *(verify in G6: `Entity<T>`)* | Presentation only, as the spec's 6.1 lists |
| Window geometry, column widths, side panel width, text size | a store of the window's own, `window.json` in dm's home | egui kept these in its own memory (`window.ron`); read once from there at first start of the GPUI window so nothing is lost |

The selection is a set of `Id`s, never row indices, so it survives sorting, insertion and
removal (spec 8, slice 2).

## 4. Views

```text
DmGpui (app entity: Session, Backend handle, view state, the waker task)
├── MainWindow (Render)
│   ├── MenuBar (Tasks, File, Downloads, View, Help; from dm_shell::app::commands)
│   ├── Toolbar (from the same commands, enabled by commands::applies)
│   ├── SidePanel (categories with counts, Unfinished, Finished, queues)
│   ├── Banners ("N downloads didn't finish", "Add dm to your browser")
│   ├── DownloadTable (virtualized list keyed by Id; columns, sort, hover actions, context menu)
│   └── StatusBar
└── one OS window per dialog (B42): AddUrl, DownloadDetails, Options (tabs), Scheduler,
    SpeedLimiter, Grabber, Properties, Progress, DownloadComplete, the questions (B39 to B41,
    B50), Playlist, Links, ZipContents, About, confirmations
```

Names follow EGUI_FEATURE_INVENTORY.md's "Target GPUI view" column. Each dialog is a window of
its own, opened over the middle of the main window and modal where the egui one is: the main
window takes no input while a modal dialog is open and a click on it raises the dialog
(`dialogs/apart.rs` today). GPUI's macOS dialog kind is a sheet, which cannot be moved outside
the main window (B42), so dialogs are normal or floating windows with dm's own modality
*(verify in G6)*.

**As built (G7, slice 4, 2026-11-10).** `dm-gpui/src/addurl.rs` (Add URL) and `details.rs`
(Download details) are windows of their own, each its view inside gpui-component's `Root` (its
text fields need it), as tall as what they hold (`fit.rs`), built from the system's measures
(`native.rs`: text sizes, control heights, margins, the form's label column, the order of
buttons); `question.rs` asks the questions as macOS sheets, queued by the main view
(`Downloads::sheet`) and opened on the main window once it is in front; `popup.rs` is the pop-up
button whose click opens the system's own menu
(`NativeMenu`), its choice coming back as a `Pick` action to the window's view. What each dialog
offers and does is dm's, in dm-desktop-base for both windows (`add_url`, `file_info`, `asks`).
A dialog that needs the main view holds a `WeakEntity<Downloads>`; a task that updates the main
view runs in the main window by its async context, not by `WeakEntity::update_in`, which GPUI runs
in whichever window last drew the entity (`docs/decisions.md`, 2026-11-10).

**The table** is GPUI's virtualized list *(verify in G6: `uniform_list`)*, drawing only the
visible rows; a row's cells are formatted when drawn, from the snapshot's `Arc<Entry>`; a
progress tick notifies only the table, not the window.

**Commands** come from `dm_shell::app::commands`: each becomes a GPUI action with the
registry's key binding *(verify in G6: `actions!`, `KeyBinding`)*; menus, toolbar and context
menus are built from the registry, enabled by `commands::applies`, carried out by
`commands::verb` or, for those that open a dialog, by the window.

**Theme.** Today's tokens (`theme/`, light, dark, High Contrast, accent, reduced motion) become
one `Theme` value the views read; `gpui-component`'s theme is set from it. Fonts: GPUI shapes
text itself, so `bidi.rs` and `theme/scripts.rs` may not be needed *(verify in G6 with Arabic,
Hebrew, Chinese, Japanese and Korean names)*. Wording: `i18n` from dm-desktop-base, unchanged.

## 5. The control port and QA

`human-qa.py` and MCP read the window through the control port's `ui.*` methods. G4 makes them
a trait in dm-desktop-base, `WindowProbe`, with the JSON fixed by tests; each window implements
it:

| Method | The GPUI window answers it from |
|---|---|
| `ui.texts` (visible words with their rectangles and window) | A list each view fills while it is laid out: every text it draws, with its bounds and window *(verify in G6 that bounds are available after layout)*; the accessibility tree is the fallback |
| `ui.click`, `ui.type`, `ui.key`, `ui.scroll` | Synthetic input dispatched to the window with focus, one step per drawn frame as today *(verify in G6: dispatching platform input to a live window)*; the script guard (`by_script`, G3) is told when each step is delivered |
| `ui.inspect`, `ui.select`, `ui.place` | The view state of section 3 |
| `ui.windows`, `ui.window`, `ui.move_window` | GPUI's window handles and bounds |
| `ui.screenshot` | A capture of the main window *(verify in G6)*; `human-qa.py` pictures dialog windows from the screen itself |
| `ui.accessibility` | The window's AccessKit tree *(verify in G6)* |
| `commands.list`, `command.run` | The command registry, as section 4 |

**Headless** (`--headless`) keeps today's egui `DmApp` until G9 (decisions.md 2026-11-08 fixes
its answers). At the switch, headless becomes dm-desktop-base's tick with a `WindowProbe` over
the view state alone, so its `command.run` and `ui.inspect` answers stay the same.

## 6. Platform pieces

From PLATFORM_GAP_ANALYSIS.md: kept as they are (toolkit-free): `tray-icon` for the tray,
`notify-rust` and the Windows toasts, `rfd` for file dialogs (GPUI's Linux ones need the
portal), `arboard` for the clipboard watch, autostart, power, the URL handlers; adapted to
GPUI's window handle: taskbar progress, the jump list, the Dock, Windows title-bar colours,
drag-out (`drag` crate on macOS and Windows, dm's own X11 and Wayland code); redone: drops of
links and text (GPUI takes files only), the dialogs' placement on X11, and each dialog window's
accessibility adapter if GPUI does not give one per window.

## 7. Tests

The window's tests are written on GPUI's test context *(verify in G6: `TestAppContext`, and
whether it can query the accessibility tree)*; what they assert about the application moves to
dm-shell in G3. [GPUI_TEST_PLAN.md](GPUI_TEST_PLAN.md) has the gate, the systems and the
performance comparison.
