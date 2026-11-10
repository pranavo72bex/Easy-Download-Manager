# How a change reaches the screen

From the engine to whatever presentation draws it, as the code does it today.

```text
engine (dm-core runner)            application (dm_shell::app)         presentation (egui window)
───────────────────────            ───────────────────────────         ──────────────────────────
progress bytes ──► row's Slot (no signal; read on the next poll)
state change ──► Queue::save ──► Queue::on_change ──► changes::changed()
                                                       │ CHANGED = true
                                                       └─► each subscriber ──► request_repaint_after(250 ms)
                                                                              │
                                                       frame: logic() ◄───────┘
                                                       read now? (CHANGED, or 250 ms since the last read)
                                                       changes::take()
                                                       Session::read: Backend::snapshot() ──► Snapshot {rows, retrying, swarms, allowance_spent}
                                                         ├─► detect::Detector: what finished / failed / started (the window's toasts, sounds, dialogs)
                                                         └─► events::Events::publish ──► each Subscription (typed DownloadEvents)
                                                       refresh(): react, then draw
```

## Policies

- **What is told, and what is read.** A change is told as a mark and a wake-up,
  never as data. The presentation reads a `Snapshot` when it next draws. A
  thousand changes in a quarter-second cost one read; no change is lost to a
  full channel; no subscriber can fall behind or hold the engine.
- **Subscribers never block.** `changes::subscribe` callbacks run on the
  engine's threads, many times a second while downloads run. They only signal
  (set a flag, request a repaint, wake a loop).
- **Clear before reading.** `changes::take` (the window: `QUEUE_CHANGED`) is
  cleared before the queue is read, so a change made during the read is still
  owed a read afterwards. Reversing the order loses changes (a resumed download
  stayed "Stopped" until it finished, decisions.md).
- **Progress is polled.** Byte counts change hundreds of times a second and
  raise no signal. While anything is running or queued the window reads every
  250 ms; idle, only when told. One read per quarter-second keeps a window with
  10 downloads at about a tenth of a core (a repaint per change took 14.6 %,
  main.rs).
- **Snapshots share, never copy.** `Snapshot::rows` is `Vec<Arc<Entry>>`: n
  pointer copies per read, and a row's own bytes only when it changed.

## Typed events (`dm_shell::app::events`)

For a presentation, a script or a test that wants to know what happened to
each download rather than draw the list, `session.events.subscribe(wake)` gives
a `Subscription`, drained when it likes, of `DownloadEvent`s: `Added`,
`StateChanged`, `Completed`, `Failed` (the engine's words), `Removed` and
`Progress`. The egui window does not subscribe: it reacts to the same read's
`Read::detected` (toasts, sounds, dialogs). The events are for a second
presentation, scripts and tests (`session.rs`'s tests subscribe). They come from the same reads as everything else (no read, no
events; nothing polls the engine), each read compared with the last.

- **Lifecycle events are never dropped.** Each subscription keeps them in
  order until it drains them. A subscription that is dropped stops collecting
  at once; one kept and never drained keeps them all, which is its owner's to
  avoid.
- **Progress is coalesced.** One entry a row, the newest since the last drain,
  however many reads came between: a subscriber's memory for it is bounded by
  the number of rows, not by how often bytes arrived.
- **Subscribers are independent and cannot block.** Publishing appends under
  each subscription's own lock and calls its `wake` after letting go; `wake`
  only signals, as a `changes` subscriber does.
- **Unchanged rows cost a pointer check.** The queue hands out the same
  `Arc<Entry>` for a row that did not change, so a read of 10,000 rows where a
  few moved compares a few. The map of what was last seen is updated in place.
- **Nobody listening costs nothing.** With no subscription the publisher keeps
  nothing; the first read after one subscribes is its starting point (it is
  not told of what was already there).

## Cost at large queues

Measured 2026-11-09 on macOS in release (`cargo test -p dm-shell --release --lib
large_queue -- --ignored --nocapture`), four downloads running against a server
that never finishes them:

| Rows | Per tick (read, dry look, schedules), nobody subscribed | A read with two subscribers | Move a row | Pause and resume a running row | `queue.list` |
|---|---|---|---|---|---|
| 100 | | 0.04 ms | 0.7 µs | 9 µs | |
| 1,000 | 0.33 ms | 0.23 ms | 0.6 µs | 6 µs | 10 ms |
| 10,000 | 2.8 ms | 2.9 ms | 3 µs | 24 µs | 96 ms |

`queue.list` serializes every row for the control port: a script's call, not a
frame. The first measure of the events rebuilt the map of rows at every read
and took 4.2 ms at 10,000; updated in place, and skipped with nobody
subscribed, it is the figures above. The quick version of the load test (100
rows) runs with every `cargo test`.

Earlier, for the window as a whole:

Measured: a window with 5,060 rows and 60 active downloads answered a call in
a median 3 ms, worst 19 ms (Linux, `stress-check.py window`); 20,000 listed
links slow small downloads to about 10 files a second, the cost of rewriting
the queue file (decisions.md 2026-10-27). Per read the window does about five
passes over the rows, all O(n) except an "added again" check that is O(new ×
n). The control port's `ui.inspect` grows with the square of the list (about
7 s at 5,000 rows): a tool for checks, not a path a person waits on.

## What a second presentation does

1. `dm_shell::app::changes::subscribe(|| /* wake my loop or request a frame */)`,
   and, for what happened to each download, `session.events.subscribe(..)`.
2. On a wake (or every 250 ms while something runs): `changes::take()`, then
   `Session::read` (the snapshot, what was detected, the events published),
   then draw from the snapshot and react to the events.
3. Act through the application: `commands::applies` and `commands::verb` for
   the commands, `Backend`'s verbs for the rest; ask servers through
   `lookups`, `search`, `update` and `files`, never itself.
