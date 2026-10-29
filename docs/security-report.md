# Security report, 2026-10-29

An authorized black-box and source test of dm on this computer, by a
testing agent working for the maintainer: the application only, servers of
its own on loopback, no real folder or credential touched, nothing outside
reached. Debug build 0.8.4 on branch `ytdlp-gaps`. Denial of service out
of scope. What was done about each finding is noted under it.

**Counts:** critical 0, high 0, medium 1, low 3.

**Done the same day:** M1 (inward redirects refused: `lib.rs` `inward`,
tested). **Recorded for the maintainer:** L1 and L2 as gaps.md rows 39 and
40. **Accepted:** L3, the standard native-messaging model, in which the
browser enforces which extension may start the host.

---


Authorized black-box + source review. Target: `/Users/pranav.jha/Easy-Download-Manager`
(branch `ytdlp-gaps`), debug build `desktop/target/debug/dm-desktop` (dm 0.8.4).
All testing on 127.0.0.1 against local servers; download_dir pointed at a temp folder
(`scratchpad/pentest/downloads`); DM_HOME = `scratchpad/pentest/dmhome`.
DoS/resource-exhaustion out of scope and not reported.

## Summary of severities
- Critical: 0
- High: 0
- Medium: 1  (SSRF: redirects to loopback/link-local/private addresses are followed)
- Low: 3    (credentials-in-URL stored cleartext in queue.json; Referer forwarded cross-host on redirect; native host does not itself validate the extension origin)
- Informational / defense notes: see end

The product is notably hardened against classic download-manager bugs: filename traversal,
overwrite, header injection, zip-slip, cross-host credential leakage, and local-API auth were
all confirmed safe by live testing.

---

## Findings

### M1 — SSRF: redirects to loopback / link-local / private addresses are followed (Medium, high confidence)
**What.** `redirect_policy` (`desktop/crates/dm-core/src/lib.rs:525`) restricts only https->http
downgrades and the redirect count (<=10). It does not restrict the destination IP. A download
from an attacker URL that returns 3xx Location to an internal address makes dm connect there (GET).

**Reproduction.** evil server on 127.0.0.1:18080:
- `/redir-control` -> `302 Location: http://127.0.0.1:53331/` (dm's control port)
- `/redir-linklocal` -> `302 Location: http://169.254.169.254/latest/meta-data/`
Added via `downloads.add`; queue.list states:
```
redir-control  => failed "network: ... (SendRequest): invalid HTTP version parsed"
redir-linklocal => running (dm was attempting the 169.254 connection)
```
redir-control proves dm opened a TCP connection to the loopback control port and spoke HTTP
(it only failed because the control port is JSON-lines, not HTTP). redir-linklocal proves dm
attempts link-local connections. No allow/deny-list for RFC1918/loopback/link-local targets.

**Impact & honest constraints.** Blind, GET-only SSRF:
- No credentials forwarded to the redirected host (reqwest strips Authorization/Cookie cross-origin — see S5).
- Response body is written to the user's download folder, not returned to the attacker (hand-off
  is one-way; control API needs the local token) — no exfil channel back to the attacker.
- Requires the victim to start/catch a download from an attacker URL.
Realistic risk: GET side effects on unauthenticated internal services (routers, admin endpoints,
cloud metadata IMDSv1). Medium, not High; close to Low on a laptop with no internal services.

**Fix.** In `redirect_policy()` reject `attempt.url()` whose host is loopback/link-local/ULA/RFC1918
when a previous hop was public (block public->private redirects). Optionally a user setting.

### L1 — Credentials embedded in a URL persisted cleartext in queue.json (Low, high confidence)
**What.** `http://<login>@host/...` is stored verbatim (password included) in `<DM_HOME>/queue.json`;
the queue keeps the raw `url` string (`queue/mod.rs:84`).
**Reproduction.** `downloads.add {"urls":["http://<login>@127.0.0.1:18080/normal"]}` ->
`grep` for the password in queue.json hits `"url":"http://<login>@127.0.0.1:1808..."`. The control API
masks it on read-back: queue.list returns the address with its password masked (authors already
treat it as secret for display). Only on-disk persistence is cleartext.
**Mitigation present.** queue.json is 0600 (verified). Residual risk: plaintext creds on disk/backups/sync.
**Fix.** Strip userinfo before persisting (keep credential separately), reusing the existing masking
logic, in `queue/persist.rs` / the `Entry` serialization.

