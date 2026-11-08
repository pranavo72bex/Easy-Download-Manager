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
                                                       changes::take() (QUEUE_CHANGED.store(false))
                                                       Backend::snapshot() ──► Snapshot {rows, retrying, swarms, allowance_spent}
                                                       refresh(): what finished / failed / started, then draw
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

## Cost at large queues

Measured: a window with 5,060 rows and 60 active downloads answered a call in
a median 3 ms, worst 19 ms (Linux, `stress-check.py window`); 20,000 listed
links slow small downloads to about 10 files a second, the cost of rewriting
the queue file (decisions.md 2026-10-27). Per read the window does about five
passes over the rows, all O(n) except an "added again" check that is O(new ×
n). The control port's `ui.inspect` grows with the square of the list (about
7 s at 5,000 rows): a tool for checks, not a path a person waits on.

## What a second presentation does

1. `dm_shell::app::changes::subscribe(|| /* wake my loop or request a frame */)`.
2. On a wake (or every 250 ms while something runs): `changes::take()`, then
   `Backend::snapshot()`, then draw.
3. Act through the application (`Backend`'s verbs; `rules::go_on` for Resume).
