# Options › Downloads

How dm fetches, and what happens around a download. See
[Options](options.md) for how the window works.

## Connections and files at once

- **Connections per file:** "How many parts of a file dm fetches at once.
  More is faster on most sites, but some limit it."
- **Presets:** **Slow (2)**, **Normal (8)**, **Fast (16)** and **Maximum
  (32)** each set the number above; the one that matches it is marked. A new dm
  uses 16.
- **Files at once:** "How many files download at the same time."

A site that allows fewer connections can be given its own number: see
**Sites with their own connection limit:** in
[Options › Advanced](options-advanced.md).

## Videos

- **Video quality:** the quality a video is downloaded at unless you pick
  another: **Best available**, **Up to 1080p**, **Up to 720p**, **Up to
  480p** or **Audio only**. See [Videos and their qualities](videos.md).
- **Sound language:** **The video's own** (at first), or a language by its
  own name: English, Español, Français, Deutsch and others. "Where a video
  has its sound in several languages, this one is taken; where none speaks
  it, the video's own." A dubbed video (a YouTube video with dubs, or a
  stream offering several sound tracks) is saved with the one chosen.
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

## Forgetting finished downloads

- **Forget finished downloads after (days, 0 for never):** "A finished
  download leaves the list once it is that many days old. Its file stays
  where it was saved." dm looks when it starts, once a day after that, and
  as soon as you change the number. 0, the setting at first, keeps every
  finished download. A download that failed, one in the Recheck queue and
  one whose **Delete file after** has still to come are kept. To forget a
  single download as soon as it is whole, tick **Keep no record** in
  [Download details](download-details.md).

## Watching a folder and running a command

- "Watch a folder: links (.url, .webloc or a text file of links) and
  .torrent files saved there are added, then moved to its "done" folder.
  Leave it empty to turn this off." **Browse…** picks the folder. dm looks
  in it every few seconds.
- "When a download finishes, run this command (a virus checker or a script).
  The file's path is added at the end:" type it in **Command**. Empty runs
  nothing.

## A metered connection and the battery

- **On a metered connection:** **Go on** (at first), **Ask** or **Wait**.
  "Where the system marks the connection as metered: a phone's hotspot, a
  capped plan. Downloads that wait go on by themselves once it is not."
  With **Wait**, every download waits while the connection is metered, its
  row saying "Waiting: the connection is metered", and goes on by itself
  when it is not. With **Ask**, dm asks "The connection is metered" once
  each time the connection becomes metered, the downloads waiting until you
  choose **Download anyway** or **Wait**; with no window open, **Ask**
  waits. Windows says a connection is metered from Settings › Network
  (and for a phone's connection by itself); on Linux, NetworkManager says
  it. macOS does not tell dm, so there the downloads go on.
- **On battery: wait below (%, 0 for never):** "Downloads wait while the
  computer runs on its battery below this level, and go on once it charges
  or is above it again." Their rows say "Waiting: the battery is low". 0,
  the setting at first, never waits.

On a phone, four more lines follow, each off at first:

- **Wi-Fi only:** "On mobile data, downloads wait, and go on by themselves
  on Wi-Fi." Their rows say "Waiting: the phone is not on Wi-Fi".
- **Not while roaming:** "Downloads wait while the phone is roaming." Their
  rows say "Waiting: the phone is roaming".
- **Pause when Data Saver is on:** "Downloads wait while Android's Data
  Saver is on, and go on once it is off." Their rows say "Waiting: Data
  Saver is on".
- **Keep downloads and torrents going when dm is closed:** "Swiped away from the recent apps, dm goes on under its notification until every download is done and every torrent has finished sharing." Opened again, dm shows them where they got to.

A connection or a battery the system says nothing of never holds a
download. A download you start with **Start download** while they wait
goes on; one you paused stays paused when the wait ends.
