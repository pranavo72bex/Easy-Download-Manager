# Crates dm builds with a change of its own

Each is the published crate as it is on crates.io, with dm's change, used in
place of it through `[patch.crates-io]` in `desktop/Cargo.toml`. When the
crate is upgraded, its change is carried over or, once the crate does it
itself, the copy is removed.

Every copy is made the same way (gaps rows P3, A17 and X5): the published
crate, checked against its SHA-256, with what dm's build does not use removed
and `patches/<crate>.patch` applied. `cargo xtask vendor` makes all five again
(`cargo xtask vendor gpui-pre-linux` one), the same on Windows, macOS and
Linux (`desktop/xtask/src/vendor.rs`, which holds each crate's version and
SHA-256 and what is removed from it; it needs `curl`, `tar` and `git`, which
Windows 10 and later have, `git` from Git for Windows). Nothing from the
crates is run, and run on a clean tree, `git status` then shows no change.
The crates are unpacked in `DM_VENDOR_WORK` where that names a folder, and
otherwise in a new folder of the system's own, removed after.
Each patch explains its hunks at its head, where `git apply` skips the text.

Each week CI tries the next GPUI snapshot (`.github/workflows/vendor-next.yml`):
where crates.io has a `gpui-pre` newer than the one pinned, it makes both GPUI
copies from it with `cargo xtask vendor --version <new> gpui-pre-linux
gpui-pre-windows` (`--version` takes a version that has no SHA-256 held, and
says so) and checks `dm-gpui` on Linux with it pinned; its summary says whether
each patch still applies and whether the build passes. It commits nothing:
moving to the new snapshot is the change below, made by hand.
Each change in a copy is marked in its code `dm(<tag>):`, the tag one row of
the table below; `cargo xtask layers` (`desktop/xtask/src/vendor_marks.rs`)
fails on a mark with no tag, a tag with no row and a row no mark names.

## Every change

The other systems: why a system that builds another copy, or the published
crate, does not need the change. Upstream told: whether the change has been
offered to the crate's makers.

