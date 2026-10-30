# Clean-room behaviour specification: the other macOS manager's download engine and its video helper

Prepared 2026-10-30 by the analyst side of a clean-room process, for dm's
implementers. It describes behaviour only. It contains no source code, no
assembly, no decompiled output and no copied interface text; where the
program's own words matter they are paraphrased. Every finding is marked:

- **Observed**: seen directly, in a tool's output or a run recorded here.
- **Inferred**: concluded from observations, not seen directly.

Subject: the other macOS manager, version 1.1.10, installed at
`/Applications/<its name>.app` (the shorter application path named in
the request does not exist on this computer; **Observed**). Tool: REA 6.8.0
(`npx rea-agents`), with macOS's own `codesign`, `spctl`, `xattr`, `lsof`,
`ps` and `pgrep` for run-time evidence. The evidence files are in the
session's scratchpad (`rea/macho-*.json`, `rea/plist.json`, `rea/sig.json`,
`split-observe.py`, `errors-observe.py`, `errors.log`).

---

## Stage 1: the bundle's binaries (REA `inspect-macho`, `inspect-plist`, `inspect-signature`)

Evidence: five REA Mach-O reports, each with `confidence: observed` and
`authority: shipped-artifact`, from REA's "macOS native inspection
utilities" provider.

| Binary | Architecture, minimum macOS | What it links | Role |
|---|---|---|---|
| The window app | arm64 only, macOS 14 | AppKit, SwiftUI, Foundation, Network and about 45 other Apple frameworks; SQLite | The window, the notch, the native-messaging bridge (**Observed**: its symbols name a native-messaging reader and writer) |
| The engine | arm64 only, macOS 11 | CoreFoundation and Security only, with libc++ and zlib | A Node.js program packed as a single executable (**Observed**: a Node single-executable marker and a Node 22 version string; its JavaScript is stored as compiled bytecode, so no source is readable) |
| aria2 | arm64 only, macOS 11 | libSystem and libc++ only | The multi-connection file downloader (**Observed**) |
| QuickJS | arm64 only, macOS 14 | libSystem only | The JavaScript runtime handed to yt-dlp (**Observed**) |
| yt-dlp | arm64, macOS 11 | libSystem and zlib; a Python 3.14 runtime beside it | The video reader and downloader, in yt-dlp's unpacked ("onedir") layout (**Observed**) |

The application as a whole is notarized under a Developer ID
(**Observed**: `spctl` reports it accepted as a notarized Developer ID
application).

## Stage 2: saving constructs (REA's import tables, then the run-time record)

**Observed** in the window app's imports (30 of its 4,268 imports are
file or transfer related): URLSession data and download calls, FileManager
directory enumeration and item replacement, NSSavePanel and NSOpenPanel,
NSFileHandle reads, writes and seeks, the volume-capacity resource keys
(available capacity, and available capacity for important use), and the
POSIX calls `pwrite`, `ftruncate` and `fsync`. **No** URLSession download
*delegate* type appears among its imports or exports (**Observed**).

**Inferred**: the window app does not move download bytes. Its URLSession
calls fetch small things (update manifests, thumbnails, licence checks),
its save panels let a person choose a folder, and its capacity keys check
free space. **Observed** in support of that: during every download recorded
below, the only process with the destination file open was aria2, not the
window app.

**Observed** in the engine's string constants (read from its compressed
bytecode blobs; names only): aria2's RPC options for connections per
server, split count, minimum split size, continue, allow-overwrite,
automatic renaming, tries, retry wait and not-found limits; a constant for
the most connections; the POSIX error names for permission denial,
read-only volume, disk full, quota, connection reset, refused, timed out
and name lookup failure.

## Stage 3: cross-references (not run)

**Observed**: REA reports no decompiler provider here (Hopper, Ghidra, IDA
and Kuna are all not configured). Cross-reference mapping was therefore
not possible. It was also not needed: the engine's run-time behaviour
answers every question in stage 4 directly, and a decompiler would have
taken the analysis outside clean-room bounds.

## Stage 4: how a file is split and put together (run-time record)

Run: the engine fetches one 96 MiB file from a local server that holds
each connection to 512 KiB/s, while a script samples the download folder,
aria2's open files and aria2's TCP connections every two seconds
(`split-observe.py`). The full table is in the evidence; its shape:

