# dm's architecture

dm is layered so that the window is one presentation of an application that
does not know about it: the egui window today, another (GPUI) later, without
changing the engine or the application's rules. This page says what the layers
are, where they live, and how far the code has moved towards them. The audit
behind it is [audit.md](audit.md); the rules for each module are in
[MODULE_BOUNDARIES.md](MODULE_BOUNDARIES.md); how a change reaches the screen is
in [EVENT_FLOW.md](EVENT_FLOW.md); where new code goes is in
[DEVELOPMENT_GUIDE.md](DEVELOPMENT_GUIDE.md).

## The layers, on the real crates

```text
┌──────────────────────────────────────────────────────────────┐
│ Presentation: dm-desktop (egui)                              │
│   the window, dialogs, tray, toasts, drag and drop, and the  │
│   desktop's Platform (clipboard, notifications, power)       │
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

A second presentation (GPUI) depends on dm-shell and dm-core only. It
implements `dm_shell::app::platform::Platform`, subscribes to
`dm_shell::app::changes`, reads `Snapshot`s, and asks the application to act.

## Where the code is today (2026-11-08, branch ui-independent-architecture)

| Layer | In place | Still in the window crate (to move) |
|---|---|---|
| Domain and engine | All of dm-core: no GUI types (enforced) | — |
| Application | `dm_shell::app`: `power` (WhenDone, Report), `platform` (Platform trait), `changes` (change fan-out), `snapshot` (Snapshot), `rules` (go_on, already_listed, worth_keeping, Dup, link_expired), `backend` (Backend: the queue, the settings in force, every verb on rows), `control_api` (queue.list, downloads.pause/resume/cancel, the checks of downloads.add and downloads.speed_limit), `detect` (Detector: finished, failed, expired, started elsewhere, added again), `schedule` (the schedule's window, work left, rows each schedule looks after); dm-shell's settings, host, inbox | The per-tick work of `DmApp::logic` (countdown, delete-after, saved searches, watched folder, clipboard, hand-off routing), the reactions to `Detected` (worded by the window), settings coordination (`DmApp::apply_settings`), and headless mode, which still runs `DmApp::logic` with a dummy `egui::Context` |
| Presentation | dm-desktop | — |

Steps 1 to 10 of [audit.md](audit.md) section 7 are done, each held to
`cargo test --workspace` and the whole behaviour contract. Left, after
`parity-with-reference-a` ships: step 11 (the per-tick work into an
application `Session` that reports what to say or open, each presentation
wording it), step 12 (settings coordination) and step 13 (headless with no
window toolkit). Step 13 waits on one decision: whether a headless
`command.run` keeps working through a window-free command executor
(recommended) or is dropped.

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
  them; the shell and window still do.
- `Entry` carries a row's secrets (cookies, headers, a form's body). Readers
  that show or send rows must redact them (the control port's `queue.list`
  does); an `Entry::redacted()` would make that impossible to forget.
