# Reference behaviour: two other download managers

dm's engine is compared with two other download managers: Reference A, a
commercial one for Windows, and reference B, a free one for Windows, macOS
and Linux. What they do is taken from their public pages and, since
2026-10-31, from reference A run normally on a Windows test machine against
a local test server (its section below); each claim says where it comes
from. No binary of either program is opened, disassembled or decompiled:
reference A's licence forbids it, and dm is written clean-room. Nothing of
either program's code is used; dm's behaviour is written and tested
independently.

Labels: **Verified** (the reference's own page states it), **Reported**
(third-party guides only), **Observed** (seen from outside, by running it),
**Unknown** (not documented anywhere found).

## Dynamic segmentation (reference A, Verified)

Source: Reference A's support page on segmentation.

- The file is divided "dynamically", during the download, not once at the start.
- When a connection becomes free, reference A "finds the largest segment to download
  and divide[s] it in half"; the free connection takes the second half.
- A segment is not split once it is "too small for this connection type".
  No number is given (Unknown).
- "Full reuse of connections that have finished downloading of assigned
  segments": no new connect or login stage.
- Positions are saved "several times per minute", so a pause or a power loss
  resumes and assembles correctly.

## Connections (reference A, Verified)

Source: Reference A's support page on its options.

- One "Max. connections number" per download. Reference A's own advice is to keep it
  at four or below for most servers.
- An Exceptions table gives some servers fewer connections, "for the servers
  that limit the number of connections from the same client", FTP servers
  that allow one or two among them.
- The default number is 8 (Reported; not stated by reference A).

## Other engine features (reference A, Verified)

Source: Reference A's feature list.

- Resume from where a download stopped; error recovery after lost
  connections, network problems, shutdown and power loss.
- Queues and a scheduler; shut down or disconnect when done.
- Periodic synchronisation: download a file again when it has changed.
- A download quota (megabytes per hour).
- Categories that decide where files go.
- Basic, Negotiate, NTLM and Kerberos sign-in; proxies.
- A virus scanner run on completion.
- A site grabber, and "Download all" links of a page.

## Naming a page's video (reference A and reference B, Reported)

Both name a video they find on a page by the page or video title alone. A
number is added only to tell apart two files that would otherwise clash
(Reported, from how both behave for duplicate names; not stated on a page).

## Reference B

Reference B's own documentation does not say how it segments or limits connections
per server (Unknown). Treated as agreeing with reference A where both are described
by third parties.

## Reference B 6 and video sites (Verified, 2026-10-29)

Seen by running reference B 6.35.2 (its Linux `.deb`) in Docker, reading the
names its libraries export, and reading its video add-on's own source (MIT,
on GitHub, 1.3.14). Reference B's binaries were not
disassembled.

- Reference B itself downloads no video site. Given a YouTube link it says "Video
  downloads require an add-on" and points to the add-on's site.
- That add-on, made by a third party, is the external video tool. It bundles the external video tool's Python source and
  the external video tool's YouTube challenge solver (its `ejs` package); reference B runs it with the
  system's Python (3.10 or later) and Deno (2.3 or later), which reference B installs
  when first asked (`PythonDependencyManager`, `DenoDependencyManager`),
  inside a bubblewrap sandbox on Linux (`QtProcessSandboxManager`: "using bwrap").
- The external video tool only reads the page: `-J --flat-playlist --no-warnings`, plus
  `--no-playlist` for one video. Reference B's own engine fetches the formats it
  lists (`MsDownload`) and joins picture and sound with ffmpeg
  (`QtFfmpegAvMerger`, ffmpeg a package dependency).
- Sign-in: the browser's cookies for the page when reference B has them, written to a
  temporary Netscape file; otherwise `--cookies-from-browser` with the
  default browser, Firefox if that one is not supported.
- Only an HTML page of 3 MB or less is offered to the external video tool
  (`isPossiblySupportedSource`), and a page's reading is not asked for
  again within 300 s (`minIntevalBetweenQueryInfoDownloads`).
- "Sign in to confirm you're not a bot" and the age check are reported as
  needing cookies; "Unsupported URL" as unsupported; anything else as a parse error.

