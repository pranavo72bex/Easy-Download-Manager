# Gaps

The work list: what is known to be missing from a production-ready download
manager and is both wanted and possible for free. A row says what is missing
and how it closes; when it closes, its row is deleted (what was done is in
`CHANGELOG.md`, `docs/parity.md` and git history).

| Status       | Meaning                                                      |
|--------------|--------------------------------------------------------------|
| OPEN         | Nothing done, or done but never run                          |
| PARTIAL      | Works and is tested, but not everywhere it must              |
| BLOCKED      | Needs something this project cannot get for free, or a person |

## Is dm production ready?

As far as code and automated checks can say, yes, as of 2026-10-24, apart from
the open rows below. What they cannot stand in for is a person at a real computer: Windows
has run only on CI's machines ([windows-checklist.md](windows-checklist.md)),
nothing is signed with a paid certificate, and no release has been cut. That
is not a gap in the program; it is what has not yet been watched.

## Open work

A full check on 2026-10-28 found no failing test: `cargo test --workspace`
(desktop and browser), `cargo clippy --all-targets -D warnings` and `cargo
fmt --check` are clean; `scripts/smoke.py` on a debug build passes; and
`scripts/real-sites.py` (YouTube through dm's own engine, OVH, W3Schools,
Wikimedia) passes 4 of 4. The same day the video route's gaps found against an external tool's reading of a YouTube page were closed (request slices, renewed addresses, per-stream headers, chunked requests; `docs/decisions.md`, "The video route follows what the external video tool's reading says").

### The Windows session (next, with the maintainer)

As of 2026-11-06 every open row below is waiting on a person, a Windows
computer or the maintainer's word; none is code that can be written alone on
macOS or Linux (macOS QA that day: 63 of 64 contract rows hold, B25 as row 154
says; `cargo test --workspace` 1478 passed). The maintainer chose to do the rest
on Windows. This is the order for that session; each step names the rows it
closes, and a row is closed (deleted here, written in `CHANGELOG.md`) only when
its step passes there.

**Done on Windows 2026-11-07, besides the steps below.** The video website compatibility test plan (`scripts/video-sites-check.py`, signed out, no cookies given to dm): engine cases 19 of 20 (ENG-07b needs a removable drive pulled by hand), the public files and YouTube by Add URL pass the plan's gate (P0 6 of 6, core 5 of 5), and through the panel YouTube, Vimeo, TikTok, a Twitch highlight and clip and PeerTube are whole with sound; Rumble and Reddit answer an automated browser with a bot check, Dailymotion is not judged, Bilibili is row 155. Wikimedia's originals stalled on 429 (BUGLOG BUG-010, fixed). Beside reference A on the same server and public files: the same bytes and names wherever both finished (`docs/reference-behavior.md`; rows 65 and 152). The stress suite on Windows (row 20); a home outside the profile made the account's alone (`docs/security-report.md` L4); every source file given its copyright and licence header (`cargo xtask headers`). Found: rows 155 (Bilibili) and 157 (a source review), both closed the same day by the macOS and Linux session; 156 (each toast kept three handles: BUGLOG BUG-012) and 158 (a large file's speed: BUG-011), fixed the same day on Windows. On the day's final tree: `cargo test --workspace` 1476 passed in `desktop/` and 140 in `browser/`, clippy clean in both, `human-qa.py` 64 of 64 rows (106 checks), the release build and the extension's packages made.

**Before it starts.** On the Windows PC: pull branch `parity-with-reference-a`;
Rust, Python 3, Git Bash and Microsoft Edge installed; reference A installed
with its Edge extension (for the side-by-side steps); a spare account for each
site that needs a sign-in (Facebook, Instagram, YouTube, Reddit), not the
maintainer's own. The commands are for PowerShell from the repository's root;
`human-qa.py` and `stress-check.py` look for `dm-desktop` with no `.exe`, so
`DM_BINARY` is set on Windows.

1. **Build.** `cd desktop; cargo build -p dm-desktop; cargo build --release -p dm-desktop; cd ..`,
   then `sh scripts/package.sh` in Git Bash.
2. **The test suite, row 115 (done 2026-11-07: three clean runs each, after four fixes; `CHANGELOG.md`).** `cd desktop; cargo test --workspace -- --test-threads=4`,
   then `cargo test --workspace` in full, three times each. For a failing
   window test, read its message and lengthen the wait it names when that wait
   is bound by a loaded machine rather than by dm.
3. **The contract, rows 141 and B25 (done 2026-11-07: 64 of 64 rows hold; B36 67 of 67 and 69 of 69).**
   `$env:DM_QA_WORK="D:\dm-qa"; $env:DM_BINARY="<target dir>\debug\dm-desktop.exe"; python scripts/human-qa.py`.
   Report every row it prints. B25 here compares against
   `docs/behaviour-baseline/windows/`, which was taken with the work folder at
   `D:\dm-qa`: its path shows in Download details, and the default
   `C:\tmp\dm-human-qa` is longer and cut off there (1.04% of pixels differed). If B36 fails, the run prints the
   extension's own words for the failed panel check (row 141): fix what they
   show.
4. **Held downloads, row 24 (done 2026-11-07: 41 checks, no finding).** With `DM_BINARY` as above:
   `python scripts/stress-check.py holds hidden details`.
5. **Memory, row 137 (done 2026-11-07: 64 to 76 MB working set, 30.5 MB private; budget 100 and 40 MB, `docs/decisions.md`).** `$env:DM_BINARY="$PWD\desktop\target\release\dm-desktop.exe"; python scripts/memory-check.py`;
   record the numbers in row 137 and set Windows' budget from them.
6. **The secret store, row 39 (Windows part; the restart half done 2026-11-07, the other account left for later).** In the release build: a row with
   a login and a browser download with cookies; quit dm and start it again (both
   still work); copy dm's home to another Windows account and start dm there (it
   cannot open them, and says so once).
