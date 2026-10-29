# Architecture: UI and core layers

The full description is [how-it-works.md](how-it-works.md) and
[../AGENTS.md](../AGENTS.md). This page only classifies the code by layer, so
work on the engine knows which files the UI owns.

| Layer | Where | Notes |
|---|---|---|
| UI and presentation (protected) | `desktop/crates/dm-desktop/src/` (`app/`, `dialogs/`, `window.rs`, themes, `i18n/`) | egui window. Not changed by engine work. |
| Application state and orchestration | `dm-core/src/queue/` (`runner.rs`, `mod.rs`, `persist.rs`) | The queue the window reads (`queue.list`) and commands (`pause`, `resume`). |
| Core download logic | `dm-core/src/download/` (`mod.rs`, `ranged.rs`, `plan.rs`, `assemble.rs`, `files.rs`) | Probing, ranges, dynamic segmentation (`split_largest`), assembly, naming (`claim_path`). |
| Networking and protocols | `dm-core/src/` `lib.rs` (client), `probe.rs`, `ftp/`, `hls.rs`, `dash.rs`, `torrent.rs`, `metalink.rs`, `auth.rs`, `ntlm.rs`, `proxy.rs` | reqwest over rustls; one keep-alive pool per client. |
| Video pages | `dm-core/src/ytdlp.rs`, `ytdlp/fetch.rs` | yt-dlp reads the page; dm fetches the streams it found. |
| Filesystem and persistence | `download/files.rs`, `download/space.rs`, `queue/persist.rs`, `dm-shell/src/settings.rs` | Staged file plus a ledger per range; the queue file. |
| Scheduling and concurrency | `queue/runner.rs`, `limit.rs` (speed), quota | Tokio tasks; a speed limiter shared by connections. |
| Errors and recovery | `retry.rs`, `error.rs` | What is retried and how long to wait. |
| Integration and platform | `dm-shell/` (host, `--register`, lock), `dm-desktop/src/` platform files, `browser/`, `extension/` | Native messaging, tray, notifications. |

The UI talks to the core only through `dm_core::Queue` (and the control port
the same queue serves). Engine changes stay behind it.
