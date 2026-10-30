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
Wikimedia) passes 4 of 4. The same day the video route's gaps found against a
real yt-dlp reading of a YouTube page were closed (request slices, renewed
addresses, per-stream headers, yt-dlp's `--http-chunk-size`, unpacked yt-dlp
builds; `docs/decisions.md`, "The video route follows what yt-dlp's reading
says"); rows 21 and 23 are what is left of them.

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

### Video without a helper, 2026-10-29 and 2026-10-30

A YouTube page is read by dm itself (`youtube.rs`: no yt-dlp run when that
reading holds), and every other video site is read by yt-dlp, which now
ships inside every installer as its pinned release's zip and is unpacked
into dm's tools folder on first use, with no network and no macOS prompt
(`docs/decisions.md`, "yt-dlp shipped as its zip, not unpacked"). The
former rows 6 (YouTube needed yt-dlp to be read) and 32 (yt-dlp had to be
fetched on the first video) are closed by these. What is left of the
video route is below: 21, 23, 34, 37, 43, 45 and 46.

| #  | Gap | Status | How it closes |
|----|-----|--------|---------------|
| 5  | The 71 reworded strings (and the 14 added on 2026-10-30 for the quality list, the part fields and the Playlist dialog) were translated into the other twelve languages by a machine pass (consistent with each catalog's own vocabulary, checked for placeholders and length) and have not been read by a native speaker | BLOCKED | One native reader per language goes through the 71 strings listed by `git log -p -- desktop/crates/dm-desktop/i18n` for the 0.7.0 commit and corrects them |
| 11 | High Contrast (`contrast.rs`, `Theme::HighContrast`) and the taskbar jump list (`jumplist.rs`, `--new-download`) are written and compile for Windows (`scripts/windows-modules-check.sh`), and the High Contrast look is checked here as pictures (`DM_HIGH_CONTRAST=black` or `white`, a debug build), but neither has run on Windows (`docs/fluent-audit.md` §3, §10, §12) | PARTIAL | On a Windows computer: switch on Aquatic, Desert, Dusk and Night sky in turn (Settings › Accessibility › Contrast themes) and check each screen reads, the focus shows and selection is clear, while dm runs; right-click dm's taskbar icon: Add URL opens its dialog in the running dm, Open downloads folder opens Explorer there |
| 14 | Held notifications while a fullscreen app, a presentation or Focus Assist is on (`quiet.rs`, `Platform::tick`; standards §9) are written and compile for Windows (`scripts/windows-modules-check.sh`), and the holding and releasing are tested here, but `SHQueryUserNotificationState` has not been asked on a Windows computer | PARTIAL | On a Windows computer: with a video or game fullscreen, or Focus Assist on, finish a download: no toast and no sound; leave it: the toast comes; check the Windows codes 2, 3, 4 and 6 are the states seen |
| 16 | Options has a line under 13 of the settings whose names are the program's own, and a Restore defaults per tab; the rest have none, and there is no separate Advanced page for connections, proxies and the user agent (§8) | PARTIAL | A line under each remaining setting (each is a string in all thirteen catalogs, which `tests/i18n.rs` enforces); the expert tabs (Network, Proxy) behind one Advanced page |
| 18 | A very long list costs memory and speed. With 20 000 rows dm holds 65 MB just after a start (3.3 KB a row) but 100-300 MB after the list has been added and read back (freed copies of the list stay resident), and small files finish at 22/s on Linux (17/s before the write gap followed the write time) because each write is still the whole 9 MB list, at most one in ten times its own duration. On macOS (release build, 2026-10-28) a queue of 2 000 small files runs at 740 files/s with a short list (131/s before a finished download woke the scheduler) and 200 small files finish at 116/s with 20 000 queued links (37/s before); the Linux figure was measured before that and has not been taken again. Adding 20 000 links takes 4 s, a restart lists them after 1-2 s, cancelling 2 000 takes 0.3 s | PARTIAL | A journal of changes beside a snapshot, and a smaller row (the cookies, paths and strings every row carries are mostly empty); `stress-check.py bigqueue` measures it. A kill within the write gap (150 ms to 5 s) of a change can lose that change |
| 20 | Not covered by the stress tests: the clipboard offer (it needs the clipboard written by another program), a full disk and the computer sleeping (both would harm this computer), the real DASH download of a playable stream (only hostile manifests were tried), HTTPS through a proxy (CONNECT), FTPS and every other use of TLS (no certificate could be made without another program), and Windows. Linux (Debian 12, a container with no network, `scripts/linux-stress.sh`): the workspace tests and 24 of the stress phases pass with no finding; the Edge, torrent and window-with-desktop checks were not run there. Covered since: SOCKS5 proxies, and torrents on loopback inside a sandbox and once against webtorrent.io's public Creative Commons "The WIRED CD" (54 MB, 18 files, 21 s, paused and resumed, nothing outside the folder) | OPEN | Each needs its own phase in `scripts/stress-protocols.py`; a full disk and sleep need a disposable volume or machine |
| 21 | Streams in pieces (`m3u8_native`) always go to yt-dlp, though dm has its own HLS downloader (`download/hls.rs`). The design is written (`docs/decisions.md`, "Routing `m3u8_native` to dm's own HLS") but not built, because yt-dlp already fetches fragments concurrently and the gain is unmeasured | OPEN | Record a run on an HLS site and time yt-dlp with `--concurrent-fragments 8` against dm's `download_stream` on the same playlist; build the route only if dm is clearly faster, then prove the picture-and-sound join on real streams |
| 23 | yt-dlp's unpacked builds are fetched and unpacked on Windows by the `zip` crate (`tools::unpack_portable`), checked on macOS and in unit tests and timed on Linux (0.59 to 0.24 s a start), but not run on Windows, where the file names inside `yt-dlp_win*.zip` (`yt-dlp.exe`, `_internal/`) come from reading the archive on macOS, and Windows security software may scan the many small files once | PARTIAL | On a Windows computer with no yt-dlp: add a video link; dm fetches `yt-dlp-windows/yt-dlp.exe`, a page reads and downloads, an earlier one-file `yt-dlp.exe` in dm's tools folder is gone; time `--version` before and after |
| 24 | Not covered by this audit: real mouse and keyboard presses (the loop drives commands and the control port, which reach the same code but not the click handling), a download held by the schedule or the download limit (the "Start now" button and its confirmation), the window in the tray or minimised (its slower wakes), several details windows open at once under a change, and Windows and Linux. Also known and left: the window reads the queue at most four times a second, so a state shown can be a quarter of a second old, and Start queue and Stop queue act on the named Main queue only, so they say "Stopped 0 downloads in Main queue" with ordinary downloads running outside it | OPEN | Each its own run of `scripts` against a built window: the schedule and limit holds first, since they have a button of their own that depends on the row's state |
| 25 | What a frame still does with a long list (5000 rows, 20 downloads moving; the window's thread is busy 5-6 % of the time, from 17-19 % at the start of 2026-10-28): each read of the list (four a second while anything moves) copies every row from the queue (`Queue::list`) and `Backend::queues` still reads them all under a lock, and every row's kind, filter text and name are worked out again for a row that changed. Memory is 190 MB at 5000 rows (about 15 KB a row), which is row 18's | OPEN | A read that returns only what changed (a counter on the queue and a list of changed rows) rather than every row; the queue names kept as the rows are added and removed. Measure as before: sample the window's thread under the same load (`sample`, a symbolized release build) and compare the busy time |
| 26 | `cargo tree --duplicates` lists 97 crates present twice, some from our own manifests: `axum` 0.7 in `dm-core` and `dm-desktop` beside another dependency's 0.8, `dirs` 6 in `dm-desktop` beside 7. Not measured for build time or binary size, and an `axum` bump changes route syntax | OPEN | Align the versions the manifests choose where the crates allow it, then measure clean build time and release binary size before and after, and keep the change only if one of them improves |
| 27 | Google's Rust style guide (`docs/decisions.md`, "Google's Rust style guide, what was taken and what was not") has parts not done: the Windows-only `unsafe` blocks have no `// SAFETY:` comment (the macOS and test ones do, and the lint that keeps them so is off on Windows): `drops.rs` has the most (its Windows half, about 60 uses), then `taskbar.rs`, `wake.rs`, `jumplist.rs`, `power.rs`; about 1100 public fields, variants and methods have no doc comment (`missing_docs`: `dm-desktop` 616, `dm-core` 379, `dm-shell` 95; `dm-protocol`'s are written and `#![warn(missing_docs)]` holds it there since 2026-10-29); 375 functions and methods that return a value nobody should ignore lack `#[must_use]` | OPEN | On a Windows computer or one with its toolchain, write each Windows block's `SAFETY:` and drop the `not(windows)` from the lint; then `dm-shell` (the smallest left), each field and variant with what it means, not its name again, and `#![warn(missing_docs)]` there, as `dm-protocol` has; `#[must_use]` on the pure functions `clippy::must_use_candidate` lists, crate by crate, only where ignoring the result is a bug |
| 28 | Microsoft's Pragmatic Rust Guidelines (`docs/decisions.md`, "Microsoft's Pragmatic Rust Guidelines") leave two things: `dm_core`'s public items are reachable by two paths (`dm_core::category::Category` and `dm_core::Category`, about twenty items, `M-SINGLE-ITEM-PATH`), and `unsafe impl Sync for DropTarget` in the Windows half of `drops.rs` has no written argument for why it is sound (`M-UNSOUND`: a bypass of `Sync` needs one) | OPEN | Decide the one path for each item and change its users (a public-path change, so yours to ask for); on a Windows computer, write why `DropTarget` is `Sync` (what it holds and who touches it) next to the `impl`, or take the `impl` away if it is not |
| 29 | A report from CachyOS with the AppImage and Brave: "dm could not get the video from this page". Second report, after 0.8.4: "(<no Python frame>)", reproduced with CachyOS's own yt-dlp (`pacman -S yt-dlp`, scenario `system-ytdlp`): the AppImage's `PYTHONHOME` reached the system's Python; fixed in the tree (unreleased), checked by a patched binary in the 0.8.4 AppImage completing where 0.8.4 fails. Reproduced on CachyOS with Brave (`scripts/linux/cachyos-brave/run.sh`): a `noexec` `/tmp` stops the single-file yt-dlp that dm 0.8.3 fetches from starting, and the status said every yt-dlp failure it had no words for as that one sentence. The unpacked yt-dlp in the unreleased tree starts under it, and the status now carries yt-dlp's own words. Whether it is this person's cause is not known: nobody has asked them for their `/tmp`'s mount options | PARTIAL | Released as 0.8.4 and checked on CachyOS + Brave with the released build's AppImage: noexec /tmp and a single-file yt-dlp both complete where 0.8.3 failed (the unpacked yt-dlp, and a temporary folder of dm's own for any yt-dlp when `/tmp` is `noexec`); confirm with the reporter after they update; ask the reporter for `mount \| grep " /tmp "` and the words in brackets after the sentence, and tell them for now `mkdir -p ~/tmp && TMPDIR=$HOME/tmp ./dm_….AppImage`; if it is not `noexec`, run `run.sh` against what they report |
| 30 | Brave (1.98.60, CachyOS) listed "Unsafe attempt to load URL chrome-extension://…/pkg/dm_catcher.js (and `_bg.wasm`) from frame with URL https://www.youtube.com/… Domains, protocols and ports must match", from `content.js`'s `import()`; `scripts/linux/cachyos-brave/stress.sh` on real YouTube showed 28 such lines in 40 rounds with either manifest. Changed: the background injects `pkg/panel.js` (WebAssembly written in), the page fetches nothing of the extension's, and `web_accessible_resources` is gone. Checked on CachyOS + Brave against a local page at a youtube.com address (plain, service worker, restart, Private window, 15 stress rounds: panel every time, no request to `chrome-extension://` from the page). On real YouTube in Microsoft Edge 156 on macOS (2026-10-29, `youtube-check.py`, 49 of 49) the panel loads, lists the qualities and the video arrives whole; not yet run on real YouTube in Brave: Google answered the container's Brave with its "unusual traffic" page after the earlier runs. The Docker QA suite's browser check (Chromium, 69 checks) passes with it, including "on a site whose policy forbids WebAssembly the panel stays away, quietly", which it first failed (BUGLOG.md, BUG-005) | PARTIAL | `sh scripts/linux/cachyos-brave/stress.sh` (no `LOCAL`) once Google lets the container's Brave reach YouTube again: no "Unsafe attempt" line in 40 rounds and the panel in most checks; confirm with the reporter after the next extension release |
| 34 | yt-dlp runs the site's player script (in the JavaScript runtime dm hands it) with the whole computer open to it: browser profiles, SSH and GPG keys, keyrings, every file. Reference B runs its yt-dlp in a bubblewrap sandbox on Linux. A sandbox was built and tested on 2026-10-29 (`ytdlp/sandbox.rs`: root read-only, its staging and the temporary folders writable, those folders hidden, plain yt-dlp wherever `bwrap` is missing or refused; the yt-dlp tests passed in Docker both with and without it) and set aside by the maintainer | OPEN | Put that module back behind the maintainer's approval; then macOS (`sandbox-exec` with a profile denying `~/Library/Application Support/*` of the browsers and `~/.ssh`; deprecated but present on every macOS) and Windows (a restricted token or AppContainer for the child process). Each with the same test: a hidden file reads empty, the staging folder writes, a download completes |
| 37 | Some YouTube videos and signed-in accounts answer "Sign in to confirm you're not a bot" or stop the stream after about a megabyte: YouTube wants a proof-of-origin token, which yt-dlp gets from a separate provider dm does not ship. dm passes the page's own cookies from the extension, which covers most cases, and says the rest in words. Looked into on 2026-10-29: yt-dlp's recommended provider runs YouTube's bot-detection program in Node.js (not in the Deno or QuickJS dm fetches), as a server or a script of its own, and is itself changed whenever that program changes | BLOCKED | Needs Node.js fetched beside the other tools (about 50 MB) and a third-party provider kept current with YouTube's bot program: upkeep the maintainer declined for a YouTube client, declined here on the same ground for now. Revisit if yt-dlp takes token-making into itself (its `yt-dlp-ejs` already runs the player challenges) or a Deno-run provider appears |
| 39 | Secrets are kept in files of dm's own, readable by this account alone (0600): since 2026-10-30 a row's login and its browser cookies in `secrets.json` beside the queue file (never in `queue.json`, `queue.rs` `a_login_and_cookies_stay_out_of_the_queue_file_and_come_back`), and the Sign-ins' and the proxy's passwords in the settings file (`docs/decisions.md`, 2026-10-17). A copy of the whole home carries them all in clear. Found by the security test of 2026-10-29 (`docs/security-report.md`, L1) | PARTIAL | The operating system's keychain for all three (macOS Keychain, Windows Credential Manager, the Secret Service on Linux), the file as now where there is none. Waits for a signed macOS build: an unsigned or ad-hoc-signed dm is asked by macOS for each item at every rebuild, which the QA loops and `scripts/human-qa.py` cannot answer; and a Secret Service on CI's Linux (`docs/decisions.md`, "A row's secrets go beside the queue file, not yet in the keychain") |
| 45 | A part of a video is cut only where the streams carry an index of pieces and dm fetches them itself: YouTube's MP4 streams (dm's own reading and yt-dlp's). Other sites read by yt-dlp whose files are fragmented MP4 would work if their reading named the init and index (`clip::locate` finds them in the first 256 KiB where it does not); WebM, HLS and DASH streams in pieces, and plain MP4s cannot be cut this way; from the browser's panel a part is reached through Download details (the panel's click opens it, Part is a row of it), not from the panel's own list | PARTIAL | A "part" line in the panel's list (a span in `Start`, through `host::Request`, the dialog's fields filled); for DASH and HLS, the pieces by the manifest's own timing (`dash.rs`, `stream.rs`); for a plain MP4, the sample table's byte ranges for the span |
| 47 | A download uses one network path; the other macOS manager's "Turbo Mode" adds an iPhone's connection (USB or hotspot) beside the Mac's Wi-Fi and splits the ranges across both | OPEN | A second `reqwest::Client` bound to the second interface's address (`ClientBuilder::local_address`, or `IP_BOUND_IF` by socket2 where binding alone does not route), the ranges shared by each path's measured speed, the interfaces listed in Options › Network with each one's share shown in the progress window; tested here with lo0 and en0 against a server bound to 0.0.0.0 whose log shows two source addresses |
| 48 | A finished file cannot be given a time to be deleted after (the other macOS manager's Auto-Delete: after 10 minutes, an hour, a day); Clear finished removes rows, never files | OPEN | A per-row "delete the file after" (an `Entry` field and a sweep on the queue's tick), offered in Download details' menu and the row's menu, said in the row's status until then |
| 49 | Download details says which browser a caught download came from by the host's parent process (`host::parent_browser`), asked with `ps` on macOS and Linux; on Windows it says nothing | OPEN | The parent process's name on Windows (`CreateToolhelp32Snapshot`, or the `sysinfo` crate if it is already in the tree), the same words |
| 51 | The extension release's Mozilla signing step (`release.yml`, `web-ext@8.3.0 sign --channel=listed`, first run in CI on 2026-10-30 for 3.2.0) was refused by AMO's validator as "Unsupported file type, please upload a supported file (.crx, .xpi, .zip)", though `web-ext build` of the same `dist/firefox` makes an ordinary `dm_download_manager-<version>.zip`. The step no longer stops the release; the Firefox add-on on AMO stays at its last hand-signed version until this is fixed | OPEN | With the AMO keys: run the same `web-ext sign` by hand to see which upload it rejects (the add-on or `--upload-source-code`'s `source.zip`), try the current web-ext release, and sign 3.2.1 |
