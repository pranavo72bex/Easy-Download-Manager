# Implementation decisions

Decisions that shape dm and are not obvious from the code, newest first. Each says what was decided and why, so a later change
can see what it is undoing.

## 2026-10-27: Every type is caught out of the box

A download the browser starts is already a download (a page it navigates to
is not), so the type list only left files with the browser (`.txt`, `.json`,
an unlisted type) that a person wanted in dm. The default is now no list,
which `CatchRules::takes` already read as every type. A saved list that is
exactly the old defaults was never chosen and becomes empty
(`bring_up_to_date`, edition 2); a changed list, an Alt-click, a skipped site,
the size floor and a private window still leave a download with the browser.

## 2026-10-27: The panel's list for YouTube is read from the page's data

Measured: yt-dlp takes 2.6-4.5 s over a watch page (its `player_skip=js` saves
about a second but drops the addresses the download needs; `android_vr`,
`tv_simply` and `mweb` list one to five formats, and `web` and `tv` fail with
"page needs to be reloaded"). The watch page's own `ytInitialPlayerResponse`
carries every picture and sound with its size, in one request of 0.8 s. The
panel's list is read from that (`ytdlp/page_data.rs`, same labels, qualities
and order as `choices`, checked against yt-dlp on three videos); the SABR
streams carry no addresses, so the download is still yt-dlp's reading of the
page, started behind the list (`warm_reading`). Any page that is not
`status: OK`, is live or lacks the data goes to yt-dlp as before. Dropped: a
`player_client` pick for yt-dlp (fewer formats, and fails by video).

## 2026-10-26: yt-dlp reads a video page, dm fetches it

yt-dlp downloaded what it read on one connection a stream, so a video ran at
one connection's speed (1 MB/s from a server holding each connection to
that) while dm's engine sat idle. Now `ytdlp::download_page` asks yt-dlp
only what it would fetch (`-O "%()j"`, nothing downloaded, with the same
format choice, name template, sign-in and proxy as its own download) and
dm fetches each stream that is a plain file (`http`/`https`) with its own
engine, with the agent yt-dlp gave it (`prefer_agent`; a site can bind its
addresses to the client it gave them to), and joins picture and sound as
before (`finish_dm`). Measured with the real yt-dlp against that server,
40 ms away: 8 MB 8.69 s → 1.78 s, 32 MB 32.69 s → 4.77 s, the page's
reading included (`tests/throughput.rs`, `video_helper`).

Nothing that downloaded before can fail now. Streams in pieces (HLS, DASH
fragments), live ones and a site's own protocols stay yt-dlp's; where dm's
fetch is refused, yt-dlp downloads the same reading (`--load-info-json`),
so the page is not read twice, and from the page again if that reading no
longer serves. A refusal is remembered in the staging folder, so a resume
goes to yt-dlp at once. A pause keeps each stream's parts under its format
id; a resume reads the page again (addresses expire after hours) and the
engine adopts them by size and validator. Subtitles are fetched from the
reading's addresses and carried in as before. The reading can hold the
site's cookies: written for this account alone and removed after.

The page is read once, not twice. The panel's list (`list_choices`, in the
browser's host) keeps yt-dlp's whole `-J` reading for ten minutes in
`<home>/readings` (a file per page, named by a hash of its address, for
this account alone, the stale ones deleted at each write), and the
download a click starts has yt-dlp choose its format from it
(`--load-info-json`, 0.23 s here with the server gone) instead of reading
the site again (1.7 to 6 s on YouTube, measured 2026-10-22/25). A reading
that no longer serves is passed over for a fresh one. A fresh reading
skips YouTube's HLS manifests and translated subtitles, as the list's
does: dm fetches neither, and they were about a third of a reading's time.

A picture and sound the extension caught from a player and that refuse dm
(YouTube's 403 after about a megabyte without the player's token) now fall
back to their page through the same route, rather than failing the row.

Why not without yt-dlp, as IDM is: IDM reads the player's own requests in
the browser, which dm's extension already does (2026-10-25), and keeps up
with each site by shipping fixes every week or two (30-odd builds in a
year, most "resolved video recognition problems"). YouTube's web player
has since moved to SABR, whose requests name no format and cannot be
fetched by range; taking it over would mean writing YouTube's streaming
protocol (protobuf requests, UMP answers, its redirects and policies) into
dm, using the page's proof-of-origin token, and keeping it working alone.
yt-dlp is kept current by many and fetched afresh by dm weekly. Recording
what the player plays was weighed too: only the quality the player picks,
and only as fast as it plays.

## 2026-10-26: A speed pass, measured (`tests/throughput.rs`)

The engine was measured before anything was changed: 1.1 GB/s on one
connection and about 1.5 GB/s on eight over loopback on a two-core machine,
server included. Nothing between the socket and the disk holds a real link
back, so the write path, buffers and locks were left as they are. What did
hold downloads back was waiting:

- **Small files on one connection.** `MIN_SEGMENT` (1 MiB) holds a file under
  2 MB to one range, and with `workers = connections.min(planned)` nothing
  was free to halve it. Cutting the file finer at the start (256 KiB, as a
  first version did) took a 1.5 MB file from a server that holds each
  connection to 1 MB/s, 40 ms away, from 1.55 s to 0.40 s, but cost every
  fast distant server a round trip, since each extra range opens a
  connection (80 ms away, no cap: 512 KB and 1.5 MB 0.27 s → 0.35 s). So the
  ranges are planned as before, and the connections beyond them are
  *helpers* (`ranged::run`): each waits 100 ms, watches the bytes arrive for
  80 ms, and halves a range only when more than 0.3 s of the file is left at
  that rate (`slow_enough_to_help`). A fast server finishes before they
  decide and never sees them (the test counts connections); a slow one is
  helped within a fraction of a second. Against the same two servers, in the
  same benchmark (`DM_BENCH_SMALL=1`), before → now: capped, 1.5 MB 1.55 s →
  0.80 s, 3 MB 1.14 s → 0.80 s; uncapped, 0.27 s → 0.27 s and 0.35–0.45 s →
  0.44 s. The rate is watched over a window rather than since the start,
  which counts the wait for the first byte as slowness (a first try, with
  "nothing yet" taken as slow, made 3 MB and 6 MB at 80 ms slower). Two FTP
  tests that paused a 4 MB download at 0.4 s found it finished and fetch 8 MB
  now.
- **A hand-off to a running dm waited for the inbox poll** (every 500 ms).
  dm now writes the port of a loopback UDP socket to `inbox/bell`, and the
  host sends one empty datagram after filing a request (`host::ring`); the
  watcher looks at once. The datagram carries nothing, so anyone else
  sending one only makes dm read its inbox early, and a flood is held to a
  look every few milliseconds. The poll stays for an older host, a missing
  bell, or a socket that could not be had. A notifier on the directory was
  weighed again and dropped for the reason it was dropped before. From
  `download.start` to the server's first request (`scripts/launch-latency.py`,
  median of nine, macOS debug builds): 520 ms → 59 ms with dm running (Linux
  under Xvfb, median of five: 509 ms → 13 ms); with
  dm closed, starting it took 16 ms before and 20 ms after (noise).
- **A download added while others run waited for the scheduler's next
  tick** (50 ms). An add, a resume, a retry or a queue start now wakes it
  (`Queue::kick`); the tick stays as it was.
- **The probe's byte is read** when its answer is small, so its connection
  goes back to the pool for the first range. Most servers send that byte
  with the headers, and hyper reused the connection already; one that
  sends the body apart had its connection closed and cost the first range
  a handshake.
- **One timer per body** in the range loop (`hls::Reader`) instead of a
  `timeout` made and dropped for every chunk; a pause is still seen within
  a quarter of a second. Within the benchmark's noise on loopback; kept
  because it does less per chunk and changes nothing else.

Weighed and not done: fetching the first range on the probe's own request
(`bytes=0-`) would save a round trip before the first byte, but a server
that answers a range of the whole file with 200 (Wikimedia does) would then
read as one without ranges and lose every connection but one, and a second
request beside the probe meets servers that allow one connection with a
429 on the probe. A caching DNS resolver: Windows, macOS and systemd all
cache lookups already.

## 2026-10-25: Update manifests are signed; logins are exact-host and https

`latest.json` is signed with ed25519 (`latest.json.sig`, made in the release
job by `cargo xtask sign-update`); the public key is built into `update.rs`
and checked with `ring`, already in the tree. The private key was generated
by `cargo xtask update-keygen` and is kept by the owner, never committed: it
must be set as the repository secret `UPDATE_SIGNING_KEY` or the release
job fails. Saved site logins no longer cover subdomains by default (a leading
dot opts in) and are never sent over plain http except to localhost.
Quarantine on macOS is set through `setxattr` (libc), on files only.

## 2026-10-25: TorBox built, then removed at the owner's request

A TorBox integration (own API key, Add URL › Through TorBox, a row command;
`torbox.rs`) was built and tested against a stand-in and against the live API
with a bad key, then reverted: no account was available to prove a real
fetch, and the owner judged it not worth carrying yet. It is in git history
(commits `cbfa3c8`, `291fc09`) if it is wanted again. The 2026-10-24 drop of
debrid providers stands.

## 2026-10-25: Sharing a finished file was built twice and removed both times

**Share on Gofile** (a guest upload, its page link copied) was built on the
owner's request (commits `0d505d1`, `f4089e0`, `020e3dc`) and a real upload
worked. It was removed the same day because it did not do what it was for: a
Gofile link cannot be downloaded by dm. The link is a JavaScript page (3 KB of
HTML), and Gofile's content API answers `error-notPremium` to a guest
account: listing or fetching a folder through the API is closed to free
accounts, and the website gets round it with a token taken from its own
script. Using that token would be getting round a restriction Gofile put
there, and it would break whenever they change the site. Gofile's terms had
also never been readable.