7. **Torrents from the browser, row 103 (done 2026-11-07: on webtorrent.io a `.torrent` link and a magnet link each went to dm and reached the swarm, the magnet's 129 MB whole).** Release build, extension added to Edge:
   a torrent site's Download button and a `magnet:` link each start a download in
   dm.
8. **Signed-in sites, rows 62, 63, 71, 75, 110, 114, 122 (Facebook done 2026-11-07, rows 62 and 114 closed; in the kept test profiles `D:\dm-facebook-profile` and `D:\dm-reddit-profile`, `DM_CHROME_PROFILE`).** One site at a time;
   the check opens a visible Edge at the login page and waits six minutes while
   the maintainer signs in by hand (the check never types, reads or keeps a
   password, and its profile is thrown away after the run):
   `$env:SITES_SIGNIN="facebook"; python scripts/sites-check.py facebook facebook-watch`
   (rows 62, 114, 75), the same with `instagram` (rows 71, 75), `youtube` (row
   122: the four kinds through the panel) and `reddit reddit-videos` (row 63).
   Row 110 closes on the whole run's results, after the maintainer says which of
   what is left is wanted.
9. **Beside reference A, rows 65, 72 (72f), 142, 152.** Both extensions in one
   Edge (`$env:SITES_HOLD="20"; $env:DM_KEEP_OTHER_EXTENSIONS="1"`) on Reddit and
   TikTok for 72f; the four differing scenarios of row 65 with reference A's
   dialogs open; every site of `docs/site-list.md`, filling in its two result
   columns (row 152), with a new row here for each place dm does less.
10. **Real streams, rows 45, 57, 121, 68, 69.** The panel's Part line (B59) on a
    real HLS, DASH and plain MP4 page (row 45); a video site's signed address
    that runs out mid-download, tab left open, goes on in the same row (rows 57
    and 121); the first real site rules written from step 9's comparison and
    checked with the app serving them (rows 68 and 69).
11. **By hand, row 60.** The maintainer drags each kind of dialog out of dm's
    window, and onto a second monitor if there is one.
12. **The maintainer's word, rows 93 and 110.** G74 (a single file: ask first)
    and G76 (where the bar sits) each change what a person sees or what a click
    does, so nothing is built before a yes or no on each. (G84, dial-up and VPN,
    was built 2026-11-08 at the word of 2026-11-07.)

**Not for the Windows session.** Nothing on macOS: rows 105, 154 and row 160's
macOS half were closed on the maintainer's Mac, 2026-11-07 and 08; row 160's
Windows half on Windows, 2026-11-08 (`human-qa.py` whole on this build, 64 of 64 rows).
Row 39's Linux and macOS parts are proved (2026-11-08 and 07); its Windows part is the
other account. Row 20's sleep is covered on macOS and Linux (2026-11-08, `stress-check.py sleep`). Row 30 passed on real YouTube (2026-11-08) and waits on the reporter's word; row 47 needs an iPhone; row 5 native
readers; row 51 the next tagged extension release; row 151 is a periodic search.

### The macOS and Linux session (2026-11-07, beside the Windows one)

Run on the maintainer's Mac at the same time as the Windows session, so the
two do not take the same row. Taken here, and closed: row 105 (dm.app
installed on this Mac with the maintainer's OK of 2026-11-07: a magnet and a
`.torrent` reached nothing until `opened.rs` took the system's Apple events);
row 155 (a) and (b), approved by the maintainer on 2026-11-07 ("Approve (a)
and (b)"; the Bilibili page gave a whole video with sound in Edge on macOS);
row 157 (the source review; what it left is row 159, whose (b) is for
Windows); row 158 measured on macOS (7.7 s for 4.5 GiB from the same server,
so the slow time is Windows' own); row 159 (a) and (d); row 39's macOS part;
Linux's `window` and `large` stress phases (row 20). `human-qa.py` on macOS
after these changes: 63 of 64 rows hold, B25's two dialog pictures not taken
(row 154). Left to Windows: rows 156, 158's Windows profile, 159 (b), and
every step above. Row 159 was closed on macOS on 2026-11-08 ((c) and (d); `CHANGELOG.md`).
Row 154 was closed on 2026-11-08 once Screen Recording was allowed: the two
dialog pictures taken, checked by eye and copied in, and `human-qa.py` on macOS
64 of 64 rows (B25 0.02 to 0.05% of pixels apart). Each push here
is rebased on the branch first.

### State audit, 2026-10-28

Reported: a paused download's details window showing Stop instead of Resume,
dialogs that do not match the row, and a Resume whose progress indicator is
hard to see. Looked for by running the real window against a local server and
comparing, every 40 ms, the queue's own state with what the window had read
(`ui.inspect`) through random pause, resume, cancel, stop all, start queue and
retry on 8 downloads (4 seeds of 45 to 60 s each), and by photographing the
details window in each state (`ui.screenshot`). The reported Stop-for-Resume
was not seen in any frame once the window had read the list; what was found:

Found and closed (tests beside each; `CHANGELOG.md`, `docs/decisions.md`
"The window reads the queue when it changes"):

- The window did not read the list again after a change made while nothing
  looked to be moving: its next repaint was then 30 seconds away. A resume
  over the control port, the command line or an AI client from a state where
  everything was stopped left the window (and an open details window, with its
  Resume button and a still bar) showing "Stopped" for as long as the download
  ran: 4.3 s until it finished, in the run that showed it. Now read within a
  quarter of a second.
- `downloads.pause` / `downloads.resume` / `downloads.cancel` were answered
  from the window's copy of the row: a pause followed at once by a resume was
  refused as "it is not paused or failed" and the download stayed stopped (3 of
  3 tries). Now from the queue's row.
- A row that was paused, queued, finished or cancelled kept the speed and time
  left of its last run in `queue.list`, `list --json` and MCP (a finished file
  "moving" at 1.1 MB/s, on every seed). The window itself never drew them.
- The details window of a finished or cancelled download said "Can resume:
  Unknown until the server answers".
- The progress window of a download that finished stayed open (Show complete
  window is off by default), reading "Complete" with no button but the ×, one
  stacked behind the next for each. It closes when its download completes.
- A stopped download's bar in the list was drawn in the grey of its track, the
  faintest thing in the row beside Resume. It is the progress colour, dimmed
  (solid, and whole in High Contrast), checked by screenshot in the dark and
  light looks.

Not reproduced, so not rows: a snapshot older than about a quarter of a second
at any other time (none longer than 0.7 s in any run); more running than the
setting allows; a download that went backwards, passed its size, or completed
with bytes missing; a Resume that was answered and not applied.

Found by a security review (2026-10-24) and a stress test (2026-10-27,
`scripts/stress-check.py` and `scripts/stress-protocols.py`: local servers, a
dm of its own and Microsoft Edge; torrents on loopback inside a sandbox, and
one public Creative Commons torrent); each row says how it closes.

### Video without a helper, 2026-10-29 to 2026-10-31

YouTube is read by dm itself (`youtube.rs`), and every other site's video
comes from the browser extension's caught player requests — exactly as
reference A works. The external video tool is removed entirely (`docs/decisions.md`,
"the external video tool removed; videos as reference A gets them"). The former rows 6,
23, 29, 32 and 34 are closed. What is left of the video route: 37 and 45.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
| 5  | The 71 reworded strings (and the 14 added on 2026-10-30 for the quality list, the part fields and the Playlist dialog) were translated into the other twelve languages by a machine pass (consistent with each catalog's own vocabulary, checked for placeholders and length) and have not been read by a native speaker | BLOCKED | One native reader per language goes through the 71 strings listed by `git log -p -- desktop/crates/dm-desktop/i18n` for the 0.7.0 commit and corrects them |
| 20 | Not covered by the stress tests: Windows' parts below. The computer sleeping is covered since 2026-11-08 on macOS and Linux without sleeping the computer running the check (`stress-check.py sleep`, run by `linux-stress.sh`): dm's process is frozen (SIGSTOP, as the system stops it) for 20 s in the middle of 8 downloads while its network is gone, either every connection dropped or each left silent as a peer gone away without a word leaves it, then woken (SIGCONT): 8 of 8 byte-exact both ways, nothing failed, 7 s and 16 s after waking, on this Mac and in the Linux container. A frozen process's clock goes on where a sleeping computer's stops, so after a real sleep dm may wait its whole silence timeout (30 s) before reconnecting a silent connection; the check allows for it. Windows keeps itself awake while a row runs (`awake.rs`) and has no SIGSTOP, so the phase is skipped there. The clipboard offer is covered since 2026-11-08 on Linux, where the container's Xvfb clipboard is no one's (`stress-check.py clipboard`, run by `linux-stress.sh`; it refuses to run anywhere else): a link another program copied (xclip) is offered, downloaded byte-exact through Download details; a link already listed, a sentence with a link, and dismissed links are not offered; three links are offered as three; a burst of 300 copies leaves dm answering in 3 ms at worst and offering the last; 5 MB of copied text is no offer and no stall; Take a copied link at once opens Download details with no bar; 11 of 11, no finding. Covered on 2026-10-31: a full disk (`scripts/stress-protocols.py fulldisk`, macOS, a 30 MB disk image as the download folder: a 50 MB file is paused with "not enough room: the file needs 50.0 MB, and … has 29.3 MB free", two of 20 MB at once land one and hold the other with the room the first was promised, which resumes whole once there is room, no part-written file left, a 5 MB file then lands byte-exact), HTTPS through an HTTP proxy (`proxytls`: a real site's file through CONNECT, byte-exact as curl's direct copy, with and without the proxy's login, and refused with a wrong one), and a real playable DASH stream (`scripts/real-sites.py`, Shaka's Angel One: picture and sound in separate sets, 60 s, whole). FTPS, RTSPS and HTTPS redirects were already covered by tests with certificates made by `rcgen` (`ftp.rs`, `rtsp.rs`, `lib.rs`), and RTMPS is since 2026-10-31 (`rtmp/connect.rs`: carried over TLS to a server with a test root, and refused where the public roots do not vouch for the server). Linux (Debian 12, a container with no network, `scripts/linux-stress.sh`): the workspace tests and 24 of the stress phases pass with no finding; since 2026-11-07 also `window` (5060 rows and 60 active downloads under Xvfb: a call answered in median 3 ms, worst 19 ms; 153 MB at most) and `large` (1.5 GB on 16 connections, byte-exact, 235 MB/s, 32 MB at most), no finding (`DM_ONLY="window large" sh scripts/linux-stress.sh`); the Edge phase was run on 2026-11-08 in the x86_64 Edge image under emulation on the maintainer's Mac (`stress-check.py edge`, Microsoft Edge for Linux): 40 downloads clicked at once, all 40 taken by dm, none kept by Edge, 40 of 40 byte-exact, no finding; of the browser check's own 56 steps run with it, the one timing step ("dm's first request comes soon after the click", 700 ms) read 1.25 s under the emulation, where CI's native Linux run reads 247 ms. Linux's window, browser and package checks of 2026-11-07 and 08 all pass (`CHANGELOG.md`). Torrents run there since 2026-11-06 (`stress-protocols.py torrent`, the container with no network as its sandbox, proved by the same probe: 6 swarms byte-exact, 400 files, every path outside the folder refused, pause and resume, a magnet through a tracker), twice with no finding. Covered before: SOCKS5 proxies, and torrents on loopback inside a sandbox and once against webtorrent.io's public Creative Commons "The WIRED CD" Windows 11, 2026-11-07 (debug build, temporary files on D:): `scripts/stress-check.py`, all 23 phases, byte-exact wherever a file was checked; the `control` and `hostile` phases made to ask Windows (they had never run there) and clean; two findings: `bigqueue` (20 000 listed links slow small downloads to 9.8 files/s, the known cost of rewriting the list, `docs/decisions.md` 2026-10-27) and `leak` (row 156). `scripts/stress-protocols.py` socks, proxy, mirrors, ftp, watch, scheduler, mcp, lossy and manifests: no finding. Not on Windows: `fulldisk` (needs a small volume, a VHD made in Disk Management) and `torrent` (its sandbox is macOS's or a container's) | PARTIAL: Windows run 2026-11-07 | On macOS and Windows the clipboard offer needs a clipboard that is not a person's (Linux's is covered); on Windows, `fulldisk` a small volume (a VHD) and `torrent` a sandbox that keeps it off the public network |

| 30 | Brave (1.98.60, CachyOS) listed "Unsafe attempt to load URL chrome-extension://…/pkg/dm_catcher.js (and `_bg.wasm`) from frame with URL https://www.youtube.com/… Domains, protocols and ports must match", from `content.js`'s `import()`; `scripts/linux/cachyos-brave/stress.sh` on real YouTube showed 28 such lines in 40 rounds with either manifest. Changed: the background injects `pkg/panel.js` (WebAssembly written in), the page fetches nothing of the extension's, and `web_accessible_resources` is gone. Checked on CachyOS + Brave against a local page at a youtube.com address (plain, service worker, restart, Private window, 15 stress rounds: panel every time, no request to `chrome-extension://` from the page). On real YouTube in Microsoft Edge 156 on macOS (2026-10-29, `youtube-check.py`, 49 of 49) the panel loads, lists the qualities and the video arrives whole; not yet run on real YouTube in Brave: Google answered the container's Brave with its "unusual traffic" page after the earlier runs. The Docker QA suite's browser check (Chromium, 69 checks) passes with it, including "on a site whose policy forbids WebAssembly the panel stays away, quietly", which it first failed (BUGLOG.md, BUG-005) Tried again 2026-11-06 (the image built afresh on this Mac, amd64 under emulation, `VARIANTS=fixed`): Google still answered every round with its "unusual traffic" page (`google.com/sorry`), so no YouTube page loaded. Run on real YouTube on 2026-11-08 (`VARIANTS=fixed sh scripts/linux/cachyos-brave/stress.sh`, the container's Brave under amd64 emulation on the maintainer's Mac; Google let it in): 40 rounds of opening, hovering, related videos, embeds, back and closing tabs and the worker's restarts, the panel shown in 32 of 37 checks (the 5 others on a tab left at `about:blank`, no video there) and 0 "Unsafe attempt" lines | PARTIAL: the reporter's word | Confirm with the reporter (Brave on CachyOS) after the next extension release |
| 37 | Some YouTube videos and signed-in accounts answer "Sign in to confirm you're not a bot" or stop the stream after about a megabyte: YouTube wants a proof-of-origin token. dm passes the page's own cookies from the extension, which covers most cases, and says the rest in words | BLOCKED | Needs a proof-of-origin token provider, which requires running YouTube's bot-detection program — upkeep the maintainer declined. Revisit if a Rust or Deno provider appears; LAST: done together with the maintainer during the human QA, who guides it (maintainer, 2026-11-01) |
| 39 | Secrets are kept in files of dm's own, readable by this account alone (0600): a row's login and its browser cookies in `secrets.json` beside the queue file, and the Sign-ins' and the proxy's passwords in the settings file (`docs/decisions.md`, 2026-10-17). Built and tested on Windows 2026-11-04 (16 `secretstore` tests, 4 of them against the real Credential Manager, and 38 settings tests pass): on Windows the queue's `secrets.json` is sealed (ChaCha20-Poly1305) under a random key that Windows Credential Manager keeps (`secretstore.rs`); a file in clear moves into the store on its first read, and where the store is not there the file is as before and a notice says so. Also built and tested there: the Sign-ins' and the proxy's passwords go to the same store under a name each and the settings file keeps them empty (`dm-shell/src/settings/secrets.rs`; a file with clear passwords is moved in on its first read; a refusing store leaves them in the file and a notice says so), and the macOS Keychain (`security`) and Linux Secret Service (`secret-tool`) backends are written as small command-line wrappers that could not be compiled or run on this Windows computer (an ad-hoc-signed dm on macOS may be asked for each item at every rebuild, and CI's Linux has no Secret Service, where the file stays as before). Found by the security test of 2026-10-29 (`docs/security-report.md`, L1)  Compiled and proved on macOS and Linux on 2026-11-05: three tests against a real store (`secretstore::tools::tests`, run with `DM_SECRET_STORE_TEST=1`): kept, read, replaced and forgotten; a 32-byte key with zeros comes back whole; a file sealed under a key the store keeps opens, and fails once the key is gone. macOS: the `security` tool on a keychain of the test's own (`ToolStore` names it on every command; the account's search list put back at once and checked unchanged; nothing written to the login keychain). Linux: `secret-tool` against gnome-keyring's Secret Service in the Debian container | PARTIAL | Windows: proved 2026-11-07 in the release build against this account's real Credential Manager (13 checks): a row with a login in its address and a browser hand-off with cookies (the host, `download.start`) are fetched with their sign-in; `queue.json` holds neither and `secrets.json` is a ChaCha20-Poly1305 envelope with neither in clear; dm ended and started again, both rows come back (the browser's still signed in), resume with ranged requests carrying their sign-in, and both files are whole. Left for Windows: a copy of the home started under another Windows account, which must not open them and says so once ("the saved logins and cookies could not be opened (the key that seals the secrets is not in this computer's store) ..."); this computer has one account, and the maintainer left it for later. macOS: proved 2026-11-07 with dm.app installed in /Applications (ad-hoc signed, a home of the test's own, the account's login keychain): a row with a login in its address and a browser hand-off with cookies are kept in a sealed `secrets.json` (the key `dm:queue-secrets-key` in the keychain; neither file holds either in clear), and after dm is quit and started again both resume and finish against a server that refuses them without; a Sign-in (a user name with a space and a quote) leaves `settings.json` for the keychain, under its base64url name, and is read back at the next start with no notice; no keychain prompt at any step. Linux: proved 2026-11-08 with the installed .deb in the Debian container, in a D-Bus session with an unlocked gnome-keyring (`scripts/linux/secrets_check.py`, run by `linux-package.sh`, 11 checks): first it found the package did not bring `secret-tool` (Debian and Ubuntu leave `libsecret-tools` out), so the installed dm kept a row's login and cookies in a `secrets.json` in clear and a Sign-in's password in `settings.json`; the .deb, the .rpm and the PKGBUILD now name it, and then a row with a login in its address and a browser hand-off with cookies are fetched with their sign-in, no file of dm's home holds either or the Sign-in's password, `secrets.json` is a ChaCha20-Poly1305 envelope under `dm:queue-secrets-key` in the keyring, the Sign-in's password is in the keyring and the settings file keeps it empty, and after dm is ended mid-download and started again both rows resume with ranged requests carrying their sign-in and finish whole. The AppImage cannot name a dependency: where `secret-tool` is missing it keeps the files as before and says so once |

| 45 | A part of a video is cut from YouTube's MP4 streams (dm's own reading), and now from HLS streams (the segments whose summed lengths overlap the span; a fragmented MP4 trimmed exactly, a transport stream joined at the nearest segments), DASH streams (template, timeline, list, or one file with an index; trimmed exactly in MP4) and plain MP4 files (the sample tables' byte ranges and the frames between, built by dm's MP4 writer). Still not cut: a WebM DASH pair is joined at the nearest pieces, not trimmed; a plain MP4 whose server gives no ranges or length is refused; the panel's Part line (contract row B59) was seen on a real YouTube page in a visible window on 2026-11-02 (the start and end typed, the part saved), and not yet on a real HLS, DASH or plain MP4 page. Run on real streams 2026-11-06 (the control port's `downloads.add` with `from` 0:30 and `to` 0:40, headless, macOS): Apple's HLS example in fragmented MP4, Mux's HLS test stream in transport stream, Akamai's DASH Big Buck Bunny and two plain MP4 files (archive.org, media.w3.org) each gave a part of 10.0 s, picture and sound, that ffmpeg decodes with no error; the plain MP4's first frame is the source's frame at 0:30 exactly (PSNR infinite, about 10 dB against 0:29 and 0:31); the fragmented MP4 keeps its stream's own clock, which starts at 10 s as the source's does | PARTIAL | Offline tests pass (2026-11-02: 61 stream + 23 dash + 2 mp4mux::part, all ok); the cutting is checked on real streams; left: the panel's Part line (B59) on a real HLS, DASH and plain MP4 page in the final human QA |
| 47 | A second network path (a phone's connection used beside the computer's own) is built: Options › Advanced › Network › Second network picks an interface (`dm_core::interfaces`), and every other connection of a ranged download goes out on it, bound to its address and to the interface (`second_path.rs`, `ranged::run`); the ranges go to whichever connection frees up, so the faster path takes more. Tested with this Mac's own Wi-Fi address beside the loopback (`tests/second_path.rs`: the server saw both source addresses, the file whole). Each path's share is shown since 2026-10-31: the progress window's Connections section says "{} of {} came over the second network", from which ranges a second-path connection took last (`second_path::fetched_by`, in this run; `dm-desktop/tests/second_network.rs`, a test binary of its own as the second path is the process's). Since 2026-11-01 HLS segments and DASH pieces alternate onto it too (`second_path::client_for_piece`; tested 2026-11-05, `tests/second_path.rs` `a_streams_pieces_alternate_onto_the_second_path`: the server saw both source addresses, the stream whole). On Windows the networks were never listed (`interfaces()` was empty there, so Options offered none): since 2026-11-05 Windows' adapters are read with `GetAdaptersAddresses` by the names Windows shows (`windows_lists_its_adapters_by_name`), and the tests' second path is 127.0.0.2 there, as Windows refuses the loopback from another adapter's address. Left: it has not been run with a real phone connection; single-connection downloads use one path (nothing to split) | PARTIAL | Run once with an iPhone over USB and Wi-Fi together and record the speeds and the shares (no phone here) |
| 51 | The extension release's Mozilla signing step (`release.yml`, `web-ext@8.3.0 sign --channel=listed`, first run in CI on 2026-10-30 for 3.2.0) was refused by AMO's validator as "Unsupported file type, please upload a supported file (.crx, .xpi, .zip)", though `web-ext build` of the same `dist/firefox` makes an ordinary `dm_download_manager-<version>.zip`. The step no longer stops the release; the Firefox add-on on AMO stays at its last hand-signed version until this is fixed. Cause found 2026-11-06: web-ext 8.3.0 builds its upload with a stand-in for a File (a Blob subclass that only names itself one), and the Node the runner has (22 and later; 26 here) encodes it as `filename="blob"`, an upload with no `.zip` or `.xpi` name, which is AMO's "Unsupported file type" word for word; Node's own File keeps the zip's name (checked by encoding both). web-ext 8.4 and later send Node's File: both signing workflows now pin 10.8.0 (same `sign` options; `web-ext lint` of `dist/firefox`: no error) | PARTIAL | The next tagged extension release with the AMO keys set: the signing step passes and the listed 3.2.1 or later appears on AMO |
| 57 | Expired-address resume (built 2026-11-01, `Entry::fresh_link` and `Queue::adopt_fresh_link`: a browser row whose signed address expired waits and takes the browser's next hand-over of the same file, matched by name, exact size and family of servers) was never reached in the app until 2026-11-05: only its test set `fresh_link_ok`, and no hand-off from the inbox did (fixed: the host marks a browser's hand-off `from_browser`, and the inbox offers it to the waiting row first where Download details does not ask, B63; with the row-121 renewals). It has run only in offline tests (a row that failed at its very first request, with no name or size yet, now takes the fresh address of the same server family and the very same path; `tests/fresh_link.rs`) | PARTIAL | A real-site run with a signed address that expires (a video site's CDN link) in the human QA at the end |
| 60 | Every dialog is a window of its own that can be dragged anywhere (`dialogs/apart.rs`, contract row B42, built 2026-11-01), proven on this computer with the system's mouse for every kind of dialog (each dragged wholly outside dm's window and read back from Windows as a top-level window), but the maintainer could not drag one out on their own computer on 2026-11-01: only a build made after the change shows it (`D:\dm-target\debug\dm-desktop.exe`), an older or installed dm draws dialogs inside the window. One monitor only here. `scripts/human-qa.py` B42 failed on 2026-11-02 only because another window lay over the dialog's title bar and took the mouse press; the script now brings the dialog to the front first, and on 2026-11-04 all 61 rows hold | PARTIAL | Delete this row when the maintainer has dragged a dialog out of dm's window with that build, and on a second monitor where they have one |
| 63 | Reddit was tried on 2026-11-01 the same way: the listing and post pages answer an automated browser with a "Prove your humanity" challenge, and old.reddit asks for a login, so no post could be opened. Reddit's video is an HLS or DASH stream with the sound apart; the extension lists HLS playlists a page loads (checked live on X and Vimeo, whose audio renditions dm joins) | PARTIAL (2026-11-02, signed in with a test account in Microsoft Edge: Reddit's security policy forbids WebAssembly, so the full panel never loaded; a plain-JavaScript panel now takes its place and its Download video button shows on the post's video and sends it to dm. The list now also offers the post's own HLS playlist (the `src` of Reddit's player element), downloaded and joined by dm's own HLS engine: on a post with a sound track a whole video with sound landed (16 s, 854p, audio and video); on a silent post the playlist names no audio and the file has none, as it should) | A person opens a Reddit video post in a normal window of the browser with the extension and runs the check with its address (`SITES` in `scripts/sites-check.py`); LAST: done together with the maintainer during the human QA, who guides it (maintainer, 2026-11-01) |
| 65 | Side-by-side of reference A and dm from the outside (`D:\compare\compare.mjs` and `compare2.mjs`, 2026-11-02; results in `docs/reference-behavior.md`). Same outcome in 8 of 12 local scenarios (file, name, bytes); 4 differ (a named or UTF-8-named download, a server that lies about length, an HTML page) and may be the harness misreading reference A's dialogs. Closed in dm 2026-11-02: a first ranged request answered with a server error is asked again without `Range` (reference A asks plain first; `probe.rs`, test `a_server_that_fails_ranged_requests_but_serves_the_plain_one_gives_the_file`). Measured, not changed: 64 MiB is cut by halves in reference A (20 connections) and in fixed 8 MiB pieces in dm; FTP is cut by halves with a LIST in reference A and in 1 MiB pieces after SIZE/MDTM/REST 0 in dm; a first download cut at 512 KB left reference A with no second request and no file (dialogs were auto-closed, so not yet explained). dm against 14 real sites 2026-11-02: 14 of 14 as expected (YouTube whole at 240p, direct files byte-identical to curl, HLS, DASH, a torrent, and two refusals with a reason). Left: resume after kill, the second-look data check, speed and time-left smoothing, SOCKS4/5 and CONNECT proxies against reference A, the finished-parts join, and the four differing scenarios Run again 2026-11-07 on Windows with reference A's windows read before they were closed (`D:\compare\plan_side.py`, the video test plan's cases; `docs/reference-behavior.md`, "The video test plan's cases, side by side"): the cut one and a 10 s outage are whole in both, so reference A does retry (the 2026-11-02 runs had closed its windows unread); ranges, no ranges, three redirects, 4.5 GiB and four public files give both apps the same bytes and names; reference A gives up on a 429 and on an unknown host where dm waits and tries again, and asks for a login on a 401. Still to run: the named and UTF-8-named download, the server that lies about length, the HTML page, resume after a kill, the second-look check, speed and time-left smoothing, the proxies and the finished-parts join | PARTIAL | At the human QA with reference A's dialogs left open: re-run the four differing scenarios and the cut one, then the left-over measurements; change dm only where the maintainer's rule applies and dm is not already better |
| 66 | Interface study of reference A against dm, 2026-11-01 (43 pictures of reference A, 14 of dm; behaviour and information only, nothing of its artwork or wording is copied), first batch of improvements, asked for by the maintainer: (a) a file-type icon on every row of the list; (b) a double-click on a running row opens its progress window, and the window's title shows the percentage; (c) Add URL offers the addresses used recently; (d) Find and Find Next in the list (Ctrl-F, F3) beside the search box; (e) the Download complete notice offers Open, Open with and Open folder | PARTIAL | Built on 2026-11-01 as contract rows B45 to B48, with unit and window tests and the wording in all thirteen catalogs: (a) a shape per file kind on each row's picture (the coloured sheet existed, the kinds differed by colour only); (b) the double-click on a running, waiting or stopped row and the percentage in the progress window's title; (c) the rule that Add URL's Recent list keeps and offers no address with a secret in its query (the list and its Clear history existed); (d) Find previous, Shift-F3 (Find, Find next and F3 existed); (e) was already there (Open, Open with… through an app picker, Open folder). Seen in a visible window on 2026-11-02: B45 to B48 all pass in `scripts/human-qa.py`. Left: the maintainer's own look |
| 68 | The mechanism for site rules served by the app is written (`dm-protocol` `Rules`, `dm-shell/resources/rules.json`, an optional newer `rules.json` in dm's home, the hello's `rules`, merged in the extension); its shipped document (revision 3) holds answer rules for seven sites and finders for Facebook and Vimeo, written from what the comparison with reference A showed and not yet checked on those live sites. The update check also reads an optional `rules` entry of the manifest (address under dm's releases, SHA-256, size) and keeps a newer document in the home (built and tested: `update.rs` `the_manifests_rules_entry_is_read_only_whole_and_from_the_projects_releases` and `fetch_rules`) | OPEN | At the QA: write the first real rules from what the comparison with reference A shows, and check the extension with the app serving them; publish a `rules` entry in `latest.json` with the first real rules |
| 69 | The page hook that reads the JSON answers of a site's own requests is written (`extension/document.js`, `content.js`, `decide/answers.rs`), driven by the `sites` of row 68's rules; seven sites have a profile (`rules.json`), none of them checked live. It missed answers the page fetched before `content.js` ran (document_idle); since 2026-11-06 the hook keeps the JSON answers that come before the rules (24 at most, 4 MB) and passes on the ones the rules name when they arrive (`chrome-check.py`, "an answer the page fetched before the rules arrived … is read once they do": fails without it) | OPEN | At the final QA, on real pages: find the sites that build their stream from an answer, write their profiles, check each stream is offered |
| 71 | Instagram and TikTok were tried on 2026-11-02 with `scripts/sites-check.py instagram tiktok x` in a visible Edge, no sign-in. Instagram: the panel was refused on a reel when a layer from another branch of the player sat over the video (now accepted when the layer and the video share a player-sized parent, `content/visible.rs`); the list reads the page's own renditions (one site-agnostic scan, `decide/page_lines.rs`, see decisions) and offers "MP4 file, 41 sec, quality 1280p HD" beside dm's own line; a whole video with sound landed. Open: Instagram's logged-out page puts its own sign-in dialog over the video after a while, and the panel stays away then (a full-page layer is refused by design), so one run in five fails (a feed such as Facebook's has its renditions in the answers of its API, which the scan now reads too: the list shows there); TikTok's page also fetches its own 2-second sign-in clip, and when that is the only file seen the panel offers it (about one run in four); TikTok's panel works and a whole video with sound (portrait 1280p) landed; its list comes from the same scan, and the clip tried names one quality, so one line is right for it, other clips list each; X works (a single file). Run again 2026-11-06: TikTok's list held the video's own file alone (no sign-in clip) and a whole video with sound landed; Instagram's logged-out reel was covered by its sign-in dialog again, so the panel stayed away. | OPEN | Instagram: a signed-in run, as row 62's, at the final QA with the maintainer; TikTok's clip: watch for it in that run |
| 72 | Floating button, quality list, matched to reference A (watched side by side 2026-11-02, `docs/reference-behavior.md`): DONE the order of a page's renditions (best first; Facebook reel and Instagram, tested). Since 2026-11-05 72a to 72d are built (the title first on each line, Download all, caption files, 60 fps lines), at the maintainer's word that dm "should work exactly like" reference A; left: 72f, the side-by-side on Reddit and TikTok, which needs reference A's extension added to that test profile by hand. The check for each is the same two pages in one Edge window (`SITES_HOLD`, `DM_KEEP_OTHER_EXTENSIONS`) and a screenshot of both lists. | PARTIAL (72f) |
| 72f | Reddit and TikTok were not yet compared side by side: the Reddit test profile (`D:\dm-reddit-profile`) has no reference A extension, and TikTok's page had only one quality. Needs the extension added there by hand, then the same screenshots. | OPEN |
| 75 | File names: reference A names a download from the page's title; dm uses the media file's own name (a hash, or `video`). Spec G75. | CODE BUILT AND TESTED (2026-11-03: dm-core `naming.rs` tests pass; title carried from `document.title` through `PanelClick`, `Start.title`, `host::Request.title`, `AddOptions`/`Entry`/`download::Options.title_hint`; `assemble.rs` names by the title only when no name was chosen and the found name is meaningless; a download the browser caught sends the title of the open tab at its referrer, `web/downloads.rs` `tab_title`, `decide::referrer_title` with a test). Found live on X 2026-11-06 and fixed: the panel never sent the title (`content/handoff.rs` read it for the list's lines only), so X's video kept the name "GiFPAi8WYAEgiBu.mp4"; every panel hand-off now carries it (`video_title`), and the same post saved as "NASA on X - Everybody, wave! @fireflyspace’s Blue Ghost lunar lander caught Earth on video as the lander continued.mp4". Facebook signed in, 2026-11-07: a video post's page and the Watch page are both titled only "(1) Facebook", so the file stays `video.mp4` (the post's words are not in the page's title; a site rule's `title` selector, row 124, is the way to them). Left: Instagram signed in, and whether reference A names a Facebook video otherwise (step 9) |
| 77 | Pages where only one manager showed a bar (logged out). Not a defect; recorded so a later run is not misread. | NOTE |
| 78 | Desktop apps compared side by side 2026-11-02 (`docs/reference-behavior.md`). Checked in dm's code and found already done, so not gaps: a connection limit per server (`Options::host_limits`, Options > Advanced), a command run after each download (a virus checker or a script), the last folder per category (`last_folders`), the file's date from the server (`set_modified`), the list columns Last try and Description (and Referrer and Folder besides). | NOTE |
| 93 | Order to build, rows 72 to 98: G73, G75, G79, G80, G95, G96, G97, then G82, G85, G87, G86, G92, G90; G98 is a measurement to run first. Wait for the maintainer's word before 72a to 72d, G74, G76, G91 (G84 built 2026-11-08 at the word of 2026-11-07: CHANGELOG, `docs/decisions.md`) (each changes visible wording, layout or what a click does). | PLAN |
| 99 | Extension, watched 2026-11-02 with both on one page: the right-click items match in kind (a page: dm "Download all links with dm", reference A "Download with" its own name; a link: "Download link with dm" and the same; a selection: "Download selected links with dm" and the same). Differences are rows 74, 76, 86 and 94. | NOTE |
| 102 | **Torrent: what was tested 2026-11-02, and what is left** (reported: a magnet button on a torrent site does not open dm). Tested and working: the engine (a public test torrent, Sintel, 129,241,752 bytes in about a minute from the real swarm); a magnet link opened through a Windows protocol handler starts in dm (a handler registered by hand for the test, then removed); seven ways a page opens a magnet (plain link, link with a script handler, button that sets `location`, new-tab link, icon inside a link, `window.open`, a link a script makes and clicks) all reach dm through the extension. Not tested: the real 1337x site (behind a Cloudflare check an automated browser does not pass), a build from this branch installed on the person's computer. Specs T1 to T8. | NOTE |
| 109 | T7: a magnet opened in a browser's Reading view (`read://`), a PDF viewer, or another program reaches dm only through the system handler (extensions do not run there). Spec T7 is a check list for a person on the real site. | NOTE |
| 110 | EDM-V1, video quality grabbing on every site. Spec EDM-V1. | PARTIAL: built; run live logged out on 2026-11-06 (`scripts/sites-check.py`, Edge, macOS): Vimeo's playlist named 1080p, TikTok's file found by its type in a script's answer, X's file, each a whole video with sound in dm; not yet run signed in: the page hook relays manifests and media files from any host and observes MediaSource (`extension/document.js`, `content.js`); `rules.json` revision 2 has answer rules for six sites. Of the spec's four unbuilt parts, read again 2026-11-06: AdaptivePair is `StreamKind::Picture`/`Sound` joined by `Stream::sound` and the app's `download_media_pair` (row 119); headers in Start are row 116; RefreshStream is rows 57 and 121; the itag table is not wanted (YouTube's heights come from the player response). Nothing of it is left to build | Run `scripts/sites-check.py` on a signed-in machine; build the rest after the maintainer says which is wanted |
| 121 | **An expired stream address is renewed from the open page.** Reference A takes the address at the moment the player asks for it and starts at once, before the signature runs out. dm's `fresh_link` re-reads the page itself (row 57), which fails where the page refuses dm (row 120). `RefreshStream` (row 110) is not built. Spec R121. | BUILT 2026-11-05, not yet run on a live site: the newest address at hand-off (`RequestHeaders::renewed`, as before), and now a download that already started and hit 403/410: the extension offers the page's newer signed address of a stream it handed over (`decide::Handed`, `Start::renews`, only to an app whose hello says `renews`), and the app gives it to the waiting row or drops it (`shell::take_from_browser`). Tests: `decide::renewal` (5), `dm-shell/tests/fresh_from_browser.rs` (3, a local server whose old address answers 403), the host's `a_renewal_is_filed_for_a_running_dm_only_and_never_asks_or_launches`, dm-protocol's `a_hello_says_whether_the_app_takes_renewed_addresses` | At the final QA: a real stream whose address runs out mid-download (a video site's CDN link) with the tab left open, in Microsoft Edge, goes on in the same row |
| 122 | **YouTube: what reference A reads from the page that dm does not.** From the page: `ytcfg` (`STS`, `DATASYNC_ID`, `LOGGED_IN`, `VISITOR_DATA`, `EOM_VISITOR_DATA`, `PLAYER_JS_URL`, `SESSION_INDEX`); the player script, fetched inside the page, with the signature and n functions cut out by pattern and sent to the app; and one `sessionStorage` value keyed by a hash of the account or visitor id (probably the cached proof-of-origin token). Its MIME table knows `application/vnd.yt-ump`, the stream format YouTube's newer player uses. (The reference's own write-up says it never deciphers; its code shows it does both.) dm reads `ytcfg` and visitor data from the page it fetches itself. Spec R122. **Measured 2026-11-06, no sign-in** (`cargo run -p dm-core --example youtube_read -- <url>…`): an ordinary video (14 formats), two 4K videos (44 and 41, to 2160p), a music video with signed formats (26, to 2160p) and a Short (23) were read whole, every format with its address: no video served only as UMP, and nothing missing that the page would have given. An age-restricted video says "Sign in to confirm your age" (needs an account); a live stream is turned away by design (`a live stream`; it is recorded from the panel's HLS). What is left needs an account: age-restricted, members-only and a signed-in-only quality, read with a person's sign-in. | NEEDS A PERSON | Sign in to YouTube in the browser with the extension and run the four kinds through the panel; for each that fails, send the page-side fact (`DATASYNC_ID`, `SESSION_INDEX`) over `dm.media` and use it in `youtube.rs` |
| 131 | **Read in reference A's extension and not to build** (2026-11-03). Commercial DRM (Widevine, PlayReady, FairPlay): reference A does not decrypt it either; dm already says so (`browser/src/decide/protection.rs`, G73). AES-128 HLS is already in dm (`download/hls.rs`); SAMPLE-AES is treated as DRM, as reference A does. A kernel network driver: its write-up says it tunes TCP windows; nothing in the extension shows that, and it needs a signed driver. A named pipe in place of native messaging: dm's native messaging is already direct. Following the browser's proxy (`proxy.settings`, PAC data, `Proxy-Authorization`) is row 80's "browser fallback". | NOTE | |
| 132 | **Real-site side-by-side, 2026-11-04** (`D:\compare\sites.mjs`: reference A by its command line, dm by its control port, one after the other, same machine). Same bytes and same name on 9 sites: proof.ovh.net 10 MB (10.8 s / 10.7 s), Hetzner 100 MB (78-127 s / 88-90 s), a GitHub release (5.9 / 4.4 s), GitHub codeload with no length (9.3 / 7.0 s), Mozilla's redirect to a 59 MB installer with a name from the redirect (21.2 / 20.8 s), python.org 28 MB (10.4 / 12.8 s), W3Schools mp4 (5.8 / 4.6 s), GNU FTP (8.2 / 11.7 s). What differed is in rows 135 to 139; rows 133 (a server that refuses dm's made-up `Referer`), 134 (a limiter answering 429 each second) and 140 (empty store zips on Windows) are closed (`CHANGELOG.md`). Pause then resume, and a hard kill at 30 MB then a restart, both finish a 100 MB file byte-identical to reference A's | NOTE | |
| 142 | **Real video sites, 2026-11-04, both apps in Microsoft Edge.** Run again on the day's last build: YouTube, Facebook (signed in), X, TikTok, Vimeo and Instagram (logged out) in the Facebook test profile, and two Reddit posts in the signed-in Reddit test profile: each gave dm a whole video with sound (`D:\compare\sites-final`). Earlier the same day Instagram's button did not show: its logged-out page lays a sign-in dialog over the reel after a while, and dm keeps its button off a covered video (row 71, about one run in five). Reference A, read in the maintainer's own Edge on YouTube (its Manifest V2 extension is disabled by Edge 156 in a test profile): its list now matches dm's but for 60 fps lines (row 72d). Reddit's plain-JavaScript panel (Reddit forbids WebAssembly) still words its lines its own way ("HLS stream, best quality") | OPEN | Instagram's covered reel; Reddit's panel in reference A's words; row 72d; the other sites beside reference A's panel |
| 143 | **Torrents against a BitTorrent client, 2026-11-04** (`D:\compare	orrent_check.mjs`, Sintel, 11 files, 123 MB). Works: a magnet (first bytes in 5 s, paused at 54.8 MB and held there, resumed, done in 34 s) and a `.torrent` file from the web (done in 28 s); every file byte-identical to the web seed's copy. Missing beside a BitTorrent client: (a) no seeding once finished: `torrent.rs` deletes the torrent from its session and moves the files (no upload limit or share ratio either); (b) no peers, seeds or upload speed on the row or in its details; (c) no choosing which files of a torrent to fetch. Reference A has no BitTorrent at all, so these are the maintainer's "like a BitTorrent application" | NOTE | (b) and (c) are done (`CHANGELOG.md`); (a) is row 147 |
| 135 | **Download all links, measured on live pages:** on 7-zip.org/download.html dm queued 85 entries at once (52 are files, the rest are site pages such as `faq.html` and other-language home pages); on python.org's 3.13.0 release page 148 entries for 11 files. Reference A (select the page, right-click, "Download with" its name) shows a window with every link unchecked, its file name, type, size, source, the folder each goes to and the link text, with Check all, Uncheck all, Edit, "Hide HTML files", "Hide images located on this web page", "Hide repeated files" and a Save-to choice (by category, one category, one folder); screenshot `D:\compare\ref-a-links-0.png`. Data for row 94 | NOTE | Built from it 2026-11-05: row 94, closed |

### A BitTorrent client's features, from the maintainer's list (2026-11-05)

The maintainer supplied a BitTorrent client's feature list to build against (its name is left out, as for
the reference apps). Checked against dm's code and librqbit 9.0.1 (the torrent library dm uses) the same day.
Already in dm, so not rows: DHT, peer exchange (`ut_pex`) and local peer discovery (librqbit; DHT and local
discovery are off behind a SOCKS5 proxy, as they would leave it); magnet links and `.torrent` files (rows
100 to 104); a speed limit for everything and one for a single download, both applied to a torrent (the shared
limiter and the row's own, `torrent.rs` `download_cap`); dead and slow peers dropped (librqbit); every piece
checked by its hash and only a bad one fetched again (librqbit; a resumed torrent's files are checked, not
fetched again); a JSON control port and MCP for scripts and other front ends (`--control`, `app/control.rs`,
`mcp.rs`); the engine without the window (`dm-core`, `--headless`); the watched folder takes `.torrent`
files (`app/watch.rs`); the command after each download runs for a finished torrent too (`app/sync.rs`). How
little memory dm takes is row 137.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
| 152 | **The sites reference A works on** (the maintainer's list, 2026-11-05): dm should work on each as reference A does, checked by a person in the human QA beside reference A. Video sites (YouTube, Vimeo, Dailymotion, Twitch, Facebook, Instagram, TikTok, X, Reddit, Rumble, Bilibili and more), file hosts and cloud storage (Google Drive, OneDrive, Dropbox, MediaFire, MEGA and more), software repositories (GitHub, GitLab, SourceForge, Microsoft's and Apple's download pages and more), course platforms, music, stock images, and four sites for testing the engine (an Ubuntu ISO mirror, VLC, Test-Videos.co.uk, NASA's library); 68 sites in all, each with what to try, whether it needs a sign-in, and how dm gets it, in [site-list.md](site-list.md). Already run beside reference A: YouTube, Vimeo, Facebook, Instagram, TikTok, X, Reddit (row 142) and a GitHub release (row 132). Expected and not gaps: a DRM-protected stream refused by both (row 131); a site that is gone Run without reference A on 2026-11-06: eleven real download pages' own buttons (`scripts/handover-check.py`: plain links, script-started downloads, countdown and mirror pages, signed redirects, an address the server names) each handed over to dm out of the box; `docs/site-list.md` has the table 2026-11-07, `scripts/video-sites-check.py` (dm alone, signed out, a new Edge profile on Windows): YouTube, Vimeo, TikTok, a Twitch highlight and clip, and PeerTube each gave a whole video with sound; Rumble (Cloudflare's check) and Reddit ("Prove your humanity") answered the automated browser with a bot check; Dailymotion's player frame could not be held by the check; Bilibili was row 155 (closed on macOS the same day: a whole video with sound). Beside reference A by its command line, the plan's public files (Internet Archive, Wikimedia Commons, NASA, Test-Videos) came the same bytes and names in both apps | OPEN | In the final human QA (LAST, with the maintainer): run every site of `docs/site-list.md` in Microsoft Edge with both extensions, fill in its two result columns, and open a row here for each place dm does less than reference A |
| 151 | Protecting the code from being cloned (the maintainer's notes, 2026-11-05). Done: GPL-3.0-or-later with a section 7(b) attribution term (2026-11-06); the repository stays public (the maintainer's answer, 2026-11-06: Releases, the AUR package and SignPath need it); builds are hardened, not obfuscated (the stores refuse obfuscated extension code): no path of the building computer in the app or the extension's WebAssembly (`scripts/release-flags.sh`, checked in CI), and no name, e-mail or computer path in the repository's files and pictures. Left: watching for copies | OPEN | Search GitHub now and then for dm's distinctive strings (`dm-engine`, `com.dm.engine.json`, `easy-download-manager@pranavo72bex`) and file a DMCA notice for a copy that drops `NOTICE` |
| 162 | **YouTube's weekly check is not running, and its cure is the external video tool's scripts** (the maintainer, 2026-11-08: the weekly YouTube check up and running, fixing itself when YouTube changes, with no external video tool and no other third party). Found 2026-11-08: (a) `.github/workflows/solver-update.yml` is not on `main` (GitHub answers 404 for it), so the schedule has never fired; it lives on `parity-with-reference-a` only. (b) All it does is move the pin of the two challenge scripts dm fetches from the external video tool's project (`youtube.rs` `SOLVER_VERSION`, `SOLVER_LIB`, `SOLVER_CORE`, `solver_scripts`; `scripts/update-solver.py`), the one dependency on that project left. (c) Today's player (`a5c2aafe`, 2.6 MB, read without running it) names everything through one string table (`w[…]`, 88 entries) and keeps the signature steps inside a branch of a flattened dispatcher (`Xn(a,b,c)` with numeric selectors), not a function of their own, so fixed patterns would break with each new player. Plan: dm's own solver: run the player in the JavaScript engine with a small stand-in browser, expose its scope (the names in its leading `var` list), and find the signature and `n` functions by what they do (a signature's answer is a reshuffle of its input's characters; an `n` answer a different code of the same kind, the same every time), and where more than one passes, keep the one YouTube accepts for a few bytes of a real stream. Then the weekly job reads real YouTube videos through dm's own reader, records the player and what was found, and opens an issue when a video fails; the ejs pin, `update-solver.py` and `test-update-solver.py` go; `docs/decisions.md` 2026-11-01 ("The YouTube challenge scripts stay fetched…", where an own solver was declined) is replaced. Open questions for the maintainer: whether QuickJS (the engine running YouTube's own code, fetched pinned by digest) counts as third party (the alternatives: a Rust engine such as boa built in, or one of dm's own); and pushing to `main` once verified (not before: the maintainer, 2026-11-08, "do not push until verified properly"). Blocked in the 2026-11-08 session: running the real player locally (sandboxed Node, or dm's pinned QuickJS) was refused by the session's permission check, so nothing was built | OPEN: later (the maintainer, 2026-11-08) | A permission to run the player locally; dm's own solver with offline tests on stand-in players built like the real one (string table, dispatcher), then real videos (an ordinary one, one with signed formats, a 4K one, a Short) whole through it; the new weekly job on `main`, run once by hand and green; no address of the external video tool's project left in the repository (`git grep` for its name empty) |
| 163 | **The pull request's GitGuardian check fails on two made-up test passwords in its history.** GitGuardian scans every commit of pull request #33 (202 of them) and found `s3cret` and `p4ss`, the settings tests' logins of 2026-11-01 (commit 62baeca, `dm-shell/src/settings/tests.rs`), replaced by secrets made up for each run (`desktop/test-support/secrets.rs`) on 2026-11-04: nothing like them is in the tree now. Rewriting the branch's history would break the other laptop's work, so it is left. Checked again on 2026-11-08: gitleaks 8.21.2 over the whole history (749 commits, every branch) finds no leak, and GitGuardian's check still names only these two (219 commits of the pull request). Neither the check's Skip action nor its incidents can be reached from a script: GitHub runs a check's actions only from a person's click, and the incidents are in GitGuardian's dashboard; its server-side check reads no file of the repository's to ignore a match | OPEN: the maintainer | In GitGuardian's dashboard, resolve incidents 39386940 and 39386941 as test credentials (or Skip on the pull request's GitGuardian check); the check passes on the next push |
| 164 | **The UI-independent architecture (branch `ui-independent-architecture`).** Steps 1 to 13 of `docs/architecture/audit.md` section 7 done 2026-11-08; Phase 6 measured 2026-11-09 in release (per tick 0.20 ms at 1,000 rows, 2.4 ms at 10,000; `queue.list` 9.8 ms and 100 ms). Step 15 done 2026-11-09: the command registry is the application's (`dm_shell::app::commands`: names, labels, keys as `Chord`s, `applies`, `verb`, `when_done`, `limited`, `recheck`); the window keeps `Shown` and `pressed`. Step 16 done 2026-11-09: `dm_shell::app::names`, `import`, `files`, `schedule::holds_waiting`, `links::Clipboard`. The maintainer, 2026-11-09: "do not rebase for now. just implement the UI-independent architecture fully then we will fix the merge conflict". So the branch stays on its base (f9d071d) until the steps are done. A rebase on parity's fd063e8 was tried that day and kept, unpushed, as local branch `rebased-on-parity-fd063e8`: its three conflicts (parity's B59 rule in `take_handed`, moved into `handoff::Hands::next` with a test; both new decisions at the end of decisions.md; parity's rows 164 and 165, this row then 166) show how to resolve them. The branch's Desktop CI (run 42547059434 on 76494f7) fails "Refuse warnings" on Linux and Windows from its old base (test helpers parity has since limited to their systems); it passes once rebased | OPEN: the maintainer's plan | Step 14 (`Entry::redacted`, the `reqwest` seam), each held to `cargo test --workspace` and `human-qa.py`; then, with the maintainer, the rebase on parity's head (conflicts as above), the branch's CI on Windows and Linux, and the merge after parity ships |

### Implementation specs, rows 72 to 92 (2026-11-02)

Each spec says what a person sees today, what they should see, where in the code it lives, the steps, and the
check that proves it. "Reference A" is the other manager, watched on screen only (`docs/reference-behavior.md`).
A change to visible wording, layout or what a click does needs the maintainer's word first
(`docs/behaviour-contract.md`); such a spec is marked **Approval**, and its commit names the approval and edits the
contract row in the same commit. Check on a real page with the side-by-side setup: `DM_KEEP_OTHER_EXTENSIONS=1`
and `SITES_HOLD=<seconds>` with `scripts/sites-check.py <site>`, then drive the held browser.

#### G73 Vimeo: a normal video refused as "Protected"
**Seen 2026-11-02.** On a public Vimeo video dm's bar says "Protected video" and refuses it. Reference A offers the video and
lists eight lines (a TS file and an MP4 file at each of 270p 453 kbps, 360p 900, 540p 1755, 720p HD 2991). dm's test is only
that the player has a `mediaKeys` object (`browser/src/content/show.rs`, lines 39 to 42: `let protected = Reflect::get(element,
"mediaKeys")...`). A key object does not mean the stream is encrypted.

**Goal.** Refuse a video only when a playlist or manifest the page loaded says it is DRM-encrypted. With nothing read, keep
refusing (no site that really has DRM changes).

**Step 0, ten minutes: look before building.** In the held test browser on the Vimeo page (`SITES_HOLD=600 py
scripts/sites-check.py vimeo`, then `D:\t\drive.py`-style eval), list `performance.getEntriesByType('resource')` names that
contain `.m3u8` or `.mpd`, fetch the first, and look in its text for `#EXT-X-KEY` or `ContentProtection`. Write the finding
here. If Vimeo's playlist is encrypted with `METHOD=AES-128` and a key address it is clear to dm's HLS engine; if it names
`SAMPLE-AES` or a DRM system it is protected and this row closes as "dm is right". Do not build before this is known.

**Build (if Step 0 says clear).**
1. New file `browser/src/decide/protection.rs`, exported from `browser/src/decide.rs`:
   ```rust
   /// Whether one playlist or manifest says its media is encrypted by a DRM system.
   pub fn is_drm(text: &str) -> bool;
   /// What the texts the page loaded say: Some(true) if any is DRM, Some(false) if at least one
   /// playlist or manifest was read and none is, None if none was read.
   pub fn stream_protection(texts: &[String]) -> Option<bool>;
   ```
   Rules for `is_drm`: an MPD (`<MPD`) with a `ContentProtection` whose `schemeIdUri` (case-insensitively) is one of
   `urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed` (Widevine), `urn:uuid:9a04f079-9840-4286-ab92-e65be0885f95` or
   `urn:uuid:79f0049a-4098-8642-ab92-e65be0885f95` (PlayReady), `urn:uuid:94ce86fb-07ff-4f43-adb8-93d2fa968ca2` (FairPlay);
   an HLS text (`#EXTM3U`) with an `#EXT-X-KEY` line whose `METHOD=SAMPLE-AES` (or `SAMPLE-AES-CTR`) or whose `KEYFORMAT` is
   anything but `"identity"`. `METHOD=AES-128` with `KEYFORMAT` absent or `identity` is NOT DRM. `ContentProtection` with
   only `urn:mpeg:dash:mp4protection:2011` (common encryption's marker, no system) is not DRM by itself.
2. `extension/document.js`: add `|#EXTM3U` to `MEDIA_WORDS` so a media playlist (whose lines name `.ts` or `.m4s`, not `.m3u8`)
   is kept too.
3. New `browser/src/content/bodies.rs`: move `kept bodies` out of `handoff.rs` `with_page_lines` into
   `pub(super) fn kept_texts() -> Vec<String>` (reads `globalThis.__dmBodies`), used by both.
4. `show.rs`: replace the three lines with
   ```rust
   let key_object = Reflect::get(element, &"mediaKeys".into()).ok().is_some_and(|k| !k.is_null() && !k.is_undefined());
   let protected = key_object && decide::stream_protection(&bodies::kept_texts()) != Some(false);
   ```
**Tests (`browser/src/decide/tests.rs`).** `is_drm`: a Widevine MPD true; a PlayReady MPD true; an MPD with only the
mp4protection marker false; a clear MPD false; HLS `#EXT-X-KEY:METHOD=SAMPLE-AES,URI="skd://x",KEYFORMAT="com.apple.streamingkeydelivery"`
true; HLS `#EXT-X-KEY:METHOD=AES-128,URI="https://k/1"` false; HLS with no key false; empty and non-manifest text false.
`stream_protection`: `[]` None; one clear playlist Some(false); a clear and a DRM one Some(true); a page of ordinary JSON None.
**Live.** The Vimeo page lists renditions and a whole video lands (`kinds` audio and video); a Widevine demo page is still
refused. Then run `scripts/chrome-check.py` (69 checks) and the browser tests.

#### G74 A single file: ask first (Approval)
- **Checked 2026-11-06 (by reading, no run):** the app already does this. `app/sync.rs` `take_handed` asks for a panel click when `ask_panel_downloads` is on (default `true`, `settings.rs:614`), and `tests/window/dialogs.rs` `a_video_from_the_browsers_panel_opens_download_file_info_with_its_quality` covers it. The harness's app did not open it because `tests/window/support.rs:506` builds it with the setting off. What is left is the human-qa row and the side-by-side screenshot.
- **Today:** on a tweet with one MP4, reference A's bar opens its File Info window (URL, Category, Save As, Remember
  this path, Description, Download Later, Start Download, Cancel); dm's bar hands the file over and the app starts it.
- **Goal:** the panel's click opens dm's own Download details dialog (contract row B02) before starting, as reference A
  does, when the person's setting is "ask".
- **Change:** `browser/src/web/page.rs` `hand_chosen(.., ask: true, ..)` already asks the app; the app side must open
  Download details for a panel click when `Options > Downloads` says ask (find where `Request::Hand`'s `ask` is read in
  `desktop/crates/dm-desktop/src/app/links.rs`). Confirm first why the harness's app did not open it.
- **Done when:** a window test in `desktop/crates/dm-desktop/tests/window/dialogs.rs`; `scripts/human-qa.py` row for
  the panel click; side-by-side screenshot.

#### G75 File names from the page's title
**Seen 2026-11-02.** Reference A's Save As is the page's title (`NASA on X- 'Everybody, wave! @fireflyspace's...'`). dm names
the file from the media's own name: `GiFPAi8WYAEgiBu.mp4` (X), `video.mp4` (Facebook). (`browser/src/decide/download.rs`
`taken_name` only words the notification; the saved name is decided in the app.)

**Goal.** Name by the page's title when the file's own name says nothing; otherwise keep the file's name. Order of choice:
(1) the name in the server's `Content-Disposition`; (2) the address's name unless `is_meaningless`; (3) the page's title
made safe, plus the type's extension; (4) the address's name.

**Build.**
1. `dm-protocol/src/lib.rs` `Start` gains `#[serde(default, skip_serializing_if = "String::is_empty")] pub title: String`
   (no version bump: an older app ignores it). `Batch` gets nothing.
2. Extension: `extension/content.js` `finish()` and `choose()` add `title: document.title` to the `dm.media` message;
   `browser/src/decide/media_panel.rs` `PanelClick` gets `title: Option<String>`; `browser/src/web/page.rs` copies it into
   `Start.title`. For a download the browser caught, use the tab's title (`tabs.get(tabId).title` in `web/downloads.rs`).
3. App: `dm-core/src/queue/mod.rs` `AddOptions` gains `pub title_hint: Option<String>`; `dm-shell/src/host.rs` (where `Start`
   becomes an `AddOptions`) sets it; after the probe, where `probe.filename` becomes the row's name (search `probe.filename` in
   `dm-core/src/queue/runner.rs`), call the new function below when `save_name` is `None`.
