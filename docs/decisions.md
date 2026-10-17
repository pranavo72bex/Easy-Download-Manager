# Implementation decisions

Decisions that shape dm and are not obvious from the code, newest first. Each says what was decided and why, so a later change
can see what it is undoing.

## 2026-10-17: A key is taken by the command that asks for the most modifiers

egui matches a shortcut ignoring an extra Shift, so ⌘N matched ⌘⇧N and Add
URL opened in place of the numbered series. Rather than match modifiers
exactly (which would break a key whose character needs Shift on some
keyboards), every command's keys are tried with the most modifiers first,
and the first that matches consumes the press. Remove follows the platform:
⌘⌫ (shown) and ⌫ on macOS, whose keyboards have no Delete without Fn, and
Delete elsewhere. Keys that act on the selection do nothing while a dialog
is in front; the per-download progress and completion windows do not count
as in front. A dialog asked for while it is open is raised, not rebuilt.
Window state (`persist_window`) goes to `$DM_HOME/window.ron` when `DM_HOME`
is set, so test and screenshot runs leave the person's own alone.

## 2026-10-17: A frame carries the oldest protocol version that reads it

`download.start` gained `cookies`, `mime` and `deadline` while
`PROTOCOL_VERSION` stayed 1, and serde ignores keys it does not know: a
newer extension (stores update it on their own) talking to an older app
had `mime` ignored, so a sign-in page served in a caught file's place was
taken and the browser's copy erased. Bumping the number alone was not
enough: a version-1 host does not answer a frame of another version at
all, so a hello at 2 would read as "no app" and the menus would stop
working too. So a frame's `protocolVersion` is the oldest version that
reads it correctly: a `download.start` for a download the browser holds
(`held`) goes at 2; everything else, a file chosen from a menu or the
panel included (checked, but the browser holds nothing to lose), and every
reply, at 1. Both sides read 1 and 2. The hello says the newest
version the app reads (`protocol`, absent from a version-1 app), and the
extension hands a download the browser holds only to an app that says 2 or
more; an older app keeps getting what a person asks for from the menus and
the panel, which the browser never started, and the popup and one
notification say "update the dm app". A caught download sent to an older
app anyway is not answered and is given back after the wait. A frame of a
version outside 1..=2 now names the side to update, not `--register`.

## 2026-10-17: Alt-click and the page's links are plain JavaScript

They were registered by the WebAssembly page script, so on a page whose
policy forbids WebAssembly (where Firefox applies it to content scripts)
Alt-click no longer kept a download in the browser and "Download all
links" said to reload. They are a few lines each and read nothing
WebAssembly decides, so `extension/content.js` registers them before it
imports anything; the panel stays Rust. A unit test holds the message
names in the script to the constants in `decide.rs`.

## 2026-10-17: A lock dm cannot read stops dm, and says so

`instance.lock` that cannot be opened or locked (a directory in its place,
a file system without locks) used to read as "another dm is running", and
dm handed its links to nobody and exited without a word, every launch.
Starting anyway without the lock was weighed and dropped: the next launch
would start a second dm the same way, writing the same `queue.json`. So
dm exits with the reason on stderr and in a dialog (none when headless),
and the native host treats the lock as unreadable rather than as a running
dm: it refuses the hand-off, and the browser keeps its download. A lock
file left by dm run as another user is opened to read instead, which an
advisory lock allows, so `sudo dm` once does not cause this. The host's
probe still takes the lock for a moment; a dm starting then retries for
half a second before believing another is running.

## 2026-10-17: Waiting connections to the control port are evicted, not refused

Connections that have not presented the token wait in their own list,
apart from the 16 authenticated slots, and for at most 5 s. When the list
is full the oldest is closed for the newcomer, so a program filling the
port with idle connections cannot keep out the owner, whose connection
authenticates in a round trip. A first line that is not a valid `auth`
closes the connection.

## 2026-10-17: A handed-over sign-in belongs to one download and one server