| Tag | The change | Copy | Marks | The other systems | Upstream told |
| --- | --- | --- | --- | --- | --- |
| `x11-a11y` | AccessKit told where the X11 window stands on the screen, when its adapter starts and on each move or resize; upstream defines the update and nothing calls it | gpui-pre-linux | `src/linux/x11/window.rs`, 4 | Wayland tells a program nothing of its window's place; on macOS and Windows AccessKit's adapter asks the system where the window is | not yet |
| `x11-place` | Where a configure event resizes the window, its place asked of the X server rather than kept | gpui-pre-linux | `src/linux/x11/window.rs`, 1 | X11's configure events alone; Wayland has no place, macOS and Windows tell the window its place | not yet |
| `links` | A dropped line that is not a file's address (a link dragged from a browser) handed on as the "path" it says, which dm reads as a link; upstream dropped it | gpui-pre-linux | `src/linux/x11/client.rs`, `src/linux/wayland/client.rs`, 1 each | macOS and Windows: dm takes links off the drag itself (its own drag methods and OLE drop target, `dm-gpui` `platform/drops.rs`) | not yet |
| `x11-drop-all` | A drop's data read whole, in pieces; upstream read its first 4 KiB | gpui-pre-linux | `src/linux/x11/client.rs`, 1 | XDND's property read is X11's alone | not yet |
| `x11-drop-pt` | A drop's place in the window's points, not pixels | gpui-pre-linux | `src/linux/x11/client.rs`, 1 | X11's alone: Wayland's drop is in points already, and on macOS and Windows dm takes drops itself | not yet |
| `x11-drag-out` | Files dragged out of the window on X11 (`start_external_drag`), XDND spoken as the drag's source | gpui-pre-linux | `src/linux/x11/drag_out.rs` (new), `x11.rs`, `x11/client.rs`, `x11/window.rs`, 5 | macOS and Wayland: upstream has it; Windows: `win-drag-out` | not yet |
| `wl-copy` | A drag out on Wayland offers a copy only; upstream offered a move too | gpui-pre-linux | `src/linux/wayland/client.rs`, 1 | macOS: upstream offers other programs a copy only; X11 and Windows: dm's own drags (`x11-drag-out`, `win-drag-out`) offer copy or link | not yet |
| `mica` | Mica behind the whole window, the frame spread over it (`DwmExtendFrameIntoClientArea`) | gpui-pre-windows | `src/window.rs`, 3 | Mica is Windows 11's backdrop; dm asks for it on Windows only | not yet |
| `theme` | The app's own Light or Dark (`set_window_appearance`, empty upstream on Windows), and each window told by `WM_DM_APPEARANCE_CHOSEN` | gpui-pre-windows | `src/events.rs` 3, `src/platform.rs` 1, `src/util.rs` 3 | macOS: upstream has it. Linux needs it too and has not got it: upstream's Linux platform leaves it empty as well, so Options › General › Theme does nothing there | not yet |
| `win-drag-out` | Files dragged out of the window on Windows, made by the shell (`SHDoDragDrop`) from a posted message | gpui-pre-windows | `src/drag_out.rs` (new), `events.rs` 2, `gpui_windows.rs` 1, `window.rs` 1 | macOS and Wayland: upstream has it; X11: `x11-drag-out` | not yet |
| `mse` | Encrypted peer connections (Message Stream Encryption) | librqbit | `Cargo.toml` 1, `src/mse.rs` (new), `lib.rs`, `peer_connection.rs` 4, `session.rs` 1, `torrent_state/mod.rs` 1 | One crate on every system | not yet |
| `listener` | A failed check of an incoming connection no longer stalls the others (`task_listener`) | librqbit | `src/session.rs`, 1 | One crate on every system | not yet |
| `names` | A torrent's file names checked before any file is made | librqbit | `src/storage/filesystem/fs.rs`, 2 | One crate on every system (the danger is Windows' drive names, the check is made everywhere) | not yet |
| `no-axum` | axum-extra and the dual-stack listener's axum support only with the HTTP API | librqbit | `Cargo.toml`, 1 | One crate on every system | not yet |
| `ring` | reqwest's rustls without aws-lc-rs; ring installed as the process's provider | librqbit | `Cargo.toml` 2, `src/session.rs` 1 | One crate on every system | not yet |
| `trim` | The `[[example]]` targets and the test that read `resources/` left out with the files removed | librqbit | `Cargo.toml` 1, `src/session.rs` 1 | One crate on every system | dm's alone: upstream keeps those files |
| `sha1` | `sha1-ring` backed by the pure-Rust sha1 and sha2 crates rather than aws-lc-rs | librqbit-sha1-wrapper | `Cargo.toml` 1, `src/lib.rs` 2 | One crate on every system | not yet |
| `mac-keys` | A system menu's items on macOS show the key bound to each one's action (the `NSMenuItem`'s key equivalent and modifiers), set as GPUI's own menu bar sets them; upstream's `NativeMenu` showed none | gpui-component | `src/native_menu/macos.rs`, 8 | Windows: dm writes the key after a tab in the item's words, which Windows' menus show at the item's right (`dm-gpui` `window/rowmenu.rs`); Linux: the drawn fallback menu shows the bound keys itself | not yet |

## librqbit and its SHA-1 wrapper (gaps row P3)

`librqbit/` and `librqbit-sha1-wrapper/` are the crates published on crates.io
at 9.1.0, with dm's change to each in `patches/librqbit.patch` and
`patches/librqbit-sha1-wrapper.patch`. Removed from each: `webui/` (the web
interface, built only with the `webui` feature, with upstream's notes for an
AI assistant in it), `resources/` (test torrents), `examples/` (and their
`[[example]]` targets, in the patch), the crate's own `Cargo.lock` (dm's is
`desktop/Cargo.lock`), `Cargo.toml.orig`, `.cargo_vcs_info.json` and
`.cargo-ok`: 95 files, 810 KB. Upstream's one test that read `resources/`
(`session.rs`, `test_torrent_file_from_info_and_bytes`) is left out by the
patch. The other `librqbit-*` crates (core, bencode, peer protocol, DHT,
trackers and the rest) are used from crates.io as published, at 9.1.0.