4. New `dm-core/src/naming.rs`, `pub mod naming;` in `lib.rs`:
   ```rust
   pub fn is_meaningless(name: &str) -> bool;
   pub fn title_name(title: &str, extension: &str) -> Option<String>;
   pub fn choose(from_header: Option<&str>, from_url: &str, title: Option<&str>, extension: &str) -> String;
   ```
   `is_meaningless` (stem = the name before its last dot, case-insensitive for the word list) is true when: the stem is empty; the
   stem is one of `video audio download file playback media index stream master manifest segment`; the stem is all digits; the
   stem is hex or a UUID of 16 or more characters; or the stem is 12 or more characters with no space, dot, hyphen or underscore,
   has a letter and a digit, and 25% to 75% of its letters are upper case.
   `title_name`: trim; turn `:` into ` -`; remove control characters and `\ / * ? " < > |`; collapse runs of spaces; strip trailing
   dots and spaces; cut to 120 characters at a word end; refuse (None) an empty result or a Windows reserved name (`CON PRN AUX NUL
   COM1..COM9 LPT1..LPT9`, with any extension); append `.` + `extension`.
**Tests (`dm-core/src/naming.rs` `#[cfg(test)]`).** `is_meaningless` true: `GiFPAi8WYAEgiBu.mp4`, `o4DPxfQfIIk3KMBHYxVIEGxCCEuyk9deAAzL6j.mp4`,
`video.mp4`, `123456789.mp4`, `3f2a9c1e7b8d4a6f9c0e1d2b3a4c5d6e.mp4`, `0b6d3c1a-7e2f-4a8b-9c1d-2e3f4a5b6c7d.mp4`; false: `setup-installer.exe`, `BigBuckBunny720p.mp4`,
`Report2026_Final.pdf`, `holiday photos.zip`, `NASA.mp4`. `title_name`: `NASA on X: "Everybody, wave!" / Moon` +
`mp4` gives `NASA on X - Everybody, wave! Moon.mp4`; `CON` gives None; a 300-character title is cut at a word end under 120; an
empty or whitespace title is None. `choose`: a header name beats everything; a hash name with a title takes the title; a real
URL name beats the title; no title keeps the URL name.
**Live.** The X tweet, a Facebook reel and an Instagram reel each save under the page's title; a link to `report.pdf` keeps
`report.pdf`.

