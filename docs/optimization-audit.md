# Optimization audit: dm against the GPUI and Rust cross-platform spec

The maintainer gave an engineering spec on 2026-11-11, "GPUI + Rust Optimization for
Cross-Platform Desktop Applications". This page holds dm to each of its requirements, sections 2
to 13, as the code stood at `5b03929f` (branch `gpui-par`). Each requirement is **MET**, **NOT MET**,
**PARTLY** (part is met and the rest is a row), or **N/A** (does not apply to dm, with why).
Everything not met is a row in [gaps.md](gaps.md), rows 180 to 192, each marked "(optimization
spec §N)". Linux and Windows were audited from the code and the docs only. A measurement on
them is owed to the final Linux pass or to the Windows laptop.

Paths are under `desktop/crates/` unless they say otherwise. `g/` is `dm-gpui/src`, `s/` is
`dm-shell/src`, `b/` is `dm-desktop-base/src` and `c/` is `dm-core/src`.

## §2 Architecture

dm keeps its own crates. The spec's layout is only an illustration, and no rule needs it: the
engine (`dm-core`) and the application layer (`dm-shell`) are already apart from both windows.

| Requirement | Verdict | Evidence |
|---|---|---|
| GPUI only in the presentation; the engine and app core never depend on it (rule 1) | MET | `cargo xtask layers` (`desktop/xtask/src/main.rs:287-345`) refuses any GUI crate in `dm-protocol`, `dm-core`, `dm-shell` and `dm-desktop-base`, GPUI included under all four of its names. It runs in `cargo test --workspace` (`xtask/src/main.rs:729`) and in CI (`.github/workflows/desktop.yml:87`) |
| No `cfg(target_os)` in shared logic unless genuinely platform-specific (rule 2) | MET | About 102 lines in `dm-core` and 57 in `dm-shell`. They are OS bindings: the keychain and credential store (`c/secretstore.rs`), the system's JavaScript engine (`c/javascript.rs`), free space and FAT32 (`c/download/space.rs`), network interfaces (`c/second_path.rs`), the registry or manifest files of a browser host (`s/register.rs`), Windows' RAS (`s/dialup.rs`) and the system's openers (`s/shell.rs`) |
| Platform code behind small documented adapter interfaces (rule 3) | MET | `s/app/platform.rs:14` `trait Platform`: notifications, clipboard, reveal and open, trash, sounds, power, file choosers. `b/desktop.rs` implements it. `docs/architecture/MODULE_BOUNDARIES.md` says what may call what |
| UI state is never the source of truth for downloads (rule 4) | MET | The window reads snapshots that it shares with the queue (`s/app/backend.rs:84-96`, `Vec<Arc<Entry>>`). It owns only its view state (MODULE_BOUNDARIES.md, "State has one owner") |
| The engine is usable from a CLI, a background process, a test harness or another UI (rule 5) | MET | `--headless` (`b/headless.rs`), the command line (`list`, `status`, `pause`, and so on), MCP, more than 50 engine test binaries in `dm-core/tests`, and two windows (egui and GPUI) over the same `dm_shell::app` |
| Acyclic crate dependencies (rule 6) | MET | Cargo forbids a cycle, and `xtask layers` holds each crate to the crates below it |
| No tiny crates without an ownership or compile-boundary reason (rule 7) | MET | Six crates and `xtask`. Each has a boundary that is enforced (the table above) |
| Narrow public APIs with documented invariants (rule 8) | MET | `#![warn(missing_docs)]` is set at every crate root (`c/lib.rs:23`, `s/lib.rs:16`, `b/lib.rs:38`, `g/main.rs:26`), and CI runs clippy with `-D warnings`. `dm-core`'s root exports 56 modules and items, which is wide. Narrowing it is a refactor no rule asks for, so it was left |
| Dependency inversion at platform boundaries | MET | The `Platform` trait above. `WindowServices` is the window's part of it (`b/desktop.rs`) |

## §3 GPUI rendering

### 3.1 The render path

