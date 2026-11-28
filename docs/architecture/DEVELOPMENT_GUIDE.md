# Where new code goes

A short guide for people and coding agents adding to EDM without blurring its
layers. The layers are in [ARCHITECTURE.md](ARCHITECTURE.md); what each module
may depend on is in [MODULE_BOUNDARIES.md](MODULE_BOUNDARIES.md).

## Ask these in order

1. **Does it fetch, parse, or store downloads?** It is the engine: `edm-core`.
   Example: a new stream format, a retry rule for a server's answer.
2. **Is it a rule about what EDM does that more than one place, or another
   front end, would need?** It is the application: `edm_shell::app`.
   Example: whether a row can be resumed (`rules::go_on`), what When done
   means, a new kind of change to tell presentations of.
3. **Is it how something looks, or what a person clicks?** It is the
   presentation: `edm-gpui`, the window people install. It asks the
   application to act and words the answer itself. What the program needs
   that is not drawing (words, numbers, the system's services) goes in
   `edm-desktop-base`.
4. **Does it need the computer itself (a notification, the clipboard, the
   tray, power)?** Declare what the application needs on `Platform` (or
   another trait in `edm_shell::app`); implement it in the desktop.

## Examples

**A new decision about rows** (say, "can this row be moved to another queue?"):
add a pure function to `edm_shell::app::rules` returning an enum, with a test
per state; call it from the menu, the dialog and the control port, each
reacting its own way. `go_on` and its five callers are the pattern.

**Something new the window must show** (say, a row's average speed): if the
queue already knows it, add it to `Snapshot` and fill it in
`Backend::snapshot`; the window reads it there. Never read the queue from a
drawing function.

**A new command** (a menu item, a button, a key): add it to
`edm_shell::app::commands` (its name in `Command::ALL` and `id`, its English
label, its `Chord`s, when it applies in `applies`, and, where it needs no
dialog, what it does in `verb`). The window's menus give it words from its
label (`edm-gpui/src/window/menus.rs`), and the window carries it out or opens its
dialog in `Downloads::run` (`edm-gpui/src/window/list.rs`); the control port and MCP
reach it by its name with nothing more.

**A new control-port or MCP method**: a UI-free one goes in
`edm_shell::app::control_api` (`answer`, with its check in `refused` if a
person alone may call it); only `ui.*` methods and `command.run`'s dialogs are
the window's (`edm-gpui/src/probe.rs`, through
`edm_desktop_base::probe::WindowProbe`).

**Something asked of a server that is not a download** (a page read, a size
asked, a list fetched): a function in `edm_shell::app::lookups` (or `search`,
`update`, `files`) that takes the `Backend`, uses `Backend::client` (the proxy
and timeout in force), runs on `backend.runtime`, and gives its answer to a
callback. The window keeps the slot it shows and asks for a repaint in the
callback. Never build a client or spawn a request in a window's crate.

**Something the window must react to at once**: it is told through
`edm_shell::app::changes`; subscribe, signal, and read on the next wake.
Never send data through a subscriber or block in one.

**What happened to each download** (for a script, a second front end, a
test): `session.events.subscribe(wake)` and drain the `Subscription` after a
read: lifecycle events in order, progress coalesced (EVENT_FLOW.md).

## Holding it

- `cargo test --workspace` runs `xtask layers`: a window toolkit in edm-core or
  edm-shell fails it.
- Every change to `crates/`, `assets/` or `extension/` is held to the whole
  behaviour contract (`python3 scripts/human-qa.py`) before it is merged.
- Moves keep the old path with `pub use`; delete the shim only when nothing
  uses it.
