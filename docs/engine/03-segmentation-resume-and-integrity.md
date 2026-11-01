# 03. Segmentation, resume and integrity

Goal: a file comes out byte-exact, as fast as the server and the link allow,
and survives any stop: a pause, a cut connection, a crash, a power loss, a
server that changes the file.

Standards: RFC 9110 section 14 and RFC 7233 (ranges, `206`, `416`,
`Content-Range`), RFC 9110 section 13.1.5 (`If-Range`), RFC 9111 (validators).

## Data structures

| Name | Fields | Rule |
|---|---|---|
| Range (`plan::Segment`) | `start`, `end` (both inclusive), `done` | `cursor = start + done`. Finished when `done >= end - start + 1`. A file of unknown length is one range ending at the maximum value; lengths saturate. |
| Plan | list of ranges, covering `[0, size-1]` once, no overlap | Rebuilt on each run from the ledgers and the probe. |
| Staged file | one file in the destination folder, pre-sized sparse where the file system allows | Every range writes at its own offset. Renamed to the final name when every range is finished and checks pass. |
| Ledger | `done-NNNN` per range, beside the plan | Written after the bytes it vouches for are flushed, never ahead. At most every few seconds while running, and at once on pause. |
| Row (queue) | address, final address, name, size, validators, status, per-range state, secrets (kept apart) | Persisted by the queue (file 04). |

