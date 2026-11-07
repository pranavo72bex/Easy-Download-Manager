# Security report, 2026-10-29

An authorized black-box and source test of dm on this computer, by a
testing agent working for the maintainer: the application only, servers of
its own on loopback, no real folder or credential touched, nothing outside
reached. Debug build 0.8.4 on branch its file. Denial of service out
of scope. What was done about each finding is noted under it.

**Counts:** critical 0, high 0, medium 1, low 3.

**Done the same day:** M1 (inward redirects refused: `redirects.rs`
`inward`, tested). **Done the next day, 2026-10-30:** L2 (dm follows
redirects itself, `redirects.rs`; across hosts `Referer` goes as its origin
alone and the ranges that follow carry the same; tested end to end), L1 (a
row's login and cookies kept in `secrets.json` beside the queue file, never
in it; tested; the keychain is gaps.md row 39), and L3 as defence in depth
(the host answers only the extension ids built in or allowed by a manifest
dm wrote, `shell::launcher_known`; tested). The browser's own check remains
the one that counts for L3.

---


Authorized black-box + source review. Target: this repository
(branch its file), debug build `desktop/target/debug/dm-desktop` (dm 0.8.4).
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
The external video tool unpack (`tools.rs:392`) also gates enclosed_name() and only unpacks the pinned SHA2-256-verified
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
  (the external video tool "preparing_progress" in tools.rs, i18n, docs; mtimes 22:39-22:42). None of those are mine. This
  pentest made zero edits to tracked files and committed nothing; all artifacts are under scratchpad/pentest/.
- Not tested dynamically (reasoned from source/tests): https->http downgrade (S4), and Cookie (vs
  Authorization) stripping specifically — both share the mechanism demonstrated in S5.
- All dm and server processes started for this test were terminated at the end (verified: no listener).

---

## Review of the changes since 0.8.4, 2026-10-30

