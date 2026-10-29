# Reference behaviour: two other download managers

dm's engine is compared with two other download managers: Reference A, a
commercial one for Windows, and reference B, a free one for Windows, macOS
and Linux. Neither is installed on the development machine (reference A
runs only on Windows), so what they do is taken from their public pages,
and each claim says where it comes from. Nothing of either program's code is
used; dm's behaviour is written and tested independently.

Labels: **Verified** (the reference's own page states it), **Reported**
(third-party guides only), **Unknown** (not documented anywhere found).

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
- That add-on, made by a third party, is yt-dlp. It bundles yt-dlp's Python source and
  yt-dlp's YouTube challenge solver (`yt_dlp_ejs`); reference B runs it with the
  system's Python (3.10 or later) and Deno (2.3 or later), which reference B installs
  when first asked (`PythonDependencyManager`, `DenoDependencyManager`),
  inside a bubblewrap sandbox on Linux (`QtProcessSandboxManager`: "using bwrap").
- yt-dlp only reads the page: `-J --flat-playlist --no-warnings`, plus
  `--no-playlist` for one video. Reference B's own engine fetches the formats it
  lists (`MsDownload`) and joins picture and sound with ffmpeg
  (`QtFfmpegAvMerger`, ffmpeg a package dependency).
- Sign-in: the browser's cookies for the page when reference B has them, written to a
  temporary Netscape file; otherwise `--cookies-from-browser` with the
  default browser, Firefox if that one is not supported.
- Only an HTML page of 3 MB or less is offered to yt-dlp
  (`isPossiblySupportedSource`), and a page's reading is not asked for
  again within 300 s (`minIntevalBetweenQueryInfoDownloads`).
- "Sign in to confirm you're not a bot" and the age check are reported as
  needing cookies; "Unsupported URL" as unsupported; anything else as a parse error.

Compared with dm: the same design (yt-dlp reads, the download manager
fetches). dm joins picture and sound itself, needs no ffmpeg, and runs
yt-dlp unsandboxed; it does not read a browser's cookie store, by decision
(`decisions.md`, the entry on reading a page's video list).

## Reference B's download engine, measured (2026-10-29)

Reference B 6.35.2 (emulated x86-64) and dm (native) downloaded the same
32 MiB files from a server of this project's that logs every connection and
request, in Docker; the lab is kept outside the repository.

- **First request**: a plain `GET` with no `Range`, left streaming as the
  first piece while its New download dialog waits. dm asks `bytes=0-0` first.
- **Ranges**: open-ended (`bytes=N-`); it closes a connection itself when it
  reaches the next piece. dm asks closed ranges (`bytes=a-b`).
- **Splitting**: halves of what is left (offsets near 1/2, 1/4, 3/4, 1/8 ...).
- **Connections grow with slowness**: from a server holding each connection
  to 2 MiB/s it opened 15 at once and 14 more a second later, 29 in all, 24
  at the same time, and finished in 1.2 s. dm kept to its 8 and took 2.0 s
  (32 MiB / 8 / 2 MiB/s). From a fast server it used 6.
- **A 503 on a second connection**: it added no more connections and finished
  on the first; dm moved the refused ranges to its other connections.
- **A page's only video**: it proposes "Lab clip (1).mp4", yt-dlp's numbering.
- **Sandbox**: in Docker, where `bwrap` cannot make namespaces, it ran the
  add-on's yt-dlp outside one, without a word.