### L2 — Referer forwarded to a different host across a redirect (Low, high confidence)
**What.** On a cross-host redirect dm forwards `Referer` (previous URL) to the new host.
**Reproduction.** `/redir-xhost` -> `302 Location: http://localhost:18080/afterredir`. Second-hop
server log: `GET /afterredir host: localhost:18080 referer: http://127.0.0.1:18080/redir-xhost`.
Authorization was correctly dropped; the referrer (no creds in it) leaked to the other host.
**Impact.** Minor info disclosure of the originating URL/path; browsers do similar. Low/Informational.
**Fix.** Drop/trim `Referer` when a redirect changes host (dm sets it in `download/http.rs:129`,
`probe.rs:64`); e.g. attach it only on the first request or re-evaluate per hop vs current host.

### L3 — Native messaging host does not itself validate the extension origin (Low/Informational, high confidence)
**What.** `dm-desktop <chrome-extension://...>` enters host mode for any extension origin; the binary
does not check the id. `shell::browser_launch` (`dm-shell/src/shell.rs:39`) only uses the arg to detect
host mode; `host::answer` never inspects origin.
**Reproduction.** Framed JSON on stdin to `dm-desktop "chrome-extension://unlistedarbitraryid.../"`:
engine.hello -> reply; download.start file:///etc/passwd -> failed "dm downloads http(s)... only";
download.start http://.../hostnormal -> started (fetched into download_dir). An arbitrary id is served.
**Why Low.** Which extension may launch the host is enforced by the browser via the manifest
allowed_origins/allowed_extensions (`dm-shell/src/register.rs:723-729`, pinned Chrome/Edge/Firefox ids).
Anything that can run dm-desktop directly is already a local process with the user's privileges. Standard
native-messaging trust model. Optional defense-in-depth: reject origins not in the registered set.

---

## Tested and found SAFE (with evidence)