Compared with dm: the same design (the external video tool reads, the download manager
fetches). dm joins picture and sound itself, needs no ffmpeg, and runs
the external video tool unsandboxed; it does not read a browser's cookie store, by decision
(`decisions.md`, the entry on reading a page's video list).

## Videos without a helper: what the player's requests look like today (Verified, 2026-10-29)

Reference A is described (a write-up handed to this project on 2026-10-29)
as reading the player's own requests in the browser, taking the picture's
and the sound's addresses, fetching each in up to 32 pieces at once and
joining the two itself. dm does each of those steps (`browser/src/decide/
streams.rs` `track_of`, `download/dash.rs` `download_media_pair`, its own
muxer), and needs no helper for them: `tests/caught_pair.rs` fetches and
joins a pair with no the external video tool on the computer at all.

What that write-up assumes of YouTube no longer holds. Measured in Microsoft
Edge 156 on a real watch page, playing: 15 requests to googlevideo in 30 s,
of which 13 were `POST videoplayback?sabr=1` with a binary body and no
`itag`, `mime` or `range` (the other 2 connectivity probes). The player
asks for its media by a protobuf body and is answered in UMP frames, so
there is no address of a picture or a sound to take, and none that a plain
range fetch could read. The same is reported across the field: The external video tool
lists YouTube's web client as SABR-only and routes around it with other
clients and proof-of-origin tokens; reference B hands YouTube to an external video tool
add-on; reference A ships a new build whenever YouTube changes. Nothing
reads YouTube without someone keeping up with it.

## Reference B's download engine, measured (2026-10-29)

Reference B 6.35.2 (emulated x86-64) and dm (native) downloaded the same
32 MiB files from a server of this project's that logs every connection and
request, in Docker; the lab is kept outside the repository.

- **First request**: a plain `GET` with no `Range`, left streaming as the
  first piece while its New download dialog waits. dm sends `bytes=0-` and
  keeps that response streaming as the first piece.
- **Ranges**: open-ended (`bytes=N-`); it closes a connection itself when it
  reaches the next piece. dm asks closed ranges (`bytes=a-b`).
- **Splitting**: halves of what is left (offsets near 1/2, 1/4, 3/4, 1/8 ...).
- **Connections grow with slowness**: from a server holding each connection
  to 2 MiB/s it opened 15 at once and 14 more a second later, 29 in all, 24
  at the same time, and finished in 1.2 s. dm kept to its 8 and took 2.0 s
  (32 MiB / 8 / 2 MiB/s). From a fast server it used 6.
- **A 503 on a second connection**: it added no more connections and finished
  on the first; dm moved the refused ranges to its other connections.
- **A page's only video**: it proposes "Lab clip (1).mp4", the external video tool's numbering.
- **Sandbox**: in Docker, where `bwrap` cannot make namespaces, it ran the
  add-on's the external video tool outside one, without a word.

## Reference A's download engine, measured (Observed, 2026-10-31)

Reference A 6.43 build 16, run normally on Windows 11 and started from its
documented command line (`/d URL /p folder /f name /n`), against a local
HTTP server on 127.0.0.1 that logs every connection, request and byte sent.
The file's content is a known pattern, so every saved file was checked byte
by byte. dm 0.8.3 was run the same way (a portable copy, `--headless`, its own
data folder) for the dm column. Only the server's side was watched: no
packet capture, no look inside either program. Not covered: pause and resume
from the window, the browser, video sites, and a server that honours
`If-Range` (the test server ignores it, so dm's own `If-Range` check is not
measured here).

| What happened | Reference A | dm 0.8.3 |
|---|---|---|
| First request | A plain `GET` with no `Range`, kept as the first piece | `bytes=0-`, kept streaming as the first piece; closed ranges for the rest |
| Pieces | Halves of what is left, from the start: the second connection takes `N-` (N = what the first had), then halves of the largest piece; 8 connections within 1.3 s | 8 equal pieces at once; halves of the largest when a connection frees |
| Seam between pieces | Each range ends 64 bytes into the next piece; a resumed range starts 64 bytes back | 1 byte |
| Smallest piece | A 100 KB file on 4 connections, pieces down to about 14 KB | A 100 KB file on 1 connection (pieces at least 128 KiB) |
| Validators | No `If-Range` or `If-Match`, though the server sent an ETag; an ETag changed during a cut was not noticed (all 8 pieces resumed) | `If-Range` with the ETag |
| Content changed under the same ETag, then a cut | Every resumed connection closed after its first 16 KB; about 6 s later the whole file again, from byte 0, on 1 connection | Caught on the first chunk of each resumed range; the whole file again at once, on 8 connections; file correct |
| All connections cut | Back on all 8 within 0.04 s | Back within 1.2 s |
| Server accepting and dropping every connection for 40 s | About 205 attempts a second, never gave up; resumed within 0.01 s of the server's return; file correct | Attempts 1, 2, 4, 8 and 16 s apart; resumed 22 s after the server's return; file correct |
| Server stopped for 30 s (connection refused) | 8 connection attempts open the whole time; back within 0.1 s of the server's return; file correct | Not run |
| Server with no `Range` support | 1 connection, file correct | 1 connection, file correct |
| Same, connection cut at 3 s | Asked `Range` again, got 200, closed it; nothing more within 20 s, no file | The whole file again from byte 0; file correct |
| No `Content-Length` | 1 connection, file correct | Not run |
| 302 redirect | Every piece asked of the final address | Not run |
| 503 for 12 s, then 200 | 2 attempts 6 ms apart, then stopped; nothing after the server recovered | Attempts 1, 2, 4 and 8 s apart, then the download, on 8 connections; file correct |
| 404 | 2 attempts, then stopped | 1 attempt, then stopped |
| Headers | An old browser's User-Agent (Trident 7.0), `Accept-Encoding: identity`, `Accept-Language`, `Accept-Charset`, and a `Referer` of the address's own folder when none was given | dm's own User-Agent, `Accept-Encoding: identity` |

Where reference A is ahead: smaller
pieces (small files go faster), a wider seam check, and almost no time lost
after a dropped connection or an outage. Where dm is ahead: a changed file
is fetched again at once on all connections, a server without ranges
recovers from a cut, and a 503 is waited out rather than ending the
download. Reference A's 205 attempts a second against a failing server is
not something to copy as it is: the fast return it buys can be had with a
short, bounded wait.

## Reference A's FTP engine, measured (Observed, 2026-11-02)

Reference A 6.43 and dm 0.8.6, same method as above, against a minimal FTP
server on 127.0.0.1 (8 MiB file). FTP commands logged per control session.

| What happened | Reference A | dm |
|---|---|---|
| Connections | 6 FTP control sessions for an 8 MiB file | 9 FTP control sessions |
| Probe | First session: `USER PASS TYPE PASV RETR QUIT` — receives file start; second session: `USER PASS TYPE SIZE PASV TYPE LIST QUIT` — reads directory listing | First session: `USER PASS TYPE SIZE MDTM REST QUIT` — probes with SIZE, MDTM, and REST 0 to check resumability; no LIST |
| Segmentation via REST | Subsequent sessions use REST to seek to a position (REST 116800, 4252703 … — halving the remaining file) | Fixed 1 MiB pieces (REST 0, 1048576, 2097152, 3145728, 4194304, 5242880, 6291456, 7340032) |
| EPSV | Not seen | Tries EPSV first, falls back to PASV |
| NOOP | Sent in some sessions | Not sent |
| Strategy | Dynamic halving, same as HTTP | Fixed-size chunks, same as HTTP |

## Segmentation of a 64 MiB file (Observed, 2026-11-02)

| What | Reference A | dm |
|---|---|---|
| Connections | 20 | 13 (+ spot-check requests) |
| Total requests | 20 | 25 |
| First range | `bytes=236230-67108863` (after ~230 KB probe) | `bytes=0-` (open-ended probe kept as piece 1) |
| Split strategy | Halves of the largest remaining piece | Fixed 8 MiB pieces; halves of the largest when a connection frees |
| Time (loopback) | 36 s | 2.6 s |

Reference A's time disadvantage on loopback is because it opens connections one at a time after the probe, waiting for each piece to arrive before splitting it; on a real internet connection the network latency is the bottleneck and the difference narrows.

## Connection cut mid-transfer (Observed, 2026-11-02)

Server cuts TCP connection after sending 512 KB of each range's first attempt; retries go through cleanly. A 16 MiB file.

| What | Reference A | dm |
|---|---|---|
| Result | Failed (1 request, did not retry) | Completed |
| Requests | 1 | 34 |
| Note | Reference A's first request (no Range) was answered with a 200 and cut after 512 KB of 16 MiB. In two runs it made no second request within 90 s and left no file in the folder. dm retried each cut range and completed the file. |

Seen twice, not explained: the harness closes reference A's dialogs
automatically, so a prompt offering a retry may have been closed unread. Do
not take this as "reference A never retries a cut 200" until it is run with
its dialogs left open, at the human QA.

## Outage re-measured (Observed, 2026-11-02)

A 16 MiB file; the server accepts and drops every connection for 20 s from
the first request, then serves normally. dm 0.8.6: 25 attempts, 13 of them
dropped, the first request after the server returned came 1.2 s later, the
file was whole (total 22 s). The 22 s recovery in the table above was dm
0.8.3 and no longer holds: a dropped connection is now asked again at once,
then within 2 s. Reference A made one request and stopped in this run, which
disagrees with the 2026-10-31 measurement (205 attempts a second); the
harness closes its dialogs unread, so its result here is not used.

## HTTP proxy test (dm only, 2026-11-02)

FTP and proxy tests via the harness (`D:\compare\compare2.mjs`). SOCKS5 proxy test for dm pending (proxy settings not loading from DM_HOME/settings.json in the harness; to be verified in the human QA). HTTP CONNECT proxy: dm issues `CONNECT host:port HTTP/1.1` to the proxy and downloads over the tunnel — verified working.

## The floating button's list of qualities, watched side by side (2026-11-02)

Both managers' extensions ran in one visible Edge window (a kept test profile, no one's own), on the
same page, the pointer over the same playing video; reference A's button and list were read from the
screen (its button is a window of its own over the browser, not part of the page), and nothing was
downloaded. Observed behaviour only: no file of reference A was opened, and no binary was read.

