# Architecture audit: what sits under the window today

Phase 1 of the UI-independent refactor (branch `ui-independent-architecture`,
at `f9d071d`). I read the code and docs only: I built nothing and ran nothing.
Paths are relative to `desktop/crates/` unless they start with `docs/` or
`scripts/`. Line numbers are as of `f9d071d`.
The binding constraint throughout is `docs/behaviour-contract.md`: nothing
proposed here may change a row.

## 1. Crate and module map

### Crates and their edges

| Crate | Responsibility | dm-* deps (Cargo.toml) | Held by `cargo xtask layers` |
|---|---|---|---|
| `dm-protocol` | The wire format between the extension and the native host, plus the catch rules (`lib.rs`, `rules.rs`) | none | no GUI, no sockets (`xtask/src/main.rs:322`) |
| `dm-core` | The engine and the queue: probe, plan, ranged/HLS/DASH/FTP/torrent/live fetch, muxers, persistence | none | no GUI (`xtask/src/main.rs:324`) |
| `dm-shell` | Everything around the window: settings, native host and inbox, startup, single-instance lock, lists and crawler, registration, dial-up | dm-core, dm-protocol | no GUI (`xtask/src/main.rs:326`) |
| `dm-desktop` | The egui window, tray, login item, notifications, control protocol, MCP, CLI | dm-core, dm-protocol, dm-shell | may use GUI (`xtask/src/main.rs:327`) |
| `dm-catcher` (`browser/`) | The extension (WebAssembly) | dm-protocol | no GUI, no sockets (`xtask/src/main.rs:329`) |

`layers()` checks only the edges between crates and the GUI and socket
crates each one may use (GUI list at `xtask/src/main.rs:286-295`). It refuses
any crate it does not know (`:330-332`), so a new crate needs a line there.
It also holds every source file to 2,500 lines (`:382`). Nothing checks the
modules *inside* dm-desktop. The design intent is already written down in
`dm-shell/src/lib.rs:6-13`: "It knows nothing about any window toolkit, so
the window can be replaced without any of this being written twice."

### dm-core (51.5 k lines)

- `queue/`: the rows (`Entry`, `State`, `Id`: `queue/mod.rs:59-260`) and the
  `Queue` handle (`mod.rs:532`). Its parts:
  - `actions.rs`: pause, resume, retry, cancel, remove.
  - `edit.rs`: add, set fields, shift.
  - `runner.rs`: the scheduler loop and progress intake.
  - `persist.rs` and `journal.rs`: the queue file.
  - `settings.rs`: limits, schedules, `on_change`.
  - `query.rs`: `list`, `list_shared`, `get`, `retrying`, `swarms`.
  - `queues.rs`: the named queues.
  - `recheck.rs`: rechecking finished files.
- `download/` (`mod.rs`, `ranged.rs`, `hls.rs`, `dash.rs`) fetches one file;
  `probe.rs`, `plan.rs`, `retry.rs` and `limit.rs` support it.
- `torrent.rs`, `ftp*`, `live.rs`, `rtmp*`, `rtsp.rs`, `mms.rs` and
  `stream*` are the other protocols.
- `mp4mux*`, `mkvmux.rs`, `tsmux*`, `fmp4.rs`, `mp3*`, `gif.rs`, `clip.rs`
  and `shape.rs` are the muxers and post-processing.
- `proxy.rs`, `pac.rs`, `auth.rs`, `ntlm.rs`, `cookies.rs`, `secretstore.rs`
  and `http_client.rs` handle the network and sign-ins.
- `videopage*` and `youtube*` read video pages.
- `schedule.rs`, `quota.rs`, `category.rs`, `naming.rs`, `extract.rs`,
  `zipinfo.rs` and `tools.rs` cover policy and files.

### dm-shell (12.9 k lines)

- `settings.rs` (+`settings/secrets.rs`): `Settings` and its load, save,
  refusals, site rules and logins.
- `shell.rs`:
  - startup: `start` at `:402`;
  - putting settings in force: `check` at `:507`, `apply` at `:552`;
  - the scheduler: `wake` at `:623`, `watch_rules` at `:595`;
  - adding and its checks: `add` at `:697`, `unusable` at `:636`;
  - the inbox watcher: `watch_inbox` at `:1031`, `take_from_browser` at `:1151`;
  - opening and revealing files, and running programs.
- `host.rs` (+`host/`): the native messaging host, `Request`, depositing into
  the inbox, catch rules.
- `lists.rs`: `.dmlist`, numbered series, the crawler, `bare()`.
- `register.rs`: browser registration. `instance.rs`: the one-copy lock.
  `dialup.rs`: dial-up and VPN. `firewall.rs`: the firewall rule.

### dm-desktop (39 k lines)

