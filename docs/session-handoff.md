# Session handoff — 2026-11-01

Branch: `parity-with-reference-a`. All code is written but most is not yet
built or tested. This file records exactly where work stands so the next
session picks up without loss.

---

## Build status

`cargo build --workspace --all-targets` (CARGO_TARGET_DIR=/d/dm-target-r60) is
running in the background. Earlier rounds had two compile errors, both fixed:

1. `host_of` name collision in dm-core/src/lib.rs: renamed the private copy to
   `server_of`.
2. `ProxyKind` not in scope in dialogs/options/mod.rs and tabs.rs: imported as
   `use dm_core::ProxyKind as Kind`.

If any new errors appear run `cargo build` again and fix by reading the error
message and the file it points to. The warnings (dead_code in aac/) are expected
because the AAC decoder is gated off.

---

## What is NOT yet committed / pushed (working tree changes)

These files were changed by `cargo fmt --all` and by the build-fix edits.
Commit them before starting tests:

```
desktop/crates/dm-core/src/aac/decoder.rs     (fmt + mut fix)
desktop/crates/dm-core/src/aac/mod.rs         (fmt)
desktop/crates/dm-core/src/aac/transform.rs   (fmt)
desktop/crates/dm-core/src/dash/tests.rs      (fmt)
desktop/crates/dm-core/src/download/plain.rs  (fmt)
desktop/crates/dm-core/src/mp4mux.rs          (fmt)
desktop/crates/dm-core/src/mp4mux/part.rs     (fmt)
desktop/crates/dm-core/src/queue/persist.rs   (fmt)
desktop/crates/dm-core/src/secretstore.rs     (fmt)
desktop/crates/dm-core/src/stream/tests.rs    (fmt)
desktop/crates/dm-desktop/src/dialogs/options/mod.rs   (ProxyKind fix)
desktop/crates/dm-desktop/src/dialogs/options/tabs.rs  (ProxyKind fix)
browser/src/decide/excluded.rs                (new exclusion list + test)
.gitignore                                    (engine-local/ added)
docs/engine/  (all 13 files — rm from tracking, kept locally under docs/engine-local/)
```

Commit command (all fixes in one):
```
cd desktop && git add \
  crates/dm-core/src/aac/decoder.rs \
  crates/dm-core/src/aac/mod.rs \
  crates/dm-core/src/aac/transform.rs \
  crates/dm-core/src/dash/tests.rs \
  crates/dm-core/src/download/plain.rs \
  crates/dm-core/src/mp4mux.rs \
  crates/dm-core/src/mp4mux/part.rs \
  crates/dm-core/src/queue/persist.rs \
  crates/dm-core/src/secretstore.rs \
  crates/dm-core/src/stream/tests.rs \
  crates/dm-desktop/src/dialogs/options/mod.rs \
  crates/dm-desktop/src/dialogs/options/tabs.rs && cd ..
git add browser/src/decide/excluded.rs .gitignore
git -c user.name=pranavo72bex -c user.email=pranavj304@gmail.com commit \
  -m "Build fixes: ProxyKind import, host_of rename; fmt; exclude OS-update and sticker hosts" \
  -m "Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>" \
  -- desktop/crates/dm-core/src/aac/decoder.rs \
     desktop/crates/dm-core/src/aac/mod.rs \
     desktop/crates/dm-core/src/aac/transform.rs \
     desktop/crates/dm-core/src/dash/tests.rs \
     desktop/crates/dm-core/src/download/plain.rs \
     desktop/crates/dm-core/src/mp4mux.rs \
     desktop/crates/dm-core/src/mp4mux/part.rs \
     desktop/crates/dm-core/src/queue/persist.rs \
     desktop/crates/dm-core/src/secretstore.rs \
     desktop/crates/dm-core/src/stream/tests.rs \
     desktop/crates/dm-desktop/src/dialogs/options/mod.rs \
     desktop/crates/dm-desktop/src/dialogs/options/tabs.rs \
     browser/src/decide/excluded.rs \
     .gitignore
git push origin parity-with-reference-a
```

---

## Next steps in order

### 1. Finish build and fix all errors
Run: `cd desktop && CARGO_TARGET_DIR=/d/dm-target-r60 cargo build --workspace --all-targets`