The first version kept handed-over cookies in a process-wide map by host,
filled when a row started and never emptied: a later download from that
site, one never signed in, or one whose address was edited, carried them
too. Now a row's cookies travel in that download's options as a `SignIn`
(the server they came from, the header) and are sent only to requests for
that same server, so a mirror, a CDN redirect or another row never sees
them, and an address edited onto another server drops them. A row keeps its sign-in until it is deleted from the list: dropping it
when the download finished was tried and broke Redownload, and left the
recheck queue probing signed out, where a sign-in page of another length
reads as a changed file. dm's files are written readable by the account
alone instead. yt-dlp gets
them in a private cookies file of its run (mode 0600, removed after),
not as a command-line header any local `ps` could read. The control
protocol and MCP say only whether a row has a sign-in.

## 2026-10-16: Released unsigned, with the free signing there is

dm earns nothing, so nothing that costs a yearly fee is bought. Releases
go to GitHub unsigned for every platform, and the README says how to open
one. Where signing is free it is taken: Mozilla signs the Firefox build at
no cost without a store listing (self-distributed), which ends the
"temporary add-on every session" problem; SignPath Foundation signs
open-source Windows builds through CI at no cost, and dm applies. Package
managers that cost nothing carry it by name: winget and Chocolatey on
Windows, a Homebrew cask on macOS (no quarantine warning there), the AUR
and Flathub on Linux. The Chrome Web Store's one-time five dollars is the
one paid item worth it, when it can be spared; Edge Add-ons is free. Apple's
notarisation has no free route and is not bought: the dmg stays unsigned
with right-click-Open instructions.

## 2026-10-16: A row remembers the folder its parts began in

The folder for unfinished parts is a setting now. The obvious reading —
"takes effect for downloads started afterwards; the rest start over" —
loses progress silently, so instead each row records the folder it first
staged under and resumes there whatever the setting says since. The
queue's parts helpers (discard, connections, resumable) read the row's
folder first. A `.ts` stream is remuxed, not re-encoded: `-c copy` into
an MP4, kept as `.ts` when ffmpeg is absent, a digest names the `.ts`
bytes, or the remux fails — a stream a person can play beats no stream.

## 2026-10-16: Escape is one key, and the menu's before the dialog's

An Escape that closes a menu, a combo box or a right-click menu is theirs:
egui closes them on it without consuming the key, and they draw before
the dialogs, so by the time the dialogs see the key the popup is already
gone. The window remembers whether one was open at the end of the last
frame and lets that Escape pass. A text field with the keyboard lets it
go on the same Escape that closes its dialog, as dialogs generally do;
the two-step the code once described was never what the code did.
Dialogs close in the order a person means — a question first, Find and
the other small ones over the large ones, then the large — since nothing
records which was opened last. Found by a code review of 2026-10-16.

## 2026-10-16: A site search stays on its site and stops at a hundred pages

"Files on a site" can now read pages past the first, but only pages on
the start page's host, at most five deep and a hundred in all, and a
path can be skipped by a word in it. The bounds are the point: a search
that follows every link is a crawler, and a person wanting a site's
files wants that site's. Saved searches run from the window's own tick
(once a minute it looks for one that is due), through the same client as
the downloads, so the proxy and the timeout apply; what they find is
added for later, never started, and never twice.

## 2026-10-16: A proxy that is off is no proxy; the computer's settings are a kind

reqwest's default client reads the environment's `HTTP_PROXY` and friends,
and with its `system-proxy` feature the macOS network settings too. Turning
that feature on to offer "the computer's proxy settings" made every client
dm builds take over a second on this Mac (`SystemConfiguration` is slow to
answer), and a test that paused a download at 400 ms found the download had
not started. So the system's settings are read only for the one proxy kind
that asks for them (`builder_with(read_timeout, system_proxy)`); a proxy
that is off goes out directly, whatever the environment says, and a proxy
of dm's own covers every address but its bypass list. A proxy dm uses is
one chosen in its Options. PAC scripts are not read: they are JavaScript.