| Requirement | Verdict | Evidence |
|---|---|---|
| No network, file-system, database or blocking work in render | NOT MET (row 180) | Each frame of the main window has 2 `block_on` calls into the engine (`g/toolbar.rs:82-83`, `view.backend.queues()` for Start Queue and Stop Queue). Each finished row on screen costs 2 `stat()` calls (`status_of`, `b/rows.rs:272` → `s/app/rules.rs:89` `Path::exists`, called from `g/table.rs:236` and `:276`). The torrent sharing mutex is taken for each row (`b/rows.rs:288`). A progress window does `block_on` and reads the plan and ledger files at each wake (`g/progress.rs:136`, `:151`). Download details runs `clash()`, a scan of every row plus file checks, at every render (`g/details.rs:510` → `b/file_info.rs:143-162`). Properties (`g/properties.rs:270`, `:306`) and the browser bar (`g/bars.rs:179`, a file check at most every 3 s) touch the disk too. No network access and no process start were found in render |
| No parsing, sorting, filtering or formatting of large datasets in render | PARTLY (row 180) | Sorting and filtering run once per read, not per frame (`g/list.rs:1894-1914`). Formatting covers only the rows on screen. Each frame still makes about 12 passes over the chosen rows: `chosen()` (`g/list.rs:1581`) runs once per toolbar button through `enabled()`. Each frame also scans every row in `stalled_bar` and `failed_bar` (`g/bars.rs:60`, `:115`), the status bar's tally (twice) and `queue_names` (`g/sidebar.rs:42`) |
| No cloning of large collections or owned strings on each render | NOT MET (row 180) | `Backend::settings()` deep-clones the 80-field `Settings` under its mutex (`s/app/backend.rs:275`; about 20 of the fields are collections). A main-window frame does this about 12 times: `g/toolbar.rs:48`, `g/sidebar.rs:27` and `:69`, `g/table.rs:50`, `g/statusbar.rs:66` (twice), and twice in each of the 3 calls GPUI makes to the row list's processor per frame (`g/table.rs:324-325`) |
| Cache derived values with correct invalidation | PARTLY (row 181) | The place counts are cached per read (`s/app/places.rs:241-251`), and so are the names rows will get (`shown_names`). `SavedSizes::apply` costs (finished rows × all rows) at every read (`s/app/snapshot.rs:49-52`): at 10,000 finished rows, about 10⁸ comparisons a wake |
| State close to its component; no app-wide notifications for local changes | NOT MET (row 181) | The main window is one entity, `Downloads`, with no `.cached()` views. Each wake ends in `cx.notify()` with no condition (`g/list.rs:475`), so the title bar, toolbar, sidebar, every bar, the header, the rows and the status bar are all built again. The progress, Complete, drop-target and Properties windows observe `Downloads` and render again with it |
| No custom rendering complexity without measured need | PARTLY (row 180) | There is no hand-painted element. But every label is wrapped in a `div` plus a `canvas`, so that its bounds can be recorded for the control port's `ui.texts` (`g/screen.rs:42-70`). This happens on every frame, even with no control port open. It roughly doubles the element count of a screenful of rows |

### 3.2 Progress coalescing

| Requirement | Verdict | Evidence |
|---|---|---|
| Don't forward every engine event to the UI | MET | The engine samples each download every 250 ms (`c/download/ranged.rs:51-53`) into a bounded channel (`c/queue/runner.rs:353`, `channel(32)`). The runner keeps only the latest sample in the row (`c/queue/runner.rs:384`). Progress does not wake the window |
| Latest progress per stable ID; repeats coalesced | MET | One `progress` field per row (`runner.rs:384`). The typed events keep one progress entry per id (`s/app/events.rs:21-23`) |
| Controlled publishing cadence (5 to 10 a second to start, then benchmark) | MET, with a different rate | A queue change wakes the window with `Wake::Soon`, and wakes that come within 250 ms are read once (`g/list.rs:441-460`). While anything runs, a 1 s clock redraws the progress (`g/list.rs:506-515`). These are the egui window's rates, and people already see them. Making it 5 to 10 a second would change what a person sees and add CPU (row 176), so the rate stays until a measurement argues for another |
| Notify only affected UI state and visible rows | NOT MET (row 181) | Each wake reads the whole list again (`Downloads::read`, `g/list.rs:1600-1640`: 3 `block_on` calls, every row named again, placed, filtered and sorted) and redraws the whole window (see 3.1) |
| Skip updates whose displayed values did not change | NOT MET (row 181) | `changes::take()` says whether anything changed, but its answer is ignored (`g/list.rs:1601`). A wake from the clock alone does the same full read and redraw |
| Critical transitions separate from telemetry, never dropped | MET | The window reads state from the queue's snapshot, so a coalesced wake cannot lose a Completed or a Failed. Lifecycle events are kept in order and never dropped (`s/app/events.rs:17-24`). The only progress report allowed to drop is `Retrying` (`try_send`, `runner.rs:441`) |
| Use the project's ID and event types; no duplicate models | MET | `dm_core::Id`, `Entry`, `State` and `dm_shell::app::events::DownloadEvent` |

### 3.3 Large lists (10,000+ records)

| Requirement | Verdict | Evidence |
|---|---|---|
| Virtualization, checked against what the pinned GPUI does | MET | `uniform_list` (`g/table.rs:320`). At `gpui-pre` 0.3.12 it builds only the visible range (`uniform_list.rs:492` of the crate) |
| Visible rows plus a small overscan | MET | As above. There is no overscan, and none is needed at fixed row height |
| Stable row identity by DownloadId | NOT MET (row 182) | Each row's element id is its position, `.id(i)` (`g/table.rs:341`). The click and menu handlers capture `i` and look the row up later (`g/list.rs:1868`), so a sort that lands between the frame and the click acts on whatever row is now at that position |
| Sorting and filtering outside render | MET | `reshow` runs once per read (`g/list.rs:1894`) |
| No clone of the whole collection per update | MET | The rows are shared, not copied (`list_shared`, `c/queue/query.rs:49`). The per-read costs are row 181's |
| Cache display formatting where profiling justifies it | MET | Formatting is done only for the visible rows. No profile asks for more |
| Keep the scroll position on insert, remove and update | NOT MET (row 182) | `UniformListScrollHandle` keeps a pixel offset with no anchor (`g/list.rs:110`). Only Find and the arrow keys scroll to a row (`g/list.rs:1043`, `:1862`). Nothing tests a row inserted above the view |
| Keyboard navigation and accessibility stay correct | MET | The selection is kept by id (`g/list.rs:125`, a `BTreeSet<Id>` with an anchor id), so it survives a sort. Each row has `Role::Row` and a spoken label (`g/table.rs:342-345`). `human-qa.py` holds 64 of 64 on macOS (gaps.md G7) |
| Test rapid scrolling while downloads update | NOT MET (row 182) | `ui-bench.py` scrolls once, before its busy phase (`scripts/ui-bench.py:139-145`). The `window` phase of `stress-check.py` (5,000 rows and 60 downloads) does not scroll |