| Time | On disk | aria2 connections to the server |
|---|---|---|
| 0 to 2 s | nothing (queued, then started) | 0 |
| 4.7 s | the file at its **final name and full size** (96 MiB apparent, 0 bytes allocated), and a 70-byte control file beside it named after the file with an added suffix | 4 |
| 7 to 30 s | the same file; its *allocated* size grows from 4 MiB to 99.6 MiB while its apparent size stays 96 MiB; the control file grows to 170 bytes | 6, then 9 |
| 32.8 s | the file alone, fully allocated; the control file gone; aria2 no longer has it open | (closing) |

From this:

1. **No parts are merged** (**Observed**). There is never more than one
   data file. Each connection writes its range straight into the final
   file at that range's offset (**Inferred** from the sparse file filling
   in while several connections are open; consistent with aria2's
   positional writes).
2. **The file is created sparse at its full length** (**Observed**:
   apparent size equals the full size at once while 0 bytes are
   allocated). Disk space is taken as bytes arrive, not reserved at
   the start (**Inferred**).
3. **Progress is kept in a small control file beside the data file**
   (**Observed**: it appears with the file, grows as ranges are split and
   finished, and is deleted at completion). It is what lets a stopped
   download continue (**Inferred**, consistent with the `continue` option
   observed in aria2's arguments and a session file kept in the app's
   support folder).
4. **Connections ramp up** as ranges are split (**Observed**: 4, then 6,
   then 9). aria2 is started with 16 connections per server, 16 splits and
   a 1 MiB minimum split (**Observed** in its process arguments). "Up to
   32" is a licensed maximum (**Observed**: a licence token field for the
   most connections reads 32, and the engine names a matching constant);
   that it is applied per download through aria2's options is **Inferred**.
5. **On completion the file is marked as downloaded** (**Observed**: it
   carries the quarantine attribute naming the app as its downloader, so
   macOS asks before a downloaded program first runs).

Videos take another path (**Observed** in the processes the engine
started on 2026-10-30): yt-dlp downloads the picture and the sound
(several fragments at once for streams in pieces), ffmpeg joins them into
the final file, and ffprobe reads the result. Here there *is* a join step,
done by ffmpeg into a new file (**Observed** from the process sequence;
the intermediate file names were not sampled).

---

## The behaviour, as a specification

### Inputs

- **A link** (**Observed**): an `http`/`https` address handed over by the
  browser extension or pasted into the window, with optionally the page it
  came from, the browser's user agent and the site's cookies.
- **A target folder** (**Observed**): one default download folder in the
  person's Downloads, chosen in settings or with a save panel; a per-site
  sign-in setting decides whether cookies go along.
- **Per-download choices** (**Observed**): a quality for videos, a clip
  start, an action (download now, choose first, sound only, clip).

### Outputs: the file layout on disk

- **A plain file** (**Observed**): one file at its final name in the
  target folder from the first byte; beside it, while the download runs,
  a small control file with the same name plus a suffix; at completion,
  the control file is removed and the data file is marked as downloaded.
- **A video** (**Observed**): the final file (MP4 by default) named after
  the video's title; the separate picture and sound are joined into it and
  then removed (**Inferred** for the removal).

### Core states

| State | What is true | Evidence |
|---|---|---|
| Queued | The job exists and is listed; nothing is on disk | **Observed** (jobs list says queued; folder empty) |
| Initializing | The server is asked for the file's length and range support; the data file is created sparse at full length with its control file | **Observed** (file and control file appear together, full apparent size, 0 allocated) |
| Downloading chunks | Several connections each write their range in place; the control file records what is done; ranges are split as connections free up | **Observed** (connections 4 to 9, allocation rising, control file growing) |
| Merging chunks | Not a separate state for plain files: there is nothing to merge | **Observed** (one data file throughout) |
| Merging (videos only) | ffmpeg joins picture and sound into the final file | **Observed** (process sequence) |
| Final flush | Writes complete, the file is closed, the control file deleted, the download mark applied, the job marked completed | **Observed** (file closed, control file gone, attribute present, status completed) |

### Error cases

