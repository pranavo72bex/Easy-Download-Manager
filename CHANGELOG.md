# Changelog

What changed in each release, newest first. The app (`desktop-v*` tags) and
the browser extension (`v*` tags) are versioned separately.

## Unreleased

- An HLS stream whose picture is a transport stream and whose sound is packed AAC (`.aac` segments) is joined into one MP4 by dm itself, with no video helper.
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
  check refuses a `latest.json` that lacks a good ed25519 signature
  (`latest.json.sig`).
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
