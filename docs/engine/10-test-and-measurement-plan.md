# 10. Test and measurement plan

Two jobs: prove that dm's engine meets this blueprint (tests), and learn
how reference A behaves where the maintainer wants dm to match it
(measurements). Both run against local servers that log everything. Neither
needs a look inside any program.

## Layers of proof

| Layer | What | Where | Runs |
|---|---|---|---|
| Unit | Arithmetic and parsers: plan, retry table, schedule, progress, manifests, headers, containers | `#[cfg(test)]` in each module | `cargo test --workspace` |
| Wire | A real local server and the real client, per scenario | `desktop/crates/dm-core/tests/*.rs` (about 300 tests today) | `cargo test --workspace` |
| Fuzz | Random and malformed input to every parser | `tests/fuzz.rs`, `fuzz.proptest-regressions` | `cargo test` |
| Stress | Many downloads, random pause/resume, kills, full disk | `scripts/stress-check.py`, `scripts/stress-protocols.py` | by hand before a release |
| Real sites | A few public files and one video page through the whole path | `scripts/real-sites.py`, `scripts/sites-check.py` | by a person with a network |
| Behaviour list | Every row of the contract in a visible window | `python3 scripts/human-qa.py` | before a merge and after any change in `desktop/`, `browser/`, `extension/` |
| Measurement | Reference A and dm side by side (below) | `scripts/compare/` (to be written; the first harness lives outside the repo) | by a person, on Windows |

## Test servers

All are small programs in the repository's test tooling (Rust or Python),
bound to 127.0.0.1, with JSON-lines logs. A byte at offset `n` of a fixture
is a function of `n` (and a seed) so any slice can be checked without a
copy.

| ID | Server | Controls | Log line has |
|---|---|---|---|
| T1 | HTTP logging server | size; ranges on/off; `Accept-Ranges` lying; per-connection speed cap; cut a connection after N bytes or after T seconds; accept then drop; refuse; status script by request number (503 for 12 s, then 200); `ETag`/`Last-Modified` on/off/changing; `If-Range` honoured or ignored; wrong `Content-Range`; redirects of every kind; `Content-Disposition` forms; cookies required; referer required; agent allow-list; chunked body; no length; max simultaneous connections (extra get 503) | time, connection id, source port, method, path, every header, status sent, bytes sent, how it ended |
| T2 | TLS variant of T1 | test CA; expired, wrong-name, self-signed certificates; TLS 1.2 only; h2 offered | handshake outcome |
| T3 | FTP logging server | logins allowed; REST on/off; PASV/EPSV; AUTH TLS; MLSD or LIST; MDTM; connection limit per client | every command and reply on the control connection, data connection open and close |
| T4 | Proxy logging server | HTTP CONNECT, SOCKS4, SOCKS5 (with and without login); refuse; delay | the bytes of the handshake, target asked for, login seen |
| T5 | Stream server | HLS (master, media, byte-range, AES-128, fMP4, TS, live that grows), DASH (template, timeline, list, index, dynamic, protected) | as T1 |
| T6 | Page server | HTML pages with each kind of video markup; JSON answers; a stand-in player that loads media | as T1 |
| T7 | Small disk | a disk image or quota-limited folder | the OS error |
| T8 | Process control | start, kill -9, suspend/resume, sleep the network (block the port) at chosen times | marks in a shared log |

A scenario is a short file: server settings, what to start, what to assert
(file hash, request counts, timing bounds, state of the row).

## Fixtures

* A patterned file in sizes 1 KB, 100 KB, 1 MB, 5 MB, 32 MiB, 100 MB, 1 GiB, 5 GiB (sparse).
* An MP4 with known frames (one keyframe per second, B-frames), a second
  with only sound, fragmented and plain versions; a TS stream with the same
  content; WebM pair.
* HLS and DASH sets generated from those at four qualities.
* ZIP files: normal, with a folder, Zip64, truncated, bomb, traversal.
* Player scripts for file 06's corpus (kept outside the repository if
  licensing is unclear; keep hashes in it).

## Automated acceptance: map from requirement to test

Every requirement in files 02 to 09 has a **Test** column. A package is done
when every test in it passes. The test names follow the requirement ID, for
example `s3_second_half_goes_to_the_free_connection`, so a failing test finds
its requirement with a search.

## The engine measurement list (gaps row 65) as black-box experiments

Gaps row 65 lists nine engine questions about reference A. Each is answered
from outside: reference A is started normally from its documented command
line, pointed at a local logging server, and the only evidence is what the
server logs, what appears in the download folder, and what the program shows.
No debugger, no disassembler, no memory reading, no packet capture of its
process, no look at its files except the downloaded ones and its visible
settings. dm runs the same scenario; the difference is listed and fixed or
recorded.

Common setup for every experiment: a clean Windows user, reference A freshly
installed with defaults (its connection number left at the default, then
repeated at 4 and 16 where it matters), the server on 127.0.0.1, fixture
files by size. Each scenario is run three times; use the median and report
the spread. Record the program's version and build. Use the program only as
its licence allows.

