# Reference behaviour: IDM and FDM

dm's engine is compared with Internet Download Manager (IDM) and Free
Download Manager (FDM). Neither is installed on the development machine (IDM
runs only on Windows), so what they do is taken from their public pages,
and each claim says where it comes from. Nothing of either program's code is
used; dm's behaviour is written and tested independently.

Labels: **Verified** (the reference's own page states it), **Reported**
(third-party guides only), **Unknown** (not documented anywhere found).

## Dynamic segmentation (IDM, Verified)

Source: internetdownloadmanager.com/support/segmentation.html.

- The file is divided "dynamically", during the download, not once at the start.
- When a connection becomes free, IDM "finds the largest segment to download
  and divide[s] it in half"; the free connection takes the second half.
- A segment is not split once it is "too small for this connection type".
  No number is given (Unknown).
- "Full reuse of connections that have finished downloading of assigned
  segments": no new connect or login stage.
- Positions are saved "several times per minute", so a pause or a power loss
  resumes and assembles correctly.

## Connections (IDM, Verified)

Source: internetdownloadmanager.com/support/options.html.

- One "Max. connections number" per download. IDM's own advice is to keep it
  at four or below for most servers.
- An Exceptions table gives some servers fewer connections, "for the servers
  that limit the number of connections from the same client", FTP servers
  that allow one or two among them.
- The default number is 8 (Reported; not stated by IDM).

## Other engine features (IDM, Verified)

Source: internetdownloadmanager.com/features2.html.

- Resume from where a download stopped; error recovery after lost
  connections, network problems, shutdown and power loss.
- Queues and a scheduler; shut down or disconnect when done.
- Periodic synchronisation: download a file again when it has changed.
- A download quota (megabytes per hour).
- Categories that decide where files go.
- Basic, Negotiate, NTLM and Kerberos sign-in; proxies.
- A virus scanner run on completion.
- A site grabber, and "Download all" links of a page.

## Naming a page's video (IDM and FDM, Reported)

Both name a video they find on a page by the page or video title alone. A
number is added only to tell apart two files that would otherwise clash
(Reported, from how both behave for duplicate names; not stated on a page).

## FDM

FDM's own documentation does not say how it segments or limits connections
per server (Unknown). Treated as agreeing with IDM where both are described
by third parties.

## FDM 6 and video sites (Verified, 2026-10-29)

Seen by running FDM 6.35.2.7075 (Linux `.deb`) in Docker, reading the names
its libraries export, and reading the add-on's own source (MIT,
github.com/meowcateatrat/elephant, 1.3.14). FDM's binaries were not
disassembled.

- FDM itself downloads no video site. Given a YouTube link it says "Video
  downloads require an add-on" and points to free-addons.org.
- That add-on, Elephant, is yt-dlp. It bundles yt-dlp's Python source and
  yt-dlp's YouTube challenge solver (`yt_dlp_ejs`); FDM runs it with the
  system's Python (3.10 or later) and Deno (2.3 or later), which FDM installs
  when first asked (`PythonDependencyManager`, `DenoDependencyManager`),
  inside a bubblewrap sandbox on Linux (`QtProcessSandboxManager`: "using bwrap").
- yt-dlp only reads the page: `-J --flat-playlist --no-warnings`, plus
  `--no-playlist` for one video. FDM's own engine fetches the formats it
  lists (`MsDownload`) and joins picture and sound with ffmpeg
  (`QtFfmpegAvMerger`, ffmpeg a package dependency).
- Sign-in: the browser's cookies for the page when FDM has them, written to a
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
