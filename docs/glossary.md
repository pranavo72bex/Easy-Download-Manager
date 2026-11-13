# Glossary

The code names some things several ways. This page says what each word means, and which
name is the one to use in new code. (Gaps row A21: the names themselves are left as they are
until the queue/queues rename.)

## The list

| Word | What it is | In the code |
|------|------------|-------------|
| **download** | One file dm is fetching or has fetched, as a person thinks of it. | The word for the person: menus, docs, messages. |
| **row** | The same thing as the list stores it: one download's address, state, progress, folder and options. | `dm_core::Entry` (`queue/mod.rs`, "One row"). Code says "row" for an `Entry`; prefer it in new code over "entry" or "item". |
| **id** | A row's number, stable for as long as the list keeps the row. | `dm_core::Id` (`u64`). |
| **the list** / **the queue** | Every row dm knows, and the runner that starts them. | `dm_core::Queue` (`queue/mod.rs`, "A queue of downloads"): one per program, cheap to clone. |
| **a queue** (named) | A line rows wait in to be started later: "Main queue", "Recheck queue", or one a person makes. | A row's `queue: String` field; the Scheduler's queues. Not a `Queue`. This clash is the rename row A21 leaves for later. |
| **place** | An entry in the side panel: All, a kind, a person's category, Unfinished, Finished, a named queue. A filter over the one list. | `dm_shell::app::places::Place`. |
| **state** | Where a row has got to: Queued, Running, Paused, Completed, Failed (with its reason), Cancelled. | `dm_core::State`. "Settled" means Completed, Failed or Cancelled. |

## One download, inside the engine

| Word | What it is | In the code |
|------|------------|-------------|
| **probe** | The first request: what the server says of the file (its size, name, whether it takes ranges). | `probe::Probe` (`dm-core/src/probe.rs`). |
| **job** | The shared state of one download while it runs: its plan, ranges, files and connections. | `download::Job` (`download/mod.rs`, "Shared state for one download"). |
| **range** / **segment** | One stretch of a file's bytes, both ends included, fetched by one connection at a time. A range can be split while it runs. | `dm_core::Segment` (`plan.rs`). "Range" is the word in comments and docs; `Segment` is the type. |
| **ledger** | How much of a range is safely on disk, written after the bytes it speaks for. | `done-NNNN` files beside the plan. |
| **seam** | Where one range ends and the next begins; its bytes are fetched again and compared. | `assemble::check_seams`. |
| **staged file** | The file a ranged download writes every range into, at its own offset, before it is moved into place. | `Job::staged()`. |
| **part** | A file of its own for one piece of a stream or FTP download, named only when whole; also a cut of a video (a **span**: "from 1:30 to 2:05"). | `part_path` (`part-NNNN`); `clip::Span` for the cut. |
| **piece** | One request of a stream: an HLS segment, a DASH media piece, or a ranged slice of a one-file track. | `stream::Part` (HLS), `dm_core::dash::Piece` (DASH). |
| **stream** | A video or sound served as a playlist of pieces (HLS, DASH) rather than one file. | `dm-core/src/stream.rs`, `dm_core::dash`. |
| **control** | What lets a running download be paused or cancelled from outside. | `dm_core::Control`. |
| **limiter** | The speed limit, shared by every connection it covers. | `dm_core::Limiter` (the engine's); `dm_gpui::limiter::Limiter` is the Speed limiter window. |

## Around the engine

| Word | What it is | In the code |
|------|------------|-------------|
| **backend** | What a window or `--headless` holds: the queue, the settings in force, and the problems no row carries. | `dm_shell::app::backend::Backend`. |
| **session** | What one run of a window remembers that is not saved: When done, what was said, the watched folder's state. | `dm_shell::app::session::Session`. |
| **presentation** | Whatever shows dm to a person: the GPUI window, or the loop of `--headless`. | `dm-gpui`, `dm-desktop-base/src/headless.rs`. |
| **host** (browser's) | The small program a browser starts to hand a download to dm (native messaging). | `dm_shell::host`. |
| **host** (address's) | The server's name in an address: `example.com`. | `dm_core::host_of`. |
| **host** (JavaScript's) | Was the computer's JavaScript engine, run for YouTube's checks; dm now runs its own interpreter in process. | `dm-js`, `dm_core::javascript`. |
| **reference A**, **reference B** | The download managers dm is measured against. Never named in code, comments or docs. | `bench/`, `docs/engine-local/` (git-ignored). |