- **Permission denied** (**Observed**, 2026-10-30): with the download
  folder made read-only, the engine refused the hand-off at once with an
  "insufficient storage" HTTP status and a message that it may not save in
  that folder, asking for another. Nothing was queued and nothing was
  written. **Inferred**: the folder's writability is checked before a job
  is accepted, not discovered mid-download.
- **Disk full** (**Inferred**, not reproduced: that would have meant
  changing its settings or filling a real disk). Its engine names the
  disk-full and quota error codes, and its window reads volume capacity.
  Because the data file is sparse (stage 4), space runs out *during* the
  download, not at its start, so a full disk must surface as a failed
  write mid-download.
- **Network drop** (**Observed**, 2026-10-30): the server was killed at
  31%. Within a second the job went back to **queued** at 31%, kept its
  bytes, and stayed queued for the whole 40-second outage. About 21 s
  after the server returned, the job was downloading again from 34% and
  completed whole at 104.6 s. Nobody resumed it.
- **A file no longer on the server** (**Observed**, 2026-10-30, from a
  mistake in my first test run): a 404 failed the job at once, marked not
  worth retrying, with a message that the file is no longer on the server.

---

## The question behind the request: how it makes yt-dlp work out of the box

All **Observed** on this computer:

1. **It ships yt-dlp inside the app bundle**, in yt-dlp's unpacked layout
   (the program and a folder of Python libraries, 96 MB), beside QuickJS,
   ffmpeg, ffprobe and aria2.
2. **Every bundled helper is re-signed with the app's own Developer ID
   team**, and the bundled yt-dlp carries an identifier under the app's
   own name. The whole app is notarized. So Gatekeeper accepts every
   helper on first run, with no prompt and nothing for the person to
   install.
3. **On first use it copies yt-dlp out to its support folder** and runs it
   from there, and it keeps that copy current from yt-dlp's own releases
   (the copy there was 2026.10.26; the bundled one is 2026.08.19). A small
   state file records the version, the layout (unpacked) and when it last
   checked. Its update manifest checks every four hours.
4. **The updated copy is yt-dlp's own ad-hoc-signed build**, which
   Gatekeeper's assessment *rejects*, yet it runs. It carries no
   quarantine attribute, because the engine downloaded it itself, not a
   browser. **Inferred**: this is the mechanism that keeps updates
   prompt-free; macOS only checks programs that carry the quarantine mark.
5. **It points yt-dlp at its own QuickJS** through a yt-dlp configuration
   file in its support folder, so yt-dlp needs no Deno or Node on the
   computer.

### What dm already does, and what differs (Inferred unless marked)

- dm fetches yt-dlp's unpacked build, checks its checksums, keeps it
  current and hands it QuickJS (**Observed** in dm's own code and runs).
  Like the other manager's updated copy, dm's fetched copy has no
  quarantine mark, because dm's own HTTP client writes it.
- dm reads YouTube itself first and needs no yt-dlp there (**Observed**).
- **The difference that matters for "out of the box"** is the first run.
  The other manager's first video needs no network fetch of yt-dlp, and
  its bundled copy is signed by the app's own notarized identity. dm's
  installers are signed ad hoc (`signingIdentity = "-"`), and its
  packaging only includes yt-dlp when `scripts/fetch-ytdlp.py` has run.
  A dm downloaded in a browser is quarantined as a whole, so a yt-dlp
  shipped inside it inherits that and meets Gatekeeper with no Developer
  ID to pass on. Most reports of yt-dlp failing to install most likely
  come from the fetch on first use: a network that blocks GitHub, a proxy,
  a read-only or `noexec` temporary folder (seen on Linux, gaps row 29),
  or no network at all.

### Recommended for dm (for the implementer side; not done in this analysis)

1. Always package yt-dlp's unpacked build inside every installer (make the
   packaging step fail rather than continue without it), pinned by version
   and checksum in the repository (gaps row 50).
2. On macOS, sign every bundled helper with dm's own Developer ID and
   notarize the app, once dm has one (gaps row for code signing). Until
   then, copy the bundled yt-dlp out to dm's tools folder on first use, as
   the other manager does: the copy dm writes itself carries no quarantine
   mark.
3. Keep dm's own updater as it is: fetch newer releases into the tools
   folder and prefer them over the bundled copy (`ytdlp::binary` already
   does: fetched, then bundled).
4. Say plainly in the window when neither copy can run, and why, instead
   of a generic failure.