| Module | One line |
|---|---|
| `main.rs` | Wiring: runtime, `shell::start`, `on_change`, inbox task, `Backend`, headless loop, eframe |
| `platform.rs`, `tray.rs`, `window.rs`, `hidden.rs`, `signals.rs`, `login.rs`, `dock.rs`, `taskbar.rs`, `jumplist.rs`, `toast.rs`, `awake.rs`, `wake.rs`, `autostart.rs`, `drops.rs`, `xdnd.rs` | OS integration (notifications, tray, login item, drag and drop, keep-awake, wake timers) |
| `backend.rs` | `Platform` trait (`:21-105`) and `Backend` (`:115`): blocking wrappers over `Queue` plus add-admission, settings apply, export/import. **No egui** |
| `power.rs` | `WhenDone`, `Report`, the countdown, power actions. **No egui** |
| `commands.rs` | The `Command` registry: 63 variants, `Command::ALL` at `:158` |
| `control.rs` | Control-protocol listener (TCP, token, `Request` at `:36`, `listen` at `:155`). **No egui** |
| `mcp.rs`, `cli.rs` | Separate-process *clients* of the control protocol (`mcp.rs:12-15`, method map `:222-232`). **No egui** |
| `app/` | `DmApp`: the window's state *and* the application logic (see section 3) |
| `dialogs/` | One file per dialog. `Dialogs` state at `dialogs/mod.rs:887`. Functions take `app: &mut DmApp` |
| `table*`, `format*`, `theme*`, `icons.rs`, `i18n*`, `bidi.rs`, `a11y_tree.rs`, `accent.rs`, `contrast.rs`, `motion.rs` | Presentation |
| `update.rs` | The weekly update check: a reqwest fetch that wakes an `egui::Context` (`:281-323`) |

These are the module edges inside dm-desktop:

```
main ──> app::DmApp ──> backend::Backend ──> dm_core::Queue, dm_shell::{shell,lists,Settings}
              │  ▲              └──> power::{WhenDone,Report}  (Platform::power)
              │  └── dialogs::*, table::*   (take &mut DmApp; DmApp owns Dialogs: a cycle)
              ├──> commands::Command, i18n, format, theme, update
              └──> control::Request (mpsc) <── control::listen (threads) <── TCP <── mcp.rs / cli.rs / scripts
```

## 2. State ownership

| State | Owner | Mutated by | Reaches the window by |
|---|---|---|---|
| Rows (`Entry`, `State`, `Progress`) | `dm_core::Queue`: `Inner.slots: Mutex<HashMap<Id,Slot>>` + `order` (`queue/mod.rs:445-448`), `Slot.entry: Arc<Entry>` (`:365-368`) | Only `Queue` methods (`actions.rs`, `edit.rs`, `queues.rs`), the runner's progress intake (`runner.rs:360-392`) and the inbox task (`shell.rs:1084-1104`) | `Backend::list_shared` (`backend.rs:177`) → `Queue::list_shared` (`query.rs:49-56`): one `Arc` clone per row under both locks; held as `DmApp::rows` (`app/mod.rs:302-304`) |
| Change signal | `Queue.inner.changed`, **one subscriber** (`queue/settings.rs:9-13`), fired by `save()` and `note()` (`persist.rs:164`, `:362`) but not by progress (`runner.rs:372-391`) | main.rs sets the global `QUEUE_CHANGED` and asks for a repaint in 250 ms (`main.rs:425-430`) | `refresh_plan` (`app/mod.rs:69-79`) in `logic()` (`app/sync.rs:542-550`) |
| Per-run extras (retry counts, swarms, quota held) | `Slot.retrying` and `Slot.swarm` (`queue/mod.rs:383-385`), the quota meter | engine | Copied each refresh into **thread-locals** of the window crate (`app/sync.rs:39-42` → `app/rows.rs:93-150`) |
| Derived row state (kind, haystack, visible order, counts, queue names) | `DmApp` caches keyed by `rows_gen` (`app/mod.rs:519-541`, `app/places.rs:77-200`, `app/mod.rs:745-758`) | `refresh`, `place_rows` | direct |
| "What changed" memory (`last_state`, `notified`, `failed_told`, `ask_seen`, `own`, `dry`, schedule `Watch`es) | `DmApp` (`app/mod.rs:307`, `:389`, `:427-429`, `:434`, `:440`, `:476`, `:504`) | `refresh()` | none: events are derived by diffing |
| Settings | Authoritative: `Backend.settings: Arc<Mutex<Settings>>` (`backend.rs:121`). Mirror: `DmApp.applied` (`app/mod.rs:393`). On disk: `Settings::save` | `Backend::set_settings` (`backend.rs:378-417`): refusal, clamp, `shell::check`, login item, `shell::apply` to the queue, store, save. Wrapped by `DmApp::apply_settings` (`app/sync.rs:679-722`), which adds the script guard, `i18n::set`, `place_rows` and the clipboard watch. Called from 20+ places (menus, dialogs, schedule, searches) | `applied` replaced on success, re-read on failure |
| Engine-wide globals | `dm_core::set_identify_as_dm`, `set_second_path`, `cookies::use_file`, `tools::allow_fetching` (`shell.rs:557-590`), `shell::TO_WINDOW` (`shell.rs:135-145`) | `shell::apply` | n/a |
| Queue schedule (hold and start) | `Queue.inner.schedule`, `queue_schedules` and `starts` (`queue/mod.rs:482-498`); the loop is `shell::watch_rules`, every 2 s (`shell.rs:595-617`) | `shell::apply` | n/a |
| Schedule *hooks* (arm When done, retry failed rows, run a program, hang up, slow-hours limit, clear a spent `once_at`) | `DmApp::schedule_hooks` (`app/scheduled.rs:182-247`), state in `scheduled::Watch` (`:22-134`) | run after every `refresh()` (`app/sync.rs:115`) | in-window |
| Other timers: When done countdown, Delete file after, saved searches, watched folder, clipboard, weekly update | `DmApp` (`app/sync.rs:517-540`, `:739`; `app/search.rs:72`; `app/watch.rs:24`; `app/clipboard.rs:24`; `app/sync.rs:645`) | `logic()` each wake | in-window |
| Hand-offs that need a question | `Backend.handed: Arc<Mutex<VecDeque<host::Request>>>` (`backend.rs:128`), filled by the inbox task (`main.rs:442-465`) | `DmApp::take_handed` (`app/sync.rs:290-397`) | in-window |

