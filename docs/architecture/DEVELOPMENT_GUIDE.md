# Where new code goes

A short guide for people and coding agents adding to dm without blurring its
layers. The layers are in [ARCHITECTURE.md](ARCHITECTURE.md); what each module
may depend on is in [MODULE_BOUNDARIES.md](MODULE_BOUNDARIES.md).

## Ask these in order

1. **Does it fetch, parse, or store downloads?** It is the engine: `dm-core`.
   Example: a new stream format, a retry rule for a server's answer.
2. **Is it a rule about what dm does that more than one place, or another
   front end, would need?** It is the application: `dm_shell::app`.
   Example: whether a row can be resumed (`rules::go_on`), what When done
   means, a new kind of change to tell presentations of.
3. **Is it how something looks, or what a person clicks?** It is the
   presentation: `dm-desktop`. It asks the application to act and words the
   answer itself.
4. **Does it need the computer itself (a notification, the clipboard, the
   tray, power)?** Declare what the application needs on `Platform` (or
   another trait in `dm_shell::app`); implement it in the desktop.

## Examples

**A new decision about rows** (say, "can this row be moved to another queue?"):
add a pure function to `dm_shell::app::rules` returning an enum, with a test
per state; call it from the menu, the dialog and the control port, each
reacting its own way. `go_on` and its five callers are the pattern.

**Something new the window must show** (say, a row's average speed): if the
queue already knows it, add it to `Snapshot` and fill it in
`Backend::snapshot`; the window reads it there. Never read the queue from a
drawing function.

**A new control-port or MCP method**: today it goes in
`dm-desktop/src/app/control.rs` beside the others; once that moves (audit step
8), UI-free methods go in the application and only `ui.*` methods stay in the
window.

**Something the window must react to at once**: it is told through
`dm_shell::app::changes`; subscribe, signal, and read on the next frame.
Never send data through a subscriber or block in one.

## Holding it

- `cargo test --workspace` runs `xtask layers`: a window toolkit in dm-core or
  dm-shell fails it.
- Every change to `desktop/`, `browser/` or `extension/` is held to the whole
  behaviour contract (`python3 scripts/human-qa.py`) before it is merged.
- Moves keep the old path with `pub use`; delete the shim only when nothing
  uses it.
