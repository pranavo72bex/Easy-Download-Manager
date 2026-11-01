# dm engine blueprint

A plan an engineer can follow to build, check and finish dm's own download
engine: everything built in, with no external video extractor, no third-party
plugin and no helper program to install or fetch. It says what the engine
must do, which part of `desktop/crates/dm-core/src` already does it, and how a
person or a test proves it.

This is documentation only. It changes no behaviour. The behaviour contract
([../behaviour-contract.md](../behaviour-contract.md)) still overrides it:
nothing here licenses a change to a row of that list without the maintainer's
word.

## Status legend

| Mark | Meaning |
|---|---|
| BUILT | Code exists in the path named and a test or measurement in the repo exercises it. |
| PARTIAL | Code exists but a case is missing, or it was written and never run on the target (a platform, a real site). The row says which. |
| TO BUILD | Nothing exists, or only a stub. |
| DECISION | The maintainer must choose before work starts. |

A mark describes the repository as read on 2026-11-01. A change in the working
tree after that date can make a mark stale; check the code path before you
rely on a mark, and fix the mark in the same commit as your change.

## How to follow it

1. Read [01](01-principles-and-boundaries.md) first. It fixes what "built in"
   means and what is out of bounds.
2. Pick a work package from [11-roadmap.md](11-roadmap.md). Each names its
   dependencies and the gaps rows it closes.
3. Open the file for that area. Each requirement has an ID, a status, the code
   path, and a **Test** line.
4. Write the test first when it can be automated (a local server from
   [10](10-test-and-measurement-plan.md)). Make it fail, then build.
5. When the package is done: run `cargo test --workspace`, update the status
   mark here, update `docs/gaps.md` (delete a closed row), and add a
   `docs/decisions.md` entry if a decision was made.

A requirement is closed only when its Test line passes. "Code written" is not
closed.

## Reading order

| File | What it holds |
|---|---|
| [01-principles-and-boundaries.md](01-principles-and-boundaries.md) | Scope, "built in", clean-room rules, layering |
| [02-connection-and-http-layer.md](02-connection-and-http-layer.md) | TLS, proxies, auth, cookies, redirects, referer, agent, probe, mirrors, FTP control |
| [03-segmentation-resume-and-integrity.md](03-segmentation-resume-and-integrity.md) | Range planning, splitting, helpers, retries, resume, assembling, checksums |
| [04-queue-scheduler-and-limits.md](04-queue-scheduler-and-limits.md) | Queues, concurrency, schedule, quota, speed limit, persistence |
| [05-streams-hls-dash-mp4.md](05-streams-hls-dash-mp4.md) | HLS, DASH, segments, part cutting, muxing |
| [06-site-and-video-extraction-without-external-tools.md](06-site-and-video-extraction-without-external-tools.md) | Own readers per site kind, the signature problem and the options |
| [07-post-processing-and-formats.md](07-post-processing-and-formats.md) | Audio, GIF, subtitles, ZIP listing, categories, file marks |
| [08-browser-integration-protocol.md](08-browser-integration-protocol.md) | Extension, native messaging, hand-off, rules |
| [09-failure-modes-and-recovery.md](09-failure-modes-and-recovery.md) | Every failure, what the engine does, how to provoke it |
| [10-test-and-measurement-plan.md](10-test-and-measurement-plan.md) | Test servers, fixtures, the black-box measurement list |
| [11-roadmap.md](11-roadmap.md) | Ordered work packages, sizes, dependencies, gaps rows |
| [99-excluded-sources-and-provenance.md](99-excluded-sources-and-provenance.md) | What was read, what was left out and why |

## Glossary

| Term | Meaning |
|---|---|
| Reference A | The commercial Windows download manager dm is measured against. Its behaviour is known from its public pages and from running it normally against a local server (`docs/reference-behavior.md`). Never opened, disassembled or decompiled. |
| Reference B | A free download manager used the same way. |
| Probe | The first request to a server to learn size, range support, name, type and validators. |
| Range / segment / piece | A byte span of the file, both ends included, fetched by one request. |
| Ledger | Small per-range record of how many bytes are safely on disk. Written after the bytes. |
| Staged file | The one file the ranges are written into at their own offsets; renamed into place when whole. |
| Seam | The overlap of bytes re-read at the edge of two ranges to prove they join. |
| Helper connection | A connection opened beyond the planned ranges because the others are slow. |
| Validator | `ETag` or `Last-Modified`, sent back in `If-Range` to prove the file did not change. |
| Manifest | An HLS playlist (`.m3u8`) or DASH description (`.mpd`) listing media segments. |
| Mux | Put a picture track and a sound track into one container file. |
| Caught request | A media request seen by the browser extension on a page, handed to dm. |
| Signature / challenge | A value a video site makes the player compute (by running its own script) before the server will serve a stream. See file 06. |
| Logging server | A local test server that records every connection, request, header and byte it sends (file 10). |
| Contract row | A line `Bnn` in `docs/behaviour-contract.md`. |

## Source of truth

If this blueprint and the code disagree, the code is what runs; fix the
blueprint. If it and the behaviour contract disagree, the contract wins.
