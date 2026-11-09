# Module boundaries

Which code may depend on which, and what each part owns. The crate rules are
enforced by `cargo xtask layers` (`desktop/xtask/src/main.rs`, run by
`cargo test --workspace`); the rest is held by review and by this page.

## Crates

| Crate | May depend on (dm's own) | May not use | Owns |
|---|---|---|---|
| `dm-protocol` | nothing | a window toolkit, sockets | the messages between the browser extension and the app |
| `dm-core` | nothing | a window toolkit | the queue and its rows, every downloader, persistence of the queue |
| `dm-shell` | `dm-core`, `dm-protocol` | a window toolkit | the application layer (`dm_shell::app`), settings, the browser's native host, the inbox, lists, registration |
| `dm-desktop` | `dm-core`, `dm-protocol`, `dm-shell` | — | the egui window and the desktop's side of the computer (tray, toasts, drops, power actions, the Platform implementation) |
| `dm-catcher` (browser/) | `dm-protocol` | a window toolkit, sockets | the extension's decisions, compiled to WebAssembly |

A future GPUI crate takes `dm-desktop`'s place: `dm-core`, `dm-protocol` and
`dm-shell` only, and never `dm-desktop`.

## Inside the application layer, `dm_shell::app`

| Module | Owns | Never |
|---|---|---|
| `power` | `WhenDone` (what happens when the queue runs dry), its words, `Report` | carrying the action out (the desktop's `power.rs` does) |
| `platform` | the `Platform` trait: what the application asks of the computer | calling the system itself |
| `changes` | that the queue changed, told to every subscriber; the `CHANGED` mark | carrying what changed, blocking, reading the queue in a subscriber |
| `snapshot` | `Snapshot`: the rows (shared) and what each row's line says beside its state | copying rows, being read per frame |
| `rules` | decisions about rows used in more than one place (`go_on`) | wording: each presentation says what was decided its own way |
| `commands` | the registry: every `Command`, its stable name and English label, its keys as `Chord`s, which apply to the rows chosen (`applies`, with what only the presentation sees behind `Looks`), and the commands that are the application's alone (`verb`, `when_done`, `limited`, `recheck`) | a command's words in the language in force, a toolkit's key type, opening a dialog |
| `names` | what a row is called until the server names it, its kind, the folder it is bound for and the free name it will get there | writing any of it into the row |
| `import` | how much of a list is listed already, the rows an import adds, the lines with no link | reading the file, the question asked about what is listed |
| `files` | a finished ZIP extracted off the presentation's thread | the system's openers (`Platform`'s) |
| `links` | links in text and files, the clipboard watch's memory and what it offers | reading the system's clipboard |

What belongs in `dm_shell::app`: a command's rules and validation, state a
presentation draws from, the changes it is told of, scheduling, settings
coordination. What does not: drawing, widgets, fonts, the window's view state
(selection, sort, scroll, which dialogs are open), and anything that only
makes sense with a screen.

## Rules that hold everywhere

- Domain types (`dm_core::Entry`, `State`, `Id`, `Queue`) carry no GUI type
  and no presentation's wording.
- The application never names `egui` (or any toolkit). A trait it declares
  (`Platform`) is how it asks the presentation for what only a desktop does.
- A presentation never fetches anything itself: it asks the application, which
  asks the engine.
- State has one owner: the queue owns rows; `Settings` (behind `Backend`) owns
  settings; the window owns only its view state.
- Moves keep the old path working with a `pub use` until every caller is moved.
