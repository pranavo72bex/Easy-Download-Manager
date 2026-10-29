# CLEANUP — dead code, unused files, build leftovers

Done on 2026-10-29, after the Docker QA suite (`qa/`) passed three runs in a
row (BUGLOG.md). Every check below was run; where it found nothing, nothing
was removed. Nothing was deleted on a guess.

## What was checked, and what it found

| Check | Result |
|---|---|
| `cargo clippy --all-targets -- -D warnings` (desktop workspace, browser) with the default `dead_code` and `unused` lints | clean: no dead code, no unused imports or variables |
| `cargo machete` (desktop workspace, browser) | no unused dependencies |
| `cargo hack --each-feature` | not needed: no crate declares a Cargo feature |
| `knip` / `depcheck` (extension) | not applicable: the extension has no npm packages or `package.json`; its JavaScript is three loaders and the popup |
| Manifest permissions (`downloads`, `contextMenus`, `cookies`, `nativeMessaging`, `notifications`, `scripting`, `storage`, `webRequest`) | each is used (`browser/src`, `extension/*.js`; native messaging through `runtime.connectNative`) |
| Manifest files and assets | the panel's files are no longer offered to pages (`web_accessible_resources` removed with BUG-003); every file a manifest names ships (`scripts/package.sh` checks it) |
| `TODO`, `FIXME`, `XXX`, `todo!()`, `unimplemented!()` | none outside the vendored winit copy |
| Commented-out code | none: every line that looks like code in a comment is prose |
| `unwrap`/`expect`/`panic!` on the engine's path | already denied outside tests in every crate (BUGLOG.md, "Loop runs") |
| Committed build output and junk (`target/`, `dist/`, `node_modules/`, `*.zip`, `*.crx`, `*.exe`, `*.msi`, logs, `.DS_Store`, `Thumbs.db`, editor folders) | none in `git ls-files` |
| Large committed files | the six screenshots in `docs/` (0.4–1.1 MB each) are all linked from README or docs; the fonts are compiled into the app; `scripts/fixtures/clip.webm` is used by a check |

## What was changed

- `.gitignore`: `qa/` was ignored as a whole ("internal QA notes"); now only
  the suite's own files are tracked (`qa/run.sh`, `docker-compose.yml`,
  `app/`, `lib/`, `origin/`, `mockvideo/`, `tests/`) and anything else put in
  `qa/` (notes, `qa/reports/`) stays ignored. Added `**/__pycache__/`,
  `Thumbs.db`, `*.swp`, `.vscode/`, `.idea/` so they cannot be committed.
- Nothing else: no file or dependency was found unused.

## Sizes

| | Before | After |
|---|---|---|
| Repository (`.git`) | 19 MB | 19 MB (nothing removed; history not rewritten) |
| Tracked files | 716 | 716 + the QA suite and BUGLOG.md / CLEANUP.md |
| Release `dm-desktop` (macOS arm64) | — | 24.1 MB (`cargo build --release`, LTO) |
| Extension zips | — | 488 KB each: `pkg/panel.js` (BUG-003) carries the panel's WebAssembly a second time, as base64, so a page fetches nothing of the extension's |

No large file was found in history that would need rewriting.

## Verified after

- `cargo test --workspace` (desktop) and `cargo test` (browser): pass;
  `cargo fmt --check` and clippy with `-D warnings`: clean.
- `cargo build --release -p dm-desktop`: builds; `sh scripts/package.sh`:
  both extension packages built and checked.
- The Docker suite builds dm and the extension from scratch inside its image
  on every run (a clean build): three runs in a row green.

## Needs confirmation

Not deleted: someone who knows why they are kept should decide.

- `UX_REPORT.md` (repository root) and `docs/noob-ux-report.md`: dated audit
  reports that no other file links to. They read as records of a review, not
  as living docs; keep them, link them from `docs/testing.md`, or remove them.
- `scripts/`: 40 check scripts, most of which need a browser, a desktop
  session, Windows or a real network; not all were run here, so none is
  called dead.
