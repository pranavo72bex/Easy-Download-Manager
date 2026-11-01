# 09. Failure modes and recovery

Each row: what goes wrong, what the engine must do, what the person sees,
and how to provoke it. "Server" means the local logging server of file 10.
Every row ends with a byte-exact file or a failure in words; nothing ends
"running forever".

| ID | Failure | Engine behaviour | Person sees | Status | Provoke and check |
|---|---|---|---|---|---|
| R1 | Connection refused / reset / timed out on one range | Retry that range with back-off, other ranges go on | Nothing, or a note in the details | BUILT | Server resets 1 in 10 connections; file exact. |
| R2 | Every connection fails (outage) | Hold "waiting for the network"; bounded probing; resume within seconds of return | "Waiting for the network" | PARTIAL (S12) | Server down 40 s at 30%. |
| R3 | DNS failure | Same as R2, with "cannot find the server" after the rounds | Words naming the host | BUILT | Name that does not resolve. |
| R4 | TLS certificate untrusted, expired, wrong name | Fail at once, no retry; never offer to bypass silently | "The certificate could not be checked", the reason | BUILT | Server with a bad certificate. |
| R5 | 401 / 407 | Ask for a login once; try the schemes (N10); wrong password fails once | Login box, then words | BUILT | Auth servers. |
| R6 | 403 | Try next user agent (N13), then referer rule (N14); then fail | "The server refused", with its words if useful | BUILT/PARTIAL | Server that wants a referer. |
| R7 | 404 / 410 | Fail at once, no retry (B14, B41) | "This link no longer works" and one window | BUILT | Server 404. |
| R8 | 408, 425, 429, 5xx except 501 | Retry with back-off and `Retry-After` | Nothing until tries run out | BUILT | 503 for 12 s. |
| R9 | 416 on a resume | The file is shorter or already whole: probe; if the size equals the ledger total, finish; else restart | Nothing or a note | BUILT | Server returns 416. |
| R10 | `200` where `206` expected | Server ignores ranges: if on a resumed range, treat as "no ranges", restart on one connection | Note "server does not resume" | BUILT | Server that ignores ranges after the probe. |
| R11 | File changed on the server | Detected by validator (S9), size, or look-back mismatch; refetch whole | Note "the file changed on the server" | BUILT | Change under the same ETag, then cut. |
| R12 | Link expired | Wait for a fresh hand-over (S21) | "Waiting for a new link" with how to give it | PARTIAL | Address stops working at 40%. |
| R13 | Disk full | Pause with the numbers; resume when room exists (B54) | "Not enough room: needs X, has Y" | BUILT | Small disk image. |
| R14 | Folder not writable | Refuse before starting, or pause with the folder named | Words and both ways out | BUILT | Read-only folder. |
| R15 | Target name taken | Claim a free name; ask if the setting says | Name shown | BUILT | Two same names. |
| R16 | File is being used / locked on Windows at rename | Retry the rename a few times; keep the staged file; tell | "Could not save; the file is open" | PARTIAL | Hold the target open in another process. |
| R17 | Power cut or crash | Ledgers and look-back (P1, S9); row comes back; torn tail repaired | Row continues | BUILT | Kill at random times (E4). |
| R18 | Queue file damaged | Use the last good copy / journal; say so | A note on start | PARTIAL | Flip bytes. |
| R19 | Server's length wrong (longer / shorter than sent) | End at the stream's end; verify size if checksum known (B11) | Done, or a size warning | BUILT | Server mis-declares. |
| R20 | HTML where a file was expected | Ask (B40) | The question | BUILT | `.zip` returns HTML. |
| R21 | Redirect loop / too many hops / insecure hop / inward hop | Stop with the specific reason | Words | BUILT | N7 tests. |
| R22 | Proxy refuses / needs login / unreachable | Say which; don't fall back silently to a direct connection if the proxy was set | Words | BUILT | Logging proxy returns 407, 502. |
| R23 | A stream segment missing (404 on one segment) | Retry; then fail the row naming the segment index; keep good segments | "Piece 41 of 600 is missing" | BUILT/PARTIAL | HLS fixture with a hole. |
| R24 | Stream key unreachable / unsupported encryption | Refuse in words | "This stream is protected" | BUILT | H4. |
| R25 | Playlist changes while downloading (live, rotating tokens) | Re-read per kind; for VOD with token-in-address, use the renewed address | Usually nothing | PARTIAL | Fixture whose segment addresses expire after 60 s. |
| R26 | Muxer meets an unsupported codec | Save the tracks separately and say so | "Saved picture and sound apart: this codec cannot be joined" | PARTIAL | Fixture with an unsupported codec. |
| R27 | Decoder missing (AAC / H.264 on the system) | Do not offer the conversion; say why on request | Missing choice | BUILT (Linux AAC, GIF PARTIAL/TO BUILD) | File 07. |
| R28 | Site changed (reader cannot cut out the challenge function) | Fail "the site changed and dm cannot read it yet" with a "Copy details" block (B55) | Words and the block | PARTIAL | File 06 test. |
| R29 | Extension cannot reach the app | Browser keeps the download (B-2) | The browser's normal download | BUILT | Stop the app. |
| R30 | Two instances of dm | Second hands its command line to the first; a lock dm cannot read stops dm and says so | Words | BUILT | Start twice. |
| R31 | Computer sleeps during a download | On wake, connections that died are retried (R1); wake-lock prevents sleep when set | Continues | PARTIAL (gaps row 20: untested) | Sleep the machine for 2 minutes. |
| R32 | Network changes (Wi-Fi to wired, VPN on/off) | Same as R2; new connections use the new route | Continues | PARTIAL | Switch adapters. |
| R33 | Very slow or stalled server | Read timeout (N15); helper connections (S5); replace slow connection (S6) | Slow speed shown | BUILT/TO BUILD | Server trickling 1 KB/s. |
| R34 | Server gives different content for each connection (CDN edge with different versions) | Seam check (S7) and look-back catch it; fall back to one connection (S10) | Note | PARTIAL | Alternating-bytes server. |
| R35 | Hostile input (huge headers, malformed playlists, zip bombs) | Limits (N24, F7); no panic | Words | BUILT | Fuzz tests. |

## Cross-cutting rules

* **The message names the cause and the next step**, in one short sentence
  each (B54). It never shows a raw error type.
* **"Copy details"** on any failed row copies one block with the version, the
  system, the time, the state, the reason, and no secrets (B55).
* **No silent loops.** Every retry has a bound on count and rate (P5).
* **Partial data is kept** on every failure except R11 and R34, where it is
  known to be wrong.
* **Reproducibility.** Every row has a deterministic test with the local
  server; if a failure can only be seen on a real network, a row says so and
  names the human check.
