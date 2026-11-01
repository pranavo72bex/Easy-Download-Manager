# 01. Principles and boundaries

## What the engine is

The engine is `desktop/crates/dm-core`. The window (`dm-desktop`) and the
host (`dm-shell`) reach it only through `dm_core::Queue` and the control port
(`docs/architecture.md`). The browser extension (`browser/`, Rust compiled to
WebAssembly) catches downloads and hands them over; it does not move bytes
for files.

## "Built in" means

| Allowed | Not allowed |
|---|---|
| Rust libraries compiled into the dm binary (an HTTP client, TLS, a ZIP reader, a hash, an XML parser). They run in dm's process. | A separate program dm fetches, installs or starts to read a site or to fetch, join or convert media. |
| Reading a standard: RFC 9110/9111/9112, 7233 (ranges), 6265 (cookies), 7616 (Digest), 8216 (HLS), ISO/IEC 23009-1 (DASH), ISO/IEC 14496-12 (ISO BMFF), 14496-14, 14496-15, MPEG-2 TS (ISO/IEC 13818-1), WebVTT, Matroska. | A plugin system for third-party extractors. |
| Files that ship inside dm and are covered by dm's release checksums (a rules document, icons). | A script or program fetched at run time and run, even with a pinned digest (see file 06, option B). |
| The operating system's own services (decoders, a credential store, a clipboard), called as libraries. | Asking the person to install a tool first. |

Status: the repository meets this everywhere except one place. YouTube's
stream-address challenge is solved today by a JavaScript engine that dm
fetches (`tools.rs`) and runs on scripts that dm fetches and checks by digest
(`youtube.rs`). That is a helper program plus third-party scripts. File 06
sets out the choices; roadmap package WP-12 closes it. Mark: PARTIAL.

## Principles

| ID | Principle | Test |
|---|---|---|
| P1 | Bytes on disk are the truth. A range's ledger is written only after its bytes are flushed, so a crash loses in-flight bytes and nothing else. | Kill the process at random points while a logging server serves a patterned file; restart; the file is whole and no byte is fetched twice beyond the seam. (`tests/integrity.rs`; file 10, experiment E4.) |
| P2 | Never report a file complete that is not byte-exact. | Patterned fixture; compare every byte after every scenario in file 09. |
| P3 | Every input from outside (a header, a playlist, a manifest, a reply line, a page) is untrusted. Parsers check every index and never panic. | Fuzz tests (`tests/fuzz.rs`); `#![deny(clippy::indexing_slicing)]` stays on parsing modules. |
| P4 | A failure ends in words the person can act on, with the server's answer when it adds something. | One test per failure in file 09 that reads the message. |
| P5 | Waiting is bounded and polite: back-off between tries, a cap on tries, no tight retry loop against a failing server. | Logging server that drops every connection for 40 s; count attempts per second (file 10, E6). |
| P6 | Secrets (logins, cookies, tokens) never reach a log, a command line, a copied failure report, or a different host. | Redirect to another host: the logging server on it sees no `Authorization` or `Cookie`. (`redirects.rs`, `tests/auth.rs`.) |
| P7 | A site's video is got the way a person's browser gets it: from the requests the page's player makes, plus dm's own readers where a site's pages can be read plainly. | File 06 tests. |
| P8 | No protection is defeated. dm does not get around copy protection (`docs/decisions.md`, 2026-10-22). | A stream with `ContentProtection` or a non-AES-128 key is refused in words (file 05). |

## Clean-room rules

1. Behaviour comes from public standards, public pages of the reference
   programs, and black-box runs against a local logging server.
2. No binary of a reference program is opened, disassembled or decompiled.
   No plan to do so is followed. Names of functions, classes, strings or
   resources taken from a reference binary are not used, even as hints.
3. No artwork or wording of a reference program is copied.
4. Where a measurement shows the reference does something odd (for example
   hundreds of connection attempts a second against a failing server), the
   blueprint records it as an observation and says whether to copy it.
5. Provenance of every source is in
   [99-excluded-sources-and-provenance.md](99-excluded-sources-and-provenance.md).

## Layering

| Layer | Path | Rule |
|---|---|---|
| Wire | `lib.rs`, `redirects.rs`, `proxy.rs`, `auth.rs`, `ntlm.rs`, `cookies.rs`, `probe.rs`, `ftp/` | Knows bytes and protocols; knows nothing of queues or windows. |
| Transfer | `plan.rs`, `download/`, `retry.rs`, `limit.rs`, `progress.rs`, `checksum.rs` | Turns a probe into a whole file. |
| Streams | `stream.rs`, `dash.rs`, `download/hls.rs`, `download/dash.rs`, `fmp4.rs`, `tsmux/`, `mp4mux/`, `mkvmux.rs`, `clip.rs`, `live.rs` and the live protocols | Turns manifests and segments into one playable file. |
| Sites | `youtube.rs`, `videopage/` | Turns a page address into streams for the layer above. |
| Product | `queue/`, `schedule.rs`, `quota.rs`, `category.rs`, `control.rs` | Rows, states, rules, persistence. |
| Post | `mp3/`, `aac/`, `gif.rs`, `frames.rs`, `subtitles.rs`, `zipinfo.rs`, `extract.rs`, `motw.rs` | What happens after the bytes are in. |

A lower layer never calls a higher one. Tests for a layer use a local server
and no layer above it.

## Platform line

Everything in the wire, transfer, stream and product layers is the same on
Windows, macOS and Linux. Only post-processing that needs a system decoder
(AAC for MP3 and GIF) differs by platform; file 07 lists which.

## What is out of scope

Digital rights management; sites that need a person to pass a human check;
anything that reads a browser's cookie store directly (the extension hands
cookies over instead); kernel drivers for catching traffic.