## 2026-10-16: Digest is answered; NTLM and Kerberos are not

A 401 with a `Digest` challenge is answered in `auth.rs` with the login the
address carries (RFC 7616, MD5 and SHA-256), and the challenge is kept per
server so the ranges sign in from the start. Negotiate, NTLM and Kerberos
are not attempted: they authenticate a TCP connection, not a request, over
several round trips, which a pooled HTTP client does not promise to keep
on one connection, and dm has no Kerberos ticket cache to draw on. A server
asking only for those gets the 401 said with what it asked for.

## 2026-10-16: Attempts, timeout and per-site connections are settings; one ceiling still

Five attempts and a 30-second read timeout were constants. A person on a
link that drops for minutes wants more attempts, or none at all ("keep
trying": zero), and a server that assembles an archive for a minute is not
dead to the person who knows it; both are Options › Network now, with
bounds (100 attempts, 600 s). A server that refuses a second connection
gets a limit of its own by host or suffix, which never raises the setting.
The speed ceiling stays one for everything (see 2026-10-12): per-download
limits were asked about and are still not wanted, because a shared ceiling
is what a link has.

## 2026-10-16: Rows are clicked by rectangle, not by the accessibility tree

The Linux real-input check (`scripts/linux/gui.py`) finds the toolbar, menus
and dialogs through the AT-SPI accessibility tree and clicks them where it
says they are. Table rows are the exception: egui's accessibility tree
reports a virtualized table's cells at the wrong screen position (a row at
y=244 was announced at y=431), so a click there selects nothing. Rows are
instead clicked by their own rectangle, which dm reports over the control
protocol (`ui.inspect`), plus the window's on-screen origin. The same
protocol is the check's oracle: on any failed check it prints dm's view of
the window (open dialogs, selection, rows) so a failure says why.

A running or queued download cannot be deleted until it is stopped
(`enabled(Command::Remove)` excludes those states): dm keeps an active
download safe, and the test follows that — Stop, then Delete.

## 2026-10-16: An installer is installed and run before it is released

Building an installer proves nothing about installing it: the .deb named
too few libraries for a month without a failing build, because GTK (gone
with the tray's switch to D-Bus) had been bringing winit's X11 libraries
along. So the release workflow installs each installer on its own system
the way a person would (apt, dnf, the AppImage itself, the Windows
installer silently, the app copied out of the dmg), runs
`scripts/smoke.py` against what was installed — the native host's answer,
the window opening under a display, a download byte-exact — removes it,
and drafts the release only when every one passed. The libraries winit and
glow open are named in every package, by package name for .deb and Arch
and by library (soname) for .rpm, which Fedora and openSUSE both resolve.
Test runs use nextest's `ci` profile, a hung test killed at five minutes.

## 2026-10-15: The Linux tray is a StatusNotifier item

Linux uses `tray-icon`'s KSNI backend over the session D-Bus instead of its
GTK/AppIndicator backend. Niri, GNOME and KDE expose StatusNotifier hosts;
requiring a separately installed AppIndicator library made the tray vanish
on this CachyOS/Niri machine even though the session already had a host.
KSNI owns its service thread and exports the same icon and menu without
GTK-specific startup. The test machine's StatusNotifier registration and
menu callbacks are verified.

The tray is made whether or not a watcher is up, since at login dm can start
before the panel and KSNI registers when one appears. Whether the window may
hide is asked on closing, and the answer is whether the watcher lists dm's
item, not merely whether a watcher exists: a watcher that has just started
can take its bus name before it answers, and drop a registration that lands
in that gap without a reply (seen in the Linux check after a watcher
restart, which is what reloading waybar does). So a thread checks every five
seconds and registers the item again when the watcher does not list it, and
a login launch that finds no tray showing it after ten seconds opens its
window rather than run unseen.

## 2026-10-15: dm's own names, list format and remote control

dm is its own program and names no other. Its menus, dialogs, columns,
categories and themes use dm's own words (Program menu, "Download details",
"Add numbered series", "Files on a site", Videos/Audio/Apps/Archives/
Pictures, Light/Dark/Classic). Export and import use dm's own `.dmlist`, a
JSON list of rows; a plain list of links still imports. The only remote
control is dm's own token-protected control protocol (`--control`) and its
MCP server (`dm-desktop mcp`); the extra JSON-RPC server that spoke another
program's protocol was removed, with its setting. The window's controls are
held by window tests that click every menu item, toolbar button and
row-menu item by its name, not by a catalog of another window.

