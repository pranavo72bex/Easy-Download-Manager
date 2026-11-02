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
| 11 | High Contrast (`contrast.rs`, `Theme::HighContrast`) and the taskbar jump list (`jumplist.rs`, `--new-download`) are written and compile for Windows (`scripts/windows-modules-check.sh`), and the High Contrast look is checked here as pictures (`DM_HIGH_CONTRAST=black` or `white`, a debug build), but neither has run on Windows (`docs/fluent-audit.md` §3, §10, §12) | PARTIAL | On a Windows computer: switch on Aquatic, Desert, Dusk and Night sky in turn (Settings › Accessibility › Contrast themes) and check each screen reads, the focus shows and selection is clear, while dm runs; right-click dm's taskbar icon: Add URL opens its dialog in the running dm, Open downloads folder opens Explorer there; LAST: done together with the maintainer during the human QA, who guides it (maintainer, 2026-11-01) |
| 14 | Held notifications while a fullscreen app, a presentation or Focus Assist is on (`quiet.rs`, `Platform::tick`; standards §9) are written and compile for Windows (`scripts/windows-modules-check.sh`), and the holding and releasing are tested here, but `SHQueryUserNotificationState` has not been asked on a Windows computer | PARTIAL | On a Windows computer: with a video or game fullscreen, or Focus Assist on, finish a download: no toast and no sound; leave it: the toast comes; check the Windows codes 2, 3, 4 and 6 are the states seen; LAST: done together with the maintainer during the human QA, who guides it (maintainer, 2026-11-01) |
| 20 | Not covered by the stress tests: the clipboard offer (it needs the clipboard written by another program, which would overwrite the person's own), the computer sleeping (it would sleep this computer), and Windows. Covered on 2026-10-31: a full disk (`scripts/stress-protocols.py fulldisk`, macOS, a 30 MB disk image as the download folder: a 50 MB file is paused with "not enough room: the file needs 50.0 MB, and … has 29.3 MB free", two of 20 MB at once land one and hold the other with the room the first was promised, which resumes whole once there is room, no part-written file left, a 5 MB file then lands byte-exact), HTTPS through an HTTP proxy (`proxytls`: a real site's file through CONNECT, byte-exact as curl's direct copy, with and without the proxy's login, and refused with a wrong one), and a real playable DASH stream (`scripts/real-sites.py`, Shaka's Angel One: picture and sound in separate sets, 60 s, whole). FTPS, RTSPS and HTTPS redirects were already covered by tests with certificates made by `rcgen` (`ftp.rs`, `rtsp.rs`, `lib.rs`), and RTMPS is since 2026-10-31 (`rtmp/connect.rs`: carried over TLS to a server with a test root, and refused where the public roots do not vouch for the server). Linux (Debian 12, a container with no network, `scripts/linux-stress.sh`): the workspace tests and 24 of the stress phases pass with no finding; the Edge, torrent and window-with-desktop checks were not run there. Covered before: SOCKS5 proxies, and torrents on loopback inside a sandbox and once against webtorrent.io's public Creative Commons "The WIRED CD" | OPEN | Sleep needs a disposable machine; the clipboard offer a clipboard that is not a person's; Windows a Windows computer |
| 24 | Not covered by this audit: real mouse and keyboard presses (the loop drives commands and the control port, which reach the same code but not the click handling), a download held by the schedule or the download limit (the "Start now" button and its confirmation), the window in the tray or minimised (its slower wakes), several details windows open at once under a change, and Windows and Linux. Also known and left: the window reads the queue at most four times a second, so a state shown can be a quarter of a second old, and Start queue and Stop queue act on the named Main queue only, so they say "Stopped 0 downloads in Main queue" with ordinary downloads running outside it | OPEN | Each its own run of `scripts` against a built window: the schedule and limit holds first, since they have a button of their own that depends on the row's state |
| 27 | Google's Rust style guide: done 2026-11-02. `#![warn(missing_docs)]` passes clean in all four crates; every `unsafe` block has `// SAFETY:`; `undocumented_unsafe_blocks` is on; `clippy::must_use_candidate` produces zero warnings across all four crates | DONE | — |


| 30 | Brave (1.98.60, CachyOS) listed "Unsafe attempt to load URL chrome-extension://…/pkg/dm_catcher.js (and `_bg.wasm`) from frame with URL https://www.youtube.com/… Domains, protocols and ports must match", from `content.js`'s `import()`; `scripts/linux/cachyos-brave/stress.sh` on real YouTube showed 28 such lines in 40 rounds with either manifest. Changed: the background injects `pkg/panel.js` (WebAssembly written in), the page fetches nothing of the extension's, and `web_accessible_resources` is gone. Checked on CachyOS + Brave against a local page at a youtube.com address (plain, service worker, restart, Private window, 15 stress rounds: panel every time, no request to `chrome-extension://` from the page). On real YouTube in Microsoft Edge 156 on macOS (2026-10-29, `youtube-check.py`, 49 of 49) the panel loads, lists the qualities and the video arrives whole; not yet run on real YouTube in Brave: Google answered the container's Brave with its "unusual traffic" page after the earlier runs. The Docker QA suite's browser check (Chromium, 69 checks) passes with it, including "on a site whose policy forbids WebAssembly the panel stays away, quietly", which it first failed (BUGLOG.md, BUG-005) | PARTIAL | `sh scripts/linux/cachyos-brave/stress.sh` (no `LOCAL`) once Google lets the container's Brave reach YouTube again: no "Unsafe attempt" line in 40 rounds and the panel in most checks; confirm with the reporter after the next extension release |
| 37 | Some YouTube videos and signed-in accounts answer "Sign in to confirm you're not a bot" or stop the stream after about a megabyte: YouTube wants a proof-of-origin token. dm passes the page's own cookies from the extension, which covers most cases, and says the rest in words | BLOCKED | Needs a proof-of-origin token provider, which requires running YouTube's bot-detection program — upkeep the maintainer declined. Revisit if a Rust or Deno provider appears; LAST: done together with the maintainer during the human QA, who guides it (maintainer, 2026-11-01) |
| 39 | Secrets are kept in files of dm's own, readable by this account alone (0600): a row's login and its browser cookies in `secrets.json` beside the queue file, and the Sign-ins' and the proxy's passwords in the settings file (`docs/decisions.md`, 2026-10-17). Written on 2026-11-01 and not yet built or tested: on Windows the queue's `secrets.json` is sealed (ChaCha20-Poly1305) under a random key that Windows Credential Manager keeps (`secretstore.rs`); a file in clear moves into the store on its first read, and where the store is not there the file is as before and a notice says so. Also written, not built: the Sign-ins' and the proxy's passwords go to the same store under a name each and the settings file keeps them empty (`dm-shell/src/settings/secrets.rs`; a file with clear passwords is moved in on its first read; a refusing store leaves them in the file and a notice says so), and the macOS Keychain (`security`) and Linux Secret Service (`secret-tool`) backends are written as small command-line wrappers that could not be compiled or run on this Windows computer (an ad-hoc-signed dm on macOS may be asked for each item at every rebuild, and CI's Linux has no Secret Service, where the file stays as before). Found by the security test of 2026-10-29 (`docs/security-report.md`, L1) | PARTIAL | Prove the Windows store in the final build and QA (a login and cookies survive a restart; a copy of the home on another account cannot open them), then the settings passwords the same way; compile and prove the macOS and Linux backends on those systems |

