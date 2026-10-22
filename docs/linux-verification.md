# Linux Verification Findings

**Status, 2026-10-15: Linux core and browser flows pass on this host; Linux release readiness is still partial.** W3Schools now downloads successfully on this network (direct and through its live player); see the latest assessment below. Packaging breadth and real tray/input verification remain incomplete for the newest revision.

Date: 2026-10-14
Scope: report-only; no product or test-harness files were changed.

## Summary

The Linux desktop client successfully downloaded a live YouTube video through yt-dlp, plus direct files from Internet Archive and GitHub. The extension successfully handed downloads and pages to the desktop client in Chromium simulations. The live YouTube floating-panel trigger did not appear during the Linux browser simulation, although the extension toolbar handoff on that same live page reached dm and the client completed the video.

## Issues Observed

### 1. Live YouTube floating panel did not appear

**Severity:** User-visible extension integration issue. **Resolved:** the later live run below passed on YouTube (35/35), and the panel is driven with real input on Debian, Fedora and Arch Chromium (`scripts/linux-chromium-human.sh`).

**Reproduction:** Ran the live YouTube check in Chromium with the Linux yt-dlp executable on `PATH` and a temporary in-memory guard waiting for the local test fixture DOM. The extension loaded, its local fixture suite passed, and the live YouTube page and player loaded. The simulated pointer hover did not show the panel: `YouTube: hovering the playing player shows 'Download this video'` failed with `{}`. The panel click, quality list, and panel handoff therefore were not reached on that live YouTube run.

In the same run, the extension toolbar button handed the YouTube page to dm and the client completed the video as `a video (1) [video-1].mp4`. Separately, `scripts/real-sites.py` fetched YouTube's `Me at the zoo` video through the Linux client as a playable 19-second, 240p WebM. This points to the extension's live-site floating-panel trigger, not a general client yt-dlp failure.

### 2. Chromium integration test has a fixture-load race

**Severity:** Test-harness issue, not a product failure. **Resolved:** `chrome-check.py`'s `loaded()` now waits for the page's own address and the link itself before clicking.

The unmodified `scripts/chrome-check.py` failed twice at `click(..., "kept.zip")` with `TypeError: Cannot read properties of null (reading 'click')`. Its click helper checks `document.readyState`, which can already be `complete` for the initial blank page before `/page.html` has populated the expected link. At that point the extension/client caught-download path had already passed, including a byte-exact file in dm's temporary folder.

With an in-memory-only wait for the expected link element, the entire Chromium integration suite passed **35/35**. No repository test script was edited.

### 3. Optional Linux tray library missing in this environment

**Severity:** Environment/runtime warning. **Resolved:** dm's tray now speaks StatusNotifierItem over D-Bus itself (see the tray follow-up below) and needs no AppIndicator library.

`scripts/app-check.py` passed its client handoff, downloads, HLS, yt-dlp, speed limit, schedule, proxy, control protocol and MCP checks. During its launches, stderr repeatedly reported a panic in the tray thread because neither `libayatana-appindicator3` nor `libappindicator3` was installed. The tests continued and passed, but this environment did not verify tray availability or tray interaction. This should not be confused with a failed download.

## Verification Evidence

- `SITES=youtube,reddit,archive,github python3 scripts/real-sites.py`: **4/4 expected**. YouTube produced a playable video; Internet Archive and GitHub files were byte-identical to curl; anonymous Reddit was refused with a reason and no fake completed file.
- `python3 scripts/real-sites.py`: **9/9 expected**, including Wikimedia image, HLS, BitTorrent, and sign-in-restricted Vimeo. All temporary download data was removed.
- Chromium extension plus dm, using a temporary fixture-readiness guard: **35/35 passed**. Verified extension startup/WASM/listeners; caught download byte-exact in dm; referer; browser copy removed; settings and cookie refusal; menu-handler handoff; multi-link queue; local floating panel and picked quality; toolbar handoff; and no-host browser fallback.
- `python3 scripts/app-check.py`: all reported functional checks passed. The normal-window and hidden-window count checks were explicitly not checked in the current display environment.
- Live YouTube panel check: panel was **not shown** after hover. The toolbar-to-dm video handoff completed in that run.
- Installed Mozilla Firefox 158.0 under `~/.local/opt/firefox`; `FIREFOX="$HOME/.local/opt/firefox/firefox" python3 scripts/firefox-check.py native`: extension accepted with no warnings, WASM handlers loaded, caught navigation download landed in dm, browser download list was empty afterward, "Download link with dm" landed in dm, and the local floating video panel showed and handed `clip.mp4` to dm.

## Coverage Not Run