### 2. cargo test --workspace
Run: `cd desktop && CARGO_TARGET_DIR=/d/dm-target-r60 cargo test --workspace`
Known flaky tests (rerun individually if they fail):
- `an_unreachable_folder_pauses_and_another_folder_finishes_it`
- `one_slow_connection_does_not_hold_the_download_up`
- `removing_many_rows_writes_the_queue_once`
- `the_doorbell_wakes_the_inbox_watcher_at_once`
- `a_double_click_opens_properties`
- `five_hundred_rows`
Known failing at base commit: `a_finished_download_hands_its_place_to_the_next_at_once` (timing)

### 3. Push everything
`git push origin parity-with-reference-a`

### 4. Engine improvements from reference material (clean-room)
Source: docs/engine-local/requirements.md (git-ignored, 166 items)

**WP-1: Data integrity spot-check (ENG-D12)**
Reference A re-downloads a completed byte range to verify it. After a segment
finishes, dm should: pick the last 4 KB of the just-written segment, re-fetch it,
byte-compare it. If it differs: log "data check failed", fall back to single
connection and restart. Code in desktop/crates/dm-core/src/download/ranged.rs
near fetch_segment (line 410+).

**WP-2: FTP MDTM-change restart (ENG-G6)**
Reference A watches `Last-Modified` / `MDTM`: if it changes between resume
connections, the download restarts. dm/ftp.rs: check if MDTM is saved and
compared on reconnect. Currently: NOT done.

**WP-3: HLS/DASH scheme prefixes from extension (ENG-J5)**
Reference A passes a stream's type to its app through URL scheme prefixes of
its own, one per kind (HLS, a second HLS form, DASH). dm's
browser/src/decide/streams.rs: check every kind is handled and the correct
download type is started. Currently: PARTIAL.

**WP-4: Content-type HTML check first bytes (ENG-C4)**
Reference A's CContentTypeChecker::CheckHTML detects when first bytes are HTML
(an error page instead of the file). dm probe.rs: check Content-Type header only,
not first bytes. Add: if response starts with `<!DOCTYPE`, `<html` or `HTTP/`
when Content-Type says binary, treat as error page.

### 5. Human QA (with maintainer)
Run: `python3 scripts/human-qa.py` in a visible window
Must be done together. All rows B1-B59 must pass.

### 6. Rows done with the maintainer (sign-in required)
- Row 11: High Contrast theme (test on Windows)
- Row 14: Focus Assist / notifications held
- Row 37: YouTube proof-of-origin token (lay out options first)
- Row 62: Facebook (maintainer signs in)
- Row 63: Reddit (maintainer signs in)

---

## Gaps.md open rows summary

| Row | What remains |
|-----|-------------|
| 11  | Visual QA with maintainer |
| 14  | Visual QA with maintainer |
| 20  | Stress test gaps (clipboard, sleep) |
| 24  | Real mouse/keyboard QA |
| 27  | Enable missing_docs lint at final build |
| 28  | 11 modules partly done; root re-exports added |
| 30  | Brave CSP warning (WASM load) |
| 37  | YouTube bot-check token (decide with maintainer) |
| 39  | macOS/Linux stores not compiled or tested |
| 45  | Part line written, not seen on real page |
| 47  | Second path for HLS/DASH written, not tested with phone |
| 51  | Mozilla AMO signing refused (ZIP issue) |
| 53  | Linux AAC decoder: Huffman tables missing |
| 54  | Linux GIF: Linux H.264 not built |
| 57  | Expired-address resume: needs real expired link test |
| 60  | Movable dialogs: visual QA |
| 62  | Facebook: requires sign-in |
| 63  | Reddit: requires sign-in + challenge |
| 64  | Stream quality names: needs real page |
| 65  | Engine measurements: needs reference A running |
| 66  | Interface comparison: needs reference A |
| 67f | Connections-per-server in Options (written, not tested) |
| 68  | Rules via update manifest written; no site has rules yet |
| 69  | Page hook written; no site has a profile yet |
| 70  | App-drawn floating video button: not built |

---

## Files in docs/engine-local/ (git-ignored, stays local)
- requirements.md: 166 items, 14 areas, 17 work packages
  Source: the maintainer's reference notes (local, clean-room)
  111 items from static-analysis notes, rest from observation/standards
  Numbers marked "to measure" need black-box experiments with reference A

## Key architecture decisions (docs/decisions.md)
- Secrets: sealed with ChaCha20-Poly1305 under Windows Credential Manager key
- macOS keychain: `security -i` stdin (not command line) — security fix 9762f0f
- Rules update: TLS fetch + SHA-256 digest; no signature on manifest yet
- Browser panel Part line: optional from/to on dm-protocol Request
- Linux AAC: decoder skeleton only, COMPLETE=false until Huffman tables supplied
