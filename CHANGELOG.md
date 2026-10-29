# Changelog

What changed in each release, newest first. The app (`desktop-v*` tags) and
the browser extension (`v*` tags) are versioned separately.

## App 0.8.4

- yt-dlp now starts on a computer whose `/tmp` is mounted `noexec`, whichever build of it dm runs: where the system's temporary folder does not let programs run, dm runs yt-dlp with a temporary folder of its own beside its tools. The single-file build (one fetched before this version, or a standalone one a person installed) unpacks itself into `/tmp` at every start and could not, and every video then said "dm could not get the video from this page".

- A connection that goes quiet is asked again after five seconds instead of held for the 30-second read timeout, once the other connections show it up (under a sixty-fourth of the download's rate while that is over 1 MiB, never under dm's own speed limit, only on a range's first tries): 32 MB with one silent connection 31.2 s before, 6.2 s now.
- HLS segments are fetched up to eight at a time, not one after another (40 segments at 100 ms each: 0.65 s; one at a time they take over 4 s); DASH pieces are taken by the next free connection rather than in groups that wait for their slowest; yt-dlp, where it still downloads, is given `--concurrent-fragments`.
- A video stream whose server holds each request to a size (yt-dlp's `downloader_options.http_chunk_size`) is asked for in slices of that size on each connection, not one long request that gets slowed or cut: 16 MiB where every response slows after 1 MiB, 6.4 s before, 0.55 s now. Streams that state no size are fetched exactly as before.
- A video address that runs out while its stream downloads (YouTube's do after some hours) is renewed: dm reads the page once more and goes on from the bytes it has, instead of throwing them away and handing the video to yt-dlp to start again.
- A video stream is asked with every header its page reading names (`Accept-Language`, `Sec-Fetch-Mode` and the rest), for that stream alone; before only the agent was, and it stayed with the site until dm closed.
- yt-dlp, when it still downloads a file itself, asks for it in 10 MB pieces, as it already did for YouTube's own formats; a server that ignores ranges is unaffected.
- yt-dlp is fetched as its unpacked build on Windows and Linux too (musl included), as on macOS: it starts in 0.22 s where the single file took 0.6 s on Linux, and it starts on a computer whose `/tmp` does not let programs run (mounted `noexec`), where the single file could not and every video said "dm could not get the video from this page". An earlier single file is replaced at the next start.
- A link on port 0 is refused when it is added, with a reason, instead of becoming a row that can only fail; the command line and AI assistants are told of an address already in the list once, not once for each time it was repeated.
- A queue of small files no longer waits for the scheduler's next tick after each one: a finished download hands its place to the next at once. 2 000 small files at once: 131 files/s before, 740 now (all byte-exact); 200 small files behind 20 000 queued links: 37 files/s before, 116 now.
- The window no longer shows a stale state after a change made while nothing was moving: a download resumed from the command line, the control port or an AI client while everything was stopped was shown as stopped (the details window with its Resume button) until it finished; the list is now read within a quarter of a second of any change.
- Pausing and then resuming a download over the control port, the command line or MCP in quick succession no longer refuses the resume as "it is not paused": the answer comes from the queue, not from the window's copy of the list.
- A paused, waiting, finished or cancelled download no longer reports the speed and time left of its last run to the command line, the control port or an AI client; and the details window of a finished download no longer says "Can resume: Unknown until the server answers".
- The progress window of a download that has finished now closes by itself (it stayed open reading "Complete", with no button, one more for each download), and a stopped download's bar in the list is drawn in the progress colour, dimmed, rather than in the grey of its own track.
- The window does about half the work per frame with a long list: with 5000 rows and 20 downloads moving its thread was busy 18 % of the time and is now 10 %. Two dialogs no longer parse every row's address on every frame while closed, the list of queue names is read without copying every row, and the toolbar's selection checks no longer copy and sort the whole list.
- With a long list the window does less in every frame: the rows it shows, the sidebar's counts and each row's kind and search text are kept until the list is read again or a sort, filter or place changes, rather than made again each frame (5000 rows: 370 µs a frame before, 7 ns now), and its thread is busy 5-6 % of the time under 5000 rows with 20 downloads moving, where it was 17-19 %.

- Windows has a portable build, `dm_…_x64-portable.zip`, beside the installer: unzip it anywhere (a USB stick is fine) and run `dm-desktop.exe`. Its settings and list are kept in a `dm-data` folder beside it and nothing else is written to the computer. Each release builds it and runs it once headless before the release is drafted (`scripts/portable-check.py`).
- A video page dm could not download no longer says only "dm could not get the video from this page": yt-dlp's own last error line follows it, in brackets (for example "Requested format is not available" or "HTTP Error 403: Forbidden"), so the cause can be acted on and reported.

**Browser extension 3.2.0**

- Every download type is caught by default, and a list still at the old defaults becomes every type.
- A plain click on a link to a PDF, video or song goes to dm by the catch rules.
- A download the browser hands over starts in dm at once; the everyday controls are where the eye is.
- The "Download video" panel reads a video page's qualities when it is raised, gives up after 50 seconds and says so, and offers a picture and a sound a player fetched apart as one line.
- The popup has "Pause capturing on this site": that site's downloads stay with the browser, except a click with the take key held.
- A new page script, in the page's own world, looks at the address of the page's `fetch` and `XMLHttpRequest` calls to find a player's `googlevideo.com` streams on sites no page reader knows. It changes no request and sends nothing off the computer (PRIVACY.md says so, under "YouTube's player requests"). The Chromium manifest gains that one content script and no permission; the Firefox manifest is unchanged.
- The panel's icon is an SVG in a larger circle.
- The panel's WebAssembly loads in Brave and other Chromium builds that refused the extension's per-session (`use_dynamic_url`) address with "Unsafe attempt to load URL chrome-extension://… Domains, protocols and ports must match", which left every video page with no panel. The Chromium manifest no longer asks for dynamic URLs.

## App 0.8.3

- CI fixes only: a shellcheck guard in `scripts/linux/stress-inside.sh` and a Windows-only timing in one test; no change to the program.

- The queue file is written at most once in ten times what the last write took (150 ms to 5 s), so a very long list spends a tenth of its time saving itself.
- Linux checks: `scripts/linux-stress.sh` runs the workspace tests and the stress phases in a container with no network; three tests that need a non-root user skip under root.

- A compressed response (gzip, deflate, brotli) is decoded by dm, not by the HTTP library, so a body that unpacks to more than 100 times what was sent (and past 64 MiB) is stopped as a decompression bomb instead of filling the folder.
- The queue file is streamed to disk, and written compact above 500 rows, instead of built as one string first.

- A long list no longer slows everything down: the queue file is written at once after a quiet moment and otherwise once every 150 ms (a dm ending writes what is waiting), so 20 000 links are added in 3 s (97 s before) and 2 000 of them cancelled in 0.1 s (60 s); 2 000 small files finish at 132 a second (57); describing the window to a script (`ui.inspect`) takes no longer with 5 000 rows.
- A download of no known length stops before it takes the disk below 256 MB; a link that redirects to a `file://` or `ftp://` address fails with a reason (it was listed as a finished empty download); a response whose headers cannot be read fails at once (it was retried for 150 s); a master playlist that names itself, or a playlist with no media, fails (it was watched for ever); a Metalink whose first mirror serves other bytes goes on to the next mirror the document's digest accepts.

- A YouTube click gets real bytes in 1.7-2.0 s whether it is made at once or after a few seconds of looking (a real video in Edge, measured by the video file growing on disk: 8.4 s at once and 6.0 s after 3 s of looking before). Three changes: the page is read for the download at the same moment as the quality list, not after it; a video's streams that YouTube refuses to dm are no longer handed to yt-dlp as pages (two more runs for each, about 1.4 s); and the read goes through yt-dlp's visionos client first (2.0 s against 3.2 s, the same 31 formats up to 2160p), which also lists no wait where the default merges in a client whose streams YouTube asks to be waited 4 s for. Where that client gives nothing, the default read is made as before. An earlier note in this file put the speed-up at 5-8 s to 1.3-3.7 s: that was measured by the size of files on disk, which counted files dm had only reserved space for.

- A video click now starts in 1.3-3.7 s instead of 5-8 s (a real YouTube video in Edge, from a click on a quality to the first 200 KB on disk: 5.1-5.7 s to 1.25 s after looking at the player for 8 s, 8.2 s to 3.7 s when clicked at once). The page reading made behind the quality list was ended with the browser's host a few seconds after the list was answered, so it never reached the download; it is now made by a process of its own (`dm-desktop --warm-reading`, the page and session on its standard input) that outlives the host.
- With the progress window off, the first download says once, in the status line, to hover a download and press Details to watch its progress.

- A video click made while the page is still being read behind the quality list waits for that reading (up to 45 s, and only while the read says it is alive), instead of having yt-dlp read the page a second time at the same moment.
- A small file from a server that holds each connection down is helped sooner: a range may be halved down to 128 KB pieces (256 KB before). Measured on a server holding each connection to 1 MB/s, 40 ms away: 1.5 MB, 0.9 s to 0.65 s; a server that does not hold connections down is not affected.

- A failed read of a video page says why: the panel's list had thrown yt-dlp's error output away, so every failure read "could not read that page". It now gives yt-dlp's own last error line ("…: This video is unavailable"), and says in plain words when yt-dlp could not run YouTube's player script (its solver or JavaScript engine is missing), which a sign-in does not fix.

- dm takes every download the browser starts, whatever its type (text, zip, anything), unless a list of types is chosen in Options › File types. A list that is still the old defaults becomes every type when dm starts; one a person changed is kept.

- The progress and finished windows saved as on by an earlier dm (which had them on for everyone) are turned off once, and the saved file records it, so turning one back on in Options stands. Options says where to find a download's progress with it off: Details on its row.
- A row has its own buttons under the pointer for what a person wants next: a finished download has Open (plays it), Open folder and Remove; one still going has Stop or Resume, Details (opens its progress window, which no longer opens by itself) and Remove. Enter on a selected row and Show progress in the menu open it too.

- The panel's list of qualities for a YouTube video comes from the watch page's own player data, in about a second (1.3 s, 0.8 s, 1.3 s on three videos, against 4.0 s, 2.6 s, 3.3 s through yt-dlp, with identical lists), and no longer waits for yt-dlp or for it to be fetched. yt-dlp then reads the page behind the answer for the download; where the page gives no list (a sign-in or consent page, a live stream) yt-dlp reads it as before.
- The panel's bar shows a drawn download icon in a larger circle, and dm no longer opens a progress window or a finished window by default (Options › Pop-ups turns them on).

## App 0.8.2

- The update check no longer asks for a signature on `latest.json` (no signing key is held); the installer is still checked against its SHA-256. 0.8.0 and 0.8.1 were never released (their release builds failed); their changes are below.

## App 0.8.0

- The search box in the menu bar sits on the menus' own line, its hint and magnifier on its middle (they were drawn high, with the box grown downward).
- A video picked from the panel's list starts at once: the download reuses the reading of the page the list just made, instead of the video helper reading the page a second time (seconds on YouTube).
- Videos from YouTube and other sites download on every connection set: the video helper only reads the page, and dm fetches the picture and sound itself (32 MB from a server holding each connection to 1 MB/s: 32.7 s → 4.8 s). Where a site refuses dm's own fetch, the helper downloads as before, without reading the page again. A video caught from a player that the site will not serve to dm is fetched from its page instead of failing.
- Windows: while a fullscreen app, a presentation or Focus Assist is on, dm holds its notifications and says them afterwards, and plays no sound.
- Windows: High Contrast. The window is drawn in the system's own colours whatever look was chosen, and follows the theme when it is changed.
- Windows: the taskbar button's jump list has Add URL (opens that dialog in the running dm, as `dm-desktop --new-download` does) and Open downloads folder.
- A download that is retrying says when it tries again ("next try in 8s").
- Controls in the main window are 32 points tall, as dialogs' are, and menu items are 32 with no gap; and a list with room for one row (300% text, the clipboard's bar above it) no longer pulls itself away from the row scrolled to.
- A dialog's shadow is the deepest of 128, 64 and 28 its window has room for; a menu's is 32.
- The buttons that appear on the row under the pointer no longer draw over a window in front of the list (Download complete, for one).
- Options has no divider line under some of its rows and not others; none has one.
- A finished download's row says when it finished, beside "Complete".
- Add dm's welcome asks where downloads go and ends with Done: the browser, the folder, done.
- Closing the window asks once whether dm keeps running in the tray, and keeps the answer (Options › General › When I close the window).
- Options: a line under the settings whose names are jargon, and Restore defaults on each tab.
- The extension's popup has "Pause capturing on this site": its downloads stay with the browser, except a click with the take key held.
- A very big download (5 GB or more) says once that the Scheduler can run it at night.
- The Queue column is drawn only while a download has a queue of its own, so the sixth column is not cut half off at the default width.
- Properties explains Referrer and SHA-256 on hover.
- A finished download taken off the list leaves at once, with no question (its file stays), and Undo in the status bar puts it back where it was for five seconds. Deleting the file is its own command, File › Delete file, which asks and moves the file to the Recycle Bin or the Trash, not erasing it.
- Several links pasted into Add URL are one batch in Download details, in place of "Add one at a time".
- A double-click opens a finished file on a new install (it showed Properties); an unfinished download still shows Properties.
- A running download of known size with no rate yet says "Calculating…" in Time left.
- Dialogs' fields, drop-downs and buttons are 32 points tall, the Fluent 2 standard control height, in place of 24.
- A download the browser handed over and dm waits to ask about (Download details, a page's links, "Download again") brings dm's window forward, so it is not left behind another window or in the tray as if lost.
- A link that gives "404" now says "this link no longer works", in all thirteen languages, in place of "the server has no file at this address".
- A plain click on a link to a PDF, a video or a song, which a browser shows in the tab and starts no download for, now goes to dm by the same rules as a download (the types listed, the sites to leave alone, the master switch). Where dm does not take it, the link opens as it would have; Alt keeps it with the browser as before.
- A list of file types to catch saved by an earlier version gets the types added to the defaults since (documents among them) once, so a .docx or .xlsx is no longer left to the browser for good; types taken out afterwards stay out.
- Text fields in every dialog, not only Options, no longer show a white outline at their rounded corners when they have the keyboard, in the dark look especially.
- The row under the pointer has Stop/Resume and Remove buttons; the status bar's speed limit is a button; downloads that finish together give one notification, and a Linux notification offers Open folder; the floating drop target shows overall progress.
- "Don't show this window again" in Download complete now stays ticked and keeps the window open until you close it, and is saved then.
- More downloads reach dm instead of the browser: Office documents, disk images and more video and archive types are caught by default, and one slow start of the app no longer sends the next few downloads to the browser. A link whose earlier download failed, or whose file was deleted, downloads again without asking.
- A link only the signed-in browser can fetch is now taken by dm with that session for the one download, instead of being left to the browser; a link already in dm's list asks before it is fetched again, and the browser says why when it keeps a download.
- The floating "Download video" button and "Download with dm" always hand the file to dm; a site that refuses dm no longer sends it to the browser's downloader (it shows as a failed row in dm instead).
- A download taken from the browser (the floating "Download video" button, or a caught download) now starts in dm at once; "Download details" opens first only if Options › "Ask where to save…" is on, which it no longer is by default.
- An HLS stream whose picture is a transport stream and whose sound is packed AAC (`.aac` segments) is joined into one MP4 by dm itself, with no video helper.
- Faster: a small file from a server that holds each connection down is fetched on more connections once the first is seen to be slow (3 MB at 1 MB/s a connection, in Edge: 1.7 s → 1.2 s; 1.5 MB in the engine's own benchmark: 1.55 s → 0.8 s; a fast server is never given a connection it does not need); a download handed over by the browser to a running dm starts at once (from the host's hand-off to dm's first request: 520 ms → 59 ms), and one added while others run starts without waiting for the scheduler's next tick.
- The video list opens at once: the panel reads a page's qualities when it is raised over the player, leaves out what a list does not need (a third less time on YouTube), and a reading is kept ten minutes, so a page opened again lists in a few hundredths of a second.
- An HLS stream cut from one file by byte ranges, and a picture and sound of encrypted (AES-128) fragmented MP4, are fetched and joined by dm itself.
- A video a site's player fetches as a separate picture and sound (as YouTube's does) is offered in the panel's list as one line; dm fetches both at once, by byte range, and joins them into one MP4 with no helper program. The pieces of a DASH or HLS track are fetched on as many connections as are set.
- Options › General has an **Appearance** group (Theme, and Text size: Small, Medium or Large, which scales the whole window), every setting there is a toggle switch rather than a check box, the pop-ups are one **Pop-ups** group, and the "Update download link" window that opens by itself when a link runs out has a switch of its own.
- The panel no longer sits on "Reading qualities…": the wait is said in words, streams the page itself loaded are offered after eight seconds, and past 50 seconds it says dm did not answer and gives the click back.
- The panel reads a video page's qualities as soon as it is raised over the player, so a click lists them at once (0.04 s on YouTube in Edge) instead of waiting on "Reading qualities…"; a click made sooner joins the read already under way.
- Windows: the taskbar button shows progress (green while downloading, yellow when paused, red for a minute after a failure), and the program declares per-monitor DPI awareness so text is sharp at 125% and 150%. The computer is kept awake while a download runs.

## App 0.7.1

This release also carries what was prepared as 0.6.0 and 0.7.0, which were never published (0.7.0's release check failed on Windows, in a test script, and was withdrawn).

- Security: a download finished on macOS carries the `com.apple.quarantine`
  mark, so Gatekeeper checks it; a saved site login is sent only over https
  (plain http only to this computer) and only to its exact host, or to
  subdomains too when saved with a leading dot (`.example.com`); the update
  check reads the release's `latest.json` and checks the installer against its digest.
- DASH streams (`.mpd`) are read and joined by dm itself: the picture and
  sound are fetched, with byte ranges where the manifest cuts a file so, and
  joined into one MP4 or WebM without a re-encode. A DASH stream no longer
  needs a video helper downloaded; a live one, or one under a rights system,
  is refused with the reason.
- An address that differs only by tracking parameters (`utm_*`, `fbclid`,
  `gclid`, `igshid`, `si`, `feature`) is the same address when dm asks
  whether a link is already in the list.
- On tiling Wayland desktops (sway, Hyprland, river, niri, …) closing the
  window, Super+Q included, now exits dm, asking first while downloads run.
  dm used to minimize it there, which those desktops ignore, so the key
  looked dead.
- The tray menu has Start queue and Stop all, so downloads can be started and
  stopped without opening the window.

- Shorter, plainer words in Options, the remove question, errors and hints,
  in the sentence case WinUI uses ("Start dm when I sign in", "Allow the
  command line to control dm", "Its file is kept."). The other twelve languages are
  retranslated to match (a machine pass, not yet read by native speakers).

- Options › General › **Allow AI assistants to control dm (MCP)**: on by default, and
  unticked to turn it off. While it is on, dm opens its control port at every
  start (loopback only, behind a token only this account can read), as the
  command-line setting does. An assistant is refused, in words, while it is
  off, and turning it off takes hold at once on a connection already open.
  A port opened only for assistants answers assistants and nothing else; it
  is given a new token at every start; steps a client queues for the window
  are dropped if they wait more than five seconds; an answer that never comes
  ends the assistant's wait after 90 seconds; `dm_cancel` and `dm_move`
  refuse to run without ids.

- The list's filter is a search box in the middle of the top row (modern
  look, window wide enough); the filter row keeps only Clear and the count
  while words are typed. The Find window keeps further from the found row.
- "Add dm to your browser" is an InfoBar, as WinUI draws one: a rounded card
  with a hairline edge, an accent badge and strong title, in both themes.
- Security: over the control protocol and MCP, a download may only be saved
  inside the download folder; and what a client clicks or types can no longer
  run a downloaded file (Open, Open with, Preview, a double-click that opens,
  Install now) or set the command run when a download finishes.
- The floating drop target is a small 44-point card in the window's own
  surface, with a hairline edge and a Fluent download glyph in the accent, and
  fills with the accent only while a link is held over it (it was a solid blue
  64-point tile with a large arrow and a label); it opens in dm's window's bottom-right corner, on
  the same display, not in the middle of the screen.
- AI clients can use the window like a person: `dm_screen` (read what is on
  screen), `dm_click`, `dm_type` and `dm_press_key`, behind the control
  protocol's `ui.texts`, `ui.click`, `ui.type` and `ui.key`. Clicks go to the
  front-most matching words, as the pointer would; running a downloaded file
  (Open, Open with, Preview) is refused.
- AI clients (`dm-desktop mcp`): new tools `dm_pause`, `dm_resume`,
  `dm_cancel` (by id; pause and resume of all when no ids), `dm_move`,
  `dm_queue` and `dm_set_speed_limit` (an exact KB/s, or a preset). To connect,
  start dm with `--control 0`, then register `dm-desktop` with the argument
  `mcp` as a server in the client's settings (for example
  `{"mcpServers":{"dm":{"command":"/path/to/dm-desktop","args":["mcp"]}}}`).
  Removing a download stays a person's click.
- Command line: a word that is no link, file or command (`dm-desktop lst`)
  is now refused with exit 2 instead of silently opening dm; `list --help`
  prints usage; `list` shows percent done and speed, and says "no
  downloads" when there are none; `pause 1 1` counts one download once.
- A folder dm watches (Options › Downloads): a `.url`, `.webloc`, text file of
  links or `.torrent` saved there is added, then moved to its `done` folder.
- Site rules (Options › Network): a folder and a speed limit for a site's
  downloads, applied however the download arrives.
- Properties shows a finished file's SHA-256 and checks it against a digest
  you paste; Move/Rename of several files takes a name pattern (`{n}`,
  `{name}`).
- Options › General › Allow the command line to control dm: the
  command line finds a dm started in the ordinary way (off by default).
- FTP: a part a power cut tore is repaired when a download resumes, as HTTP's
  is.
- Extract a finished ZIP: **Extract here** or **Extract to…** in the row
  menu, into a new folder. An archive with an entry that would leave the
  folder, a link, a password or an oversized total is refused whole, and
  nothing is replaced.
- A command line for a running dm: `dm-desktop list`, `status`, `pause`,
  `resume` and `cancel`, with `--json` and exit codes.
- A live stream's recording can be set to stop after a number of minutes
  (Options › Downloads); what was recorded is kept.
- Closing the window on KDE Plasma and other Wayland desktops no longer
  leaves a frozen window on the screen: dm minimizes it and keeps going.
- An expired link can be refreshed from the browser: Properties › Get it
  from the browser takes the next download the browser hands over as the
  row's new address, and the download continues from where it stopped.
- The progress window's bar shows each connection's share.
- Speed presets (low, medium, high, unlimited) in Downloads › Speed limiter
  and the tray.
- On Windows the schedule can wake a sleeping computer to start.
- A live HLS stream is recorded as it goes, until Stop or the stream ends.
- Preview while downloading: a video or song being fetched can be opened
  in its player from the row menu, from what is on disk so far.
- Windows 11: an opt-in Mica backdrop (Options › General, from the next
  start).
- Three more languages: Simplified Chinese, Japanese and Korean.
- A download being retried says why: "Server error (retry 5 of 10)", "Connection dropped (retry 2 of 5)".
- The Speed column fits in the window at its first size (Queue moved after it).
- A download retrying a failing server no longer holds up the queue: the next one starts while it waits.
- About dm links the source, licence, privacy statement and code signing policy.
- Portable: an empty file named `portable` beside the program keeps dm's
  settings and list in a `dm-data` folder beside it.

## App 0.5.0 and extension 3.1.0

Nothing else to install, a window that works the way a download manager
should, and safer by default.

**Nothing to install, and faster**
- No separate video tool, ever: picture, sound and subtitles are joined by
  dm on every computer alike, and an installed converter is no longer used.
- The first video page fetches about 36 MB (yt-dlp and QuickJS, each
  checked against its checksum) instead of 120 MB. An installed yt-dlp
  months out of date is passed over. Options › Downloads can turn the
  fetching off.
- A server that takes only a few connections at once no longer slows the
  whole download: a connection it turns away steps aside at once (a mirror
  that took 168 s now takes 15 s, close to curl's 13 s).

**Downloads**
- A full disk, a folder dm may not write to, or a drive taken out now
  pauses the download with why, instead of failing it. "Choose another
  folder…" moves what was fetched so far there and goes on.
- A link that expires part-way asks for a fresh one, and goes on from where
  it stopped.
- A file of the same name in the folder: keep both (numbered) or replace it
  once the new one is whole.
- A speed limit of one download's own, in its progress window.
- FTPS (FTP over TLS), implicit and explicit.
- Each named queue can start by itself at a time of day.
- `--when-done exit|sleep|hibernate|shutdown` on the command line.
- A file over 4 GB bound for a FAT32 drive is refused before it starts.

**The window**
- A Filter over the list narrows it as you type.
- Download details opens for downloads caught from the browser (new
  installs), remembers the last folder for each kind of file, and the
  numbered series lists every address before adding them.
- New installs sort files into category folders and start with the
  computer in the tray; the first window says so and offers the browser
  extension for each browser it finds.
- View › Drop target: a small window over every other that takes dropped
  links, even while dm is in the tray.
- Drag and drop works under Wayland too, both ways.
- Clicking a finished download's notification opens the file (Windows,
  Linux); failures are announced with why.
- The update is checked against its release's SHA-256 and offered with
  Install now.
- The computer's accent colour, regional number and date formats, and its
  "reduce motion" setting are followed; Windows 11 uses Segoe UI Variable
  and the title bar takes the window's colours.
- `--help` prints the command line; the tray says what is downloading.

**Browser extension 3.1.0**
- The "Download video" panel can be dragged, and Options › Browser turns it
  off on every site at once.
- A copy-protected (DRM) player shows "Protected video": dm does not
  download protected video.
- Download all links can list only the file types typed.
- An opt-in switch hands the browser's sign-in over with every download,
  for that download alone.
- A page's script can no longer hand dm a magnet link; with no app, the
  popup offers Get the dm app.

**Security**
- On Windows every finished file carries the "downloaded from the internet"
  mark, so SmartScreen still warns on downloaded programs.
- A secure address that redirects to plain http is stopped.
- Rights-protected HLS streams are refused rather than half-fetched.

## App 0.4.0

Video sites with nothing else to install, and drag and drop that takes a
browser's links.

- Nothing to install for video: the first time a video page needs them, dm
  fetches yt-dlp (and Deno, which it runs YouTube's checks with) into its
  own folder, checked against their published checksums, and keeps yt-dlp
  current weekly. Picture and sound are joined by dm itself, MP4 and WebM
  alike (VP9, AV1, Opus), at the same qualities as before, 4K included.
- Streams with no other tool: HLS and other transport streams become MP4s
  (H.264, HEVC, AAC, MP3, AC-3, E-AC-3); `rtmp://`, `rtsp://` and `mms://`
  are recorded by dm, into a file that plays even if cut short. Subtitles
  go inside the video. Only RTMPE, RTMPT and RTSPS still fetch a separate video tool.
- Drag and drop: a link dragged from a browser onto the window is queued
  on macOS, Windows and Linux (X11), and a finished file can be dragged out
  of the list on Linux (X11) as on macOS and Windows. Under Wayland neither
  works yet; the window says what to do instead.
- Windows finds a yt-dlp, and the video tool dm then used, installed by
  winget, pip, Scoop or Chocolatey, and the dm a browser starts no longer holds the browser's
  pipes open.
- Clearer: "Site files" is the one name for the finder, with Find files
  right after its fields; the numbered series shows where `*` goes; a
  number box says when what was typed is not a number; View › Columns stays
  open while columns are ticked; a long `.tar.gz` name keeps `.tar.gz` when
  shortened; menu shortcuts no longer touch the edge.
- Fixed: a dialog about as tall as the window flipping between scrolling
  and not, so its buttons missed clicks (Site files on Windows); a joined
  sound running a few milliseconds past its end in newer players.
- Security: rustls-webpki 0.103.16 (RUSTSEC-2026-0381, -0382, -0383).

## App 0.3.0

A Fluent look throughout, a faster and lighter engine, and new ways in:
sign-ins, streams and FTP folders.

- Looks: Fluent 2 throughout — WinUI's colours, dialogs laid out with
  their buttons on a footer band, the navigation pane's expandable places,
  Fluent UI System Icons, Segoe UI on Windows and Selawik elsewhere. Drop-
  downs as tall as the buttons beside them, a picked item marked with the
  accent, number boxes and fields underlined, Options' tabs fitting at 150%
  text, Properties opening in the middle.
- Faster: a download over several connections is written in place into
  one file and finished with a rename, not copied together at the end —
  2 GB in about 2 s where it took about 3.6 s, and half the writes.
- Lighter: started at login, dm waits in the tray without making a window
  until one is asked for (13 MB rather than 43, and no flash); threads end
  once downloads do; ten downloads at once hold 20–30 MB less; an idle
  window wakes only when something changes.
- Sign-ins: NTLM, under NTLM or Negotiate, for Windows servers and company
  intranets (`DOMAIN\user` in Options › Sign-ins or the address).
- Streams: `rtmp://`, `rtsp://` and `mms://` addresses, fetched through an
  installed converter and saved as playable files.
- Files on a site: reads an FTP server's folders too, and a saved search
  can stop reading pages after so many minutes.
- Clearer: a failed row says why in the list itself; a full disk says
  which folder's; "not enough room" names the download folder; Escape
  closes the window in front; Copy link in the row and File menus.
- Fixed: a file downloaded twice at once under a chosen name pushing the
  other to "(1)"; a deleted file's row turning Complete when the same
  address was downloaded again; Time left cut off at the first window
  size; two tooltips over a shortened error.

## App 0.2.0

Nine rounds of hands-on testing, each fixed.

- Fits at large text: every dialog stays inside the window at 150%, 200%
  and 300% text, scrolling where it must; Options never draws over itself.
- Reads well: sizes, speeds and times in tabular digits; Inter for every
  script it covers, bold titles in Cyrillic and Greek; status sentences
  whole, read out to a screen reader, and translated in all ten languages.
- Keyboard: F10 and the arrow keys walk the menus, Shift+F10 opens a row's
  menu, a visible focus ring round every control; Cut, Copy and Paste on a
  right-click in any field.
- The list: drag a row to reorder its queue, columns that follow the window
  until you size them, a third heading click back to queue order, a
  re-added file shown under the name it will get.
- Downloads: the wait between retries can be set; a retrying download can
  be given a new address; sites whose links work only once are taken
  without a check that would spend them; a video asked for is never saved
  as its page's HTML; the progress window shows each connection's share.
- Linux packages suggest a font for Chinese, Japanese and Korean names, and
  a Firefox unpacked into /opt is registered.

## App 0.1.0 and extension 3.0.0 — first public release

### The app
- Downloads over HTTP, HTTPS and FTP with several connections at once,
  resumes after a dropped connection, a crash or a restart, and checks each
  part against the server before trusting it.
- A queue with pause, resume, retry, named queues with their own limits, a
  scheduler and a speed limiter.
- Video and audio: HLS streams (with conversion to MP4 where a converter is
  installed), and video sites through an installed yt-dlp.
- BitTorrent (magnet links and `.torrent` files) and Metalink with mirrors.
- Sign-ins (Basic and Digest), a `cookies.txt` file, and proxies (HTTP,
  HTTPS, SOCKS4, SOCKS5, or the computer's own settings).
- Categories that sort downloads into folders, checksum verification, a
  download history, and an update check that never installs anything by
  itself.
- Lives in the tray, can start at login, and runs one copy at a time.
- Windows, macOS and Linux (.deb, .rpm, AppImage, Arch).

### The browser extension
- Chrome, Edge, Brave, Vivaldi, Opera and Firefox.
- Hands the browser's downloads to the app. With no app running, the
  browser keeps its downloads, so nothing is lost.
- "Download with dm" on links, media and selections, and a "Download video"
  panel over video players.
- Optional per-site switches: take every download from a site, or hand over
  a site's sign-in.

### Known limitations
- The builds are not code-signed. Windows SmartScreen and macOS Gatekeeper
  warn on first launch; the README says how to open dm anyway.
- dm has not yet been run on a real Windows computer.