The Firefox test above launched the actual locally installed Firefox binary, but the test used its headless mode and programmatic browser events. A literal physical right-click/menu selection was not run: this session is Wayland and has no `wtype`, `ydotool`, or `xdotool`; Xvfb and Docker are also absent. The Firefox extension was installed temporarily in the isolated test profile, not into a persistent everyday Firefox profile. No Reddit download success is claimed: anonymous Reddit was explicitly refused by policy/tool support.

## Change Attribution

The fixes described above came from the latest pulled commit, not from this verification pass. During verification I did not edit product code or test scripts; the earlier Chromium fixture wait was applied only in memory.

## Resolution (2026-10-14)

1. **YouTube panel on Linux — not a product fault.** Debian's Chromium does
   not autoplay the video, and dm's panel is offered once a video plays, by
   design. The check assumed autoplay and hovered a paused
   player. `youtube-check.py` now presses play first; its initial Linux
   verification was 31/31. The fresh post-pull run below passed 34/34 with
   the panel, three qualities, and the picked MP4 landing in dm.
2. **Chromium check race — fixed.** `chrome-check.py` and `youtube-check.py`
   wait for the page by its address and for the element they need, asking
   again across the navigation.
3. **Tray library missing — fixed.** dm checks for the library before the
   tray crate loads it, so nothing panics; the window's banner says what to
   install, and stderr is empty.

