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
| Application | `app::power` (WhenDone, Report), `app::platform` (Platform trait), `app::changes` (change fan-out), `app::snapshot` (Snapshot), `app::rules` (go_on); dm-shell's settings, host, inbox | `Backend` (`dm-desktop/src/backend.rs`, no egui in it); change detection and its side effects in `DmApp::refresh` (`app/sync.rs`); schedule hooks (`app/scheduled.rs`); the control port's UI-free methods (`app/control.rs`); timers (countdown, delete-after, searches, watched folder, clipboard); duplicate and admission rules |
| Presentation | dm-desktop | — |

The rest moves after `parity-with-reference-a` ships, on a branch rebased on
`main`, in the order and steps of [audit.md](audit.md) section 7 (steps 3 and
7 to 13): the files they touch are the ones still changing on that branch.

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
