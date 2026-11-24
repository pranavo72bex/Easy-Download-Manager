# Download progress

Each download can have a window of its own that follows it as it runs. Its
title is "Download progress — " and the file's name, with how far it has
got first ("42% Download progress — film.mp4"). When the download finishes
the title becomes "Download complete — " and the name.

It opens:

- when a download starts, while **Options › General › Show progress when a
  download starts** is on (one window for one download; adding many at
  once opens none);
- with **File › Show progress**, or **Details** on a download's row, for
  each chosen download that has not finished.

Windows for several downloads open a little apart, each below and to the
right of the last. A window you closed is not opened again by itself;
**Show progress** opens it.

## What it shows

- First, in larger type, how far it is, its speed and the time left
  ("54% · 300 KB/s · 3s left"); where it is not running, its status as the
  list says it. Under it the bar.
- The download's address. Under a failure dm adds one plain next step where
  it knows one (see [What dm's messages mean](failures.md)).
- **File size:** "unknown" until the server says.
- **Downloaded:** how much has come.
- **Networks:** where a second network is chosen (Options › Advanced ›
  Network) and carries part of the download, how fast each network brings it
  and its share of what is done: **This computer's connection** and the second
  one by its name. "the second network brings nothing now" means it adds no
  speed: it may share one internet line with the first (see
  [A second network for faster downloads](second-network.md)).
- **Can resume:** whether a stop can go on from where it stopped, as the
  server said: **Yes**; "No — a stop starts it again from the beginning";
  "No — the server refused it"; "Not known: it failed before the server
  answered"; "Unknown until the server answers". A torrent says "Yes, from
  the pieces it already has", or "No — nothing of it was fetched" when it
  failed before it had any. Not shown once the download has finished or
  was cancelled.
- The bar: one bar, or a piece for each connection where the file's size is
  known and it is fetched in several pieces.
- "Speed over the last 5 minutes": a bar for each five seconds of its
  speed, the latest at the right, while it has not finished, failed or been
  cancelled (see [Statistics](statistics.md)).

## Choices

Each is a line that says how it is set; click it to open it, and again to
close it. The first two show only while the download has not finished,
failed or been cancelled.

- **Speed limit** ("No limit", or the speed): opens to **Limit this download
  to** … **KB/s:** tick it to hold this one download
  to the speed typed (500 at first). It works beside the limit every
  download shares (see [Speed limiter](speed-limiter.md)). While it is
  ticked, a new number takes effect as you type it.
- **When done** (the choice): opens to **When this download is done**, what
  dm does when this download finishes:
  **Do nothing**, **Open**, **Open folder**, **Exit dm**, **Sleep**,
  **Hibernate** or **Turn off computer**. A choice this computer cannot do
  is not offered. Exit, sleep, hibernate and turning off go through the
  same countdown as **Downloads › When done** ("Downloads are done" in
  [Questions dm asks](questions.md)). dm keeps this choice only while it
  runs.
- **Connections:** click to open or close the list of the download's
  pieces: "… of … pieces done", then each piece with its own bar and
  speed. Before anything has come it says "Nothing fetched yet."; once
  every piece is in, "Every piece is in; putting them together." Not shown
  for a torrent.
- A torrent shows its swarm instead: "Peers: … connected, … known" and
  "Sent to peers: … at …".

## Buttons

Which buttons show depends on where the download stands:

- **Stop download:** while it runs or waits in line.
- A next-step button beside **Retry**, for some failures: **Choose another
  folder…** (the disk is full), **Update download link** (the link no
  longer works), **Open the page** (the site redirects in a loop),
  **Move/Rename** (something of that name is already there) or **Options**
  (the dial-up or VPN connection did not come up).
- **Retry:** after a failure.
- **Resume:** after a stop. It reads **Start now** for a download the
  Scheduler stopped; starting it then asks **Start outside the schedule?**.
- **Cancel download:** asks **Cancel this download?** first (see
  [Questions dm asks](questions.md)).
- **Open** and **Open folder:** once it has finished, when **Options ›
  General › Show a message when a download finishes** is on. Either one
  closes the window.

## Good to know

- **Escape** closes the window; the download goes on.
- When a download finishes and **Options › General › Show a message when a
  download finishes** is off, its window closes. With that option on, an
  open window stays, as Download complete; with no window open,
  [Download complete](download-complete.md) opens instead.