**Share on this network** (commit `81d9e00`: one finished file served on the
computer's own address, for an hour, with byte ranges and an unguessable link)
was built in its place, tested (a container fetched it byte for byte, by
range and eight at once; real clicks in the window; a security review that
found and fixed two weaknesses) and then removed at the owner's word: sharing
is not part of a download manager's job, and it was the one feature that
opened a port for others to reach. Neither feature is left in the code, menus,
strings, privacy text or allowed addresses; both are in git history.

Not built, and not wanted unless the owner says so: sharing beyond one's own
computer. dm downloads; it does not host.

## 2026-10-24: The work list is empty: what was built, and what was dropped and why

`docs/gaps.md` held 58 open rows. Each is now either built (below) or dropped
here with its reason, so nothing is left half-listed. The rule for building
was the project's own: free, legal, nothing that needs a server of ours, and
nothing on by default that listens or sends. A dropped row is not rejected
for ever; it names what is missing — a person, an account, money, a security
design — so it can be picked up when that exists.

**Built** (each has its own entry below or above): ZIP extraction, a command
line with an opt-in setting to find a running dm, a watched folder, per-site
rules, batch rename and a file's SHA-256 in Properties, a recording time
limit, the FTP torn-part repair, and the Wayland close fix, checked under
sway and KWin in Docker.

**Dropped because it needs a person's own machine, account or release**, which
no automated run can stand in for: dm on a real Windows desktop with a GPU
driver, tray and SmartScreen (`windows-checklist.md` lists the 17 checks); a
real login on any system; VoiceOver and NVDA; a drag by hand on macOS or
Windows; Brave's fresh-profile automation; a Chrome context menu clicked with
real input; the updater against a published release; the x86_64 AppImage and
each installer on a person's desktop; the two CI jobs only a release tag starts;
the Firefox store listing, which waits in Mozilla's review; waking a sleeping
computer on macOS and Linux, which `pmset` and `rtcwake` do only for an
administrator.

**Dropped because it costs money**, and dm is free and stays free: Apple's
notarisation ($99 a year, no free route); Chrome Web Store's one-time fee, so
Chrome-family browsers load the extension unpacked or from Edge Add-ons. A
free Windows signing certificate exists (SignPath Foundation, for open
source) but is applied for by the owner, not by code; until then the README
says how to open an unsigned installer.

**Dropped for law, terms or privacy.** Debrid providers and Gofile: their API
terms could not be read, and the reference project the owner named uses keys
from each person's own accounts, one of them possibly paid; dm will not claim
a provider it has not tested under terms it has read. Cutting advertising or
sponsor segments out of a video: it touches a service's terms and
technical protections, detection from the video alone is unreliable, and a
lossless MP4 cutter is a project of its own. Catching downloads other programs
start would need a system-wide hook, which is invasive and not free of
privacy cost.

**Dropped because it needs a server or a new security design.** Sending a link
from a phone: only a LAN listener fits (a relay for a phone off the network
would be a hosted server). It would add an address and nothing else —
no queue control, files or credentials — with explicit pairing, a per-device
token that can be revoked, TLS or a pinned key before any token crosses the
network, a rate limit and a confirmation in the window. That design is
written down here so it is not reinvented, and not built.

**Dropped as technically not worth it, or not possible.** Kerberos (needs the
operating system's GSSAPI; Basic, Digest and NTLM are done); Arabic and
Hebrew layouts (egui lays text out left to right only, so it needs
bidirectional ordering inside its layout and a mirrored window); the "Download
video" panel on a site whose security policy forbids WebAssembly (inherent:
the menu still works); telling an advertisement from the programme on a
player (needs a site's own knowledge, which dm does not keep); rewriting
yt-dlp's site extractors and QuickJS (site logic is not dm's to write); a
browser that fetches a file again when it keeps a download dm refused
(Firefox's pause drops the connection); the one-name-for-every-package
command (`dm-desktop` in the .deb, `dm` elsewhere; the README says which).

**Left for later though free and possible**, because nothing needs them yet:
a schedule, retry count and When done for each queue (one for all today); a
button over a selection of links in the page; a stop time or start time for
one recording and a readout of its elapsed time and size; the hundred small
polish findings of the internal QA rounds (a column's arrow, a tooltip, a
window's size at 300% text), none of which loses a download or hides a
control. A defect found again is a new row.

## 2026-10-24: The command line finds a running dm by an opt-in setting

`dm-desktop list|status|pause|resume|cancel` reach the running dm over the
loopback control port. That port used to exist only under `--control`. Options ›
General now has "Let the command line list and steer downloads", off by
default, which opens the same port, with the same token only the account can
read, at every start. Off, dm listens on nothing, as before, and the command
says to start dm with the setting or `--control`.

## 2026-10-24: A watched folder

Options › Downloads names a folder dm reads every five seconds (even in the
tray or headless). A `.url`, `.webloc`, text file of links, or `.torrent` saved
there is read exactly as a drop is read (the first 64 KB of a text kind; a
torrent by its path), its links are added, and the file is moved into a `done`
folder inside it, so nothing is added twice and nothing is deleted. A link
already in the list is put aside without being added again. A file less than
two seconds old is left for the next look; symbolic links, hidden files and
other kinds are never read; a file with no link in it, or one that cannot be
moved, is said once and then left alone until dm restarts. Empty is off.

## 2026-10-24: Site rules apply when a row is added

Options › Network lets a host (and its subdomains, the most specific winning)
have its downloads saved to a folder and held to a speed of their own. The rule
is applied at the doors every row comes through (typed, clipboard, drop,
browser hand-off, the command line, the launch-time inbox) by one function,
`Settings::with_site_rule`, and a folder or speed a person chose for that one
download stands over the rule. The speed is the row's own limit (`speed_limit`
on `AddOptions`), so it is kept with the row and shown where a row's own limit
already is. A relative folder in a rule is ignored.

## 2026-10-24: Batch rename by a pattern, and the SHA-256 in Properties

Move/Rename of several finished files takes a name pattern: `{n}` counts from 1
in the order listed, `{name}` is the old name, the ending (and `.tar.gz`) is
kept, and empty keeps the names. Properties of a finished file shows its
SHA-256, worked out on the app's runtime so the window never waits, and a box
that checks it against a digest the person was given (MD5, SHA-256 or SHA-512,
told by its length, in any case).

## 2026-10-24: FTP parts are checked at their ends when a download resumes

HTTP re-reads a part's last 64 KiB on resume and cuts the part back to the last
byte that agrees; FTP trusted the part's length, so a part torn by a power cut
(zeros at its end) was kept and the file came out the right size and wrong in
the middle. FTP now does the same once per range per run, over a short extra
session (`REST` to 64 KiB before the end), and not at each retry of its own
writing. A server that cannot be asked is left to the fetch to report.

## 2026-10-24: The ZIP crate is 9, and the advisory check gates it

`zip` 8.6 carried three advisories about `ZipArchive::extract`, which dm does
not call (it reads and judges each entry itself), but `cargo audit
--deny warnings` fails the Audit workflow on any. It is `zip` 9, whose entry
names are fallible: a name that is not text refuses the archive. The same run
brought `tokio` and `clipboard-win` past a new advisory and a yanked version.

## 2026-10-23: ZIP extraction refuses whole archives and never replaces

Every entry is judged before any is written: a path that leaves the folder
(`..`, absolute, drive, backslash, a `:`), a link or device file, a password, a
repeated name in any case, a file that is also a folder, and totals of
entries, bytes and compression ratio past the limits fail the archive. The
bytes actually written are counted against the sizes the header claims. The
folder is always new beside the archive (or inside a folder the person picks),
so there is no overwrite question to ask; the archive is kept and nothing
extracted is opened or run; a failure or a cancel removes what was made. Only
Deflate and Stored entries: the crate is built without its C-backed codecs.

## 2026-10-23: Closing the window on Wayland minimizes it

A program cannot hide or unminimize its own window on Wayland (winit's
`set_visible` does nothing there), so close-to-tray cannot hide it. Marking the
window hidden anyway stopped dm drawing a window the compositor still showed
(a frozen ghost, KDE Plasma). dm minimizes on a Wayland display and keeps
drawing; the task manager restores it, which the tray's Show cannot. Checked
under sway and under KWin 6.8.1 in Docker (`linux-kde-close-check.sh`), where
the check fails on the original code.

## 2026-10-23: A recording's time limit is one setting

Not a field of each row: that would be a new column of the saved queue and a
migration, which one number in Options does not need. It stops the recorder
through its own control so the file is closed, instead of dropping the task
mid-write.

## 2026-10-23: Source is split by concern, and a file has a size limit

`app.rs`, `dialogs.rs`, `download.rs`, `queue.rs`, `main.rs` and the window
tests were each one file of thousands of lines, found by reading the code
rather than by any rule. They are now directories of files by concern
(`src/app/`, `src/dialogs/`, `dm-core/src/download/`, `queue/`, the window
tests under `tests/window/`), moved verbatim: the only edits are `pub(super)`
on what now crosses a file, and rustfmt's rewrapping of the longer lines.
Behaviour, the window, and the tests' names are unchanged.

Two rules keep it that way. A source file over 2500 lines fails `cargo xtask
layers`. A module's tests go in `tests.rs` beside it, not inline, and the
privacy and translation scans skip a file by that name (before, they stopped
reading at `#[cfg(test)]`); a scan that read a moved test file as shipped code
would have taken its made-up addresses and strings for the product's. Struct
definitions stay in the parent module, so the fields their children use need
no widening.

## 2026-10-22: Arabic and Hebrew wait for bidirectional text in egui's layout

egui 0.37 shapes Arabic (harfrust) but lays every line out left to right in
stored order, and has no paragraph direction (epaint's `unicode.rs` still
says "TODO: heed bidi characters"). dm reorders single-line names itself
(`bidi.rs`). An interface in Arabic or Hebrew cannot be done the same way:
a message that wraps must be reordered line by line after it is broken,
which only the layout can do, and reordering strings before they reach
egui would hand screen readers the words backwards. The way there is a
vendored epaint, as winit is vendored: the Unicode bidirectional algorithm
run per laid-out row (reorder its glyph runs by level), right-aligned rows
for a right-to-left paragraph, then the window mirrored (the places on the
right, the toolbar and columns from the right) and the two catalogs. Until
then dm offers the eleven left-to-right languages it has.

## 2026-10-22: Mica is opt-in, off by default, from the next start

Options › General offers "Mica" on Windows 11 22H2 and later (build
22621, read from the registry) and nowhere else. Chosen, the next start
makes the window see-through, clears its frame (`theme.rs` makes the
`frame` token transparent; the list, dialogs and fields stay solid to be
read on), asks DWM for the main-window backdrop and extends the frame over
the client area (`chrome.rs`). Read once at start, since a window is made
see-through only when it is created. Off by default for the reason below:
an old GPU driver or a desktop with no compositor can draw the window
black, and it has not been watched on a real Windows 11 desktop. Someone
who sees black turns it off in Options; deleting `"mica": true` from the
settings file does the same.

## 2026-10-22: The title bar takes the window's colour; no Mica backdrop yet

Fluent's windows read as one surface from title bar to content. On Windows
dm tells the window's frame its look through DWM: dark mode where the look
is dark, and the caption in the window's own background colour, its words
in the text colour (`chrome.rs`). Mica itself (`DWMWA_SYSTEMBACKDROP_TYPE`)
shows only through a see-through window, which means drawing egui's
panels with a transparent clear colour; where the compositor or an old GPU
driver cannot, the window comes out black. That stays off until it can be
watched on a real Windows 11 desktop.

## 2026-10-22: dm does not get around copy protection

dm has no DRM remover and never will: no licence-server requests, no key
extraction, no content-decryption-module hooks. What it decrypts is HLS's
own AES-128 scrambling, whose key the server hands to any player that asks
(as every player and standard decoder reads it). A stream whose key belongs to a rights
system is refused as copy-protected: SAMPLE-AES, any `KEYFORMAT` other than
`identity` (FairPlay, Widevine, PlayReady), or a key address dm could only
reach through one (`skd://`, a `data:` licence). yt-dlp is never given
`--allow-unplayable-formats`, and its own "DRM protected" refusal reaches the
row in plain words. In the browser, a player the page plays through EME (its
`mediaKeys` set) gets a "Protected video" bar that says dm does not download
protected video and hands nothing over.

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
an MP4, kept as `.ts` when no converter is installed, a digest names the `.ts`
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
An openly licensed typeface is embedded (Inter since 2026-10-18; see below). Fonts given to egui take
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

## Free space is checked once, at the start (2026-10-17)

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

## A refusal's body is said only when it says something (2026-10-17)

"the server answered 404 Not Found" carries the canonical reason. The body
is read down to its text as before, and then dropped when, without the
status, its reason phrase, filler words and a server signature, nothing is
left — the boilerplate pages of Python's http.server, nginx and Go. A body
with words of its own (a rate-limit sentence) is kept.

A file system that reports no total size (some FUSE and SMB mounts answer
0 free of 0) is taken as unknown and not checked.

## A folder name: usable here to load, portable to make (2026-10-17)

`one_folder` decides whether a saved category or kind's folder is loaded
and used: one folder, not `.` or `..`, and on Windows a name Windows can
make. `portable_folder` adds every computer's rules (no `<>:"|?*` or control
characters, no trailing dot, no device name) and is applied only to a name
being made or renamed. Holding saved names to it dropped "Live: 2024" made
on a Mac at the next launch, and the next save deleted it with its types
and sites.

## When done: what counts as done (2026-10-17)

"When done" fires when nothing is left that dm will finish by itself. That
means nothing Running or Queued and nothing the schedule or download limit
holds, since those start again when the window reopens. Something must also
have finished or failed since the queue last ran dry. A row a person stopped
is theirs: it is left alone, not waited for, so it does not keep the action
from firing once the rest has finished. A Stop all that finished nothing is
not done, and the choice stays armed for the next stretch. Failed rows are
settled, because nothing retries them without a person and their parts stay
for Retry.

## Login item and host manifests follow the installed copy (2026-10-17)

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

## Free space is shared between downloads under way (2026-10-17)

The free-space check is made against a ledger shared through
`Options::room`, not by each download alone. Each download that passes
claims what it still needs. A part's claim falls as its bytes arrive,
because the file system's free space then counts those bytes. A finished
file on another disk claims its whole length until the download ends. The
check and the claim happen under one lock, so two downloads starting
together cannot both pass against the same space.


## The shown byte count follows the speed limit (2026-10-17)

Under a limit, a chunk is counted as the limiter lets it through, 16 KiB
at a time (`Limiter::take_counting`), not as it arrives. A connection
reads what the socket holds, up to ~256 KiB at once, and eight connections
counting on arrival put two megabytes in the count before the ceiling had
passed any of it: the speed and ETA showed twice the limit for seconds.
The count is still a ledger of what is on disk: a wait cut short by a
pause or cancel counts the rest of its chunk. Smoothing the meter over a
longer window was not chosen: it would hide the error, not remove it, and
slow the number's response to a real change.

## Linux openers are waited for (2026-10-17)

`xdg-open` exits 3 or 4 at once when it has nothing to hand a file to; a
spawn that succeeded was taken as opened. Each opener is now waited for up
to 1.5 s, in place, so `open_file` keeps its `Result`. A non-zero exit
tries the next opener; one still running at the deadline is taken as
opening the file and reaped on a thread. Its stderr goes to a temporary
file rather than a pipe, so a program it runs in the foreground is not
killed writing to a closed pipe.

## A browser is registered when its program is installed (2026-10-17)

Besides a browser whose profile folder exists, `register` and `ensure`
take one whose program is installed (Linux: on `PATH`; macOS: a bundle in
`/Applications` or `~/Applications`) and make its `NativeMessagingHosts`
folder. A browser with neither gets no folder. Windows writes every
browser's key regardless, as before.

## The toolbar keeps every name (2026-10-18)

The modern toolbar used to keep one row by dropping names from the right,
leaving pictures to guess at a narrow window or in German. Now each button
keeps its name: beside its picture where one row has room, under it in
smaller type where it has not, and on a second row only when even that does
not fit. The classic looks wrap the same way.

## Progress windows for downloads the window did not start (2026-10-18)

A row first seen running or queued that the window did not add itself (a
browser hand-off through the inbox, a second launch's link) opens its
progress window when "Show a progress window" is on. Rows the window adds
(Add URL, series, lists, imports) are marked as its own and follow the old
rule: one window for a single Start now, none for a batch.

## Exit asks; scripts do not (2026-10-18)

Exit, Cmd+Q and the tray's Quit ask while anything is Running or Queued;
the downloads resume at the next launch either way. `program.exit` over the
control protocol quits without asking, since nothing there can answer.

## A download the browser has already finished is still caught (2026-10-18)

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

## Chrome's file name answer is held while dm decides; `scripting` added (2026-10-18)

Chrome asks where to save a download (its Save dialog) when the name is too
long for the disk, or when "Ask where to save each file" is on, and it asks
only after every `downloads.onDeterminingFilename` listener has answered. A
dialog opened while dm was being asked stayed open after the extension
cancelled the download it had taken, and caught the next clicks. So the
extension now listens there (Chrome only; Firefox has no such event) and
holds its answer until `onCreated` has decided: a download dm takes is
cancelled before Chrome asks anything, one it leaves is answered with no
suggestion, as if dm were not installed. The hold lasts at most
`DETERMINE_WITHIN_MS` (handshake, settings and hand-off times plus 5 s) and
is then answered whatever happened. Considered: suggesting a shorter name
(fixes only the long name, not the "ask" setting); cancelling sooner (the
dialog can open before the app has answered).

"Download all/selected links" asks the page script for the page's links;
Chromium puts declared content scripts only into pages loaded after the
extension, so a tab open before dm was installed, updated or reloaded had
nobody to answer. The extension now puts `content.js` into that frame with
`scripting.executeScript` and asks again. That needs the `scripting`
permission (both manifests); with `<all_urls>` already granted it adds no
install warning in Chrome, but the Firefox listing needs a new version.
`content.js` replaces a predecessor cut off by a reload rather than
deferring to it. Pages no extension may enter (the browser's own, stores)
get a message saying so.

## Dialogs in front are modal; Remove always asks (2026-10-18)

The dialogs that stand in front of the list (every one but Find, the
progress windows and Download complete) are egui's modal layer: Tab goes
round their own controls and the menu bar and list behind take no clicks
until they close, which is what "focus stays in the dialog" takes in egui.
Find stays beside the list, since finding selects rows a person may then
click. Enter with none of the dialog's controls focused presses its default
button; a focused button of its own takes Enter itself. Remove now asks for
finished rows too, whichever way it was asked for — the Delete key took
them off at once while the toolbar asked — except headless, where nobody
can answer. Considered: asking only from the key (two behaviours for one
command).

## Non-Latin names use the computer's fonts, read on demand (2026-10-18)

Inter and egui's fonts have no Chinese, Japanese, Korean, Arabic, Hebrew,
Thai or Devanagari. Bundling Noto CJK would add tens of megabytes to a
7.9 MB installer, so the computer's own font for the script is read the
first time a row's name, path or note needs it (fontconfig's answer on
Linux, the usual files on macOS and Windows) and appended after dm's fonts.
A computer without one draws what it can, as before. The files are read
into memory (no memory-mapping crate in the window), once a process.

## Starting one row outside the schedule (2026-10-18)

`Queue::start_outside_schedule` starts the row at once and marks its slot so
the schedule and the download limit do not put it down; a pause, a cancel
or the row going back in line clears the mark. The mark is not saved: after
a restart the row is the schedule's again. Considered: turning the schedule
off for a while (holds every other row's start too).

## In progress is what is under way (2026-10-18)

"In progress" lists running, queued and schedule-held rows only, matching
the status bar's count. Failed and stopped rows are under All and their
kind, not here.

## Every opening of dm registers with the browsers (2026-10-18)

Registration ran only in the copy that opened the window, so opening dm
again while it ran (what the README says to do after installing a browser)
handed off and exited without telling the new browser. Now a launch that
hands off registers first, and the running dm checks again whenever it is
asked to show itself. `ensure` is idempotent and reads a few small files,
so it costs nothing when nothing changed. A failure is said in the window
once until it changes. The extension with no host cannot know whether dm
runs, so its words cover both: open dm, again if it is open.

## Firefox's downloads panel is left to Firefox (2026-10-18)

Firefox opens its panel for every download it starts. Measured: cancelling
and erasing inside `onCreated`, before asking anything, keeps it shut, but
dm may cancel only after the app has taken the download (never lose one),
and the app's answer comes later than the panel. No extension API closes
the panel or sets `browser.download.alwaysOpenPanel`. Considered and
dropped: cancelling at once and restarting the download if the app refuses
(a restart is a second request — a one-time link or a POST is lost, and the
restart is caught again); erasing at once and cancelling later (an erased
download leaves the list the extension's pause, cancel and resume act on,
so it could not be given back; reasoned from Firefox's code, not run). The README says the panel may show and dm has the download.


## Right-to-left names reordered before egui draws them (2026-10-18)

egui 0.37 has no bidirectional layout: a line is laid out in stored order,
and its shaper (harfrust, guessing each font run's direction) turns each
Arabic or Hebrew word round, but not the order of the words. dm does not
wait for egui: `bidi::visual` runs the Unicode bidirectional algorithm
(`unicode-bidi`, a left-to-right line, as dm's window is) and hands egui the
runs in drawing order, keeping each right-to-left word in stored order so
the shaper's own turn is not undone, and mirroring brackets. It is applied
only where a name or a path is drawn; text a person edits is left alone,
since a reordered string in a text field would be edited backwards. A
screen reader is given the stored name. The one case it gets wrong is an
Arabic-Indic number touching an Arabic word in the same font, which egui
shapes as one run; a name with one is rare.

## A selected row is filled with the accent, its words white (2026-10-18)

The light grey selection (235 on 255) was invisible on many screens. The
list's selection is now a 3:1 fill with 4.5:1 words, the way desktop file
lists draw it, and every coloured word on a selected row (red Error, faded
File missing, the progress bar) takes the selection's text colour. Text
selection in fields keeps the soft tint. The focus ring gained a white inner
line, which is what shows on a primary button or a selected row, both in the
accent colour; the colours are tested as numbers in `theme.rs`.

## Scroll bars that take their room and can be seen (2026-10-18)

egui's default scroll bars float over the content and, in dm's looks, are
invisible until the pointer is over them: their handle is drawn in a
button's fill, which the looks keep clear, and at zero opacity while
dormant. A tester found Options › General cut off with no sign of more, and
End leaving the list's last row under the sideways bar. The list and the
Options tabs use `theme::steady_scroll`: solid bars that take their own
room, the handle in the text colour at a low opacity even when the pointer
is elsewhere. Other scroll areas (small dialogs that rarely scroll) keep
egui's.

## An enabled proxy with no host is a refusal, in the engine too (2026-10-18)

`Proxy::url` is `None` for a proxy turned on with no host or port, and
`client_with` used to build a direct client from that: every download went
out around the proxy someone had asked for. `client_with` now refuses it,
which is what the launch path already turns into "the saved proxy could not
be used, so it is turned off" for every route (dm's own client and the
tools), said in the window in its language. Options refuses it before it
gets that far. The native host's pre-check (`host.rs`) still falls back to a
direct client when its proxy cannot be built; it only asks the server
whether a caught download can be fetched, but it is a direct request, and
is left for a change of its own.

## Link drags from a browser: a hint, not a fix (2026-10-18)

winit delivers dropped files only, and reports nothing while a link is
dragged over the window, so dm cannot tell that a link was dropped on it.
What it can do: a drop holding no link says how to add one (Add URL, the
browser's right-click menu, or a dropped `.url`/`.webloc` file), and the
empty list says the same.

## A hand-off of a listed address is said, not asked (2026-10-18)

Add URL asks before adding an address already in the list. A browser's
hand-off cannot be asked about the same way: the native host files it in the
inbox, the inbox adds it to the queue (`shell::watch_inbox`, before the
window hears of it), and the browser has already cancelled and erased its
own download. A question then could only throw the download away, and a
window in the tray would hold it unanswered. So the window says it in the
status line — "Added file.zip again: that address was already in the list."
— where a person who did not mean it can remove the second row.

## Plurals: forms divided by `||` (2026-10-18)

A sentence about a count holds its forms in one catalog key, divided by
`||` ("Added {} download.||Added {} downloads."); the translation holds its
language's own forms the same way, and `i18n::form` picks one by CLDR's
integer rules: English, German, Spanish, Italian, Dutch two (one, other),
French and Portuguese two with 0 singular, Polish and Russian three, and
Indonesian and Turkish one. Separate keys per form would not have served
Polish and Russian. `tests/i18n.rs` holds every form to the English's
placeholders and every language to its number of forms.

## The list draws its own up-and-down bar (2026-10-18)

egui_extras' table has no sideways scrolling, so the list sits in a sideways
scroll area, and the table's up-and-down bar was at the columns' far edge,
out of sight whenever they ran past the window. The table's bar is hidden
and the list draws one of its own in a strip at the visible right edge: it
follows the table's offset, and a drag or a wheel on it is handed to the
table on the next frame. The sideways offset egui keeps across launches is
reset to the left edge on the first frame.

## Only Exit quits (2026-10-18)

The Exit question used to close dm by itself once nothing was running,
reasoning that there was nothing left to ask about. A person reading the
question, or reaching for "Keep downloading", saw dm vanish. dm now quits
only on the person's Exit (or a script's or a signal's explicit quit):
when everything finishes under the question, it changes to "Nothing is
downloading any more. Exit dm?" with Exit and Stay. The question keeps one
width and the room of three lines, because a dialog anchored by its middle
that shrinks moves its buttons, and Exit had slid under the pointer.

## A `.bin` is Other (2026-10-18)

`.bin` was sorted under Apps. It is as often firmware, a disc image, a
model's weights or a raw dump as a program, and the extension cannot say
which; the server's type (`application/octet-stream`) cannot either. Other
is the honest place. A name with "setup" or "install" in it says it is an
installer, and goes to Apps. The browser's catch list still catches `.bin`.

## Key names are catalog words with a `key:` prefix (2026-10-18)

German keyboards say Strg, Umschalt and Entf. The shortcut text is built by
dm rather than egui on every system without the Mac's symbols, from
catalog keys such as `key:Delete`: the prefix keeps "Delete" the key
("Entf") apart from "Delete" the command ("Löschen"), and is taken off
where no catalog has the word (English).

## The pointer over controls is the desktop's, not a web page's (2026-10-18)

The arrow over buttons, checkboxes, menu items, the toolbar and rows; the
text cursor over a field; resize arrows over a column's edge and the rail's
splitter. A hand over every clickable thing is a web page's convention, and
egui can only give it to plain buttons, so turning it on would have left
checkboxes, menus and rows with the arrow beside buttons with the hand —
the inconsistency a person notices. `the_pointer_is_the_same_kind_over_the_same_kind_of_thing`
holds it. Addresses are drawn in the window's one font, not a second
monospace one.

## Inter is the window's typeface (2026-10-18)

It replaces the earlier embedded face, which had no Cyrillic or Greek: a
Russian window drew those letters in egui's fallback, a second face with no
semibold, so headings lost their weight and one label could mix two faces.
Inter (SIL OFL 1.1, `fonts/LICENSE-Inter.txt`) covers Latin, Cyrillic and
Greek in regular and semibold, and is an independent design rather than a
stand-in for any vendor's system font. Scripts it lacks still fall back to
the computer's own fonts.

## Numbers are drawn with tabular digits (2026-10-18)

Inter's digits are proportional, so a column of sizes was ragged and a
speed jittered as it changed. Inter has tabular figures, but behind its
`tnum` feature, and egui applies no optional font features. So
`scripts/tabular-digits.py` (fonttools) makes those figures the drawn
digits in a copy cut down to digits and the punctuation numbers use,
named "Inter Tabular Digits" (9.5 KB; the OFL allows modified versions and
Inter reserves no name). It leads the `numbers` family, Inter behind it for
letters and units, used for the list's Size, Time left and Speed, the
status words and the progress window's figures. Making every digit
tabular was not chosen: in names and sentences tabular "1" reads gappy.

## Links that work once are named by the person, not guessed (2026-10-18)

The native host asks a caught download's server once before taking it, so a
link only the signed-in browser can fetch stays with the browser. For a site
whose links work a single time, that question spends the link. Detecting
such links from their address (tokens, signatures) guesses wrong both ways,
and reusing the check's connection for the download would mean the host
fetching files. So the person lists those sites in Options › Browser, and a
hand-off from one is taken without the question, knowing a sign-in-only link
from it then fails in dm instead of finishing in the browser.

## Only questions dim the window (2026-10-18)

Remove, Exit, Cancel this download and the other questions dim the window
behind them and take every click until answered: an answer meant for one
must not land on something else. Dialogs that set things (Options,
Scheduler, Speed limiter, Properties) do not: the list stays readable
beside them, and nothing is lost by clicking past one. A tester asked for
the Scheduler and Speed limiter to dim like the questions (UI round 8);
they are settings, so they stay as Options is.


## The Queue column starts at one width (2026-10-19)

The list's columns fit themselves to the window at each launch until a
person drags one. Queue started at 96 points in a list where no row had a
queue and at 140 in one where some did, so the columns after it moved
between launches and "Time left" was cut to "T" (UI round 11). It now
starts at 116 either way. An empty Queue column no longer gives its room to
the name: a list that stays where it was is worth more than 24
points of name. Widths a person dragged are kept until the text size
changes, when the list fits itself again: kept at 140% text, Status and
every column after it were out of view.

## Behind a proxy, a torrent finds peers through http(s) trackers only (2026-10-19)

A SOCKS5 proxy carries TCP connections; the DHT, local service discovery
and `udp://` trackers are UDP, and the torrent engine sends them directly.
Left on, they told the swarm the info-hash from the real address on a
machine set up to make no direct connection. Behind a proxy they are off,
and a torrent's `udp://` trackers are dropped from it (its info dictionary
kept byte for byte, so it stays the same torrent). A magnet or a torrent
that lists only `udp://` trackers is refused with that reason rather than
sent around the proxy. An HTTP proxy is still refused for torrents
altogether, as before.

A torrent shares the whole speed ceiling while none of dm's own downloads
is running; with one running, the ceiling is split as before, one part for
dm's own downloads and one for each torrent or yt-dlp run.

## The modern looks follow WinUI 3's Fluent 2 (2026-10-19)

Asked for twice: the window follows Fluent 2 as WinUI 3 draws it, with the
values read from WinUI's own open-source control templates
(microsoft-ui-xaml's `*_themeresources.xaml`), the ones the WinUI 3 Gallery
renders with. dm is not presented as a Microsoft product and does not ship
Segoe UI (not redistributable); Inter stands in.

What was taken, and where it was bent:

- Brushes: the base, layer and dialog surfaces, text primary/secondary/
  disabled, the strong and soft strokes, the accent fills from the default
  accent (#0078d4), status colours, and the subtle fills for hover and
  selection, kept see-through as WinUI's are. Two were darkened a shade to
  hold dm's contrast floors on the list: the strong stroke (#8d8d8d to
  #8a8a8a, 3:1) and disabled text (#9f9f9f to #9a9a9a, 2.5:1). The hover test
  now asks only that hover changes something, since WinUI's subtle fill is
  about 1.1:1 by design; the selection is told apart by the accent pill.
- A selected row is the subtle fill with the accent pill, not the solid
  accent of the selected row: the pill is 3:1 on the list and the words keep their
  colours. Selected words in a field stay on the accent (#0078d4), white.
- Density: WinUI's compact sizes (24 px fields and drop-downs, 32 px list
  rows and places), which WinUI offers for information-dense apps; the
  standard 32/40 px pushed Options' and Properties' content out of reach at
  large text.
- Dialogs: no egui title bar; the title painted at the top left in the
  Subtitle style, a subtle close button, 24 px padding, and the buttons on a
  footer band at the right. The title is painted rather than a label so a
  screen reader is not given the dialog's name twice; a scrolled body
  scrolls under the title, which keeps the close button in reach.
- Shadow: Fluent's 28 for dialogs rather than 64, which is large for a
  window this small.
- Radio buttons stand in a column under their group's name, as WinUI's
  RadioButtons do; side by side, egui shrank a choice's words to a letter a
  line at 200%.

Tried and dropped: egui's experimental style providers (they cannot tell a
ticked box from an empty one, and they sit behind a feature flag), and
right-aligning the footer from last frame's window size (it fed back on
itself and grew Options past the window at 200%).

## The page script loads its WebAssembly only where there is media (2026-10-20)

The "Download video" panel is the only thing the page's WebAssembly does,
and it was loaded into every page and every frame: on a page of advert
frames, one compile of half a megabyte each. `content.js` now loads it once
the page shows media: a `<video>` or `<audio>` in the document (checked as
the page changes, at most once a second), one in a component's shadow root
under the pointer (checked at most twice a second), or a play. The
key-held clicks, magnet links and "Download all/selected links" stay plain
JavaScript and work from the start, as before.

## Segoe UI from Windows, Selawik elsewhere (2026-10-20)

Fluent's face is Segoe UI (Segoe UI Variable on Windows 11), which
Microsoft licenses with Windows: dm may not ship it. Chosen by the person:
on Windows dm draws with the computer's own Segoe UI and its semibold
(`%WINDIR%\Fonts\segoeui.ttf`, `seguisb.ttf`, mapped, as WinUI uses the
system's); on macOS and Linux, and on a Windows without them, with Selawik,
the open stand-in Microsoft made to Segoe UI's measure for other systems
(SIL OFL 1.1, Reserved Font Name, shipped unmodified, 44 KB a face). Selawik
has Latin only, so Inter stays behind it for Greek and Cyrillic (Russian,
in the translations); its digits are one width, like Segoe UI's. The
static Segoe UI is used rather than the variable one, whose weights egui
cannot pick. Not yet seen on Windows.

## Fluent UI System Icons for the window's pictures (2026-10-20)

Segoe Fluent Icons, WinUI's icon font, comes with Windows and may not be
shipped. Chosen by the person: Microsoft's Fluent UI System Icons (MIT), the
same icons published for other platforms, as a font cut down to the 26 the
window draws (`fonts/FluentSystemIcons-dm.ttf`, 5 KB, made with fontTools'
`pyftsubset`; `icons::fluent` names each by its `ic_fluent_<name>_20_regular`).
The modern look draws the toolbar, every place (each its own, as the WinUI 3
Gallery's pane has them), the pane's group chevrons, drop-downs' and
submenus' chevrons from it; the classic look keeps its drawn pictures. The
progress window's Connections opener keeps its drawn chevron, which turns
with egui's opening animation where a glyph would jump.

## Inter's tabular digits removed (2026-10-20)

`Inter-TabularDigits.ttf` led the numbers family while Inter, whose digits
are proportional, was the window's face. Segoe UI and Selawik, which lead
it now, have digits of one width themselves, so it was never reached and
is gone; the numbers family is the window's face and what follows it.


## A login launch makes its window only when asked (2026-10-20)

eframe makes the window and its GL context as it starts, about 40 MB of
the 43 a dm waiting in the tray held. A `--hidden` launch now runs winit's
event loop itself (`run_hidden`, `eframe::create_native`), with dm's logic
on a stand-in egui context (`run_logic`: no fonts, no drawing) whose
repaint observer wakes the loop; the first ask for the window passes the
loop to eframe, the window's state to its creator, and the stand-in's
pending viewport commands (a control request's screenshot, the focus) to
eframe's context, which every later wake of the stand-in is forwarded to.
A normal launch keeps `run_native`. Tried and dropped: the wgpu renderer,
which reached the first frame no sooner and held as much.

## 16 KiB write buffers (2026-10-20)

tokio's file keeps its own copy of each write, so a connection held its
`BufWriter` twice: 256 KiB cost 20–30 MB over ten downloads of eight
connections against 16 KiB, which fetched 2 GB over loopback no slower
and with no more CPU (measured on 2026-10-20).

## NTLM sign-ins (2026-10-20)

NTLMv2 is written in dm (`ntlm.rs`, MD4 from RustCrypto, HMAC-MD5 over the
`md5` already used for Digest) rather than taken from a crate: the
algorithm is short, its worked example in MS-NLMP pins it, and the crates
on offer bring signing and sealing dm has no use for. It is answered under
NTLM and under Negotiate, which Windows servers accept. Only the NTLMv2
answer: LM and NTLMv1 are broken. NTLM signs in a connection, which
reqwest does not expose; each reply is read to its end so the connection
returns to the pool, whose next request takes it again, and an answer
that still lands elsewhere (ranges signing in at once) is tried once more.
Kerberos is not offered: it needs the system's GSSAPI or SSPI and a domain
to be tested against.

## RTMP, RTSP and MMS through an external converter (2026-10-20)

These are media servers' protocols, not files: dm has no client for them
and does not write one. The converter dm already finds for joining
streams reads all three, so an address of these schemes is handed to it
with `-c copy`: nothing is re-encoded. MP4 is written fragmented
(`frag_keyframe+empty_moov`), so a live stream stopped by hand still
plays; MMS, which carries Windows Media, goes into its own container. A
stream has no length and no position to resume from: progress is the
bytes written, and a resume starts it again. That converter cannot go through
dm's proxy for these, so with one in force the download is refused.

## yt-dlp and a video tool fetched by dm itself (2026-10-21)

YouTube is to work out of the box, with nothing installed by hand. dm still
writes no site logic of its own: it runs yt-dlp. Where the computer has no
yt-dlp (or converter), the first video page that needs one has dm fetch it
into `<dm home>/tools`, through dm's proxy like any download: yt-dlp's own
standalone build from its GitHub releases, checked against the
`SHA2-256SUMS` published with it (on macOS the unpacked build, which starts
in 0.3 s where the single-file one took 8–14); the converter as one file from
a static-build project's b6.1.1 release on GitHub, checked against digests written
into dm, since that project publishes none. Fetched yt-dlp is fetched
afresh once a week. One the person installed is preferred, wherever it is.
Not bundled in the installers: a bundled yt-dlp goes stale between dm
releases, and both would add 100–150 MB to every installer. Not written as
dm's own extractor: that is the site logic this project does not write.

## A video page's session goes with its video (2026-10-21)

Measured on 2026-10-21: YouTube's streams stop with a 403 after about 1 MB
unless the client holds a proof-of-origin token, and an address that has
asked too often is told "Sign in to confirm you're not a bot". yt-dlp gets
past both with a JavaScript runtime (Deno, now fetched with it) and the
browser's session. So the extension sends the page's cookies with every
video handed over (the panel, "Download media with dm") and with the
panel's quality list, whether or not the site is in the popup's sign-in
list, and the `cookies` permission moved from optional to required (it
carries no install warning in Chrome or Firefox). Bounded the way a kept
sign-in is: to the page's own server, in a private file of yt-dlp's run
removed when it ends, and — unlike a kept one — dropped from the row once
the video is downloaded (`Start.cookies_once`, `Entry.cookies_once`).
Nothing from a private window. Dropped: writing a token generator of dm's
own, which is the site logic this project does not write.

## dm joins a video's picture and sound itself (2026-10-21)

A separate video tool was fetched (40–80 MB) for every video page only to join a picture
and a sound that yt-dlp had already fetched. `mp4mux.rs` does that join in
Rust: each input's samples are read from its index — a plain `moov`, or the
`moof` fragments a DASH server's files carry — and written as one plain MP4
with its index first and picture and sound interleaved a second at a time,
the samples copied byte for byte. A DASH picture with B-frames and no edit
list is given one that starts it at its first shown frame, as the converter's own
join does (it played 67 ms behind its sound without). With no converter, yt-dlp
is asked for an MP4 picture and an M4A sound (`bv[ext=mp4]+ba[ext=m4a]`),
leaves them side by side (its default `ignoreerrors='only_download'` turns
the missing merger into a warning), and dm joins them under the name yt-dlp
reported. Measured: 120 MB joined in 96 ms where the converter's stream copy took
174 ms; the decoded frames and sound match the inputs' (`tests/mp4mux.rs`),
and AVFoundation reads the result. The converter is still fetched where dm does not
do the work: a WebM picked in the panel, embedded subtitles, RTMP/RTSP/MMS.
Not written: a WebM (Matroska) muxer.

Later the same day, the HLS side: `tsmux.rs` reads a transport stream's
tables, its H.264 pictures (start codes turned into lengths, the picture
size read from the SPS) and its ADTS AAC frames, and hands them to the same
writer, picture and sound kept the distance apart they had in the stream.
It runs before the converter for dm's own HLS downloads, and on a file yt-dlp saved
as `.mp4` that is a transport stream (yt-dlp's HLS downloader leaves that to
its converter's fix-up). A stream with any other picture or sound kind (HEVC, MP3,
AC-3…) is refused whole, so nothing is dropped from it: a converter does it
where there is one, else the `.ts` is kept. When dm joins, yt-dlp's joiner-location option is
pointed at nothing, so it never uses a converter dm
did not choose. Tests that would pass with a converter yt-dlp found itself
were checked to fail with dm's join turned off.

## WebM joined by dm too (2026-10-21)

`mkvmux.rs` joins a WebM picture (VP9, AV1) and sound (Opus) the way
`mp4mux.rs` joins MP4: tracks and blocks read from each input's EBML, written
as one WebM with clusters starting at the picture's key frames, `Cues` and a
`SeekHead` so it seeks, the frames copied as they are (laced or not, block
groups kept). A WebM line picked in the panel no longer fetches the converter. A
real YouTube 720p VP9/Opus pair (21 MB) joins in 44 ms and decodes identical
to its halves. Reads move within one megabyte buffer rather than seeking
(seeking threw the buffer away per block: 1.47 s for the same file).

## Subtitles put in by dm (2026-10-21)

With no converter, yt-dlp is asked for subtitles as WebVTT beside the video
(`--sub-format vtt`, no `--embed-subs`), and dm carries them inside what it
writes: in an MP4 as `tx3g` text tracks, one per language in one alternate
group, each cue a sample and each gap an empty one, with a closing empty
sample (readers time a subtitle by the sample after it; without one the
last cue ended with the picture); in a WebM as `D_WEBVTT/SUBTITLES` tracks,
each block "id line, settings line, text" as the WebM spec has it (a standard
decoder drops a block without the two lines; Matroska's newer `S_TEXT/WEBVTT`
is not read by the version 7 of it tried). A file that was already one stream is written
again with them; one that will not take them keeps its picture and sound.
A real YouTube video's English and German subtitles read back exact. A
block before zero (Opus's delay) now keeps its time, the first cluster at
zero and its offset negative, rather than moving everything 7 ms on.

## More of a transport stream put into MP4 by dm (2026-10-21)

`tsmux.rs` now also carries HEVC pictures (an `hvc1` with its `hvcC` built
from the VPS, SPS and PPS: profile bytes as they are, sub-layers, chroma
format, bit depths and size read from the SPS), MPEG-1/2 audio (MP3, MP2:
frames whole, object type 0x6B or 0x69) and AC-3 (`ac-3` with its `dac3`,
frame sizes from the standard's table). Each was made by a standard encoder, put into an
MP4 by dm and decoded identical to the stream; AVFoundation reads the HEVC
and AC-3 one. E-AC-3 too (`ec-3` with its `dec3`; one independent
substream). Still refused, and left to an installed converter or kept as `.ts`: MPEG-1/2
and MPEG-4 Part 2 pictures and LATM AAC, which HLS does not use.

## RTMP spoken by dm (2026-10-21)

`rtmp.rs` is a client of dm's own for `rtmp://` and `rtmps://` (TLS through
tokio-rustls with the webpki roots reqwest already uses, the ring provider
named since two are compiled in): the simple handshake, the chunk stream
(all four header forms, extended timestamps, the server's chunk size,
acknowledgements at its window, pings answered), and AMF0 `connect`,
`createStream`, `play`. H.264, HEVC (both RTMP extensions) and AAC or MP3
arrive as FLV tags, aggregates included, and go into `fmp4.rs`: a
fragmented MP4 written a fragment a second, so a stream cut short plays.
When the stream ends it is rewritten plain by `mp4mux`, which now keeps a
fragmented track's start (`tfdt`) as an edit, so sound that began after the
picture still lines up. Checked against a real streaming server sending a clip over RTMP: the
saved file decodes identical to the clip, picture and sound, with dm told
there is no converter. RTMPE (Adobe's encryption) and RTMPT (HTTP tunnelling)
stay with the converter; neither is in use any more.

## RTSP recorded by dm (2026-10-21)

`rtsp.rs` records `rtsp://` with the `retina` crate (MIT/Apache, a
maintained RTSP client used for IP cameras): DESCRIBE, SETUP with RTP
interleaved on the RTSP connection (passes NAT and firewalls), PLAY, and
retina's H.264, H.265 and AAC depacketizers giving frames already in MP4
form, which go into the same fragmented MP4 as RTMP. A sign-in in the
address goes to retina separately (it refuses an address carrying one).
The initial-timestamp policy is permissive: mediamtx, and servers like it,
leave `rtptime` out of the PLAY reply, which retina's default rejects.
Checked against mediamtx 1.12 fed by a standard encoder (H.264 and AAC; the recorded
pictures are the clip's frames 75–198 in order) and, in the suite, against
a one-client RTSP server in `tests/live.rs` sending real H.264 as single NAL
and FU-A packets (all 75 pictures identical). RTSPS and MMS remain the converter's.

## MMS over HTTP by dm (2026-10-21)

`mms.rs` asks an `mms://` (or `mmsh://`, `mmst://`) address over HTTP as a
Windows Media player does: a first request for the ASF header, from which
the packet size and every stream's number are read, then `xPlayStrm=1`
with every stream switched on; the `$H`, `$D` and `$E` chunks are read
across reads and each data packet padded back to the header's packet size.
What is saved is the ASF the server streamed: in the suite an encoder-made
Windows Media file served this way comes back byte for byte. MMS's own TCP
protocol (MMST) is not spoken: servers that had it also answered HTTP, and
none are known to be left. With this, the converter is fetched only for RTMPE,
RTMPT, RTSPS and a transport stream whose picture is MPEG-1/2 or MPEG-4
Part 2.

## The same formats as the converter route (2026-10-21)

Without the converter dm had asked yt-dlp for an MP4 picture and M4A sound, which
would have given a lower picture for a video whose tallest came only as
WebM. `mkvmux::mp4_tracks` now reads a WebM's VP9 (`vp09`, its `vpcC` from
the first key frame's header), AV1 (`av01`, WebM's `av1C`) and Opus (`Opus`,
`dOps` from OpusHead) as MP4 tracks, and `mp4mux` takes a WebM input beside
MP4 ones, so dm asks for what the converter route asked for (`bv+ba/b` and its height
forms, with the same sort) and puts it into an MP4 as the converter did; a WebM is
made only when one is asked for. Checked: on six YouTube videos at best and
at 720p, yt-dlp picks the same format IDs for the old and the new selector,
and `scripts/out-of-box-check.py` gets a 4K video's best (format 625, 2160p
VP9 over HLS) as one MP4 with its AAC sound that plays.

## Links dragged in and files dragged out, around winit (2026-10-21)

winit 0.30 takes only files in a drop, and on Linux the `drag` crate needs a
GTK window. Each system is met where winit stops, rather than moving off
winit:

- macOS: `draggingEntered:` and `performDragOperation:` on winit's window
  delegate class are replaced at run time; a pasteboard with files goes on to
  winit's own, one with a URL or text holding a link is taken by dm.
- Windows: winit's drop target is revoked and dm registers its own OLE
  `IDropTarget`, which reads files, `UniformResourceLocatorW` and text.
- X11: winit is vendored (`desktop/vendor/winit`, `[patch.crates-io]`) with a
  small patch, marked `dm:`, choosing `_NETSCAPE_URL` or UTF-8 text when no
  `text/uri-list` is offered — Firefox offers none for a link. Dragging out
  is dm's own XDND source on a connection of its own (`xdnd.rs`): winit's
  pointer grab (core and XInput 2) is let go so the drag can take it, and
  dm's window is told the button is up afterwards, since winit never sees
  that release.
- Wayland: neither. winit has no data-device support there; a hint says so.

Tried and dropped: asking for the selection with type `ANY` (X servers
answered TypeMismatch; the chosen target's own atom is asked for).
Replacing the vendored copy goes away when a winit release reads links.

## Out of the box for someone new: nothing to install but the extension (2026-10-21)

dm no longer fetches a separate video tool for anything. What it was still fetched for is
dm's own now, or refused: RTMPT through an HTTP tunnel in `rtmp.rs`, RTSPS
through a loopback TLS bridge in front of retina (which speaks plain RTSP
only: patching it was the alternative, and the bridge needs no vendored
copy), and a video in a container dm does not join asked of yt-dlp as one
file that needs no joining. RTMPE is refused: it is Flash's encrypted RTMP,
a protection dm does not undo. A converter the person installed is still
used where it helps.

Bounded, as everything read from a server is: an RTMPT answer is read in
pieces and refused past 16 MiB. The RTSPS bridge takes the first
connection to a random loopback port: another program on the same
computer that got there first would reach the camera without dm's login,
which retina sends only on its own connection, and dm's recording would
then fail rather than go anywhere else. Accepted, as a program already
running as the person can read dm's files anyway.

yt-dlp stays the one thing fetched, with Deno. Written into dm instead it
would be the site logic this project does not write (see "Player pages go
to the user's yt-dlp"): YouTube's player code changes weekly and is answered
by running it, which is why yt-dlp needs a JavaScript runtime, and yt-dlp's
thousands of sites are kept working by hundreds of people. Fetched and kept
current is what makes video work for someone who installs nothing; the row
says "Getting ready for videos (first time only)" while it happens.

Someone who installed only the app is shown how to add the extension: a bar
over the list, until a browser connects (the native host leaves
`browser.seen` in dm's home on the extension's hello) or they say not now,
with the same buttons in Options › Browser. Messages never say "install
yt-dlp" or tell the person to install a converter: a failed video says dm could not get ready for
videos and to check the connection, and an extension talking to an older
app says to update the app. The two store addresses are allowed in shipped
code by `cargo xtask privacy`, since they open in the person's browser only
when clicked.

## yt-dlp may use a joiner the person installed (2026-10-22, later)

The option that pointed yt-dlp at a joiner that is not there was removed:
it was the last place dm named a tool it does not use, and the owner chose
to drop it. On a computer with no joiner of its own nothing changes: yt-dlp
leaves picture and sound apart and dm joins them. Where one is installed,
yt-dlp may join them itself and dm takes the one file as it is (putting
subtitles inside where it can, as it does for any single file). The cost:
the file can differ from computer to computer, and that route is tested
only where the test machine has such a joiner. dm still fetches nothing
of the kind and runs nothing of the kind itself.

## No separate video tool, not even an installed one (2026-10-22)

dm joins picture and sound (`mp4mux`, `mkvmux`), carries subtitles, puts
transport streams into MP4 (`tsmux`) and records live streams itself, so a
converter on the computer is no longer used for any of it either: yt-dlp is
pointed at a joiner that is not there and always leaves picture and sound
for dm, an HLS stream dm cannot convert (MPEG-2 pictures) is kept as the
`.ts` it came as, and RTMPE is refused. The same file on every computer,
whatever is installed, and one route to test instead of two.

yt-dlp stays, and is not rewritten inside dm. It is what reads a video
site's page: YouTube's player answers with signatures and throttling codes
that change week to week and are worked out by running the player's own
JavaScript (which is why yt-dlp needs Deno), and yt-dlp covers well over a
thousand sites, kept working by the many people who maintain it. dm's own
version would be site logic this project does not write (see "Player
pages go to the user's yt-dlp"), would break within weeks of being
written, and could not behave as yt-dlp does across its sites. The
person never installs it: dm fetches it, checked, and keeps it current.
User-facing words call it dm's video helper; PRIVACY.md names it and Deno,
since people are owed knowing what program dm downloads and from where.

## QuickJS rather than Deno for yt-dlp's JavaScript (2026-10-22)

yt-dlp answers YouTube's player challenges by running JavaScript, with
Deno, Node, QuickJS or Bun. dm fetched Deno (85 MB) for it; it now fetches
QuickJS-ng 0.17.0 (1–3 MB), pinned, each build checked against the digest
written into dm since the release publishes no checksums file. Measured
first on 2026-10-22: the same formats chosen at every quality, a 108 MB
1080p stream in 9 s either way, a page read in about 6 s against Deno's 3.
`scripts/out-of-box-check.py` passes every case on it. The first video
now waits on about 36 MB instead of 120. A Deno the person installed is
still used, as it reads a page a little faster.

Not shipped in the installers: with 36 MB fetched once, bundling yt-dlp and
QuickJS would add OS-specific programs to six installer formats (the Arch
package builds from source), invite antivirus false alarms on Windows, and
go stale between releases, for a few seconds saved once.

## Drag and drop on Wayland, in the vendored winit (2026-10-22)

winit 0.30 has no Wayland drag and drop, and a Wayland drag can only be
started on the connection that owns the window, with the serial of the
press that began it, so dm cannot do it from a connection of its own as it
does on X11 (`xdnd.rs`). Both directions are added to the vendored winit
with sctk's data-device support, which winit already depends on: a drop is
read through a pipe on winit's event loop; a drag out is asked for by dm,
and winit starts it on its next turn, woken at once so the press is still
held. Tested under sway (headless) with a virtual pointer written for the
test (`scripts/linux/vpointer.c`, wlroots' virtual-pointer protocol), since
a headless compositor has no pointer of its own and `swaymsg seat … cursor`
reaches no client without one.

## 2026-10-24 Find keeps 24 points from the row it found (was 4)

The top-row search box moved the list up 30 points, and Find's placement,
which reads the found row's rectangle from the frame before, then left the
dialog 6 points over the row (`find_stays_clear_of_the_row_it_found`). A wider
margin absorbs that one frame of lag rather than chasing the layout; the test
that caught it covers the margin. Tried and dropped: reading the row's
rectangle again after the list's own layout, which needs a second pass over
the frame for one dialog.

## 2026-10-25 Gaps found by a behaviour walk-through, and what was left

Done: tracking parameters are ignored when comparing addresses (`lists::bare`,
used for every "already in the list" question; the address that is fetched is
untouched), and the tray gained Start queue and Stop all.

Left as it is, on purpose:
- A certificate that cannot be checked stops the download with the reason
  said. A "continue anyway" would be the one place dm fetched over a link
  anyone on the path can read and swap; nothing a person is waiting for
  justifies it. Considered: a per-row override, dropped.
- Taking a download the browser would keep is Shift-click, and the keys are
  options; a second default combination adds nothing.
- A floating button over selected links, with a checklist of them: the
  right-click "Download selected links" hands over the same set, and dm's
  own Add links window has the checkboxes. A page overlay needs a real
  browser to check, which this project's shell does not have, so it is not
  built unseen.

## 2026-10-25 Closing the window quits on a tiling Wayland desktop

Close-to-tray on Wayland minimizes (2026-10-23), and sway, Hyprland, river,
niri and similar have no minimize: Super+Q (their kill-window key) changed
nothing on screen. Where `XDG_CURRENT_DESKTOP` names one of them
(`minimize_is_ignored`, unit-tested) a close takes the no-tray path: dm exits,
asked first while downloads run. Not seen on a real compositor from this
shell; `swaymsg kill` in `linux-wayland-close-check.sh` sets no desktop name,
so it still checks the minimize path. Considered: a setting for what closing
does, dropped until someone needs the other choice.

## 2026-10-25 DASH is dm's own; yt-dlp is left the pages

A DASH manifest went to yt-dlp, so a stream whose address was a plain `.mpd`
needed a 40–80 MB helper fetched first. A manifest is a published format with
no site logic in it: it says where the pieces are. So `dash.rs` reads it
(templates with `$Number$`/`$Time$`/`$RepresentationID$`/`$Bandwidth$` and
widths, `SegmentTimeline` with repeats, `SegmentList` and `mediaRange`,
single files), `download/dash.rs` fetches the chosen picture and sound, and
`mp4mux`/`mkvmux` (which already read fragmented MP4 and WebM) join them.
Decided: one period only (an advertisement break would join two initialisation
segments, the case `beyond_dm` refuses for HLS); live (`dynamic`) refused
rather than half-recorded; anything under `ContentProtection` skipped, and
named if nothing else remains; a byte range answered with anything but
`206` and the right length is an error, since a server that ignores the range
and sends the file would be written in as a piece. Sound is a main-role track
over a louder commentary, in the picture's container where there is one.

Not changed: a *page* (a video site's watch address) still goes to yt-dlp,
because the media's address is minted by the site's own player and reading
it is site logic, which this project does not write (2026-10-11, 2026-10-21).
The way past that without site logic is to take the address from the player
in the browser, which already ran the site's code: see docs/gaps.md.

A picture and sound that an HLS master keeps in playlists of their own
(`#EXT-X-MEDIA TYPE=AUDIO` with a `URI`) are dm's too when both are plain
fragmented MP4: fetched as the two tracks of a DASH stream and joined by
`mp4mux`. Encrypted, byte-ranged or transport-stream pairs are still
`Beyond`, because joining them wrongly saves a silent film as complete.

## A picture and a sound the player fetched apart (2026-10-25)

The extension reads a video site's player requests (`webRequest`, and
`extension/document.js` in the page's own world, which tells the same address
through `content.js`) and, where the request is a `videoplayback` of a
`googlevideo.com` host, keeps the file's own address (the per-piece `range`,
`rn` and `rbuf` taken off) as a picture or a sound by its `mime`/`itag`. A
tab that has loaded both shows one line, "1080p video + sound", whose hand-off
carries a second address (`Start.sound`, `Entry.sound`, `Options.sound`). dm
fetches both at once in 8 MiB byte-range pieces, as many at a time as the
connection setting, and joins them with its own muxer
(`download::dash::download_media_pair`). A server that does not answer a
range is refused, not fetched whole.

Known limit, measured on 2026-10-21 (see "A video page's session goes with
its video"): such addresses stop with a 403 after about 1 MB where the site
wants a proof-of-origin token the page's player holds. This route is tried
where the player's addresses work; where it fails the row fails with the
server's refusal, and the same page remains one click away through the video
helper. Not tested against the live site (no browser network in the build
environment): tested against local servers that answer ranges as a CDN does.
Hostnames stay out of the manifest (the privacy check forbids them), so the
page script is registered for every page and itself ignores any host but
`googlevideo.com`; it only reads an address and leaves the request alone.

## Reading a page's qualities faster (2026-10-25)

The panel's list took 2 to 39 seconds, nearly all of it yt-dlp reading the
page. Three things, kept in this order of effect:

1. The extension asks as soon as the panel is raised over a player (on
   play), not at the click, and a click made sooner joins the read under
   way (`content/handoff.rs`, `reading`). Measured in Edge 156 on a
   YouTube watch page: hovering, waiting eight seconds, clicking: the list
   in 0.03 s.
2. `list_choices` passes `--extractor-args youtube:skip=hls,translated_subs`:
   the same heights are in the DASH manifests, and the list needs neither
   the HLS ones nor the translated subtitles. 2.4 s to 1.7 s on the same
   page, the same eight heights and sizes. The client choice was tried and
   dropped: `android_vr` and `mweb` read in about 1.6 s but offered five
   formats, nothing above 338 p; `tv`, `ios` and `web_safari` were refused.
   Downloads are untouched.
3. A reading is kept for ten minutes in `formats.cache.json` in dm's home
   (48 pages), keyed by the page and the *names* of the cookies sent: a
   site restamps some cookie values on every request, and keyed by value a
   page was never found again (measured: the second read took 1.6 s).
   A page opened again, in another tab or by another host process, lists
   in 10 to 20 ms. Only a non-empty reading is kept.

## HLS byte ranges and encrypted pairs (2026-10-25)

`#EXT-X-BYTERANGE` (and a `BYTERANGE` on `#EXT-X-MAP`) are read into
`Part::range` and each piece is asked for by its bytes, exactly or refused
(`fetch_segment_range`). A range dm cannot read is an error, not ignored: an
ignored one saved the whole source repeated and called it done. A pair
(picture and sound in playlists of their own) may now be AES-128 as well:
`dash::Piece` carries a `Lock` (key and the vector its segment number
makes), undone as each piece is fetched. Still `Beyond`: a transport-stream
picture with packed-AAC sound (gaps.md row 7).

## 2026-10-26: A transport-stream picture with packed-AAC sound is joined by dm

An HLS master that keeps its sound in `.aac` segments beside a `.ts` picture
went to yt-dlp. dm now reads the ADTS frames as `tsmux` reads a transport
stream's, takes the sound's start from the first segment's ID3
`transportStreamTimestamp` (the picture's 90 kHz clock), and writes both as the
two tracks of one MP4 (`tsmux::join_packed`). The join is chosen by sniffing
the gathered files (0x47 sync byte; ID3 or ADTS), not by a new flag on the
track, so the DASH and fMP4 paths are untouched. A stream `tsmux` refuses
(HEVC with odd sound, MPEG-2 picture) fails the join with the reason rather
than falling back; dropped: asking yt-dlp after the fetch, which would fetch
the stream twice.

## 2026-10-26: A browser hand-off starts at once; "Download details" is opt-in

`ask_panel_downloads` and `ask_caught_downloads` now default to off (this
reverses "A video from the panel asks first", 2026-10-12). A click on the
floating "Download video" button, or a download dm catches, was handed over
and then waited in "Download details" for a "Start" press, which read as the
browser having downloaded it and dm opening late. Measured in Edge 156: the
browser's copy is paused 0.02 s after it is created; the 1.5–1.9 s before dm's
first request is the browser starting the native host the first time in a
session (22 ms after), which `on_startup` already warms. The two switches stay
in Options for anyone who wants the dialog. Existing settings files keep what
they hold. Side effect: with both off, "Download all links" queues the links
without the picker (the links dialog follows the same switch).

## 2026-10-26: A file asked of dm by name is never started in the browser instead

The floating "Download video" button and the menus' "Download with dm" handed a
file as `caught`, so the host fetched it first and, if dm was refused (a 403
where the browser has the site's session), the extension gave it back and the
browser downloaded it: the "download started in the browser" the person saw.
They are handed over as asked for now (`caught: false`); dm takes the file and
a refused one shows as a failed row to retry or sign in for. The browser still
takes over only when no dm app can be reached at all. Dropped: probing first,
which is what let the browser start.

## 2026-10-26: A caught download dm is refused is retried with the session, not left in the browser

A link only a signed-in browser could fetch was refused by dm's pre-check and
stayed in the browser. The extension now hands it over a second time with the
page's cookies for that one download (`cookies_once`, to the download's own
server, dropped when it ends), and the browser keeps it only if dm fails again.
A caught download of an address already in dm's list opens "Already in the
list" (Show it / Download again) instead of being fetched a second time
unasked. When the browser does keep a download because dm could not take it or
could not be reached, it says so in a notification. Left with the browser by
design: Alt-click, a type dm does not catch, catching switched off.

## 2026-10-26: Smaller ways a download ended up in the browser

- "No app" is believed for 2 s, not 10: one slow or failed start no longer
  sends the next ten seconds of downloads to the browser.
- The default catch list gained Office documents (doc, docx, xls, xlsx, odt,
  ods, odp), disk images (vmdk, vdi, vhd, vhdx, ova), jar, whl, ipa, msix,
  more video (rmvb, vob, m2ts, mts, asf) and archives (lz, lzma, tbz2, txz).
  A settings file that already lists types keeps its own list.
- A caught link of an address in the list asks (Show it / Download again) only
  while that row is worth keeping: a failed row, or a finished one whose file
  is gone from the disk, is fetched again without asking.

## 2026-10-26: "Don't show this window again" ticks, then saves when the window closes

The box in "Download complete" applied the setting on the frame it was ticked
and shut the window under the pointer, so it never showed as ticked. It now
stays ticked (`Dialogs::complete_never`), the window stays, and the choice is
saved when the window closes by Close, ×, or the last Open; a later download
that finishes opens nothing (tested through the engine's own settings).
Escape closes the window as Close and × do, and the ticked box is kept on that
path too (`Dialogs::close_top` empties the set; the window saves the choice on
its next frame). Rule for dismissal in dm: a dialog's check box is a choice made
in the dialog, kept ticked while it is open and saved when it is dismissed by
any route, never applied (and the dialog shut) under the pointer.

## 2026-10-26: Everyday controls where the eye is

A pass against a list of what a download manager should do without being
asked twice:

- The row under the pointer shows Stop or Resume (Retry when it failed) and
  Remove on itself (`table/hover.rs`), through the toolbar's commands; their
  names carry the file's, so they are not the toolbar's twins to a screen
  reader.
- The status bar's speed-limit text is a button: a click turns the cap off or
  on, and opens the limiter's settings when none was ever set (turning on a
  cap of 1 KB/s was what the toggle did then).
- Downloads that finish in one pass are one toast ("N downloads finished."),
  a single file keeps its toast with Open and, on Linux, an Open folder
  button. Not done: the Windows toast's buttons and a batch spread over many
  seconds (each pass is judged alone).
- The floating drop target (View › Drop target) shows the overall progress of
  what is downloading as a bar along its foot, and the speed on hover. Not
  done: a separate resizable mini window.
Already in place and unchanged: categories, Filter and sort, drag in and out,
the clipboard bar, the context menu and extensions, the tray, the scheduler.

## 2026-10-26: A saved list of caught types is brought up to the defaults once

A person who never opened Options kept the list of file types an older version
had saved, and documents (.doc, .docx, .xls, .xlsx and others), added to the
defaults later, stayed in the browser for good; a saved value always beats a
new default. `catch.defaultsVersion` (0 when absent) says which edition of
`DEFAULT_EXTENSIONS` a list has been brought up to; below the current one, the
types it lacks are added once, in the app as it loads settings and in the host
the browser asks (`CatchRules::bring_up_to_date`). Dropped: adding the defaults
on every start, which would put back a type a person took out; and telling an
untouched list from an edited one, which the file cannot say. An empty list is
every type and is not touched. The same applies to the two "ask first"
settings, which are saved as they were: an older file keeps asking before a
caught download starts, until the person turns it off in Options.

## 2026-10-26: A click on a file the browser shows is caught by the page script

A link to a `.pdf`, `.mp4` or `.mp3` opens in the tab: the browser starts no
download, so `downloads.onCreated` never fires and a type on dm's list was
read, not caught. The page script now sees the person's plain left click
(`isTrusted`, no key held, no `download` attribute, an http(s) link to one of
the browser-shown types that is also on the app's list), keeps the page from
opening it, and asks the background (`dm.inline`); the background applies the
app's rules (`CatchRules::takes`, the master switch, the popup's switch, not a
private window) and hands the file over. Any "no" (no app, a site to leave
alone, a failed hand-off) makes the page open the link itself, so a click is
never swallowed. Dropped: `declarativeNetRequest` setting
`Content-Disposition: attachment` on those responses, which would also take
the Alt key's way out and every address typed or bookmarked, and make a PDF
impossible to read in the browser at all; and `webRequest` blocking, which
Manifest V3 does not give an extension. Not covered: an address typed into
the bar, a bookmark, a link opened from another app, a PDF in an embedded
viewer: those are the browser's page loads, not clicks the page can see.

## 2026-10-26: A waiting dialog comes forward; a saved "ask first" is not migrated

A download the browser handed over and dm asks about has already left the
browser, so a dialog behind another window or in the tray looked like a lost
download. Opening Download details, the page's links or "Download again"
from a hand-off now sets `show_asked`, which the window already honours (it
shows itself, unminimized and focused), and a hand-off that starts at once
does not. Dropped: migrating a saved `ask_caught_downloads` or
`ask_panel_downloads` to off, as the catch list was brought up to date: the
file cannot tell an old default from a person's choice, and wrongly turning
off a question someone wanted is worse than a question they can switch off
in Options › Browser. The 404 text was reworded for a person (not the
server's words) in all thirteen catalogs; the other plain-wording rows stay
open (gaps row 9) since each is a string in every catalog.

## 2026-10-26: Remove from the list asks nothing for a finished download

The standards separate "Remove from list" (safe, no prompt, Undo for five
seconds) from "Delete file" (a question, the Recycle Bin, never permanent).
A finished row's removal loses nothing, so `Remove` now takes it off at once
and the queue keeps its snapshot (`Queue::finished_rows`) for `put_back`,
which restores it in its place; Undo is a link in the status bar for five
seconds, not a toast with its own window. An unfinished row still asks: its
fetched bytes go with it and cannot be put back. `Delete file` reuses the
removal question with the file ticked, and `Platform::trash` (the `trash`
crate on a desktop, a plain delete in the tests) moves the file out instead of
erasing it. Dropped: undo for an unfinished row, which would mean keeping its
parts folder for five seconds and the disk it holds. The new dependency,
`trash` 5.2.9, is the one crate for the three systems' recycle bins.

## 2026-10-26: The first run is the welcome; hints are said once; paused sites are the extension's

The standards ask for a first run of at most three skippable steps. dm already
opened a welcome on a new install (the extension for each browser found, start
with the computer); it now also shows the folder downloads go to, which can be
changed there, and ends with Done (Later still skips it). A hint is said once
ever and kept in the settings file (`hinted_big`, for a download of 5 GB or
more; the Scheduler is what it names): said again, it is a nag. The close
button asks once (`on_close`: ask, tray, exit) and the answer is the setting,
changeable in Options, so no answer is hidden. "Pause capturing on this site"
is a list in the extension's storage (`dm.catch.paused`), as "Take every
download from this site" is, not a setting in the app: the app's skip list
(Options › Browser) is the app's own, for every browser, and a popup that
wrote into it would need the host to be running. Restore defaults copies a
tab's saved settings from `Settings::default()` by key, as `changed_only`
merges them, so a setting the tab shows but not a key is a test failure
(`tab_settings`); Sign-ins has none, since a person's logins are not a
default to go back to. Dropped: a Windows quiet-hours check written without
being able to compile it (gaps row 14), and hiding the sideways extra columns
(Last try, Description, Referrer, Folder) by default, which changes what every
saved window shows and which fifteen tests read.

## 2026-10-26: Windows modules are checked from here, and run on Windows later

`cargo check --target x86_64-pc-windows-msvc -p dm-desktop` stops at the C
build of `aws-lc-sys` without a Windows toolchain, so Windows-only code could
not be compiled on a Mac, and was left unwritten. The new Windows code
(`quiet.rs`, `contrast.rs`, `jumplist.rs`) is written to need only `std`,
`windows-sys` and `windows`, which are plain Rust, and
`scripts/windows-modules-check.sh` includes each file in a crate of its own and
checks it for the Windows target; their logic (what is quiet, what is held, the
palette's contrast) is plain code the tests run anywhere. What it cannot say is
whether the system answers as expected, so gaps rows 11 and 14 stay open as
checks to make on Windows. Dropped: a `windows` crate behind a feature in
dm-core, to check the whole of dm-desktop (a second build for one check), and
leaving the rows blocked (a person can only verify what exists).

High Contrast is not a fifth choice in Options: Windows says it is on, and
every program is expected to follow, whatever it was set to; the debug-only
`DM_HIGH_CONTRAST=black|white` shows the look elsewhere. A jump list is
published once at start, in the language then in force.

The list's vertical bar was a scroll area under egui's 64-point minimum, in a
strip of one row it could not reach as far as the list and pulled it back each
frame. Controls at 32 made that strip one row at 300% text with the clipboard's
bar, which is how it was found; it is fixed by giving the bar the strip's own
height (`min_scrolled_height`), and held by a test.

A dialog's shadow follows the room: Fluent's 128 where the window is 1000
points tall, 64 from 800, and 28 below (`dialog_shadow_blur`), since egui
counts a shadow in a window's size. The menu has 32; tooltips share egui's one
popup shadow and have 32 too, not 16.

## 2026-10-26: `app-check.py` reads the window after waking it

The real-binary check failed one run in three or four, on builds from before
this day's changes as well as after (4 of 12 and 2 of 12), at steps that click
and then read the window's words once after half a second. A request for the
words (`ui.texts`) reads the last frame drawn and does not make the window
draw one; any other control call (`ui.inspect`) does. The steps now wait up to
three seconds for what they expect, waking the window each time round
(`soon`), and a batch finishing no longer opens a Download complete window
over the dialog being typed into (`show_complete` off in the check's own
settings). 16 of 16 runs pass. The app was not at fault: a person's pointer
makes frames all the time.


## 2026-10-27: Update-manifest signing removed

The 2026-10-25 ed25519 signing of `latest.json` needed a private key kept as
the repository secret `UPDATE_SIGNING_KEY`; the owner has none and will not
set one up for now, so the 0.8.0 release job failed at the signing step. The
signing step, `cargo xtask sign-update`/`update-keygen` and the check in
`update.rs` are removed; the installer is still checked against the SHA-256
in the manifest. It is in git history (commit `89a66d6`) if wanted again.