How often the window reads:
- `REFRESH` is 250 ms while anything runs or waits (`app/mod.rs:57`,
  `app/sync.rs:606-639`).
- Idle it is 30 s; hidden it is 1 s while busy and 30 s idle (`app/mod.rs:87-92`).
- Any change triggers a read at most 255 ms after the last one
  (decisions.md:2728-2751).
- **Every `Command`** ends in a full `refresh()` (`app/commands.rs:497`), and
  so do `remove_now` and others: 11 call sites.

**Headless** mode runs the same `DmApp::logic` against a dummy
`egui::Context`, every 50 ms (`main.rs:502-514`). Today the "application" is
the egui crate's `DmApp`, even with no window.

## 3. Coupling inventory: application logic inside dm-desktop

Key to the "egui?" column. **none**: the code uses no egui types. **ctx**:
it takes an `egui::Context` only to request repaints. **UI**: it draws
widgets or is drawn inline with them.

| Where | What it does | egui? | Target layer |
|---|---|---|---|
| `backend.rs:115-752` | Every queue verb as a blocking call. Add admission (`admit` `:754`, site rule and login `:496-548`). `login_now` before resume and retry (`:231-243`). `wake` after starts. Export and import (`:670-751`) | none | application (dm-shell) |
| `backend.rs:21-105` | `Platform` trait (notify, open, reveal, trash, chime, power, choose file) | none | application *port*; impl stays in `platform.rs` (infrastructure, GUI libs) |
| `power.rs:16-140` | `WhenDone`, `COUNTDOWN_SECS`, `Report`, `available()` | none (Windows FFI at `:256`, `:304`) | types to application; actions stay in OS integration |
| `app/sync.rs:30-205` `refresh()` | Reads the snapshot. Detects finished, failed and started-elsewhere rows and addresses added again. Posts notifications and plays sounds (`:55-114`). Runs the on-complete program (`:78-89`). Opens progress and complete windows (`:116-163`) | none (writes `dialogs.*`) | split: change detection and the program to application; wording and dialogs to presentation |
| `app/sync.rs:266-287`, `app/mod.rs:906-960` | `when_queue_dry` / `DryWatch`: queue ran dry, then sound and the When done countdown | none | application |
| `app/sync.rs:517-540` | `countdown_tick`: the power action at the end of the countdown | none | application |
| `app/sync.rs:290-397` | `take_handed`: routes a hand-off (dedupe in 5 s, awaiting a fresh link, link lists, duplicate policy `dialogs::worth_keeping`/`Dup`, ask or add) | none (opens dialogs) | application decides; presentation shows the question |
| `app/sync.rs:399-441` | `take_new_download`, `take_when_done`: inbox files from a second launch | none | application |
| `app/sync.rs:443-475` | Awaiting a fresh link; `shell::route_to_window` | none (`awaiting_bar` `:479` is UI) | application |
| `app/sync.rs:542-640` `logic()` | The tick: refresh plan, hints, hand-offs, countdown, delete-due, mailboxes, searches, watched folder, clipboard, control, weekly update, then picks the wake interval | ctx (`request_repaint_after` `:639`) | application tick + presentation wake |
| `app/sync.rs:679-722` | `apply_settings`: the script guard (`keep_programs_of`), then `Backend::set_settings`, `i18n::set`, `place_rows` | none | application (minus i18n and place_rows) |
| `app/sync.rs:739-783` | "Delete file after": trash, then remove the row | none | application |
| `app/sync.rs:834-868` | Remove with Undo (`finished_rows`/`put_back`) | none | application (the Undo timer is presentation) |
| `app/sync.rs:963-1025` | `add`, `add_sized`, `add_items`: batch add, marks `own` | none | application |
| `app/sync.rs:1042-1157` | `watch_asks`/`Ask` (B39 name, B40 page, B41 404) and pending renames | none | application event; presentation question |
| `app/scheduled.rs:22-334` | Schedule `Watch` state machine, `schedule_hooks`, `end_of_window`, `hang_up_later` (Windows) | none (test at `:623` uses a dummy ctx) | application |
| `app/commands.rs:11-497` | `enabled()` and `run(Command)`. They mix row verbs (resume or retry `:211-217`, stop, shift, set queue, redownload) with opening dialogs | none (sets `dialogs.*`) | split: verbs to application, dialog choices to presentation |
| `app/control.rs:19-75` | Takes control `Request`s and gates them by MCP and assistants-only. `ui.screenshot` | ctx (`request_screenshot`) | presentation (screenshot); gate to application |
| `app/control.rs:80-422` `control_call` | `commands.list`, `command.run`, `ui.select`, `ui.place`, `ui.inspect`, `queue.list` (redacts cookies and headers `:216-219`), `downloads.pause/resume/cancel` (`:226-280`), `downloads.speed_limit`, `downloads.add` (folder confinement `:327-343`, dedupe `:367-385`), `batch` | none | `queue.list`/`downloads.*` to application; `ui.*`/`command*` presentation |
| `app/human.rs:60-543` | `ui.texts`, `click`, `type`, `key`, `scroll`, windows; `by_script()` | ctx | presentation (but `by_script` is an application rule) |
| `app/search.rs:11-207` | Saved site searches: timer, crawl, add found | none | application |
| `app/watch.rs:24-130` | Watched folder every 5 s | none (uses `drops::DroppedPath`) | application |
| `app/clipboard.rs:10-80` | Clipboard watch and offer | none | application (`Platform::clipboard`) |
| `app/lists.rs:11-120` | Import and export with dedupe | none | application |
| `app/files.rs:11-90` | Open, reveal, extract zip, `schedule_holds` | none | application (via `Platform`) |
| `app/naming.rs:16-110` | Planned folder and name a row will get | none | application (read model) |
| `dialogs/add.rs:8-260` | `bare`, `worth_keeping`, `Dup`, `answer_duplicate` (B17/B50 policy) | none for these fns | application |
| `dialogs/series.rs:176` | `listed_addresses` | none | application |
| `dialogs/progress.rs:514-529`, `app/bars.rs:123-165`, `dialogs/add.rs:236-243`, `dialogs/properties.rs:319-333`, `dialogs/row_dialogs.rs:70-210`, `dialogs/file_info.rs:774-872`, `table/menus.rs:85`, `table/columns.rs:59` | Row verbs called straight from widgets | UI | presentation calls an application verb |
| `update.rs:240-323` | The update check | ctx | application (wake through callback) |
| `main.rs:425-430`, `:442-498` | `on_change` → global flag, inbox task, keep-awake, wake timer | ctx | composition root |

