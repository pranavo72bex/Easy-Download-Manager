# The egui presentation as it stands (before the GPUI migration)

An audit of `desktop/crates/dm-desktop` on branch `gpui-migration` at
`aea95ae` (2026-11-09), written to plan replacing egui with GPUI while
`dm_shell::app` (`desktop/crates/dm-shell/src/app/`) and `dm-core` stay as
they are. Everything here was read in the code. Nothing was built or run.
Paths are relative to `desktop/crates/` unless they start with `docs/`,
`scripts/` or `desktop/`. A statement marked **(guess)** or **(inferred)**
was not traced to the end.

Starting points were `docs/architecture/ARCHITECTURE.md`, `audit.md`,
`EVENT_FLOW.md`, `MODULE_BOUNDARIES.md` and `DEVELOPMENT_GUIDE.md`. Where
they and the code disagree, section 9 lists it.

---

## 1. Entry point and process model

### 1.1 One binary, many modes

The binary is `dm-desktop/src/main.rs`; the library is `dm_desktop`
(`src/lib.rs`). Some modules are compiled into the **binary only**:
`hidden`, `login`, `platform`, `signals`, `tray`, `window`, plus `dock` and
`opened` on macOS. Integration tests therefore cannot reach them.

`main()` sends each launch down one of these paths, in this order:

| Order | Condition | What runs | Window? | Lock? |
|---|---|---|---|---|
| 1 | `args == [dm_core::javascript::HOST_FLAG]` | `dm_core::javascript::host_main()`: the JS engine's host process | no | no |
| 2 | `args == [host::WARM_FLAG]` | `host::warm_main`: reads a page for the browser panel | no | no |
| 3 | `shell::browser_launch(&args)` (a `chrome-extension://…` or `moz-extension://…` origin, a host manifest path, or `--native-host`) | `host::run(home)`: the **native-messaging host**, framed JSON on stdin and stdout. Refused unless `shell::launcher_known` | no | no |
| 4 | `--version` / `--help` | prints, exits | no | no |
| 5 | `cli::is_command(args[0])` (`list`, `status`, `pause`, `resume`, `cancel`) | `cli::main`: a **client** of the running dm's control port | no | no |
| 6 | `mcp` | `mcp::main`: an **MCP server** on stdin and stdout that relays to the running dm's control port | no | no |
| 7 | `--register` / `--unregister` | `dm_shell::register::from_args`, plus `remove_login_item` and `unregister_the_rest` | no | no |
| 8 | stray words | error, exit 2 | no | no |
| 9 | otherwise | `instance::claim(home)`, the single-instance lock (below) | — | yes |
| 9a | `--headless` | `DmApp::logic` in a 50 ms loop against a stand-in `egui::Context` | none | yes |
| 9b | `--hidden` (`shell::launched_at_login`) | `run_hidden` (`hidden.rs`): a winit loop of dm's own; eframe is made on the first show | lazily | yes |
| 9c | default | `eframe::run_native("dm", options, …)` | yes | yes |

**Single instance.** `instance::claim(&settings::home())` returns one of
three things:

- `Ok(Some(lock))`: this process owns dm. The lock is held until the end of
  `main`.
- `Ok(None)`: another dm runs. This launch calls `shell::hand_off(home,
  &handed, register::ensure_if_claimed)` and exits. The hand-off writes
  orders, links and a show request into the inbox, which the running dm
  watches.
- `Err`: dm prints the reason, shows an `rfd::MessageDialog` (except in
  headless mode) and exits with 1.

The orders `--when-done` and `--new-download` are written to the inbox with
`shell::leave_when_done` and `leave_new_download`. The window reads them back
at each refresh (section 3.3).

**MCP and the CLI** never hold the lock and never start an engine. They read
`control.port` and `control.token` from dm's home and speak the control
protocol over TCP. Several MCP tools depend on window-only methods
(`mcp.rs:157-232`):

- `dm_move` sends `ui.select` and then `command.run file.move_up/down`.
- `dm_pause` and `dm_resume` without ids, `dm_queue`, and the speed presets
  send `command.run`.
- `dm_select`, `dm_inspect`, `dm_screenshot`, `dm_screen`, `dm_click`,
  `dm_type` and `dm_press_key` map one-to-one onto `ui.*` methods.

A new front end must answer all of them the same way, or the MCP tools
break.

**Control port.** It starts when any of these is set:

- `--control <port>`
- `DM_CONTROL`
- `saved.allow_command_line || saved.allow_mcp` (then the port is 0, meaning
  any free port)

If only `allow_mcp` opened it, the port answers MCP and nothing else
(`assistants_only`). `start_control` (`main.rs:104`) does four things:

1. Writes a fresh token with `control::fresh_token`.
2. Calls `control::listen(port, token, to_window, wake)`. That starts std
   threads that accept connections, authenticate them and turn each JSON line
   into a `control::Request` on an `mpsc` channel. The `wake` closure is
   `ctx.request_repaint()`.
3. Calls `control::write_port`.
4. Calls `DmApp::attach_control(receiver)` and
   `restrict_control_to_assistants`.

### 1.2 Startup of the full app (paths 9a–9c)

1. `tokio::runtime::Runtime::new()`: one **multi-threaded** runtime for the
   process.
2. `on_signal(&runtime, wake)` (`signals.rs`) installs SIGTERM, SIGINT and
   SIGHUP handlers (on Windows: the ctrl and logoff events). They set
   `SIGNALLED` and wake the window through `repaint_early`, an
   `Arc<OnceLock<egui::Context>>` that is filled once a context exists.
3. `runtime.block_on(shell::start(&handed))` returns `Started { queue,
   settings, notices }`.
4. `platform: Arc<dyn Platform> = Arc::new(Desktop)` (`platform.rs`). The
   login item is then made to follow `saved.start_on_login`
   (`autostart::at_launch`), except when `DM_HOME` is set.
5. Change fan-out: `dm_shell::app::changes::connect(&queue)`, then
   `changes::subscribe(|| ctx.request_repaint_after(250 ms))`.
6. `runtime.spawn(shell::watch_inbox(queue, on_show, on_handed, on_unregister))`.
   Each callback sets a flag, or pushes onto `handed:
   Arc<Mutex<VecDeque<host::Request>>>`, and requests a repaint.
   `runtime.spawn(queue.run())` starts the engine.
7. `awake::watch(...)` starts a thread that keeps the computer awake while a
   row is `Running`. It calls `handle.block_on(queue.list())`, a deep copy,
   every 5 s.
8. `Backend::new(runtime.handle(), queue, saved, notices, platform)`.
   `wake::watch` starts a Windows wake timer for the schedule. Then
   `backend.handed = handed`.
9. In headless mode, `DmApp::new(backend)` runs with `app.headless = true` in
   a `logic(&ctx); sleep(50 ms)` loop until `quit_asked` or `SIGNALLED`, then
   `backend.flush()`.
10. In window modes, `eframe::NativeOptions` is built (1.3) and a `make`
    closure, called with the real or stand-in context, does the following:
    - sets `repaint_early`;
    - hooks the Dock reopen (macOS);
    - sets `i18n::set(language)`;
    - calls `start_tray(ctx, sender)`;
    - registers the browsers;
    - calls `DmApp::new(backend)`;
    - publishes the Windows jump list;
    - calls `start_control`;
    - returns `Window { app, tray, … }`.
11. `egui_winit::OTHER_WINDOWS_INPUT.set(dialogs::apart::feed)`. This hook
    exists only in the vendored egui-winit (1.4).
12. After the loop ends: `control::closing()` and `control::forget_port`.
    `Window::on_exit` has already called `backend.flush()`.

### 1.3 eframe options