## 2026-10-15: FTP by a client of dm's own

A download needs six FTP commands; a library would bring TLS and a second
async runtime's worth of choices for them. Passive only (active needs the
server to reach the client, which a home router stops). The data
connection goes to the host the control connection reached, never the
address a PASV names, as curl does: right behind NAT, and no way for a
server to point dm elsewhere. FTPS is not done. FTP goes through a SOCKS5,
SOCKS4 or HTTP CONNECT proxy (2026-10-16), both connections, with the
server's name handed to the proxy for the data connection; an HTTPS proxy
is refused, not sent around.

## 2026-10-15: Ranges are halved under the connection fetching them

Pieces alone (about four per connection) left the last slow piece to one
connection while the rest sat finished. Now, when a connection frees up,
dm halves the largest range left and gives it the second half. Each range has a
small lock of its own holding where it ends and how far it has claimed;
its connection claims a chunk before writing it, and a split moves the end
only past the claim. Splits append ranges, so part order is no longer file
order: assembly sorts by start, and the overlap byte is checked against the
range before in the file. The plan file is rewritten before a new range
fetches, which keeps "the plan beside the parts is the plan a resume
finishes" true. Halves stop at 256 KiB, and a download has at most 256 ranges.

## 2026-10-15: A video is an MP4 unless WebM is asked for

Left to itself, yt-dlp picks YouTube's best stream, which is AV1 or VP9
in WebM with Opus sound: files many players (QuickTime, Windows' Films &
TV, phones) open badly or not at all. dm now
asks for MP4 picture and M4A sound, H.264 ahead of AV1 at the same height,
joined into an MP4, whenever no WebM line was picked. Above 1080p YouTube
has no H.264, so 4K is VP9 in an MP4: the height asked for wins over the
codec.

## 2026-10-15: The window's language is per thread, and held by a walk

`i18n` keeps the language in force per thread. The window is drawn on one
thread, so it is the window's; and the window tests, each on a thread of
its own, can put one window in German while another runs in English. A
process-wide value made them fail at random. Words for a script or an
agent (the control protocol, MCP) stay English on every machine; the status line's sentence is kept in English
with its values (`i18n::Said`) and worded only where it is drawn, so
`said` reaches them in English too. What is translated is held two ways: a scan of the source for every string passed
to `t`/`tf`, and a walk of the window in each language that records every
string asked for and missing, which the scan cannot see for words chosen
at run time. A translation may name its values `{0}`, `{1}` where its word
order differs.

## 2026-10-15: A file dm is refused goes back to the browser, from the panel too

A caught download dm cannot fetch was already left to the browser. A file
from the panel or a menu was not: dm was refused (W3Schools' 403 on one
Linux network) and the person got nothing. Such a hand-off is now checked
the same way, and when dm is refused or absent the extension starts the
browser's own download of it and the panel says so. A player page (handed
to yt-dlp) is not sent back: the browser cannot download a page as a video.
The probe also tries a browser's agent, with `dm/<version>` after it, as a
last rung after dm's own and the plain `dm/<version>`: it is still named.

## 2026-10-14: Browser drags can be URL-backed temp files, not just regular files

The window reads dropped files through the egui file handle, but some browser
sources hand over a temporary file or a URL-shaped path when they cannot expose
readable bytes. dm now falls back to the dropped path and parses URL-like text
from it before giving up, which fixes the common browser-drag case without
claiming a full live-browser drag has been proven end to end.

## 2026-10-14: More pieces than connections, taken in turn