| | Reference A | dm (before this entry) |
|---|---|---|
| Where the button sits | At the top right of the video, above dm's; on a narrow video (a reel) it shrinks to a small play mark and a close mark | At the top right of the video, a bar with a drag grip, a mark, "Download video", a help mark and a hide mark |
| First line of its list | "Download all" | A "Part" line, then dm's own best-quality line |
| YouTube, one 15-minute video | TTML subtitles line; then "MP4 file, quality 1080p HD 60 fps", "720p HD", "720p HD 60 fps", "480p", "360p", "240p", "144p"; each line starts with the video's title; no sizes in the list (sizes are in its "Download all" dialog) | "1080p MP4 — 252 MB", "720p MP4 — 137 MB", "480p", "360p", "240p", "144p", then audio-only lines; sizes shown; the 60 fps variants are not listed apart |
| Facebook, one reel | Best first: "MP4 file, 13 sec, quality 1280p HD, 4590 kbps", "MP4 file, 13 sec, quality 640p, 707 kbps", then "MP4 file, quality SD", "MP4 file, quality HD", then an SRT file; each starts with the video's title | The same lines, the two renditions smallest first (changed below), no title, no caption file, with dm's "video + sound" line first |
| Wording | "MP4 file, <seconds> sec, quality <height>p[ HD][ 60 fps][, <n> kbps]" | The same words for a page's own renditions; YouTube lines are dm's own ("1080p MP4 — 252 MB") |