| ID | Row 65 item | Question | Scenario on T1 (or the server named) | What to record | Decision it feeds |
|---|---|---|---|---|---|
| E1 | (1) how a file is cut | First piece's size; where the largest remaining piece is split (middle or weighted by speed); smallest piece; most pieces | Fixtures 100 KB, 1 MB, 5 MB, 32 MiB, 1 GiB, each held to 512 KiB/s per connection; also unthrottled | For each file: the list of `Range` headers in request order with timestamps; the seam overlap in bytes; the smallest span ever asked; the largest number of distinct ranges | `MIN_SEGMENT`, `MAX_PIECES`, seam width (S1, S3, S7) |
| E2 | (2) connections by size; replacing a slow one | How many connections open for each size; how fast it ramps; what it does with one slow connection | Same fixtures; then T1 with one connection in every N held to 20 KB/s and others to 2 MiB/s; then a server limiting 2 simultaneous connections (extra ones get 503) | Connections open over time (from the server's accept log); time to finish; whether the slow one is closed and its span re-asked | S2, S5, S6 |
| E3 | (3) retries after errors | What it asks again after a cut, 4xx, 5xx, reset, a server taking only some connections; how long it waits; how many tries | One scenario per case: cut at 30%; 404; 403; 429 with `Retry-After`; 500; 503 for 12 s then 200; reset by peer after headers; accept and close; first connection fine, others refused | For each: request timestamps, the gap between attempts, the number of attempts, the final state shown and the file hash | S11 table, R-rows |
| E4 | (4) resume after Stop and after kill | Which byte it asks for; what it re-reads to check; what it does when the file changed (validator, size) | Stop from its window at 40%, then Start; kill its process at 40%, then start; then change the file (same size; different size; same ETag) before the restart; server with and without `If-Range` | The first range asked after restart (compare to what it had written), any overlap, headers sent (`If-Range`, `If-Match`), whether it restarts whole | S9 |
| E5 | (5) the data check | When it asks a span a second time; what it does on a difference | T1 that serves a wrong byte at a chosen offset on the second request for it; T1 that serves different content on alternate connections | Which spans are asked twice, when; what is shown and saved; the final file against the fixture | S7, S10 |
| E6 | (3) continued: outage | How long it waits and how often it tries against a server that is down or dropping | Server off for 30 s then on; server accepting and dropping every connection for 40 s; server refusing | Attempts per second; time from the server's return to the first byte; whether it ever gives up and after how long | S12 and P5. Remember dm should not copy hundreds of attempts a second; copy the fast return only if it can be had with a bounded schedule |
| E7 | (6) speed and time left | Sampling window and smoothing; behaviour at start, on a stall, after a pause | A server whose rate steps (2 MiB/s, then 200 KiB/s, then 2 MiB/s) at known times; record screenshots or window text every second (the window is read, not the program) | The speed and time-left series against the true rate | S20 constants (weight 0.3 today) |
| E8 | (7) FTP | Command sequence; REST/RETR; passive or active; number of connections; behaviour when a server allows few | T3 with REST on/off, 1 or 2 logins allowed | The control connection transcript; how many data connections; the REST offsets | N19, N20 |
| E9 | (8) proxies | The CONNECT request; SOCKS4 and SOCKS5 handshakes and login | T4 in each mode, with and without login | The handshake bytes; target form (name or address) asked for | N8 |
| E10 | Headers | Agent, accept headers, referer rule, language, encoding | Any T1 scenario | Every request header line; compare for the first request, the ranges, and after a redirect | N13, N14 |
| E11 | (9) finished parts | Whether and how parts are joined; free disk it needs; what is on disk during and after | Fixture 1 GiB; watch the folder and free space every second; small-disk image (T7) with room for the file but not twice the file | Files present over time; peak space used; time from last byte received to "complete" | S8, S15 |

Notes for running E1 to E11:

* The first harness (outside the repository) already covers the first
  requests and E3/E6 in part (`docs/reference-behavior.md` records what it
  found on 2026-10-31). Move it into the repository (WP-02) so the numbers
  are repeatable.
* Do not infer a program's internals. If a result is not clear, add a
  scenario, do not look inside.
* Write each result as a row in `docs/reference-behavior.md` with the label
  Observed and the date, then a decision row in `docs/decisions.md` if dm
  changes. The maintainer's rule, "exactly like reference A where dm is not
  already better", is applied one row at a time and approved in words.
* A measurement never changes a contract row by itself.

## Performance targets

| Case | Target |
|---|---|
| 168 MB at 1 MB/s per connection | At least as fast as 8 connections allow, within 5% of file size / (8 x 1 MB/s) |
| 32 MiB at 2 MiB/s per connection | Under 3 s (helpers, S5) |
| 100 KB file | One round trip beyond the probe for small pieces if ranges help; not slower than one connection |
| 1 GiB unthrottled loopback | At least 80% of a single `curl`-class client's rate; CPU under one core |
| Memory | Under 100 MB for a 5 GiB download with 32 connections; flat over time (`tests/memory.rs`) |
| Start-up | Row in "Running" within 300 ms of a click on loopback |

## What a person checks by hand

The behaviour contract list (`scripts/human-qa.py`), a real YouTube page
(`scripts/real-sites.py`), and the rows that need a person at a real
computer: gaps rows 11, 14, 20, 24, 57, 60, 62, 63, 64.