One fixed range per connection let one slow connection decide when a
download finished. Splitting a range under a connection that is still
filling it would need part files to change
meaning mid-download, which dm's resume and its one-byte overlap check rely
on not happening. Instead the file is cut into about four pieces per
connection, and connections take the next unstarted piece in file order:
the same effect for any slowness short of the very last piece, with no
change to resume or assembly. 8.36s became 2.26s in `tests/speed.rs`.

## 2026-10-13: The installed app registers itself with the browsers

A person who installs dm from a release should not need a terminal. Each
time the installed app opens, it checks that every installed browser's
host manifest names this copy, and writes those that do not, keeping any
extension ids already allowed there. A development build, a dm with
`DM_HOME` set (every test), and a copy running from the mounted disk image
do not, so none of them can take the browsers from the installed one. An
AppImage registers the AppImage file, not the program mounted inside it.

## 2026-10-13: A range answered whole is not always a changed file

A 200 to a range used to mean the file was republished, and the download
failed. Wikimedia's caches answer several ranges at once that way under the
same ETag, and a range of every byte too. Now the validator decides: the same
one means the server stopped serving ranges, and the download starts again
on one connection (a whole answer to a range of every byte is the file);
another one still means the file changed. dm's user agent also names the
project's address, which Wikimedia requires.

## 2026-10-12: Light or Dark by default, following the computer

dm's window draws every colour from `theme.rs` tokens. The default look is
Light or Dark as the operating system is set; Classic stays as a choice.
Selawik (OFL) is embedded as the window's typeface. Fonts given to egui take
effect a frame later, and egui panics on a family it has not loaded, so
semibold is used only from that frame on.

## 2026-10-12: Notifications are posted off the window's thread

On macOS, posting a notification waits on the system for about two seconds.
From the window's thread, each finished download froze the window that long.

## 2026-10-12: A video from the panel asks first

The app opens "Download details" for a panel video (`ask_panel_downloads`,
on by default). The picked quality travels with its container, and yt-dlp is
told to prefer that container, so an "MP4" line arrives as .mp4.

## 2026-10-12: One speed ceiling for everything

A limit per download would let N downloads use N times the limit. dm's
is one ceiling, shared by every connection and split with torrents and
yt-dlp runs. A limit is set to leave the link usable, and that is a
statement about the link, not about one download.

## 2026-10-12: The browser extension is Rust compiled to WebAssembly

Only two small loaders are JavaScript, because a Manifest V3 background and
a page script must start before WebAssembly can load. Its decisions live in
`decide.rs`, tested on the host.

## 2026-10-11: The window is egui, not a web view

dm's window is drawn in Rust. A Tauri
web view was replaced: one language, one process, and a window the tests
drive through its accessibility tree.

## 2026-10-11: Player pages go to the user's yt-dlp

dm writes no site logic, token minting or signature work of its own. It runs
the yt-dlp the person installed and places what it produces. Nothing behind
DRM is attempted.

## Always: free, with nothing reported

No analytics, crash reporting, telemetry or paid service. `cargo xtask
privacy` fails the build on an address that would phone home.

## 2026-10-15: Power cuts are repaired at resume, not prevented by fsync

A part's length is its resume ledger, and after a power cut the length can
be on disk while the last bytes are not. Syncing every write would prevent
it at the cost of throughput; instead a resume fetches each part's last
64 KiB again, compares, and cuts the part back to where it agrees. The
queue and settings are synced before they replace the old copy, since an
empty queue file forgets every download. The plan is not: it is rewritten
at every split, a flush each time halved 8-connection speed, and a stale
plan still covers every byte while an empty one only costs a refetch.

## 2026-10-17: A recheck replaces a file only with a whole one

A recheck that finds a file changed fetches the new version beside the
old, which the row keeps pointing at (`replaces`), and moves it over the
old one only once it is whole; a failure leaves the old file and says so.
Deleting first, a server that failed halfway left nothing. What is compared
is what the row was fetched from: a Metalink row by the file's length
alone, since its ETag is one mirror's of many; a yt-dlp or stream row not
at all, because its address is a page or a playlist and nothing it serves
describes the file made from it.

