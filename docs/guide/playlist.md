# Playlist

Playlist lists the videos of a playlist so you can tick which to download.
It opens when you give [Add URL](add-url.md) a YouTube playlist address,
and when [Download details](download-details.md) finds that the page it
was given holds several videos. A feed's address (RSS or Atom: a podcast,
a channel's uploads, an address ending in `.rss` or named `feed`) opens it
too, listing the feed's items: each episode's file, or each item's link.

While dm reads the list it says "Reading the playlist…". If it cannot, it
says "The playlist could not be read:" and why.

## Fields and buttons

- The playlist's title, and how many videos it lists ("12 videos"). A long
  list read only in part adds "Only the first … videos are listed."
- **Select all:** ticks every video, or unticks them all when all are
  ticked. Every video is ticked when the list opens.
- **Video quality:** one quality for every video: **Best available**, **Up
  to 1080p**, **Up to 720p**, **Up to 480p** or **Audio only**. It starts
  at **Options › Downloads › Video quality**.
- The list: each video's number, title and length ("3. Intro · 4:05"),
  with a tick box.
- **Add for later:** adds the ticked videos to the Main queue without
  starting them.
- **Cancel:** adds nothing.
- **Start now:** adds the ticked videos and starts them.

**Add for later** and **Start now** stay greyed until the list has been
read. With no video ticked, dm says "Pick at least one video." and the
window stays open. Once they are added the status bar says "Added …
downloads."

## Following a playlist or a feed

- **Check again every … hours, adding what is new to …:** ticked, the
  playlist, channel or feed is followed once **Add for later** or **Start
  now** adds its videos. Every so many hours (24 at first; 1 at least) dm
  reads it again and adds only the videos or files it did not list before,
  for later, to the queue chosen. A video you left unticked, or removed
  from the list, is not added again. The status bar says "Following …,
  checked again every … hours.", then after each reading "Added …
  downloads to …" or "… has nothing new."
- **Only add new items whose titles have the words:** shown for a torrent
  feed (one whose items are `.torrent` files or magnet links, which dm adds
  as torrents). While the feed is followed, only new items whose titles
  have every one of these words, in any case, are added; left empty, every
  new item is. An item left out is not counted as seen.

Each followed playlist or feed is listed under **Grabber projects** in the
side panel, by its title. Clicking it opens **Followed**, which says
"Checked again every … hours; what is new goes to …." with three buttons:

- **Check now:** reads it at once.
- **Stop following:** forgets it; nothing already added is removed.
- **Close:** leaves it as it is.

A torrent feed followed with words also says "Only items whose titles have
the words … are added."

If a reading fails, the status bar's problems say "the followed … could
not be read:" and why (see [What dm's messages mean](failures.md)); it is
tried again at its next time.

## Keys

- **Return** is **Start now**.
- **Escape**, Ctrl+W (⌘W on macOS) and the close button are **Cancel**.
