# Using EDM

The parts of EDM the Help menu points to. For installing EDM and its browser
extension, see the [README](../README.md).

## How to…

Step by step, for the things most people do first. **Help › How to…** opens
each one, without a connection.

- [Your first download](guide/first-download.md)
- [Downloads from your browser](guide/browser-downloads.md)
- [Downloads overnight](guide/overnight-downloads.md)
- [Download a video](guide/download-a-video.md)
- [Stop now, finish later](guide/resuming.md)
- [Keep some speed for everything else](guide/speed-limits.md)
- [EDM on a USB stick](guide/usb-stick.md) (Windows)
- [EDM on a home server](guide/home-server.md) (Docker)

## Every page of the help

- **When something goes wrong:** [What EDM's messages mean](guide/failures.md),
  each failure and note a download can show, with what to do next.
- **Topics:** [Videos and their qualities](guide/videos.md),
  [Sign-ins](guide/sign-ins.md), [Proxies](guide/proxies.md),
  [Speed limiter](guide/speed-limiter.md).
- **Options:** [how it works](guide/options.md) and each page:
  [General](guide/options-general.md), [Browser](guide/options-browser.md),
  [Folders](guide/options-folders.md),
  [Downloads](guide/options-downloads.md),
  [Advanced](guide/options-advanced.md),
  [Sign-ins](guide/options-sign-ins.md).
- **Adding downloads:** [Add URL](guide/add-url.md),
  [Download details](guide/download-details.md),
  [Playlist](guide/playlist.md), [Download links](guide/download-links.md),
  [Links on the clipboard](guide/clipboard-links.md),
  [Add numbered series](guide/numbered-series.md),
  [Grabber](guide/grabber.md), [Add EDM to your browser](guide/welcome.md).
- **While and after a download:** [Download progress](guide/download-window.md),
  [Download complete](guide/download-complete.md),
  [File properties](guide/properties.md),
  [Update download link](guide/update-link.md),
  [Move to queue](guide/move-to-queue.md),
  [Move/Rename](guide/move-rename.md), [Scheduler](guide/scheduler.md),
  [Questions EDM asks](guide/questions.md), [About EDM](guide/about.md),
  [Statistics](guide/statistics.md).

## Queues and the Scheduler

A queue is a list of downloads that wait their turn and start together.
Every download is in one; **Main queue** is the one they go to unless you
choose another.

- **Put a download in a queue:** in **Download details**, click **Add for
  later** and pick the queue beside it. In the list, right-click a download
  and choose **Move to queue**.
- **Make a queue:** in **Move to queue**, type its name under **Or a new
  queue**.
- **Start or stop a queue:** **Start queue** and **Stop queue** on the
  toolbar act on the queue the list shows; the small arrow beside each lists
  every queue, and the one you click is started or stopped.
- **How many at once:** **Options › Downloads** sets how many downloads run
  at the same time, and lists the queues with a number of their own.
- **Start and stop at set hours:** open **Downloads › Scheduler**. Pick a
  queue on the left. The Main queue's hours are the ones every queue
  follows unless you give it **Hours of its own for this queue**. Tick the
  days, or pick one date, and the time to start and to stop.
  **Start now** and **Stop** act on the queue you picked; **OK** keeps the
  hours of every queue.
- **When the downloads are done:** the Scheduler can close EDM, put the
  computer to sleep, hibernate it or shut it down once the scheduled
  downloads have finished, and try a failed download again a few times
  first. Every setting is in [Scheduler](guide/scheduler.md).

## Grabber

The Grabber reads a website and lists the files it links to, so you can
pick which to download. Open it from the toolbar's **Grabber** button.

1. **Start page:** the page to begin from.
2. **Template:** fills in the file types for pictures, documents, videos and
   music, archives and programs, or every file. You can change the list.
3. **Page depth:** 0 reads the start page only; 1 also reads the pages it
   links to on the same site, and so on. **Also follow links to other sites**
   reads pages elsewhere too.
4. **Files at least / at most (KB):** leave files out by size. A file whose
   size the server does not say is kept.
5. **Only paths containing / Skip paths containing:** words a file's address
   must have, or must not have.
6. Click **Find files**. Tick the files you want and click **Add for later**.

If **Options › Sign-ins** keeps a login for the site, the Grabber uses it and
says so.

**Save this search as** gives the search a name; **Run every (hours)** runs it
again on its own and adds only the files it has not found before. Saved
searches are listed under **Grabber projects** in the side panel: click one
to open the Grabber with it filled in. Every setting is in
[Grabber](guide/grabber.md).

## Download links from a page

Right-click a page and choose **Download all links with EDM**, or select part
of it and choose **Download selected links with EDM**. EDM lists the links
with their name, type, size, where each will be saved and the link's words
on the page. Tick the ones you want, choose where they go, and click **Start
now**, or **Add for later** to a queue you pick. **Use link texts as
descriptions** keeps each link's words with its download.

## Videos and their captions

Move the mouse over a playing video and click **Download video**. The list
shows each quality with its size; on YouTube a height offered at two frame
rates is two lines ("720p HD 60 fps" and "720p HD"), and each caption track
is a line of its own ("TTML file, EN subtitles"; "(ASR)" marks captions
YouTube made by speech recognition). **Download all** hands every line
over.

## Torrents

Click a magnet link or open a `.torrent` file and EDM downloads it.
**Options › Browser** shows which program opens magnet links and `.torrent`
files, with a button to make it EDM. The first torrent makes Windows ask
whether EDM may use the network: allow it and other peers can reach EDM, so
torrents go faster.

## Import and Export

**Tasks › Export…** writes every download in the list to a `.dmlist` file;
**Tasks › Import…** adds the downloads in such a file, or in a plain list of
links, one to a line. When some of them are already in the list, EDM asks
first ([Some are already in the list](guide/questions.md#some-are-already-in-the-list)).

Where the computer has no file panel to choose the file with (a Linux
desktop without one), a small window, "Export the list" or "Import a list",
says "No file picker here: type the path." Type the file's full path under
**File’s path** (`~` is your home folder) and click **OK**; Export offers
`downloads.dmlist` in the download folder. A path that is not a full one is
answered with "Type the file's full path, such as …", and a file that
cannot be read or written is said there too; the window stays open with
the path to be put right.

## Back up and restore

**File › Back up EDM…** writes one `.dmbackup` file of EDM's settings, queues,
schedules, site rules, saved searches and list of downloads, to carry EDM to
another computer or keep it safe. The sign-ins (Options › Sign-ins'
passwords, the proxy's and the dial-up connection's, a login in a
download's address, and the cookies and headers a browser handed over) go
in only when you type a **Passphrase** (twice: **Passphrase again**); they
are then sealed with it, and opened with nothing else. Left empty, the
sign-ins stay behind. Click **Back up** and choose where to save it. A
download marked "Keep no record" is not written, nor your typed addresses
and recent folders.

**File › Restore…** opens such a file, then says what restoring does. Its
settings, queues, schedules, site rules and saved searches replace yours,
as **OK** in Options does, and its downloads are added to the list; those
whose address the list holds already are left out, not added twice.
Downloads running go on. No file on the disk is replaced: a finished
download comes back as its record, and an unfinished one waits, paused,
to be fetched again from its start. Where its sign-ins are sealed, type
the passphrase; left empty, everything else is restored without them, and
a wrong one is said in the window, which stays open, and nothing is
restored. The programs EDM runs (the command after each download, the
program at the end of the scheduled hours, the proxy script) are never set
from a file: set them again in Options. A backup made by a newer EDM is
read as far as this one knows its fields; what it cannot read keeps its
default, and the status bar says so.