## 2026-10-17: SOCKS5 resolves names at the proxy; FTP paths are the login's

A SOCKS5 proxy is handed out as `socks5h://`, to dm's client and to
yt-dlp, so a name is resolved by the proxy: resolved locally, a name only
the proxy's network knows cannot be reached, and every site is told to the
local resolver. librqbit reads only `socks5://`, so the swarm is given that
spelling; its peers are addresses already. An FTP address's path is relative
to the login directory, as RFC 1738 has it, and `%2F` reaches the root.

## 2026-10-17: A page named as a document is that document

A link whose last segment names a document (`.html`, `.xml`, `.txt`, not a
script's `.php` or `.aspx`) is fetched by dm's own client, with the row's
referrer and cookies, and never handed to yt-dlp. It used to go to yt-dlp
first, falling back only on "unknown site"; any other refusal (a 403 to a
request without the page's sign-in, a 429) failed a page dm could save.
Some video sites do end watch pages in `.html`: a quality picked in the
browser's video panel (which carries a container) still goes to yt-dlp. A
watch address with no extension, or a script's, is yt-dlp's or refused.

## 2026-10-17: How far a power-cut repair looks back

A resume compares a part's last 64 KiB with the server; where the disk has
zeros the server does not, the part is cut back to where the zeros begin.
That scan stops 4 MiB back: far more than a disk block or a write buffer,
and little to fetch again where the zeros were the file's own (a disk
image, a sparse archive), which unbounded meant gigabytes. A longer loss is
still found, 4 MiB at a time: the window at the cut opens on zeros the
server does not have and is cut again. Each step goes straight to the next
attempt rather than spending one of the range's retries.

## 2026-10-17: A stream's segments are named by their bytes

The first segment's first 4 KiB (from the staged part on a resume) decide
TS, fMP4 or packed ADTS audio; the playlist's addresses decide only where
the bytes say nothing or are encrypted. Tokenised segment addresses have no
extension, and ADTS from them was called `.ts` and sent to the remux. A
changed initialisation segment is decided once, in `beyond_dm`, on resolved
addresses, and goes to yt-dlp.

## 2026-10-17: Secrets stay off yt-dlp's command line

The proxy (with any user name and password) goes in a 0600 config file of
the run's own, passed with `--config-locations` and removed when the run
ends, as the cookies already were: any local user's `ps` reads a command
line. The process group is ended with `libc::killpg`, not by running
`kill`, which a minimal `PATH` may not find.

## 2026-10-17: egui 0.37, for screen readers on at-spi2-core 2.62

at-spi2-core 2.62 (Arch since 2026) dropped the bus's `ScreenReaderEnabled`
property and keeps only `IsEnabled`. egui 0.36's accesskit_unix 0.21 waited
for a change to `ScreenReaderEnabled` before putting the window on the
accessibility bus, so there dm was invisible to Orca and to every AT-SPI
lookup; accesskit_unix 0.24 (egui-winit 0.37) listens to `IsEnabled`. A
`[patch.crates-io]` of accesskit_winit alone was the fallback, not needed.
What 0.37 changed under dm, kept as it was: eframe's clipboard and egui's
emoji fonts became opt-in features (`clipboard`, `monochrome_emoji_fonts`),
both turned on; a screenshot is now a callback (`request_screenshot`), not
an input event. 0.37's test harness panics on a glyph no font has, which
found the table's sort arrows (↑ ↓) drawn as boxes all along: Hack, already
bundled, is now the last fallback for the proportional families.

## Tried and dropped

- **A JSON-RPC server speaking another download tool's protocol** — built,
  then removed: dm's own control protocol and MCP server cover scripts and
  agents, and dm names and mirrors no other program.
- **`chrome.power` / lifecycle work** — built, then removed: it cost a permission for a benefit resume already provided.
- **HTTP Basic site logins** — built, removed as out of scope, and back as
  Options › Sign-ins (Basic, and Digest since 2026-10-16) once the engine
  was the one fetching protected files.
- **A CORS proxy for player-served media** — does not work even in principle;
  such URLs are bound to the session and address that minted them.
- **"Chrome for Testing will not read a host manifest from a scratch
  profile"** — wrong, as it turned out: Chrome's user-level host directory
  is derived from the user data directory, and `chrome-check.py` registers
  the host in `<profile>/NativeMessagingHosts` only and it is found. What
  had hung was a host binary under `~/Downloads`.

## Free space is checked once, at the start (2026-10-17, gap 60)

A download whose stated length exceeds the free space is refused before a
range is fetched: the parts' disk must hold what is not yet staged, and the
destination's disk the whole file when it is another disk (on one disk,
assembly frees each part as it is consumed, so the length is enough). Read
with `statvfs` on Unix (`libc`, already a dependency) and
`GetDiskFreeSpaceExW` on Windows through `windows-sys` — already in the
tree through tokio and others, only its `Win32_Storage_FileSystem` feature
named; a crate of its own for one call was not worth it. Where the space
cannot be read nothing is refused. Space taken by other programs during the
download is not watched: a disk that fills midway fails the write as before.