## Requirements

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| S1 | **Initial split.** With a known length and range support, cut the file into pieces so every connection has work at once, and keep pieces at least a minimum size. Today: connections up to 32, a minimum piece of 1 MiB, up to 4 pieces per connection, at most 128 pieces. The first piece is the probe's own response, left streaming. | BUILT | `plan.rs` (`MIN_SEGMENT`, `MAX_CONNECTIONS`, `PIECES_PER_CONNECTION`, `MAX_PIECES`), `download/ranged.rs` | Logging server, files of 100 KB, 5 MB, 100 MB, 4 GB: the log shows the ranges asked; they tile the file once; the count of connections follows the table in file 10, E2. |
| S2 | **Connections per file.** The person sets a number (default 8, presets 2, 8, 16, 32, contract B56). A per-server exception table lowers it for servers that limit clients. A `503` or `429` on an extra connection lowers the ceiling for that server for this run and the refused ranges go to the connections that remain (B09). | BUILT | `download/mod.rs` (`connections_for`), `ranged.rs` | Server accepting 2 connections and answering `503` to a third: file complete; the log shows the third was not retried in a loop. |
| S3 | **Dynamic splitting.** When a connection has nothing left, it takes the second half of the largest unfinished range; the connection already on that range is shortened to the first half (B06). A range is not split below the minimum. | BUILT | `plan.rs` (`split_largest`), `download/ranged.rs`, `docs/decisions.md` 2026-10-15 | Fast server, 3 pieces, 8 connections: the log shows halves of the largest remaining span; no two connections ask the same byte except the seam. |
| S4 | **More pieces than connections.** Finished connections take the next piece in turn (B07). | BUILT | `ranged.rs` (`docs/decisions.md` 2026-10-14) | Requests greater than connections in the log. |
| S5 | **Helper connections.** When the bytes arriving are slow enough that more than 0.3 s of the file would still come after a new connection's round trip, a helper opens and takes half of the largest range. It waits 100 ms, watches 80 ms, and gives up waiting for a rate after 1.5 s. With a speed limit set, no helper. | BUILT | `download/ranged/helping.rs` | Server that holds each connection to 2 MiB/s, 32 MiB file: finishes in a few seconds, not 16 (B08). Fast server: no helpers (connections equal planned). |
| S6 | **Replace a slow connection.** If one connection is far slower than its siblings for a window, close it and give its remaining span to a new connection or a sibling. | TO BUILD | none | Server that holds one in eight connections to 20 KB/s and the others to 2 MiB/s: with the feature the file is not held by the slow one; measure time against a run without it (E2). |
| S7 | **Seam check.** The end of a range is fetched again from the next range's first bytes (an overlap) and compared; a mismatch means a mixed file. Today the overlap is 1 byte. | BUILT (small) | `tests/seam_check.rs`, `ranged.rs` | Server that serves a wrong byte at one boundary: detected, file refetched (S10). Measurement E5 decides whether to widen the overlap. |
| S8 | **Writes.** Each connection writes into the staged file at its offset with a bounded buffer (16 KiB) and no more than a bounded number of bytes in memory. Space is checked at the start and shared between downloads under way. A write error (disk full, permission) pauses the row with the cause in words (B54). | BUILT | `download/files.rs`, `space.rs` | Small disk image as the download folder: file of 50 MB is paused with a "not enough room" message; two files of 20 MB at once land one and hold the other. |
| S9 | **Resume validation.** On resume: probe again; compare size and validator with the row's. Same: each range goes on from its cursor with `Range: bytes=cursor-end` and `If-Range` (strong ETag). The end of what each ledger vouches for is fetched again and compared (a look-back window) to repair a torn write after a power cut. Different size, or `200` to an `If-Range`, or a different strong ETag: the file changed, start whole again (after the person is told when the setting says so). A server that ignores `If-Range` is caught by the look-back comparison on the first chunk of each resumed range. | BUILT | `ranged/staged.rs`, `queue/recheck.rs`, `assemble.rs`, `docs/decisions.md` 2026-10-15, 2026-10-17 | E4: kill dm at random times, resume; compare. Variant: change the file under the same ETag, then cut: the first 16 KiB of each resumed range mismatches, the whole file is refetched on all connections, the result equals the new file. |
| S10 | **Data check on a difference.** If a re-read span differs from what is on disk, the range is discarded back to the last good ledger and refetched; if the same range differs twice, fall back to one connection and say so in words. | PARTIAL (look-back and refetch BUILT; the one-connection fallback and its message need a test) | `ranged/staged.rs` | Server that returns different bytes for the same range on alternate requests: the download ends byte-exact on one connection, and the row says why. |
| S11 | **Retries.** Retry only what can work: 408, 425, 429, every 5xx except 501, a cut or timed-out connection, a body ended short. Not: other 4xx, a certificate error, a malformed reply, a disk error, a changed file. At most 5 attempts per range, with exponential back-off (1, 2, 4, 8, 16 s) capped, and `Retry-After` obeyed up to a cap. After the cap the row is paused or failed per the "tries" setting; the number of rounds is a setting. | BUILT | `retry.rs` (`MAX_ATTEMPTS`, `transient`), `tests/errors.rs` | 503 for 12 s then 200: attempts at 1, 2, 4, 8 s then the download (reference A stops after 2 tries; dm waits it out). 404: one attempt, a plain message. |
| S12 | **Outage patience.** If every connection fails with a network error (refused, reset, timed out) the row is held "waiting for the network" and probed on a short bounded schedule (for example every 1 s for 10 s, then every 5 s), not abandoned and not retried hundreds of times a second. It resumes within a second or two of the server's return. | PARTIAL: dm resumed 22 s after a 40 s outage (measured); a tighter probe schedule is TO BUILD | `retry.rs`, `queue/runner.rs` | Server dropping every connection for 40 s: attempt rate under 5 a second; resumed within 3 s of return; file correct (E6). |
| S13 | **No ranges.** One connection, whole file. A cut mid-way on such a server restarts from byte 0 (nothing else is possible) and says so; the row keeps the partial file only if the server later shows it can range. | BUILT | `download/plain.rs` | Server without ranges, cut at 3 s: whole file again, byte-exact, message names the cause. |
| S14 | **Unknown length.** No `Content-Length` and no ranges: stream to the end of the body; a chunked body is complete at the zero chunk; the size is learned at the end. A server that says the file is longer than it sends: complete at the end of the stream, not "running" forever (B11). | BUILT | `download/plain.rs` | Server closing early; server with chunked body of unknown size. |
| S15 | **Assembling.** There is no join step for plain files: the staged file is renamed. Streams that stage a part file per piece (HLS, FTP) join them by append in order, then rename. Rename is atomic on the same volume; if the final name is taken, a free name is claimed (`claim_path`). | BUILT | `download/assemble.rs`, `files.rs` | Ten-way download: no second file of the same size ever on disk beyond the staged file. |
| S16 | **Checksums.** MD5, SHA-1, SHA-256, SHA-512 against a published digest (from a Metalink, a sidecar, or the person), computed while the file is streamed back or after; a mismatch fails the row and keeps the file for inspection. SHA-256 shown in Properties. | BUILT | `checksum.rs`, `metalink.rs` | Fixture with the right and a wrong digest. |
| S17 | **Time on the server.** Optionally set the saved file's modified time from `Last-Modified`. | BUILT | `tests/server_dates.rs` | Server date in the past; the file's time equals it. |
| S18 | **Re-download when changed.** A row can be re-checked: probe again; if size or validator differs, offer a new download. | BUILT | `queue/recheck.rs`, `queue/mod.rs` (`differs`) | Change the file; trigger recheck. |
| S19 | **Large files.** Sizes above 4 GiB and 2^53 are handled as 64-bit; sparse pre-sizing falls back where unsupported (FAT32 4 GiB limit is told in words before start). | PARTIAL | `plan.rs`, `files.rs` | 5 GiB sparse fixture; a FAT32 image refusing it. |
| S20 | **Speed and time left.** Bytes per second smoothed by a weighted average over short samples (weight 0.3 today); time left from the smoothed rate and bytes remaining; unknown when there is no basis. | BUILT | `progress.rs` | Feed samples; compare to expected values; E7 measures how the reference's number moves. |
| S21 | **Expired links.** A signed address that expires and answers 403, 404 or 410 on resume waits for the browser's next hand-over of the same file (matched by name, exact size and server family) and goes on from where it was. | PARTIAL (built; offline tests only; cannot match a row that failed at its first request) | `queue` (`fresh_link`, `adopt_fresh_link`), `tests/fresh_link.rs`, gaps row 57 | Local server whose address stops working at 40%: row waits; a second hand-over of the same file with a new address continues; byte-exact. Real site: row 57. |

## Procedure: one ranged download

1. Probe (N3). Decide: ranged, plain, or refused.
2. Build the plan (S1) from the row's ledgers. A new row gets pieces; a
   resumed row gets its saved pieces.
3. Create or open the staged file; ensure room (S8).
4. Start connections up to the ceiling (S2). Each takes the next unfinished
   range or splits (S3).
5. Each chunk is written, counted, and charged to the speed limiter (file 04).
6. A failure goes to the retry policy (S11, S12).
7. When every range is finished: seam checks (S7), checksum (S16), rename
   (S15), set time (S17), mark the file (file 07), report complete.

## What makes it correct

* A range's `done` never exceeds the bytes flushed (P1).
* A range is only extended by bytes whose position the server confirmed in
  `Content-Range`. If the `Content-Range` start is not the requested start,
  the response is discarded and the range retried.
* A `206` whose total differs from the probe's size marks the file changed.

## Tests to write first

`tests/integrity.rs` exists with 16 tests. Add: S6 (slow-connection
replacement), S10 (alternating bytes), S12 (outage attempt rate), S19
(5 GiB sparse). Each uses the logging server of file 10.