### 3.4 Images and assets

| Requirement | Verdict | Evidence |
|---|---|---|
| Decoded icons cached; no repeated decode; bounded caches; async loading; no growth over a large history; decode at the size drawn | MET | 22 SVGs are embedded (`g/assets.rs`) and drawn with `svg().path`. GPUI rasterizes each once per size into its atlas. The kind icon comes from that fixed set, so a large history adds no images. dm has no thumbnails |

### 3.5 Animation

| Requirement | Verdict | Evidence |
|---|---|---|
| No continuous animation when nothing changes; event-driven; an idle app is not kept busy | MET | There is no `with_animation`, no animation frame and no spinner. `on_next_frame` is used only for one-shot calls (`g/fit.rs:22`, `g/list.rs:397`). With nothing running, the window wakes every 30 s (`g/list.rs:92`). Idle CPU was measured in §9 |
| Respect reduced-motion settings | N/A | dm has no animation for such a setting to turn off. A future animation must read it (AGENTS.md) |

## §4 Rust performance and maintainability

| Requirement | Verdict | Evidence |
|---|---|---|
| Borrow over clone; `Arc` only for shared ownership | PARTLY (row 180) | The rows are `Arc`-shared from the queue to the window. The deep `Settings` clones in render are 3.1's |
| Short critical sections; blocking work to dedicated workers | PARTLY (rows 184, 185) | See 4.2 and §5 |
| Bounded channels and backpressure | PARTLY (row 181) | Every engine channel is bounded (`runner.rs:353`; `c/videopage/fetch.rs:694`, `:697`). The window's wake channel is unbounded (`g/main.rs:199`, `async_channel::unbounded::<Wake>()`), so it grows while the window's thread is held, for example by a modal dialog (row 183) |
| Reuse buffers where profiling proves a benefit | MET | The connection's 16 KiB write buffer was measured against 256 KiB (`c/download/ranged.rs:1068-1072`), and one 256 KiB buffer serves each digest (`c/checksum.rs:110`) |
| Box large enum payloads where it matters | MET | Nothing measured asks for it |
| Serialize only at boundaries | MET | The queue is serialized only when it is written (`c/queue/persist.rs:248`, off the runtime) and at the control port |
| Prefer ownership and messages over shared mutability | PARTLY (row 184) | Progress reaches the row through a channel. The queue's rows sit behind tokio mutexes (`c/queue/mod.rs:476`) |
| Typed errors with actionable context | MET | `c/error.rs` (`thiserror`). Problems are told in plain words (`queue.note`) |

### 4.1 Allocation discipline

| Requirement | Verdict | Evidence |
|---|---|---|
| No temporary `String`, `Vec` or `format!` in hot loops without evidence | PARTLY (rows 180, 181) | The engine's write loop does none (`c/download/ranged.rs:1180-1215`). The window builds a `String` for each row name at each read (`g/list.rs:1621`) and clones `Settings` per frame |
| Reuse I/O buffers; bound caches and pools; no pooling of small objects without benchmarks | MET | The name cache is emptied past 2,000 entries (`s/app/names.rs:228`). The place counts are kept per read. dm has no object pools |

### 4.2 Locks

| Requirement | Verdict | Evidence |
|---|---|---|
| Never hold a mutex or `RwLock` across `.await` | PARTLY (row 184) | **std guards: MET.** `dm_core::lock` is a std mutex (`c/lib.rs:226`). Clippy's `await_holding_lock` is on and allowed nowhere, and CI refuses warnings (`desktop.yml:84`). **tokio guards across I/O: NOT MET.** `store` is held across the queue file's write. The crate is edition 2021, so the guard made in an `if let` stays alive to the end of the block (`c/queue/persist.rs:212`, `:346`). `slots` is held across `remove_file` (`c/queue/runner.rs:478-512`) and across a synchronous `create_dir_all` (`runner.rs:612-624` → `c/queue/mod.rs:791`). A job's `segments` is held across `spawn_blocking` (`c/download/ranged.rs:454-494`). A global lock is held across a network fetch of the JavaScript engine (`c/tools.rs:377`), and another across a proxy-script fetch (`c/pac.rs:341`) |
| Never hold a lock while notifying the UI or calling a callback | PARTLY (row 184) | The change callback is called after its guard is dropped (`persist.rs:164`, `s/app/changes.rs:57`, `s/app/events.rs:258-266`). `queue.note()`, which calls it, runs with `slots` held (`runner.rs:500`) and inside `saving` (`persist.rs:268`, `:273`). Neither deadlocks today, because the subscribers only set a flag or send a wake |
| Document lock ordering | NOT MET (row 184) | `Inner` (`c/queue/mod.rs:476-557`) says nothing about order. The order in use (options → slots → order; store → saving → slots → order) can be read only from the code |
| Single owner plus messages for hot mutable state; atomics only for simple values | MET | The progress readers own their writes into the row. Atomics are flags and counters (`c/control.rs`, `Control(Arc<AtomicU8>)`; `persist.rs` `unwritten`, `trailing`) |