## A refusal's body is said only when it says something (2026-10-17, gap 69)

"the server answered 404 Not Found" carries the canonical reason. The body
is read down to its text as before, and then dropped when, without the
status, its reason phrase, filler words and a server signature, nothing is
left — the boilerplate pages of Python's http.server, nginx and Go. A body
with words of its own (a rate-limit sentence) is kept.

A file system that reports no total size (some FUSE and SMB mounts answer
0 free of 0) is taken as unknown and not checked (gap 78).

## A folder name: usable here to load, portable to make (2026-10-17, gap 77)

`one_folder` decides whether a saved category or kind's folder is loaded
and used: one folder, not `.` or `..`, and on Windows a name Windows can
make. `portable_folder` adds every computer's rules (no `<>:"|?*` or control
characters, no trailing dot, no device name) and is applied only to a name
being made or renamed. Holding saved names to it dropped "Live: 2024" made
on a Mac at the next launch, and the next save deleted it with its types
and sites.

## When done: what counts as done (2026-10-17, gap 83)

"When done" fires when nothing is left that dm will finish by itself. That
means nothing Running or Queued and nothing the schedule or download limit
holds, since those start again when the window reopens. Something must also
have finished or failed since the queue last ran dry. A row a person stopped
is theirs: it is left alone, not waited for, so it does not keep the action
from firing once the rest has finished. A Stop all that finished nothing is
not done, and the choice stays armed for the next stretch. Failed rows are
settled, because nothing retries them without a person and their parts stay
for Retry.

## Login item and host manifests follow the installed copy (2026-10-17, gaps 84-85, 88-93)

On launch an installed release dm (no `DM_HOME`) rewrites the host
manifests when they name another copy or do not allow this version's
extension ids. A development build fixes only a missing one. The ids
already allowed are kept, including Firefox's retired
`dm@pranavo72bex.github.io`: that is harmless, and it keeps an old copy of
the extension working until it is replaced.

The login item is taken from another copy only when that copy's program is
gone. Otherwise two installed copies (a .deb and an AppImage) would take it
from each other at every launch. `--unregister` removes only this copy's
item, one whose program is gone, or one it cannot read, and it names the
copy it leaves alone. On Windows it calls `disable()` unconditionally,
because an item turned off in Task Manager reads as not enabled.

On macOS a copy is transient when it runs from AppTranslocation, or from a
read-only mount under `/Volumes` (a disk image). A writable external disk
is an install like any other. A transient copy, or one in Downloads,
Desktop or Documents, is never registered with the browsers or as the
login item.

## Free space is shared between downloads under way (2026-10-17, gap 94)

