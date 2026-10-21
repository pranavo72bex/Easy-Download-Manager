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
