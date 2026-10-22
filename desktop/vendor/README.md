# Crates dm builds with a change of its own

Each is the published crate as it is on crates.io, with one change, used in
place of it through `[patch.crates-io]` in `desktop/Cargo.toml`. When the
crate is upgraded, its change is carried over or, once the crate does it
itself, the copy is removed.

## winit 0.30.13

X11 drops (`src/platform_impl/linux/x11/dnd.rs`, `event_processor.rs`,
`atoms.rs`): a link dragged from a browser is handed over as the dropped
"path", for dm to read as a link (`dm-desktop` `app::dropped_links`).
Upstream winit takes `text/uri-list` of `file://` addresses only, and turned
a link away twice: an `https://` address in `text/uri-list` (Chrome's) failed
the whole drop, and Firefox, which offers a link as `_NETSCAPE_URL` and
UTF-8 text but not as `text/uri-list`, was refused at once. Now the drag is
asked for `text/uri-list` where it is offered, else `_NETSCAPE_URL`, else
UTF-8 text, and a link's address is passed on. Files are as before. Search
the files for `dm:`. Checked with real input: `scripts/linux-drag-check.sh`.

Wayland drag and drop (`src/platform_impl/linux/wayland/dnd.rs`, and `dm:`
lines in `state.rs`, `seat/mod.rs`, `event_loop/mod.rs`, `mod.rs`,
`src/platform/wayland.rs`): upstream winit 0.30 has none on Wayland. A drop
is read as `text/uri-list`, Firefox's `text/x-moz-url` or text, and each
item handed over as a dropped "path" as on X11. A drag out
(`platform::wayland::dm_request_drag_out`) is started on winit's own
connection from the window under the pointer, with the press's serial,
which only that connection holds, and hands the target `text/uri-list`;
`dm_take_drag_ended` tells dm the button is up. Checked with a real
Wayland drag both ways: `scripts/linux-wayland-drag-check.sh`.
