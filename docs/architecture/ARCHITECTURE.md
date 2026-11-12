# dm's architecture

dm is layered so that the window is one presentation of an application that
does not know about it: the GPUI window (`dm-gpui`) is the one built and
shipped on macOS, Windows and Linux, and the older egui window (`dm-desktop`)
drives the same application until gaps row G9 removes it, neither changing
the engine or the application's rules. This page says what the layers
are, where they live, and how far the code has moved towards them. The audit
behind it is [audit.md](audit.md); the rules for each module are in
[MODULE_BOUNDARIES.md](MODULE_BOUNDARIES.md); how a change reaches the screen is
in [EVENT_FLOW.md](EVENT_FLOW.md); where new code goes is in
[DEVELOPMENT_GUIDE.md](DEVELOPMENT_GUIDE.md).

## The layers, on the real crates

```text
┌──────────────────────────────────────────────────────────────┐
│ Presentation: dm-gpui (GPUI), the window shipped             │
│   the window, dialogs, tray, drag and drop, and what the     │
│   system does for Desktop (clipboard, choosers, notices)     │
│ (dm-desktop, egui: the older window, the same place, to G9)  │
└───────────────────────────┬──────────────────────────────────┘
                            │ WindowServices
┌───────────────────────────▼──────────────────────────────────┐
│ The desktop program, not drawn: dm-desktop-base              │
│   Desktop (the Platform), the control port, MCP, the command │
│   line, words and numbers (i18n, format), the system's       │
│   services; no window toolkit (`cargo xtask layers`)         │
└───────────────────────────┬──────────────────────────────────┘
                            │ dm_shell::app: Snapshot, changes, rules, Platform
┌───────────────────────────▼──────────────────────────────────┐
│ Application: dm-shell, module dm_shell::app (+ settings,     │
│   host, inbox, lists, register: the glue it coordinates)     │
│   no window toolkit, ever (`cargo xtask layers`)             │
└───────────────────────────┬──────────────────────────────────┘
                            │ dm_core::Queue, Entry, State, Id, …
┌───────────────────────────▼──────────────────────────────────┐
│ Domain and engine: dm-core                                   │
│   the queue and its rows (domain), the downloaders (HTTP,    │
│   HLS, DASH, FTP, torrents, YouTube's reading), persistence  │
└──────────────────────────────────────────────────────────────┘
        dm-protocol: the words the browser extension and the app speak
```

No new crate was made for the layering: dm-shell was already the code around
the window that is not the window, and the build tool already forbids it any
window toolkit. The application layer is its module `dm_shell::app`.

The GPUI window depends on dm-desktop-base, dm-shell, dm-core and
dm-protocol only, never dm-desktop (`cargo xtask layers`). It gets its
`Platform` from `dm_desktop_base::desktop::Desktop`, supplying
`WindowServices`, takes `dm_shell::app::changes`, reads the application's
rows, and asks the application to act. The egui window does the same and
re-exports each of the base crate's modules at its old path.

## Where the engine's code is, by concern

So that work on the engine knows which files are its own; the window never
reaches past `dm_core::Queue` (and the control port the same queue serves).