### 4.3 unsafe

| Requirement | Verdict | Evidence |
|---|---|---|
| Every unsafe block has a SAFETY comment | MET | `#![warn(clippy::undocumented_unsafe_blocks)]` is set at every crate root that has unsafe (`c/lib.rs:24`, `s/lib.rs:27`, `b/lib.rs:39`, `dm-desktop/src/lib.rs:27`, `g/main.rs:27`, `dm-protocol/src/lib.rs:61`), and CI refuses warnings |
| Prefer safe Rust; no speculative unsafe; isolate it behind small APIs | MET | The unsafe is FFI: Objective-C, Win32, JavaScriptCore and Chakra, the keychain. Each sits in its own module (`c/javascript.rs`, `c/secretstore.rs`, `c/frames.rs`, `g/syswindow.rs`) |
| Review before expanding | MET (now a written rule) | AGENTS.md, "Rules for performance and platform work", rule 7 |

## §5 Async runtime and threads

| Requirement | Verdict | Evidence |
|---|---|---|
| The GPUI thread separate from the async runtime | MET | GPUI runs on the main thread. The tokio runtime is its own (`b/boot.rs:101`) |
| A bounded worker pool for hashing, CPU transforms and archive extraction | NOT MET (row 185) | Hashing runs on the async workers, between reads (`c/checksum.rs:109-127`). Each extraction gets its own OS thread (`s/app/files.rs:154`), and so does each JavaScript evaluation (`c/javascript.rs:434`). Assembly goes to tokio's blocking pool, which is left at its default of 512 threads (`b/boot.rs:101`, `Runtime::new()`) |
| Blocking I/O workers; a persistence worker | MET | 29 `spawn_blocking` sites in `dm-core`. The queue is written off the runtime one write at a time, coalesced, and no more often than a tenth of the time a write takes (`c/queue/persist.rs:163-205`, `:231-250`) |
| Never block the GPUI thread | NOT MET (row 183) | `Backend` has 52 `block_on` calls that the window makes on its own thread (`s/app/backend.rs:67-702`). The worst is Move/Rename to another disk, which copies the whole file while the window waits (`g/rowdialogs.rs:426` → `s/app/backend.rs:602` → `c/queue/queues.rs:120-123`). A bulk Pause, Resume or Retry makes one `block_on` and possibly one queue write per row (`s/app/commands.rs:605-609`). Folder and file choosers are synchronous `rfd` (`g/services.rs:33-50`, with 9 callers); only 2 use GPUI's async `prompt_for_paths` (`g/links.rs:160`, `g/details.rs:391`). Settings are written synchronously on a column drag (`s/app/backend.rs:354`, `:371`). Export, import and the trash run synchronously (`g/actions.rs:196`, `g/list.rs:1787`). The clipboard is read every second while it is watched (`g/services.rs:22`) |
| No OS thread per download | MET | Downloads and connections are tokio tasks (`c/queue/runner.rs:251`, `c/download/ranged.rs:210`, a `JoinSet`) |
| Bounded concurrency for network and CPU tasks | PARTLY (row 185) | Network: connections per download, downloads at once and per-host limits are settings. CPU work has no limit of its own (above) |
| Cancellation tokens | PARTLY (row 185) | A pause or cancel is a flag (`c/control.rs`) that is checked between chunks (`ranged.rs:1167`). The other connections get 1 s and are then aborted (`ranged.rs:403`). Work in `spawn_blocking` cannot be stopped: the assembly copy (`c/download/assemble.rs:43`) and a digest (`c/checksum.rs:109`) both run to their end |
| Backpressure | MET | Each write is awaited before the next chunk is read (`ranged.rs:1210`; `c/download/plain.rs:100`). The progress channel holds 32 |
| Graceful shutdown; no task outlives the app silently | NOT MET (row 186) | On quit the queue is flushed (`g/list/questions.rs:314` → `s/app/backend.rs:226`). Running downloads are neither stopped nor awaited, and the runtime is dropped with them. The next start puts them back as Queued (`c/queue/persist.rs:105`). Nothing tests quitting in the middle of a transfer |
| No nested runtimes without a documented reason | MET | `c/pac.rs:331-348` documents its single-thread runtime: reqwest's choice of proxy cannot await. `s/host.rs:478` is the native host, a process of its own |
| Don't assume async file APIs are non-blocking | PARTLY (row 184) | `tokio::fs` is used knowing it is a blocking pool. A few `std::fs` calls run on async workers: `persist.rs:24-31` and `:336` (the quota file), and `queue/mod.rs:791` under `slots` |

### 5.2 Download-manager safeguards