From 9.0.1 to 9.1.0 upstream fixed, among others: a nesting limit of 128 in
the bencode reader (librqbit-bencode), a piece offset out of the piece's
bounds in a stream's read (librqbit-core `lengths.rs`), a piece downloaded
again when its file was chosen anew during its hash check, a handshake that
came in more than one read, and a listener that panicked once every branch
of its `select!` was disabled (`session.rs` `task_listener`, `else =>
continue`, kept beside dm's change there, below). To move to another
version: change `LIBRQBIT_VERSION` and the SHA-256s in `xtask/src/vendor.rs`,
run `cargo xtask vendor librqbit librqbit-sha1-wrapper`,
mend any hunk `git apply` refuses, and write each patch again as the `git
diff` from the published crate (with the same files removed) to the mended
copy.

## librqbit 9.1.0

Encrypted peer connections (`dm(mse)`: `src/mse.rs`, and changes in
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

Also fixed on the way, in `session.rs` `task_listener` (`dm(listener)`): a
check of an incoming connection that failed disabled the branch that collects
the checks (the pattern `Some(Ok(..))` in `tokio::select!`), so the checks
still under way were not polled until another connection came in; an
encrypted handshake, which takes several round trips, stalled behind any
refused connection.

A torrent's file names checked before any file is made (`dm(names)` in
`src/storage/filesystem/fs.rs`, `init` and `plain_relative`): only plain parts, no root,
no `..`, no drive and, on Windows, no `:`. On Windows a name such as `D:x` made
`PathBuf::push` start again on drive D:, so the storage made empty files outside the
torrent's folder before dm's own check refused the torrent (dm's gaps row 159 b). Tests:
`dm_tests` in that file (`cargo test --lib dm_tests` here) and dm-core's
`tests/torrent_names.rs`.

No second crypto library and no web server (`dm(no-axum)` and `dm(ring)` in
`Cargo.toml` and `src/session.rs`; dm's gaps row P7). Upstream's `rust-tls`
feature asked for reqwest's `rustls`, which brings aws-lc-rs as rustls's
provider, and for the SHA-1 wrapper's `sha1-ring`, which is aws-lc-rs too: a C
library built with cmake, beside the ring that dm's own TLS uses. `rust-tls`
now asks for `reqwest/rustls-no-provider`, and `Session::new_with_opts` makes
ring the process's default provider when none is installed, so the clients
this session and its trackers build have one (a reqwest client built with
none panics). The SHA-1 wrapper is a patched copy of its own, below. And
`axum-extra` and `librqbit-dualstack-sockets`' `axum` feature, which only the
HTTP API (`http-api`, which dm does not turn on) uses, come with that feature
now, so dm builds no axum outside its tests. `cargo tree -i aws-lc-sys`,
`-i cmake`, `-i axum-extra` and `-e normal -i axum` (from `desktop/`, with
`--target all`) are empty.

Checked against another implementation, libtorrent 2.0.8 (the library most
BitTorrent clients are built on) set to make and take only encrypted
connections, both ways (`scripts/mse-interop.sh`,
`dm-core/tests/mse_interop.rs`), and in `dm-core/tests/torrent_features.rs`
`a_seeder_that_takes_only_encrypted_connections_is_reached`. The protocol's own
tests are in `src/mse.rs` (run from `desktop/vendor/librqbit` with
`cargo test --lib mse`, as the crate is not a workspace member).

## librqbit-sha1-wrapper 9.1.0

The SHA-1 and SHA-256 behind librqbit's piece checks and info hashes, under
the feature name librqbit and librqbit-core ask for, `sha1-ring` (`Cargo.toml`
and `src/lib.rs`, `dm(sha1)`; dm's gaps row P7). Upstream backs that
feature with aws-lc-rs, a C library built with cmake, a second crypto library
beside ring; here it is the pure-Rust `sha1` and `sha2` crates (RustCrypto),
which dm builds already. The `sha1` crate uses the processor's SHA
instructions where it has them (x86 SHA-NI, ARMv8 SHA). The other backend,
`sha1-crypto-hash` (the system's library), is as published. A test of the
FIPS 180 "abc" vector for SHA-1, fed in two parts, is added beside upstream's
SHA-256 one (`cargo test --features sha1-ring --no-default-features` here).
Licence Apache-2.0, as published.

## gpui-pre-linux 0.3.12

`gpui-pre-linux/` is the crate published on crates.io at 0.3.12 (Zed's
`gpui_linux` crate as the gpui-pre snapshot of zed@d2a4fb6 publishes it;
licence Apache-2.0, its `LICENSE-APACHE`), with dm's change in
`patches/gpui-pre-linux.patch` (14 hunks in five files, one of them new). It is
the version the GPUI window (`crates/dm-gpui`, pinned `gpui-pre` 0.3.12) is
built with on Linux. Removed: the crate's `Cargo.lock`, `.cargo_vcs_info.json`
and `.cargo-ok`; `Cargo.toml.orig` is kept. The other `gpui-pre-*` crates are
used as published. To move to another snapshot: change `GPUI_VERSION` and the
two GPUI SHA-256s in `xtask/src/vendor.rs` (with the version
`desktop/Cargo.toml` pins, which a test of that file holds them to), run
`cargo xtask vendor gpui-pre-linux gpui-pre-windows`, mend any hunk `git apply` refuses, and write each patch again as the
`git diff` from the published crate (with the same files removed) to the
mended copy, keeping the text at the patch's head.

`src/linux/x11/window.rs` (`dm(x11-a11y)`): AccessKit is told where an X11
window stands on the screen when its adapter starts and each time the window
moves or is resized (`X11WindowStatePtr::dm_a11y_bounds`, called from
`set_bounds` and `a11y_init`; `a11y_update_window_bounds` now calls it too).
Upstream defines that update but nothing calls it, so AT-SPI gave every
control its place inside the window as if the window stood at the screen's
top-left corner: a screen reader's pointer and a click at a control's place
missed it (`scripts/linux-gui-check.sh` with `DM_WINDOW=gpui` clicked Add URL
at (8, 39) and opened nothing, 2026-11-11). Wayland does not tell a program
where its window is, so the Wayland window is unchanged.

The same file's `set_bounds` (`dm(x11-place)`): where a configure event resizes the window, its
place is asked of the X server (`TranslateCoordinates`) rather than kept as it was. Upstream
keeps the old place, since such an event's own often counts from the window manager's frame;
openbox, keeping a resized window on the screen, moved it 13 points up, and the menu titles'
places (for a screen reader, and a click aimed from them) and the window's own `bounds()`
stayed 13 points low.

Links dropped on a window (`dm(links)`: `src/linux/x11/client.rs`, `SelectionNotify`, and
`src/linux/wayland/client.rs`, the data device's drop): a file's address
becomes its path, as upstream; any other line of the drop (a link dragged
from a browser, as `text/uri-list` or, on X11, as text) is handed on as the
dropped "path" it says, which dm reads as a link
(`dm_shell::app::links::links_at`). Upstream kept file addresses alone, so a
link dragged from Firefox onto the window did nothing. Checked with real
drags: `scripts/linux-drag-check.sh` (X11) and
`scripts/linux-wayland-drag-check.sh`. On X11 the drop's data is read whole,
in pieces (upstream read its first 4 KiB; `dm(x11-drop-all)`), and the drop's
place is in the window's points (upstream left it in pixels, twice as far on
a 2x screen; `dm(x11-drop-pt)`).

Files dragged out of a window (`dm(x11-drag-out)`: `src/linux/x11/drag_out.rs`, new, and
`x11.rs`, `src/linux/x11/window.rs` and `client.rs`): upstream hands an
element's drag to the system once it leaves the window
(`PlatformWindow::start_external_drag`) on macOS and Wayland, and on X11
declines it. Here X11 starts it too: the press's grab on the window's
connection is let go of, and a connection of its own, on a thread, speaks
XDND as the drag's source (the earlier window's `dm-desktop` `xdnd.rs`, moved
here), handing the files over as `text/uri-list`; when it is over it sends
the window `_DM_DRAG_ENDED`, which ends GPUI's drag as the other systems'
end does. On Wayland (`src/linux/wayland/client.rs`, `dm(wl-copy)`) the drag
offers a copy only, where upstream offered a move as well: a file manager
taking a move would carry the download away from where dm saved it. Owed to
the Linux pass: `scripts/linux-drag-check.sh` and
`scripts/linux-wayland-drag-check.sh` dragging out of the GPUI window
(docs/gaps.md G10).

## gpui-pre-windows 0.3.12

`gpui-pre-windows/` is the crate published on crates.io at 0.3.12 (Zed's
`gpui_windows` crate from the same snapshot; licence Apache-2.0, its
`LICENSE-APACHE`), with dm's change in `patches/gpui-pre-windows.patch` (11
hunks in six files, one of them new). It is the version the GPUI window is
built with on Windows. Removed and kept as for `gpui-pre-linux`, and moved to
another snapshot with it. macOS builds the published `gpui-pre-macos`
unchanged.

Mica behind the whole window (`dm(mica)` in `window.rs`,
`extend_frame_into_client`): Windows puts a system backdrop behind the frame it
draws itself only, and this window draws its own title bar, so what it left
clear for Mica showed black (dm's gaps row G11). The frame is spread over the
whole window (`DwmExtendFrameIntoClientArea`, margins of -1) for Mica; an
opaque window is left as upstream draws it. Not yet enough: the frame is drawn one flat colour
(gaps row G11).

The app's own Light or Dark (`dm(theme)` in `util.rs`, `platform.rs` and `events.rs`):
upstream leaves `Platform::set_window_appearance` empty on Windows, so
Options › General › Theme did nothing and every window followed Windows' mode
(dm's gaps row 211). The choice is kept in `util.rs` (`choose_appearance`) and
`system_appearance` answers with it where one is chosen; each window is posted a
message of dm's own (`WM_DM_APPEARANCE_CHOSEN`, as Windows will not post
`WM_SETTINGCHANGE` with its string) and takes the new look, and its title bar's
dark mode, from its own message loop (`events.rs` `follow_appearance`, which
Windows' own `ImmersiveColorSet` change goes through too). Upstream's Linux
platform leaves `set_window_appearance` empty as well, and the Linux copy does
not carry this change: there the choice is still ignored.

Files dragged out of the window (`dm(win-drag-out)`: `src/drag_out.rs`, and changes in
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

## gpui-component 0.9.1

`gpui-component/` is the crate published on crates.io at 0.9.1 (the GPUI
component library dm-gpui's text fields, controls and system menus come from,
built for the gpui-pre snapshot dm pins; licence Apache-2.0, its
`LICENSE-APACHE`), with dm's change in `patches/gpui-component.patch` (8 hunks
in one file). Removed and kept as for the GPUI copies; `tests/` and the tests
in `src/` are kept as published, though they read files of upstream's
repository that the crate does not carry and are not run here. To move to
another version: change `GPUI_COMPONENT_VERSION` and its SHA-256 in
`xtask/src/vendor.rs` (with the version `desktop/Cargo.toml` pins, which a test
of that file holds it to), run `cargo xtask vendor gpui-component`, mend any
hunk `git apply` refuses, and write the patch again as the `git diff` from the
published crate (with the same files removed) to the mended copy.

The keys of a system menu on macOS (`dm(mac-keys)` in
`src/native_menu/macos.rs`; dm's gaps row U27): `NativeMenu`, the menu the
system draws (an `NSMenu` on macOS, Windows' own, a drawn one on Linux), had no
field for an item's key, so the row menu and the title menus showed none on
macOS. Now, when the menu is shown, the key bound to each item's action is
looked up as the drawn fallback menu looks it up (for the focused element, else
wherever focus is; one keystroke, not a sequence) and set as the item's key
equivalent, its character and modifiers as GPUI's own menu bar sets them
(`gpui-pre-macos` `create_menu_item`; the mapping of key names to AppKit's
characters is that crate's private `key_to_menu_equivalent`, written again
here), with automatic localization of the key turned off where macOS has it
(12 and later), as GPUI does. Every `NativeMenu` gains its keys, as on Linux:
the row menu, the title menus, the toolbar's arrows and the pop-ups; an item
whose action has no binding shows none. The key is shown only: a popped-up
menu is not in the menu bar, so AppKit looks for no key equivalent in it once
it is closed and the key goes to the window's binding as before, and while it
is open the menu's tracking takes the key, so it chooses that one item, whose
action is dispatched once. On Windows dm writes the key after a tab in the
item's words (`dm-gpui` `window/rowmenu.rs`), and the Windows and Linux code
of the crate is as published.