#### G76 Where the bar sits (Approval)
- **Today:** reference A's bar sits on the video's top edge, right-aligned, and shrinks to a play mark and a close mark
  on a narrow video; dm's is always 240 px wide inside the video's top right (it covers most of a 292 px reel) and on
  Instagram it sat on the site's sticky header.
- **Goal:** the bar is inside the visible part of the video, and compact on a video under 400 px wide.
- **Change:** `browser/src/decide/media_panel.rs` `panel_at` (position and size) and `browser/src/content/show.rs`
  (the visible rect: clip the video's rect by the viewport and by any fixed element over its top).
- **Done when:** unit tests for `panel_at` (narrow, partly off screen, under a header); screenshots on a reel and on
  Instagram.

#### G79 Scheduler: a schedule on each queue
- **Today:** one window of hours for every queue (`dm-core/src/schedule.rs` `Schedule`: `from`, `to`, `days`, `wake`,
  `when_done`, `retries`, `run_when_done`; dialog `dialogs/speed_schedule.rs`; runner `app/scheduled.rs`). Reference A
  lists its queues (Main, Synchronization, Download limits), each with: start at a time (once on a date, or daily on
  chosen weekdays), stop at, retries per file, open a file when done, exit the app or turn off the computer (with
  "force processes to terminate"), a Files in the queue tab, and Start now and Stop buttons.
- **Goal:** each named queue can carry its own schedule; a once-at date; Start now and Stop in the dialog.
- **Change:**
  1. `dm-core/src/queue/queues.rs`: a `schedule: Option<Schedule>` on each queue, kept in `queue/persist.rs`; `Schedule`
     gains `once_at: Option<String>` (`YYYY-MM-DD HH:MM`, local) and `force: bool`.
  2. `dm-core/src/schedule.rs`: `open_at` honours `once_at` (open from that moment, then clear it); tests beside
     the existing ones for weekdays and overnight windows.
  3. `dm-shell/src/settings.rs`: the global one becomes the default for a queue with none.
  4. `desktop/crates/dm-desktop/src/dialogs/speed_schedule.rs`: a queue list on the left, the schedule on the right,
     Files in the queue tab, Start now, Stop, New queue, Delete. Layout follows the existing dialog style.
  5. `app/scheduled.rs` `run_due` per queue.
- **Done when:** dm-core tests for `once_at` and per-queue windows; a window test
  (`tests/window/speed_schedule.rs`) that two queues keep different windows; `human-qa.py` row; side-by-side.

**Exact shapes (so the work is mechanical).**
- `dm-core/src/schedule.rs`: `Schedule` gains `#[serde(default)] pub once_at: Option<String>` (`"YYYY-MM-DD HH:MM"`, local) and
  `#[serde(default)] pub force: bool`; `Schedule::open_at(&self, weekday: u8, minute_of_day: u32)` keeps its meaning, and a new
  `open_now(&self, today: (i32, u8, u8), weekday: u8, minute_of_day: u32) -> bool` adds: with `once_at` set, closed until that
  moment, then open until `to` that day, after which `once_at` is cleared by the caller (`app/scheduled.rs`).
- `dm-shell/src/settings.rs`: `pub queue_schedules: BTreeMap<String, Schedule>` (key: the queue's name); a queue without an entry
  uses the global `schedule`. `dm-core/src/queue/queues.rs` asks `Options::schedule_for(&str) -> &Schedule`.
- `dialogs/speed_schedule.rs`: left, the queues (the Main queue and the named ones, from `queues.rs`); right, today's form
  (the hours switch, start, stop, weekdays, wake, when done, retries, run a program) plus a "Start at" date for `once_at`;
  buttons Start now, Stop, OK, Cancel. Start now calls the queue's start; Stop calls its stop.
**Tests.** `schedule.rs`: `once_at = "2026-11-03 23:00"`: at 2026-11-03 22:59 closed, at 23:00 open, at 2026-11-04 00:01 open until
`to`, at `to` closed; weekday filters still apply; an overnight window (`to` before `from`) still works; two queues in settings
with different windows each answer their own. `tests/window/speed_schedule.rs`: choose queue B, change its start, OK, reopen: A is
unchanged, B kept.

#### G80 Proxy: configuration script, per protocol, browser fallback
- **Today:** `dm-core/src/proxy.rs` `Kind`: System, Http, Https, Socks4, Socks5, with a host, port, user, password and a
  bypass list; UI in `dialogs/options/tabs.rs` (Advanced, lines ~704 to 930). Reference A adds: an automatic
  configuration script (an address), the proxy per protocol (http, https, ftp, each its own), "use the browser's proxy if a
  download handed over by the browser fails", and FTP in passive mode (dm's FTP already supports passive).
- **Goal:** the three additions, in this order: per-protocol proxy, browser fallback, configuration script.
- **Change:** (1) `Proxy` gets `per_scheme: BTreeMap<String, Proxy>`; `client_with` picks by the URL's scheme.
  (2) the extension sends the browser's proxy (`proxy.settings.get`, needs the `proxy` permission: a manifest
  change, so needs the maintainer's word) with a hand-over; dm tries it once after a failure. (3) A script: evaluate
  `FindProxyForURL` with the QuickJS dm already fetches (`dm-core/src/tools.rs`), in a sandbox with `shExpMatch`,
  `dnsDomainIs`, `isInNet`, `isPlainHostName`, `localHostOrDomainIs`; results `DIRECT`, `PROXY h:p`, `SOCKS h:p`.
- **Done when:** tests with a local proxy server for each scheme; a script test with a fixed `FindProxyForURL`;
  a refused script address fails closed (direct is not assumed).

#### G82 Sounds
- **Today:** `sound_on_complete` and `sound_on_failure` play a fixed sound (`app/sync.rs` lines ~97 to 102 call
  `platform.chime()` and `platform.alert()`). Reference A: a sound file chosen for each of Download complete, Download
  failed, Queue processing started, Queue processing stopped or finished (Browse, Play).
- **Goal:** a file per event, and the two queue events.
- **Change:** `dm-shell/src/settings.rs`: `sounds: BTreeMap<String, String>` (keys `complete`, `failed`,
  `queue_started`, `queue_done`; an empty path keeps the fixed sound); a `Platform::play(path)` beside `chime`;
  Options gets a Sounds section (a row per event: switch, path, Browse, Play); `app/sync.rs` and the queue start and
  finish hooks call it. Only `.wav` is promised (Windows `PlaySound`, macOS `afplay`, Linux `paplay` or `aplay`).
- **Done when:** a settings round-trip test; a window test that the two new events fire once.

#### G85 Download-start options (audit, done 2026-11-05)
- **Reference A's Downloads tab, item by item, against dm:**
  - Show the start dialog: present (Options › Browser, the two "ask" switches; B63).
  - "Do not start downloading, only add to the queue": present (Download details' Add for later, with the queue beside
    it).
  - Show the complete dialog: present (Options › General, `show_complete`; B63).
  - Start downloading while its File Info window is open: absent; row 85.
  - The queue chooser on Download Later and on closing batch dialogs: present in Download details; added to Download
    links on 2026-11-05 (`links-queue`, `window/dialogs.rs` `download_links_adds_for_later_to_the_queue_chosen`). The
    clipboard's links dialog adds to the Main queue (note: its links are one list a person ticks; a queue picker there
    is the same control when asked for).
  - Ignore the file's modified-time change when resuming: absent on purpose. dm adopts staged parts only under the
    validator they were fetched under and starts a changed file over (`plan.rs` `Recorded::validator`,
    `download::fetched_once_more_if_changed`): appending a new version's bytes to an old one's gave files that matched
    neither, reported complete.
  - A customizable progress dialog: partial, no row. dm's progress window can be on or off (Options › General) and
    shows a fixed set of lines (B46).
  - The User-Agent for added downloads: partial, no row. Options › Advanced › "Identify as dm only" sends dm's own name;
    off, a browser's. A typed string is not offered: the two cover what sites check, and a typed one is a way to break
    every download.
  - What to do with a duplicate: present (B17, B50, and the duplicate question's "remember my choice").

#### G86 General tab
- **Today:** dm has a clipboard watch (`app/clipboard.rs`) that offers a copied link (it does not start it), take or
  keep keys, and Add dm to Edge, Firefox, Chrome and others. Reference A: start a copied URL by itself; a list of the
  browsers it captures from, each a check box, with Add browser; edit the items it adds to the browser's right-click menu;
  edit which buttons its panel shows in the browser.
- **Goal, in order:** (1) a switch "start a copied link at once" (default off) in `app/clipboard.rs`; (2) a per-browser
  capture list in Options > Browser, backed by the native-messaging registration per browser; (3) extension options page
  for the right-click items and the panel's buttons (`extension/` options, `browser/src/web/settings.rs`).
- **Done when:** a window test for (1); `human-qa.py` row for each.

#### G87 What the browser hands over (audit, done 2026-11-05)
- **Types:** reference A catches its list (3GP 7Z AAC ACE AIF APK ARJ ASF AVI BIN BZ2 EXE GZ GZIP IMG ISO LZH M4A M4V
  MKV MOV MP3 MP4 MPA MPE MPEG MPG MSI MSU OGG OGV PDF PLJ PPS PPT QT R0* R1* RA RAR RM RMVB SEA SIT SITX TAR TIF TIFF WAV
  WMA WMV Z ZIP). dm catches every type out of the box (B37; `docs/decisions.md` 2026-10-27, "Every type is caught out
  of the box"), so each of those is caught, and more; a list chosen in Options narrows it as reference A's does. Kept:
  narrowing to reference A's list would leave in the browser what B37 says dm takes (a `.csv`, a `.json`, a `.docx`).
- **Sites never captured:** dm has its own (`browser/src/decide/excluded.rs`: OS update servers, sticker and GIF hosts,
  adverts, trackers), and the served rules add to it without a store release.
- **Addresses to skip:** present: Options › Browser, the sites dm never takes from (`catch.skip_hosts`), matched on the
  file's address, the address asked for and the page's.

#### G90 Start queue and Stop queue with a drop-down of queues
- **Today:** one Start queue and one Stop queue command (`app/menus.rs` line ~226). Reference A: each toolbar button has a
  small arrow that lists the queues; Downloads > Start queue > and Stop queue > are submenus.
- **Change:** `table.rs`/toolbar: split buttons; `app/menus.rs`: the two menu items become submenus of the queues
  (`dm-core/src/queue/queues.rs` names); keep the plain click as "the selected queue".
- **Done when:** a window test for starting a chosen queue from the arrow.

#### G91 Help and View menus (done 2026-11-05)
- **Help:** Help (F1), Tutorials, Queues and the Scheduler, Grabber help (each a part of `docs/user-guide.md`), Tip of the
  day (a dialog with eight tips), Home page, Contact support (the issues page), Tell a friend (an email written in the
  person's own mail program, sent only if they send it), Check for updates, About: reference A's items.
- **View:** reference A's tray icon options are dm's Options › General (what closing the window does); its Customize URL
  List is View › Columns; its Font is View › Text size. Nothing more to add.

#### G92 Site grabber
- **Today:** `dialogs/site_files.rs`: start page, file types, page depth, also other sites, keep the site's folders, stop
  after N minutes, only and skip paths, Find files, save a search and run it every N hours. Reference A: a step wizard
  (project name, start page, a project template, authorization with a manual login or a logout page, then levels,
  filters, a results step with check boxes), kept under a Grabber projects node in the category tree.
- **Goal, in order:** (1) size filters (minimum and maximum) in the dialog; (2) a results step: list what was found with
  check boxes, then add the ticked ones; (3) project templates (a few: pictures, documents, videos, whole site);
  (4) sign-in inside the grabber (use Options > Sign-ins for the site before crawling; already done for sites with a
  saved login, so show it); (5) a Grabber projects node listing the saved searches.
- **Change:** `dialogs/site_files.rs`, `dm-core` crawler (search `Grab` in `dm-core/src`), `app/ui.rs` category tree.
- **Done when:** a dm-core test with a local site (filters, depth, results); a window test for the results step.

#### G72a to G72d The floating list's wording (Approval, one commit each)
- **72a Title at the start of each line.** Reference A: "type of,  MP4 file, 13 sec, quality 1280p HD, 4590 kbps".
  Change: `browser/src/content/handoff.rs` `with_page_lines` and `decide::choice_line` take the page's title
  (`document.title`, trimmed to 40 characters) and put it first, followed by a comma and two spaces. Done when: unit
  test on `choice_line`; the same reel shows the same text in both lists.
- **72b "Download all" first line.** Change: `browser/src/content/handoff.rs` `list` adds a first line that hands every
  listed quality to the app's "Download all links" dialog (`dm-protocol` already carries a list of links: see
  `Request::Links`); the Part box stays above it. Done when: a window test that the dialog opens with every line.
- **72c Caption files as lines.** Reference A: "TTML file, HI subtitles (ASR)" (YouTube), "SRT file, 70 Bytes" (Facebook).
  Change: `decide/page_lines.rs` reads caption addresses (an address ending `.vtt`, `.srt`, `.ttml`, or a JSON key
  containing `caption` or `subtitle`) near the video's record and offers each as a line "SRT file" (label by
  extension); today `not_video` throws these away. Done when: unit tests for each extension; live on a captioned video.
- **72d 60 fps and HD named apart on YouTube.** Reference A: "MP4 file, quality 720p HD" and "720p HD 60 fps" are
  two lines. dm keeps one line per height. Change: dm's own YouTube reading (`dm-core/src/youtube.rs`) keeps the frame
  rate and the app's `Choice` carries it (`dm-protocol`); `decide::choice_line` writes "720p HD 60 fps". Done when: a
  dm-core test on a fixed player response with two 720p formats; side by side on the same video.

#### G94 Link grabbing: the "Download links" window (Approval for the default)
- **Reference A:** right-click, "Download selected links with ..." opens a window with every link, none checked: File Name,
  File Type, Size (it asks each server for it: `292.96 KB` for each of eight local files), Download from, Save to (the
  category's folder: `Downloads\Compressed\a.zip`), Link Text. Below it: Check All, Uncheck All, Edit; Save To (every file
  to the folder of its category / all files to one category / all files to one directory with Browse); "Use link texts as
  download descriptions" (on); "Hide HTML files"; "Hide images located on this web page" (on); "Hide repeated files".
- **dm:** `desktop/crates/dm-desktop/src/dialogs/links.rs` has File name, Type, Size, "Only these types", "Hide HTML files",
  Check all, Uncheck all, Add for later, Start now, Cancel. It opens only when `ask_panel_downloads` is on
  (`dm-shell/src/settings.rs`, default `false`); otherwise `app/sync.rs` (line ~310) queues every link at once, in the
  Main queue, waiting for Start queue. A run on this page: all eight links appeared at once as "Waiting for Start queue".
- **Goal:** the defaults ask, as reference A does, and the window gets the missing controls.
- **Change:**
  1. Default `ask_panel_downloads` and `ask_caught_downloads` to `true` (contract rows B02, B30 and B37 describe what a click
     does today: **Approval**, and the commit edits those rows). A person who turned them off keeps off.
  2. `dialogs/links.rs`: columns Save to and Link text; a Save To choice (by category, one category, one folder, with Browse);
     "Use link texts as descriptions" (puts the text in the row's note); "Hide images" and "Hide repeated files" beside
     "Hide HTML files"; an Edit button for the chosen row's name or address. `dialogs/state.rs` `LinksImport` carries them.
  3. The sizes: `open_links` already asks each address (`Found.size`); keep it bounded (8 at a time, 5 s each).
- **Done when:** a window test in `tests/window/` that opens the dialog from a page's links with the defaults, ticks two, and
  adds only those to the folder chosen; `scripts/human-qa.py` row; a side-by-side screenshot on the local page
  (`python -m http.server` with eight files, the page in `scripts/fixtures/`).

**Exact shapes.**
- `dm-protocol/src/lib.rs` `Batch` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub texts: Vec<String>`, parallel to
  `urls` (the link's visible text, trimmed to 200 characters). `extension/content.js` (the `dm.links` handler, line ~185 to 206)
  pushes `node.textContent.trim().slice(0, 200)` beside `node.href`; `browser/src/web/page.rs` passes it on.
- `dialogs/state.rs` `LinksImport` gains `texts: Vec<String>`, `save_to: SaveTo`, `hide_images: bool`, `hide_repeats: bool`,
  `use_texts: bool`, with
  ```rust
  pub enum SaveTo { ByCategory, OneCategory(String), OneFolder(String) }
  ```
  Defaults: `ByCategory`, `hide_images: true`, `hide_repeats: true`, `use_texts: true`, as reference A's.
- `dialogs/links.rs`: add columns Save to (the folder the row's category gives, or the chosen one) and Link text; the Save To
  radio group (three choices; a folder box with Browse for the third; a category list for the second); three check boxes
  "Use link texts as descriptions", "Hide images", "Hide repeated files" beside the existing "Hide HTML files"; an Edit button
  for the row chosen (its name and address). An image is a row whose `Found.kind` is Pictures; a repeat is a row whose bare
  address (`bare()`) is already listed or already a download.
- On OK, each ticked row becomes `AddOptions { save_dir, description: text if use_texts, later, queue: MAIN_QUEUE, referrer }`.
**Tests.** `tests/window/`: eight local links, none ticked on open; Check all ticks eight; "Hide images" hides the one picture;
a repeated address is hidden by "Hide repeated files"; choosing one folder and OK saves only the ticked into it; the row's note
is the link text. `dm-protocol` round-trip: a `Batch` without `texts` (an older extension) still reads.

#### G95 Engine: pieces for small files
- **Measured:** reference A, a 100 KB file: 4 connections, pieces down to about 14 KB; dm: 1 connection (a piece is at least
  128 KiB, `dm-core/src/download/ranged.rs`).
- **Goal:** a small file finishes as fast as reference A's without a flood: pieces down to 32 KiB, never more connections than
  the setting, and not under 4 pieces for a file over 256 KiB.
- **Change:** the piece-size floor in `ranged.rs` (and where it is read in `download/mod.rs`); a setting is not needed.
- **Done when:** `tests/download.rs` and `tests/speed.rs` runs on 100 KB, 256 KB and 1 MB local files: the connection
  count and the bytes match; the whole suite (`tests/limits.rs`, `tests/seam_check.rs`) still passes.

#### G96 Engine: the seam between pieces
- **Measured:** reference A starts each piece 64 bytes before its own start and ends it 64 bytes into the next; dm
  overlaps 1 byte (`spot_check_segment`, ENG-D12, `download/ranged.rs`).
- **Goal:** compare 64 bytes at each seam, so a server that changes its content between pieces is caught at the joins.
- **Change:** the overlap constant and the compare in `spot_check_segment`; on a mismatch keep today's answer
  (`job.disagreed`, restart on one connection).
- **Done when:** `tests/seam_check.rs` gains a case where bytes 10 to 60 across a seam differ (the 1-byte check passes it, the
  64-byte one fails it).

#### G97 Engine: recovery after a cut or an outage
- **Measured:** every connection cut: reference A back on all in 0.04 s, dm in 1.2 s. A server that stops for 30 s: reference A
  back within 0.1 s of its return, dm waits 1, 2, 4, 8 s and so on. (Its 205 attempts a second against a failing server is
  not copied: a short, bounded wait gives the same result.)
- **Goal:** back within about a second of a cut, and within 2 s of a server's return after any outage.
- **Change:** `dm-core/src/retry.rs` and `download/ranged.rs`: a cut of a connection that was working is retried at once
  (first retry with no wait); after the first failed retry the wait is capped at 2 s while the file had made progress; the
  attempt limit (Options > Advanced, 15) stays.
- **Done when:** `tests/wire.rs` and `tests/errors.rs`: a cut at 3 s resumes in under 1.5 s; a server back after 20 s is
  reached within 2 s; a server that never returns still ends after the limit.

#### G98 Engine cases to run on dm (audit)
- **Run both** on the local logging server (`qa/origin`): a 302 redirect, a reply with no `Content-Length`, a server that
  ignores `If-Range`, and a `Referer` check. Record each in `docs/reference-behavior.md` beside the measured table (the
  rows marked "Not run").
- **Headers:** reference A sends an old browser's User-Agent, `Accept-Encoding: identity`, `Accept-Language`,
  `Accept-Charset`, and a `Referer` of the address's own folder when none was given. Compare with `download/http.rs`
  `identify_as` and the default User-Agent; open a row for each header a real site refuses without.
- **Done when:** the "Not run" cells are filled and any difference is a row.

#### Design rule for every spec: dm's look, reference A's way of working
- **Look stays dm's.** Every new or changed dialog, menu, list and button is drawn in dm's current WinUI-style theme
  (`desktop/crates/dm-desktop/src/theme/`, the `window()` and control helpers in `dialogs/frame.rs` and `controls.rs`;
  compact spacing, dark and light). Reference A's old grey Win32 look, its icons and its colours are not copied, and no
  picture of it goes in the repo.
- **Way of working follows reference A**, so a person who knows it finds things where they expect them: the same names for the
  same things (Add URL, Scheduler, Options, Download all links, Categories, Queues, Site grabber), the same order of steps
  (Add URL, then a details window with Start, Download later and Cancel, then progress), the same shapes of choice (save by
  category, one category or one folder; check boxes with Check all and Uncheck all; per-queue start and stop), the same
  defaults that matter (it asks before it starts a browser download or a list of links).
- **What a spec may change without asking:** behaviour, defaults that are not in `docs/behaviour-contract.md`, and
  controls added to an existing dialog in its own style. **What it may not change without the maintainer's word:** a row of
  the contract, the window's layout, an existing dialog's wording or its places (the contract's rule).
- **Check for each:** the screenshot beside reference A's shows the same controls in dm's style, a person can do the same task
  in the same number of steps, and `python3 scripts/human-qa.py` still passes in full.
- **Status of this plan:** research and specs only (2026-11-02). Nothing in rows 72 to 99 is built yet except the order of a
  page's renditions in the floating list (row 72).

#### Torrent: what is left (T1 to T8), 2026-11-02
**Design rule as everywhere:** dm's look, the way of working a person expects (a torrent button opens the download client and
starts the download). Nothing here changes a row of `docs/behaviour-contract.md`.

**T1. The person's computer.**
- *Seen:* installed dm 0.8.3 at `C:\Users\prana\AppData\Local\dm`, last written 28 October, help text without "magnet"; the
  Windows magnet key absent under both `HKCU\Software\Classes\magnet` and `HKLM\SOFTWARE\Classes\magnet`; no dm extension among the
  Edge profile's extensions.
- *Done in the code (commit `f997e38`):* `dm-shell/src/register.rs` `register_magnet_windows` and `unregister_magnet_windows`,
  called from `register()` and `unregister()`: the installed app writes the key at each start unless another program owns it.
- *To do, by a person:* build and install a dm from this branch (`desktop/packaging/`), start it once (this registers), add the
  extension from `dist\chrome` in each browser used (Edge: `edge://extensions`, Load unpacked), keep "Catch downloads" on in its
  popup, allow the firewall prompt. Then run the T7 check list.
- *Done when:* `reg query HKCU\Software\Classes\magnet\shell\open\command` names the installed `dm-desktop.exe`, and a magnet
  link opened from the Run box (`Win+R`, paste the link) starts a download in dm.

**T2. `.torrent` files on Windows.** (Written once, not compiled or tested, then put aside: rebuild from this spec.)
- *Goal:* double-clicking a `.torrent` file opens dm and starts it; dm's registration never takes the extension from another
  program.
- *Change:* `dm-shell/src/register.rs`, beside the magnet functions:
  ```rust
  pub fn torrent_keys(exe: &Path) -> Vec<Vec<String>>;            // 5 `reg add` lists
  pub fn register_torrent_windows(exe: &Path, reg: impl FnMut(&[String]) -> Result<(), String>) -> Result<bool, String>;
  pub fn unregister_torrent_windows(reg: impl FnMut(&[String]) -> Result<(), String>) -> bool;
  ```
  Keys: `HKCU\Software\Classes\dm.torrent` (default `BitTorrent file`, a marker value `dm`=`1`, `shell\open\command` default
  `"<exe>" "%1"`) and `HKCU\Software\Classes\.torrent` (default `dm.torrent`, `Content Type` `application/x-bittorrent`).
  Register only when `reg query HKCU\Software\Classes\.torrent` fails (free) or `dm.torrent` carries the marker (dm's own,
  rewritten so a moved program is followed); unregister only when it carries the marker. Call both from `register()` and
  `unregister()` on Windows, best effort, as the magnet ones are.
- *Tests (`register/tests.rs`, recording `reg` closure, as the magnet ones):* five `add` lists with the command quoted;
  free extension: five adds; owned by another (query finds `.torrent`, not the marker): no add; dm's own: five adds; relative
  program refused; unregister: two `delete`s only when the marker is there.
- *Live:* with the keys written for a test build, a `.torrent` file opened from Explorer (`Start-Process file.torrent`) starts in
  dm (use a temporary `cmd /c set DM_HOME=...&& ...` command as in T6), then the keys are removed.

**T3. macOS.**
- *Goal:* a `magnet:` link and a `.torrent` file open dm.
- *Change:* `desktop/crates/dm-desktop/Cargo.toml` `[package.metadata.packager]`: `deepLinkProtocols = [{ schemes = ["magnet"],
  role = "viewer" }]` and `fileAssociations = [{ ext = ["torrent"], mimeType = "application/x-bittorrent" }]`; handle the
  app's open-URL and open-file events in the window loop (`desktop/crates/dm-desktop/src/main.rs`, where the command-line links
  are read) by passing them to the same path as a command-line link.
- *Done when:* `open "magnet:?xt=urn:btih:..."` on a Mac starts a download in dm; written without a Mac to run on, so proved by
  hand at the final QA.

**T4. Who opens magnet links: a place in Options.**
- *Goal:* Options > Browser shows "Open magnet links with: dm / another program" and, when it is another program, a button "Use
  dm for magnet links" (rewrites the key, with a line saying what changes and how to undo); a switch for the same for `.torrent`.
- *Change:* `dialogs/options/tabs.rs` (Browser page) and `dm-shell/src/register.rs`: `pub fn magnet_owner(reg) -> Owner`
  (`Dm`, `Other`, `None`) from `reg query` of the magnet key and its marker; a `force` variant of `register_magnet_windows`.
  A visible control in an existing page, in dm's own style (**Approval** for the wording and place).
- *Tests:* `magnet_owner` over the three states with a recording closure; a window test that the button shows only for `Other`.

**T5. The two Windows prompts.**
- *Firewall (row 101):* while the first torrent starts, show on its row "Windows may be asking about the firewall" if the row has
  not got a peer within 10 seconds and the program has no inbound allow rule (`netsh advfirewall firewall show rule
  name=all dir=in` read once, matched by program path). Never answer the prompt for the person.
- *"How do you want to open this?":* when a handler is registered but no default is chosen, Windows 11 may ask once. Registering
  under `HKCU\Software\RegisteredApplications` with a `Capabilities` key naming `URLAssociations\magnet` makes dm appear in
  Settings > Default apps so the person can choose it there; optional, and a change to how dm shows in Windows (**Approval**).

**T6. Automated tests for the launch paths.**
- *Extension:* a `scripts/chrome-check.py` scenario with the seven variants on a local page (hashes made up), a visible browser,
  and the app run with `--headless --control 0` so no window or firewall prompt is raised by the check itself; assert that the
  app's list holds seven rows with the seven `dn=` names. The variants are in `D:\t\site\magnets.html` as written for this
  note.
- *Windows handler:* a test marked `#[ignore]` in `dm-desktop/tests/` that writes the magnet key under a throwaway key path
  (`HKCU\Software\Classes\magnet` is the real one: so the test runs only with `DM_REGISTRY_TEST=1`, saves what was there,
  restores it after), sets the command to `cmd /c "set DM_HOME=<temp>&& "<exe>" --headless --control 0 "%1""`, calls
  `Start-Process "magnet:?xt=urn:btih:<made up>"`, and polls `list` until a row appears. This is what was done by hand on
  2026-11-02 and it passed.
- *Engine:* `dm-core/tests/torrent*.rs` already run a local swarm; add none until a defect is found.

**T7. A person's check list on the real site** (an automated browser does not pass the site's Cloudflare check, and that check is
not to be worked around):
  1. Edge with the dm extension loaded; dm from this branch installed and started once.
  2. Open a torrent's page; click "Magnet Download". Expected: dm comes forward, a row "Finding peers..." then a download.
  3. Same page in Reading view (`read://`): the magnet button, clicked. Expected: Windows opens dm through the handler.
  4. Same page in a browser without the extension. Expected: the same.
  5. A `.torrent` button. Expected: dm takes the file (contract row B37) and starts it.
  6. Write what was seen in `docs/reference-behavior.md`; a failing step becomes a row.

**T8. Not torrent, noted on the way.** The screenshot helper that took a picture of the whole screen once included the person's
own browser window; those pictures were deleted. Any later watch of a real person's session must capture the one window only.

#### EDM-V1 Easy Download Manager: Comprehensive Video Quality Grabbing and Universal Site Support Architecture
**Origin & Context (2026-11-03).**

revealed the exact mechanisms that allow reference A to capture video streams and multi-quality choices reliably across
almost every web platform (YouTube, Meta/Facebook/Instagram, Vimeo, TikTok, Twitter/X, Reddit, and generic HTML5/MSE players).

While dm's modern Rust architecture (async Tokio, memory safety, zero telemetry, sparse files, no kernel drivers) is structurally
superior to EDM's legacy 32-bit MFC and ring-0 WFP driver (`EDMwfp.sys`), dm's video sniffing is currently constrained by:
1. Hardcoded domain checks in `extension/document.js` (restricted strictly to `googlevideo.com` and `/videoplayback`).
2. Absence of `MediaSource` (`MSE`) and `URL.createObjectURL` hooking in the page's MAIN world, blinding dm to streams played
   via `blob:http...` on modern platforms (Vimeo, Twitter, Facebook, TikTok).
3. An unpopulated `rules.json` and empty site profiles engine (`browser/src/decide/sites.rs`), missing reactive API interceptors.
4. Fragmented adaptive stream handling, where split video-only and audio-only renditions on non-YouTube sites are either missed
   or offered without paired audio tracks.

**Goal.**
Implement a unified 5-stage video grabbing and quality resolution pipeline in Easy Download Manager (`dm`) that achieves 100%
site compatibility and multi-quality extraction parity with EDM, while strictly adhering to dm's pure user-mode Rust/WASM architecture.

---

##### 1. Architectural Overview & Component Topology
```
 Web Browser (Page Context)
 ┌────────────────────────────────────────────────────────────────────────┐
 │ MAIN World (extension/document.js)                                     │
 │ • Hooks window.fetch, XMLHttpRequest.open/send                         │
 │ • Hooks MediaSource.prototype.addSourceBuffer & SourceBuffer           │
 │ • Hooks URL.createObjectURL(mediaSource) -> maps blob: to stream origin│
 │ • Extracts deep framework objects (React fiber, ytcfg, __INITIAL_STATE)│
 └───────────────────────────────────┬────────────────────────────────────┘
                                     │ window.postMessage (origin-checked)
                                     ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │ ISOLATED World (extension/content.js & pkg/panel.js)                   │
 │ • Bounding-box player tracking & floating "Download video" panel       │
 │ • Relays caught stream metadata & DOM player tokens to background     │
 └───────────────────────────────────┬────────────────────────────────────┘
                                     │ ext.runtime.sendMessage
                                     ▼
 WebExtension Background (browser/src/ - WASM)
 ┌────────────────────────────────────────────────────────────────────────┐
 │ • Network Sniffer: webRequest.onHeadersReceived MIME inspection        │
 │ • Site Rule Engine (rules.json & answers.rs): API response parser      │
 │ • Stream Aggregator: matches video-only streams with best audio tracks │
 │ • Quality Hierarchy Sorter: height, bitrate, 60fps, itag mapping       │
 └───────────────────────────────────┬────────────────────────────────────┘
                                     │ Native Messaging Host (dm-shell/host.rs)
                                     ▼
 Easy Download Manager Engine (desktop/crates/dm-core)
 ┌────────────────────────────────────────────────────────────────────────┐
 │ • videopage / stream runner: parallel fetch of video + audio tracks    │
 │ • Multiplexing Engine: mp4mux / mkvmux combines tracks into final file │
 │ • Token Expiry Handler: requests fresh CDN signatures if stream drops  │
 └────────────────────────────────────────────────────────────────────────┘
```

---

##### 2. Detailed Technical Specifications (Stage 1 to Stage 5)

###### Stage 1: Main-World Interception Enhancement (`extension/document.js`)
- **Hook `MediaSource` and `URL.createObjectURL`**:
  Modern streaming players feed fragmented MP4/WebM chunks (`.m4s`, `.mp4`, `.ts`) into an MSE `SourceBuffer` and attach a
  `blob:https://...` URL to the `<video>` element. To bridge this:
  1. Hook `URL.createObjectURL`:
     ```javascript
     const origCreateObjectURL = URL.createObjectURL;
     URL.createObjectURL = function (object) {
       const url = origCreateObjectURL.call(this, object);
       if (object instanceof MediaSource) {
         window.postMessage({ source: 'dm-mediasource-created', blobUrl: url }, location.origin);
       }
       return url;
     };
     ```
  2. Hook `MediaSource.prototype.addSourceBuffer`:
     Intercept MIME type declarations (e.g. `video/mp4; codecs="avc1.640028"`, `audio/mp4; codecs="mp4a.40.2"`). Tag the MediaSource
     instance with its configured audio/video tracks.
  3. Hook `SourceBuffer.prototype.appendBuffer`:
     Inspect initialization segment headers (e.g. ISO BMFF `ftyp`, `moov`, `moof` boxes) to correlate chunks to active media players.
- **Universal Streaming URL Detector (Remove domain restriction)**:
  Replace the restrictive `googlevideo.com` check in `tell()` with universal media pattern matching:
  ```javascript
  const STREAM_URL_RE = /\.(m3u8|mpd|m4s|ts|webm|mp4|m4a|aac|f4m)(\?|#|$)|[\?&](itag|videoplayback|live)=/i;
  ```
  Relay every matching URL via `dm-media-seen` to `content.js` along with initiator information.
- **Deep Framework Object Sniffer**:
  Expose a method to query page framework state:
  - React components: traverse `__reactFiber$` or `memoizedProps` for Meta (Facebook/Instagram), TikTok, and Reddit video IDs.
  - Global state variables: read `window.ytcfg.data_`, `window.__INITIAL_STATE__`, `window.__playinfo__` (Bilibili/Vimeo).

###### Stage 2: Dynamic Site Profile System (`rules.json` & `browser/src/decide/sites.rs`)
Activate the site profile subsystem (`dm-shell/resources/rules.json` and `browser/src/decide/answers.rs`):
- **Schema in `rules.json`**:
  ```json
  {
    "profiles": [
      {
        "name": "youtube",
        "hosts": ["youtube.com", "youtu.be", "googlevideo.com"],
        "api_matches": ["/youtubei/v1/player", "/get_video_info"],
        "selectors": ["#movie_player video", ".html5-video-player video"],
        "adaptive": true
      },
      {
        "name": "meta",
        "hosts": ["facebook.com", "fb.watch", "instagram.com"],
        "api_matches": ["/api/graphql", "/graphql/query", "/api/v1/media/"],
        "selectors": ["video[src*='fbcdn.net']", "video[src^='blob:']"],
        "adaptive": true
      },
      {
        "name": "vimeo",
        "hosts": ["vimeo.com", "player.vimeo.com"],
        "api_matches": ["/video/*/config"],
        "selectors": [".vp-video video", "div.player video"],
        "adaptive": false
      },
      {
        "name": "tiktok",
        "hosts": ["tiktok.com"],
        "api_matches": ["/api/item/detail", "/api/recommend/item_list"],
        "selectors": ["video[src*='tiktokcdn']", "video[src^='blob:']"],
        "adaptive": false
      },
      {
        "name": "twitter",
        "hosts": ["x.com", "twitter.com"],
        "api_matches": ["/graphql/*/TweetDetail", "/i/api/1.1/videos/tweet/config"],
        "selectors": ["video[src*='twimg.com']", "video[src^='blob:']"],
        "adaptive": true
      },
      {
        "name": "reddit",
        "hosts": ["reddit.com", "v.redd.it"],
        "api_matches": ["/comments/*/.json", "DASHPlaylist.mpd"],
        "selectors": ["video[src*='v.redd.it']", "video[src^='blob:']"],
        "adaptive": true
      }
    ]
  }
  ```
- **Site Extractors (`browser/src/decide/answers.rs` & `sites.rs`)**:
  - `Meta (Facebook/Instagram)`: Extract `dash_manifest`, `browser_native_hd_url`, and `browser_native_sd_url` from GraphQL JSON replies.
  - `Vimeo`: Parse config JSON to extract progressive MP4 files with explicit widths/heights/bitrates and M3U8 master playlists.
  - `TikTok`: Extract watermark-free HD URLs (`bitrateInfo.PlayAddr` and `downloadAddr`).
  - `Twitter / X`: Parse video variant lists from TweetDetail JSON, ranking by bitrate.
  - `Reddit`: Link `v.redd.it/<id>/DASH_1080.mp4` (video) with `v.redd.it/<id>/DASH_audio.mp4` (audio).

###### Stage 3: Adaptive Stream Video+Audio Auto-Pairing & Multiplexing
- **Extension Data Model Update (`browser/src/decide/streams.rs`)**:
  Enhance `Stream` and `PageLine`:
  ```rust
  pub struct AdaptiveChoice {
      pub video_url: String,
      pub audio_url: Option<String>,
      pub resolution: (u32, u32),
      pub frame_rate: Option<f32>,
      pub video_codec: String,
      pub audio_codec: Option<String>,
      pub estimated_bytes: Option<u64>,
      pub label: String,
  }
  ```
- **Pairing Algorithm (`browser/src/decide/page_lines.rs`)**:
  When a DASH manifest or fragmented MSE player exposes separated video and audio representations:
  1. Group video representations by height (2160p 4K, 1440p 2K, 1080p, 720p, 480p, 360p) and frame rate (60fps vs 30fps).
  2. Select the optimal audio representation (highest bitrate AAC/M4A or Opus, matching audio language preference).
  3. Emit an `AdaptiveChoice` specifying both `video_url` and `audio_url`.
- **Multiplexing Pipeline (`desktop/crates/dm-core/src/videopage/fetch.rs`)**:
  When `AdaptiveChoice` is handed to dm:
  1. Concurrently fetch the video stream (via `ranged::run` or chunked requests) and the audio stream.
  2. Feed both into `dm_core::mp4mux::mux` (for MP4/H.264/AAC/AV1) or `dm_core::mkvmux` (for WebM/VP9/Opus).
  3. Ensure timestamps (`pts`/`dts`) are aligned from track init segments.

###### Stage 4: Multi-Quality Aggregation, Deduplication & itag Evaluation
- **YouTube `itag` Evaluation (`FUN_00457c80` Parity)**:
  Add an `itag` resolver module `desktop/crates/dm-core/src/youtube/itags.rs` and mirror in extension WASM:
  - Video itags: 137 (1080p H264), 248 (1080p VP9), 399 (1080p AV1), 299 (1080p60 H264), 303 (1080p60 VP9), 2160p (313/401/272).
  - Audio itags: 140 (128kbps AAC-LC), 251 (160kbps Opus), 250 (70kbps Opus).
  - Multiplexed itags (legacy direct): 18 (360p MP4), 22 (720p MP4).
- **Segment Filter**:
  Guarantee that individual transport stream segments (`.ts`), raw initialization slices (`init.mp4`), and byte-range chunk requests
  (`range=...`, `bytestart=...`) are filtered out of the user's floating download panel list. Only complete master streams,
  playlists, and complete file choices are displayed.
- **Quality Sorting Hierarchy**:
  Sort the panel list:
  `Resolution Height (descending) -> Frame Rate (60fps before 30fps) -> Codec Quality (AV1 > VP9 > H264) -> Audio Bitrate`.

###### Stage 5: Authenticated CDN Signature & Token Refresh
- **Pass Complete Request Context in `dm-protocol::Start`**:
  Video streaming CDNs validate requests via cookies, session tokens, and strict headers:
  - Add to `Start`: `user_agent: String`, `referer: String`, `origin: String`, `custom_headers: Vec<(String, String)>`.
  - Pass the exact headers extracted from the browser request.
- **Token Expiry Re-Capture (`Entry::fresh_link`)**:
  Signed CDN URLs (e.g. YouTube `expire=`, Facebook `oh=`, TikTok `x-expires=`, Akamai `hdnea=`) expire in 1-6 hours:
  - If a download receives HTTP 403 Forbidden, 410 Gone, or TCP stall mid-transfer:
  - Mark download status as `ExpiredSignature`.
  - Notify the extension via native host: `Request::RefreshStream { download_id, original_url, page_url }`.
  - The extension re-queries the page's current player/manifest in background and returns the updated signed URL.
  - `dm-core` calls `Queue::adopt_fresh_link` and resumes the remaining byte ranges without restarting from zero.

---

##### 3. Files to Modify & Exact Code Locations
1. `extension/document.js`:
   - Add `MediaSource` and `URL.createObjectURL` proxy wrappers.
   - Broaden `STREAM_URL_RE` to match all streaming media extensions and query keys across all hosts.
   - Add framework property extractors (`__reactFiber$`, `memoizedProps`, `ytcfg`).
2. `extension/content.js`:
   - Listen for `dm-mediasource-created` and `dm-media-seen`.
   - Bind detected `blob:` URLs back to their corresponding remote origin URLs.
3. `browser/src/decide/streams.rs`:
   - Expand `StreamKind` to include `AdaptivePair(Box<Stream>, Box<Stream>)`.
   - Implement `is_segment(&url)` filter to prevent segment pollution.
4. `browser/src/decide/answers.rs` & `sites.rs`:
   - Parse JSON and GraphQL payloads for Facebook, Instagram, Twitter/X, TikTok, Reddit, Vimeo.
5. `desktop/crates/dm-shell/resources/rules.json`:
   - Populate site matchers, selectors, and API endpoints for top 10 streaming platforms.
6. `desktop/crates/dm-protocol/src/lib.rs`:
   - Update `Start` and `Choice` structs with `audio_url`, `fps`, and `headers`.
7. `desktop/crates/dm-core/src/videopage/`:
   - Extend `fetch.rs` to download paired adaptive streams and invoke `mp4mux` / `mkvmux`.
8. `desktop/crates/dm-core/src/queue/runner.rs`:
   - Hook HTTP 403 / 410 responses to trigger the `adopt_fresh_link` workflow.

---

##### 4. Verification & Testing Plan
- **Unit Tests**:
  - `browser/src/decide/tests.rs`: Test MSE MIME parsing, adaptive stream pairing logic, segment rejection regexes.
  - `desktop/crates/dm-core/tests/videopage.rs`: Test dual-stream fetch and muxing pipeline with mock video and audio endpoints.
  - `desktop/crates/dm-core/tests/itags.rs`: Validate itag-to-resolution-and-fps mapping table against YouTube specifications.
- **Automated Integration Tests**:
  - `scripts/chrome-check.py`: Validate that loading an MSE player with `blob:https://...` raises the floating panel and populates choices.
  - `scripts/sites-check.py`: Run live checks against:
    - YouTube (1080p60 and 4K adaptive downloads with sound).
    - Vimeo (verify non-DRM streams resolve all bitrate choices).
    - Twitter/X (verify highest bitrate video variant with sound).
    - Reddit (verify `v.redd.it` DASH video and audio muxing).
    - TikTok (verify full HD video download).

#### EDM-E1 EDM: High-Performance Download Engine Optimization and Advanced Segmentation Architecture
**Origin & Context (2026-11-03).**
Empirical benchmark measurements of commercial Download manager (Reference A) against Easy Download Manager (`dm`)
recorded in `docs/reference-behavior.md` and decompiled protocol internals revealed key performance advantages in Reference A's
low-level download engine:
1. Dynamic midpoint chunk splitting: work-stealing segmentation ensures connections never sit idle while byte ranges remain.
2. Connection speed balancing: slow or stalled sockets are detected and evicted before they throttle the entire download.
3. Sub-piece threshold concurrency: small files (100 KB–1 MB) are split into up to 4 parallel streams (down to 14 KB–32 KB slices)
   rather than falling back to single-connection mode.
4. Instant socket re-connect: dropped connections are retried in < 0.1 s with progress-capped backoff.
5. Seam verification: 64-byte overlaps across piece boundaries protect against non-deterministic or cached dynamic server responses.
6. Archive tail-sniffing: querying the final 64 KB of a ZIP archive enables instantaneous central directory parsing and selective preview.
7. Multiplexing advantage: modern HTTP/2 and HTTP/3 multiplexing allows parallel stream transfers over 1–2 sockets, preventing
   server-side per-IP connection limits that hinder traditional multi-socket engines.

**Goal.**
Implement these 8 architectural optimizations in EDM's core download engine (`dm-core`) so that throughput, small-file speed,
fault tolerance, and connection agility match or exceed Reference A, while retaining dm's clean zero-telemetry, memory-safe Rust design.

---

##### 1. Detailed Engineering Specifications (Parts 1 to 8)

###### Part 1: Dynamic Midpoint Work-Stealing Segment Scheduler
- **Today in dm**: Fixed 8 MiB slices are pre-allocated upfront; when a connection completes its slice, it seeks the largest remaining piece.
- **Reference A Method**: Continuous dynamic halving (`ChunksList::FindMaxFreeChunk` + `NeedNewConnection`). Any connection that completes
  its chunk inspects all active segments, finds the segment with the largest remaining byte count (`end_offset - current_offset`),
  and cuts it in two at midpoint:
  $$\text{split\_point} = \text{current\_offset} + \left\lfloor \frac{\text{end\_offset} - \text{current\_offset}}{2} \right\rfloor$$
- **Implementation in `dm-core/src/download/ranged.rs`**:
  - Introduce an asynchronous work-stealing coordinator:
    ```rust
    pub struct SegmentState {
        pub id: usize,
        pub start: u64,
        pub current: u64,
        pub end: u64, // inclusive
        pub task_id: Option<usize>,
    }
    ```
  - When connection $A$ finishes its assigned range:
    1. Lock segment ledger and find active segment $S$ with $\max(S.\text{end} - S.\text{current})$.
    2. If $S.\text{end} - S.\text{current} \ge 64\text{ KiB}$:
       - Calculate midpoint $M$.
       - Send boundary update message `LimitBound(M)` over a Tokio `mpsc` channel to the worker running segment $S$.
       - Allocate new range $(M + 1)..=S.\text{end}$ to connection $A$.
       - Update registry/state persistence so resume tracks the split chunks accurately.

###### Part 2: Slow Connection Eviction & Rebalancing Watchdog
- **Problem**: On asymmetric networks or throttled CDN edge nodes, 7 out of 8 connections may finish rapidly while 1 stalled socket
  drags total completion time from 2 seconds to 30 seconds.
- **Implementation in `dm-core/src/download/ranged.rs` and `dm-core/src/progress.rs`**:
  - Implement rolling exponential moving average (EMA) throughput tracking per socket:
    $$\text{Rate}_t = \alpha \cdot \text{Rate}_{\text{current}} + (1 - \alpha) \cdot \text{Rate}_{t-1}, \quad \alpha = 0.3$$
  - Run watchdog check every 500 ms:
    - If connection $i$'s rate is $< 0.15 \times \text{AveragePeerRate}$ for $> 3.0\text{ s}$, or 0 bytes received for $> 3.5\text{ s}$:
      1. Issue task abort on connection $i$.
      2. Record current byte position reached.
      3. Re-queue remaining range `current_offset..=end_offset`.
      4. Spawn a fresh HTTP connection or assign the range to the highest-throughput active worker.

###### Part 3: Small-File Concurrency (Adaptive Piece Floor)
- **Today in dm**: Files under 128 KiB use 1 connection.
- **Reference A Method**: Files down to 100 KB use 4 connections with slices down to ~14 KB, completing in parallel within tens of milliseconds.
- **Implementation in `dm-core/src/download/ranged.rs`**:
  - Dynamically scale the connection count and piece size floor based on probed `Content-Length`:
    - Files $< 64\text{ KiB}$: 1 connection.
    - Files $64\text{ KiB} \le \text{Size} < 256\text{ KiB}$: up to 2 connections, minimum slice $32\text{ KiB}$.
    - Files $256\text{ KiB} \le \text{Size} < 1\text{ MiB}$: up to 4 connections, minimum slice $64\text{ KiB}$.
    - Files $\ge 1\text{ MiB}$: up to configured maximum connections (default 8, max 32), minimum slice $128\text{ KiB}$.

###### Part 4: Zero-Delay Reconnect & Outage Recovery State Machine
- **Today in dm**: Connection cuts trigger progressive backoff delays (1s, 2s, 4s, 8s, 16s).
- **Reference A Method**: Dropped active connections are retried in $0.04\text{ s}$–$0.1\text{ s}$.
- **Implementation in `dm-core/src/retry.rs`**:
  - Differentiate between connection drop types:
    1. **Established Socket Drop (Mid-stream cut)**: If connection dropped after transferring $> 32\text{ KiB}$, retry **immediately** ($0\text{ ms}$ delay) for the first attempt.
    2. **Progress-Capped Backoff**: If any other connection is actively receiving bytes, cap the retry wait of failing sibling connections at a maximum of $1.5\text{ s}$–$2.0\text{ s}$.
    3. **Total Server Outage**: If all connections fail or return 503/502/refused, use bounded backoff (1s, 2s, 4s, capped at 10s) with active health probes every 2s to resume immediately when the host returns.

###### Part 5: 64-Byte Seam Boundary Verification
- **Implementation in `dm-core/src/download/ranged.rs` (`spot_check_segment`)**:
  - Increase boundary check overlap constant:
    ```rust
    pub const SEAM_OVERLAP_BYTES: u64 = 64;
    ```
  - For piece $K$ starting at byte offset $O > 0$, issue the HTTP range request as `Range: bytes=(O - 64)-(End)`.
  - Read first 64 bytes and compare against the previously written 64 bytes on disk at $(O - 64)..O$:
    - If match: seek disk pointer to offset $O$ and proceed streaming.
    - If mismatch: set `job.disagreed = true`, immediately cancel all multi-part tasks, truncate target file to 0, and restart download on a single stream from byte 0.

###### Part 6: Zero-Copy File Writing & OS-Level Sparse Pre-Allocation
- **Contrast**: Reference A writes separate temporary part files (`.N.0`, `.N.1`) and concatenates them at 99% via `JoinFile`, doubling disk I/O.
- **dm Architecture**: Pre-allocated sparse files with direct byte-offset writes (`pwrite`).
- **Optimization in `dm-core/src/download/files.rs` and `assemble.rs`**:
  - Implement OS-native fast pre-allocation:
    - Windows: `SetFileInformationByHandle` (`FileAllocationInfo`) or `SetFileValidData` (bypassing zero-fill if running with appropriate rights).
    - Linux: `libc::fallocate(fd, 0, 0, length)`.
    - macOS: `libc::fcntl(fd, F_PREALLOCATE, &fstore)`.
  - Add 256 KiB write-behind ring buffers per connection task, avoiding micro-writes and reducing CPU syscall overhead on 10 Gbps networks.

###### Part 7: ZIP Central Directory Tail-Sniffing & Instant Archive Preview
- **Implementation in `dm-core/src/probe.rs` and `dm-core/src/extract.rs`**:
  - If URL points to `.zip` or response header indicates `application/zip`:
    1. Concurrently with piece 1, issue request for tail range:
       $$\text{bytes} = (N - 65536) \text{ to } (N - 1)$$
    2. Scan buffer backwards for End of Central Directory record signature (`0x06054b50`).
    3. Parse Central Directory headers: extract filenames, uncompressed sizes, compression methods, and local header offsets into `ZipIndex`.
    4. Expose `dm_core::zip::inspect(&file_id) -> ZipIndex` for instant UI archive preview.
    5. Allow partial extraction: user can extract a single file before download finishes by fetching only the byte range corresponding to that file's local header and compressed data.

###### Part 8: HTTP/2 & HTTP/3 Stream Multiplexing Pipeline
- **Problem**: Legacy Download manager engines open 8 to 32 raw TCP sockets via Winsock, which triggers anti-DDoS firewalls or per-IP connection throttling on CDNs (Cloudflare, Akamai, CloudFront).
- **Implementation in `dm-core/src/download/http.rs`**:
  - Configure `reqwest` / `hyper` client with ALPN negotiation:
    - If server advertises `h2` or `h3` (QUIC):
      - Open 1 or 2 underlying TCP/UDP connections.
      - Multiplex the 8–16 byte-range requests as concurrent stream IDs over the shared multiplexed session.
    - Preserves maximum multi-range throughput while appearing as a single legitimate HTTP/2 or HTTP/3 browser session.

---

##### 2. Files to Modify & Exact Code Locations
1. `desktop/crates/dm-core/src/download/ranged.rs`:
   - Implement midpoint work-stealing algorithm (`find_max_remaining_segment` and channel boundary updates).
   - Implement connection watchdog and EMA throughput calculation.
   - Expand `SEAM_OVERLAP_BYTES` to 64 and handle mismatch reset.
2. `desktop/crates/dm-core/src/retry.rs`:
   - Implement zero-delay retry on established cut and progress-capped backoff.
3. `desktop/crates/dm-core/src/download/files.rs`:
   - Add platform-specific `fallocate` / `SetFileValidData` sparse pre-allocation.
4. `desktop/crates/dm-core/src/probe.rs` & `extract.rs`:
   - Add ZIP tail-range probe and Central Directory parser.
5. `desktop/crates/dm-core/src/download/http.rs`:
   - Enable HTTP/2 and HTTP/3 connection multiplexing for multi-range requests.

---

##### 3. Verification & Testing Plan
- **Unit Tests**:
  - `desktop/crates/dm-core/tests/seam_check.rs`: Verify that 64-byte seam mismatch triggers fallback to single connection.
  - `desktop/crates/dm-core/tests/work_stealing.rs`: Verify that completing a piece divides the largest remaining piece at midpoint.
  - `desktop/crates/dm-core/tests/zip_tail.rs`: Verify instant preview parsing from mock ZIP end bytes.
- **Engine Benchmark Checks**:
  - `scripts/stress-protocols.py`: Run against local test server with simulated connection drops at 3s and 10s (verify reconnect in < 0.2s).
  - Test 100 KB and 256 KB downloads (verify 4 parallel connections spawned and piece sizes match).

#### Order and rules
Build G73, G75, G79, G80, G95, G96, G97, then G82, G85, G87, G86, G92, G90; G98 is a measurement to run first; G94 waits for the maintainer's word on the default. After each: `cargo test --workspace`, the browser tests for
G73, G75 and G76, `python3 scripts/human-qa.py` in a visible window, the side-by-side look, then update this file and
`docs/parity.md`, and delete the row when closed (git keeps its history).

### Reference A's browser extension, read from its source (2026-11-03): rows 120 to 131 (116 to 119 are built)

Read: a copy of reference A's browser extension (version 6.43.1, Manifest V3), unpacked and
de-obfuscated outside this repository, and the write-ups made with it. The write-ups and the code do
not always agree; where they differ, these rows follow the code. Its site knowledge is not in the
extension: the app sends it on connect (patterns for addresses, selector chains, property paths), and
the extension carries only the machinery. So these rows are about the machinery, and the per-site
content is dm's own `rules.json`.

**Order.** R116 to R119 are built and tested in Edge (2026-11-04; git keeps their text). What is left: R121 (a started download renewed; it needs the app to ask the extension), the measurements R122 and R129, the audit R125, R123's column and R124 once their visible parts are approved, R128, and R120, which waits for the maintainer's word.

**How its three parts work together.** A page-world script (`document.js`) hooks `fetch`, XHR
(`open`, `setRequestHeader`, `send`), the `Response` reading methods and the `body` getter. It is
switched on and given its patterns by the content script, so on a site with no rule nothing is
hooked. Each request it hooks is marked by one extra media type added to its `Accept` header
(`application/vnd.….pfm-<id>` for fetch, `.pxr-<id>` for XHR); the worker reads the mark at
`onBeforeSendHeaders`, removes it again where the browser lets it (Firefox; in Chrome the extra
`Accept` value reaches the server, harmlessly), and so ties the answer the page read to the exact
request: its headers, cookies and server address. The content script (all frames, `document_start`)
finds players, follows them with Mutation, Resize and Intersection observers, and works out the
rectangle the app draws its button on. The worker listens at `onBeforeRequest` (with
`requestBody`), `onBeforeSendHeaders` (with `requestHeaders` and `extraHeaders`),
`onHeadersReceived`, `onResponseStarted`, `onErrorOccurred`, `onCompleted` and `onBeforeRedirect`,
and decides by MIME type, extension and `Content-Disposition`.

**R120, fetch inside the page.** The reference's content script runs
`fetch(url, {method, headers, body, referrer, priority: 'high', cache: 'force-cache',
signal: AbortSignal.timeout(…)})` with an `Accept` mark the worker removes, and returns text, or bytes
as a byte string, only on 200. dm's limit: only the tab's own site, and hosts the site's rules name;
never a request the person did not start with a download.

**R121, renewing.** Uses the extension's per-URL memory of the browser's requests (built with R116, `decide/headers.rs`, and already used to hand over the newest address): when the app reports 403 or 410 for a caught
stream, the extension looks in that tab for a newer request with the same `itag` (YouTube) or the
same path with a new signature, and answers with it; else "open the page again to renew it".

**R122, YouTube.** Facts the reference's content script takes from the page: `ytcfg.set({...})` with
`EXPERIMENT_FLAGS`, `STS`, `DATASYNC_ID`, `LOGGED_IN`, `EOM_VISITOR_DATA`, `VISITOR_DATA`,
`PLAYER_JS_URL`, `SESSION_INDEX`; then it fetches `PLAYER_JS_URL` with XHR from the page, and cuts
out by pattern the signature function, the n function and the arrays they use, sending them to the
app (or, when its patterns fail, the whole script, newer versions). It also reads
`sessionStorage.getItem(<hash of DATASYNC_ID or visitor data>)`. Measure before building.

**R123 to R130.** As in the rows. For R123 the order the reference uses: links (`a`, then `area`),
images, then, for "all links", frames and every address in inline scripts; for "selected links",
plain-text addresses in the selection and in a selected `textarea`, leaving out addresses that are a
selected link's own text. Its plain-text address pattern takes a scheme, optional user and
password, a host with a dot, a port, a path that may hold single spaces before a non-scheme word,
a query and a fragment.