| Requirement | Verdict | Evidence |
|---|---|---|
| Independent network and disk concurrency limits | NOT MET (row 185) | Disk writes run where the connections run (each writes its own file through tokio's pool). No limit on assemblies, digests or extractions is separate from the network's |
| Slow disks never cause unbounded memory growth | PARTLY (row 185) | By design, yes: the socket is not read again until the write is done, and the buffers are fixed (16 KiB per connection, a 1 MiB decode slice in `c/decode.rs:32`). No test slows a disk to prove it |
| A stalled UI never stalls downloads | MET | The engine wakes the window with `try_send` (`g/main.rs:207`), and the subscribers only signal (`s/app/changes.rs:17-19`). The engine never waits on the window |
| A stalled download never stalls unrelated ones | PARTLY (row 184) | No queue-wide lock is held across a network await. Still, `FETCHING` (`c/tools.rs:377`) and `ASKING` (`c/pac.rs:341`) hold back the downloads behind one fetch, and `store` and `saving` delay every save behind a slow write of the queue |
| Progress lossy and coalescible; critical transitions never dropped | MET | 3.2 |
| Queue persistence and crash recovery stay correct | MET | A journal over the queue file (`c/queue/journal.rs`). A file that cannot be read is set aside, never overwritten (`persist.rs:41-80`). `stress-check.py crash` (`kill -9` during 24 downloads, then every one finishes byte-exact on restart). `dm-core/tests/queue.rs:641` |
| Cancellation reaches the network, the disk and the workers | PARTLY (row 185) | Network and disk, yes (above). Assembly and digests, no |

## §6 Cross-platform

Shared behaviour lives in the shared crates, and the native parts sit behind `Platform` (§2).

| Requirement | Verdict | Evidence |
|---|---|---|
| macOS: Metal and a GPU fallback | MET / N/A | GPUI draws with Metal (`gpui_platform`, `runtime_shaders`, `g/Cargo.toml`). Every Mac dm supports (macOS 11 or later, `minimumSystemVersion`) has Metal, so a fallback has nothing to fall back from |
| macOS: Apple Silicon (Intel only if supported) | MET / N/A | Built and tested on Apple chips. Intel Macs are not built (README.md:81), so they are out of scope |
| macOS: lifecycle and termination | PARTLY (row 186) | Quit, the Dock and SIGTERM are handled (`b/signals.rs`; `human-qa.py`). A quit during a transfer is row 186 |
| macOS: native dialogs and permissions | MET | `rfd` opens `NSOpenPanel` and `NSSavePanel`, and Options uses GPUI's prompt. The panels being synchronous is row 183 |
| macOS: notifications, menus, drag and drop | PARTLY | Notifications go through `notify-rust`, and the menus are GPUI's in the system menu bar. Links and files dropped on the window are taken. Dragging a finished file out is gaps row G10 |
| macOS: high DPI and display scaling | MET | GPUI's scale factor. `human-qa.py` passes 64 of 64 on a Retina screen (G7) |
| macOS: packaging, signing, notarization | MET / N/A | `.app` and `.dmg` from CI (`desktop.yml:198-202`), ad-hoc signed or signed with secrets where they exist (`desktop.yml:238-262`). Notarization costs $99 a year with no free route, and is not bought (`docs/decisions.md`, 2026-10-16, "Released unsigned, with the free signing there is") |
| Windows: supported versions and architectures | MET | Windows 10 and 11 on x64 (README.md:56, :69). No Arm build is offered |
| Windows: Win32 integration and text rendering | MET | GPUI on DirectWrite. 63 of 64 contract rows held from the installer (G7, 2026-11-11) |
| Windows: native dialogs and drag and drop | PARTLY | `rfd` opens `IFileDialog`. Drop-in is coded (G6, check 5). Drag-out is G10 |
| Windows: notifications and tray | PARTLY | Both are coded and checked by hand (G6, G8). Open in G6 |
| Windows: high DPI and **mixed-DPI** multi-monitor | NOT MET (row 191) | GPUI links its own manifest, PerMonitorV2 (`gpui-pre-0.3.12/resources/windows/gpui.manifest.xml`), and handles `WM_DPICHANGED` (`gpui-pre-windows events.rs:105`). Nobody has moved the window between two monitors of different scale |
| Windows: long paths, Unicode paths, file locking | PARTLY (row 191) | Unicode names are tested (`shared/naming-cases.json`; the property in `dm-core/tests/fuzz.rs:135`). Files that are still open are handled (`c/queue/runner.rs:481`). The manifest has no `longPathAware`. Rust's `std::fs` reaches long paths on its own, but nothing tests a folder longer than 260 characters through Open, Show in folder, the trash or the installer |
| Windows: installer, update, uninstaller | MET | The NSIS installer and its uninstall section (`g/Cargo.toml`, `nsis`), the update check (`s/app/update.rs`), and a smoke test of the installed program in CI (`desktop.yml:406`) |
| Windows: network interruption and removable drives | PARTLY (row 187) | Outages are covered (`bench outage`, `stress-check.py outage`, `sleep`). Pulling a removable drive is checked by hand only (`scripts/video-sites-check.py` ENG-07b) |
| Linux: X11 and Wayland | PARTLY (G6, row 192) | X11: checks 1 to 4 pass, 64 of 64 contract rows and 122 of 122 real-input checks (G6, G7). Wayland: checks 2 and 4 only |
| Linux: KDE Plasma and GNOME; Niri and Hyprland where relevant | NOT MET (row 192) | KWin is covered for closing only (`scripts/linux-kde-close-check.sh`). Niri and Hyprland are covered by unit tests of the minimize rule (`b/tray.rs:67`, `:223`). The GPUI window has not been run on GNOME's Wayland, on Plasma's Wayland, on Niri or on Hyprland |
| Linux: NVIDIA, AMD and Intel GPUs | NOT MET (row 192) | Run only on Mesa in Docker (software rendering) |
| Linux: native and portal file dialogs | NOT MET (row 192) | `rfd` uses the portal and falls back to zenity (`docs/migration/PLATFORM_GAP_ANALYSIS.md` item 16). Neither path has been checked with the GPUI window |
| Linux: notifications and tray availability | PARTLY (row 192) | `b/tray.rs` asks whether a StatusNotifier host is showing and opens the window otherwise (`g/list.rs:403-412`). This is checked in Docker, not on each desktop |
| Linux: AppImage and other packaging | MET | `.deb`, `.rpm` and AppImage for x86_64 and arm64, each smoke-tested in CI (`desktop.yml:178-191`, `:368-389`) |
| Linux: permissions, mount removal, disk full | NOT MET (row 192) | The disk-full phase runs on macOS only (`scripts/stress-protocols.py:1120`). Mount removal is not checked anywhere automated |
| Don't assume identical tray, notification or portal behaviour across desktops | MET | `b/tray.rs:67` (compositors that ignore minimize) and the host check above |

## §7 Cargo build and release

| Requirement | Verdict | Evidence |
|---|---|---|
| Evaluate `opt-level`, thin vs fat LTO, `codegen-units`, strip and `panic = "abort"`; keep separate debug symbols; measure startup, throughput, memory and binary size | PARTLY (row 189) | `desktop/Cargo.toml:16-45` sets `"z"` with `dm-core` and the CPU-bound crates at 3, fat LTO, `codegen-units = 1` and `strip = true`. That was measured on 2026-11-06 for the egui window (`docs/decisions.md`, "Release profile"). It has not been measured again for `dm-gpui`, and its overrides still name `egui` and `epaint`. Thin LTO has never been measured on macOS (on Linux in Docker, fat LTO's link ran out of 7.6 GB, G8). No debug symbols are kept for a release build. `panic = "abort"` was turned down for the egui tray's `catch_unwind` (`dm-desktop/src/tray.rs:152`), which `dm-gpui` does not have. dm has no crash reporting by rule (AGENTS.md rule 6), so the trade-off is tokio's per-task panic isolation against size |
| Fast incremental dev builds | MET | The default dev profile. CI uses `line-tables-only` (`desktop.yml`, env) |
| Reproducible profiling builds | NOT MET (row 189) | No `[profile.profiling]` (release with debug info) exists |
| Record compiler version, target triple, features and dependency revisions with benchmark results | NOT MET (row 188) | `bench.mjs` records the OS, CPU, memory, Node and the binary's size and date (`bench/bench.mjs:386-395`). It does not record the commit, rustc, the target, the profile, the GPU or the display scale |
| Pin GPUI to a known revision | MET | `gpui = { package = "gpui-pre", version = "=0.3.12" }`, `gpui-component = "=0.9.1"` (`g/Cargo.toml`). One patched copy is vendored (`desktop/vendor/gpui-pre-linux`) |
| Commit Cargo.lock; no floating git branches | MET | `desktop/Cargo.lock` and `browser/Cargo.lock` are tracked, and every source is the registry (no `source = "git`). CI builds with `--locked` |
| Review unnecessary default features | MET | `reqwest`, `tokio-rustls`, `chrono`, `librqbit`, `zip`, `tray-icon`, `image`, `eframe` and `objc2-*` are built with `default-features = false`. GPUI keeps its four defaults (font-kit, wayland, x11, windows-manifest), which dm uses |
| Track each dependency's compile time and binary-size contribution | NOT MET (row 190) | No `cargo bloat` or `--timings` record is kept |
| Update dependencies in controlled batches | MET | Dependabot, weekly, at most 5 pull requests per ecosystem (`.github/dependabot.yml`) |

## §8 Profiling and benchmarking

| Tool | Verdict | Evidence |
|---|---|---|
| clippy, rustfmt | MET | `desktop.yml:72`, `:84`; `ci.yml:41-46` (the extension) |
| cargo audit | MET | `.github/workflows/audit.yml:35`, with each ignored advisory explained (`desktop/.cargo/audit.toml`) |
| cargo deny | NOT MET (row 190) | There is no `deny.toml`. It was left as "the maintainer's to write" (BUGLOG.md:461), and the spec now asks for it |
| cargo nextest | MET | `desktop.yml:106` |
| cargo llvm-cov | MET | `desktop.yml:135-141`, against `docs/coverage-floor.txt` |
| cargo flamegraph, OS profilers (Instruments, WPR/WPA, perf) | NOT MET (row 190) | `docs/testing.md` says nothing about profiling |
| criterion | NOT MET (row 190) | dm has no `[[bench]]`. The engine's timing tests are pass or fail (`dm-core/tests/throughput.rs`) |
| cargo bloat | NOT MET (row 190) | Not used |
| cargo tree | MET | Used for duplicates (`docs/decisions.md:4094`) |

| Benchmark scenario | Verdict | Where |
|---|---|---|
| 1 Cold startup | NOT MET (row 187) | `ui-bench.py` times launch to first frame, but it never empties the system's file cache first, so every run after the first is warm. `launch-latency.py` "cold" times the browser's hand-off to a dm with no window |
| 2 Warm startup | MET | `ui-bench.py` "start" (§9) |
| 3 Idle CPU and memory | MET | `bench idle`, `memory-check.py`, `ui-bench.py` "idle" (§9) |
| 4 Opening a history with many records | MET | `ui-bench.py --rows 10000` "start" (§9) |
| 5 Scrolling a large list | PARTLY (rows 182, 187) | One 10-line scroll (`ui-bench.py:139-145`) |
| 6 Sorting and filtering large lists | NOT MET (row 187) | `ui-bench.py`'s Find (`ui-bench.py:147-160`) waits for a Find next button that only the egui window has, so with the GPUI window it times out (50 s), and nothing is measured. Neither a sort nor the search box's filter is timed |
| 7 Hundreds of concurrent progress updates | PARTLY (row 187) | `stress-check.py window`: 60 downloads at once, beside 5,000 rows |
| 8 High-throughput downloads while the UI stays interactive | PARTLY (row 187) | `stress-check.py window` times the window's answers at 60 × 1 MB/s, and `bench fast` times the transfer, but never the two together |
| 9 Disk full and removable drive | PARTLY (row 187) | Disk full: `stress-protocols.py fulldisk` (macOS). Removable drive: by hand only |
| 10 Pause, resume, cancel and retry under load | MET | `stress-check.py pause_storm` (80 downloads) |
| 11 Database recovery after forced termination | MET | `stress-check.py crash` |
| 12 Shutdown with active downloads | NOT MET (row 186) | No test |

| Regression policy | Verdict | Evidence |
|---|---|---|
| Documented inputs and hardware; a baseline; repeated runs with their spread; no CI failure on noisy microbenchmarks; coarse thresholds on shared CI | PARTLY (row 188) | `bench/` has all of these (the frozen floor, medians, best and worst, and it never runs in CI). `ui-bench.py` prints medians with no baseline to compare against. Memory has a budget only when `memory-check.py` is run by hand. Binary size is not tracked |
| Benchmark or profile evidence for performance-sensitive changes | MET (now a written rule) | The bottom line in gaps.md, and AGENTS.md rule 10 |

## §9 Initial targets, measured on this Mac

These numbers come from an Apple M1 (8 cores, 16 GB) on macOS 27.0.1, with the built-in Retina
display (2560 × 1600, scale 2), and the release `dm-gpui` built from `5b03929f` with the profile in
`desktop/Cargo.toml` (19,868,544 bytes, rustc 1.98.1, `aarch64-apple-darwin`). They were taken on
2026-11-11 **while another track was building and benchmarking the engine on the same computer**, so
treat them as upper bounds. Commands:

- `python3 scripts/ui-bench.py --rows 100,10000 --runs 3 --idle 30 desktop/target/release/dm-gpui`
  (medians of 3; the rows are queued for later, against a local server)
- `DM_BINARY=desktop/target/release/dm-gpui python3 scripts/stress-check.py window`

| Rows | Start to first frame | Idle CPU | Resident | Footprint | Scroll (one, through the control port) | CPU, 20 downloads at 1 MB/s |
|---|---|---|---|---|---|---|
| 100 | 0.44 s (runs 0.75, 0.41, 0.44) | 0.2% | 112 MB | 58 MB | 0.03 s | 11.2% |
| 10,000 | 0.91 s (runs 0.91, 2.27, 0.84) | 1.4% | 143 MB | 94 MB | 0.17 s (0.16 to 0.23) | 17.2% |

`stress-check.py window` (5,060 rows, 60 downloads at 1 MB/s each): the window answered a
control-port call in a median of 43 ms, worst 127 ms. `ui.inspect` took 0.1 s. Peak: 151 MB resident,
61% of a core, 43 threads.

ui-bench's Find timed out (50 s) on every run: it waits for a button the GPUI window does not have
(row 187).

**Later on 2026-11-11 (rows 187 to 190).** `ui-bench.py` was given the scenarios it lacked and run
again on the same build of the code (`78ce2636`, 19,952,080 bytes; `python3 scripts/ui-bench.py
--rows 100,10000 --runs 3 --idle 30 --save-baseline`, again with the other track working on the
computer). The medians are the stored macOS baseline, `scripts/ui-bench-baselines/darwin-arm64.json`:

| Rows | Start, binary evicted / warm | Paging, a scroll (worst) | Sort, median (slowest) | Filter / Find next / emptied | Answer, 300 queued (worst) | Answer at full speed (worst) | 512 MB, window / headless |
|---|---|---|---|---|---|---|---|
| 100 | 0.46 / 0.43 s | 0.016 (0.022) s | 0.017 (0.026) s | 0.064 / 0.004 / 0.012 s | 6 (13) ms | 2 (57) ms | 1.16 / 1.30 s |
| 10,000 | 0.77 / 0.73 s | 0.172 (0.190) s | 0.263 (0.400) s | 0.330 / 0.166 / 0.254 s | 107 (120) ms | 86 (142) ms | 1.33 / 1.26 s |

A drive pulled mid-download (`stress-protocols.py detach`, a disk image detached by force): the row
ended paused with its reason, dm answered in 1 ms, and resumed it whole once the drive was back.
Still not measured: a start after `purge` (needs root), the frame time (no Instruments on this Mac,
no timing in GPUI), and `bench fast` with the window against `--headless` (the benchmark's track).
The release profile's alternatives are in `docs/decisions.md` (row 189).

| Target | Measured | Verdict |
|---|---|---|
| Cold startup under 1.5 s | Not measured cold: the file cache was never emptied (`purge` needs root). The first launch after the build took 0.75 s at 100 rows | NOT MEASURED (row 187) |
| Warm startup (no target) | 0.44 s at 100 rows. 0.91 s at 10,000 rows, with one run of 2.27 s | Recorded |
| UI interaction latency under 50 ms | 30 ms at 100 rows. **170 ms at 10,000 rows.** Under 60 downloads at 5,060 rows: median 43 ms, worst 127 ms. Each figure includes the control port's round trip. The time grows with the list, because each wake reads and redraws everything (rows 180, 181) | NOT MET at 10,000 rows (rows 180, 181) |
| Frame budget 16.7 ms at 60 Hz | No frame timing exists | NOT MEASURED (row 187) |
| Idle CPU under 1-2% | 0.2% at 100 rows, 1.4% at 10,000 rows (wakes every 30 s, each reading all 10,000 rows) | MET, close to the limit at 10,000 rows (row 181) |
| Idle memory under 150 MB | 112 MB resident (58 MB footprint) at 100 rows, 143 MB (94 MB) at 10,000 rows. Against reference A's floor (39.2 MB) it is row 175 | MET, close to the limit at 10,000 rows |
| 10,000 records without a UI freeze | It starts in 0.9 s and keeps answering. A scroll takes 0.17 s, and 20 downloads cost 6 points more CPU at 10,000 rows than at 100 | MET without a freeze. The latency is not met (above) |
| No unbounded progress backlog | Progress is kept as the latest value per row (3.2). The window's wake channel is unbounded (`g/main.rs:199`) | PARTLY (row 181) |
| No measurable UI-induced engine throughput regression | Not measured: no run compares the window with `--headless` | NOT MEASURED (row 187) |
| Graceful, deterministic shutdown | Neither | NOT MET (row 186) |
| Record hardware, OS, GPU, display scale, build profile and dataset size with each result | Recorded here by hand. No harness records them all | NOT MET (row 188) |

## §10 CI

| Requirement | Verdict | Evidence |
|---|---|---|
| A matrix for the supported OS and architecture | PARTLY (row 190) | Tests run on `ubuntu-latest`, `windows-latest` and `macos-latest`, which is Apple Silicon (`desktop.yml:51`). Packages and smoke tests cover Linux x86_64 and arm64, Windows x64 and macOS (`desktop.yml:178-202`). Linux arm64 ships, but its tests are never run (smoke only, on tags) |
| `cargo fmt --check`, clippy `-D warnings`, `cargo test --workspace`, `cargo audit` | MET | `desktop.yml:72`, `:84`, `:106` (nextest, plus doctests at `:115`); `audit.yml:35` |
| `cargo deny check` | NOT MET (row 190) | §8 |
| Target-specific builds, packaging and smoke tests; native tests on each OS | MET | The bundle matrix and `smoke.py` on each OS (`desktop.yml:332-437`), `windows-check.yml` (`app-check.py` on Windows), and `qa.yml` (Docker). Cross-compiling is used for nothing it cannot prove |

## §11 AI agent rules

AGENTS.md had rules 11 and 14 (rules 0 and 1 of "Rules for a change") and part of 12 (CLAUDE.md:
"Do not change the window ... to make work easier"). The others, and the optimization workflow
and change report, were added on 2026-11-11 as "Rules for performance and platform work".

| Rule | Before | Now |
|---|---|---|
| 1 No GPUI in the engine | Enforced by `xtask layers`, not stated | Stated |
| 2 No blocking in render or event handlers | Missing | Stated (rows 180, 183) |
| 3 Never rebuild the whole app state for one progress update | Missing | Stated (row 181) |
| 4 No unbounded queues for progress or telemetry | Missing | Stated |
| 5 No platform behaviour in shared crates without reason | MODULE_BOUNDARIES.md only | Stated |
| 6 Find and weigh the existing abstraction before adding one | Missing | Stated |
| 7 No unsafe without documented invariants and review | Enforced by lint, not stated | Stated |
| 8 No new dependency without saying why the existing ones fall short | Missing | Stated |
| 9 Every async task has an owner, a cancellation and a shutdown | Missing | Stated (rows 185, 186) |
| 10 Every optimization has benchmark or profile evidence | gaps.md's bottom line | Stated |
| 11 Preserve user-visible behaviour unless authorized | Rule 0 | Kept |
| 12 No UI design change during engine or infrastructure work | CLAUDE.md, `docs/architecture/ARCHITECTURE.md` | Stated |
| 13 Narrow public APIs, documented invariants | `missing_docs` | Stated |
| 14 A regression test for each bug found | Rule 1 | Kept |
| 15 Never claim platform support from compilation alone | "Wired is not working" | Stated |

## §12 Roadmap

dm's phases map onto the rows. Phase 1 (this audit, the dependency map in
MODULE_BOUNDARIES.md, the baselines in §9). Phase 2: rows 180 to 182. Phase 3: rows 184 to 186.
Phase 4: rows 191 and 192, with G6, G8 and G10. Phase 5: rows 187 to 190.

## §13 Definition of done

Each new row's "How it closes" asks for the measurement before and after, the behaviour kept
(`human-qa.py` whole where the window is touched), and the trade-offs written in
`docs/decisions.md`, as §13 asks.