**The same rule written more than once.** "Resume a stopped row, retry a
failed one, ask about a held one" is written five times:
`app/commands.rs:211-217`, `app/control.rs:249-271`,
`dialogs/progress.rs:517-526`, `dialogs/add.rs:236-243` and
`app/bars.rs:130,165`. Adding with the site rule and the saved login is
written three times: `backend.rs:496-548`, `shell.rs:1100-1104` and
`shell.rs:431-438`. Only `Backend` also applies `admit`. The duplicate test
is written three times: `dialogs/add.rs:160`, `app/control.rs:367-385` and
`app/sync.rs:168-185`.

## 4. Existing abstractions to reuse

- **`Backend`** (`backend.rs:115`) is already a UI-free application facade:
  it is `Clone`, owns the runtime handle, the queue, the settings and the
  platform, and every method returns plain data. Its only non-std,
  non-dm-shell dependency is `crate::power` (`:43`, `:49`, `:72-76`). It can
  move into dm-shell almost verbatim.
- **`Platform`** (`backend.rs:21`) is the port for OS effects. A second GUI
  supplies its own implementation. Test doubles exist (`backend.rs:815`,
  `app/scheduled.rs:497`).
- **dm-shell** already provides:
  - `start` (`shell.rs:402`), `check`/`apply` (`:507`, `:552`), `wake`
    (`:623`) and `watch_rules` (`:595`);
  - `add`/`unusable`/`checksum` (`:697`, `:636`, `:741`);
  - `watch_inbox`/`take_from_browser`/`Taken` (`:1031`, `:1151`, `:1134`);
    `route_to_window` (`:139`);
  - `run_after`/`run_program` (`:1284`, `:1261`), `open_file` and
    `reveal_in_folder`;
  - `lists::{bare, to_list, from_list, crawl_with}`, `Settings::{with_login,
    with_site_rule, refusal, clamped, keep_programs_of}`.
- **The `dm_core::Queue` API**: 79 public functions (`queue/*.rs`).
  - Reads: `list`, `list_shared`, `get`, `retrying`, `swarms`, `queues`,
    `connections`, `resumable`, `problems`, `allowance_spent`.
  - Verbs: add, add_with, add_each, pause, resume, retry, cancel, remove,
    remove_many, pause_all, resume_all, start_queue, stop_queue, shift,
    set_queue, set_url, relocate, rename, move_to, redownload,
    settle_early, put_back, adopt_fresh_link.
  - Settings: set_options, set_client, set_schedule, set_queue_schedules,
    set_concurrency, set_speed_limit, set_own_limit, set_allowance,
    set_recheck_every.
  - Life cycle: `run`, `persist_to`, `flush`, `on_change`.
- **Snapshot and event mechanisms that already exist:**
  - `Arc<Entry>` copy-on-write rows: `Slot::entry_mut` copies the row first
    if a reader still holds it. With `Arc::ptr_eq`, a reader can tell which
    rows did not change (decisions.md:297-316; `app/places.rs:89-95`).
  - `Queue::on_change` with a single subscriber, and the `QUEUE_CHANGED`
    flag.
  - `rows_gen`, a generation counter (`app/mod.rs:524`).
  - mpsc for control requests; `Arc<Mutex<Option<T>>>` mailboxes for
    background results (`app/mod.rs:443-460`: `rechecked`, `opener_said`,
    `hung_up`, `search_results`).
  - `download::Update` (`Named`, `Progress`, `Validator`, `Files`:
    `download/mod.rs:568`) is the engine-internal event stream; it is
    exported, but only the runner consumes it.
  - There is no public queue event type and no `watch` or `broadcast`
    channel.