The free-space check (gap 60) is made against a ledger shared through
`Options::room`, not by each download alone. Each download that passes
claims what it still needs. A part's claim falls as its bytes arrive,
because the file system's free space then counts those bytes. A finished
file on another disk claims its whole length until the download ends. The
check and the claim happen under one lock, so two downloads starting
together cannot both pass against the same space.


## The shown byte count follows the speed limit (2026-10-17, gap 112)

Under a limit, a chunk is counted as the limiter lets it through, 16 KiB
at a time (`Limiter::take_counting`), not as it arrives. A connection
reads what the socket holds, up to ~256 KiB at once, and eight connections
counting on arrival put two megabytes in the count before the ceiling had
passed any of it: the speed and ETA showed twice the limit for seconds.
The count is still a ledger of what is on disk: a wait cut short by a
pause or cancel counts the rest of its chunk. Smoothing the meter over a
longer window was not chosen: it would hide the error, not remove it, and
slow the number's response to a real change.

## Linux openers are waited for (2026-10-17, gap 106)

`xdg-open` exits 3 or 4 at once when it has nothing to hand a file to; a
spawn that succeeded was taken as opened. Each opener is now waited for up
to 1.5 s, in place, so `open_file` keeps its `Result`. A non-zero exit
tries the next opener; one still running at the deadline is taken as
opening the file and reaped on a thread. Its stderr goes to a temporary
file rather than a pipe, so a program it runs in the foreground is not
killed writing to a closed pipe.

## A browser is registered when its program is installed (2026-10-17, gap 113)

Besides a browser whose profile folder exists, `register` and `ensure`
take one whose program is installed (Linux: on `PATH`; macOS: a bundle in
`/Applications` or `~/Applications`) and make its `NativeMessagingHosts`
folder. A browser with neither gets no folder. Windows writes every
browser's key regardless, as before.

## The toolbar keeps every name (2026-10-18, gap 115)

The modern toolbar used to keep one row by dropping names from the right,
leaving pictures to guess at a narrow window or in German. Now each button
keeps its name: beside its picture where one row has room, under it in
smaller type where it has not, and on a second row only when even that does
not fit. The classic looks wrap the same way.

## Progress windows for downloads the window did not start (2026-10-18, gap 109)

A row first seen running or queued that the window did not add itself (a
browser hand-off through the inbox, a second launch's link) opens its
progress window when "Show a progress window" is on. Rows the window adds
(Add URL, series, lists, imports) are marked as its own and follow the old
rule: one window for a single Start now, none for a batch.

## Exit asks; scripts do not (2026-10-18, gap 111)

Exit, Cmd+Q and the tray's Quit ask while anything is Running or Queued;
the downloads resume at the next launch either way. `program.exit` over the
control protocol quits without asking, since nothing there can answer.

## A download the browser has already finished is still caught (2026-10-18, gap 103)

`first_look` used to leave a download whose state was `complete` with the
browser ("already finished, usually from the cache"), and one that could
not be paused and was no longer running ("it finished first"). Chromium
finishes a small file served fast before the extension can hold it — a PDF
almost always (it is not held for a danger check the way a `.bin` is), and
a pause does not stop what it has already received — so those PDFs never
reached dm. A cache hit and a fast fetch look the same from the extension,
so the rule could not be kept for one and not the other, and it guarded
nothing a person asked for: the catch rules said dm takes the type. Now a
finished download is handed over by its address like any other, still
`held` (protocol 2), so dm probes it, compares the type the browser got,
and refuses what it cannot fetch itself (a one-time link, a POST result, a
sign-in page). Only once dm has taken it is the browser's copy removed:
`downloads.removeFile` for a finished one (cancelling it would leave the
file), `cancel` for one still running, then `erase`. Refused, the finished
copy stays, and `give_back` does not start it again. The size floor in the
catch rules is the setting for leaving small files to the browser.
Considered: leaving the browser's copy and handing over too (two copies of
every fast PDF, which is what the hold-then-complete race already did);
keeping the rule and lowering latency (the race is lost before
`onCreated` runs).
