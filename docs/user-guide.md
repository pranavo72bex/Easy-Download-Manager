# Using dm

The parts of dm the Help menu points to. For installing dm and its browser
extension, see the [README](../README.md).

## Queues and the Scheduler

A queue is a list of downloads that wait their turn and start together.
Every download is in one; **Main queue** is the one they go to unless you
choose another.

- **Put a download in a queue:** in **Download details**, click **Add for
  later** and pick the queue beside it. In the list, right-click a download
  and choose **Move to queue**.
- **Make a queue:** in **Move to queue**, type its name under **Or a new
  queue**.
- **Start or stop a queue:** **Start Queue** and **Stop Queue** on the
  toolbar act on the queue the list shows; the small arrow beside each lists
  every queue, and the one you click is started or stopped.
- **How many at once:** **Options › Downloads** sets how many downloads run
  at the same time, and lists the queues with a number of their own.
- **Start and stop at set hours:** open **Downloads › Scheduler**. Pick a
  queue on the left. The Main queue's hours are the ones every queue
  follows unless you give it **Hours of its own for this queue**. Tick the
  days, or give one date (YYYY-MM-DD), and the time to start and to stop.
  **Start now** and **Stop** act on the queue you picked; **OK** keeps the
  hours of every queue.
- **When the downloads are done:** the Scheduler can close dm, put the
  computer to sleep, hibernate it or shut it down once the scheduled
  downloads have finished, and try a failed download again a few times
  first.

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
to open the Grabber with it filled in.

## Download links from a page

Right-click a page and choose **Download all links with dm**, or select part
of it and choose **Download selected links with dm**. dm lists the links
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

Click a magnet link or open a `.torrent` file and dm downloads it.
**Options › Browser** shows which program opens magnet links and `.torrent`
files, with a button to make it dm. The first torrent makes Windows ask
whether dm may use the network: allow it and other peers can reach dm, so
torrents go faster.