### S1 — Filename path traversal / unsafe names: contained
`probe::sanitize` (`probe.rs:348`) reduces a suggestion to its last path component (splits `/` and `\`),
strips RTL/bidi controls, maps `<>:"|?*`/control(<0x20)/0x7f to `_`, trims leading dots and trailing
dots/spaces, prefixes Windows reserved names, truncates to <=240B/200ch. `claim_path` uses create_new so
it never replaces (`download/files.rs:181`). save_name re-sanitized (`assemble.rs:208`, `queue/mod.rs:653`).
Live (all inside download_dir; nothing in /tmp or parent):
- `../../../../../../tmp/dm_evil_traversal.txt` -> `dm_evil_traversal.txt`
- `/tmp/dm_evil_absolute.txt` -> `dm_evil_absolute.txt`
- `..\..\..\dm_evil_backslash.txt` -> `dm_evil_backslash.txt`
- `filename*=UTF-8''%2e%2e%2f...dm_evil_star.txt` -> `dm_evil_star.txt`
- `CON` -> `_CON`;  `....` -> `dots` (URL fallback)
- `invoice<U+202E>gpj.exe` -> `invoicegpj.exe`
- 6000xA + .exe -> truncated ~200ch + .exe
- `PRECIOUS.txt` (already present) -> `PRECIOUS (1).txt`; original bytes untouched
Evidence: `evidence/filename_tests_downloads.txt` + queue.list paths under .../pentest/downloads/.

### S2 — NUL and CRLF header injection: rejected at HTTP layer
`filename="ev\x00il.txt"` and `filename="a\r\nX-Injected: yes\r\n.txt"` -> reqwest "invalid HTTP header
parsed" (download failed). Defense in depth: sanitize also maps NUL (<0x20) to `_`.

### S3 — file:// and scheme redirects: blocked
`302 Location: file:///etc/passwd` -> failed "the server sent dm on to file:///etc/passwd, an address dm
does not follow"; /etc/passwd not read. `accepted()` (`queue/mod.rs:405`) whitelists http(s)/ftp/torrent/
live; the browser Start handler also rejects `file:` (`host.rs:739`).

### S4 — https->http downgrade on redirect: blocked (code-review + unit tests)
`redirect_policy` errors on a hop to http when a previous hop was https (`lib.rs:527-534`, INSECURE_REDIRECT).
Unit test `lib.rs:815+`. Not re-tested dynamically (needs trusted TLS cert); medium-high confidence.

### S5 — Cross-host credential leakage on redirect: safe (reqwest strips sensitive headers)
`http://<login>@127.0.0.1:18080/redir-xhost` -> redirect to localhost:18080. First hop carried
a Basic `authorization` header; the hop to the different host (localhost) carried NO auth
header. Cookie is in the same reqwest sensitive class and stripped identically. Sign-in is host:port
scoped (`cookies.rs SignIn::for_url`); cookies.txt matched per request-host (`cookies.rs:76`), recomputed
against the landed URL.

### S6 — Local control API: auth enforced, loopback-only, robust
- Bound 127.0.0.1 only (`control.rs:153`); lsof confirmed `127.0.0.1:53331 (LISTEN)`.
- Token required for every method: unauth queue.list -> "authenticate first" + close; wrong token -> same.
  64-hex bearer, constant-time compare (`control.rs:373`).
- control.token 0600, queue.json 0600 (verified ls -l).
- Malformed JSON after auth -> "that is not JSON", connection stays usable. Oversized post-auth (>1 MiB)
  -> "a request may be at most 1 MiB". Pre-auth >4 KiB -> "authenticate first".
- Not reachable from a web page: a browser-style `POST / HTTP/1.1` (with Origin header) -> "that is not
  JSON" and close before any auth; response not CORS-readable; 64-hex token unknowable to a page; DNS
  rebinding buys nothing (not HTTP, still needs the token).

### S7 — MCP server: stdio only, not web-reachable
`dm-desktop mcp` speaks JSON-RPC on stdin/stdout (`mcp.rs:7,491`) as a client of the control port using
the local token. The only TcpListener in mcp.rs is inside `#[cfg(test)]`.

### S8 — Browser hand-off cannot choose save location or name
`download.start` (`dm-protocol/src/lib.rs:181` Start) has no save_dir/save_name field; host builds Request
with `..Request::default()` for those (`host.rs:769-782`). Live: a Start with injected save_dir/save_name
to /tmp was ignored — file landed in download_dir as normal_download.bin, nothing in /tmp. Control API's
`folder` param is constrained to inside download_dir with no `..` (`app/control.rs:320-333`).

### S9 — Archive extraction (zip-slip): safe, user-initiated only
`extract.rs` is reached only via Extract here/to (`app/commands.rs:306`), never automatically. Each entry
validated before any write: `safe_path` rejects empty/NUL/absolute/`..`/drive/backslash names and double
-checks the zip crate's enclosed_name() (`extract.rs:116,199-205`); symlinks/device/pipe/socket and encrypted
entries refused; case-folded duplicates refused; zip-bomb ratio/size capped; create_new into a new folder.
yt-dlp unpack (`tools.rs:392`) also gates enclosed_name() and only unpacks the pinned SHA2-256-verified
release — arbitrary downloads are never auto-extracted. Torrent/metalink names sanitized
(`torrent.rs:509`, `metalink.rs:111`); per-file torrent paths delegated to the torrent library.

### S10 — Servers lying about length/ranges: no corrupt "complete" file
- Declared 1,000,000, sent 10 then closed: never finalized, no file (then cancelled).
- Declared 5, sent 50: saved exactly 5 bytes, extra ignored. No overflow.

### S11 — Dependency audit: clean
cargo audit (1386 advisories): desktop/ (807 deps) exit 0, no vulns/warnings; browser/ (72 deps) exit 0,
clean. Evidence: `evidence/cargo_audit_desktop.txt`, `evidence/cargo_audit_browser.txt`.

---

## Notes / scope caveats
- A concurrent autonomous dev agent was editing tracked files on this branch during the assessment
  (yt-dlp "preparing_progress" in tools.rs, i18n, docs; mtimes 22:39-22:42). None of those are mine. This
  pentest made zero edits to tracked files and committed nothing; all artifacts are under scratchpad/pentest/.
- Not tested dynamically (reasoned from source/tests): https->http downgrade (S4), and Cookie (vs
  Authorization) stripping specifically — both share the mechanism demonstrated in S5.
- All dm and server processes started for this test were terminated at the end (verified: no listener).