Decided: renditions from a page's own data are listed best first, as reference A does (the maintainer asked
that both be the same; no contract row fixes the order). Not yet done, each a visible wording or layout
change that needs the maintainer's word (gaps row 72): the video's title at the start of each line, a
"Download all" line, caption files as lines, the 60 fps variants and "HD" in YouTube's lines, and leaving
sizes out (dm's sizes were asked for and are kept).

### More pages, same two extensions (2026-11-02)

- **Vimeo (public video).** Reference A: a bar above the video's top right and a second bar at the page's bottom left; its list is eight lines, a TS file and an MP4 file for each of 270p, 360p, 540p and 720p HD, with kbps. dm: "Protected video" and a refusal (the player has an encryption key object).
- **X (one MP4 tweet).** Reference A: a click opens its "Download File Info" window (URL, Category, Save As from the page's title, Remember this path, Description, Download Later / Start Download / Cancel). dm: the click sent the file at once, and the app came forward with it complete; named by the file's own name.
- **TikTok and Instagram (logged out).** dm showed its bar; reference A showed none. On Instagram dm's bar sat on the site's header, since the video's top lies under it.
- Reference A's bar sits on the video's top edge, right-aligned, and is a plain play mark and close mark on a narrow video.

### The desktop apps, side by side (2026-11-02)

Reference A's main window (menus Tasks, File, Downloads, View, Help, Registration; toolbar Add URL, Resume,
Stop, Stop All, Delete, Delete Completed, Options, Scheduler, Start Queue, Stop Queue, Tell a Friend; a category
tree of All Downloads (Compressed, Documents, Music, Programs, Video), Unfinished, Finished, Grabber projects and
Queues; list columns File Name, Q, Size, Status, Time left, Transfer rate, Last Try Date, Description) and dm's
(menus Program, File, Downloads, View, Help; toolbar Add URL, Resume, Stop, Stop all, Remove, Clear finished,
Options, Scheduler, Start queue, Stop queue, Site files; Everything with Videos, Audio, Documents, Apps, Archives,
Pictures, Other, In progress, Done, Queues) were opened in visible windows and every menu, the Options dialog's
tabs, the Scheduler, Add URL and the site grabber were read from the screen. Nothing was downloaded or changed.
dm was a build of this branch on its own data folder; the person's installed dm was not touched. The differences are
gaps rows 79 to 92.

- Reference A's Tasks menu: Add new download, Add batch download, Add batch download from clipboard, Run site
  grabber, Show drop target, Export, Import, Exit. Its Downloads menu: Pause All, Stop All, Delete All Completed,
  Find (Ctrl-F), Find Next (F3), Scheduler, Start queue, Stop queue, Speed Limiter, Options.
- dm's Program menu has the same things (Add numbered series for batch, Add links from clipboard, Site files);
  its File menu is larger (Show progress, Update download link, Copy details, Move/Rename, Open with, Preview,
  Extract here or to, Move to queue, Take out of queue, Move up and down), and its Downloads menu adds Find previous
  and When done.