| 45 | A part of a video is cut from YouTube's MP4 streams (dm's own reading), and now from HLS streams (the segments whose summed lengths overlap the span; a fragmented MP4 trimmed exactly, a transport stream joined at the nearest segments), DASH streams (template, timeline, list, or one file with an index; trimmed exactly in MP4) and plain MP4 files (the sample tables' byte ranges and the frames between, built by dm's MP4 writer). Still not cut: a WebM DASH pair is joined at the nearest pieces, not trimmed; a plain MP4 whose server gives no ranges or length is refused; the panel's Part line (contract row B59) was seen on a real YouTube page in a visible window on 2026-11-02 (the start and end typed, the part saved), and not yet on a real HLS, DASH or plain MP4 page | PARTIAL | Offline tests pass (2026-11-02: 61 stream + 23 dash + 2 mp4mux::part, all ok); left: a real HLS, DASH and plain MP4 part in the final human QA, checking the cut points by playing the file |
| 47 | A second network path (a phone's connection used beside the computer's own) is built: Options › Advanced › Network › Second network picks an interface (`dm_core::interfaces`), and every other connection of a ranged download goes out on it, bound to its address and to the interface (`second_path.rs`, `ranged::run`); the ranges go to whichever connection frees up, so the faster path takes more. Tested with this Mac's own Wi-Fi address beside the loopback (`tests/second_path.rs`: the server saw both source addresses, the file whole). Each path's share is shown since 2026-10-31: the progress window's Connections section says "{} of {} came over the second network", from which ranges a second-path connection took last (`second_path::fetched_by`, in this run; `dm-desktop/tests/second_network.rs`, a test binary of its own as the second path is the process's). Since 2026-11-01 HLS segments and DASH pieces alternate onto it too (`second_path::client_for_piece`, written, not yet built or tested). Left: it has not been run with a real phone connection; single-connection downloads use one path (nothing to split) | PARTIAL | Run once with an iPhone over USB and Wi-Fi together and record the speeds and the shares (no phone here) |
| 51 | The extension release's Mozilla signing step (`release.yml`, `web-ext@8.3.0 sign --channel=listed`, first run in CI on 2026-10-30 for 3.2.0) was refused by AMO's validator as "Unsupported file type, please upload a supported file (.crx, .xpi, .zip)", though `web-ext build` of the same `dist/firefox` makes an ordinary `dm_download_manager-<version>.zip`. The step no longer stops the release; the Firefox add-on on AMO stays at its last hand-signed version until this is fixed | OPEN | With the AMO keys: run the same `web-ext sign` by hand to see which upload it rejects (the add-on or `--upload-source-code`'s `source.zip`), try the current web-ext release, and sign 3.2.1 |
| 53 | A part or a video's sound saved as MP3. Done on macOS on 2026-10-31: dm's own Layer III encoder (`mp3.rs`) fed by the system's AAC decoder (AudioToolbox); "MP3 audio" is in every quality list, the panel's included. Done on Windows on 2026-11-01: Media Foundation's source reader gives the sound as PCM (`mp3/decode/win.rs`, `mf.rs`), offered only where the system has an AAC decoder, which a Windows "N" edition lacks until the Media Feature Pack is added. Linux: dm's own AAC-LC decoder is written (`aac/`: AudioSpecificConfig, frame syntax, sections, scale factors, pulses, mid/side, TNS, IMDCT by FFT, windows, overlap-add; the M4A read through the MP4 reader; not built or tested) but gated off: the Huffman books (121 scale-factor and eleven spectral codebooks) and the band layouts of rates other than 44.1/48 kHz were not reproducible from memory, and one wrong code word decodes to noise without an error, so `aac::decoder_ready()` is false and Linux still offers no MP3. Left: supply and check those tables in `aac/tables.rs`, set `COMPLETE`, compare a decode of `sound.f140.m4a` with a reference decode | PARTIAL | Fill in `aac/tables.rs` from the standard (maintainer to supply or approve a source), then a decode test |
| 54 | A part saved as a GIF. Done on macOS on 2026-10-31: VideoToolbox decodes the part (`frames.rs`), dm writes the GIF (`gif.rs`); Part as: Video / GIF in Download details. Done on Windows on 2026-11-01: Media Foundation's H.264 decoder fed with dm's own samples (`frames/win.rs`), so a part's edit list is kept (its own MP4 reader plays no edit list that starts after zero and stops where the movie's duration says in the media's time, which lost a cut part's last pictures); offered only where the decoder is there. Left: Linux (no decoder in the system) | PARTIAL | Linux: an H.264 decoder of dm's own is not proposed |
| 57 | Expired-address resume (built 2026-11-01, `Entry::fresh_link` and `Queue::adopt_fresh_link`: a browser row whose signed address expired waits and takes the browser's next hand-over of the same file, matched by name, exact size and family of servers) has run only in offline tests, and cannot match a row that failed at its very first request, which never learned a name and size | PARTIAL | A real-site run with a signed address that expires (a video site's CDN link) in the human QA at the end |
| 60 | Every dialog is a window of its own that can be dragged anywhere (`dialogs/apart.rs`, contract row B42, built 2026-11-01), proven on this computer with the system's mouse for every kind of dialog (each dragged wholly outside dm's window and read back from Windows as a top-level window), but the maintainer could not drag one out on their own computer on 2026-11-01: only a build made after the change shows it (`D:\dm-target\debug\dm-desktop.exe`), an older or installed dm draws dialogs inside the window. One monitor only here. `scripts/human-qa.py` B42 still fails on this computer (2026-11-02: asked for (616, 260), the dialog stayed at [576, 40, 1064, 397]); the other 60 of 61 rows hold | PARTIAL | Delete this row when the maintainer has dragged a dialog out of dm's window with that build, and on a second monitor where they have one |
| 62 | Facebook was tried on 2026-11-01 with `scripts/sites-check.py facebook` in a visible Edge with no sign-in: NASA's public video page answers "This Video Isn't Available" and a Log In wall, so no player appears and nothing could be downloaded. The transport Facebook shares with Instagram (a file asked for in pieces from `fbcdn.net`, each piece named by `bytestart` and `byteend`, picture and sound apart) is handled and checked live on Instagram (a whole 41 s video with sound) and offline in `browser/src/decide/tests.rs`; Facebook's own page has not been run | PARTIAL (2026-11-02, signed in with a test account in Microsoft Edge: the panel offers the player's own line, the page's SD and HD files and each rendition of the page's DASH manifest with its height, bitrate and length, and a whole video with sound lands in dm; checked with `SITES_SIGNIN=facebook-watch`) | A person signs in to Facebook in the browser that has dm's extension and runs `python scripts/sites-check.py facebook` (the page's session goes with the download for that download alone); LAST: done together with the maintainer during the human QA, who guides it (maintainer, 2026-11-01) |
| 63 | Reddit was tried on 2026-11-01 the same way: the listing and post pages answer an automated browser with a "Prove your humanity" challenge, and old.reddit asks for a login, so no post could be opened. Reddit's video is an HLS or DASH stream with the sound apart; the extension lists HLS playlists a page loads (checked live on X and Vimeo, whose audio renditions dm joins) | PARTIAL (2026-11-02, signed in with a test account in Microsoft Edge: Reddit's security policy forbids WebAssembly, so the full panel never loaded; a plain-JavaScript panel now takes its place and its Download video button shows on the post's video and sends it to dm. The list now also offers the post's own HLS playlist (the `src` of Reddit's player element), downloaded and joined by dm's own HLS engine: on a post with a sound track a whole video with sound landed (16 s, 854p, audio and video); on a silent post the playlist names no audio and the file has none, as it should) | A person opens a Reddit video post in a normal window of the browser with the extension and runs the check with its address (`SITES` in `scripts/sites-check.py`); LAST: done together with the maintainer during the human QA, who guides it (maintainer, 2026-11-01) |
| 64 | The panel names a stream's quality from its playlist or manifest (built 2026-11-01, `browser/src/decide/streams.rs` and `web/streams.rs`, 6 offline tests), but the playlist read runs only in a browser, so those tests do not cover it, and it sends no `Referer` (a host that wants one leaves the line as it was) | PARTIAL | See a real Vimeo and TikTok page in the final QA |
| 65 | Side-by-side of reference A and dm from the outside (`D:\compare\compare.mjs` and `compare2.mjs`, 2026-11-02; results in `docs/reference-behavior.md`). Same outcome in 8 of 12 local scenarios (file, name, bytes); 4 differ (a named or UTF-8-named download, a server that lies about length, an HTML page) and may be the harness misreading reference A's dialogs. Closed in dm 2026-11-02: a first ranged request answered with a server error is asked again without `Range` (reference A asks plain first; `probe.rs`, test `a_server_that_fails_ranged_requests_but_serves_the_plain_one_gives_the_file`). Measured, not changed: 64 MiB is cut by halves in reference A (20 connections) and in fixed 8 MiB pieces in dm; FTP is cut by halves with a LIST in reference A and in 1 MiB pieces after SIZE/MDTM/REST 0 in dm; a first download cut at 512 KB left reference A with no second request and no file (dialogs were auto-closed, so not yet explained). dm against 14 real sites 2026-11-02: 14 of 14 as expected (YouTube whole at 240p, direct files byte-identical to curl, HLS, DASH, a torrent, and two refusals with a reason). Left: resume after kill, the second-look data check, speed and time-left smoothing, SOCKS4/5 and CONNECT proxies against reference A, the finished-parts join, and the four differing scenarios | PARTIAL | At the human QA with reference A's dialogs left open: re-run the four differing scenarios and the cut one, then the left-over measurements; change dm only where the maintainer's rule applies and dm is not already better |
| 66 | Interface study of reference A against dm, 2026-11-01 (43 pictures of reference A, 14 of dm; behaviour and information only, nothing of its artwork or wording is copied), first batch of improvements, asked for by the maintainer: (a) a file-type icon on every row of the list; (b) a double-click on a running row opens its progress window, and the window's title shows the percentage; (c) Add URL offers the addresses used recently; (d) Find and Find Next in the list (Ctrl-F, F3) beside the search box; (e) the Download complete notice offers Open, Open with and Open folder | PARTIAL | Built on 2026-11-01 as contract rows B45 to B48, with unit and window tests and the wording in all thirteen catalogs: (a) a shape per file kind on each row's picture (the coloured sheet existed, the kinds differed by colour only); (b) the double-click on a running, waiting or stopped row and the percentage in the progress window's title; (c) the rule that Add URL's Recent list keeps and offers no address with a secret in its query (the list and its Clear history existed); (d) Find previous, Shift-F3 (Find, Find next and F3 existed); (e) was already there (Open, Open with… through an app picker, Open folder). Seen in a visible window on 2026-11-02: B45 to B48 all pass in `scripts/human-qa.py`. Left: the maintainer's own look |
| 68 | The mechanism for site rules served by the app is written (`dm-protocol` `Rules`, `dm-shell/resources/rules.json`, an optional newer `rules.json` in dm's home, the hello's `rules`, merged in the extension); its shipped document is empty, so no site has a rule yet. The update check also reads an optional `rules` entry of the manifest (address under dm's releases, SHA-256, size) and keeps a newer document in the home (written, not built) | OPEN | At the QA: write the first real rules from what the comparison with reference A shows, and check the extension with the app serving them; publish a `rules` entry in `latest.json` with the first real rules |
| 69 | The page hook that reads the JSON answers of a site's own requests is written (`extension/document.js`, `content.js`, `decide/answers.rs`), driven by the `sites` of row 68's rules; no site has a profile yet, so it reads nothing today. It misses answers the page fetched before `content.js` ran (document_idle) | OPEN | At the final QA, on real pages: find the sites that build their stream from an answer, write their profiles, check each stream is offered |
| 70 | Not built from the notes: a video button drawn by the app itself so page overlays cannot hide it (large: it tracks player windows and zoom; needs live testing) | OPEN | the maintainer's word. (Connection presets are written, contract row B56; per-site connection limits and the list's export/import already existed. "Copy details" on a failed row is written, row B55; their checks are at the QA) |
| 71 | Instagram and TikTok were tried on 2026-11-02 with `scripts/sites-check.py instagram tiktok x` in a visible Edge, no sign-in. Instagram: the panel was refused on a reel when a layer from another branch of the player sat over the video (now accepted when the layer and the video share a player-sized parent, `content/visible.rs`); the list reads the page's own renditions (one site-agnostic scan, `decide/page_lines.rs`, see decisions) and offers "MP4 file, 41 sec, quality 1280p HD" beside dm's own line; a whole video with sound landed. Open: Instagram's logged-out page puts its own sign-in dialog over the video after a while, and the panel stays away then (a full-page layer is refused by design), so one run in five fails (a feed such as Facebook's has its renditions in the answers of its API, which the scan now reads too: the list shows there); TikTok's page also fetches its own 2-second sign-in clip, and when that is the only file seen the panel offers it (about one run in four); TikTok's panel works and a whole video with sound (portrait 1280p) landed; its list comes from the same scan, and the clip tried names one quality, so one line is right for it, other clips list each; X works (a single file). | open |
| 72 | Floating button, quality list, matched to reference A (watched side by side 2026-11-02, `docs/reference-behavior.md`): DONE the order of a page's renditions (best first; Facebook reel and Instagram, tested). Each row below is a visible wording or layout change, so it needs the maintainer's word before it is made (`docs/behaviour-contract.md`); the check for each is the same two pages in one Edge window (`SITES_HOLD`, `DM_KEEP_OTHER_EXTENSIONS`) and a screenshot of both lists. | PARTIAL |
| 72a | The video's title at the start of every line, as reference A has ("type of,  MP4 file, 13 sec, quality 1280p HD, 4590 kbps"). dm's lines carry no title. Closes when the same reel shows the same text in both. | OPEN, needs approval |
| 72b | A "Download all" first line that hands every listed quality to dm's own "Download all links" dialog, as reference A's first line does. dm's first line is a Part box. | OPEN, needs approval |
| 72c | Caption files as lines (reference A: "TTML file, HI subtitles (ASR)" on YouTube, "SRT file, 70 Bytes" on Facebook). dm lists none; subtitles are a separate setting. Needs the page's caption addresses read by the same site-agnostic scan. | OPEN, needs approval |
| 72d | YouTube: reference A names 60 fps and HD apart ("720p HD" and "720p HD 60 fps" are two lines); dm keeps one line per height. Needs dm's own YouTube reading to keep the frame rate and offer both. | OPEN, needs approval |
| 72e | Sizes: reference A's list shows none (they are in its Download all dialog); dm's list shows them ("1080p MP4 — 252 MB"), which the maintainer asked for earlier. Kept; listed so it is not mistaken for a gap. | CLOSED as a difference |
| 72f | Reddit and TikTok were not yet compared side by side: the Reddit test profile (`D:\dm-reddit-profile`) has no reference A extension, and TikTok's page had only one quality. Needs the extension added there by hand, then the same screenshots. | OPEN |
| 73 | Vimeo: dm refuses as "Protected" a video that reference A downloads. Spec G73. | OPEN |
| 74 | A single file: reference A asks (its File Info window) before it starts; dm's panel sends at once. Spec G74. | OPEN, needs approval |
| 75 | File names: reference A names a download from the page's title; dm uses the media file's own name (a hash, or `video`). Spec G75. | OPEN |
| 76 | Where the floating bar sits, and its compact form on a narrow video. Spec G76. | OPEN |
| 77 | Pages where only one manager showed a bar (logged out). Not a defect; recorded so a later run is not misread. | NOTE |
| 78 | Desktop apps compared side by side 2026-11-02 (`docs/reference-behavior.md`). Checked in dm's code and found already done, so not gaps: a connection limit per server (`Options::host_limits`, Options > Advanced), a command run after each download (a virus checker or a script), the last folder per category (`last_folders`), the file's date from the server (`set_modified`), the list columns Last try and Description (and Referrer and Folder besides). | NOTE |
| 79 | Scheduler: a schedule on each queue, a once-at date, Start now and Stop. Spec G79. | OPEN |
| 80 | Proxy: a configuration script, per-protocol proxies, and the browser's proxy as a fallback. Spec G80. | OPEN |
| 82 | Sounds: a chosen file per event, and queue started and queue finished events (complete and failed already play a fixed sound). Spec G82. | PARTIAL |
| 84 | Dial-up / VPN connection and redial (reference A's last Options tab). Spec G84. | OPEN, maintainer to decide |
| 85 | Download-start options in reference A's Downloads tab: audit each against dm. Spec G85. | OPEN (audit) |
| 86 | General tab: a clipboard link started by itself, a per-browser capture list, editing the browser's menu items and panel buttons. Spec G86. | PARTIAL |
| 87 | What the browser hands over: default type list and site list. Audit. Spec G87. | OPEN (audit) |
| 90 | Start queue and Stop queue each with a drop-down of queues. Spec G90. | OPEN |
| 91 | Help and View menus: Tutorials, Tip of the Day, Home page, Contact support, Tell a friend, Font. Spec G91. | OPEN, needs approval |
| 92 | Site grabber: templates, size filters, sign-in inside it, a Grabber projects node, a results step. Spec G92. | OPEN |
| 93 | Order to build, rows 72 to 98: G73, G75, G79, G80, G95, G96, G97, then G82, G85, G87, G86, G92, G90; G98 is a measurement to run first. Wait for the maintainer's word before 72a to 72d, G74, G76, G84, G91 (each changes visible wording, layout or what a click does). | PLAN |
| 94 | Link grabbing: reference A's "Download All Links" window always opens with every link unchecked, its type, its size (it asks each server), its address, the folder it will save to, and its link text; dm adds every link to the queue at once unless an "ask" setting is on (both off by default), and its window lacks three of reference A's controls. Spec G94. | OPEN, needs approval for the default |
| 95 | Engine, measured 2026-11-02 (`docs/reference-behavior.md`): reference A cuts a small file into pieces down to about 14 KB (a 100 KB file on 4 connections); dm uses 1 connection under 128 KiB. Spec G95. | OPEN |
| 96 | Engine: reference A re-reads 64 bytes across each seam between pieces; dm checks 1 byte. Spec G96. | OPEN |
| 97 | Engine: after a cut or an outage reference A is back on all connections within 0.04 to 0.1 s; dm within 1.2 s, and after a long outage on a growing wait. Spec G97. | OPEN |
| 98 | Engine cases dm was not run on, reference A was: a 302 redirect (every piece asks the final address), a reply with no Content-Length, the headers sent (an old browser's User-Agent, `Accept-Encoding: identity`, `Accept-Language`, `Accept-Charset`, a `Referer` of the address's own folder when none is given). Spec G98. | OPEN (audit) |
| 99 | Extension, watched 2026-11-02 with both on one page: the right-click items match in kind (a page: dm "Download all links with dm", reference A "Download with" its own name; a link: "Download link with dm" and the same; a selection: "Download selected links with dm" and the same). Differences are rows 74, 76, 86 and 94. | NOTE |

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

#### G84 Dial-up / VPN (maintainer to decide)
- **Today:** reference A has a last Options tab: use a Windows dial-up or VPN connection, user and password, redial
  attempts and the time between them. dm has none.
- **Decision needed:** build it for Windows only (RAS: `RasDial`, `RasEnumConnections`) or leave it out as obsolete.
  If built: a settings block, an Options section on Windows, a redial loop around the queue's start.

#### G85 Download-start options (audit)
- **Reference A's Downloads tab:** show the start dialog; "do not start downloading, only add to the queue"; show the
  complete dialog; start downloading while its File Info window is open; show the queue chooser on Download Later and on
  closing batch dialogs; ignore the file's modified-time change when resuming; a customizable progress dialog; the
  User-Agent for added downloads; what to do with a duplicate (dm has it).
- **Do:** for each, open dm's Options > Downloads and Advanced and scroll; write the finding beside the item
  (present, partial, absent) in this spec; open one row per absent item.
- **Done when:** every item has a finding and a row or a note.

#### G86 General tab
- **Today:** dm has a clipboard watch (`app/clipboard.rs`) that offers a copied link (it does not start it), take or
  keep keys, and Add dm to Edge, Firefox, Chrome and others. Reference A: start a copied URL by itself; a list of the
  browsers it captures from, each a check box, with Add browser; edit the items it adds to the browser's right-click menu;
  edit which buttons its panel shows in the browser.
- **Goal, in order:** (1) a switch "start a copied link at once" (default off) in `app/clipboard.rs`; (2) a per-browser
  capture list in Options > Browser, backed by the native-messaging registration per browser; (3) extension options page
  for the right-click items and the panel's buttons (`extension/` options, `browser/src/web/settings.rs`).
- **Done when:** a window test for (1); `human-qa.py` row for each.

#### G87 What the browser hands over (audit)
- **Reference A:** a default list of file types to capture (3GP 7Z AAC ACE AIF APK ARJ ASF AVI BIN BZ2 EXE GZ GZIP IMG
  ISO LZH M4A M4V MKV MOV MP3 MP4 MPA MPE MPEG MPG MSI MSU OGG OGV PDF PLJ PPS PPT QT R0* R1* RA RAR RM RMVB SEA SIT SITX
  TAR TIF TIFF WAV WMA WMV Z ZIP), a default list of sites never captured (update servers), and a list of addresses to skip.
- **dm:** the type list is empty (all) with a size floor; the shipped rules are `desktop/crates/dm-shell/resources/rules.json`
  and `browser/src/decide/excluded.rs` (never-catch hosts).
- **Do:** compare the two lists, decide the default for a fresh install (the maintainer's word if it narrows what dm
  catches), and add an addresses-to-skip editor if absent.

#### G90 Start queue and Stop queue with a drop-down of queues
- **Today:** one Start queue and one Stop queue command (`app/menus.rs` line ~226). Reference A: each toolbar button has a
  small arrow that lists the queues; Downloads > Start queue > and Stop queue > are submenus.
- **Change:** `table.rs`/toolbar: split buttons; `app/menus.rs`: the two menu items become submenus of the queues
  (`dm-core/src/queue/queues.rs` names); keep the plain click as "the selected queue".
- **Done when:** a window test for starting a chosen queue from the arrow.

#### G91 Help and View menus (Approval)
- **Reference A's Help:** Help contents, Tutorials, Scheduler and queues, Grabber help, Tip of the Day, Home page, Contact
  support, Check for updates, About, Tell a friend. **dm's:** Help (F1), Check for updates, About. **View:** tray icon
  options, Customize URL List, Font (dm: Text size).
- **Do:** only the items the maintainer names. Each is a menu line with a link or a small dialog (`app/menus.rs`, the
  Help menu; texts in `i18n/*.json`, all 14 languages).

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

#### Order and rules
Build G73, G75, G79, G80, G95, G96, G97, then G82, G85, G87, G86, G92, G90; G98 is a measurement to run first; G94 waits for the maintainer's word on the default. After each: `cargo test --workspace`, the browser tests for
G73, G75 and G76, `python3 scripts/human-qa.py` in a visible window, the side-by-side look, then update this file and
`docs/parity.md`, and delete the row when closed (git keeps its history).
