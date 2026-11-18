# Options › Downloads

How dm fetches, and what happens around a download. See
[Options](options.md) for how the window works.

## Connections and files at once

- **Connections per file:** "How many parts of a file dm fetches at once.
  More is faster on most sites, but some limit it."
- **Presets:** **Slow (2)**, **Normal (8)**, **Fast (16)** and **Maximum
  (32)** each set the number above; the one that matches it is marked.
- **Files at once:** "How many files download at the same time."

A site that allows fewer connections can be given its own number: see
**Sites with their own connection limit:** in
[Options › Advanced](options-advanced.md).

## Videos

- **Video quality:** the quality a video is downloaded at unless you pick
  another: **Best available**, **Up to 1080p**, **Up to 720p**, **Up to
  480p** or **Audio only**. See [Videos and their qualities](videos.md).
- **Embed subtitles in videos from player pages:** "A video's subtitles are
  saved inside its file, to turn on in your player."
- **Save streams that arrive as .ts as MP4:** "Streamed videos are saved as
  MP4 files, which more players open."
- **Stop recording after (minutes, 0 for no limit):** a live stream is
  recorded for this long, then dm ends the recording and keeps what it has.
  0 records until the stream ends or you stop it.

## Queues

- **Start the Main queue when dm opens:** "Downloads waiting in the Main
  queue start without a click."
- **Queues with their own limit:** a queue that runs fewer (or more) files at
  once than **Files at once**. **Add a queue limit** adds a line with
  **Queue** and **Files at once**; **Delete queue limit** takes it away. A
  queue can have one limit, so each line offers only the queues without
  one.
- **Queues that start at a set time:** **Add a start time** adds a line with
  **Queue** and **Start at** (hour and minute, 02:00 at first); that queue
  starts every day at that time. **Delete start time** takes it away. For
  days, dates and a time to stop, use the [Scheduler](scheduler.md).
- **Recheck every (hours, 0 for manual):** how often the Recheck queue asks
  its servers whether its files have changed. 0 asks only when you start
  that queue.

## Watching a folder and running a command

- "Watch a folder: links (.url, .webloc or a text file of links) and
  .torrent files saved there are added, then moved to its "done" folder.
  Leave it empty to turn this off." **Browse…** picks the folder. dm looks
  in it every few seconds.
- "When a download finishes, run this command (a virus checker or a script).
  The file's path is added at the end:" type it in **Command**. Empty runs
  nothing.
