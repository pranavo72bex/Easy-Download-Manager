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
as asked (AccessKit panics on a window already shown); the root made again later gets
one the same way, and only the root's adapter may turn AccessKit off for all. The same on
every system (`ADAPTER_PER_WINDOW`). Checked: `scripts/linux-gui-check.sh` asks AT-SPI
alone for Add URL's OK and Cancel, inside the dialog's window; on Windows
`scripts/windows-a11y-check.py` asks UI Automation (what Narrator reads) for the buttons of
Add URL, Options, the Scheduler, the Speed limiter and About, each inside its own window
(5 of 5 on 2026-11-08; with the copy as published, none of Add URL's). MIT OR Apache-2.0,
as published.

## gpui-pre-linux 0.3.12

Licence Apache-2.0 (its `LICENSE-APACHE`); the version the GPUI window
(`crates/dm-gpui`, pinned `gpui-pre` 0.3.12) is built with on Linux. The
published package's `Cargo.lock` and `.cargo_vcs_info.json` are left out.

`src/linux/x11/window.rs` (searched for `dm:`): AccessKit is told where an X11
window stands on the screen when its adapter starts and each time the window
moves or is resized (`X11WindowStatePtr::dm_a11y_bounds`, called from
`set_bounds` and `a11y_init`; `a11y_update_window_bounds` now calls it too).
Upstream defines that update but nothing calls it, so AT-SPI gave every
control its place inside the window as if the window stood at the screen's
top-left corner: a screen reader's pointer and a click at a control's place
missed it (`scripts/linux-gui-check.sh` with `DM_WINDOW=gpui` clicked Add URL
at (8, 39) and opened nothing, 2026-11-11). Wayland does not tell a program
where its window is, so the Wayland window is unchanged.

The same file's `set_bounds`: where a configure event resizes the window, its place is asked of
the X server (`TranslateCoordinates`) rather than kept as it was. Upstream keeps the old place,
since such an event's own often counts from the window manager's frame; openbox, keeping a
resized window on the screen, moved it 13 points up, and the menu titles' places (for a screen
reader, and a click aimed from them) and the window's own `bounds()` stayed 13 points low.

Links dropped on a window (`src/linux/x11/client.rs`, `SelectionNotify`, and
`src/linux/wayland/client.rs`, the data device's drop): a file's address
becomes its path, as upstream; any other line of the drop (a link dragged
from a browser, as `text/uri-list` or, on X11, as text) is handed on as the
dropped "path" it says, which dm reads as a link
(`dm_shell::app::links::links_at`). Upstream kept file addresses alone, so a
link dragged from Firefox onto the window did nothing. Checked with real
drags: `scripts/linux-drag-check.sh` (X11) and
`scripts/linux-wayland-drag-check.sh`. On X11 the drop's data is read whole,
in pieces (upstream read its first 4 KiB), and the drop's place is in the
window's points (upstream left it in pixels, twice as far on a 2x screen).

Files dragged out of a window (`src/linux/x11/drag_out.rs`, new, and `dm:`
lines in `src/linux/x11/window.rs` and `client.rs`): upstream hands an
element's drag to the system once it leaves the window
(`PlatformWindow::start_external_drag`) on macOS and Wayland, and on X11
declines it. Here X11 starts it too: the press's grab on the window's
connection is let go of, and a connection of its own, on a thread, speaks
XDND as the drag's source (the egui window's `dm-desktop` `xdnd.rs`, moved
here), handing the files over as `text/uri-list`; when it is over it sends
the window `_DM_DRAG_ENDED`, which ends GPUI's drag as the other systems'
end does. On Wayland (`src/linux/wayland/client.rs`) the drag offers a copy
only, where upstream offered a move as well: a file manager taking a move
would carry the download away from where dm saved it. Owed to the Linux
pass: `scripts/linux-drag-check.sh` and `scripts/linux-wayland-drag-check.sh`
dragging out of the GPUI window (docs/gaps.md G10).

## gpui-pre-windows 0.3.12

Licence Apache-2.0 (its `LICENSE-APACHE`); the version the GPUI window
(`crates/dm-gpui`, pinned `gpui-pre` 0.3.12) is built with on Windows. The
published package's `Cargo.lock` and `.cargo_vcs_info.json` are left out.

Files dragged out of the window (`src/drag_out.rs`, and `dm:` lines in
`gpui_windows.rs`, `window.rs`, `events.rs`): upstream offers GPUI's
`start_external_drag` on macOS and Wayland only, so a finished download could
not be dragged from the Windows window into Explorer, a mail or a chat (dm's
gaps row G10). The window now takes it: the files become shell items, the
items a data object as Explorer's own (`BHID_DataObject`), and the shell's
`SHDoDragDrop` makes the drag with its own drop source, copy or link, never
move. That call holds a modal loop until the drop lands, so it is made from a
message posted to the window (`WM_DM_DRAG_OUT`), outside any update of GPUI's,
and GPUI is then told the drag is over (`FileDropEvent::Ended`), as the X11
copy does.