| Concern | Where |
|---|---|
| The queue the window reads and commands | `dm-core/src/queue/` (`runner.rs`, `actions.rs`, `persist.rs`, `mod.rs`) |
| Download logic | `dm-core/src/download/` (`mod.rs`, `ranged.rs` and `ranged/`, `assemble.rs`, `files.rs`) and `plan.rs`: probing, ranges, halving (`split_largest`), growing past eight connections (`grow`), assembly, naming |
| Networking and protocols | `dm-core/src/`: `http_client.rs`, `probe.rs`, `ftp/`, `download/hls.rs`, `dash.rs`, `torrent.rs`, `metalink.rs`, `auth.rs`, `ntlm.rs`, `proxy.rs` (reqwest over rustls, one keep-alive pool per client) |
| Video | `youtube.rs` (a YouTube page read by dm itself, its player's checks run in `javascript.rs`), `videopage.rs` and `videopage/` (the qualities listed, the streams fetched and joined); every other site's video is a stream the extension caught, fetched as any download |
| Files and persistence | `download/files.rs`, `download/space.rs`, `queue/persist.rs`, `dm-shell/src/settings.rs` (a staged file and a ledger per range; the queue file) |
| Scheduling and concurrency | `queue/runner.rs`, `limit.rs` (speed), `quota.rs` (tokio tasks; one speed limiter shared by connections) |
| Errors and recovery | `retry.rs`, `error.rs` |
| Integration and platform | `dm-shell` (the native host, `--register`, the one-instance lock), `dm-desktop-base` (the system's services), `browser/`, `extension/` |

## Where the code is today (2026-11-09, on main)

| Layer | In place | Still in the window crate (to move) |
|---|---|---|
| Domain and engine | All of dm-core: no GUI types (enforced) | — |
| Application | `dm_shell::app`: `power` (WhenDone, Report), `platform` (Platform trait), `changes` (change fan-out), `snapshot` (Snapshot), `rules` (go_on, already_listed, worth_keeping, Dup, link_expired, file_gone), `backend` (Backend: the queue, the settings in force and their coordination, every verb on rows), `control_api` (who may ask, what is never done over the port, `batch`, queue.list, downloads.pause/resume/cancel, the checks of downloads.add and downloads.speed_limit), `detect` (Detector, DryWatch), `schedule` (the schedule's window, work left, rows each schedule looks after), `links`, `search`, `watch`, `handoff` (where a download handed over goes), `commands` (the registry: every command's name, label and keys, which apply to the rows chosen, and the commands that are the application's alone), `names` (what a row is called and where its file will go), `import`, `files` (extraction, a ZIP's files, a file's digest), `lookups` (what a server says of a file, a page's links, a video page's qualities, a playlist), `update` (the update check and the site rules), `events` (typed download events for any number of subscribers), and `session` (Session: each read and what follows from it, the schedules' tick, When done and its countdown, delete-after, the watched folder, the hand-offs); dm-shell's settings, host, inbox | The wording of what the Session reports, the dialogs it asks to open, the egui-only control methods (`ui.*`, and `command.run`'s commands that open a dialog or change the view) |
| The desktop program, not drawn | dm-desktop-base (2026-11-10, branch `gpui-migration`, `docs/gaps.md` G2) | — |
| Presentation | dm-gpui, the window shipped (macOS 2026-11-10, Windows and Linux 2026-11-11); dm-desktop, the older egui window, until G9 | — |

Steps 1 to 16 of [audit.md](audit.md) section 7 are done, each held to
`cargo test --workspace` and the whole behaviour contract. A presentation
drives the application in this order each tick (the GPUI window in
`Downloads::read` and `Downloads::tick`, `dm-gpui/src/list.rs` and `tick.rs`;
`--headless` in `dm_desktop_base::headless`; the egui window in
`DmApp::refresh` and `DmApp::logic`; `session.rs`'s `no_window_tests` with no
window at all):

1. `changes::take`; the inbox's order files (When done, a new download) are
   taken; then `Session::read` (rows, what happened, the command run on each
   finished file).
2. `Session::schedule_tick` for that read, then `Session::ran_dry` once for
   it, and `Session::when_done_fires` when it ran dry (the schedules first:
   a schedule that starts downloads on this read keeps the queue from
   counting as run dry).
3. `Hands::next` (on `session.hands`) for each download handed over.
4. `Session::countdown_due`, `session::delete_due` (which asks for another
   read when it removed rows), `Session::look_at_watched`.
5. Control requests: `control_api::refused`, then `control_api::answer`
   (and `batch`), with the presentation answering what is its own.

The GPUI window runs the tick once per wake. The egui window runs
`DmApp::logic` from `Window::logic` on every pass of eframe's loop, painted or
not (so the tick goes on while the window is in the tray), and again from
`DmApp::ui` on a painted pass, because its window tests drive `DmApp::ui`
alone; the second run reads only what changed in between. Between steps 3 and 5 the
window also drives the hints, saved searches, the clipboard watch and the
weekly update check, each decided by the application (`hints`, `search`,
`links`, `update`; G3, 2026-11-10): the window says and shows what they
decide, and starts their timers.

Headless (`--headless`) in the shipped program is `dm_desktop_base::headless`:
no window and no toolkit, settling what a window would ask as the egui
window's headless mode does. The egui binary's `--headless` still runs
`DmApp::logic` against a stand-in `egui::Context` (decided 2026-11-08,
[decisions.md](../decisions.md)): its `command.run` and `ui.*` answers stay
exactly as they are, and every rule it follows is the application's (what stays in `DmApp` is view state and
wording: `docs/decisions.md`, 2026-11-10). `Command` is the application's (step 15): a presentation runs the same
commands, checks them with `commands::applies` and carries out the
application's with `commands::verb`. What a row is called, imports, the
clipboard watch and extraction are the application's too (step 16), and a
row's secrets are taken out by the row itself (`Entry::redacted`, step 14).
The window makes no request of a server itself, nor touches the engine's
queue: every fetch that is not a download (a site crawled, the update check,
a file's name and size, a page's links, a video's qualities, a playlist, a
ZIP's files) is the application's, with the client the settings make
(`Backend::client`), and `cargo xtask layers` plus a search for `reqwest` in
either window's crate find none (step 17). Typed events, their delivery policy and their
cost at 10,000 rows are in [EVENT_FLOW.md](EVENT_FLOW.md).

## What does not change

- The engine: reused, not rewritten. Its behaviour is fixed by
  [behaviour-contract.md](../behaviour-contract.md); every step is held to the
  whole of it (`scripts/human-qa.py`, 64 rows) and to `cargo test --workspace`.
- The window's look and wording: no visual change comes with any step.
- Public paths: each move leaves a `pub use` at the old path, so callers and
  tests change only when they choose to.

## Known coupling left in dm-core

- `Queue::new` takes a `reqwest::Client` and dm-core re-exports `reqwest`;
  `Error::Network` wraps `reqwest::Error`. A presentation never needs to name
  them; the shell does, and the window does not (the update check and a ZIP's
  listing moved to `dm_shell::app`, step 17; `pub use reqwest` kept,
  decisions.md 2026-11-09).
- `Entry` carries a row's secrets (cookies, headers, a form's body), which
  its own server needs. A reader that shows or sends rows takes
  `Entry::redacted()` (the control port's `queue.list` does).