| Option | Value | Where |
|---|---|---|
| Renderer | glow (OpenGL). eframe features: `default_fonts, glow, persistence, wayland, x11, accesskit, clipboard`. No wgpu in the shipped binary; tests use `egui_kittest` with `wgpu` | `dm-desktop/Cargo.toml` |
| Viewport | title `"dm"`, app id `"dm"` (Wayland `StartupWMClass`), inner size 820×500, min size `MIN_WINDOW / 2`. The real min is set per frame by `DmApp::ui`, divided by zoom. Icon from `icons/128x128.png`. `with_transparent(true)` under Mica (Windows) | `main.rs:535-553` |
| Persistence | `persist_window: true`. `persistence_path` is `$DM_HOME/window.ron` when `DM_HOME` is set, otherwise eframe's default for app id `"dm"`. `persist_egui_memory` keeps its default (`true`) and `App::save` is not implemented, so the store holds the window geometry **and egui's `Memory`** | `main.rs:94-101, 554-560` |
| Patched crates | `[patch.crates-io]` for `winit` 0.30.13 (link drops and drag-out on X11 and Wayland), `egui-winit` 0.37 (`OTHER_WINDOWS_INPUT`: control-port input for dialog windows), `eframe` 0.37 (an AccessKit adapter for each window) | `desktop/Cargo.toml`, `desktop/vendor/README.md` |

**What egui's memory persists (inferred, not traced end to end):**

- the table's column widths: `egui_extras` with `serde`, plus
  `insert_persisted` in `table.rs:192-393`;
- the category rail's width (`PanelState`);
- the text size: the `zoom_factor` set by View › Text size
  (`app/menus.rs:343-351`).

None of these is in `Settings`. A GPUI front end needs a store of its own for
them, or they reset on its first launch. Column order, hidden columns and the
sort are in `Settings` (`columns_hidden`, `column_order`, `sort_column`,
`sort_ascending`).

### 1.4 Threads and runtimes