A review of `desktop-v0.8.4..HEAD` (the YouTube and playlist readers, the
clip fetcher, the shipped the external video tool's adoption, the native host's new checks,
the extension's page-world quality channel, the release workflows), with
each candidate checked again for false positives. Critical, high, medium:
none. Three low and four informational findings, all hardened before the
release (CHANGELOG, "Security hardening from the pre-release review"):

- The solver runs the player's code; QuickJS's command line lets a script
  reach the OS. Attacker: only YouTube itself or a TLS interceptor the
  user trusts (the external video tool in 0.8.4 already did the same). Now: the player is
  fetched only from YouTube's own player path, and Deno, run with no
  permissions, is preferred.
- The download-folder write check followed a planted symlink. Attacker: a
  second local user with write access to this user's download folder.
  Now `create_new` with an unguessable name.
- Index ranges from the player API were unbounded before allocation; now
  capped. Quarantine removal no longer follows links; adoption staging is
  per process; the Mozilla source archive is written outside `dist/`.
- Left: the test-only environment knobs (`DM_YOUTUBE_HOST`,
  `DM_EJS_RELEASES`) are honoured in release builds; environment variables
  are trusted by this report's model.


---

## Windows, 2026-11-07

The stress suite's two security phases had never run on Windows: `control`
called `lsof` and read the token's Unix mode, `hostile` read `/etc/hosts`.
Both now ask Windows (`netstat -ano`, `icacls`, the system's hosts file) and
ran against the debug build on Windows 11 with no finding:

- **hostile:** a redirect to `file:///C:/WINDOWS/System32/drivers/etc/hosts`
  and one to `ftp://127.0.0.1` are refused ("an address dm does not
  follow"); a 1 MB header is refused; a 120 KB gzip body that unpacks to 67 MB
  is stopped as a decompression bomb; a file larger than the free space is
  held, not started; a login written into an address is masked in the list.
- **control:** the port listens on `127.0.0.1` only; no token and a wrong
  token are refused and the connection cut; 1 MB with no line end is cut
  with no growth in memory; 1500 idle unauthenticated connections are
  closed by dm; `file:` and `javascript:` addresses, a `folder` outside the
  download folder and a relative one are refused.

### L4: a dm home outside the account's profile took its drive's access list (Low; fixed)

**What.** On Unix dm makes its home 0700 (`instance.rs` `private`); on Windows
that was a no-op, and the files in it take the folder's inherited access
list. The default home, `%LOCALAPPDATA%\dm`, inherits the profile's: this
account, SYSTEM and the administrators. A home anywhere else (`DM_HOME`, a
portable copy on a second drive) inherits the drive's, which on this
computer's D: gives `Authenticated Users: Modify` and `Users: Read`
(measured with `icacls`). There, another signed-in account could read
`control.token` (the control port is open by default for MCP, and loopback is
shared by every account on the computer), the queue and unfinished parts.

**Fix.** `instance/owner_only.rs`: when dm starts, a home whose access list is
not protected is given one of its own (nothing from its parent): full
control for this account, SYSTEM and the administrators, inherited by what
is in it and applied to what is there already. A home already protected is
left alone; a drive with no access lists (FAT) is not a reason to stop dm.
Test: `instance::tests::dms_home_is_this_accounts_alone_on_windows_too` (a
parent that lets every user in; the folder, a file already in it and one
made after are left with no entry for users, signed-in users or everyone).
It fails with the fix taken out.

**Not covered.** A home folder another account made beforehand stays that
account's to change (its owner can always rewrite the list).

## Review of `desktop-v0.8.7..HEAD`, 2026-11-07 (source, on macOS)

A source review of the changes since 0.8.7 (gaps row 157), in three parts
read independently: the secret store and how a row's sign-in travels; the
desktop's local surface (control port and MCP, watched folder, native host,
renewed addresses, site rules, torrents); and the browser extension. Every
finding was read end to end in the code before it was kept. Counts: medium
4, low 7, informational 8. Fixed the same day, with a test beside each (the
tests of M5, M6's script guard and L5 were run with their fix taken out, and
fail):

- **M5, the browser's request headers read back through `queue.list`
  (found twice).** `Entry.headers` (a site's `Authorization: Bearer …`, its
  `X-…-Token`) went out to any control-port or MCP client; only `cookies` was
  removed. Now removed too (`app/control.rs`;
  `a_rows_handed_over_sign_in_is_never_read_back_out` adds a bearer header).
- **M6, an imported settings file could make dm run a program.** A queue's
  own hours (`queue_schedules`, new since 0.8.7) carry `run_when_done`;
  `Settings::imported` kept this computer's program for the shared schedule
  only, and the window's guard against a script ignored the queues' too.
  Both now keep each queue's program as it was, and a queue the file adds
  runs none (`a_settings_file_never_sets_the_program_run_at_the_end_of_a_queues_own_hours`,
  `a_script_driving_the_window_cannot_set_the_program_a_queues_hours_run`).
- **M7, a page could hand dm any number of torrents after one click.**
  `content.js` took a script-opened magnet (`dm-magnet`) whenever the page's
  user activation was live, which lasts seconds and is not used up. Now
  only within 1.5 s of a trusted pointer or key press seen by the content
  script itself, and one per press (`chrome-check.py`: "five magnets a page
  opens for one click hand dm one"; the seven ways still reach dm, pressed
  with the mouse).
- **M8, the extension re-read any playlist a page loaded, private networks
  included.** A page could `fetch('http://192.168.1.1/…m3u8', {mode:
  'no-cors'})` and have the worker ask it again with the browser's session,
  no CORS and no private-network check, then the first rendition it named on
  any host. Now a playlist (or a rendition, or a file read for its track) on
  this computer or a private network is read again only when the page that
  loaded it is on that same host (`decide::may_reread`, with a test of eleven
  inside addresses; `chrome-check.py` 88 of 88).
- **L5, the browser's headers followed a redirect to another host.** Across
  hosts dm dropped `Authorization` and cookies but kept a site's own headers
  (`X-Api-Key`…). Now a hop to another host carries only what every request
  of dm's does (`redirects.rs` `CARRIED_ACROSS`; the cross-host redirect test
  checks an `X-Api-Key`).
- **L6, on macOS no Sign-in's password reached the Keychain.** A Sign-in's
  secret name has a line break between the site and the user, which
  `security -i` would split, so the store refused every one and the password
  stayed in `settings.json` with a notice at each start. A name with
  whitespace or quotes is now kept under its base64url spelling; plain names
  (the proxy's) are unchanged (`secretstore::tools`, against a real scratch
  keychain).
- **L7, a secrets file that would not open was deleted by the next save.**
  A locked Keychain or a Secret Service not yet running at login cost every
  row's sign-in for good. It is now moved aside as `secrets.json.unopened`
  and the notice says so (`a_file_of_secrets_that_cannot_be_read_is_kept_aside_not_deleted_by_the_next_save`).

Left, each a row in `docs/gaps.md` (row 159):

- **Low: a waiting row takes a fresh address from another customer's host**
  on shared hosting: `queue/family.rs` keeps a host's last two labels, so
  `victim.s3.amazonaws.com` and `evil.s3.amazonaws.com` are one family for a
  row that never learned its name or size. Cookies do not cross (`set_url`).
  Fixed the same day: a host under one of 30 shared-hosting names (from the
  public suffix list's private part) is its own family
  (`two_customers_of_a_shared_host_are_not_one_family`).
- **Low (Windows): a torrent's file named `D:x`** passes librqbit's check
  (no `..`, no separator) and `PathBuf::push` makes it a path on another
  drive; the vendored storage creates the empty files before dm's own check
  refuses the torrent. No content is written. Fixed on Windows the same day:
  the vendored `init` checks every file's name before it makes any (plain
  parts only: no root, no `..`, no drive, no `:`), tests
  `dm_tests::a_torrents_file_name_must_stay_in_its_folder` (the vendored crate) and
  `tests/torrent_names.rs` (dm-core: the file was made outside the folder without it;
  BUGLOG BUG-013).
- **Low: the plain-JavaScript panel** (sites whose policy forbids
  WebAssembly) checks only `isTrusted`, not that it is drawn as dm drew it
  (`as_drawn`), in an open shadow root a page can restyle; and a panel
  choice now sends the cookies of whatever host the page's `<video src>`
  names. The app's Download details dialog is what remains.
- **Low: a renewal is matched across tabs.** `RequestHeaders::renewed` did
  not compare the tab or YouTube's `id`, so another tab's video with the same
  itag on the same edge host could be offered as a renewal. The host cannot
  change; the file could be the wrong video. Since the same day it compares
  `id` and `mime` with the `itag` (`another_videos_address_with_the_same_itag_is_not_a_renewal`);
  the tab is still not compared.
- **Low: a settings file that fell back to defaults** has an empty proxy
  password, and `move_passwords_to` then forgot the one in the store. Fixed
  the same day: a run whose file was not read whole, or whose store did not
  answer at start, forgets no stored password
  (`a_password_this_run_did_not_read_back_is_not_forgotten_by_the_next_save`,
  which fails without it).
- **Info:** the update check compares the rules and installer addresses by
  their raw prefix (`%2e%2e` normalises elsewhere; only a forged
  `latest.json` could use it); "Get it from the browser" takes the next
  download from any site within ten minutes; the early answer buffer in
  `document.js` (24 answers) is never released on a site with no rules; the
  kept headers are not split by private window; `Start.referrer` is the full
  page address where a browser sends its origin to another host; the panel's
  `dm-player-*` listeners do not check `event.source`; the Properties and
  Download complete dialogs' Open buttons have no script guard; with no
  store at all the notice blames "Windows Credential Manager" on macOS and
  Linux too.
- The worker's re-read of a playlist on the public web still carries the
  browser's session, as the player's own request did; only private hosts are
  refused.

## Review of `main..parity-with-reference-a`, 2026-11-08

A second source review, of everything the branch adds over `main`, each candidate
checked again by a separate reader for false positives. Two held, both new on
this branch, both fixed the same day:

- **H1, a proxy configuration script could run any program (High).** `pac.rs`
  ran the script fetched from Options' "Automatic configuration script" address
  (often plain `http://`) with QuickJS's command line, whose `qjs:os` module is
  reachable by `import()` from any script: `os.exec` ran as the user (shown with
  a QuickJS-ng binary). An attacker on the network path of an `http://` script,
  its server's operator, or a shared settings file (`Settings::imported` kept
  `proxy.script` from the file) could use it. Now: an installed Deno with no
  permissions is preferred; in QuickJS a script naming `import` or hiding words
  behind `\u` escapes is refused before it runs, and the prelude takes `eval`,
  `Function` and the generator and async function constructors away, for good,
  before the script (`pac.rs` `refused`, `PRELUDE`; tests, one against a real
  engine where one is installed). An imported settings file keeps this
  computer's own script address, and where a proxy host is set beside a script
  its login goes only to that host.
- **M9, a redirect got round M8 (Medium).** The extension's worker refused a
  playlist on a private host but followed a public one's redirect, so a page
  could have it send a GET with the browser's cookies to a router. The worker's
  fetches now treat a redirect as an error (`plumbing.rs` `fetch_bytes`,
  `redirect: "error"`); a playlist behind a redirect is left undescribed.
  `chrome-check.py` 88 of 88.
