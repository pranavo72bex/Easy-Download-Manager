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

## egui-winit 0.37.0

Licence MIT OR Apache-2.0 (its `Cargo.toml`); the version eframe 0.37.0 uses.
The `Cargo.lock` of the published package is left out; `src/lib.rs` is the
only file changed.

`src/lib.rs` (`OTHER_WINDOWS_INPUT`, searched for `dm:`): a function the
application sets once, called with the input of every window but the first as
`State::take_egui_input` hands it to egui. eframe offers the first window's
input to the app (`App::raw_input_hook`) and the others' to nobody; dm draws
each dialog as a window of its own (`dialogs/frame.rs`), and the clicks, keys
and typing a control client sends (`ui.click`, `ui.key`, `ui.type`) have to
reach them as a person's would, through the input of the window they are for
(`app/human.rs`). Nothing else is changed.

## librqbit 9.0.1

Encrypted peer connections (`src/mse.rs`, and `dm:` lines in
`peer_connection.rs`, `session.rs`, `torrent_state/mod.rs`,
`torrent_state/live/mod.rs`, `lib.rs`, `Cargo.toml`): BitTorrent's Message
Stream Encryption (MSE/PE: a 768-bit Diffie-Hellman exchange, then RC4), which
upstream librqbit does not have (dm's gaps row 150). A peer that hangs up on a
plaintext handshake is asked again encrypted, so one that takes only
encrypted connections is reached; a connection that comes in encrypted is
answered for whichever of the session's torrents it names, and a plaintext one
is passed through. `PeerConnectionOptions::require_encryption` (off in dm)
makes and takes encrypted connections only; the tests use it for a seeder that
refuses plaintext. MSE hides BitTorrent from simple traffic shaping and
reaches peers that insist on it; with RC4 and a 768-bit group it is not
strong encryption, and dm does not present it as such.

Also fixed on the way, in `session.rs` `task_listener`: a check of an
incoming connection that failed disabled the branch that collects the checks
(the pattern `Some(Ok(..))` in `tokio::select!`), so the checks still under
way were not polled until another connection came in; an encrypted handshake,
which takes several round trips, stalled behind any refused connection.

A torrent's file names checked before any file is made (`dm:` lines in
`src/storage/filesystem/fs.rs`, `init` and `plain_relative`): only plain parts, no root,
no `..`, no drive and, on Windows, no `:`. On Windows a name such as `D:x` made
`PathBuf::push` start again on drive D:, so the storage made empty files outside the
torrent's folder before dm's own check refused the torrent (dm's gaps row 159 b). Tests:
`dm_tests` in that file (`cargo test --lib dm_tests` here) and dm-core's
`tests/torrent_names.rs`.

Left out of the copy: upstream's `webui/CLAUDE.md`, notes for an AI assistant
working on its web interface (built only with the `webui` feature, which dm
does not use), which would otherwise read as instructions for this project.

Checked against another implementation, libtorrent 2.0.8 (the library most
BitTorrent clients are built on) set to make and take only encrypted
connections, both ways (`scripts/mse-interop.sh`,
`dm-core/tests/mse_interop.rs`), and in `dm-core/tests/torrent_features.rs`
`a_seeder_that_takes_only_encrypted_connections_is_reached`. The protocol's own
tests are in `src/mse.rs` (run from `desktop/vendor/librqbit` with
`cargo test --lib mse`, as the crate is not a workspace member).

## eframe 0.37.0

Each further window's own AccessKit adapter (`src/native/glow_integration.rs`, search
for `dm:`). Upstream starts an adapter for the root window only, so since every dialog
of dm's became a window of its own (contract row B42) no dialog reached a screen
reader: AT-SPI (Orca) found the main window's controls and none of Add URL's, Options'
or the close question's (gaps row 160). The proxy given to the root's adapter is kept,
and a window made after it is created hidden, given an adapter of its own, then shown
as asked (AccessKit panics on a window already shown). Not on Windows, whose build is
as published (`cfg(not(target_os = "windows"))`). Checked: `scripts/linux-gui-check.sh`
asks AT-SPI alone for Add URL's OK and Cancel. MIT OR Apache-2.0, as published.