- **The control protocol**, which is already a UI-independent command API
  (JSON lines on loopback with a token: `control.rs:6-21`). Its methods:

| Method | Implemented | UI-independent? |
|---|---|---|
| `auth` | `control.rs` listener | yes |
| `commands.list`, `command.run {id}` | `app/control.rs:88-118` → `DmApp::enabled/run` | no: commands open dialogs |
| `queue.list` | `app/control.rs:205-223` | yes (reads the window's copy) |
| `downloads.pause/resume/cancel {ids}` | `app/control.rs:226-280` (reads `Backend::row`) | yes |
| `downloads.speed_limit {kb}` | `app/control.rs:283-298` | yes |
| `downloads.add {urls, later, folder, name, description, from, to, again}` | `app/control.rs:300-392` | yes |
| `batch {steps ≤256}` | `app/control.rs:395-420` | yes (dispatcher) |
| `ui.select`, `ui.place`, `ui.inspect` | `app/control.rs:119-204` | no |
| `ui.screenshot` | `app/control.rs:49-70` | no |
| `ui.texts/click/type/key/scroll/accessibility/windows/window/move_window` | `app/human.rs:194-410` | no |

MCP maps its tools onto these methods (`mcp.rs:222-232`). The CLI uses
`queue.list` and `downloads.*` (`cli.rs:81-83`, `:161`).

## 5. Event flow today

The path from the engine to the screen:

1. The runner writes progress into `Slot.entry` (`runner.rs:372-391`). It
   does not call `save()`, so a progress-only change raises no change signal.
2. A state change calls `Queue::save` → `tell()` (`persist.rs:163-167`). The
   callback in main.rs sets `QUEUE_CHANGED` and calls
   `request_repaint_after(250 ms)` (`main.rs:425-430`). Its comment says why:
   10 downloads cost 9.7 % of a core, and 14.6 % with a repaint for each
   change.
3. Each frame runs `logic()`. It calls `refresh()` when 250 ms have passed
   since the last read (`app/sync.rs:543-550`). While anything is Running or
   Queued it wakes itself every 250 ms (`:606-639`). Progress therefore
   reaches the screen by polling, not by events.
4. The control protocol: listener threads put each `Request` on an mpsc
   channel and wake the context (`main.rs:110`). It is answered inside
   `logic()` on the window thread (`app/control.rs:19-75`). Headless, that
   happens within 50 ms.

What each read costs at n rows. Every term is O(n), except where marked:

| Step | Cost |
|---|---|
| `list_shared` | n `Arc` clones under the queue's two tokio mutexes. The runner's progress intake takes the same `slots` lock (`runner.rs:361`) |
| `retrying()` + `swarms()` | Two walks under the lock, then new HashMaps in the thread-locals |
| `place_rows` | A pointer check per row; a hash and re-place only for changed rows (`app/places.rs:89-133`) |
| `selected.retain`, `ids` HashSet, the finished/failed loop, `last_state` rebuild (clones every `State`, including `Failed(String)`: `app/sync.rs:195`) | about 5 n |
| `schedule_hooks` | `looked_after` and `work_left` per schedule (`app/scheduled.rs:228-246`) |
| "Added again" check | O(fresh × n) URL parses (`app/sync.rs:168-185`); only when new rows appear |
| `sizes_told.retain` | O(k × n) (`app/sync.rs:191-194`) |
| `visible_shared` invalidated (`rows_gen` changed) | Filter n, then sort n log n: 370 µs at 5,000 rows (decisions.md:2803-2804) |

Per frame, even with no read:
- the `moving` scan is n (`app/sync.rs:606-610`);
- `theme::cover` looks at 3n strings (`app/ui.rs:89-99`; ASCII strings are
  skipped quickly);
- `DmApp::row(id)` is a linear search (`app/mod.rs:766`).

**Copies of the row list per tick:** one pointer vector from the queue
(`rows`), plus one pointer vector for the visible list when the generation
changed. No `Entry` is deep-copied on that path; `targets()` copies only the
selected rows (`app/places.rs:234-247`). Full deep copies remain in a few
places:
- `Backend::list()` in export and import (`backend.rs:672`,
  `app/lists.rs:53-66`) and in `dialogs/state.rs:198`;
- the Windows keep-awake thread, every 5 s (`main.rs:473-477`, `awake.rs:16`);
- `queue.list` serializes every row (`app/control.rs:205`).

**Measured figures:**
- 5,000 rows with 20 moving: the window thread is 3.3–3.4 % busy, 128 MB
  (decisions.md:297-316); 6.0 % and 4.9 % busy after the per-frame cuts
  (decisions.md:2805-2807).
- The `window` phase, 5,060 rows and 60 active: a call is answered in a
  median 3 ms, worst 19 ms (Linux, docs/gaps.md:187).
- `ui.inspect` grows with the square of the list: about 7 s at 5,000 rows and
  25 ms at 1,500 (`scripts/stress-check.py:1562`).
- Listing 20,000 rows over the control port takes 0.47 s (decisions.md:4319-4330).
- `bigqueue`, 20,000 links: 9.8 files/s on Windows (docs/gaps.md:187);
  37 → 116 files/s after the journal (decisions.md:2704).

At 10,000 rows I expect each read and each sort to roughly double. That is
about 1 ms a read for the sort, four times a second, and is fine. The real
costs are elsewhere: `ui.inspect` (about 28 s, extrapolated), the "added
again" scan when a batch arrives, and a full `refresh()` after every command
or batch step.

## 6. Domain model check (dm-core public types)

| Type | Leak | Evidence |
|---|---|---|
| `Queue::new(client: reqwest::Client, …)` | HTTP client in the constructor | `queue/mod.rs:559`; field `queue/mod.rs:504` |
| `pub use reqwest` | Re-exported for callers. dm-shell and dm-desktop name `dm_core::reqwest::Client` | `lib.rs:151`; `dm-shell/src/lists.rs:510,556`; `dm-desktop/src/app/mod.rs:728`, `update.rs:240-291`, `dialogs/zip_list.rs:41` |
| `dm_core::Error` | `Network(reqwest::Error)`, `Disk(std::io::Error)`, `from_response(reqwest::Response)` | `error.rs:79,88,126,167`. Rows do not carry it: `State::Failed(String)` (`queue/mod.rs:74`) |
| `Entry` | Filesystem as `String`: `path`, `parts_dir`, `began_in`, `save_dir`, `replaces` (`queue/mod.rs:132,236,241,170,250`). Secrets on the row: `cookies`, `headers`, `post` (`:209-230`, `:260`). Redacting them is left to each reader (`app/control.rs:216-219`, `details.rs`) | `queue/mod.rs:102-261` |
| `Entry` serde | One shape serves the queue file, the control protocol's JSON and the window | `queue/mod.rs:101`; `app/control.rs:209` |
| `Queue::retrying()` | `std::time::Instant` (process-local, not serializable) | `queue/query.rs:24`; `backend.rs:188-193` |
| `Queue::move_to`/`preview` | Return a `PathBuf` | `backend.rs:641-651` |
| `download::Options` | `Control`, `Limiter`, `PathBuf` | `download/mod.rs:56-62`; re-export `lib.rs:173-176` |
| `Swarm`, `TorrentFile` | dm's own structs, not librqbit's | `torrent.rs:37,240` |
| GUI types | **None**, enforced (`xtask/src/main.rs:324`) | n/a |
| Events | No domain event type. `on_change` is a `Fn()` with no payload and a single subscriber | `queue/settings.rs:9-13` |

The engine also keeps process-wide globals, which only matters to two hosts
in one process: `set_identify_as_dm`, `set_second_path`, `cookies::use_file`,
`tools::allow_fetching` (`dm-shell/src/shell.rs:557-590`) and torrent
`SHARED` (`torrent.rs:129`). None of these block a second GUI, which would
be a second binary.

## 7. Target and incremental steps

### Target, mapped onto the real crates (no new crate)

```
dm-desktop (egui presentation + OS integration impl: platform.rs, tray, toast, drops, power actions)
   │   DmApp = view state (selection, sort, place, dialogs, caches) + Session
   ▼
dm-shell  (application layer)  new module dm_shell::app/
   │   Backend (moved), Platform trait (port), WhenDone/Report,
   │   Snapshot {rows, gen, retrying, swarms, limit_holds}, Changes (finished/failed/started/asks/dry),
   │   Session tick: schedule hooks, countdown, delete-due, searches, watched folder, hand-off routing,
   │   control_api: queue.list / downloads.* / batch dispatch
   │   + existing: settings, host, inbox, lists, register, instance (infrastructure glue)
   ▼
dm-core   (domain + engine; unchanged at first)      dm-protocol (wire)
```

A future GPUI crate would depend on dm-shell and dm-core only, implement
`Platform`, own `Session`, and draw from `Snapshot`. Headless mode becomes
`Session` with no egui. `layers()` needs no change, because dm-shell is
already `no_gui`.

### Steps

Each step leaves the workspace building, keeps every public path through a
`pub use` shim, and is one reviewable commit. Run `cargo test --workspace`
and the full `human-qa.py` after each one, as CLAUDE.md requires.

| # | Step | Moves | Existing lines touched |
|---|---|---|---|
| 1 | Move the power types | `WhenDone`, `COUNTDOWN_SECS`, `Report` → `dm_shell::app::power`. `power.rs` keeps `available()` and the actions, plus `pub use dm_shell::app::power::*` | ~50 (power.rs: no churn since 11-01) |
| 2 | Move the `Platform` trait and `chooser_here` | → `dm_shell::app::platform`. `backend.rs` re-exports them | ~90 |
| 3 | Move `Backend` | `backend.rs` body → `dm_shell::app::backend`. dm-desktop `backend.rs` becomes a `pub use` shim; its tests move with it | ~900 moved, shim 5. **Do it in one commit agreed with the parity branch** (backend.rs: 10 commits since 11-01) |
| 4 | Snapshot | `Backend::snapshot() -> Snapshot` (rows via `list_shared`, `retrying`, `swarms`, `allowance_spent`, `problems`). `refresh()` calls it, which replaces 4 lines (`app/sync.rs:35-42`). The thread-locals are still fed from it | ~20 |
| 5 | Change fan-out | `dm_shell::app::Changed` (an `AtomicBool` and a list of wakers) subscribed once to `Queue::on_change`. main.rs registers the egui waker; `QUEUE_CHANGED` becomes a re-export of the same flag | ~15 (main.rs:425-430, app/mod.rs:64) |
| 6 | The resume-or-retry rule, once | `Backend::go_on(id) -> GoOn {Resumed, Retried, Held, Settled, NotStopped}`. The 5 call sites in section 3 each keep their own reaction to `Held` (a dialog, a sentence or a control error) | ~30, one line per call site |
| 7 | Duplicate rule and admission, once | `worth_keeping`, `Dup`, `listed_addresses` and `bare` → `dm_shell::app::dupes`. The inbox and `start` adds go through one `admit_and_add` | ~40 |
| 8 | UI-free control methods | `queue.list`, `downloads.*`, `downloads.add` validation and `batch` → `dm_shell::app::control_api::call(&Backend, &Settings, …) -> Option<Result>`. `DmApp::control_call` tries it before its `ui.*` and `command*` arms. The listener in `control.rs` can follow (sockets are allowed in dm-shell) | ~250 moved (app/control.rs: no churn since 11-01) |
| 9 | Change detection | Pure `Changes::since(last_state, rows, own, notify_ready)` → {finished, failed, expired, started_elsewhere, added_again}, plus `DryWatch` and the `Ask` diff (`watch_asks`). `refresh()` keeps the side effects in the same order, worded by the window | ~150 in sync.rs (12 commits since 11-01: small, separate commits) |
| 10 | Schedule hooks | `scheduled::Watch`, `looked_after`, `work_left` and `end_of_window` → `dm_shell::app::schedule`, returning actions {SetLimit, Arm, Disarm, Retry(ids), RunProgram, HangUp}. The arming sentence stays in the window | ~200 |
| 11 | `Session` timers, one per commit | `countdown_tick`, `delete_due_files`, `tick_searches`, `take_watched`, `check_clipboard`, `take_new_download`/`take_when_done`, `take_handed` routing → `Session::tick(now) -> Vec<Effect>`. `DmApp` owns the `Session` and applies the effects | ~100 each |
| 12 | Settings coordination | The `keep_programs_of` script guard moves into `Backend::set_settings_from(origin)`. `DmApp::apply_settings` keeps `i18n::set`, `place_rows` and the clipboard watch | ~30 |
| 13 | Headless without egui | `main.rs:502-514` loops on `Session::tick` and `control_api`. Before this lands, decide whether a headless `command.run` should still be answered through a dialog-free `DmApp`, because it is today | ~30 |
| 15 | The command registry (done 2026-11-09) | `Command`, its names, labels and keys (`Chord`, `Key`) → `dm_shell::app::commands`; `enabled()`'s rules → `commands::applies` with `Looks` for what the window sees (retrying, file gone, plays); the row verbs, Start and Stop Queue, Delete Completed, When done, the speed limiter's new settings and the recheck → `commands::verb`, `when_done`, `limited`, `recheck`. `dm_desktop::commands` keeps `Shown` (title, egui shortcuts) and `pressed` | ~400 |
| 16 | The rest of section 3's rows (done 2026-11-09) | `dm_shell::app::names` (`host`, `provisional`, `name_of`, `kind_of`, the planned folder, the free name a row will get); `import` (how much of a list is listed already, the rows it adds, the lines with no link); `files::extract_later` (a ZIP off the presentation's thread); `schedule::holds_waiting`; `links::Clipboard` (the watch's memory, `turn`, `look`, `dismiss_all`). The old paths re-export; the window keeps reading files and the clipboard, its questions and its words | ~350 |
| 17 (done 2026-11-09) | The last coupling the spec names | The window's own fetches (the update check, a ZIP's files, a file's name and size, a page's links, a video's qualities, a playlist, a site crawl) and its direct use of the engine's queue (`allowance_spent`, a row removed, the crawl's options) → `dm_shell::app::{update, files, lookups, search}` and `Backend` (`client`, `allowance_spent`, `remove_later`); typed events (`events`) published by `Session::read`; the load measures of EVENT_FLOW.md | ~900 |
| 14 (done 2026-11-09) | dm-core seams: `Entry::redacted()` added and used by `queue.list`; `pub use reqwest` kept (decisions.md, 2026-11-09) | `Queue::new` takes a client factory, or keep `pub use reqwest` (shell and window still name it). Add `Entry::redacted()` so readers cannot forget the secrets | small |

Steps 1, 2, 4, 5, 6 and 8 touch few or rarely-churned lines and can land
first. Steps 3, 9 and 11 touch the most-churned files and should be
coordinated (see section 8).

## 8. Risks

**Contract rows most at risk:**
- **B03, B04, B05 and B19** (start, stop, resume, Start Queue) through step 6,
  and **B64** (the queue chosen in a drop-down: `chosen_queue`,
  `app/places.rs:261`).
- **B17 and B50** (already in the list: Show it, Download again, Replace,
  Remember my choice) through step 7.
- **B14, B39, B40 and B41** (the 404, name and page questions, each asked
  once) through step 9's `Ask` diff and its `asks_primed`/`notify_ready`
  priming (`app/sync.rs:197`, `app/mod.rs:442`).
- **B63 and B46** (progress and complete windows for rows started inside or
  outside the window: `own`, `last_state`, `app/sync.rs:141-163`) through
  step 9.
- **B23, B24 and B38** (remove, Undo, Delete Completed) through
  `remove_now`/`refresh`.
- **B49, B62 and B65** (Scheduler When done, run a program, per-queue hours)
  and **B20** (the slow-hours limit through `limit_in_force`) through
  step 10.
- **B26–B30** (hand-off routing, `route_to_window`, the 5-second dedupe)
  through step 11.
- **B25** (baseline pictures): no window change is planned.
- **B21 and B22** (persistence) are safe while `dm_core::Queue` is untouched.

**Concurrency hazards:**
- `Backend` calls `runtime.block_on` (`backend.rs:166-168`). Calling it from
  inside a tokio task panics, so `Session` must run where the window's
  `logic()` runs, or gain async twins of the methods.
- `on_change` has a single subscriber and a second call *replaces* the first
  (`queue/settings.rs:9-13`): step 5 must be the only subscriber.
- `refresh()` clears the flag *before* it reads (`app/sync.rs:31-33`). That
  order must survive, or a change made during a read is lost
  (decisions.md:2728-2751).
- Thread-locals for the retry, swarm and limit text (`app/rows.rs:93-150`)
  and the per-thread language (decisions.md:1142) both assume the window
  thread. Notification words are made with `t()` on that thread
  (`app/sync.rs:55-114`), so moving their composition to another thread
  changes the language.
- Control answers are ordered, and each is made after the previous command's
  `refresh()`. Scripts and `human-qa.py` depend on that ordering
  (decisions.md:2746-2751).

**Performance hot paths:**
- `refresh()` every 250 ms while anything moves.
- `visible_shared` and its sort.
- The `list_shared` lock against the runner's progress lock (`runner.rs:361`).
- `ui.inspect`, which is quadratic.
- `refresh()` after every command and batch step: 256 steps can mean 256
  full reads.

A `Snapshot` must stay a vector of `Arc` pointers, never deep copies; the
memory dropped from 312 to 128 MB when the copies went (decisions.md:297-316).

**Platform-specific code the moves touch:**
- Windows:
  - `power.rs:256,304` (`windows_sys`);
  - `hang_up_later` and `dialup::hang_up`, behind `cfg!(windows)`
    (`app/scheduled.rs:281`);
  - `toast.rs` (`lib.rs:58-59`);
  - keep-awake (`awake.rs:39-52`, `main.rs:471-478`).
- Linux: `chooser_here` (zenity/portal, `backend.rs:107`), `xdnd.rs` and the
  Wayland hide (`main.rs:64-68`).
- macOS: `dock.rs` (`main.rs:553-560`).

dm-shell already has `windows-sys` and `libc`, so moving the types is safe.
The OS actions stay in dm-desktop.

**Conflicts with `parity-with-reference-a`.** The branch is at the same
commit as this one today (`f9d071d`), but it moves fast: 210 commits since
2026-11-01. Its most-touched files in that time:
- in dm-desktop: `dialogs/state.rs` (17 commits), `dialogs/mod.rs` (16),
  `options/tabs.rs` (12), `app/sync.rs` (12), `app/mod.rs` (12),
  `app/rows.rs` (11), `backend.rs` and `properties.rs` (10 each);
- in dm-shell: `settings.rs` (21), `host.rs` (16), `shell.rs` (11).

`power.rs`, `control.rs`, `app/control.rs`, `mcp.rs` and `cli.rs` had none.
To keep conflicts down:
- add new files under `dm-shell/src/app/`;
- leave `pub use` shims at the old paths;
- change call sites one line at a time rather than move blocks out of
  `sync.rs`;
- do not reformat;
- rebase before each step;
- land step 3 (moving `Backend`) as a pure move, at an agreed quiet moment.

## Summary

dm already has most of the separation a second GUI needs. dm-core has no GUI
types. dm-shell is held to `no_gui` and says it exists so the window can be
replaced. dm-desktop's `Backend` and `Platform` are an egui-free application
facade and OS port, and the control protocol is a UI-independent command API
that MCP and the CLI use. What keeps it from being reusable is that the
application *behaviour* lives in dm-desktop's `DmApp`. Its `refresh()`
detects changes by diffing what it last read. The schedule hooks, When done,
Delete file after, saved searches, the watched folder, hand-off routing,
duplicate rules and settings coordination all run from the egui `logic()`
tick, even headless, against a dummy `egui::Context`. They write their results
straight into dialog state, and some rules are written in up to five places.
The proposal: grow `dm_shell::app` into the application layer, with no new
crate. First move the pure types and `Backend` behind `pub use` shims. Then
add a `Snapshot` and a change fan-out over the existing `Arc<Entry>` sharing
and `on_change`. Then pull out the UI-free control methods and the shared
row rules. Then extract change detection, the schedule hooks and the timers
into a `Session` the window owns. Each step is small, keeps the window and
the behaviour contract as they are, and leaves the most-churned files
(`app/sync.rs`, `backend.rs`, `dialogs/state.rs`) for coordinated,
mechanical commits.