Also changed: the toolbar button no longer hands the page to dm (that is
what downloaded the YouTube video in issue 1's run). It brings dm's window
up; "Download this page with dm" is gone.

Found while verifying: under trayer in the container, dm's tray menu
sometimes did not open (gap 21 in `docs/gaps.md`; closed 2026-10-15: the
test's synthetic Alt+F4 left the window manager holding the keyboard).

## Fresh Verification After Pull (2026-10-14)

Rebuilt the extension with `sh scripts/package.sh` and reran the updated checks on Linux:

- `PATH="$HOME/.local/bin:$PATH" CHROME_PATH=/usr/bin/chromium python3 scripts/youtube-check.py`: **34/34 passed** against live YouTube. The browser check pressed Play because autoplay was blocked, hovered the playing video, kept the panel visible while the pointer rested, listed three qualities, clicked the MP4 choice, and verified that the MP4 landed in dm's temporary download folder. Popup, catch toggle, no-host fallback, and local extension/client checks also passed.
- `FIREFOX="$HOME/.local/opt/firefox/firefox" python3 scripts/firefox-check.py native`: Firefox 158 accepted the rebuilt extension with no warnings; Wasm handlers loaded; browser navigation and "Download link with dm" reached dm; the local video panel handed off its file; Firefox's observed HLS response was subsequently recorded and handed to dm. Its immediate poll timed out at five seconds before a later poll confirmed the stream.
- `python3 scripts/app-check.py`: the first run had one failure in the 256 KB/s timing threshold; an immediate repeat passed all checks, including the speed limit, yt-dlp handoff and HLS. Neither run logged the prior missing-AppIndicator panic. Normal/hidden window-count checks remain unchecked in this display setup.

These browser actions are user-style interactions dispatched through browser automation, not physical mouse input. The Linux session is Wayland and still has no `wtype`, `ydotool` or `xdotool`, so literal physical pointer/menu interaction was not verified. The Firefox extension was loaded into the harness's temporary profile, not installed persistently into a daily-use profile.

## Multi-Site / Multi-Browser Follow-Up (2026-10-14)

- Chromium's current full integration run, `PATH="$HOME/.local/bin:$PATH" CHROME_PATH=/usr/bin/chromium python3 scripts/chrome-check.py`, passed **44/44** against loopback fixtures. It verified byte-exact handoff to dm, panel file download, yt-dlp quality selection, panel discovery/download of HLS streams, popup controls, and fallback behavior. These are browser integration simulations, not live-site panel tests.
- Firefox 158's `FIREFOX="$HOME/.local/opt/firefox/firefox" python3 scripts/firefox-check.py native` passed navigation capture, link-menu handler, local floating-panel file handoff, and HLS response detection/handoff. Firefox does not currently have a live YouTube page run recorded here; its panel/stream checks used local fixtures and browser automation.
- The live YouTube Chromium panel test showed the panel, listed three qualities, accepted a selected MP4 and displayed "Sent to dm", but the downloaded file did not appear. The retained queue row shows `failed`: YouTube/yt-dlp was redirected to `www.google.com/sorry` and received HTTP 429 for unusual traffic. Thus the browser control path worked, but the actual selected video did **not** finish in this run. This is distinct from the separate direct-client YouTube check below, which did complete.
- A rerun of the panel test against Internet Archive's live video page did not find a `<video>` element, so it could not exercise the floating panel. The site's direct MP4 does download successfully through dm as recorded below.
- `PATH="$HOME/.local/bin:$PATH" SITES=youtube,archive,github,wikimedia,hls,torrent python3 scripts/real-sites.py`: **6/6 expected**. YouTube produced a playable 19-second WebM; Internet Archive and GitHub matched curl byte-for-byte; Wikimedia produced a JPEG; the HLS stream and Sintel torrent produced playable video files. The script removed its 212.9 MB temporary tree.
- A preceding all-sites batch returned **8/9 expected**: only OVH's comparison was inconclusive because curl failed with `Recv failure: Connection reset by peer`; dm marked its download completed, but that attempt did not establish byte identity. The isolated rerun excluded OVH and passed the six listed sites. Reddit and Vimeo outcomes remain expected refusals for anonymous/sign-in-required access, not successful downloads.

No physical input was used: the Linux session is Wayland without an input helper. Chromium's live YouTube test and both browsers' extension workflows used automated page input; Firefox's real-site floating-panel scenario remains unverified.

## Resolution of the follow-up findings (2026-10-14)

- **Live YouTube MP4 did not finish (HTTP 429).** YouTube rate-limited that
  network and sent yt-dlp to Google's "sorry" page; no download manager can
  get past that from the same network, and dm does not try. What dm did
  wrong was the message: the row now says "the site is limiting downloads
  from this network right now (HTTP 429); try again later" instead of
  yt-dlp's raw error, and sign-in-only, private, removed, bot-check and
  unknown-site refusals are worded the same way (`ytdlp.rs` `refusal`).
- **Live panel on Firefox and on another site.** New `scripts/live-panel-check.py`
  (Chromium) and `DM_LIVE_PAGE=<url> sh scripts/linux-firefox-check.sh`
  (Firefox), run on W3Schools' HTML5 video page and Wikimedia Commons' Big
  Buck Bunny page: in both browsers the panel shows on the real player and
  the video lands in dm. Chromium 30/30 and 31/31 (on Wikimedia it listed
  five real qualities and the 240p WebM landed). This found a real bug:
  W3Schools refuses any user agent containing "github", which dm's has;
  dm now asks a site that refuses it once more as plain `dm/<version>`.
- **OVH comparison inconclusive.** curl's own copy had been reset, so
  nothing was compared; `real-sites.py` now retries curl. OVH rerun:
  byte-identical.

Still not physical input: these runs drive the browsers through automation
(Chromium) and dispatched events (Firefox), in Xvfb containers.

## Latest Retest (2026-10-14, commit `67119f2`)

Rebuilt the extension with `sh scripts/package.sh` and tested the updated client and panel flows:

- **Live YouTube panel in Linux Chromium: PASS, 35/35.** Play was pressed, the panel appeared and remained visible, three qualities were listed, the selected MP4 landed in dm, and a standard decoder confirmed it plays for 19.1 seconds at 240p.
- **Live Wikimedia panel in Linux Chromium: PASS, 32/32.** The extension found the actual embedded player, listed five qualities, selected the smallest, and dm saved a playable 634.6-second 240p WebM.
- **Live Wikimedia panel in Firefox 158: PASS.** The real Firefox page showed the panel and five qualities; the selected video landed in dm. Navigation capture, link-menu handler, local panel, and HLS stream detection also passed.
- **W3Schools panel behavior: panel appears and handoff is sent, but download FAILS.** Chromium's live-panel check reported 29/31: dm's row failed with HTTP 403 for `mov_bbb.mp4`, so no file was available to play. Firefox's live-page check likewise showed the panel and sent the MP4 URL, but no video landed. The direct-client `SITES=w3schools` check also failed with HTTP 403. The earlier agent fallback did not overcome the current live server denial; do not consider this site-specific finding resolved on this network.
- **Direct client live sites: PASS, 6/6.** YouTube, Internet Archive, GitHub, Wikimedia image, HLS, and BitTorrent all produced the expected result; playable streams were checked with a standard decoder, and direct files were compared with curl. All 213 MB of test output was removed.
- **YouTube 429 user-facing wording: unit test PASS.** `cd desktop && cargo test -p dm-core a_sites_refusal_is_said_in_words_a_person_can_act_on` passed (1 test). This tests the refusal translation; the successful live YouTube panel run did not encounter a 429.

The previous 429 observation was a transient Google rate-limit response and did not reproduce in the latest live panel run. A current 403 from W3Schools does reproduce through both browsers and the direct client. This report therefore has one remaining live-site limitation rather than all findings being resolved.

These runs use automated browser input, not physical mouse/keyboard input. Firefox used the installed Firefox 158 binary and an isolated temporary profile. W3Schools' 403 is the site response observed on this network; no attempt was made to circumvent it.

## Linux Production Readiness (2026-10-15, commit `405747e`)

**Verdict: not yet verified production-ready across Linux distributions.** The installed client and extension work on this CachyOS x86_64 host, and the key download workflows pass. This is not the same as verifying the release installers, tray behavior, and startup integration across the Linux environments dm claims to support.

### Verified on this revision

- `/usr/bin/dm` was rebuilt from `405747e`; its SHA-256 matched `desktop/target/release/dm-desktop`. Chrome and Firefox extension packages were rebuilt from the same checkout.
- `DM_BINARY=/usr/bin/dm python3 scripts/app-check.py`: all reported functional checks passed, including direct download, browser hand-off, HLS, yt-dlp, proxy refusal, speed limit, schedule, CLI, control protocol and MCP. The normal/hidden window-count checks were explicitly not checked in this Wayland session.
- `cd desktop && cargo test -p dm-desktop --test window`: **48 passed, 0 failed**. `cd browser && cargo test --locked`: **32 passed, 0 failed**. Chromium integration: **51/51 passed**.
- Live Chromium panel workflows passed on YouTube (**35/35**, selected MP4 played for 19.1 seconds), W3Schools (**30/31**, with the live video handed to dm and played for 10 seconds; one unrelated fixture assertion about restoring the catch toggle failed), and Wikimedia Commons (**32/32**, selected WebM played for 634.6 seconds). A subsequent Chromium fixture run passed its full **51/51** suite, including the toggle assertion.
- `SITES=archive,github,ftp,ovh,w3schools,wikimedia,hls,torrent python3 scripts/real-sites.py`: **7/8** initially; Wikimedia's server stopped serving ranges. `SITES=wikimedia python3 scripts/real-sites.py` then passed alone. Internet Archive, GitHub, GNU FTP, OVH and W3Schools direct files matched curl byte-for-byte; HLS and BitTorrent media was checked whole with a standard decoder. Temporary data was removed.
- `FIREFOX="$HOME/.local/opt/firefox/firefox" DM_BINARY=/usr/bin/dm python3 scripts/firefox-check.py native`: Firefox installed the temporary extension without warnings; Wasm handlers loaded; download and menu hand-offs reached dm; HLS stream detection/handoff passed. Live Firefox panel hover was not checked because this run did not use real input.

### Remaining Linux release risks

1. **Release package matrix is incomplete for the current revision.** This run built and installed a native executable directly, not the `.deb`, AppImage or Arch package. The recorded Linux package evidence is container-based, and `docs/gaps.md` still lists the x86_64 AppImage build as partial. Build, install and launch the actual release artifacts on their target distributions/architectures before claiming broad Linux release readiness.
2. **Tray and real input were not re-verified after the current tray refactor.** The latest `405747e` changes tray handling. This Wayland session has no `wtype`, `ydotool` or `xdotool`, and Docker is unavailable, so this pass could not exercise tray open/show/quit, close-to-tray, or physical menu interactions. The prior X11 container run is useful evidence for its revision, not proof for this new tray code.
3. **Login startup is not verified at an actual Linux login.** The hidden-launch behavior has prior container evidence, but this pass did not test the installed login item or session startup end to end.
4. **Firefox panel interaction and browser-menu physical input remain unverified on this revision.** The current Firefox check used its isolated test profile and reported the panel hover as not checked; no real pointer input was available. Chromium live-site checks used browser automation, not a physical mouse.
5. **Known platform limitations remain.** The parity checklist says finished-file drag-out is unavailable on Linux, and bare browser-link dragging is only partial. These need to be treated as limitations in the Linux feature/support claims, or implemented and tested before describing full parity.

### Site-specific status

The former W3Schools 403 issue is **not reproduced on this network**: the latest direct download matched curl, and the live Chromium player's MP4 completed in dm and was checked whole with a standard decoder. This does not guarantee the site will serve every network; a server-side 403 remains outside dm's control. The Wikimedia range failure occurred once in the batch and passed on isolated retry, so it is recorded as intermittent rather than a reproducible dm defect.

### Tray follow-up (2026-10-15)

The tray failure was the Linux backend, not a missing tray host. This CachyOS
Niri session has `org.kde.StatusNotifierWatcher`, but dm was built with
`tray-icon`'s GTK/AppIndicator backend and the host has no AppIndicator
library. Steam's own Linux runtime bundles AppIndicator; dm now uses the
same StatusNotifierItem protocol directly through `tray-icon`'s KSNI backend,
so it needs no extra system tray library. The rebuilt binary registered its
icon and menu with Niri. D-Bus menu events changed the isolated speed-limit
setting and quit the process; the watcher removed the item afterward. Physical
pointer interaction and other Linux desktop hosts remain to be verified.