| Thread | Started by | Does |
|---|---|---|
| **Main** (winit event loop) | `eframe::run_native` or `run_hidden` | Everything in `DmApp`: logic, drawing, every `Backend` call. `Backend` methods `runtime.block_on(..)` (55 call sites in `dm-shell/src/app/backend.rs`) |
| tokio workers (one multi-threaded runtime) | `Runtime::new()` in `main` | `queue.run()` (the engine), `shell::watch_inbox`, signal streams. `lookups`, `search`, `update` and `files` jobs run on `backend.runtime` and report back through `Arc<Mutex<Option<T>>>` slots plus a wake callback |
| Engine callbacks | engine threads | `changes::changed()` → each subscriber (the window's is `request_repaint_after(250 ms)`) |
| Control listener | `control::listen` (std threads) | Accepts, authenticates, parses, sends a `Request` on mpsc, waits up to 60 s (`ANSWER_WITHIN`) for the window thread's reply |
| `awake` | `awake::watch` | Polls every 5 s; keeps the system awake |
| `wake` (Windows) | `wake::watch` | Sets a waitable timer for the next scheduled start |
| Tray (Linux) | `tray-icon`'s KSNI D-Bus thread, plus `sni::keep_registered` | Menu events go to `mpsc::Sender<Tray>`, then `ctx.request_repaint()` |
| One per notification | `Platform::notify` (`platform.rs`) | macOS `show` blocks for about 2 s |
| Others | `hang_up_later` (`dm-hang-up`), `open_later` / `reveal_later`, `opened::listen` (macOS Apple events), an Options browser-registration thread (`dialogs/options/tabs.rs:359`) | Each writes its result into a slot that `logic()` reads |

**Rule that a GPUI front end inherits:** `Backend` blocks on the tokio
handle. Calling it from inside a tokio task panics. Call it only from the UI
thread, or from a non-tokio executor, and never from `backend.runtime.spawn`.

### 1.5 The engine while the window is hidden, closed to the tray, or never made

- **Engine:** `queue.run()` is a tokio task. It runs whatever the window
  does.
- **Application tick:** this lives in `DmApp::logic`, so something must keep
  calling it:
  - *Visible:* eframe calls `App::logic` and `App::ui` on each pass.
  - *Hidden to the tray:* `Window::hide` sends `ViewportCommand::Visible(false)`
    and sets `hidden = true`. eframe still calls `App::logic` (vendored
    `eframe/src/native/epi_integration.rs:383-400`, `update_logic_only`), but
    `Window::ui` returns early and calls only `DmApp::while_hidden`, which
    serves the floating drop target. Wakes become 1 s while busy and 30 s
    when idle.
  - *Wayland:* `Visible(false)` has no effect, so `hide` minimizes instead,
    and on tiling compositors (`minimize_is_ignored`) close quits.
  - *Login launch (`--hidden`):* `run_hidden` builds a winit `EventLoop`
    itself, makes `Window` around a stand-in `egui::Context::default()`
    (no fonts, never drawn) and runs `ctx.run_logic(RawInput::default(),
    |ctx| window.tick(ctx))` whenever `due`. A repaint observer on the
    stand-in context turns `request_repaint_after` into `due`, with a 50 ms
    floor. When the window is first asked for (`!window.hidden`), it calls
    `ApplicationHandler::resumed(&mut eframe)`, replays the queued
    `ViewportCommand`s onto the real context, and from then on forwards
    every winit event to eframe. This saves about 40 MB of GL and window
    memory while dm waits in the tray.
  - *Headless:* a plain loop (1.2 step 9).

---

## 2. Window lifecycle

There is no `update` method. eframe 0.37 (vendored) has `App::logic(ctx,
frame)` and `App::ui(ui, frame)`. On a visible pass both run inside one
`run_ui`, `logic` first (`epi_integration.rs:350-363`).

### 2.1 `Window` (main.rs) implements `eframe::App`

| Method | What it does |
|---|---|
| `raw_input_hook` | Appends `DmApp::synthetic_events(ctx)`, one control-port step per painted pass. On Linux it also pushes a pointer-up when an X11 or Wayland drag-out ended (`xdnd::ENDED`, `winit::platform::wayland::dm_take_drag_ended`) |
| `logic` | Notes whether the display is Wayland, then `Window::tick(ctx)` |
| `ui` | If `hidden`: `app.while_hidden`. Otherwise: `app.ui(ui)`, `app.scan_screen(ctx)` (control port), `app.painted()`, `drops::attach(raw handle)`, `taskbar.show(...)`, `chrome::title_bar(...)` when the look changes, and drag-out of `app.drag_out` |
| `clear_color` | Transparent under Mica |
| `on_exit` | `backend.flush()` |

`Window::tick` (`window.rs:163-283`) runs, in order:

1. Drain tray messages (`Tray::Show`, `Quit`, `Run(Command, show)`).
2. `app.show_asked` and `app.hide_asked` (set from inside the app).
3. The `show_asked` atomic (a second launch or the Dock): re-registers the
   browsers, then `show`.
4. `hidden_until`: a login launch opens the window after 10 s if no tray
   host showed up.
5. **The close button.** With a tray, `CancelClose` and then, by
   `applied.on_close`, `"exit"` → `ask_quit`, `"tray"` → `hide`, anything
   else → `dialogs.confirm_close`. With no tray and downloads running,
   `CancelClose` + `ask_quit`.
6. `app.hidden = self.hidden`; **`app.logic(ctx)`**.
7. Tray words relabelled on a language change; the tray tooltip
   (`app.tray_activity()`); the limiter item's tick.
8. `quit_asked || SIGNALLED`: `quitting = true`, `ViewportCommand::Close`.

`show` restores the position saved by `hide`
(`hidden_at`/`hidden_maximized`) and sends `OuterPosition`, `Visible(true)`,
`Minimized(false)`, `Maximized`, `OuterPosition` and `Focus`. Minimizing is
the window manager's; dm only sends `Minimized(true)` for `ui.window
{to:"minimized"}` and on Wayland's "hide".

### 2.2 `DmApp` (app/mod.rs:273-517)

`DmApp` holds about 100 fields; section 4 lists them. Its per-frame entry
points:

- **`DmApp::ui(&mut self, ui)`** (`app/ui.rs:10-258`):
  - welcome check, clipboard copy-out, field menu edits, min window size by
    zoom, AccessKit root label `"dm"`;
  - theme, accent and motion applied when changed (`theme::apply_with_accent`,
    `motion::apply`);
  - **`self.logic(&ctx)` again**;
  - `theme::cover` (loads system fonts for CJK and other scripts in row
    names), `keys`, `take_drops`, `drop_target`;
  - panels: menu bar, toolbar, status, problems, category rail; the central
    card with the awaiting, update, stalled, failed, browser, clipboard and
    filter bars, then `table::show`;
  - `dialogs::show(self, ctx)`, `underline_fields`, `focus_ring`,
    `field_menu`;
  - finally runs each `queued` command.
- **`DmApp::logic(&mut self, ctx)`** (`app/sync.rs:412-510`): the
  application tick (3.3).
- **`DmApp::refresh(&mut self)`** (`app/sync.rs:30-171`): one read and its
  consequences (3.3).

**Observed: `DmApp::logic` runs twice on every painted pass.** It is called
once from `Window::logic → tick` and again from `DmApp::ui`. `refresh` is
guarded by `refresh_plan`, so the list is read at most once per 250 ms. But
`take_handed`, `countdown_tick`, `answer_control` and the rest run twice per
pass. Hidden or headless, it runs once. The cost was not measured. A GPUI
port should call the tick exactly once per wake.

### 2.3 Repaint scheduling

`logic()` ends with `ctx.request_repaint_after(wake)`:

| State | `wake` |
|---|---|
| hidden, something moving or clipboard watched | `HIDDEN_REFRESH` = 1 s |
| hidden, idle | `HIDDEN_IDLE` = 30 s |
| visible, a row Running or Queued, or a countdown running | `REFRESH` = 250 ms |
| visible, clipboard watched | `CLIPBOARD_REFRESH` = 1 s |
| visible, idle | `IDLE_REFRESH` = 30 s |
| visible, a finished torrent being seeded (`dm_core::sharing_any`) | ≤ 1 s |
| a watched folder is set | ≤ `WATCH_EVERY` = 5 s |
| queue changed since the last read (`refresh_plan` second value) | ≤ 250 ms − elapsed + 5 ms |

Other wakes:

- `changes` subscriber: `request_repaint_after(250 ms)`.
- Immediate `request_repaint()` from the inbox, tray, signals, control
  listener, drops, update check and lookups callbacks.
- `searches_running > 0`: every 300 ms.
- Pending synthetic steps: immediate.
- `hidden_until`.

`refresh_plan(changed, since_read)` (`app/mod.rs:67-77`) reads at once when
there was no read yet or 250 ms have passed. Otherwise, if the queue
changed, it asks to look again when 250 ms will have passed.

### 2.4 Exit

The exit triggers are:

- Program › Exit, Cmd+Q and the tray's Quit, all through `ask_quit()`. It
  sets `quit_asked`, or opens `dialogs.confirm_exit` while downloads run (in
  headless mode it always sets `quit_asked`).
- When done's Quit (`Fired::QuitNow` or `Carried::Quit`).
- `command.run quit` (no question).
- SIGTERM and similar.
- An uninstaller's `--unregister`.

After any of them, `Window::tick` sends `ViewportCommand::Close`, eframe
exits, and `on_exit` flushes the queue.

---

## 3. The application API as dm-desktop uses it

### 3.1 Everything dm-desktop imports from `dm_shell::app`

**`use` lines** (`grep -rn "use dm_shell" src`):

| Where | Item |
|---|---|
| `backend.rs:10` | `pub use dm_shell::app::backend::*` (Backend, Platform, chooser_here). The whole file is this shim |
| `power.rs:14` | `pub use dm_shell::app::power::{Report, WhenDone, COUNTDOWN_SECS}` |
| `commands.rs:14` | `pub use dm_shell::app::commands::{Chord, Command, Key, SPEED_HIGH, SPEED_LOW, SPEED_MEDIUM}` |
| `update.rs:13` | `pub use dm_shell::app::update::*` |
| `app/mod.rs:20, 61, 267` | `dm_shell::Settings`; `changes::CHANGED as QUEUE_CHANGED`; `links::Clipboard` |
| `app/links.rs:9` | `pub use dm_shell::app::links::*` |
| `app/rows.rs:29, 43` | `rules::link_expired`; `names::{kind_of, name_of}` |
| `app/commands.rs:7` | `commands::{limited, recheck, verb, when_done, Ran}` |
| `app/watch.rs:17` | `watch::WATCH_EVERY` |
| `app/sync.rs:253` | `handoff::{Asking, Route}` |
| `app/scheduled.rs:104,129` | `dm_shell::dialup::HungUp` |
| `format.rs:106,125` | `names::{host, provisional}`; `dm_shell::lists::shown_url` |
| `dialogs/add.rs:7,155` | `rules::GoOn`; `rules::Dup` |
| `dialogs/progress.rs:7` | `rules::GoOn` |
| `dialogs/mod.rs:21,22,221,490,493` | `settings::SiteLogin`; `Settings`; `lookups::{stream_name, Probed}`; `files::{TakeSlot, ZipSlot}`; `files::Sums` |
| `dialogs/properties.rs:495` | `files::sum_of` |
| `dialogs/series.rs:158,214` | `lists::{series, SeriesError}` |
| `dialogs/about.rs:27`, `dialogs/options/tabs.rs:353` | `register::{Store, self, Owner}` |
| `main.rs:19` | `dm_shell::{host, instance, settings, shell}` |

**Paths used inline** (`grep -rhoE "dm_shell::app::…"`), by module:

- `changes`: `connect`, `subscribe`, `CHANGED`.
- `session`: `Session`, `delete_due`, `when_done_asked`, `Fired::QuitNow`,
  `Carried::{Quit, Asked}`.
- `handoff`.
- `control_api`: `answer`, `refused`, `person_only`, `batch`,
  `speed_limit`, `add_asked`, `listed_addresses`, `screenshot_path`.
- `commands`: `applies`, `Looks`.
- `rules`: `go_on`, `GoOn`, `Dup`, `superseded`, `link_expired`,
  `file_gone`, `already_listed`.
- `names`: `planned_folder`, `name_it_will_get_after`,
  `earlier_of_the_same_name`.
- `lookups`: `probe_later`, `probe_links`, `list_choices_later`,
  `read_playlist_later`, `LinkFound::{Known, Failed}`.
- `files`: `take_from_zip`, `list_zip_file`, `list_zip_at`, `hash_later`,
  `extract_later`, `sum_of`, `Sums`.
- `search`: `crawl_later`, `sort_finds`, `listed`, `remembered`.
- `import`: `import`, `already_listed`.
- `links`: `Clipboard`, `Looked::Take`.
- `update`: `check`.
- `schedule`: `holds_waiting`.
- `watch`: `WATCH_EVERY`.
- `power`, `detect`.

dm-desktop also uses non-`app` parts of dm-shell:

| Module | References |
|---|---|
| `lists` | 21 |
| `settings` | 15 |
| `register` | 15 |
| `dialup` | 11 |
| `host` | 7 |
| `shell` | 2 |
| `firewall` | 2 |
| `home` | 1 |

**Never used by dm-desktop:** `changes::take` and `changes::pending`;
`events::*` (no `session.events.subscribe` anywhere in dm-desktop); and
`Session::before`.

### 3.2 The pieces

| Item | Shape | How dm-desktop uses it |
|---|---|---|
| `changes` | Process-wide statics: `CHANGED: AtomicBool`, `SUBSCRIBERS: Mutex<Vec<Arc<dyn Fn()>>>`. `connect(&Queue)` makes `Queue::on_change` call `changed()`. `subscribe` is for the life of the process (no unsubscribe) | `main.rs:441-446` connects and subscribes the repaint. `refresh()` clears with `QUEUE_CHANGED.store(false, Release)` directly. `logic()` reads it with `load(Acquire)` |
| `Snapshot` | `{ rows: Vec<Arc<Entry>>, retrying: HashMap<Id, Retry>, swarms: HashMap<Id, Swarm>, allowance_spent: bool }`, made by `Backend::snapshot()` | `refresh()` keeps `rows`. `retrying`, `swarms` and `allowance_spent` go into **thread-locals** (`app/rows.rs` `set_retrying`, `set_swarms`, `set_limit_holds`), which the table and status text read while drawing |
| `Session` | `{ detector, dry, watched, pub hands: Hands, limit_in_force, schedule_watch, queue_watches, pub when_done, pub countdown, pub events }`. The presentation owns it and passes it `&Backend` and `&Settings` | Field `DmApp::session`. The dialogs touch `session.countdown` (`dialogs/confirm.rs:473-502`), `session.when_done` (`app/commands.rs:377`, `app/menus.rs:301`) and `session.forget_limit()` (`dialogs/speed_schedule.rs:511`) |
| `Session::read(&Backend, &Settings) -> Read { snapshot, detected: Detected, problems }` | Reads the snapshot, diffs it (`detect::Detector`), publishes typed events, runs `on_complete` for each finished file | Once per `refresh()` |
| `Session::own(id)` | A row added by this presentation is not "started elsewhere" | `add_items`, `lists.rs:53` |
| `Session::schedule_tick(&Backend, &Settings, rows, can_do) -> ScheduleTick { armed, hang_up, problems }` | Slow-hours limit, schedule window opened or closed (arms or disarms When done), retries and program at the window's end | `schedule_hooks()` in `app/scheduled.rs:61`, with `power::available` as `can_do` |
| `Session::ran_dry(rows) -> bool`, `when_done_fires(nobody_sees, now) -> Fired` | When done | `when_queue_dry()` |
| `Session::countdown_due(&Backend, now) -> Option<Result<Carried, String>>` | Carries out the countdown's action through `Platform::power` | `countdown_tick()` |
| `session::delete_due(&Backend, rows, now) -> Option<DeletedDue>` | "Delete file after": trash and remove | `delete_due_files()` |
| `Session::look_at_watched(folder, rows, &Backend) -> Vec<Vec<String>>` | Watched folder | `take_watched()` |
| `session.hands: Hands` → `next(&Backend, &Settings, rows, Asking, now) -> Route` | Hand-off routing (dedupe, fresh link, link lists, duplicates, ask or add) | `take_handed()`, `await_link`, `stop_awaiting_link`, `taken_from_browser`, `awaiting_bar` |
| `events::Events` | `subscribe(Option<Box<dyn Fn()>>) -> Subscription`; `drain() -> Vec<DownloadEvent>` (`Added`, `StateChanged`, `Completed`, `Failed`, `Removed`, `Progress`) | **Not used** by the egui window. Its notifications, sounds and dialogs come from `Detected` |
| `Backend` | `Clone`; `{ pub runtime, pub queue, settings: Arc<Mutex<Settings>>, notices, pub platform: Arc<dyn Platform>, pub handed }`; about 70 blocking methods | Field `DmApp::backend`. Row verbs: `pause`, `resume`, `retry`, `cancel`, `remove_many`, `put_back`, `finished_rows`, `set_url`, `rename`, `move_to`, `relocate`, `shift`, `set_queue`, `redownload`, `start_queue`, `stop_queue`, `add_each`, `settle_early`, … Settings: `settings()`, `set_settings_from(wanted, by_script) -> (Result<Settings>, Kept)`, `remember_urls`, `remember_folder`, `settings_handle`. Reads: `snapshot`, `row`, `queues`, `connections`, `resumable`, `problems`, `allowance_spent`, `client` |
| `commands` | `Command` (`ALL`, `id`, `label`, `from_id`, `Chord`s), `applies(command, targets, rows, find_typed, &impl Looks)`, `verb(command, &Backend, chosen, queue) -> Option<Ran>`, `when_done`, `limited`, `recheck` | `DmApp::enabled` (with `WindowLooks`, which reads the thread-locals and `table::gone`) and `DmApp::run` (`app/commands.rs`). `dm_desktop::commands` keeps `Shown` (translated title, egui `KeyboardShortcut`) and `pressed` |
| `control_api` | `refused`, `person_only`, `answer` (`queue.list`, `downloads.pause/resume/cancel`), `speed_limit`, `add_asked`, `batch`, `screenshot_path` | `app/control.rs` |
| `Platform` (trait, `dm-shell/src/app/platform.rs`) | Required: `set_login_item`, `notify`, `clipboard`, `reveal`, `open`, `chime`, `power`, `choose_app`, `open_with`, `choose_file`. Defaulted: `tick`, `notify_file`, `open_later`, `reveal_later`, `trash`, `play_file`, `alert`, `choose_folder`, `can_choose_folder`, `can_choose_file` | Implemented by `Desktop` in the **binary's** `platform.rs`, using `quiet`, `power`, `autostart`, `toast` (Windows), `notify-rust`, `arboard`, `rfd`, `trash`, `shell::open_file`. It has **no egui dependency**. Test doubles live in `app/scheduled.rs:132` and `tests/window/support.rs` |

### 3.3 The exact per-tick order (as the code runs it)

Each call to `DmApp::logic(ctx)` (`app/sync.rs:412`) does this:

```text
logic(ctx)
 ├─ (read_now, look_again) = refresh_plan(QUEUE_CHANGED.load(), since last read)
 ├─ if read_now → refresh():
 │    1  QUEUE_CHANGED.store(false)                 // the "changes::take" step, done directly
 │    2  take_when_done()      inbox file → session::when_done_asked → run(WhenDone*)
 │    3  take_new_download()   inbox file → show_asked + run(AddUrl)
 │    4  read = session.read(&backend, &applied)    // snapshot, detect, events.publish, on_complete
 │    5  rows = snapshot.rows; rows_gen += 1; place_rows()
 │    6  set_retrying / set_swarms / table::set_superseded / set_limit_holds (thread-locals)
 │    7  saved_sizes()         std::fs::metadata of each finished file; Arc::make_mut where it differs
 │    8  watch_asks() → promote_asks()     name / web page / 404 questions (B39–B41)
 │    9  apply_pending_renames()
 │   10  selected.retain(..); forget_said_about()
 │   11  notifications: platform.notify (failed), notify_file / notify (finished); sounds
 │   12  schedule_hooks(): clear_spent_once_at() [apply_settings], session.schedule_tick(..)
 │   13  when_queue_dry(): session.ran_dry(rows) → queue_done sound → session.when_done_fires(headless)
 │   14  finished → close progress windows, open complete windows
 │   15  expired → dialogs.update_link; started_elsewhere → progress window; added_again → said
 │   16  sizes_told.retain; problems += backend.problems()
 ├─ platform.tick()                  // quiet-hours notes released
 ├─ big_download_hint(); details_hint()   // may apply_settings
 ├─ take_handed()                    // session.hands.next(..) → Route → dialogs / add / said
 ├─ show_asked if a hand-off dialog just opened
 ├─ countdown_tick()                 // session.countdown_due
 ├─ delete_due_files()               // session::delete_due, then refresh() if anything was due
 ├─ mailboxes: opener_said, hung_up, rechecked
 ├─ tick_searches()                  // saved site searches: results in, due ones started
 ├─ take_watched()                   // session.look_at_watched → add
 ├─ check_clipboard() (every 1 s when on)
 ├─ answer_control(ctx)              // see 5.2
 ├─ weekly_update(ctx)               // update::check, About opened on a newer version
 └─ ctx.request_repaint_after(wake)  // 2.3
```

`refresh()` also runs at the end of every `DmApp::run(command)`, and after
`remove_now`, `undo_remove`, `add_items`, `take_fresh_link`,
`delete_due_files`, and `DmApp::new`. Each control-port `command.run` and
each `batch` step is therefore followed by a full read.

---

## 4. State ownership

### 4.1 Presentation-only state (stays with whichever toolkit draws)

| Group | Fields in `DmApp` | egui types? |
|---|---|---|
| View model of the list | `place: Place`, `selected`, `anchor`, `cursor`, `scroll_to`, `sort`, `filter`, `show_categories`, `show_toolbar`, `hidden_columns`, `column_order`, `columns_changed`, `queue_chosen`, `reordering` | no |
| Caches derived from rows | `rows_gen`, `placed`, `haystacks`, `filter_words`, `visible_cache`, `counts_cache`, `queues_cache`, `place_fingerprints`, `place_rules`, `rail_language` | no (`Rc`, `RefCell`) |
| Layout and geometry | `row_rects: HashMap<Id, egui::Rect>`, `list_clip`, `list_bar`, `list_bar_moved`, `list_drawn`, `list_page`, `menu_bottom`, `bars_bottom`, `min_size_zoom`, `focus_ring: Option<egui::Rect>` | **yes** |
| Focus, menus, keyboard | `keyboard`, `focus_before: Option<egui::Id>`, `was_behind`, `popup_was_open`, `open_menu_by_key`, `row_menu_by_key`, `menus: Vec<egui::Id>`, `field_menu`, `field_edits: Vec<egui::Event>`, `field_selection`, `title_search` | **yes** |
| Look | `theme_in_force`, `accent_in_force`, `motion_in_force`, `fonts_ready`, `dark` | no |
| Status line and messages | `said: i18n::Said`, `said_at`, `said_about`, `problems`, `failed_hidden` | no |
| Dialogs | `dialogs: Dialogs` (`dialogs/mod.rs:862-975`): `add_url`, `file_info`, `progress`, `complete`, `properties`, `series`, `playlist`, `clip_links`, `links`, `site_files`, `update_link`, `move_to_queue`, `rename`, `scheduler`, `limiter`, `options`, `about_open`, `tip`, `welcome`, `confirm_remove/outside/cancel/exit/close`, `duplicate`, `name_ask`, `page_ask`, `not_found`, `import_ask`, `list_file`, `skip_offer`, `focus`, `raise`, … | some (`focus_before`, `raise: Vec<egui::Id>`) |
| Control port, egui side | `control` (mpsc receiver), `screen: Vec<ScreenText>`, `synthetic`, `passes`, `delivered_at(_pass)`, `scripted_at`, `mcp_only` | **yes** (`egui::Rect`, `ViewportId`, `egui::Event`) |
| OS hand-outs | `drag_out: Vec<PathBuf>`, `copied`, `show_asked`, `hide_asked`, `quit_asked`, `hidden`, `headless` | no |

Outside `DmApp`, the code keeps more state in module-level statics and
thread-locals:

- `app/rows.rs`: retrying, swarms and limit holds.
- `table/status.rs`: `SUPERSEDED`.
- `i18n.rs`: `NOW`, `CODE` (the language is per thread).
- `drops.rs`: `PENDING`, `WINDOW: OnceLock<egui::Context>`.
- `dialogs/apart.rs`: `Shared` (dialog windows, their screens, queued input,
  focus).
- `xdnd::ENDED`.

### 4.2 Application state still held in dm-desktop

The architecture docs say the window crate still holds only "the wording of
what the Session reports, the dialogs it asks to open, the egui-only control
methods (`ui.*`, and `command.run`'s commands that open a dialog or change
the view)". All of those are present. The code also keeps the following
**rules and state** in `DmApp`. They are egui-free but tied to `DmApp`, so a
GPUI front end would have to write them again or move them into
`dm_shell::app` first.

| What | Where | Why it is application state |
|---|---|---|
| Settings mirror and its coordination | `applied: Settings`; `DmApp::apply_settings` (`app/sync.rs:544-582`): `set_settings_from(wanted, self.by_script())`, the "a script was driving" wording, `i18n::set`, `searches_checked` reset, `place_rows`, clipboard watch on or off | `by_script()` is a **security rule** (see below), and some settings writes happen only here |
| Script guard | `scripted_at`, `by_script()` (3 s after a synthetic step), `run_refused()` (`app/human.rs:60-74`) | Refuses opening or running a file, and changing the on-complete command, schedule program or proxy script, while a control client is acting (gaps row 159 f). It depends on when synthetic egui input was delivered |
| Question detection (B39, B40, B41) | `asks`, `ask_seen`, `asks_primed`, `watch_asks`, `promote_asks` (`app/sync.rs:864-960`) | A diff over reads: a new server name, a page instead of a file, a 404. `detect::Detector` does not do this |
| Renames after "use the address's name" | `pending_renames`, `use_address_name`, `apply_pending_renames` | Renames the file when the row completes |
| Once-only schedule dates | `clear_spent_once_at` (`app/scheduled.rs:34`) | Writes settings each read |
| Dial-up hang-up | `hang_up_later`, `hung_up` slot (Windows) | Carries out `ScheduleTick::hang_up` |
| Saved site searches | `search_results`, `searches_running`, `searches_checked`, `tick_searches`, `run_search`, `add_found` (`app/search.rs`) | Timer, add-for-later into the main queue, and persisting `seen` into settings |
| Weekly update check | `weekly_pending`, `update: Arc<Mutex<Update>>`, `weekly_update` | Due logic plus `update_checked` written to settings |
| One-time hints | `big_download_hint`, `details_hint` | Write `hinted_big` and `hinted_details` |
| Hand-off cancel counter | `cancelled_hand_offs`, `hand_off_cancelled` | Offers to leave a site to the browser after 2 cancels |
| Inbox orders | `take_when_done`, `take_new_download` | Read and delete files in the inbox |
| Adding | `add`, `add_sized`, `add_items`, `last_added`, `sizes_told` | `session.own(id)` marking, opening the progress window for a single add, a partial-failure problem |
| Finished file sizes | `saved`, `saved_sizes` | Reads the disk and rewrites `progress` on the shared row (`Arc::make_mut`) |
| Remove with Undo | `undo_remove`, `remove_finished_with_undo`, `undo_remove` (5 s) | The Undo window is a view matter; the row bookkeeping is not |
| Speed limiter memory | `last_limit` | Passed to `commands::limited` and set by `downloads.speed_limit` |
| When done for the CLI | `take_when_done` maps `WhenDone` to `Command::WhenDone*` and checks `power::available` | — |
| Notifications and sounds | `refresh()` steps 11 and 13, `sound_file` | Wording and choice of sound live in the window |

`dialogs/*` also holds row verbs that are called straight from widgets
(`dialogs/progress.rs`, `add.rs`, `properties.rs`, `row_dialogs.rs`,
`file_info.rs`), and `Dialogs` methods with rules in them
(`dialogs/mod.rs:305-446`: `is_html`, `save_dir`, `folder_of`,
`category_folder`; `answer_duplicate`). Every dialog function takes `app:
&mut DmApp`.

---

## 5. Coupling to egui beyond drawing

### 5.1 Headless runs on a stand-in `egui::Context`

`--headless` and the login launch's hidden phase both run `DmApp` against
`egui::Context::default()`. The context is never drawn and has no fonts.
This was decided on 2026-11-08 (`docs/decisions.md`, "Headless keeps its
window-free DmApp"), so that `command.run` and `ui.*` give the same answers.

The tests use the same pattern: `app.logic(&egui::Context::default())`
(`app/scheduled.rs:260`, many in `tests/`).

In headless mode, `ui.screenshot` is refused, and `ui.texts` returns what
`scan_screen` last stored. `scan_screen` runs only in `Window::ui`, so that is
nothing **(inferred)**.

### 5.2 Control port: the `ui.*` methods and `command.run`

These are all answered on the UI thread by `DmApp::answer_control`
(`app/control.rs:19-62`):

1. `control_api::refused(via_mcp, mcp_only, allow_mcp)`.
2. `ui.screenshot`: `ctx.request_screenshot(..)` and `save_png`, answered
   once the integration has read the frame's pixels.
3. `human_call` (`app/human.rs:186-430`), otherwise `control_call`
   (`app/control.rs:65-217`).

| Method | Source of the answer | egui coupling |
|---|---|---|
| `ui.texts` | `DmApp::screen`, filled by `scan_screen`, which walks **painted text shapes**: `ctx.graphics()` layers, `Shape::Text` galley rows, clipped. It does not read the widget tree. Dialog windows are read through `dialogs::apart::screen_of` | egui epaint internals |
| `ui.click`, `ui.scroll` | Queue a `Step::Pointer { text, nth, … }`. In `synthetic_events` (from `raw_input_hook`), the text is found again (`find_text`: exact, then substring, then "…"-truncated) and `PointerMoved`, `PointerButton` or wheel events are fed, one step per **painted** pass. Dialog windows get theirs through `apart::queue_input` and the vendored `OTHER_WINDOWS_INPUT` | egui events, viewports, vendored egui-winit |
| `ui.type`, `ui.key` | `Step::Events(Vec<egui::Event>)` sent to `apart::typing_target()` | egui key and text events |
| `ui.select {ids}`, `ui.place {place}` | Set `selected` and `place` | none (view state) |
| `ui.inspect` | JSON: `place`, `selected`, `sort`, `categories`, `toolbar`, `dark`, `hidden`, `said`, `problems`, `dialogs` (`Dialogs::open_names`), `rows[{id, name, status, queue, description, url, rect}]` (`rect` from `row_rects`) | `egui::Rect` |
| `ui.windows`, `ui.window {to}`, `ui.move_window` | `apart::placement()`, `pixels_per_point`, `ViewportCommand::{Minimized, OuterPosition}`, `hide_asked`/`show_asked` | egui viewports |
| `ui.accessibility` | `a11y_tree::windows()`: walks **NSAccessibility** on macOS (what AccessKit exposed). Not egui. Refused off macOS | AccessKit through the platform |
| `ui.screenshot` | `ctx.request_screenshot` | egui |
| `commands.list`, `command.run {id}` | `Command::ALL` with `self.enabled(c)`; `person_only`, `enabled`, Quit asks no question, otherwise `self.run(command)` and then `said.english()` | view state (selection), `Dialogs` |
| `queue.list`, `downloads.pause/resume/cancel` | `control_api::answer(&backend, &self.rows, …)` | none |
| `downloads.speed_limit`, `downloads.add`, `batch` | `control_api::speed_limit` + `apply_settings`; `control_api::add_asked` + `self.add`; `control_api::batch(params, \|m, p\| self.control_call(m, p))`. `batch` reaches `control_call` only: **no `ui.click` and the like inside a batch** | none |

`scripts/human-qa.py` (the behaviour contract's QA, 64 rows) drives the
window through these methods: `ui.select`, `ui.key`, `ui.inspect`,
`ui.type`, `ui.click`, `ui.texts` (including the `window: "main" | "dialog"`
field), `ui.place`, `ui.scroll`, `ui.move_window`, `ui.windows`
(`outer` rects × `pixels_per_point` for cropping screen grabs),
`ui.screenshot`, `ui.accessibility`, `command.run`, `queue.list`,
`downloads.add`. It relies on four behaviours:

- words found by their visible text;
- one input step per painted frame;
- dialogs being separate OS windows (B42);
- `ui.inspect().dialogs` naming dialogs by their English titles.

### 5.3 Tests built on egui

`tests/` holds 23,298 lines and 485 `#[test]`s, plus 216 unit tests in `src/`
(49 files).

| Kind | Files | egui dependence |
|---|---|---|
| `egui_kittest::Harness` (with `wgpu` and `snapshot` features; the AccessKit tree queried with `kittest::Queryable`, e.g. `get_by_label`) | `tests/window/*.rs` (dialogs, downloads, keyboard, layout, lists, options, commands, control, speed_schedule, zip), `app_cover.rs`, `app_more_cover.rs`, `control_cover.rs`, `dialog_windows.rs`, `dialogs_cover.rs`, `dialogs_more_cover.rs` | Total: they build `DmApp` and drive egui |
| `DmApp` with a stand-in `Context` | many of the above, `fuzz.rs` | `DmApp` and egui |
| Toolkit-free | `handles.rs`, `login_cover.rs`, `magnet_registry.rs`, `packaging.rs`, `quit.rs`, `second_network.rs`, `i18n.rs` (catalog coverage; 6 egui references for font checks), `window/choices.rs`, `window/asks.rs` | Little or none |

`tests/window/support.rs` is the shared harness: a fake `Platform`, a real
`Queue` against an axum loopback server, and click, type and wait helpers on
`Harness`.

### 5.4 AccessKit

eframe has the `accesskit` feature. The vendored eframe gives each dialog
window its own adapter (gaps row 160, `desktop/vendor/README.md`).
`DmApp::ui` labels the root node `"dm"`. Kittest tests find controls by
their AccessKit labels. `a11y_tree.rs` reads the result back through
NSAccessibility for `ui.accessibility`. The behaviour contract's
screen-reader rows depend on it.

### 5.5 Persistence

See 1.3: window geometry, column widths, rail width and text zoom are in
egui's memory (`window.ron`). Everything else is in `Settings` (dm-shell)
and the queue (dm-core).

### 5.6 Fonts and theme (`theme.rs`, `theme/`, 1,544 lines)

- `theme.rs`: `Theme` (resolved from the setting, the system's dark mode
  through `system_theme::prefers_dark` or `ctx.system_theme()`, and High
  Contrast through `contrast::system`) and `Tokens`, the design tokens as
  `egui::Color32` (`Tokens::get(ctx)` reads them back from egui's style).
- `theme/apply.rs`: writes `egui::Style` and `Visuals`.
- `theme/fonts.rs`: embeds Inter Regular and SemiBold,
  `FluentSystemIcons-dm.ttf` and Selawik (`dm-desktop/fonts/`) as
  `egui::FontData`, with families `semibold`, `numbers` and `bold`.
- `theme/scripts.rs`: `cover(ctx, texts)` finds and **memory-maps a system
  font** for each script the window's text needs (CJK, Arabic, …) and adds
  it to egui's fonts at run time.
- Related modules: `accent.rs` (the system accent as `Color32`),
  `motion.rs` (reduced motion into egui animation time), `bidi.rs`
  (reorders RTL text, which egui does not do), `icons.rs` (toolbar glyphs
  drawn with egui).

The palette values and font files can be reused. The code that applies
them cannot.

### 5.7 The table (`table.rs`, `table/`, 1,734 lines)

`table::show(app, ui)` uses `egui_extras::TableBuilder` and `Column as
Width`, with egui-persisted state (`list-sideways` horizontal scroll,
`list`, `list-bar`, zoom and dragged flags). It draws its own vertical
scroll bar (`list_bar`), hover rows (`table/hover.rs`), cells
(`table/cells.rs`), header menus (`table/menus.rs`), multi-select
(`table/selection.rs`) and status text (`table/status.rs`, including the
`SUPERSEDED` thread-local). The row order comes from
`DmApp::visible_shared()` (`app/places.rs:178`), which is toolkit-free.

### 5.8 i18n (`i18n.rs`, 601 lines + tests)

Catalogs are keyed by English text: `i18n/*.json`, 13 languages and 11,817
lines in all. `t()` and `tf()` look up in a **thread-local** catalog
(`i18n::set`), so words are made on the UI thread. `i18n::Said` holds a
sentence that is re-translated when the language changes. The only egui
item is `pick(ui, now)`, the language menu. The rest can move to a GPUI
crate unchanged. `tests/i18n.rs` and `app::run_time_words()` require every
`t()` string to be present in every catalog.

### 5.9 OS integration that reaches into egui or winit

| Module | Coupling |
|---|---|
| `tray.rs` (binary) | `tray(wake: egui::Context, …)`: the context is only for `request_repaint()` |
| `drops.rs` | macOS and Windows drop targets attach to the raw window handle. The wake is through a stored `egui::Context`. Dropped files arrive as an `egui::DroppedFile` impl |
| Linux link drops | Not in dm-desktop: the **vendored winit** hands a browser's link over as a dropped "path" (X11 and Wayland) |
| `xdnd.rs`, Wayland drag-out | X11 drag-out on dm's own connection plus **winit's display** to release its grab. Wayland drag-out is `winit::platform::wayland::dm_request_drag_out` (vendored) |
| `platform.rs::drag_out` | `drag` crate from `eframe::Frame`'s window handle (macOS and Windows) |
| `chrome.rs`, `taskbar.rs`, `jumplist.rs` | Raw window handle only (Windows) |
| `dialogs/apart.rs` | Each dialog is an `egui::show_viewport_immediate` OS window (B42), with modality, raise, closing and placement kept in a static `Shared` |
| `app/drops.rs` | The floating drop target is another immediate viewport, `"dm-drop-target"` |
| `update.rs` | `check(backend, into, ctx)` only to request a repaint |

---

## 6. Size

`dm-desktop/src`: **35,818 lines** in 95 files. `tests/`: 23,298.
`build.rs`: 75. For comparison, `dm-shell/src/app`: 8,086.

| Area | Lines | Kind |
|---|---|---|
| `dialogs/` (21 files) | 12,426 | Presentation, with dialog drafts and some rules (`state.rs` 1,111, `mod.rs` 1,231, `options/tabs.rs` 1,468, `file_info.rs` 968, `controls.rs` 893, `frame.rs` 829, `progress.rs` 643, `properties.rs` 574, `apart.rs` 572, `speed_schedule.rs` 523, `add.rs` 509, `confirm.rs` 506, `links.rs` 450, `row_dialogs.rs` 320, `site_files.rs` 271, `series.rs` 247, `zip_list.rs` 235, `about.rs` 232, `complete.rs` 225, `playlist.rs` 170, `asks.rs` 101) |
| `app/` (22 files) | 8,031 | Mixed (below) |
| `table.rs` + `table/` | 1,734 | Presentation |
| `theme.rs` + `theme/` | 1,544 | Presentation |
| `format.rs` + tests | 1,175 | Mostly toolkit-free wording (`shortcut` and `key_name` use egui) |
| `i18n.rs` + tests | 1,019 | Toolkit-free except `pick` |
| Binary-only (`main` 666, `platform` 400, `window` 299, `tray` 299, `hidden` 241, `opened` 198, `login` 129, `signals` 84, `dock` 75) | 2,391 | Composition root and OS (`window`, `hidden`, `main` tied to eframe and winit) |
| Protocol and processes (`control` 573, `mcp` 609, `cli` 416) | 1,598 | **Toolkit-free** |
| OS integration (`autostart` 687, `power` 391, `drops` 868, `xdnd` 374, `taskbar` 292, `system_theme` 240, `contrast` 225, `quiet` 210, `toast` 210, `region` 201, `jumplist` 130, `awake` 90, `wake` 68) | 3,986 | Toolkit-free except `drops` (egui wake and `DroppedFile`) and `xdnd` (winit display) |
| Other presentation (`icons` 402, `details` 356, `accent` 327, `commands` 156, `bidi` 152, `chrome` 136, `motion` 128, `a11y_tree` 106, `update` 79, `lib` 62, `backend` 10) | 1,914 | `details` (Copy details text) and `a11y_tree` are toolkit-free |

`app/` by module:

| Module | Lines | egui refs | Role |
|---|---|---|---|
| `sync.rs` | 1,027 | 3 | **Glue**: `refresh`, `logic`, hand-offs, settings, add, asks |
| `mod.rs` | 951 | 14 | `DmApp`, `Place`, `Column`, constants, `refresh_plan` |
| `widgets.rs` | 768 | 129 | Presentation |
| `menus.rs` | 724 | 41 | Presentation (menu bar, toolbar) |
| `rows.rs` | 579 | 0 | Status wording, sort keys, thread-locals (view model) |
| `ui.rs` | 566 | 74 | Presentation (frame layout, keys, focus ring) |
| `human.rs` | 543 | 45 | Control-port input and screen reading (egui-specific) |
| `bars.rs` | 538 | 35 | Presentation (failed, stalled, update, browser bars) |
| `commands.rs` | 431 | 0 | **Glue**: `enabled`, `run` |
| `places.rs` | 349 | 1 | View model: filter, place, sort, `visible_shared`, `targets` |
| `search.rs` | 289 | 0 | **Glue**: saved searches, Find |
| `scheduled.rs` | 268 | 1 | **Glue**: `schedule_hooks`, hang-up |
| `tree.rs` | 222 | 7 | Presentation (category rail) |
| `control.rs` | 218 | 1 | **Glue**: control-port dispatch |
| `drops.rs` | 212 | 18 | Presentation (drop target) |
| `lists.rs` | 87 | 0 | Glue: import and export |
| `files.rs` | 77 | 0 | Glue: open, reveal, extract |
| `links.rs` | 61 | 3 | Dropped links |
| `naming.rs` | 49 | 0 | Glue over `dm_shell::app::names` |
| `clipboard.rs` | 40 | 0 | Glue over `links::Clipboard` |
| `watch.rs` | 32 | 0 | Glue over `Session::look_at_watched` |

**In summary:**

- 29 source files (7,139 lines) contain no egui or eframe reference at all.
  The largest are `autostart`, `mcp`, `rows`, `control`, `app/commands`,
  `cli` and `power`.
- The `DmApp` glue that would have to be rewritten or moved is about 2,500
  lines: `app/sync`, `commands`, `control`, `search`, `scheduled`, `lists`,
  `files`, `naming`, `clipboard` and `watch`.
- Roughly 26,000 lines are egui drawing (dialogs, table, theme, widgets,
  menus, bars) to be redrawn in GPUI.

---

## 7. Event flow (as the code does it)

```text
 engine threads (dm-core)                dm_shell::app                         egui window (main thread)
 ───────────────────────                 ─────────────                         ─────────────────────────
 progress bytes → Slot.entry (no signal)
 state change → Queue::save → on_change ─► changes::changed()
                                            CHANGED = true
                                            each subscriber ──────────────────► ctx.request_repaint_after(250ms)
 inbox watcher (tokio) ── show / handed / quit flags ─────────────────────────► ctx.request_repaint()
 control listener (std threads) ── Request via mpsc ─────────────────────────► ctx.request_repaint()
 tray (menu thread) ── Tray via mpsc ─────────────────────────────────────────► ctx.request_repaint()
 lookups/search/update/files (tokio) ── Arc<Mutex<Option<T>>> slot + wake ───► ctx.request_repaint()

                                                                               eframe pass:
                                                                                raw_input_hook ← synthetic step
                                                                                App::logic → Window::tick
                                                                                  tray msgs, show/hide, close
                                                                                  DmApp::logic ─┐
                                            refresh_plan(CHANGED, since) ◄──────────────────────┘
                                            refresh():
                                              CHANGED.store(false)
                                              Session::read ─► Backend::snapshot (block_on)
                                                ├─ Detector → Detected {finished, failed, expired,
                                                │             started_elsewhere, added_again}
                                                ├─ Events::publish (no subscriber in the window)
                                                └─ on_complete command
                                              ◄── Read ─ window: rows, thread-locals, asks,
                                                         notify/sounds, progress/complete windows
                                              Session::schedule_tick → ScheduleTick
                                              Session::ran_dry → when_done_fires
                                            Hands::next → Route ─────────────► dialogs / add / said
                                            countdown_due, delete_due, look_at_watched
                                            control_api::{refused, answer, batch} ◄─ answer_control
                                                                                  request_repaint_after(wake)
                                                                                App::ui → DmApp::ui
                                                                                  (DmApp::logic again)
                                                                                  panels, table, dialogs::show
                                                                                  queued commands → run → refresh
                                                                                scan_screen, painted
```

---

## 8. What a GPUI front end must implement or replace

### 8.1 Seams it can use as they are

- `dm_shell::app::{backend::Backend, session::Session, changes, snapshot,
  commands, control_api, rules, names, lookups, files, search, update,
  import, links, handoff, schedule, watch, power, events}`.
- `dm_shell::{settings, host, instance, shell, register, lists, dialup}` for
  startup and the browser host.
- `Platform`: the binary's `Desktop` (`platform.rs`) has no egui code. It
  can be moved into a toolkit-free crate or module (it relies on
  `dm_desktop::{quiet, power, autostart, toast}`, also toolkit-free) and
  reused. The exception is `drag_out`, which needs the new window's handle.

### 8.2 What it must provide

| Need | Today | GPUI must |
|---|---|---|
| Process entry and modes | `main.rs` paths 1–9 | Keep every non-window path byte for byte (native host, warm, JS host, CLI, MCP, register, lock and hand-off). `cli`, `mcp`, `control` live in the `dm_desktop` lib, which a GPUI crate may not depend on (`MODULE_BOUNDARIES.md`), so they must move to a shared toolkit-free crate (or dm-shell: sockets are allowed there) or be duplicated |
| Runtime | One tokio multi-threaded runtime; `Backend` blocks on it | Keep tokio for the engine. Call `Backend` only from GPUI's foreground thread (or a non-tokio executor). Never inside a tokio task |
| Wake-ups | `changes::subscribe`, plus inbox, tray, signal, control and lookup callbacks, all to `ctx.request_repaint[_after]` | One thread-safe waker into GPUI's foreground loop (for example a channel drained by a foreground task, then `cx.notify()`) **(guess at GPUI API)**. Keep the 250 ms coalescing for queue changes (`EVENT_FLOW.md`: a repaint per change costs about 50 % more CPU) |
| The tick | `DmApp::logic` + `refresh` (3.3), with `refresh_plan` and the wake table (2.3) | Call it **once** per wake, in the same order. Keep "clear `CHANGED` before reading". Keep `Session` as the one owner of read, detect, schedule, When done, countdown, delete-due, watched folder and hand-offs. Reproduce or first move 4.2's rules (asks, pending renames, once-at clearing, saved searches, weekly update, hints, hand-off cancel counter, inbox orders, saved sizes) |
| Running hidden | eframe `logic` while hidden; `run_hidden` stand-in context before the first show; headless loop | A timer-driven tick that runs with no window (tray-only), a lazy first window for `--hidden`, and a headless mode. Today headless answers `command.run` and `ui.*` from the egui `DmApp`; decisions.md 2026-11-08 forbids changing those answers, so a GPUI headless needs the same view-model answers with no window |
| Close to tray, show, restore, exit | `Window::tick`, `show`/`hide` (position and maximized remembered; Wayland minimize; tiling WMs quit), `on_close` setting, `confirm_close`/`confirm_exit`, `SIGNALLED`, `on_exit` flush | The same behaviours (behaviour-contract rows), through GPUI window APIs **(guess: GPUI's hide and minimize on Wayland and X11 need checking)** |
| Window state | eframe `persist_window` + egui memory (`window.ron`): geometry, column widths, rail width, text zoom | A store of its own, and a decision whether to migrate an existing `window.ron` (RON of eframe's own format) |
| Dialogs as OS windows (B42) | `dialogs/apart.rs` (immediate viewports, modality, raise, placement, close-asks) | Separate GPUI windows with the same modality (the main window takes no input while a modal question is open; a click on it raises the dialog) |
| Platform | `Desktop` | Reuse it (8.1). Wire tray (`tray-icon` with a GPUI waker), Dock reopen, jump list, taskbar progress, Windows title-bar colours (raw handle), Mica |
| Drag and drop | Vendored winit (Linux link drops and Wayland drag-out), `drops.rs` (macOS and Windows drop targets on the raw handle), `xdnd.rs` (uses winit's X11 display) | GPUI does not use winit, so the three winit patches no longer apply. Their behaviour (a browser link dropped on Linux X11 and Wayland; drag-out of finished files on X11 and Wayland) must be rebuilt on GPUI's Linux platform **(guess: needs a look at GPUI's DnD support)** |
| Accessibility | AccessKit through eframe, one adapter per window (vendored eframe); kittest queries; `ui.accessibility` | Equivalent screen-reader output for every window, or an approved contract change **(guess: GPUI's accessibility support must be checked first)** |
| Control port `ui.*` | `app/human.rs`, `app/control.rs`, `dialogs/apart.rs` screens and input | Answer every `ui.*` method with the same JSON shapes, as `human-qa.py` and MCP expect: `ui.texts` (visible words with rects, nearest the front first, `window: main\|dialog`), `ui.click/type/key/scroll` (synthetic input, one step per painted frame, routed to the dialog with focus), `ui.inspect` (fields in 5.2, dialog names = English titles), `ui.select`, `ui.place`, `ui.windows`, `ui.window`, `ui.move_window`, `ui.screenshot` (PNG of the main window), `ui.accessibility`. And `commands.list` / `command.run` with the same `enabled` rules and `said`. The script guard (`by_script`) must be tied to when synthetic input is delivered |
| Commands | `DmApp::enabled` (`commands::applies` + `Looks`), `DmApp::run` (dialogs, view toggles, `commands::verb`) | Same registry. Its own `Looks` and `Shown` (title and key binding from `Chord`). Its own dialog-opening arms (about 20 of `DmApp::run`'s arms open or ask in a dialog; `app/commands.rs` touches `dialogs.*` 32 times) |
| Theme and fonts | `theme/` writes egui style; `Tokens` as `Color32`; `cover` maps system fonts per script; `bidi` reorders RTL | Port tokens and font files. GPUI shapes text itself, so `bidi.rs` and `theme/scripts.rs` may be unnecessary **(guess)**. Keep the High Contrast, accent and reduced-motion inputs (`contrast`, `accent`, `motion`, `system_theme` are toolkit-free apart from `Color32` in `accent`) |
| i18n | `i18n.rs` thread-local catalogs | Reuse as is (remove `pick`). Keep UI-thread-only wording |
| Tests | About 485 integration and 216 unit tests, mostly through `egui_kittest::Harness` and `DmApp` | Toolkit-free tests carry over. A GPUI test harness (GPUI's `TestAppContext`, **guess**) must replace kittest for the window rows. `scripts/human-qa.py` is toolkit-blind through the control port, provided the `ui.*` answers match |
| Baseline pictures (B25) | `docs/behaviour-baseline/` (≤ 1 % pixel difference) | A GPUI window will not match egui's pixels. B25 needs a maintainer-approved new baseline (the behaviour contract requires approval in words) |

### 8.3 The smallest ordering that keeps the contract

1. Move the toolkit-free parts out of `dm_desktop` first: `control`, `mcp`,
   `cli`, `Desktop`, `quiet`, `power` actions, `autostart`, `awake`,
   `wake`, `region`, `details`, `i18n` (minus `pick`), most of `format`.
2. Move 4.2's rules into `dm_shell::app` so the egui window and GPUI share
   them. Each move is held to `human-qa.py`.
3. Only then start GPUI.

This is a suggestion drawn from this audit, not a decision.

---

## 9. Where the architecture docs and the code disagree

| Doc says | Code does |
|---|---|
| ARCHITECTURE.md / EVENT_FLOW.md: each tick starts with `changes::take()` | `changes::take` is called nowhere. `refresh()` does `QUEUE_CHANGED.store(false, Release)` (`app/sync.rs:33`), which clears the mark the same way but discards whether it was set |
| ARCHITECTURE.md tick order: 1 read, then `ran_dry` and `when_done_fires`; 2 `schedule_tick`; 3 `Hands::next`; 4 `countdown_due`, `delete_due`, `look_at_watched`; 5 control | `refresh()` first reads the two inbox order files (`take_when_done`, `take_new_download`), then `Session::read`, then **`schedule_tick` before `ran_dry`** (`schedule_hooks` at `sync.rs:107`, then `when_queue_dry` at `:108`). Between the hand-offs and control it also runs hints, mailboxes, saved searches, the clipboard watch and the weekly update. `delete_due` triggers a second `refresh()` |
| ARCHITECTURE.md: in headless mode "every rule it follows is the application's"; MODULE_BOUNDARIES.md: "the window owns only its view state" | Section 4.2: question detection (B39–B41), pending renames, once-at clearing, saved searches, weekly update due date, one-time hints, hand-off cancel counter, the script guard and others still live in `DmApp` |
| EVENT_FLOW.md / DEVELOPMENT_GUIDE.md: a presentation subscribes to typed events (`session.events.subscribe`) | The egui window never subscribes. It reacts to `Read::detected`. Only `session.rs` tests subscribe |
| ARCHITECTURE.md, "Known coupling left in dm-core": dm-desktop names reqwest "only for the update check and a ZIP's listing" | `grep reqwest dm-desktop/src` finds nothing (moved by step 17, as the same page says elsewhere) |
| audit.md (written at `f9d071d`): `backend.rs` holds `Backend` and `Platform` (752+ lines); `update.rs` does a reqwest fetch; `app/control.rs` holds `queue.list` and `downloads.*`; line numbers throughout | All moved: `backend.rs` is a 10-line `pub use`; `update.rs` (79) delegates to `dm_shell::app::update::check`; `app/control.rs` (218) delegates to `control_api`. The audit's line references are stale (expected for a dated audit) |
| `dm-desktop/src/lib.rs` module doc: "`backend` is what the window asks of the queue" | `backend` is a re-export of `dm_shell::app::backend` |
| `dm-desktop/src/control.rs` module doc lists 7 methods | 24 methods are answered, `auth` included (5.2) |
| `dm-desktop/src/i18n.rs` module doc: "Only scripts egui draws correctly are offered: Latin and Cyrillic" | `LANGUAGES` includes `zh-CN`, `ja` and `ko`; `theme::cover` loads system CJK fonts |
| ARCHITECTURE.md: the egui window does the tick "in `DmApp::refresh` and `DmApp::logic`" | True, but `DmApp::logic` runs twice per painted pass (2.2) |
| Task brief: `DmApp::update` | There is no `update`. eframe 0.37's `App::logic` and `App::ui` are used |
| Task brief: the control port's `ui.*` methods read egui's widget tree | `ui.texts` and `ui.click` read **painted text shapes** (`ctx.graphics`). Only kittest tests and `ui.accessibility` read an accessibility tree |
