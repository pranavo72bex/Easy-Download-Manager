# Implementation decisions

Decisions that shape dm and are not obvious from the code, newest first. Each says what was decided and why, so a later change
can see what it is undoing.

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
